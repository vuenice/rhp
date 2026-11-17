---
title: "Collection Value Semantics & Copy-on-Write"
description: "Why RHP collections use PHP-style value semantics with Copy-on-Write (COW), zero-cycle memory architecture, and inout parameter conventions."
section: "Advanced"
order: 9
---

## Overview

In dynamic languages, collection variables (`arrays`, `lists`, `dicts`, `maps`) are typically implemented in one of two diametrically opposed ways:

1. **Shared Reference Types (JavaScript, Python, Ruby):**
   Assigning `$b = $a` aliases the memory buffer. Modifying `$b[] = 2` silently mutates `$a`. Pushing an array into itself creates an infinite self-referential graph cycle (`$a[0] === $a`).
2. **Value Types with Copy-On-Write (PHP Zend Engine, Swift, Rust `Rc::make_mut`):**
   Assigning `$b = $a` creates an instantaneous $O(1)$ reference-counted handle. If either variable mutates the collection, the underlying buffer detaches (**Copy-On-Write**). The original variable remains completely untouched.

**RHP strictly enforces PHP-style Value Semantics with Copy-on-Write (COW) across all collection variants (`list`, `dict`, `set`, `hashMap`).**

```php
list $a = [1];
list $b = $a;

$b[] = 2; // Copy-on-Write detachment occurs here

print($a[0]); // 1 ($a is untouched!)
print(count($a)); // 1
print(count($b)); // 2
```

---

## Why Value Semantics?

### 1. Developer Familiarity & Eliminating Silent Mutation Bugs
RHP's target audience is PHP and Laravel developers. In PHP, arrays have always been pass-by-value with Copy-on-Write.

When a developer passes an array into a helper function or assigns it to a secondary variable, they expect the original collection to remain immutable unless explicitly passed by reference:

```php
function calculateDiscount(list $items): float {
    $items[] = 0; // In RHP, this NEVER mutates the caller's array!
    return count($items) * 0.1;
}

$cart = [100, 200];
calculateDiscount($cart);
print(count($cart)); // 2 (safe, predictable, zero side effects)
```

In runtimes with reference aliasing (like JavaScript), passing an array or assigning it requires defensive manual cloning (`[...items]`, `array.slice()`). Forgetting to clone causes insidious bugs where state changes in one part of an application corrupt unrelated data structures.

### 2. Zero Collection Cycles: Pure RAII Drop Semantics Without a Cycle Collector
Because collections possess pure value semantics, **it is mathematically impossible for a collection to contain a mutable reference to itself**.

When a developer writes:

```php
list $a = [1];
$a[] = $a; // Takes a value snapshot of $a at this instant
```

Under RHP's COW engine:
1. Evaluating the right-hand `$a` creates an $O(1)$ reference (`refcount == 2`).
2. Appending to the left-hand `$a` invokes `ensure_list_mut()`, which observes `refcount > 1` and detaches `$a` into a fresh buffer.
3. The old buffer `[1]` is appended into the new buffer `[1, [1]]`.

```text
Memory Topology:
$a ──► Vec [1, Ptr] ──► Vec [1] (Snapshot)
(Directed Acyclic Tree — Zero Cycles!)
```

Because collection topologies are guaranteed to be Directed Acyclic Graphs (DAGs), standard Rust reference counting (`Rc<RefCell<...>>`) deterministically drops buffers as soon as scope exits. **No tracing garbage collector, stop-the-world pauses, or cyclic root buffer sweeps are ever required.**

---

## Comparison: PHP vs. JavaScript vs. RHP

| Behavior | PHP (Zend Engine) | JavaScript (V8) | RHP |
| :--- | :--- | :--- | :--- |
| **Assignment Semantics** | Value (COW) | Shared Reference | **Value (COW)** |
| **`$b = $a; $b[] = 2;`** | `$a` remains unchanged | `$a` is mutated | **`$a` remains unchanged** |
| **`$a[] = $a;`** | Produces `[1, [1]]` snapshot | Creates infinite graph cycle | **Produces `[1, [1]]` snapshot** |
| **`$a == $b` (self-nested)** | Returns `true` (finite comparison) | N/A (infinite nesting) | **Returns `true` (finite comparison)** |
| **References in Arrays** | Allowed via `&$var` (causes memory cycles) | All objects are references | **Forbidden (Value-only containers)** |
| **Garbage Collection** | Bacon-Rajan cyclic GC root buffers | Generational Mark-Sweep GC | **Zero-GC (100% Deterministic RAII Drops)** |

