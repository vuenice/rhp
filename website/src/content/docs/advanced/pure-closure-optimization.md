---
title: "Pure Closure Optimization"
description: "Closure purity categorization, $this elision, zero-allocation memory layout, and raw Cranelift machine-code compilation."
section: "Advanced"
order: 6
---

## Overview: The Legacy PHP Closure Overhead

In standard PHP (Zend Engine), anonymous functions and arrow functions capture their lexical environment by reference or value copy:

```php
// Standard PHP closure with mutable outer reference
$counter = 0;
$increment = function() use (&$counter) {
    $counter++;
};
```

This traditional model carries significant runtime liabilities:
1. **Zend Executor (EG) State Binding:** Closures bind to thread-local global executor state (`EG`), hindering true multi-threaded parallel execution.
2. **Cyclic Reference Leaks:** Capturing `$this` or variables by reference frequently produces uncollectable reference cycles requiring slow cyclic garbage collector (GC) sweeps.
3. **Mandatory Heap Allocation:** Every closure instantiation dynamically allocates an internal `zend_closure` structure and an environment hash table on the heap, triggering heap fragmentation and cache misses.

In contrast, RHP introduces **Pure Closure Optimization**: identifying self-contained closures at compile time, eliminating unnecessary captures, and lowering pure closures into **zero-allocation** machine code executed via **Cranelift JIT function pointers**.

---

## Empirical Codebase Findings: 98% Natural Purity

To validate this optimization model against real-world enterprise applications, RHP was evaluated against a large Laravel 12 application (`client-app`) comprising **3,094 PHP source files** and **9,462 closures**:

```text
Total Closures Analyzed: 9,462
├── Pure Closures:               9,274 (98.01%)
│   ├── Pure Non-Capturing:      4,588 (48.49%)  -> 0 upvalues, no $this, 0 heap allocations
│   ├── Pure $this-Only:         3,528 (37.29%)  -> Single pointer, fits in 16B inline struct
│   ├── Pure Value-Only:           948 (10.02%)  -> 94.3% capture <= 2 variables (inline 16-32B)
│   └── Pure Value + $this:        210  (2.22%)  -> Flat immutable environment
└── Impure Closures:               188  (1.99%)  -> use (&$var) mutable references
```

> [!NOTE]
> In production code (`app/`, `database/`, `routes/` — excluding `tests/`), only **58 closures out of 2,855 (2.03%)** captured variables by reference. Routes, Filament form/table schemas, Models, and Service Providers were **100% pure**.

---

## Closure Classification Hierarchy

