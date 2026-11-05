---
title: "Compiler & VM Internals"
description: "Deep dive into the compilation pipeline: Logos lexer, Pratt parser & combinators, typed AST, register bytecode VM engine, PhpValue, and Cranelift JIT."
section: "Advanced"
order: 3
---

## Compilation Pipeline Architecture

RHP is engineered with a dual-mode execution architecture sharing a unified frontend:

```text
Source Code (.rhp)
       │
       ▼
 [Logos Lexer]        Zero-copy DFA tokenization referencing source lifetime 's
       │
       ▼
 [Pratt Parser &]     Custom Pratt operator precedence combined with 
 [Combinators  ]      combinatorial grammar rules (delimited, separated_by)
       │
       ▼
 [Typed AST    ]      Strongly-typed logical representation (Program<'s>, Stmt<'s>, Expr<'s>)
                      with exact source span tracking (Span { file_id, start, end, line, col })
       │
 ┌─────┴────────────────────────┐
 │                              │
 ▼                              ▼
[Dev Mode: AST Tree-Walk]     [Prod Mode: IR Bytecode VM]
- Sub-ms cold start           - Flat linear register opcodes
- Precise source spans        - Multi-worker Tokio fiber pool
- DAP debugging traps         - Cranelift JIT compilation
- LCOV execution coverage     - High cache locality & zero GC
```

---

## 1. Zero-Copy Lexing (`src/lexer/`)

The lexer leverages `logos` to compile token regular expressions into a deterministic finite automaton (DFA) state machine. Tokens slice directly into the source buffer without allocating heap `String` instances:

```rust
#[derive(Logos, Debug, PartialEq, Clone)]
pub enum Token<'s> {
    #[token("fn")]
    Fn,
    #[token("function")]
    Function,
    #[token("class")]
    Class,
    #[regex(r"\$[a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*", |lex| lex.slice())]
    Variable(&'s str),
    #[regex(r"[a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*", |lex| lex.slice())]
    Ident(&'s str),
    #[token("<?php")]
    PhpOpen,
    #[token("?>")]
    PhpClose,
}
```

Every token is wrapped in a `SpannedToken<'s>` pairing the token with its source position:
```rust
pub struct SpannedToken<'s> {
    pub token: Token<'s>,
    pub span: Span,
}
```

---

## 2. Pratt Expression Parser & Combinator Engine (`src/parser/`)

Expression and statement parsing combines two complementary parsing techniques:

1. **Custom Pratt Parser (`src/parser/pratt.rs`):** Vaughan Pratt’s top-down operator precedence algorithm resolves mathematical, logical, and member access expressions in linear time $O(N)$ with zero backtracking ambiguity.
2. **Combinator Parsing (`src/parser/combinator.rs` & `src/parser/stmt.rs`):** Composable stream helpers parse structured grammar blocks, parameter lists, attributes, and statement sequences.

### Pratt Binding Power Table

Precedence is controlled by explicit numeric binding powers (`left_bp`, `right_bp`). By adjusting left vs. right binding power, operators declare their associativity:

```rust
pub mod binding_power {
    pub const LOWEST: u8 = 0;
    pub const YIELD: u8 = 1;
    pub const WORD_OR: u8 = 5;
    pub const WORD_XOR: u8 = 7;
    pub const WORD_AND: u8 = 9;
    pub const ASSIGNMENT: u8 = 20;     // Right-associative: (20, 19)
    pub const TERNARY: u8 = 25;        // Right-associative: (25, 24)
    pub const NULL_COALESCE: u8 = 30;  // Right-associative: (30, 29)
    pub const LOGICAL_OR: u8 = 35;
    pub const LOGICAL_AND: u8 = 40;
    pub const BITWISE_OR: u8 = 45;
    pub const BITWISE_XOR: u8 = 50;
    pub const BITWISE_AND: u8 = 55;
    pub const EQUALITY: u8 = 60;
    pub const RELATIONAL: u8 = 65;
    pub const SHIFT: u8 = 70;
    pub const ADDITIVE: u8 = 75;
    pub const MULTIPLICATIVE: u8 = 80;
    pub const INSTANCEOF: u8 = 85;
    pub const POWER: u8 = 90;          // Right-associative: (91, 90)
    pub const PREFIX: u8 = 93;
    pub const POSTFIX: u8 = 95;
    pub const CALL_MEMBER_INDEX: u8 = 100;
}
```

