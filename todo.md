# RHP Engineering Roadmap & Architectural Implementation Plan (TODO)

This document outlines the engineering roadmap and architectural improvements for **RHP**, incorporating high-performance design patterns inspired by the **Bun Rust codebase** (`oven-sh/bun`: `bun_js_parser`, `bun_ast`, `bun_collections`, `bun_jsc`, and `bun_alloc`).

---

## 1. Syntax & Core Language Rules

- [ ] **Native Script Syntax (No Opening/Closing Tags):**
  - `.rhp` files are treated as direct source code files.
  - Files **do not** start with `<?php` and **do not** close with `?>`. Parsing begins immediately with the first token of the source file.
  - Any occurrence of `<?php` or `?>` should be rejected or treated as an invalid token / syntax error.

- [ ] **Strict Subtyped Collection Invariants (`Arr::list`):**
  - `Arr::list` is strictly sequential and indexed by contiguous integers.
  - Attempting to set or append a string/custom key (e.g. `$arr["foo"] = 1;` or `$arr['key'] = $val;`) on an `Arr::list` must fail with an explicit runtime/compile error:
    ```text
    cannot use custom key for list. use dict or hashMap instead
    ```
  - Disallow automatic implicit type coercion or promotion from `Arr::list` to `dict` during key assignment unless using the generic permissive parent `array` type.

---

## 2. Phase-by-Phase Implementation Roadmap

### Phase 1: Frontend Lexer & AST Memory Allocation (Inspired by `bun_js_parser` & `bun_alloc`)

- [ ] **Handwritten SIMD Lexer with `memchr` (Replacing `logos` for Core Scanning):**
  - Transition from table DFA `logos` to a handwritten cursor scanner.
  - Use `memchr` / `memchr2` for SIMD-accelerated skipping of whitespace, newlines, and comments.
  - Implement precomputed ASCII lookup tables for identifier character validation (`ID_START`, `ID_CONTINUE`).
  - Seamlessly handle stateful tokenization (string interpolation, multi-line strings, heredocs) without `logos` callback overhead.

