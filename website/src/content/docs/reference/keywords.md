---
title: "Keywords Reference"
description: "Exhaustive index of all reserved words, contextual keywords, and modifiers in RHP."
section: "Reference"
order: 2
---

## Reserved Keywords

The following identifiers are reserved language keywords and cannot be used as function or class names:

```text
abstract    as          async       break       case        catch
class       const       continue    default     defer       do
else        elseif      enum        extends     final       finally
fn          for         foreach     function    if          implements
interface   import      macro       match       namespace   new
private     protected   public      readonly    return      singleton
static      switch      throw       trait       try         use
while       yield
```

---

## Type Keywords

The following primitive and collection subtype keywords are reserved for type annotations:

```text
int         float       string      bool        void        mixed
array       list        dict        set         hashmap     map
callable    iterable    object      never       null
```

### Collection & Type Boundary Rules

| Type Category | Keywords | Behavioral Contract |
| :--- | :--- | :--- |
| **Permissive Parent Collection** | `array` | Backward-compatible parent collection type. Accepts `list`, `dict`, `set`, and `hashmap` interchangeably in variable declarations (`array $arr = []`), function parameter hints (`array $items`), and return hints (`: array`). |
| **Strict Collection Subtypes** | `list`, `dict`, `set`, `hashmap`, `map` | Enforces strict compile-time and runtime boundaries. Incompatible assignments (e.g. assigning a dictionary to `list $arr` or scalar to `set $s`) throw a `VariableTypeMismatch` error. |
| **Strict Scalar Primitives** | `int`, `float`, `string`, `bool` | Enforces strict Rust-style type boundaries without implicit type-shifting (`int $x = 0; $x = 'hello';` throws a type error). Must be strictly lowercase. |
| **Special Types** | `void`, `never`, `mixed`, `callable`, `iterable`, `object`, `null` | Boundary control, dynamic escape hatches, and function return contracts. |

---

## Special Identifiers & Superglobals

| Identifier | Context | Description |
| :--- | :--- | :--- |
| `$this` | Class instances | Reference to current instance |
| `self` | Class scope | Reference to enclosing class definition |
| `parent` | Subclasses | Reference to parent class implementation |
| `$_SERVER` | Web requests | Server environment parameters dictionary |
| `$_GET` | **Deprecated** | Use `Request::get()` instead |
| `$_POST` | **Deprecated** | Use `Request::get()` instead |
| `$_SESSION` | **Deprecated** | Use `Session::get()` / `Session::set()` instead |
| `$_COOKIE` | **Deprecated** | Use `Cookie::get()` / `Cookie::set()` instead |

> **Note on Superglobals in RHP:**
> Direct use of `$_GET`, `$_POST`, `$_SESSION`, and `$_COOKIE` is deprecated. RHP provides modern, type-safe static facades (`Request`, `Session`, and `Cookie`). The `rhp run lint` command reports `deprecated-superglobal` warnings when legacy superglobals are used.

---

## Module & Native Import Keywords

| Keyword | Syntax Example | Description |
| :--- | :--- | :--- |
| `use` | `use App\Models\User;` or `use "./Cache.rs";` | Imports PHP/RHP namespaces, classes, or native `.rs` helper files |
| `import` | `import "./Cache.rs";` or `import Cache from "./Cache.rs";` | Direct import of native Rust `.rs` files as Tier 2 static facades |
| `as` | `import "./Log.rs" as CustomLogger;` | Aliases an imported class or native helper class identifier |

---


## Explicitly Disallowed Keywords: `await`

RHP deliberately rejects the **`await`** keyword to eliminate function coloring across codebases.

| Keyword | Status | Compiler Diagnostic | Recommended Alternative |
| :--- | :--- | :--- | :--- |
| `await` | **Rejected** (Compile Error) | *"The 'await' keyword is not supported in RHP. Use synchronous facades (File, DB, HTTP) to get results synchronously, or call '->wait()' on Future objects."* | Use synchronous facades (`File`, `DB`, `HTTP`) or call `$future->wait()` |

