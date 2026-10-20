---
title: "CLI Reference"
description: "Complete command-line interface documentation for the rhp binary."
section: "Getting Started"
order: 5
---

## Overview

The `rhp` binary is the central command-line interface for the RHP toolchain.

```bash
rhp [COMMAND] [PATH] [OPTIONS]
```

---

## Global Options

| Flag | Description |
| :--- | :--- |
| `-v`, `-V`, `--version` | Display the engine version and exit |
| `-h`, `--help` | Display the global CLI help dialog and command descriptions |

---

## Commands

### `rhp run dev [PATH] [OPTIONS]`

Executes the target script or directory using the direct tree-walking AST interpreter. Automatically defaults to `index.rhp` in the current working directory (`pwd`) if no file is provided.

- **Primary Use Case:** Daily local development, rapid feedback loops, and unit tests.
- **Cold Start Latency:** Less than 5 milliseconds.
- **Hot Reloading:** Watches project files and invalidates in-memory AST cache nodes without restarting server state.

```bash
# Execute index.rhp in current working directory (default)
rhp run dev

# Execute specified file via --file argument
rhp run dev --file="/your-path/server.rhp"
# or
rhp run dev --file="server.rhp"

# Execute single file via positional path
rhp run dev src/main.rhp
```

---

### `rhp run build [PATH] [OPTIONS]`

Lowers source code into linear register-based bytecode IR and executes it within the high-performance multi-worker Tokio VM with Cranelift JIT compilation enabled. Automatically defaults to `index.rhp` in the current working directory (`pwd`) if no file is provided.

- **Primary Use Case:** Production deployments, background workers, and performance benchmarks.
- **Optimizations:** JIT compiles hot loops directly to machine code (`opt_level: 2`).

```bash
# Execute index.rhp in current working directory (default)
rhp run build

# Execute specified file via --file argument
rhp run build --file="/your-path/server.rhp"
# or
rhp run build --file="server.rhp"

# Execute single file via positional path
rhp run build src/main.rhp
```

---

### `rhp run lint [PATH] [OPTIONS]`

Lints, auto-formats code, infers variable type signatures, and normalizes casing conventions across your codebase.

```bash
# Auto-format and write changes to disk
rhp run lint src/

# Dry run: check and report issues without writing files
rhp run lint src/ --check
# or
rhp run lint src/ --no-fix

# Skip changing identifier casing
rhp run lint src/ --no-case-change
```

#### Diagnostic Output Schema

When run with `--check`, errors and suggestions are emitted in machine-readable JSON matching the Carthage Mago diagnostics format:

```json
{
  "file": "src/Order.rhp",
  "issues": [
    {
      "code": "RHP0042",
      "severity": "Warning",
      "message": "Variable $total is missing explicit scalar type hint. Inferred: 'float'.",
      "line": 14,
      "col": 5
    }
  ]
}
```