- [ ] **Arena-Based AST Allocation (`bumpalo` / `StoreSlice`):**
  - Implement bump-pointer arena allocation (matching Bun's `bun_alloc::Arena` and `oxc_allocator`).
  - Store AST node slices using arena-backed storage (`StoreSlice<T>`, `StoreStr`) instead of heap-scattered `Box<Expr>` and `Vec<Stmt>`.
  - Eliminate lifetime parameter `'a` propagation across AST structures.
  - Enable instant $O(1)$ bulk deallocation of the entire AST upon compilation completion.

- [ ] **String Interning (Atoms):**
  - Introduce an identifier interner (e.g. `lasso` or `string_cache`) for variable names, property names, and keywords.
  - Represent identifiers as compact 32-bit `Atom` (`u32`) handles for fast $O(1)$ comparisons.

- [ ] **Handwritten Recursive Descent + Pratt Expression Parser:**
  - Standardize on Recursive Descent for statements and class/function declarations.
  - Retain Top-Down Operator Precedence (Pratt) exclusively for arithmetic, logical, and ternary expressions.
  - Remove combinator parsing abstractions (`nom`/`combine` style) to eliminate closure allocation and call-stack bloat.

---

### Phase 2: Runtime Value Representation — 64-bit NaN Boxing (Inspired by `bun_jsc::JSValue`)

- [ ] **Replace 40-Byte `PhpValue` Enum with 64-Bit NaN-Boxed `Value(u64)`:**
  - Replace the current multi-word `PhpValue` enum with a single 8-byte IEEE-754 quiet-NaN encoded word (`#[repr(transparent)] struct Value(pub u64)`), identical to Bun's `bun_jsc::JSValue`.
  - **Bit-level encoding layout:**
    - **Double / Floats (`f64`):** Stored as raw IEEE-754 doubles.
    - **Standard Integers (`i32`):** `0xFFFE_0000_0000_0000 | (val as u32 as u64)`.
    - **Booleans (`bool`):** Tagged constants (`TRUE = 0x07`, `FALSE = 0x06`).
    - **Null / Undefined:** Dedicated bit patterns (`NULL = 0x02`, `UNDEFINED = 0x0A`).
    - **Heap Pointers (Strings, Objects, Collections):** 48-bit pointer packed into the low 48 bits with `NOT_CELL_MASK = 0xFFFE_0000_0000_0002`.
  - **Memory benefits:**
    - Reduces value size from 40 bytes to 8 bytes.
    - Fits 8 values per 64-byte CPU cache line (up from 1.6).
    - Register moves and parameter passing compile to a single CPU instruction (`mov rax, rbx`).
  - Represent 128-bit numbers (`i128`, `u128`) via boxed heap cells so the core value remains 8 bytes.

---

### Phase 3: High-Performance Subtyped Collections (Inspired by `bun_collections`)

- [ ] **Strict Collection Subtype Invariants:**
  - `Arr::list()`: Backed by a contiguous flat vector.
    - Throw `"cannot use custom key for list. use dict or hashMap instead"` when indexing with string keys.
  - `Arr::dict()`: Key-value dictionary retaining strict insertion ordering.
  - `Arr::set()`: Unique set collection with $O(1)$ membership checks.
  - `Arr::map()`: Universal dictionary supporting arbitrary complex keys.

- [ ] **Eliminate `RefCell` Overhead:**
  - Replace `Rc<RefCell<...>>` in collections with single-owner or explicit Copy-On-Write (COW) reference counters.
  - Eliminate runtime borrow counter checks on every hot-path read/write.

- [ ] **Fast Hash Tables with `hashbrown` & `raw-entry`:**
  - Switch from `std::collections::HashMap` (SipHash) to `hashbrown` with `AHash` / `wyhash`.
  - Utilize `raw-entry` probing (`get_hashed`, `put_hashed`) to hash strings once and reuse the hash across repeated lookups.

- [ ] **Packed Array Optimization:**
  - For sequential indexed collections, bypass hash calculation and store elements in flat contiguous memory.

---

### Phase 4: Intermediate Representation (IR) & SSA Lowering

- [ ] **Compact 32-Bit Instruction Encoding:**
  - Pack bytecode instructions into dense 32-bit words (e.g. `OP: 8 bits | DST: 8 bits | SRC1: 8 bits | SRC2: 8 bits`) or 64-bit fixed words.
  - Avoid large Rust enum payloads in the bytecode array to maximize L1 instruction cache ($L1i$) hit rates.

- [ ] **Out-of-Band Debug Span Line Tables:**
  - Remove `span` lookups from the hot execution path in `chunk.instructions`.
  - Maintain a separate out-of-band table (`Vec<(u32, Span)>`) queried exclusively on traps, errors, exceptions, and debugger stops.

- [ ] **Local SSA Optimization & Type Specialization:**
  - Lower AST to basic blocks with Static Single Assignment (SSA).
  - Statically infer unboxed operations for local variables that do not change type within function bodies (e.g. emit `AddI32` instead of polymorphic `AddVal`).

---

### Phase 5: Virtual Machine Execution Engine

- [ ] **Inner Loop Optimization & Pointer Pinning:**
  - Cache the active frame's instruction pointer (`*const OpCode`) and register window base (`*mut Value`) directly in local CPU registers.
  - Eliminate per-instruction frame array indexings (`self.frames[frame_idx]`).

- [ ] **Dense Match Jump Table Dispatch:**
  - Annotate `OpCode` with `#[repr(u8)]` and verify LLVM compiles the dispatch loop to an indirect jump table (`jmp [table + rdx*8]`).

- [ ] **Instruction Quickening & Inline Caches (IC):**
  - Dynamically rewrite polymorphic property/method access opcodes to specialized slot lookups (e.g. `GetProp` -> `GetPropSlot { offset }`) upon the first shape resolution.

---

### Phase 6: Just-In-Time (JIT) Pipeline with Cranelift

- [ ] **Calibrated Compilation Thresholds:**
  - Prevent premature compilation of short loops. Increase the Cranelift compilation threshold (e.g., 5,000–10,000 invocations) to ensure only long-lived hotspots are handed to the Cranelift backend.

- [ ] **On-Stack Replacement (OSR Entry & Exit):**
  - Implement OSR loop headers to transition execution from the bytecode interpreter into native Cranelift code mid-loop without returning.
  - Implement deoptimization bailout points (OSR Exit) that safely restore interpreter registers if dynamic runtime type assumptions fail.

---

### Phase 7: Gradual Type System & Boundaries

- [ ] **Strict Function & Method Boundary Contracts:**
  - Enforce explicit type signatures across public function and method boundaries.
  - Generate monomorphic calling convention stubs between typed functions to bypass value boxing/unboxing overhead.

- [ ] **Dynamic & Permissive Typing in Function Bodies:**
  - Retain ergonomic dynamic typing for untyped local variables within function bodies.
  - Prevent implicit type-shifting once a variable is explicitly annotated with a strict scalar or collection type hint.

---

### Phase 8: Concurrency, Parent Process & Facades

- [x] **Persistent Parent Process Architecture & Singleton Process Services:**
  - Parse and compile framework code once into immutable state (`Arc<EngineState>`), eliminating per-request bootstrap overhead.
  - Require `singleton` keyword for classes declared to run as part of the parent process.
  - Enforce invocation invariants: one can use both static and non-static methods of a singleton class within other singleton classes, but can only use static methods of a singleton class inside non-singleton classes.
  - Workers execute requests concurrently against shared immutable `Arc<EngineState>` without per-request bootstrap overhead.

- [ ] **Cooperative Tokio Fiber Concurrency:**
  - Ensure synchronous-by-default standard library facades (`File::read`, `DB::table`, `HTTP::get`) yield execution fibers cooperatively without blocking underlying OS worker threads.

- [ ] **Lock-Free Asynchronous Logging:**
  - Offload `Log::info()` payloads over a lock-free `mpsc` ring buffer to a dedicated background logging worker.

- [ ] **Post-Response Defer Execution Hooks:**
  - Execute per-request `defer()` hooks strictly after the client response payload is flushed.

- [x] **Static Facades & Dynamic Macros:**
  - Enforce static class facades (`Str`, `Arr`, `File`, `HTTP`, `Log`, `DB`, `Int`, `Float`) with forbidden inheritance (`extends Arr`), allowing extension via dynamic macro registration (`Arr::macro(...)`).
  - Three-Tier Facade Architecture: Tier 1 (Native Engine Facades), Tier 2 (Custom Rust Plugins/Crates via AOT), and Tier 3 (Script-land Static Facades).
  - Engine-level `is_static_only` flag halting illegal instantiation attempts (`new Int()`, dynamic `new $cls()`) with fatal error at compile-time and runtime.

---

## 3. Progress Tracking

| Subsystem | Planned Improvement | Status | Priority |
| :--- | :--- | :--- | :--- |
| **Syntax** | Remove `<?php` and `?>` tag requirement | Pending | **P0** |
| **Collections** | Strict `Arr::list` string key error (`cannot use custom key for list...`) | Pending | **P0** |
| **Value Model** | 64-bit NaN-boxed `Value(u64)` (Bun `JSValue` pattern) | Pending | **P1** |
| **Lexer** | Handwritten SIMD scanner with `memchr` | Pending | **P1** |
| **Parser / AST** | Arena allocation (`bumpalo`) without lifetime contamination | Pending | **P1** |
| **VM Dispatch** | Pin register/IP pointers; out-of-band debug spans | Pending | **P1** |
| **Collections** | `hashbrown` with `raw-entry`, `wyhash`, COW without `RefCell` | Pending | **P2** |
| **IR Lowering** | Compact 32-bit packed instruction format | Pending | **P2** |
| **JIT Engine** | High threshold + OSR entry/exit bailout | Pending | **P2** |
| **Type System** | SSA-based local type specialization & monomorphic call stubs | Pending | **P2** |
