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

Rather than using Zend Engine's monolithic `zend_array` (which incurs massive memory overhead with 56-byte hash buckets and separate doubly linked list pointers for iteration order), RHP uses dedicated, cache-conscious Rust structures with `Rc` Copy-On-Write semantics, focusing on three core primitives (`list`, `dict`, `set`) while deferring arbitrary `hashMap` and `tuple` structures:

### 1. `Arr::list`: `Rc<Vec<Value>>`
- **Contiguous Vector Buffer:** Stored directly as a dense vector of 8-byte `Value` registers, offering direct CPU L1/L2 cache prefetching and sequential $O(1)$ indexing without hash calculations or bucket indirections.
- **Copy-On-Write (COW):** Parameter passing (`foo($list)`) and assignment (`$copy = $list`) perform an instantaneous $O(1)$ reference count increment (`Rc::clone`). In-place append/mutation uses `Rc::make_mut`—if unshared (`strong_count == 1`), mutation is in-place with zero reallocation.

### 2. `Arr::dict`: `Rc<IndexMap<Key, Value, ahash::RandomState>>`
- **Hardware AES-Accelerated Hashing (`ahash`):** Powered by `ahash::RandomState`, utilizing host CPU AES-NI instructions for 5x–10x faster key hashing than standard SipHash while remaining DOS-resistant.
- **Dense Linear Storage:** Keys and values are stored in contiguous vectors indexed by a sparse hash table (`IndexMap`), achieving compact memory layout with near-zero pointer chasing.
- **Strict Deterministic Ordering:** Iteration order strictly matches insertion order. Updating an existing key modifies its value in-place without altering its slot position in the iteration order.
- **Zero-Allocation Streaming Iteration:** `foreach ($dict as $key => $val)` iterates directly across the underlying slice by index cursor, allocating **zero temporary vectors or key-value tuples**.

### 3. `Arr::set`: `Rc<IndexSet<Key, ahash::RandomState>>`
- **Unique Value Buffer:** Backed by `IndexSet` with `ahash::RandomState` for $O(1)$ membership checks, deduplication, and mathematical set operations (`diff`, `intersect`, `union`).
- **Insertion-Preserved Ordering:** Elements preserve their insertion sequence, enabling deterministic iteration and serializability.

> [!NOTE]
> **Streamlined Architecture (`tuple` & `hashMap` on hold):**
> Universal `HashMap<Key, Value>` (with complex keys like arrays or objects) and heterogeneous `tuple` types are deliberately deferred. In web workloads, 99.9% of structured data maps cleanly to `list` (indexed items) and `dict` (string/atom-keyed attributes). Deferring complex maps eliminates expensive recursive equality hashing and keeps cache lines dense and predictable.

---

## Memory & State Rules

### 1. Boot Heap Immutability (`index.rhp` $\rightarrow$ CoW Worker Memory)
The application boot phase in `index.rhp` executes once in the master process prior to worker forking. All parsed routes, compiled bytecode, and container singletons are frozen into an immutable state (`Arc<EngineState>`), shared read-only across all pre-forked worker processes via OS Copy-on-Write (CoW).
- **Strict Single-Threaded Boot Rule:** The master process remains strictly single-threaded during boot. It is forbidden to spawn Tokio runtimes, thread pools, or database connections before forking (`fork()` deadlocks if locks are held by non-calling threads). Workers initialize their connection pools and async engines post-fork.
- **Read-Only Invariant:** Request code inside workers cannot mutate boot-time structures. Any attempt to write to frozen boot data throws a fatal runtime error.
- **Shared Mutable State:** Any state that must be shared across requests or workers must pass through explicit host-provided native APIs (e.g., Redis, SQLite/PostgreSQL, or host cache stores).

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

---

<a id="borrow-checker"></a>
## Borrow Checker and Memory Safety

A common question for engineers discovering RHP is: **how does RHP handle the borrow checker?**

Do developers have to manage Rust-style lifetime parameters (`'a`), or does the runtime enforce memory safety without either borrow-checking friction or a traditional tracing garbage collector?

RHP handles the borrow checker on two deliberate fronts:

### 1. Script-Land: Safety Without Lifetime Annotations
Application developers writing RHP code never need to declare lifetime annotations or battle borrow checker errors. Instead, the language guarantees memory safety through strict compile-time constraints combined with value semantics:
- **Pass-By-Reference Elimination:** Explicit pass-by-reference (`&$var`) and closure reference captures (`use (&$var)`) are forbidden at the compiler/type-checker level (`PassByReferenceForbidden`). This eliminates pointer aliasing hazards and reference lifetime tracking from user code.
- **Static Cycle Prevention:** The compiler statically detects cyclic assignment risks (such as closures capturing `$this` assigned into `$this->property`) and requires bidirectional object relationships to use explicit [`WeakReference`](#static-cycle-prevention-at-the-language-boundary) handles (`Weak::to($parent)`).
- **Copy-On-Write (CoW) Detachment:** Reassignments (`$copy = $dict`) share underlying memory with zero allocations via reference counting. In-place mutations on shared buffers automatically detach and clone (`ensure_dict_mut()`, `ensure_list_mut()`), providing predictable value semantics without lifetime management.
- **Request-Scoped Arenas:** Ephemeral request data is allocated inside linear bump arenas (`RequestArena`) and dropped in bulk in $O(1)$ time when the request completes.

### 2. Runtime Engine: Navigating Rust's Borrow Checker Internally
Under the hood, the RHP virtual machine is implemented entirely in Rust and strictly upholds Rust's compile-time ownership model and borrow rules (`Send + Sync`):
- **Scoped Interior Mutability:** Complex runtime values (`Array`, `List`, `Dict`, `Set`, `HashMap`, `Object`, `Future`) use `Rc<RefCell<...>>` for single-threaded mutation within the VM execution loop. To prevent runtime `BorrowMutError` panics, all borrow guards (`borrow()` and `borrow_mut()`) are tightly scoped to discrete instruction cycles.
- **Cursor-Based Streaming Iteration:** Loop iteration (`foreach`) relies on index offset cursors (`ActiveIterator`) rather than holding Rust references open across bytecode dispatch boundaries.
- **Compile-Time Data Race Elimination (`FrozenValue`):** Active runtime values (`PhpValue`) are strictly `!Send` and `!Sync` due to thread-local reference counting. Any data moving across thread boundaries (such as parent boot state or background `Future::run` tasks) must convert into an immutable snapshot (`FrozenValue`), which implements `Send + Sync + 'static'`. This guarantees that multi-threaded concurrency in worker pools is verified statically by Rust's borrow checker.


