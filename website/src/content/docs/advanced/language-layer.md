---
title: "Language Layer & Semantics"
description: "The RHP language boundary: syntax, type system, primitive stdlib, memory invariants, language-level crash containment, and the CLI Test."
section: "Advanced"
order: 3
---

## What the Language Layer Owns

The **Language Layer** (`rhp` compiler & VM) owns **everything that defines what code means**. It is strictly decoupled from the operating system, HTTP protocols, process models, and application frameworks.

```text
┌────────────────────────────────────────────────────────────────────────┐
│                   LAYER 1: LANGUAGE (rhp Compiler & VM)               │
│                                                                        │
│  - Lexer, Pratt Parser, Type Checker   - Register Bytecode VM / JIT    │
│  - Types: PhpValue, Int, Float, Collections (list, dict, set)          │
│  - Primitive stdlib: Str, Arr, DB handle- Future primitive (run, defer)│
│  - Rust FFI & Crate Interop            - Memory Safety & Crash Boundary│
└────────────────────────────────────────────────────────────────────────┘
```

---

## 1. Syntax, Pratt Parser & Type System

- **Zero-Copy Lexer:** Deterministic Logos finite automata (DFA) slicing directly into the source buffer without heap allocations.
- **Pratt Operator Precedence:** Evaluates complex nested expressions and combinators with exact source span tracking (`Span { file_id, start, end, line, col }`).
- **Gradual Static Typing:**
  - Fast-path unboxed numeric primitives (`int`, `float`, `bool`, `string`) lowered directly to Cranelift IR instructions.
  - Dynamically typed values boxed within high-density `PhpValue` tagged unions.
  - Mandatory function and method parameter type annotations enforcing strict contracts at API boundaries.
- **Migration Type Inference:** The `rhp migrate` tool infers concrete static types from docblocks, assignments, and control flow to automatically convert legacy PHP into typed RHP.

---

## 2. Primitive Data Structures & Semantics

Rather than relying on Zend Engine's monolithic `zend_array` hash bucket architecture, RHP provides specialized, CPU-conscious data types:

| Surface Type | Underlying Rust Representation | Semantic Invariant |
|---|---|---|
| `list` / `Arr::list()` | `Rc<RefCell<Vec<PhpValue>>>` | Sequential indexed elements, contiguous memory, maximum CPU cache line locality. |
| `dict` / `Arr::dict()` | `Rc<RefCell<IndexMap<String, PhpValue>>>` | Dense linear key-value storage retaining strict insertion order with sparse hashing. |
| `set` / `Arr::set()` | `Rc<RefCell<HashSet<PhpValue>>>` | Dense unique collection with $O(1)$ membership checking and mathematical set algebra. |
| `PhpValue::Frozen` | `FrozenValue` (`Send + Sync`) | Immutable value snapshots safe for cross-worker sharing with zero locks. |

### Copy-On-Write (CoW) Detachment
Collections utilize reference-counted interior mutability. Reassigning a collection (`$copy = $original;`) is an instantaneous $O(1)$ pointer copy (`Rc::clone`). In-place mutations on shared buffers trigger transparent detachment, cloning the underlying vector only when mutability is required.

---

## 3. Core Standard Library Primitives

The core language stdlib is deliberately minimal, providing only the primitives required for high-performance systems programming:

- **`Str` Facade:** Binary-safe, UTF-8 string manipulation, regex matching, slicing, and phonetic hashing.
- **`Arr` Facade:** Stateless collection pipeline operations (`map`, `filter`, `reduce`, `flatten`, `chunk`).
- **Low-Level Database Interface:** Primitive raw query and table handles for database drivers (not an ORM).
- **`Future` Concurrency Primitive:**
  - `Future::run(callable $fn)`: Spawns an asynchronous task.
  - `Future::defer(callable $fn)`: Registers a post-computation deferred callback.
  - `Future::wait(array $futures)`: Awaits multiple concurrent futures with destructuring support.
  - `Future::delayed(int $ms, callable $fn)`: Cooperative timer delay.

---

## 4. Language-Level Fault Containment Guarantee

In traditional C extensions or unmanaged runtimes, a segmentation fault, null-pointer dereference, or uncaught panic crashes the entire server process.

In RHP, the language layer enforces strict **Fault Containment**:
- Any runtime error (e.g. division by zero, invalid type cast, failed assertion) or panic caught within user code or native facades **aborts only the current evaluation scope/request**.
- The evaluation environment drops its local registers, records diagnostic telemetry, and bubbles a structured error result.
- The host process and other concurrent execution scopes remain 100% unaffected.

---

## 5. Rust Interoperability (Direct `.rs` Imports)

The language layer allows importing native Rust source files directly as helper classes:

```php
import "./FastHash.rs";

string $hash = FastHash::compute("sensitive-payload");
```

- When `rustc` is available, `.rs` files are compiled AOT to `cdylib` with SHA-256 caching.
- In rapid development or environments without `rustc`, the language engine executes the native logic via an in-process thread-safe fallback runner.
- Zero C-FFI boilerplate, zero header files, and zero runtime overhead.

---

## 6. The CLI Test (Litmus Test for Language Purity)

> **The Litmus Test:** You must always be able to execute any pure `.rhp` script from the command line with zero servers, zero workers, and no framework:
> ```bash
> rhp compute.rhp
> ```

If `compute.rhp` requires an active HTTP server, crashes because no worker pool exists, or expects web superglobals to run, **an abstraction from the Runtime or Framework layer has contaminated the Language layer.**

### Strict Boundary Non-Concerns:
- **No HTTP Knowledge:** The compiler and VM know nothing about HTTP verbs, status codes, cookies, or socket accept loops.
- **No Process Model Visibility:** The language does not expose `fork()`, `pid`, `isWorker()`, or "parent/child" terminology. User code only interacts with abstract execution phases (**boot phase** vs **request phase**), leaving OS process mechanics entirely to Layer 2.