#### Pratt Parser Loop (`parse_expr`):
```rust
pub fn parse_expr(&mut self, min_bp: u8) -> Result<Expr<'s>, ParseError> {
    let mut left = self.parse_prefix()?; // nud()

    loop {
        let next_tok = match self.peek() {
            Some(t) => t.clone(),
            None => break,
        };

        // Postfix operators ($x++, $x--)
        if let Some(postfix_bp) = postfix_binding_power(&next_tok.token) {
            if postfix_bp < min_bp { break; }
            self.advance();
            left = Expr::new(ExprKind::Unary { op, expr: Box::new(left), is_prefix: false }, span);
            continue;
        }

        // Infix operators (+, *, ->, ::, (), [], ??, =, etc.)
        if let Some((left_bp, right_bp)) = infix_binding_power(&next_tok.token) {
            if left_bp < min_bp { break; }
            left = self.parse_infix(left, next_tok, right_bp)?; // led()
            continue;
        }

        break;
    }

    Ok(left)
}
```

### Special Pratt Handling

* **Right-Associativity:** 
  * Exponentiation: `2 ** 3 ** 4` parses as `2 ** (3 ** 4)`.
  * Chained assignment: `$a = $b = 10` parses as `$a = ($b = 10)`.
  * Null-coalesce: `$a ?? $b ?? $c` parses as `$a ?? ($b ?? $c)`.
* **Fluent Chaining & Scope Resolution:** High binding power (`100`) binds `Arr::list(...)` and chains subsequent `->map(...)` and `->filter(...)` calls cleanly without parentheses.
* **Array Destructuring:** Prefix `[` detects lvalue target lists (`[$user, $posts] = Future::wait([...])`).
* **Double-Quoted String Interpolation:** `"user={$id}"` is tokenized and decomposed into `Literal::InterpolatedString` with discrete `StringPart::Literal` and `StringPart::Expr` chunks.
* **Static Facade Protection:** Instantiating static facades (`new Int()`, `new Arr()`, `new Str()`) triggers an immediate compile-time `ParseErrorKind::StaticFacadeInstantiationForbidden` error:
  ```text
  Fatal Error: Cannot instantiate static facade class 'Int'. Use static methods directly.
  ```

---

## 3. Strongly-Typed Abstract Syntax Tree (`src/ast/`)

The AST is organized into modular Rust submodules:

