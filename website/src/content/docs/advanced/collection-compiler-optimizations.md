---
title: "Collection Compiler Optimizations"
description: "Under-the-hood compiler optimizations in RHP: FBIP (Functional But In-Place) buffer reuse for zero-allocation sorts, and single-pass Stream Fusion for pure collection pipelines."
section: "Advanced"
order: 8
---

## Overview

Modern applications frequently write collection operations in an expressive, functional style:

```php
// Functional sorting with reassignment
$nums = $nums->sort();

// Higher-order transformation pipeline
$result = $nums
    ->map(fn($x) => $x * 2)
    ->filter(fn($x) => $x > 10)
    ->take(5);
```

While ergonomically elegant, this pattern historically causes extreme performance penalties in traditional dynamic runtimes like PHP (Zend Engine):
1. **Redundant Buffer Allocation:** Every step (`map`, `filter`, `take`) allocates a completely new heap array, copying thousands or millions of elements back and forth through main memory.
2. **Cache Pollution:** Elements bounce between CPU caches and RAM across intermediate arrays rather than staying hot in CPU registers.
3. **No Short-Circuiting ($O(N)$ vs $O(K)$):** Calling `->take(5)` on a 1,000,000 element array still forces `map()` and `filter()` to process all 1,000,000 items before slicing the first 5.

To solve this, RHP introduces two complementary under-the-hood compiler and bytecode engine optimizations:
- **FBIP (Functional But In-Place) In-Place Buffer Reuse:** In-place buffer mutation when `refcount == 1`.
- **Stream Fusion (Loop Deforestation):** Fusing consecutive pure method calls into a single-pass loop with algorithmic short-circuiting.

---

## 1. FBIP In-Place Buffer Reuse (`refcount == 1`)

### The Concept

In functional programming and compiler research (e.g., Koka's Perceus RC, Lean 4, and Swift's Copy-on-Write), this technique is known as **FBIP (Functional But In-Place)**.

When developer code writes:

```php
$nums = $nums->sort();
```

Semantically, `$nums->sort()` returns a sorted collection without mutating references held elsewhere. However, if `$nums` is exclusively owned by that variable with **no other aliases** in memory (`refcount == 1`), allocating a new array buffer and copying all elements is completely wasteful.

The RHP compiler and bytecode VM detect this condition and **sort the buffer in-place**:

```text
Traditional PHP Zend Engine:
  $nums (Vec A) ─── clone & alloc ───► New Vec B (sorted) ───► $nums = Vec B (Vec A dropped)
  [Memory allocations: 1 full buffer copy]

RHP FBIP Optimization:
  $nums (Vec A) [refcount == 1] ─── sort in-place ───► Same Vec A (zero allocation!)
  [Memory allocations: 0 bytes, maximum hardware speed]
```

### Compiler & VM Implementation

In RHP, lists and subtyped arrays are backed by Rust's `Rc<RefCell<Vec<PhpValue>>>`.

1. **Reassignment Detection:** During IR lowering of `$nums = $nums->sort()`, the assignment target register `dst` matches the receiver register `object` (`dst == object`).
2. **Temporary Register Relinquishment:** At `OpCode::MethodCall`, if `dst == object`, the VM temporarily moves the register value out (`std::mem::replace`), dropping the register file's reference.
3. **Uniqueness Check:** The engine checks `receiver.ref_count() == 1`. If no other variable, closure capture, or data structure references the buffer:
   - The backing buffer is sorted directly via `rc.borrow_mut().sort_by(...)`.
   - `receiver` is returned without any memory allocation.
   - The metric `fbip_inplace_sorts` is incremented.

### Safety Guarantees & Copy-On-Write (COW)

If another variable aliases the buffer:

```php
$nums = Arr::list([5, 2, 9, 1, 3]);
$alias = $nums; // creates a second alias: refcount is now 2!
$sorted = $nums->sort();
```

Because `$alias` holds a reference, `refcount > 1`. The VM automatically skips in-place mutation and executes a safe Copy-On-Write (COW) detachment, preserving `$alias` in its original unsorted state `[5, 2, 9, 1, 3]`.

Methods supported by FBIP in RHP:
- `sort()` / `asort()` (natural comparison)
- `natsort()` / `sortNatural()` (natural human order)
- `natcasesort()` / `sortNaturalCase()` (case-insensitive natural order)

---

## 2. Stream Fusion (Loop Deforestation)

### The Problem

Consider this common fluent pipeline:

```php
$res = $items
    ->map(fn($x) => $x * 2)
    ->filter(fn($x) => $x > 10)
    ->take(5);
```

Without compiler optimization:
- Stage 1 (`map`): Allocates `Vec<PhpValue>` of size $N$, executes closure $N$ times.
- Stage 2 (`filter`): Allocates second `Vec<PhpValue>` of size $M$ ($M \le N$), executes closure $N$ times.
- Stage 3 (`take`): Allocates third `Vec<PhpValue>` of size $\le 5$, copies elements.

Total intermediate allocations: **3 buffers**. Total iterations: **$2N + 5$**.

