---
title: "CLI Reference"
description: "Complete command-line interface documentation for the rhp binary."
section: "Getting Started"
order: 4
---

## Overview

The `rhp` binary is the unified, high-performance command-line interface for the RHP toolchain. All commands follow a consistent and predictable structure:

```bash
rhp [COMMAND] [OPTIONS]
```

Target paths and script files are passed as options (such as `--file="server.rhp"` or optional positional arguments).

---

## Global Options

| Flag | Description |
| :--- | :--- |
| `-v`, `-V`, `--version` | Display the engine version and exit |
| `-h`, `--help` | Display the global CLI help dialog and command descriptions |

---

## Runtime & Server Commands

### `rhp dev [OPTIONS]`

Executes in Development Mode using the direct AST tree-walking interpreter with instant sub-millisecond cold start and live hot-reloading.

```bash
# Execute with an explicit file (file name if present in current directory, or file path)
rhp dev --file="server.rhp"
rhp dev --file="path/to/server.rhp"

# Positional third parameter is also supported as a shorthand:
rhp dev server.rhp

# If the third parameter (or --file option) is not provided, defaults to index.rhp in current directory:
rhp dev
```

> [!NOTE]
> **Default Resolution:** If the third parameter in the `dev` or `build` command (or the `--file` option) is not provided, RHP will automatically use `index.rhp` at the current working directory (`pwd`).

#### Automatic Web Application Detection

When the target script is a web application (i.e. it uses superglobals `$_SERVER`, `$_GET`, `$_POST`, `HTTP_HX_REQUEST` or renders HTML), `rhp dev` automatically starts the interactive HTTP web server on port **8080** instead of terminating after a single CLI run:

```bash
# Automatically launches the web server on port 8080
rhp dev --file="server.rhp"

# Run on a custom port and host
rhp dev --file="server.rhp" --port 3000 --host 0.0.0.0
# Or shorthand port flag:
rhp dev --file="server.rhp" -p 3000

# Force one-shot CLI execution (e.g., dump rendered HTML to stdout without launching server)
rhp dev --file="server.rhp" --cli
```

#### Options for `dev` and `build`:

| Option | Shorthand | Description | Default |
| :--- | :--- | :--- | :--- |
| `--file <PATH>` | `-f <PATH>` | Script file or path to execute (e.g. `--file="server.rhp"`) | `index.rhp` in `pwd` |
| `--port <PORT>` | `-p <PORT>` | Port to bind when running as an HTTP server | `8080` |
| `--host <HOST>` | | Network interface to bind | `127.0.0.1` |
| `--cli` | | Force one-shot CLI execution even if script is a web app | `false` |
| `--server` | | Force HTTP web server mode | `false` |

---

### `rhp build [OPTIONS]`

Compiles source code into linear register-based bytecode IR and executes it within the high-performance multi-worker Register VM with Cranelift JIT compilation enabled.

```bash
# Execute in Production Mode with Cranelift JIT compilation
rhp build --file="server.rhp"
rhp build --file="path/to/server.rhp"

# Positional third parameter is also supported as a shorthand:
rhp build server.rhp

# If the third parameter (or --file option) is not provided, defaults to index.rhp in current directory:
rhp build

# Run production web server on custom port
rhp build --file="app.rhp" --port 8000
```

> [!NOTE]
> **Default Resolution:** Just like `rhp dev`, if the third parameter in the `build` command (or the `--file` option) is not provided, RHP will automatically use `index.rhp` at the current working directory (`pwd`).

---

### `rhp serve [OPTIONS]`

Dedicated command to start the built-in RHP HTTP server for web applications. Routes incoming HTTP requests into isolated zero-bootstrap worker VMs with standard superglobals (`$_SERVER`, `$_GET`, `$_POST`, `HTTP_HX_REQUEST`).

```bash
# Serve application on default port 8080 (picks app.rhp or index.rhp in current directory)
rhp serve

# Serve application directory or explicit file
rhp serve examples/todo
rhp serve --port 8080 --host 0.0.0.0
```

---

### `rhp lint [PATH] [OPTIONS]`

