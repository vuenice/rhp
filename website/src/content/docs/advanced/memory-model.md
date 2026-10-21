---
title: "Memory Model & Zero-GC"
description: "Zero-GC architecture, deterministic RAII drop semantics, bump request arenas, and static cycle prevention."
section: "Advanced"
order: 1
---

## Complete Absence of a Tracing Garbage Collector

Unlike legacy runtimes (PHP Zend Engine, Go, Node.js V8, JVM) that incur stop-the-world pauses and cyclic GC sweeps, RHP is **100% free of a tracing garbage collector**:

```php
print("Is GC Free: " . (Memory::isGcFree() ? 'true' : 'false')); // true
print("GC Total Runs: " . Memory::gcRuns());                    // 0
print("GC Pause Duration: " . Memory::gcPauseMs() . " ms");      // 0.0 ms
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
$result = Memory::scope(function(): int {
    $buffer = Str::repeat("DATA", 100000);
    return Str::byteLength($buffer);
    // $buffer is reclaimed immediately upon scope exit
});
```

---

## Subtyped Collection Memory Layout & Streaming Iteration

Rather than using Zend Engine's monolithic `zend_array` (which incurs massive memory overhead with 56-byte hash buckets and separate doubly linked list pointers for iteration order), RHP uses dedicated, cache-conscious Rust structures:

### 1. `Arr::dict`: `Rc<RefCell<IndexMap<String, PhpValue>>>`
- **Dense Linear Storage:** Keys and values are stored in contiguous vectors indexed by a sparse hash table (`IndexMap`), achieving compact memory layout with near-zero pointer chasing.
- **Strict Deterministic Ordering:** Iteration order strictly matches insertion order. Updating an existing key modifies its value in-place without altering its slot position in the iteration order.
- **Zero-Allocation Streaming Iteration:** `foreach ($dict as $key => $val)` initializes an `ActiveIterator::Dict { dict, cursor: usize }`. Each iteration step reads `dict.get_index(cursor)` directly by index offset, allocating **zero temporary vectors or key-value tuples**.
- **Copy-On-Write (CoW) Detachment:** Reassignment (`$copy = $dict`) shares the underlying reference-counted buffer with zero allocations (`Rc::clone`). In-place mutations on shared buffers trigger transparent detachment (`ensure_dict_mut()`), cloning only when necessary.

### 2. `Arr::list`: `Rc<RefCell<Vec<PhpValue>>>`
- **Contiguous Vector Buffer:** Stored directly as a dense vector of `PhpValue` tagged unions, offering direct CPU L1/L2 cache prefetching and sequential indexing without hashtable hashing overhead.

---

## Memory & State Rules

### 1. Boot Heap Immutability (`server.rhp` $\rightarrow$ `Arc`)
The boot script (`server.rhp`) executes once at startup. All parsed routes, compiled classes, and container singletons are frozen into an immutable boot heap (`Arc<EngineState>`), shared read-only across all worker threads. 
- **Read-Only Invariant:** Request code cannot mutate boot-time structures. Any attempt to write to boot data throws a fatal runtime error.
- **Shared Mutable State:** Any state that must be shared across requests or threads must pass through explicit host-provided native APIs.

### 2. Execution Invariant: Everything Finishes Before Arena Drop
- **Deferred Work:** Callbacks registered via `Future::defer()` execute strictly *before* the request arena is dropped and reset.
- **Job Queues:** Asynchronous or queued tasks are serialized into the host queue before teardown.
- **Bulk Reclamation:** Once all deferred logic completes, the entire request arena drops in a single $O(1)$ step, leaving no leaks and requiring no GC traversal.

### 3. Host-Owned Resource Handles
- Operating system resources (database connections, raw sockets, log file descriptors) live in persistent, host-owned pools.
- Request code holds lightweight handles rather than raw OS objects.
- Borrowed connections are automatically sanitized and reset upon return to the pool, and are reclaimed on timeout.

### 4. Cache Data Invariant (`Cache::get/set`)
- The cache accepts **plain data only**: primitive scalars (integers, floats, strings, booleans) and arrays/dictionaries.
- Live object instances, closures, and resource handles cannot be cached, guaranteeing zero cross-request pointer corruption.
- Supported cache drivers include file, database, and Redis, with an optional in-memory L1 cache in front.

### 5. VM Deadlines and Memory Caps
- The VM checks execution deadlines at loop headers and function invocations.
- A per-arena memory cap kills runaway requests before they exceed system capacity.

