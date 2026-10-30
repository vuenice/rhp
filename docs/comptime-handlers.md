# Milestone 2 Design Specification: RHP Comptime Handlers

## 1. Overview & Architectural Goal

In Milestone 1, the expansion engine established the internal `Expander` trait with built-in Rust handlers (`#[fillable]`, `#[guarded]`, `#[table]`, `@if`, `@for`, `@foreach`, `@datetime`).

The fundamental requirement of Milestone 2 is that **extensibility targets RHP, not Rust**. PHP developers expect `composer require package` to install packages containing compile-time macros without requiring a local Rust toolchain, C compiler, or native compilation steps.

Third-party attribute and directive handlers are declared in standard RHP source code, compiled into RHP bytecode, and executed inside a temporary sandboxed instance of the `VM` (`src/vm/engine.rs`) during the expansion phase.

---

## 2. Handler Declaration in RHP

Macro handlers can be declared either via dedicated attributes or through a fluent compiler registration API in package service providers.

### 2.1 Attribute-Based Declaration
```php
namespace App\Macros;

use RHP\Compiler\AttributeHandler;
use RHP\Compiler\DirectiveHandler;
use RHP\Compiler\ExpansionSite;
use RHP\Compiler\Expansion;
use RHP\Compiler\Ast;

class OrmMacros {
    #[AttributeHandler('slug')]
    public static function slugify(ExpansionSite $site): Expansion {
        // $site->class, $site->target, $site->args
        $field = $site->args[0] ?? 'slug';
        
        $method = Ast::method('getSlug')
            ->returns('string')
            ->body([
                Ast::return(Ast::staticCall('Str', 'slug', [Ast::var('this')->prop($field)]))
            ]);

        return Expansion::addMember($method);
    }

    #[DirectiveHandler('markdown')]
    public static function renderMarkdown(ExpansionSite $site): Expansion {
        $rawContent = $site->rawBody();
        $renderedHtml = Ast::staticCall('Markdown', 'parse', [Ast::string($rawContent)]);
        return Expansion::statement(Ast::echo([$renderedHtml]));
    }
}
```

### 2.2 Ingestion via ExpansionSite
The RHP value passed into the macro handler is an instance of `RHP\Compiler\ExpansionSite` backed by an internal bridge object:
- `$site->name`: String identifier of the attribute/directive.
- `$site->targetKind`: String: `'class'`, `'method'`, `'property'`, or `'directive'`.
- `$site->args`: Array of evaluated literal arguments.
- `$site->span`: Opaque span object preserving source provenance.
- `$site->target`: AST descriptor of the decorated element.

---

## 3. AST Exposure & Quasi-Quoting API

Handlers must never concatenate raw PHP code strings (`Ast::raw("echo ...;")`). String parsing loses span chains, strips hygiene markers, and risks compile-time injection bugs.

### 3.1 Structured Builder & Quasi-Quote
RHP exposes a structured AST construction library:
```php
// 1. Fluent Builder
$prop = Ast::property('slug')
    ->visibility('protected')
    ->type('string')
    ->default(Ast::string(''));

// 2. Quasi-Quote DSL with Hygiene Interpolation
$varName = $site->ctx->gensym('counter');
$ast = Ast::quote {
    int $$varName = 0;
    while ($$varName < 10) {
        $$varName++;
    }
};
```
Every node instantiated through `Ast::*` automatically inherits the origin span and macro provenance of the executing `$site`.

---

## 4. Sandboxed Execution in `src/vm/engine.rs`

### 4.1 Verification Report of Existing Codebase
Inspection of `src/vm/engine.rs` reveals:
1. **Instruction Metering:** `src/vm/engine.rs` **currently does not support instruction metering or fuel counting**. The fetch-decode-execute loop runs indefinitely until completion, runtime error, or stack overflow (`DEFAULT_MAX_CALL_DEPTH = 512`).
2. **I/O Capabilities:** `src/vm/engine.rs` **currently has no capability gating**. Native facades (`File::*`, `DB::*`, `Log::*`, `HTTP::*`) execute directly against host OS system calls with no permission checks.

