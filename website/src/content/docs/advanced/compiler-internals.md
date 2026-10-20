---
title: "Compiler Internals"
description: "Deep dive into the compilation pipeline: Logos lexer, Pratt parser, register VM bytecode, and Cranelift JIT."
section: "Advanced"
order: 3
---

## Compilation Pipeline Architecture

```text
Source Code (.rhp)
       │
       ▼
 [Logos Lexer]        Zero-copy DFA tokenization
       │
       ▼
 [Pratt Parser]       AST generation & symbol lowering
       │
 ┌─────┴────────────────────────┐
 │                              │
 ▼                              ▼
[Dev Mode: AST Tree-Walk]     [Prod Mode: IR Bytecode VM]
- Sub-ms cold start           - Flat linear opcodes
- Precise source spans        - Multi-worker Tokio pool
- DAP debugging traps         - Cranelift JIT compilation
```

---

## 1. Zero-Copy Lexing (`logos`)

The lexer leverages the `logos` Rust crate to produce deterministic finite automaton (DFA) state machines, tokenizing source code without heap string allocations:

```rust
#[derive(Logos, Debug, PartialEq, Clone)]
pub enum Token {
    #[token("fn")]
    Fn,
    #[token("function")]
    Function,
    #[token("singleton")]
    Singleton,
    #[regex(r"\$[a-zA-Z_][a-zA-Z0-9_]*")]
    Variable,
}
```

---

## 2. Pratt Expression Parser & Combinators

Operator precedence is resolved using a custom Pratt parser. The parser automatically rewrites symbols, intrinsics, and global helper calls directly in the AST:
- System intrinsics (`print()`, `env()`, `dd()`) $\rightarrow$ `Sys::*`
- Global PHP helper functions $\rightarrow$ `Glob::helperFunctions` (such as `Glob::customHelper()`)

Exact source spans (`Span { file_id, start, end, line, col }`) are preserved for zero-overhead diagnostics.

---

## 3. Register Bytecode VM

Production mode lowers the typed AST into flat, compact register bytecode instructions (`src/ir/opcode.rs`). Unlike stack-based virtual machines (like the JVM or Python), register VMs require far fewer instructions per operation, minimizing CPU branch mispredictions.

---

## 4. Cranelift JIT Engine

Hot loops and long-running services are detected by VM execution counters and compiled into native machine code at runtime using Cranelift (`cranelift-jit`). This delivers near-C execution performance for numerical computations and intense collection iterations.
