---
title: "Memory Model & Zero-GC"
description: "Zero-GC architecture, deterministic RAII drop semantics, bump request arenas, and static cycle prevention."
section: "Advanced"
order: 1
---

## Complete Absence of a Tracing Garbage Collector

Unlike legacy runtimes (PHP Zend Engine, Go, Node.js V8, JVM) that incur stop-the-world pauses and cyclic GC sweeps, RHP is **100% free of a tracing garbage collector**:

```php
<?rhp

Sys::print("Is GC Free: " . (Memory::isGcFree() ? 'true' : 'false')); // true
Sys::print("GC Total Runs: " . Memory::gcRuns());                    // 0
Sys::print("GC Pause Duration: " . Memory::gcPauseMs() . " ms");      // 0.0 ms
```

---

## Request-Scoped Bump Arena Allocation (`RequestArena`)

In a web application, short-lived allocations (request payloads, JSON string slices, temporary lists) are allocated from a contiguous pre-allocated memory chunk using an 8-byte aligned bump pointer:

1. **Sequential Allocation:** Fast pointer increments without thread-locking global heap mutexes.
2. **Instant $O(1)$ Reset:** When an HTTP request completes, the entire arena bump pointer is reset to 0 in $O(1)$ time (`finish_request()`).
3. **Zero Heap Fragmentation:** Pre-allocated chunks are retained across requests, avoiding OS `mmap`/`malloc` thrashing.

---

## Static Cycle Prevention at the Language Boundary

In traditional dynamic languages, cyclic references (e.g. object A referencing object B, which references object A) cannot be freed by reference counting alone, necessitating a cyclic garbage collector.

RHP prevents cycles statically at compile time:
1. **Pass-By-Reference Forbidden:** `&$var` syntax is eliminated from the language grammar.
2. **Closure Reference Capture Forbidden:** Closures cannot capture variables by mutable reference (`use (&$var)` is rejected).
3. **Weak References (`Weak::to`):** Bidirectional graph relationships (e.g., parent-child nodes) must use explicit weak references:

```php
<?rhp

class Node {
    public ?WeakReference $parent = null;
    public list $children;

    public function setParent(Node $parent): void {
        $this->parent = Weak::to($parent);
    }
}
```

---

## Scoped Arenas (`Memory::scope`)

For compute-intensive operations, isolate allocations inside a lexical memory scope:

```php
<?rhp

$result = Memory::scope(function(): int {
    $buffer = Str::repeat("DATA", 100000);
    return Str::byteLength($buffer);
    // $buffer is reclaimed immediately upon scope exit
});
```