Lints, auto-formats code according to PER-CS standards, infers variable, parameter, and property type signatures, normalizes casing conventions, and outputs diagnostics in machine-readable JSON inspired by [Carthage Software Mago](https://github.com/carthage-software/mago).

```bash
# Auto-format and write changes to disk
rhp lint src/

# Dry run: check and report issues in JSON without writing files
rhp lint src/ --check
# or alias:
rhp lint src/ --no-fix

# Auto-format and typecheck without changing identifier casing
rhp lint src/ --no-case-change
```

#### Options for `lint`:

| Option | Description | Default |
| :--- | :--- | :--- |
| `--check` | Dry-run check mode; emits diagnostics in JSON without modifying files | `false` |
| `--no-fix` | Alias for `--check`; leaves files on disk unmodified | `false` |
| `--fix` | Auto-formats code and writes inferred types and casing fixes to disk | `true` |
| `--no-case-change` | Skips identifier casing transformations (`snake_case`, `PascalCase`, `camelCase`) | `false` |

---

### `rhp migrate [PATH] [OPTIONS]`

Migrates legacy PHP codebases to modern RHP conventions:
- Strips legacy opening `<?php` and closing `?>` tags from start and end of files (native script syntax).
- Converts PHP string concatenation operators (`.` and `.=`) to modern RHP addition operators (`+` and `+=`).
- Injects modern Request/Cookie facade initialization statements when superglobals (`$_GET`, `$_POST`, `$_SERVER`, `$_FILES`, `$_COOKIE`) are detected:
  - `$_GET = Request::query();`
  - `$_POST = Request::body();`
  - `$_SERVER['REQUEST_METHOD'] = Request::method();`
  - `$_SERVER['REMOTE_ADDR'] = Request::ip();`
  - `$_SERVER['HTTP_HOST'] = Request::host();`
  - `$_FILES = Request::files();`
  - `$_COOKIE = Cookie::get();`
- Upgrades legacy cookie modifications (`setcookie(...)`, `setrawcookie(...)`, `$_COOKIE['key'] = $val;`) to `Cookie::set(...)`.
- Replaces legacy `$GLOBALS['var']` and `$_GLOBAL['var']` with `$var`, and dynamic `$GLOBALS[$k]` with `$$k`.
- Upgrades legacy procedural `curl_*` calls to fluent `Http::` facade chains (preserves original code commented out, supports Option A/B response variables, and auto-wraps unassigned requests in `Future::async(...)`).
- Flags `curl_multi_*` loops for manual review with a direct guide link and recommendations for `Http::pool([...])` or `Future::wait([...])`.

```bash
# Migrate a directory or file in-place
rhp migrate ./app
rhp migrate script.php

# Dry-run / check mode: preview changes without writing to disk
rhp migrate ./app --check
# or alias:
rhp migrate ./app --dry-run

# Output results as machine-readable JSON
rhp migrate ./app --format=json
```

#### Options for `migrate`:

| Option | Description | Default |
| :--- | :--- | :--- |
| `PATH` | Target file or directory to migrate | `.` (current directory) |
| `--check`, `--dry-run` | Check and preview migrations without writing changes to disk | `false` |
| `--format <human\|json>` | Output format: human-readable summary or JSON report | `human` |

---

## Package Management

RHP includes a full package manager integrated directly into the `rhp` binary, following modern packaging conventions like Cargo, Go, and Bun. Projects declare dependencies in `rhp.toml` with deterministic resolutions pinned in `rhp.lock`. Installed packages follow standard PHP PSR-4 autoloader conventions.

### `rhp init [NAME]`

Initializes a new `rhp.toml` manifest in the current directory with package metadata and dependency tables.

```bash
# Initialize manifest using current directory name
rhp init

# Initialize with an explicit package name
rhp init my-awesome-app
```

Generated `rhp.toml`:
```toml
[package]
name = "my-awesome-app"
version = "0.1.0"
authors = []

[dependencies]

[dev-dependencies]
```

---

### `rhp add <PACKAGE> [OPTIONS]`

Adds a dependency to `rhp.toml` and `rhp.lock`, downloads or links the package into `vendor/`, and updates the PSR-4 autoloader.

```bash
# Add from GitHub shorthand (owner/repo)
rhp add symfony/var-dumper

# Add with a specific Git branch or release tag
rhp add spatie/ray --tag v1.40.0
rhp add laravel/framework --branch 11.x

# Add by exact commit hash
rhp add owner/repo --rev a1b2c3d4e5f6

# Add by custom Git repository URL
rhp add --git https://github.com/vuenice/rhp-helpers.git

# Add a local directory path dependency
rhp add ./packages/local-sdk
rhp add my-library --path ../shared-lib

# Add as a development dependency ([dev-dependencies])
rhp add phpunit/phpunit --dev
```

#### Options for `add`:

| Option | Shorthand | Description |
| :--- | :--- | :--- |
| `--git <URL>` | | Specify custom git repository URL |
| `--branch <BRANCH>` | `-b <BRANCH>` | Specify git branch to clone |
| `--tag <TAG>` | `-t <TAG>` | Specify git tag or release version |
| `--rev <COMMIT>` | | Pin to an exact git commit hash |
| `--path <PATH>` | | Specify local directory path dependency |
| `--dev` | | Add as development dependency in `[dev-dependencies]` |

---

### `rhp remove <PACKAGE>` (or `rhp rm <PACKAGE>`)

Removes a dependency from `rhp.toml` and `rhp.lock`, deletes its directory from `vendor/`, and regenerates the autoloader.

```bash
# Remove a dependency
rhp remove symfony/var-dumper

# Shorthand alias
rhp rm spatie/ray
```

---

### `rhp install [OPTIONS]`

Installs all dependencies strictly from `rhp.lock`. This command is designed for reproducible CI/CD environments, container builds, and fresh developer checkouts.

```bash
# Install all locked dependencies
rhp install

# Strict CI install (ensures lockfile is not modified)
rhp install --frozen
```

> **Note on adding packages:**
> Unlike npm or composer, `rhp install` only operates on the lockfile and does not accept package arguments. To add a new package, use `rhp add <package>`. Running `rhp install <package>` outputs a helpful reminder:
> ```text
> Error: 'rhp install' installs all dependencies from the lockfile.
> 💡 To add a new dependency, use 'rhp add <package>'
> ```

#### Options for `install`:

| Option | Description |
| :--- | :--- |
| `--frozen` | Strictly verify lockfile and prevent modifications |

---

### `rhp update [PACKAGE]` (or `rhp up [PACKAGE]`)

Updates dependencies to their latest compatible commits, fetches updates, and refreshes `rhp.lock` and `vendor/`.

```bash
# Update all dependencies
rhp update

# Update a single package
rhp update symfony/var-dumper

# Shorthand alias
rhp up
```

---

### `rhp list` (or `rhp ls`)

Lists all installed dependencies and dev-dependencies from `rhp.toml` and `rhp.lock`.

```bash
rhp list

# Shorthand alias
rhp ls
```

---

### Build-Time PSR-4 Class Map & Autoloading

Whenever dependencies are installed, added, or removed, RHP generates static autoload manifests in `vendor/`:
- `vendor/autoload.rhp`: Build-time class map and package prelude.
- `vendor/rhp/installed.json`: Package metadata and installation manifests.
- `vendor/rhp/autoload_psr4.rhp`: PSR-4 namespace mapping table.
- `vendor/rhp/autoload_files.rhp`: Static helper files autoloader.

In RHP, there is **no `require_once` and no runtime `spl_autoload_register`**. The compiler reads the PSR-4 namespace mapping at compile time, links `vendor/` into the static module graph, and initializes packages once during boot. You simply import installed package classes directly:

```php
<?php
use Symfony\Component\VarDumper\VarDumper;

VarDumper::dump(['status' => 'success', 'engine' => 'rhp']);
```

---

## One-Time Script Execution (Direct Execution)

### `rhp ./script.rhp`

RHP also supports running scripts directly using `rhp ./script.rhp` for one-time execution and OS reliance (such as shell scripts, CLI automation, system administration utilities, and shebang scripts).

```bash
# Execute a script directly
rhp ./script.rhp

# Or with a relative or absolute file path
rhp path/to/script.rhp
```

> [!IMPORTANT]
> **Do not mix up `rhp ./script.rhp` with `rhp [COMMAND] [OPTIONS]`.**
> When executing a script directly, do not pass subcommands (such as `dev` or `build`). The script executes immediately in production JIT mode.

> [!NOTE]
> The target file **must end with the `.rhp` format** (e.g. `rhp ./script.rhp`). This requirement ensures the RHP CLI unambiguously distinguishes one-time script execution from built-in subcommands.