* **[src/ast/types.rs](file:///home/yogesh/code/rhp/src/ast/types.rs):**
  * `Ident<'s>` and `Variable<'s>` zero-copy slice wrappers.
  * `TypeHint<'s>` supporting primitives (`int`, `float`, `String`, `bool`, `void`, `mixed`), Rust native numbers (`i64`, `f64`, `u32`), nullable types (`?Type`), unions (`Type1|Type2`), and intersections (`Type1&Type2`).
  * `Visibility` (`Public`, `Protected`, `Private`) and `ClassModifiers` (`is_abstract`, `is_final`, `is_readonly`).
* **[src/ast/expr.rs](file:///home/yogesh/code/rhp/src/ast/expr.rs):**
  * `Expr<'s>` carrying `span: Span` and `kind: ExprKind<'s>`.
  * `ExprKind<'s>` enum variants: `Literal`, `Variable`, `Ident`, `Unary`, `Binary`, `Assign`, `Ternary`, `Call`, `MethodCall`, `StaticCall`, `PropertyFetch`, `StaticPropertyFetch`, `ClassConstFetch`, `Array`, `ArrayIndex`, `List`, `ArrowFunction`, `Closure`, `Match`, `New`, `Clone`, `Instanceof`, `Cast`, `Yield`, `Await`, `Grouping`.
* **[src/ast/stmt.rs](file:///home/yogesh/code/rhp/src/ast/stmt.rs):**
  * `Stmt<'s>` carrying `span: Span` and `kind: StmtKind<'s>`.
  * `StmtKind<'s>` enum variants: `Expr`, `VarDecl` (`$x: int = 10;`), `Block`, `If`, `While`, `DoWhile`, `For`, `Foreach`, `Switch`, `Break`, `Continue`, `Return`, `Throw`, `TryCatch`, `FunctionDecl`, `ClassDecl`, `InterfaceDecl`, `TraitDecl`, `EnumDecl`, `Namespace`, `Use`, `Global`, `Echo`, `Defer`, `Empty`.
* **[src/ast/visitor.rs](file:///home/yogesh/code/rhp/src/ast/visitor.rs):**
  * Implements `AstVisitor<'s>` providing visitor hooks for immutable AST traversals across all nodes.

### Exact Source Span Tracking

Every token and AST node preserves a `Span`:
```rust
pub struct Span {
    pub file_id: usize,
    pub start: usize,   // Byte offset start
    pub end: usize,     // Byte offset end
    pub line: usize,    // 1-based line number
    pub col: usize,     // 1-based column number
}
```
Spans merge transparently (`span_a.merge(span_b)`) as combinators consume tokens, enabling `SourceMap::format_diagnostic` to emit rustc-grade terminal errors with underlined code spans.

---

## 4. Intermediate Representation (IR) Lowering (`src/ir/`)

Before execution in Production Mode, the typed AST is translated into a flat, compact linear register-based intermediate representation (`IrModule` and `BytecodeChunk`):

- **Virtual Register Allocation (`Reg(u16)`):** Local variables, function parameters, and expression temporaries are allocated contiguous 16-bit virtual registers (`r0`..`r65535`).
- **Constant Pool Deduplication (`Constant` / `ConstantId`):** Primitives, interned UTF-8 string literals, 64-bit numbers, and semantic `RhpType` contracts are deduplicated in per-chunk constant pools.
- **Boundary Contract Guards:** Emits explicit `OpCode::AssertType` instructions at function/method parameter entry, return sites, and locked local declarations (`$x: int = ...`).
- **Facade & Collection Specialization:** Lowers `Arr::list`, `Arr::dict`, `Arr::set`, and `Arr::hashMap` directly into `NewCollection` opcodes, native Rust integer/float constructors into `WrapRustInt` / `WrapRustFloat`, and legacy PHP calls into canonical facade dispatches.

---

## 5. Virtual Machine Execution Engine (`src/vm/`)

Implements **Step 5** of the Technical Language Construction Architecture: a native Rust register-based bytecode VM loop engineered for ultra-fast cold start times, high CPU cache locality, and efficient instruction dispatching.

### Register VM Architecture vs. Stack-Based VMs

Unlike traditional stack-based virtual machines (such as PHP's Zend Engine, Python bytecode, or JVM) that push and pop operands repeatedly through stack frames:
1. **Fewer Instructions per Operation:** Register instructions specify explicit source and destination registers (e.g. `BINARY dst, lhs op rhs`), reducing the total instruction count by 35–45%.
2. **Minimal Stack Thrashing & L1 Cache Locality:** All active call frames index into a single pre-allocated contiguous register file (`Vec<PhpValue>`), using base offsets: `registers[frame.base_reg + reg.index()]`.
3. **Flat, Copy-able Instructions:** Each `OpCode` is 16 bytes or smaller, self-contained, and implements `Copy`, eliminating heap pointer chasing in the inner dispatch loop.

```text
┌────────────────────────────────────────────────────────────────────────┐
│              CONTIGUOUS REGISTER FILE (Vec<PhpValue>)                 │
├───────────────────────┬───────────────────────┬────────────────────────┤
│ Frame 0: <main>       │ Frame 1: calculate()  │ Frame 2: format()      │
│ base_reg = 0          │ base_reg = 32         │ base_reg = 48          │
│ [r0, r1, r2, ... r31] │ [r0, r1, r2, ... r15] │ [r0, r1, r2, ... r15]  │
└───────────────────────┴───────────────────────┴────────────────────────┘
```

### Core Value Tagged Union: `PhpValue` (`src/vm/value.rs`)

`PhpValue` is the unified runtime representation of all scalar and complex data types, featuring zero heap indirection for scalars and Copy-On-Write (COW) semantics for collections:

```rust
pub enum PhpValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    RustInt { kind: RustIntKind, value: i128 },
    RustFloat { kind: RustFloatKind, value: f64 },
    String(Rc<String>),
    Array(Rc<RefCell<Vec<PhpValue>>>),
    List(Rc<RefCell<Vec<PhpValue>>>),
    Dict(Rc<RefCell<Vec<(String, PhpValue)>>>),
    Set(Rc<RefCell<Vec<PhpValue>>>),
    HashMap(Rc<RefCell<Vec<(PhpValue, PhpValue)>>>),
    Object(Rc<RefCell<VmObject>>),
    Closure(Rc<VmClosure>),
    Future(Rc<RefCell<VmFuture>>),
    Resource(Arc<dyn std::any::Any + Send + Sync>),
}
```

* **Exact Numeric Semantics:** Primitives (`int` as `i32`/`i64`, `float` as `f64`) and native Rust integers (`i8`..`u128`) and floats (`f32`, `f64`) retain full precision.
* **Copy-On-Write (COW):** Assigning arrays or strings (`$b = $a;`) shares underlying buffers via `Rc`. Mutations detach transparently (`cow_clone()`) only when the buffer is shared.
* **Reference Handles:** Objects (`VmObject`), closures (`VmClosure`), and futures (`VmFuture`) pass by reference handles (`Rc<RefCell<...>>`).

### VM Dispatch Loop & Execution Flow (`src/vm/engine.rs`)

The core execution loop in `VM::dispatch_loop()` implements a tight fetch-decode-execute loop with 45 flat `OpCode` variants:

```rust
loop {
    let frame_idx = self.frames.len() - 1;
    let ip = self.frames[frame_idx].ip;
    let base_reg = self.frames[frame_idx].base_reg;
    let op = self.chunk_for_func(func_id).instructions[ip];
    self.frames[frame_idx].ip = ip + 1;

    match op {
        OpCode::LoadInt { dst, value } => {
            self.registers[base_reg + dst.index()] = PhpValue::Int(value as i64);
        }
        OpCode::Binary { dst, op, lhs, rhs } => {
            let left = &self.registers[base_reg + lhs.index()];
            let right = &self.registers[base_reg + rhs.index()];
            self.registers[base_reg + dst.index()] = self.exec_binary(op, left, right, span)?;
        }
        OpCode::AssertType { reg, type_const } => {
            let actual = &self.registers[base_reg + reg.index()];
            if !actual.matches_type_contract(&expected_type, self.type_hierarchy.as_ref()) {
                return Err(self.make_error(RuntimeErrorKind::TypeContractViolation { ... }, span));
            }
        }
        OpCode::Return { value } => {
            let ret = value.map(|r| self.registers[base_reg + r.index()].clone()).unwrap_or(PhpValue::Null);
            let popped = self.frames.pop().unwrap();
            if popped.base_reg > 0 { self.registers.truncate(popped.base_reg); }
            if self.frames.len() <= stop_depth { return Ok(ret); }
            if let Some(caller_dst) = popped.caller_dst { self.registers[caller_dst] = ret; }
        }
        // ... LoadConst, Unary, Jump, CallFunction, MethodCall, etc.
    }
}
```

### Zero-Cost Exception Unwinding Table

Instead of running slow stack unwinders or runtime setup on every `try` block:
- Protected blocks declare `ExceptionHandler { try_start, try_end, catches, finally_target }` in the chunk metadata.
- When `OpCode::Throw` or `OpCode::ThrowMatchError` is executed, the VM inspects active `handler_id` entries, compares nominal exception classes (`is_class_instance_of()`), binds the exception instance to the catch register, and jumps directly to `catch.target_ip`.
- If unhandled in the current frame, it unwinds caller frames up to `stop_depth`.

### Post-Response Defer Execution Queue

Hooks registered via `defer { ... }` or `Future::defer(...)` lower to `OpCode::Defer { func_id }`. The VM queues them and runs them strictly in **LIFO (Last-In, First-Out)** order after `<main>` completes, before request arenas are dropped.

### CPU Preemption & Instruction Budgeting

To prevent tight compute loops from monopolizing cooperative worker event loops:
* **Production Budget (10,000 Ticks):** At every loop back-edge (`target <= ip`) and function prologue (`ip == 0`), `vm.tick_counter` decrements. When zero, the VM yields cooperatively via `trigger_cooperative_yield()`, updating fiber statistics and relinquishing the OS timeslice without aborting valid long compute jobs.
* **Cranelift JIT Preemption:** Compiled machine-code loops track an unboxed 64-bit tick counter in native registers, branching to an out-of-line yield trampoline that restores registers and yields to the VM.
* **Dev Mode Loop Watchdog:** Warns at 1,000 iterations and enforces a hard ceiling at 100,000 iterations to catch infinite loops early.

> See the dedicated deep dive: [CPU Preemption & Instruction Budgeting](/docs/advanced/cpu-preemption).

---

## 6. Dual-Engine Execution Modes (`RunMode`)

RHP provides two complementary execution modes via the `RunMode` API:

```rust
pub enum RunMode {
    Dev { hot_reload: bool, dap_port: Option<u16> },
    Build { jit_enabled: bool, opt_level: u8 },
}

pub fn execute(mode: RunMode, ast: &ASTNode, env: &mut Environment) -> Result<Value, RuntimeError> {
    match mode {
        RunMode::Dev { .. } => eval_expr(ast, env),
        RunMode::Build { jit_enabled, .. } => {
            let bytecode = compile_to_bytecode(ast)?;
            VM::new(bytecode, jit_enabled).run()
        }
    }
}
```

| Dimension | Development Mode (`rhp run dev`) | Production Mode (`rhp run build`) |
| :--- | :--- | :--- |
| **Engine Pipeline** | Recursive AST Tree-Walk (`eval_expr`) | Register Bytecode VM (`VM::run`) + Cranelift JIT |
| **Startup Overhead** | Near-zero cold start (<1ms) | Pre-compilation & bytecode lowering pass |
| **Execution Speed** | Optimized for rapid developer iteration | Optimized for peak operations/sec & CPU cache |
| **Developer Tools** | Exact source maps, DAP server, LCOV coverage | Low-overhead telemetry, zero-cost exception tables |

---

## 7. Class Table, Flattened VTables & Method Resolution (`src/ir/class_table.rs`)

The compiler maintains a link-time global `ClassTable` holding descriptors (`ClassInfo`) for all declared and imported classes, abstract classes, interfaces, and traits.

### 7.1 Single Inheritance & Trait Precedence Linker
At link time, classes are resolved in topological order (`compute_topological_order()`). RHP enforces strict PHP-compliant method resolution order:
$$\text{Class's Own Methods} > \text{Trait Methods} > \text{Parent Methods}$$

1. **Ancestor Set Computation:** Each class computes a transitive `ancestors: HashSet<ClassId>` set representing its complete inheritance chain, implemented interfaces, and nested traits.
2. **Dense Slot Table Generation:** Rather than relying on slow string lookups at runtime, methods in flattened vtables are indexed into dense slot arrays (`method_slots: Vec<MethodId>` and `method_indices: HashMap<Arc<str>, usize>`).
3. **Magic Slot Caching:** Dedicated slots are cached directly on `ClassInfo`:
   * `call_slot: Option<MethodId>` for dynamic instance method invocation (`__call`).
   * `call_static_slot: Option<MethodId>` for dynamic static method invocation (`__callStatic`).
   * `constructor_slot: Option<MethodId>` for fast zero-overhead instantiation (`__construct`).

### 7.2 Whole-Program Class Hierarchy Analysis (CHA) & VM Table Reduction

Traditional PHP runtimes dispatch method calls by looking up function entry hash tables on every invocation. RHP eliminates this overhead by performing whole-program Class Hierarchy Analysis via `ClassTable::is_method_overridden()`:

1. **`final` Class & Method Devirtualization:**
   - Any method declared `final` or inside a `final class` can never be overridden.
   - The IR lowerer devirtualizes the call into direct static execution (`OpCode::MethodCallDirect`), pointing directly to the target method's compiled index without touching the object vtable.

2. **Open Classes Devirtualized via Whole-Program CHA:**
   - Classes are open and extensible by default (standard PHP).
   - If no derived subclass in the compiled program unit overrides a given method, `is_method_overridden()` returns `false`, devirtualizing the call directly to `MethodCallDirect`.
   - Vtable indirection ($O(1)$ dense slot array) is reserved strictly for polymorphic call-sites where active subclass overrides exist.

3. **Compile-Time Contract Enforcement via `#[\Override]`:**
   - Methods annotated with `#[\Override]` or `#[Override]` are validated during semantic type checking against ancestor classes and implemented interfaces.
   - If no matching parent method exists, compilation fails immediately, preventing subtle inheritance drift without runtime reflection cost.

4. **Trait Precedence & Explicit Conflict Resolution:**
   - Trait collisions must be explicitly adapted using standard PHP `insteadof` or `as`, or resolved by declaring the method on the class. Unresolved collisions fail at compile time (`TraitMethodCollision`).

5. **100% PHP Syntax & Identifier Compatibility:**
   - Non-standard keywords are avoided: words like `open`, `sealed`, and `override` remain valid identifiers in user code.

### 7.3 Escape Analysis for `::class` and DI Containers
When `ClassName::class` is referenced in user code:
* The compiler lowers it as a compile-time string literal constant (`OpCode::LoadConst`) with zero runtime reflection overhead.
* The referenced class is flagged as **escaped** (`is_escaped = true`), instructing Dead Code Elimination (DCE) to preserve its public methods and constructor descriptors for runtime IoC dependency injection (`app()`).

---

## 8. Pure Closure Optimization & Cranelift Compilation Tier

Closures (`function() use (...) { ... }`) and arrow functions (`fn(...) => ...`) are subject to compile-time purity analysis, `$this` elision, and inline memory allocation:

1. **Closure Categorization ([`ClosureCategory`](/docs/advanced/pure-closure-optimization)):** The compiler inspects captured upvalues, reference flags (`u.by_ref`), and `$this` usage to classify closures into 6 optimization tiers:
   - `PureNonCapturing`: 0 upvalues, no `$this`. Lowers to raw function pointer `(fn_ptr, null)` with **zero heap allocations**.
   - `PureThisOnly`: Captures only `$this` (single pointer in a 16-byte inline struct).
   - `PureValueSmall(u8)`: Captures 1 or 2 variables by immutable copy. Fits in 16–32B inline storage with **zero heap allocations**.
   - `PureValueLarge(u16)`: Captures 3+ variables by immutable value-copy.
   - `PureValueAndThis(u16)`: Captures `$this` and outer variables by immutable value-copy.
   - `ImpureReference(u16)`: Captures variables by reference (`use (&$var)`), strictly rejected by the type checker (`ClosureReferenceCaptureForbidden`) to prevent reference cycles.

2. **AST `$this` Elision (`closure_body_uses_this`):** If a closure defined inside a class method does not access `$this`, the compiler elides `$this` from its upvalue table, upgrading the closure to `PureNonCapturing` or `PureValueSmall`.

3. **Cranelift Machine-Code Function Pointers:** In production mode (`RunMode::Build`), hot `PureNonCapturing` closures compile into raw machine-code function pointers:
   $$\text{Closure} \equiv \left(\text{fn\_ptr: } \text{unsafe extern "C" fn}(*\text{mut NativeSlot}) \to \text{u32}, \; \text{env: } \text{null}\right)$$

For complete architectural details, empirical codebase audits, and benchmarks, see the dedicated [Pure Closure Optimization Guide](/docs/advanced/pure-closure-optimization).



