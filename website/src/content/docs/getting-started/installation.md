---
title: "Installation"
description: "How to install the RHP programming language runtime, compiler, and CLI toolchain."
section: "Getting Started"
order: 1
---

## Overview

RHP is distributed as a single standalone statically linked binary (`rhp`). The binary includes the complete development environment:

- **Logos DFA Lexer & Pratt Parser**
- **Tree-Walking AST Dev Interpreter** (instant sub-millisecond execution)
- **Register Bytecode Virtual Machine**
- **Cranelift Just-In-Time (JIT) Compiler**
- **AST Auto-Formatter & Static Type Checker** (`rhp run lint`)

---

## One-Line Automated Installer

The recommended method to install RHP on Linux and macOS is via the official curl installer:

```bash
curl -fsSL https://rhp.dev/install.sh | bash
```

The script automatically detects your platform architecture (`x86_64` or `aarch64`), downloads the pre-built binary, installs it to `~/.rhp/bin`, and adds it to your shell configuration (`~/.bashrc`, `~/.zshrc`, or `~/.config/fish/config.fish`).

---

## Installing via Cargo (Rust Toolchain)

If you have Rust 2024 installed locally, you can build and install RHP directly from source:

```bash
cargo install --git https://github.com/vuenice/rhp.git rhp
```

To build locally from a cloned repository:

```bash
git clone https://github.com/vuenice/rhp.git
cd rhp
cargo build --release
sudo cp target/release/rhp /usr/local/bin/
```

---

## Platform Support Matrix

| OS / Architecture | Support Tier | Engine Status |
| :--- | :--- | :--- |
| **Linux (x86_64)** | Tier 1 (Official) | Bytecode VM + Cranelift JIT + AST Dev |
| **Linux (aarch64 / ARM64)** | Tier 1 (Official) | Bytecode VM + Cranelift JIT + AST Dev |
| **macOS Apple Silicon (M1/M2/M3/M4)** | Tier 1 (Official) | Bytecode VM + Cranelift JIT + AST Dev |
| **macOS Intel (x86_64)** | Tier 2 (Supported) | Bytecode VM + Cranelift JIT + AST Dev |
| **Windows 11 / WSL2** | Tier 1 (WSL2 Linux) | Fully supported under Ubuntu / Debian WSL |
| **Windows Native (MSVC)** | Tier 3 (Alpha) | `TODO: Native Windows IOCP async runtime integration` |

---

## Verifying Your Installation

Confirm that the CLI is properly installed and accessible in your `$PATH`:

```bash
rhp --version
```

Output:
```text
rhp 0.1.0
```

Display CLI usage options:

```bash
rhp --help
```
