---
title: "Changelog"
description: "Release history, version notes, and notable changes across RHP releases."
section: "Community"
order: 2
---

## Unreleased

- Continuous optimizations and performance benchmarks.

## v0.2.0-alpha.1 (November 2026)

The v0.2.0-alpha.1 release delivers major performance, runtime, and ecosystem additions:

### Highlights:
- **Rune Type & Text/Byte Separation:**
  - First-class `rune` scalar type representing a 32-bit Unicode code point with `r'...'` literal syntax.
  - Native `Rune` facade with static methods (`Rune::from()`, `Rune::isAlpha()`, `Rune::isDigit()`, `Rune::toLower()`, `Rune::toUpper()`, `Rune::width()`).
  - Strict text vs byte separation with fluent `byte` string helpers and encoding conversions (UTF-8, Shift-JIS, EUC-JP, ISO-2022-JP).
- **Process Supervision & RHPM (PM2-Style):**
  - Enterprise-grade 3-tier runtime supervisor with worker process pooling, hot reloading, and cluster management.
  - CLI management commands: `rhp start`, `rhp stop`, `rhp restart`, `rhp status` (`rhp ps`), and `rhp logs`.
  - Corosensei stackful fibers and Tokio multi-worker thread coordination.
- **Package Manager & Scaffolding:**
  - Integrated package manager powered by `rhp.toml` manifests and lockfile verification.
  - Commands for `rhp add`, `rhp remove`, `rhp install`, `rhp update`, `rhp audit`, and `rhp init`.
  - Built-in security audits for external egress and safe/unsafe capability tracking.
- **Automated PHP Migration Toolchain:**
  - `rhp migrate` command to convert legacy PHP 8.x codebases to idiomatic RHP.
  - Rewrites strict equality (`===`, `!==` to `==`, `!=`), strips PHP opening tags and `declare` statements, reorders parameters (required before optional), and converts cURL to async HTTP calls.
- **Extended Standard Library & Facades:**
  - `Time`: Date and time handling facade.
  - `Crypt`, `Html`, and `Url`: Cryptographic hashing (AES-256, HMAC, MD5, SHA), HTML sanitization, and URL parsing helpers.
  - `Duration`: Ergonomic time unit math and conversions.
  - `Uniq`: UUID v4, UUID v7, and NanoID generation.
- **Observability & OpenTelemetry:**
  - Distributed tracing and metrics with standard OTLP JSON export over HTTP/gRPC.
  - Native span creation, baggages, and automatic HTTP request context propagation.

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
