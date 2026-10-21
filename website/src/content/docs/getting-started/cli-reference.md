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

#### Entrypoint File Resolution

When running `rhp run dev` or `rhp run build`:
1. **Default (`pwd`):** If no path or `--file` option is supplied, RHP automatically picks `index.rhp` in the current working directory (`pwd`).
2. **Explicit `--file` Argument:** If `--file` is supplied (e.g. `--file="/your-path/server.rhp"`), RHP executes that exact script.
3. **Positional Argument:** Alternatively, pass the path directly as a positional argument (e.g. `rhp run dev server.rhp`).

#### Usage Examples for `run dev`

```bash
# Automatically pick and execute index.rhp in current working directory
rhp run dev

# Explicitly specify file via --file argument
rhp run dev --file="/your-path/server.rhp"
# or relative path:
rhp run dev --file="server.rhp"

# Start dev server for a web project directory
rhp run dev examples/todo

# Shorthand syntax
rhp dev --file="/your-path/server.rhp"

# Specify custom port and host
rhp run dev examples/todo --port 3000 --host 0.0.0.0
# or short flag:
rhp dev examples/todo -p 3000

# Force one-shot CLI execution (e.g., dump rendered HTML to stdout)
rhp run dev examples/todo --cli

# Run a standalone CLI script
rhp run dev scripts/benchmark.rhp
```

#### Options for `run dev` & `run build`:

| Option | Shorthand | Description | Default |
| :--- | :--- | :--- | :--- |
| `--file <PATH>` | `-f <PATH>` | Explicit script file to execute (e.g. `--file="/your-path/server.rhp"`) | `index.rhp` in `pwd` |
| `--port <PORT>` | `-p <PORT>` | Port to bind when running as an HTTP server | `8080` |
| `--host <HOST>` | | Network interface to bind | `127.0.0.1` |
| `--cli` | | Force one-shot CLI execution even if script is a web app | `false` |
| `--server` | | Force HTTP web server mode | `false` |

---

### `rhp run build [PATH] [OPTIONS]` (or `rhp build [PATH]`)

Compiles source code into linear register-based bytecode IR and executes it within the high-performance multi-worker Register VM with Cranelift JIT compilation enabled.

Like `run dev`, it automatically defaults to `index.rhp` in `pwd` unless specified via `--file` or positional argument. If the target is a web application, it automatically launches the production HTTP server with JIT acceleration:

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
# Automatically pick and execute index.rhp in current working directory with JIT
rhp run build

# Explicitly specify file via --file argument
rhp run build --file="/your-path/server.rhp"
# or relative path:
rhp run build --file="server.rhp"

# Run production web server with JIT
rhp run build examples/todo

# Shorthand syntax
rhp build --file="/your-path/server.rhp"

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

Lints, auto-formats code according to PER-CS standards, infers variable, parameter, and property type signatures, normalizes casing conventions, and outputs diagnostics in machine-readable JSON inspired by [Carthage Software Mago](https://github.com/carthage-software/mago).

```bash
# Auto-format and write changes to disk
rhp run lint src/

# Dry run: check and report issues in JSON without writing files
rhp run lint src/ --check
# or
rhp run lint src/ --no-fix

# Auto-format and typecheck without changing identifier casing
rhp run lint src/ --no-case-change
```

#### Options for `run lint`:

| Option | Description | Default |
| :--- | :--- | :--- |
| `--check` | Dry-run check mode; emits diagnostics in JSON without modifying files | `false` |
| `--no-fix` | Alias for `--check`; leaves files on disk unmodified | `false` |
| `--fix` | Auto-formats code and writes inferred types and casing fixes to disk | `true` |
| `--no-case-change` | Skips identifier casing transformations (`snake_case`, `PascalCase`, `camelCase`) | `false` |

#### Key Capabilities:

1. **AST Code Formatting:** Enforces 4-space indentation, operator spacing, and normalized brace positioning while preserving comments and string literals.
2. **Automated Type Inference:**
   - Inactive untyped scalars: `$x = '';` $\rightarrow$ `string $x = '';`, `$y = 1;` $\rightarrow$ `int $y = 1;`.
   - Initialized array classification: sequential $\rightarrow$ `list`, string-keyed $\rightarrow$ `dict`, integer/complex-keyed $\rightarrow$ `hashMap`, empty $\rightarrow$ `array`.
   - Function parameter defaults and class property defaults.
   - Preserves user-declared types; emits typecheck errors if values mismatch.
3. **Casing Normalization:**
   - Variables $\rightarrow$ `snake_case` (preserving `$this` and PHP superglobals).
   - Classes, interfaces, traits, and enums $\rightarrow$ `PascalCase` (and all references).
   - Functions and methods $\rightarrow$ `camelCase` (preserving magic methods).
   - **Test function exception:** any function starting with `test` is normalized to `snake_case`.

#### Diagnostic Output Schema

When run with `--check` or `--no-fix`, diagnostics are emitted in Carthage Mago-compatible JSON:

```json
{
  "status": "ok",
  "files_scanned": 8,
  "files_modified": 0,
  "total_issues": 1,
  "errors": 0,
  "warnings": 1,
  "issues": [
    {
      "level": "warning",
      "code": "casing/variable",
      "message": "Variable '$userAccount' should be snake_case: '$user_account'",
      "file": "src/Order.rhp",
      "line": 14,
      "column": 5,
      "span": {
        "start": 210,
        "end": 222
      }
    }
  ]
}
```