### 4.2 Exact Engine Changes Required for Comptime Sandboxing
To safely execute untrusted macro code at compile time without risking infinite build loops or supply-chain filesystem tampering, the following additions must be made to `src/vm/engine.rs`:

1. **Fuel Counter in `VM`:**
   ```rust
   pub struct VM {
       // ... existing fields ...
       pub fuel: Option<u64>,
       pub fuel_consumed: u64,
       pub capabilities: VmCapabilities,
   }
   ```
2. **Fuel Decrement in Instruction Dispatch:**
   In `VM::run()` / `VM::step()`, decrement `fuel` on every opcode:
   ```rust
   if let Some(ref mut fuel) = self.fuel {
       if *fuel == 0 {
           return Err(RuntimeError::new(
               RuntimeErrorKind::FuelExhausted { consumed: self.fuel_consumed },
               frame.current_span(),
           ));
       }
       *fuel -= 1;
       self.fuel_consumed += 1;
   }
   ```
3. **Capability Bitflags (`VmCapabilities`):**
   ```rust
   bitflags::bitflags! {
       pub struct VmCapabilities: u32 {
           const PURE_COMPUTE   = 0;
           const FILE_SYSTEM    = 1 << 0;
           const NETWORK        = 1 << 1;
           const PROCESS_SPAWN  = 1 << 2;
           const DATABASE       = 1 << 3;
       }
   }
   ```
4. **Facade Gating in `src/vm/facade.rs`:**
   Gate OS calls: if `!vm.capabilities.contains(VmCapabilities::FILE_SYSTEM)`, any invocation of `File::read()`, `File::write()`, or `unlink()` immediately throws `SecurityException: Comptime macro attempted unauthorized file system access`.

---

## 5. Dependency, Discovery & Bootstrapping Order

Macros defined in vendor packages must be compiled into bytecode *before* any application file using those macros can be expanded.

```
┌────────────────────────────────────────────────────────┐
│ Phase 0: Discovery                                     │
│ Parse rhp.toml [expand.plugins], vendor manifests      │
└───────────────────────────┬────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────┐
│ Phase 1: Macro Compilation                             │
│ Compile macro files -> IR Bytecode (no macros needed)  │
└───────────────────────────┬────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────┐
│ Phase 2: Macro Registration                            │
│ Instantiate VM sandbox, register handlers into registry│
└───────────────────────────┬────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────┐
│ Phase 3: Application Expansion                         │
│ Expand application code AST using registered handlers   │
└────────────────────────────────────────────────────────┘
```

### 5.1 Cycle Detection & Rejection
Macro dependency graphs must be strictly Directed Acyclic Graphs (DAGs).
- If `PackageA/macros.rhp` requires `PackageB/macros.rhp`, and `PackageB` uses an attribute provided by `PackageA`, a cyclic dependency exists.
- The dependency graph is sorted using Kahn's topological sort algorithm. Cycles are caught at discovery time and reported as `RHP0508: Cyclic macro package dependency detected`.

---

## 6. The Dual-Engine Semantic Risk (VM vs Cranelift)

RHP features two execution environments:
1. **VM Bytecode Interpreter:** Runs comptime macros, dev mode scripts, and fallback loops.
2. **Cranelift JIT/AOT Backend:** Compiles hot execution code to native machine code in production builds.

### 6.1 Semantic Divergence Risks
1. **Integer Overflow & Sizing:** Native 64-bit integer wrapping in Cranelift vs boxed `PhpValue::Int` in VM.
2. **Floating-Point NaN Payloads:** Differing bit patterns across Cranelift SIMD registers vs host IEEE-754 numbers in the VM.
3. **Memory & Concurrency Primitives:** Fibers, async `Future::run`, and Tokio worker pools are unsupported inside single-threaded macro expansion.

### 6.2 Mitigation Strategy: Restricted Comptime Subset
RHP enforces a strict **Comptime Pure Subset** during macro execution:
- Async primitives (`Future::run`, `Future::wait`, `defer`) are forbidden at comptime.
- Raw FFI and native pointer calls are forbidden.
- Arithmetic strictly adheres to language boundary contracts.
