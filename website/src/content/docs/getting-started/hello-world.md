---
title: "Hello World"
description: "Create, format, and execute your first RHP script."
section: "Getting Started"
order: 2
---

## Creating Your First Script

RHP files use either the `.rhp` extension (recommended for standalone projects) or `.php` (for gradual drop-in upgrades).

Create a new file named `hello.rhp`:

```php
<?rhp

// Declare typed variables
string $target = "World";

// Utilize native Rust-backed string facades
string $message = Str::slug("Hello " . $target . " from RHP Engine");

// Standard library output intrinsic
Sys::print("Output: " . $message);
Sys::print("RHP Version: " . Sys::version());
```

---

## Executing in Development Mode

During development, you want instantaneous startup and clear stack traces. Execute the script using `rhp run dev`:

```bash
rhp run dev hello.rhp
```

Output:
```text
Output: hello-world-from-rhp-engine
RHP Version: 0.1.0
```

> **How it works:** Development Mode uses a direct tree-walking AST interpreter. It evaluates parse tree nodes directly in memory without emitting bytecode or waiting for JIT compilation, delivering cold startup times under 5 milliseconds.

---

## Executing in Production Mode

When deploying high-throughput microservices or running CPU-intensive algorithms, execute via `rhp run build`:

```bash
rhp run build hello.rhp
```

> **How it works:** Production Mode lowers the parsed AST into a register-based Bytecode Intermediate Representation (IR) and activates Cranelift JIT compilation on hot loops, running inside a Tokio multi-threaded worker pool.

---

## Auto-Formatting and Linting

RHP includes a built-in code formatter and type annotator inspired by Carthage Software Mago:

```bash
rhp run lint hello.rhp
```

The linter automatically:
1. Enforces standard 4-space indentation and normalizes operator spacing.
2. Formats identifiers according to RHP conventions (`$snake_case` for variables, `PascalCase` for classes, `camelCase` for methods).
3. Infers and prepends variable type hints where omitted.
