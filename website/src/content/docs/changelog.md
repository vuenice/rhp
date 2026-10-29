---
title: "Changelog"
description: "Release history, version notes, and notable changes across RHP releases."
section: "Community"
order: 2
---

## v0.1.0-alpha (October 2026) - Initial Release

Welcome to the initial public release of RHP!

### Highlights:
- **Core Engine:**
  - Logos zero-copy DFA lexer with full support for PHP identifiers, superglobals, and keywords.
  - Custom Pratt expression parser supporting full operator precedence and symbol rewriting.
  - Development Mode (`rhp run dev`) featuring a direct tree-walking AST interpreter for sub-millisecond cold starts and hot reloading.
  - Production Mode (`rhp run build`) lowering AST into linear IR opcodes with Cranelift JIT execution on a multi-worker Tokio pool.
- **Native Facade Standard Library:**
  - `Arr`: Explicit subtyped collections (`list`, `dict`, `set`, `hashMap`) backed by Rust `Vec`, `IndexMap`, and `HashSet`.
  - `Str`: Binary-safe raw byte storage with 23-byte small string optimization (SSO) and UTF-8 safety checks.
  - `Future`: Tokio fiber concurrency with `Future::run`, `Future::wait`, `Future::any`, and post-response `Future::defer`.
  - `Memory`: 100% GC-free deterministic destruction, bump request arena, and cycle prevention.
  - `Sys`: System intrinsics (`env`, `print`, `dd`, `version`).
- **Tooling:**
  - Integrated `rhp run lint` combining AST auto-formatting (4-space indent), casing normalization, automated variable type inference, and JSON diagnostic reporting.
- **Object Iteration & Laravel Compatibility:**
  - First-class support for iterating objects via `foreach ($obj as $v)` and `foreach ($obj as $k => $v)`.
  - Seamless execution of `IteratorAggregate` and step-by-step `Iterator` with re-entrant VM call frames.
  - Zero-cost vector unwrapping optimization for `ArrayIterator` wrapping native `list` or `dict`.
  - Plain object visible property iteration.
  - Native support for Laravel query results (`DBQuery::get()`), paginators (`LengthAwarePaginator`, `DBQuery::paginate()`), collections (`Collection`, `collect()`), and `iterable` / `Traversable` type contracts.
- **Documentation & Website:**
  - Modern marketing landing page and comprehensive documentation site built with Astro and Tailwind CSS.
