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

## Installing via Homebrew (Recommended for macOS & Linux)

The easiest way to install and manage RHP on macOS and Linux is using Homebrew:

```bash
brew install vuenice/rhp/rhp
```

Or tap the repository first and install:

```bash
brew tap vuenice/rhp
brew install rhp
```

Homebrew automatically fetches the appropriate pre-compiled standalone binary for your architecture (`aarch64` Apple Silicon, `x86_64` Intel Mac, or `x86_64` Linux) and links it directly into your `$PATH`.

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

# Option A: Install directly into ~/.cargo/bin (recommended)
cargo install --path .

# Option B: Build and symlink debug binary for instant live development
cargo build
ln -sf $(pwd)/target/debug/rhp ~/.cargo/bin/rhp

# Option C: Optimized release binary system-wide
cargo build --release
sudo cp target/release/rhp /usr/local/bin/
```

### Troubleshooting: `rhp: command not found in local`

If `rhp` is installed but running `rhp` in your shell returns `command not found`:

1. Ensure `~/.cargo/bin` is in your shell's `PATH`:
   ```bash
   echo $PATH | grep -q "$HOME/.cargo/bin" && echo "In PATH" || echo "Not in PATH"
   ```
2. If not present, append it to your shell configuration file (`~/.bashrc`, `~/.zshrc`, or `~/.profile`):
   ```bash
   export PATH="$HOME/.cargo/bin:$PATH"
   ```
3. Reload your shell:
   ```bash
   source ~/.bashrc  # or source ~/.zshrc
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