The RHP compiler categorizes every closure and arrow function into one of six optimization tiers defined in [`ClosureCategory`](/docs/advanced/compiler-internals):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClosureCategory {
    /// Pure non-capturing closure (0 captured variables, no $this).
    /// Lowers to a raw Cranelift function pointer (fn_ptr, null) with ZERO heap allocations.
    PureNonCapturing,

    /// Pure closure capturing only `$this` (single 8-byte instance pointer).
    /// Fits in a 16-byte inline struct with ZERO heap allocations.
    PureThisOnly,

    /// Pure closure capturing 1 or 2 outer variables by immutable value-copy.
    /// Fits in a 16-to-32 byte inline struct with ZERO heap allocations.
    PureValueSmall(u8),

    /// Pure closure capturing 3 or more outer variables by immutable value-copy.
    PureValueLarge(u16),

    /// Pure closure capturing $this and 1 or more outer variables by immutable value-copy.
    PureValueAndThis(u16),

    /// Impure closure capturing outer variables by reference (`use (&$var)`).
    ImpureReference(u16),
}
```

### Property Matrix

| Category | Upvalues | Captures `$this` | Inline Size | Heap Allocations | Cranelift Function Pointer |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **`PureNonCapturing`** | $0$ | No | 16 B | **0 bytes** | `(extern "C" fn, null)` |
| **`PureThisOnly`** | $0$ | Yes | 16 B | **0 bytes** | `(extern "C" fn, this_ptr)` |
| **`PureValueSmall(1)`** | $1$ | No | 24 B | **0 bytes** | `(extern "C" fn, env_ptr)` |
| **`PureValueSmall(2)`** | $2$ | No | 32 B | **0 bytes** | `(extern "C" fn, env_ptr)` |
| **`PureValueLarge(N)`** | $N \ge 3$ | No | Flat Tuple | 1 slice | `(extern "C" fn, env_ptr)` |
| **`PureValueAndThis(N)`**| $N \ge 1$ | Yes | Flat Tuple | 1 slice | `(extern "C" fn, env_ptr)` |
| **`ImpureReference(N)`** | $N \ge 1$ | Optional | Heap Handle | RefCell Box | Escaping interpreter cell |

---

## AST `$this` Elision Optimization

In standard PHP, any non-static closure defined inside a class method implicitly captures `$this`. This retains unnecessary instance references and creates cyclic reference risks when assigned to properties.

During IR lowering (`src/ir/lower.rs`), RHP runs an AST visitor pass:

```rust
pub fn closure_body_uses_this(body: &[Stmt<'_>]) -> bool;
pub fn expr_uses_this(expr: &Expr<'_>) -> bool;
```

If the closure body does **not** syntactically reference `$this`:
1. `$this` is **elided** from the closure’s `upvalues` table.
2. The closure upgrades to **`PureNonCapturing`** or **`PureValueSmall`**.
3. It incurs **zero reference count retention** of the enclosing class instance.

```php
class QueryBuilder {
    private string $table = 'users';

    public function getActiveFilter(): callable {
        // Even though declared inside an instance method, this closure
        // does not use $this. RHP automatically elides $this!
        return function(int $status): bool {
            return $status === 1;
        };
        // Result: PureNonCapturing (0 heap allocations, bare Cranelift function pointer)
    }
}
```

---

## Zero-Allocation Memory Layout

In RHP, closures that capture 0, 1, or 2 variables fit directly inside an inline 16-to-32 byte struct layout on the stack or register file without triggering heap allocations:

```rust
#[repr(C)]
pub struct InlineClosure {
    /// Raw Cranelift machine-code function pointer.
    pub fn_ptr: unsafe extern "C" fn(*mut NativeSlot) -> u32,
    /// Inline capture storage (holds up to two 64-bit values or object pointers).
    pub inline_env: [u64; 2],
    /// Optimization category metadata.
    pub category: ClosureCategory,
}
```

Because **94.3%** of all value-capturing closures in real-world code capture only 1 or 2 variables, over **95% of all closures in an application run with zero heap allocations**.

---

## Cranelift JIT Native Machine Code Generation

When JIT compilation is enabled (`RunMode::Build`), `PureNonCapturing` closures compile into raw machine-code function pointers:

$$\text{Closure} \equiv \left(\text{fn\_ptr: } \text{unsafe extern "C" fn}(*\text{mut NativeSlot}) \to \text{u32}, \; \text{env: } \text{null}\right)$$

### JIT Execution Flow

```text
Hot Pure Closure Invocation ($fn($x))
               │
               ▼
   [JitEngine::record_function_call]
               │
               ▼
     (Invocation Count >= Threshold)
               │
       ┌───────┴───────┐
       ▼               ▼
 [Pure Non-Capturing]  [Capturing Closure]
       │               │
       │               └─► Evaluates via VM dispatch or inline frame
       ▼
 [Cranelift Codegen]
 - Emits native x86_64 / aarch64 machine instructions
 - Binds params directly to CPU registers (RDI, RSI, RDX)
 - Zero environment indirection (env = null)
 - Zero garbage collector tracking
       │
       ▼
 Direct Call: call *%rax
```

---

## Static Cycle Prevention (`ClosureReferenceCaptureForbidden`)

RHP enforces compile-time cycle elimination. Capturing outer variables by mutable reference (`use (&$var)`) is strictly rejected by the type checker during AST analysis:

```php
$total = 0;
// Compile Error in RHP: ClosureReferenceCaptureForbidden
$accumulate = function(int $val) use (&$total) {
    $total += $val;
};
```

### The Idiomatic RHP Functional Alternative

Instead of mutating outer scope variables through reference captures, developers use functional expressions:

```php
// RHP Idiomatic Functional Pipeline: 100% Pure & Zero Allocations
$numbers = Arr::list([10, 20, 30, 40]);
$total = $numbers->reduce(fn(int $acc, int $n): int => $acc + $n, 0);
```

By eliminating reference variables and mutable closure upvalues:
1. Cyclic reference graphs between closures and local stack frames are mathematically impossible.
2. Closures can safely cross Tokio worker thread boundaries (`Future::wait`, `Future::any`) without requiring thread-local GC synchronization or global mutex locking.

---

## Summary Comparison

| Dimension | Legacy PHP (Zend Engine) | RHP Engine |
| :--- | :--- | :--- |
| **Purity Categorization** | None (uniform generic closure) | 6 distinct compile-time tiers (`ClosureCategory`) |
| **`$this` Handling** | Always captures `$this` unless `static` | Automatic AST `$this` elision |
| **Memory Allocation** | Heap-allocated `zend_closure` + hash table | Inline 16–32B buffer (**0 heap allocations**) |
| **JIT Optimization** | Bytecode handler jumps | Raw Cranelift native function pointers |
| **Thread Safety** | Bound to single-thread `EG` executor | Lock-free worker thread dispatch |
| **Cyclic Reference Risk** | High (requires tracing cyclic GC) | **Zero (compile-time cycle elimination)** |
