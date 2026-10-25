---
title: "Keywords Reference"
description: "Exhaustive index of all reserved words, contextual keywords, and modifiers in RHP."
section: "Reference"
order: 3
---

## Reserved Keywords

The following identifiers are reserved language keywords and cannot be used as standalone function or class names:

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

## Keyword-as-Method-Name Resolution

Keywords such as `match`, `default`, `from`, `for`, `class`, `new`, `static`, and `clone` are reserved as strict language keywords primarily at **statement-start** contexts:

* **Object Accessor Resolution:** When any keyword follows an object accessor (`->`, `?->`) or scope resolution operator (`::`), the parser lookahead stage contextually re-classifies the token as an identifier method or property name.
* **Static Facade Disambiguation:** When an operation requires explicit disambiguation, RHP provides the built-in `Glob` static facade (e.g. `Glob::getClass($obj)`, `Glob::match($pattern, $subject)`).

```php
// Valid: Keywords invoked as method or property accessors
$obj->match();
$obj->default();
$obj->from();
$obj->for();
$obj->class;
$obj->new();
$obj->static();
$obj->clone();

// Disambiguation via static facade
Glob::getClass($obj);
Glob::match('/^[a-z]+$/', $input);
```

---

## Type Keywords

The following primitive and collection subtype keywords are reserved for type annotations:

```text
int         float       string      bool        void        mixed
array       list        dict        set         hashmap     map
callable    iterable    object      never       null
```

### Strict Case-Sensitivity & Forbidden Aliases

RHP enforces strict case-sensitivity for literals and primitive types to maintain compiler performance and eliminate ambiguity:
* **Strictly Valid Literals (Lowercase Only):** `true`, `false`, `null`. Capitalized variants (`TRUE`, `FALSE`, `NULL`) are rejected as unknown identifiers.
* **Strictly Valid Primitives (Lowercase Only):** `string`, `bool`, `int`, `float`.
* **Rejected Aliases:** Capitalized variants (`String`, `Boolean`, `Integer`) and legacy PHP aliases (`boolean`, `integer`) are rejected by the type checker.

```php
// Valid: strictly lowercase primitives
int $count = 42;
string $name = "Alice";
bool $flag = true;
$emptyVal = null;

// Rejected by type checker:
// String $a;   // Error: Invalid primitive type 'String'
// Boolean $b;  // Error: Invalid primitive type 'Boolean'
// integer $c;  // Error: Invalid primitive type 'integer'
```

### Collection & Type Boundary Rules

| Type Category | Keywords | Behavioral Contract |
| :--- | :--- | :--- |
| **Permissive Parent Collection** | `array` | Backward-compatible parent collection type. Accepts `list`, `dict`, `set`, and `hashmap` interchangeably in variable declarations (`array $arr = []`), function parameter hints (`array $items`), and return hints (`: array`). |
| **Strict Collection Subtypes** | `list`, `dict`, `set`, `hashmap`, `map` | Enforces strict compile-time and runtime boundaries. Incompatible assignments (e.g. assigning a dictionary to `list $arr` or scalar to `set $s`) throw a `VariableTypeMismatch` error. |
| **Strict Scalar Primitives** | `int`, `float`, `string`, `bool` | Enforces strict Rust-style type boundaries without implicit type-shifting (`int $x = 0; $x = 'hello';` throws a type error). Must be strictly lowercase. |
| **Special Types** | `void`, `never`, `mixed`, `callable`, `iterable`, `object`, `null` | Boundary control, dynamic escape hatches, and function return contracts. |

### Default Numeric Storage

> [!IMPORTANT]
> **Both `int` and `float` default to `i32` (32-bit) storage.** Neither maps to `f64` by default.
> Use `Int::i64(...)` or `Float::f64(...)` for wider precision.

| Primitive | Default Rust Storage | Extended Variants (via facade) |
| :--- | :--- | :--- |
| `int` | `i32` | `Int::i8`, `Int::i16`, `Int::i32`, `Int::i64`, `Int::i128`, `Int::u8` … `Int::u128`, `Int::usize` |
| `float` | `i32` | `Float::f32`, `Float::f64` |

### `Int` and `Float` — Facade Identifiers (Not Type Keywords)

`Int` and `Float` (capitalized) are **static facade class identifiers**, not type keywords. They cannot be used as type annotations and cannot be instantiated with `new`:

```php
// ✅ Valid — lowercase type keyword annotations
function calc(int $a, float $b): float { ... }

// ✅ Valid — capitalized facade static method calls
$id = Int::i64(9223372036854775807);
$formatted = Float::f64(3.14)->toDecimal(2);

// ❌ ParseError — Int/Float cannot be used as type annotations
function broken(Int $a): Float { ... }

// ❌ Fatal Error — Int/Float cannot be instantiated
$x = new Int();
```

---

## Magic Constants

RHP supports standard PHP magic constants with **case-insensitive lexing** and **zero runtime overhead**. During parsing, the Pratt parser lowers them directly into compile-time integer and string literals (`ExprKind::Literal`), statically resolving active lexical and scope stacks without incurring performance penalties at runtime.

| Constant | Type | Description |
| :--- | :--- | :--- |
| `__LINE__` | `int` | Current 1-based line number in the source file. |
| `__FILE__` | `string` | Full canonical file path of the current script. |
| `__DIR__` | `string` | Directory of the current script (parent of `__FILE__`). |
| `__FUNCTION__` | `string` | Name of the active function/method (or `"{closure}"` inside anonymous functions / arrow closures, or `""` in global scope). |
| `__CLASS__` | `string` | Fully qualified class or enum name including namespace (e.g. `App\Services\Worker`), or `""` if outside class context. |
| `__METHOD__` | `string` | Method name including class FQN (e.g. `App\Services\Worker::handle`), function name for standalone functions, or `""` in global scope. |
| `__TRAIT__` | `string` | Fully qualified trait name, or `""` if outside trait context. |
| `__NAMESPACE__` | `string` | Current namespace name (e.g. `App\Services`), or `""` if in global scope. |

### Case-Insensitivity & Usage Example

All magic constants can be written in uppercase, lowercase, or mixed case (`__LINE__`, `__line__`, `__DIR__`, `__dir__`, `__CLASS__`, `__class__`):

```php
namespace App\Services;

class Logger {
    public int $created_line = __LINE__;

    public function log(string $msg, int $line = __LINE__, string $file = __FILE__): void {
        print("[{$file}:{$line}] " . __METHOD__ . "() -> " . $msg);
    }
}

$logger = new Logger();
$logger->log("Server initialized");
// Outputs: [/var/www/app/Logger.rhp:7] App\Services\Logger::log() -> Server initialized
```

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