---

## Equality Comparison Safety

In runtimes that allow cyclic data structures, comparing two distinct cyclic instances (`$a == $b`) can cause the comparison engine to walk infinitely through recursive children, resulting in a thread stack overflow (`SIGSEGV` / `SIGABRT` crash).

Under RHP's value semantics:
1. **Acyclic Structure:** `$a[] = $a` and `$b[] = $b` evaluate to finite nested trees: `[1, [1]]`.
2. **Instant Pointer Fast-Path:** `$a == $a` evaluates `Rc::ptr_eq(lhs, rhs)` in $O(1)$ time without visiting child elements.
3. **Finite Element Traversal:** Comparing `$a == $b` compares `[1, [1]] == [1, [1]]`, evaluating to `true` instantly.
4. **Recursion Guard:** As a defense-in-depth safeguard against extreme recursion depths, RHP's `strict_eq` engine enforces a hard stack limit of 256 levels:

```rust
pub fn strict_eq_depth(&self, other: &Self, depth: usize) -> bool {
    if depth > 256 {
        return false; // Guards against any infinite traversal
    }
    // Element-by-element strict comparison...
}
```

```php
list $a = [1];
$a[] = $a;

list $b = [1];
$b[] = $b;

print($a == $b); // true (0.001 ms, zero crash)
```

---

## Why Collections Forbid Reference Elements (`&`)

In legacy PHP, developers could explicitly insert references into arrays:

```php
// Legacy PHP (Anti-pattern):
$a = [];
$a[] = &$a; // Creates a cyclic reference in Zend Engine
```

This feature is widely regarded by compiler engineers as one of the most problematic designs in PHP history:
- It breaks Copy-on-Write optimizations because any write to a referenced element must propagate to every alias across the runtime.
- It introduces memory leaks, forcing PHP to build an asynchronous cycle collector that periodically halts the runtime to trace heap graphs.
- Every index lookup must check `Z_ISREF` and dereference an extra pointer indirection.

**In RHP, collections cannot store references:**
- Syntactically, attempting `$arr[] = &$b` produces a compile-time parse error.
- `&` is exclusively a **calling convention** (`inout` parameter passing), not a first-class storable value type.

---

## Pass-By-Reference: The `inout` Parameter Pattern

When a function genuinely needs to mutate a caller's collection in place, RHP uses explicit pass-by-reference annotations on the function signature and at the call site:

```php
// 1. Function explicitly declares an inout parameter with '&'
function appendMetadata(list &$items, string $tag) {
    $items[] = $tag;
}

$tags = ['auth', 'v1'];

// 2. Caller explicitly confirms inout intent with '&'
appendMetadata(&$tags, 'production');

print(count($tags)); // 3
print($tags[2]);     // "production"
```

### Why this design avoids type system friction
If references were modeled as a separate generic data type (e.g. `Ref<list>`), passing `&$tags` to a parameter typed as `list $items` would cause severe type checker friction (`Expected list, got Ref`).

By treating `&` as a calling convention (identical to Swift's `inout` or C#'s `ref`), the collection remains a `list` at both the call site and inside the function body. Type verification is seamless, fast, and static.

---

## How to Model Cyclic Graphs and Shared State

When an application requires true bidirectional links, shared graphs, or parent-child tree nodes, developers use **Objects** (`class`) paired with **Weak References** (`Weak::to()`):

```php
class TreeNode {
    public int $id;
    public WeakReference? $parent = null;
    public list $children = [];

    public function __construct(int $id) {
        $this->id = $id;
    }

    public function setParent(TreeNode $parent) {
        // Break the cycle using an explicit weak reference
        $this->parent = Weak::to($parent);
    }

    public function addChild(TreeNode $child) {
        $child->setParent($this);
        $this->children[] = $child;
    }
}
```

- **Objects are Reference Types:** `class` instances in RHP are reference-counted handles (`Rc<RefCell<VmObject>>`).
- **Weak References Eliminate Cycles:** `Weak::to($parent)` stores a `std::rc::Weak` pointer that does not increment the owning strong reference count, allowing tree nodes to drop cleanly when their root owner is dropped.
