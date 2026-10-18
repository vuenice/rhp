---
title: "Contributing Guide"
description: "How to build RHP from source, execute the comprehensive test suite, and submit pull requests."
section: "Community"
order: 1
---

## Welcome Contributors!

We welcome contributions across all areas of the RHP ecosystem:
- **Core Engine:** Compiler, Pratt parser, register VM, and JIT optimizations in Rust.
- **Standard Library:** Native facades, collections, and HTTP abstractions.
- **Developer Tooling:** LSP, Tree-sitter parsers, and editor extensions.
- **Documentation:** Guides, tutorials, and benchmark replication scripts.

---

## Setting Up Your Development Environment

### Prerequisites:
- **Rust Toolchain:** Rust 2024 edition (1.85+ recommended via `rustup`)
- **Git**
- **LLVM / Cranelift native dependencies**

### Clone and Build:

```bash
git clone https://github.com/vuenice/rhp.git
cd rhp

# Build in debug mode
cargo build

# Build optimized binary
cargo build --release
```

---

## Running the Integration Test Suite

RHP contains an extensive test suite verifying lexer tokens, Pratt expression precedence, collection subtypes, Tokio fibers, and zero-GC memory safety:

```bash
# Run all integration tests
cargo test

# Run a specific test suite (e.g. subtyped collections)
cargo test --test subtyped_list_integration_tests
cargo test --test future_concurrency_core_tests
cargo test --test memory_ownership_and_safety_tests
```

---

## Pull Request Guidelines

1. **Keep Commits Atomic:** Each commit should address a single logical change with an informative message.
2. **Add Tests:** Any parser modification or new facade method must include integration tests in `tests/`.
3. **Format Code:** Run `cargo fmt` and `cargo clippy --all-targets` before pushing.
4. **License:** All contributions are submitted under the project's MIT License.
