
## Technical Language Construction Architecture (20 Technical Points)

1. **Frontend Lexical Analysis Engine:** Implement a deterministic finite automaton (DFA) based lexer using the `logos` crate to tokenize source code files with zero-copy string slicing.
2. **Grammar & Expression Parsing:** Build a custom Pratt parser combined with combinator parsing to construct a strongly-typed Abstract Syntax Tree (AST) representing the program's logical structure.
3. **Type System Strategy (Medium / Gradual):** Enforce strict type signatures across function and method boundaries while utilizing dynamic type inference within function bodies to preserve clean developer ergonomics.
4. **Intermediate Representation (IR) Lowering:** Translate the typed AST into flat, compact linear bytecode instructions optimized for execution within a custom virtual machine.
5. **Virtual Machine Execution Engine:** Design a native Rust register-based bytecode VM loop to achieve fast startup times and efficient low-level instruction dispatching.
6. **Just-In-Time (JIT) Compilation Pipeline:** Integrate the Cranelift compiler backend to conditionally compile hot execution loops and long-lived functions into native machine code at runtime.
7. **Dynamic Value Enum Representation:** Define a unified core `PhpValue` Rust enum capable of representing primitive and complex types, utilizing copy-on-write semantics for strings and arrays.
8. **Subtyped Collection Architecture (`Arr::list`):** Map sequential lists directly to underlying Rust `Vec<PhpValue>` structures to maximize CPU cache locality during iteration.
9. **Subtyped Collection Architecture (`Arr::dict`):** Map key-value dictionaries with strict insertion ordering to Rust `IndexMap<String, PhpValue>` structures.
10. **Subtyped Collection Architecture (`Arr::set`):** Map unique collections directly to Rust `HashSet<PhpValue>` structures.
11. **Subtyped Collection Architecture (`Arr::map`):** Implement universal general key-value dictionaries supporting complex keys using Rust `HashMap<PhpValue, PhpValue>` structures.
12. **Object-Oriented Facade Standard Library & Runtime Macros:** Eliminate procedural global functions in favor of static class facades (such as `Str::contains`, `Arr::list`, and `Sys::env`) to maintain namespace hygiene. Forbid traditional class inheritance (`extends Arr`) to preserve Rust memory layout boundaries and stateless Tokio safety, instead supporting Laravel-style dynamic runtime macro registration (`Arr::macro('toCsv', ...)`).
13. **Persistent Parent Process Architecture:** Boot, parse, and compile application code inside a long-lived parent process into an immutable memory state (`Arc<EngineState>`) to avoid per-request bootstrap overhead.
14. **Worker Process Isolation & Concurrency:** Spawn multi-threaded worker tasks or use process-level copy-on-write models to handle incoming requests concurrently against the shared parent state.
15. **Database Connection Pooling:** Manage persistent database connection pools (via `sqlx`) centrally within the parent process, allowing workers to check out connections instantly during active requests.
16. **Cooperative Fiber Concurrency Runtime:** Build synchronous-by-default execution on top of Tokio, allowing blocking I/O calls (`File::read`, `DB::table`) to cooperatively yield execution fibers without blocking OS worker threads.
17. **Asynchronous Non-Blocking Logging Channel:** Decouple `Log::info()` calls by pushing structured log payloads into an in-memory lock-free multi-producer single-consumer (`mpsc`) channel consumed by a background worker.
18. **Post-Response Defer Execution Hooks:** Implement a per-request `defer()` queue that executes registered closures strictly after the HTTP response payload has been flushed to the client, preventing background tasks from adding to latency.
19. **Asynchronous Task Wrapper (`async()`):** Provide a native `async()` expression wrapper to spawn background concurrent futures and resolve non-blocking promises via explicit `.wait()` calls.
20. **Memory Ownership & Safety Enforcement:** Leverage Rust’s compile-time borrow checker and ownership model to guarantee absolute memory safety and eliminate data races across concurrent worker threads without relying on a traditional tracing garbage collector.
