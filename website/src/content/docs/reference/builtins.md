---
title: "Built-in Functions"
description: "Reference for intrinsic compiler intrinsics and legacy PHP backward-compatibility function lowering."
section: "Reference"
order: 3
---

## Reserved Compiler Intrinsics

To eliminate global namespace pollution while preserving convenient developer shorthand, the Pratt parser automatically intercepts reserved intrinsics and lowers them to static `Sys::*` facade calls:

| Shorthand Call | Lowered AST Node | Description |
| :--- | :--- | :--- |
| `echo("text", ...$args)` | `Sys::echo("text", ...$args)` | Directly compiles to Rust `print!()` (stdout without trailing newline) |
| `print("text", ...$args)` | `Sys::print("text", ...$args)` | Directly compiles to Rust `println!()` (stdout WITH trailing newline `\n`) |
| `env('KEY', 'default')` | `Sys::env('KEY', 'default')` | Read system environment variable |
| `dd($val1, $val2)` | `Sys::dd($val1, $val2)` | Dump variables and terminate execution |

> **Formatting Engine & Parameters:**
> Both `Sys::echo` and `Sys::print` directly map to Rust function calls (`print!` and `println!`) with function parameters passed as-is. RHP does **not** support C-style formatting (`%d`, `%s`, `%f`, `%x`, etc.). Use native Rust placeholders such as `{}`, `{:.2}` (floating-point precision), `{:05}` (zero padding), `{:x}` (hexadecimal), and `{:b}` (binary).
>
> **Note on `Sys::println`:**
> There is **no** `Sys::println` function in RHP. Only `echo` (inline, no newline) and `print` (with trailing newline) exist in the language and via the `Sys` facade (`Sys::echo` and `Sys::print`). Calling `Sys::println(...)` produces an `UndefinedFacadeMethod` error.

---

## Legacy PHP Function Lowering (`phpFun::*`)

For backward compatibility with legacy codebases, standard PHP function invocations are recognized via a compile-time static registry and automatically lowered into corresponding RHP facade methods:

| Legacy Function Call | Lowered Expression | Target Facade Implementation |
| :--- | :--- | :--- |
| `array_merge($a, $b)` | `phpFun::array_merge($a, $b)` | `Arr::merge($a, $b)` |
| `strlen($str)` | `phpFun::strlen($str)` | `Str::byteLength($str)` |
| `implode($glue, $arr)` | `phpFun::implode($glue, $arr)` | `Arr::implode($glue, $arr)` |
| `file_get_contents($path)` | `phpFun::file_get_contents($path)` | `File::read($path)` |
| `file_put_contents($path, $data)` | `phpFun::file_put_contents($path, $data)` | `File::write($path, $data)` |

---

## Dynamic Global Helpers (`Glob::*`)

Any unrecognized global function call that is not defined in userland is routed through the `Glob` fallback container:

```rust
// Pratt parser lowering fallback:
Expr::static_call("Glob", "customHelper", vec![callee_name, ...args])
```
