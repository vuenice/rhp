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

### `rhp run dev [PATH] [OPTIONS]` (or `rhp dev [PATH]`)

Executes the target script, project, or directory using the direct tree-walking AST interpreter with instant sub-millisecond cold start. Automatically defaults to `index.rhp` (or `app.rhp`) in the current working directory (`pwd`) if no path is provided.

#### Automatic Web Application Detection

When the target script is a web application (i.e. it uses superglobals `$_SERVER`, `$_GET`, `$_POST`, `HTTP_HX_REQUEST` or renders HTML), `rhp run dev` **automatically launches the interactive HTTP web server** on port **8080** instead of terminating after a single CLI execution:

```text
=======================================================================
🚀 RHP Development Server (Dev Mode)
📡 Listening on: http://127.0.0.1:8080
📂 Serving application: examples/todo/index.rhp
⚡ Reactive UI: HTMX + Alpine.js
⌨️  Press Ctrl+C to terminate the server.
=======================================================================
```

#### Usage Examples

```bash
# Start dev server for a web project (e.g., TODO app)
rhp run dev examples/todo

# Shorthand syntax
rhp dev examples/todo

# Specify custom port and host
rhp run dev examples/todo --port 3000 --host 0.0.0.0
# or short flag:
rhp dev examples/todo -p 3000

# Inside a project directory (auto-detects index.rhp / app.rhp)
cd examples/todo
rhp run dev

# Force one-shot CLI execution (e.g., dump rendered HTML to stdout)
rhp run dev examples/todo --cli

# Run a standalone CLI script (executes once and outputs to stdout)
rhp run dev scripts/benchmark.rhp
```

#### Options for `run dev` & `run build`:

| Option | Shorthand | Description | Default |
| :--- | :--- | :--- | :--- |
| `--port <PORT>` | `-p <PORT>` | Port to bind when running as an HTTP server | `8080` |
| `--host <HOST>` | | Network interface to bind | `127.0.0.1` |
| `--cli` | | Force one-shot CLI execution even if script is a web app | `false` |
| `--server` | | Force HTTP web server mode | `false` |
| `--file <PATH>` | `-f <PATH>` | Explicit script file to execute | `index.rhp` |

---

### `rhp run build [PATH] [OPTIONS]` (or `rhp build [PATH]`)

Compiles source code into linear register-based bytecode IR and executes it within the high-performance multi-worker Register VM with Cranelift JIT compilation enabled.

Like `run dev`, if the target is a web application, it automatically launches the production HTTP server with JIT acceleration:

```text
=======================================================================
🚀 RHP Production Server (Build Mode + JIT)
📡 Listening on: http://127.0.0.1:8080
📂 Serving application: examples/todo/index.rhp
⚡ Reactive UI: HTMX + Alpine.js
⌨️  Press Ctrl+C to terminate the server.
=======================================================================
```

```bash
# Run production web server with JIT
rhp run build examples/todo

# Shorthand syntax
rhp build examples/todo

# Production server on custom port
rhp run build examples/todo --port 8000

# Execute CLI script with JIT compilation
rhp run build scripts/compute_heavy.rhp
```

---

### `rhp serve [PATH] [OPTIONS]`

Dedicated command to start the built-in RHP HTTP server for web applications. Routes incoming HTTP requests into isolated zero-bootstrap worker VMs with standard superglobals (`$_SERVER`, `$_GET`, `$_POST`, `HTTP_HX_REQUEST`).

```bash
# Serve application on default port 8080
rhp serve examples/todo

# Serve on custom port
rhp serve examples/todo --port 8080 --host 0.0.0.0

# Automatically picks app.rhp or index.rhp in current directory
cd examples/todo
rhp serve
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