### Compile-Time Purity Contract

Stream fusion reorders and interleaves closure invocations. If a closure has observable side effects (e.g. mutating external state by reference `use (&$var)`), fusing `map` and `filter` into a single loop would alter the order and count of side effects.

RHP makes stream fusion completely sound and deterministic by enforcing **Closure Purity**:
- The RHP compiler classifies every closure into a [`ClosureCategory`](/docs/advanced/pure-closure-optimization).
- Arrow functions (`fn(...) => ...`) and non-reference closures (`function(...) use ($val)`) are classified as `PureNonCapturing`, `PureThisOnly`, or `PureValueSmall`.
- The VM only fuses pipeline stages whose closures are statically verified as **pure**. Impure closures (capturing `&$var` by reference) cleanly fall back to sequential execution.

### Bytecode Lookahead Architecture

When the bytecode virtual machine dispatches an `OpCode::MethodCall` for a fuseable stage (`map`, `filter`, `reject`, `take`, `skip`) on a `List` or `Array` receiver:

```text
Bytecode Stream:
  0014: METHOD_CALL     r9, r0->"map", args=1     <── VM is here
  0015: MAKE_CLOSURE    r11, fn#1 (Pure)
  0016: PUSH_ARG        r11
  0017: METHOD_CALL     r8, r9->"filter", args=1  <── Lookahead detects chained object=r9
  0018: LOAD_INT        r12, 5
  0019: PUSH_ARG        r12
  0020: METHOD_CALL     r7, r8->"take", args=1    <── Lookahead detects chained object=r8
  0021: RETURN          void
```

1. **Greedy Lookahead Scan:** The VM inspects forward instructions (`curr_ip..`). It simulates argument staging (`LoadInt`, `MakeClosure`, `PushArg`) while verifying closure purity.
2. **Chain Assembly:** It matches consecutive `MethodCall` opcodes where `object == prev_dst` and the method belongs to `{map, filter, reject, take, skip}`.
3. **Single-Pass Loop Execution:** If $\ge 2$ stages are identified, the VM:
   - Evaluates all items in a single pass over the source collection.
   - Feeds each element through the pipeline hot in CPU registers.
   - Emits elements directly into the final output vector (pre-sized to `take` capacity).
4. **Instruction Pointer Fast-Forward:** Advances `frame.ip` past all consumed lookahead instructions (`extra_ops_consumed = 6`), storing the result in the chain's final destination register (`r7`).

```text
Un-Fused Pipeline:
  Input (1,000,000 items) ──► [Alloc Vec 1] ──► [Alloc Vec 2] ──► [Alloc Vec 3] ──► Output (5 items)
  Time: O(N) | Allocations: 3 large buffers

Fused Pipeline:
  Input (1,000,000 items) ──► [ Fused Single-Pass Loop ] ──► Output (5 items)
                                  item -> map -> filter -> take(5) -> break!
  Time: O(K) | Allocations: 1 small buffer (capacity 5)
```

### Algorithmic Short-Circuiting ($O(N) \to O(K)$)

The most dramatic benefit occurs when `take(K)` is included in a pipeline:

```php
$big = Arr::list(range(1, 1_000_000));
$res = $big->map(fn($x) => $x * 10)->filter(fn($x) => $x >= 100)->take(3);
```

- **Without Fusion:** 1,000,000 multiplications, 1,000,000 comparisons, 2 intermediate arrays with millions of elements.
- **With Fusion:** Stops after item 12 (10 matches filter, take reaches 3, loop breaks!). **12 multiplications, 12 comparisons, zero intermediate buffers.**

---

## 3. Observability & Telemetry

Both optimizations are natively integrated into RHP's zero-overhead runtime metrics and can be inspected via the `Memory` facade:

```php
$stats = Memory::stats();

echo $stats['fbip_inplace_sorts']; // Number of zero-allocation in-place sorts executed
echo $stats['stream_fusions'];      // Number of multi-stage pipelines fused into a single loop
```

From Rust / internal VM APIs:

```rust
use rhp::vm::memory::{fbip_inplace_sorts, stream_fusions};

println!("FBIP in-place sorts: {}", fbip_inplace_sorts());
println!("Stream fusions: {}", stream_fusions());
```

---

## Summary Comparison

| Feature | Standard PHP (Zend Engine) | RHP Optimized |
|---|---|---|
| `$nums = $nums->sort()` Memory | Allocates new `zend_array` ($O(N)$ allocation) | **Zero allocation** in-place buffer reuse (`refcount == 1`) |
| Shared `$alias = $nums; $nums->sort()` | Mutates shared array in-place | **Safe COW detachment** preserving `$alias` |
| Chained `->map()->filter()->take()` | 3 heap allocations, $O(N)$ iterations | **Single-pass fused loop**, 0 intermediate allocations |
| `take(5)` on 1,000,000 elements | 1,000,000 map/filter calls ($O(N)$) | **Short-circuits immediately** after 5 matches ($O(K)$) |
| Impure Closure Safety | Undefined / unexpected behavior | **Safe fallback** to chained execution if closure captures `&$var` |
