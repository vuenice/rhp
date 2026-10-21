---
title: "Linter and Static Typing"
description: "Static type checker, variable auto-typing, casing enforcement, and JSON diagnostics in RHP."
section: "Tooling"
order: 3
---

## Overview

The `rhp run lint` command combines static analysis, AST-guided code formatting, boundary type checking, and automated type inference into a unified developer tooling pipeline inspired by [Carthage Software Mago](https://github.com/carthage-software/mago):

```bash
rhp run lint [PATH] [OPTIONS]
```

By default, running `rhp run lint` executes in fix-and-report mode:
1. **PER-CS Code Formatting:** Enforces standard 4-space indentation, normalized brace positioning, and operator spacing while preserving comments, docblocks, and string literals verbatim.
2. **Automated Type Inference:** Automatically prepends optimal type annotations before untyped variables, class properties, and parameter defaults wherever statically determinable.
3. **Casing Normalization:** Enforces uniform identifier naming conventions across variables, classes, and functions with built-in exceptions for PHP superglobals, magic methods, and test functions.
4. **Static Type Checking & Diagnostics:** Runs full static analysis and emits structured diagnostics in Carthage Mago-compatible JSON format.

---

## Command-Line Usage and Options

```bash
# Auto-format, infer types, normalize casing, and save changes to disk
rhp run lint

# Lint and auto-fix a specific file or directory
rhp run lint src/
rhp run lint app/Models/User.rhp

# Dry-run check: inspect code and report issues without modifying files on disk
rhp run lint src/ --check
# or
rhp run lint src/ --no-fix

# Auto-format and infer types while preserving existing identifier casing
rhp run lint src/ --no-case-change
```

### Options Reference

| Flag | Shorthand | Description | Default |
| :--- | :--- | :--- | :--- |
| `--check` | | Dry-run check mode; emits diagnostics in JSON without modifying files on disk | `false` |
| `--no-fix` | | Alias for `--check`; runs checks without applying automated fixes to disk | `false` |
| `--fix` | | Applies automated formatting, type inference, and casing changes to disk | `true` |
| `--no-case-change` | | Disables automated casing conversions and suppresses casing diagnostics | `false` |

---

## Code Formatting Invariants

The formatter operates on the AST and token stream, adhering to PER Coding Style (PER-CS 2.0) guidelines:

1. **4-Space Indentation:** All block levels, class definitions, function bodies, and array expressions are normalized to 4 spaces per nesting level. Tab characters (`\t`) are converted to spaces.
2. **Operator Spacing:** Binary operators (`+`, `-`, `*`, `/`, `===`, `!==`, `&&`, `||`, `=>`, `=`) receive single surrounding whitespace.
3. **Brace Normalization:** Opening braces for control structures and functions follow K&R / 1TBS convention, while class definitions adhere to PSR-12 styling.
4. **Verbatim Content Preservation:** String literals (single-quoted, double-quoted, heredoc/nowdoc) and comments (line comments `//`, block comments `/* ... */`, and docblocks `/** ... */`) are preserved verbatim with byte-exact precision.

---

## Automated Variable & Parameter Type Inference

When variables or function parameters are declared without explicit type annotations, `rhp run lint` inspects their initializer expressions and statically inserts the appropriate type annotation:

### 1. Scalar Primitives

```php
// BEFORE LINTING:
$message = "Hello, RHP!";
$count = 42;
$ratio = 3.14159;
$is_active = true;

// AFTER 'rhp run lint':
string $message = "Hello, RHP!";
int $count = 42;
float $ratio = 3.14159;
bool $is_active = true;
```

### 2. Array Classification (`list`, `dict`, `hashMap`, `array`)

When an array literal is initialized at declaration time, RHP inspects the element keys to assign specialized, high-performance collection types:

- **`list`:** Sequential integer-indexed elements or unkeyed arrays (`[1, 2, 3]`, `['apple', 'banana']`).
- **`dict`:** Non-empty arrays where all keys are string literals (`['name' => 'Alice', 'role' => 'admin']`).
- **`hashMap`:** Non-empty arrays where keys are integers, non-string scalars, or complex expressions (`[1 => 'first', 2 => 'second']`).
- **`array`:** Empty array declarations (`[]`) where keys and element types cannot yet be determined.

```php
// BEFORE LINTING:
$tags = ['php', 'rust', 'rhp'];
$user = ['id' => 'u1', 'active' => true];
$lookup = [100 => 'Pending', 200 => 'Success'];
$buffer = [];

// AFTER 'rhp run lint':
list $tags = ['php', 'rust', 'rhp'];
dict $user = ['id' => 'u1', 'active' => true];
hashMap $lookup = [100 => 'Pending', 200 => 'Success'];
array $buffer = [];
```

### 3. Function & Method Parameter Defaults

Parameters declared with default literal values receive automatic type hints:

```php
// BEFORE LINTING:
function configure($timeout = 30, $name = 'default', $options = ['cache' => true]) {
    // ...
}

// AFTER 'rhp run lint':
function configure(int $timeout = 30, string $name = 'default', dict $options = ['cache' => true]) {
    // ...
}
```

### 4. Class Property Defaults

Class properties with default values are automatically typed:

```php
// BEFORE LINTING:
class ServiceConfig {
    public $retries = 3;
    protected $endpoint = "https://api.internal";
    private $headers = ['Accept' => 'application/json'];
}

// AFTER 'rhp run lint':
class ServiceConfig {
    public int $retries = 3;
    protected string $endpoint = "https://api.internal";
    private dict $headers = ['Accept' => 'application/json'];
}
```

### 5. Preservation of User-Written Types & Ambiguity Handling

- **Existing Types Preserved:** If the developer already wrote an explicit type annotation (e.g. `int $count = "not an int";`), the linter **never overwrites** the author's declared type. Instead, the static type checker emits a type mismatch diagnostic for the developer to resolve.
- **Ambiguous or Dynamic Variables:** When a variable is initialized to a dynamic expression whose type cannot be statically resolved, or when a variable is reassigned to incompatible types within the same scope, the linter leaves the variable untyped without error ("whenever not possible, no issue").

---

## Casing Normalization Rules

RHP enforces uniform, idiomatic naming conventions throughout the codebase. References across definitions and call sites are updated consistently:

| Identifier Category | Target Convention | Example Transformation | Preserved Exceptions |
| :--- | :--- | :--- | :--- |
| **Variables** | `snake_case` | `$userAccount` $\rightarrow$ `$user_account` | `$this`, superglobals (`$_GET`, `$_POST`, `$_SERVER`, etc.) |
| **Classes / Interfaces / Enums** | `PascalCase` | `class order_item` $\rightarrow$ `class OrderItem` | Built-in facades (`Arr`, `Str`, `Trace`, etc.) and native types |
| **Functions & Methods** | `camelCase` | `function process_order()` $\rightarrow$ `function processOrder()` | Magic methods (`__construct`, `__destruct`, etc.) |
| **Test Functions** | `snake_case` | `function testUserLogin()` $\rightarrow$ `function test_user_login()` | Any function/method whose name begins with `test` |

### Consistent Identifier References

When a class, function, or variable is renamed to match convention, `rhp run lint` updates all corresponding references:
- Class instantiations (`new user_profile()` $\rightarrow$ `new UserProfile()`)
- Inheritance and interface implementation (`extends base_model` $\rightarrow$ `extends BaseModel`)
- Trait imports (`use json_serializable;` $\rightarrow$ `use JsonSerializable;`)
- Static method invocations (`auth_manager::check()` $\rightarrow$ `AuthManager::check()`)
- Type hints, `instanceof` checks, and `catch` clauses

### Disabling Casing Changes

If working with legacy code or third-party APIs where naming conventions must not be altered, pass `--no-case-change`:

```bash
rhp run lint src/ --no-case-change
```

---

## Deprecated Superglobals Diagnostics

Direct usage of procedural PHP superglobals (`$_GET`, `$_POST`, `$_SESSION`, `$_COOKIE`) is flagged with actionable warnings recommending modern static facades:

```json
{
  "level": "warning",
  "code": "deprecated-superglobal",
  "message": "Direct use of '$_GET' is discouraged in RHP. Use 'Request::get()' instead.",
  "file": "src/Http/Controller.rhp",
  "line": 15,
  "column": 12,
  "span": {
    "start": 320,
    "end": 326
  }
}
```

Recommended replacements:
- `$_GET['key']` $\rightarrow$ `Request::get('key')`
- `$_POST['action']` $\rightarrow$ `Request::get('action')`
- `$_SESSION['user_id']` $\rightarrow$ `Session::get('user_id')`
- `$_COOKIE['session_token']` $\rightarrow$ `Cookie::get('session_token')`

---

## Machine-Readable JSON Output

In CI/CD pipelines, pre-commit hooks, and IDE integrations, run `rhp run lint` with `--check` or `--no-fix` to output machine-readable JSON matching the Carthage Mago diagnostics format:

```bash
rhp run lint src/ --check
```

### JSON Output Schema

```json
{
  "status": "ok",
  "files_scanned": 12,
  "files_modified": 0,
  "total_issues": 2,
  "errors": 0,
  "warnings": 2,
  "issues": [
    {
      "level": "warning",
      "code": "casing/variable",
      "message": "Variable '$userAccount' should be snake_case: '$user_account'",
      "file": "src/Models/User.rhp",
      "line": 8,
      "column": 9,
      "span": {
        "start": 142,
        "end": 154
      }
    },
    {
      "level": "warning",
      "code": "deprecated-superglobal",
      "message": "Direct use of '$_POST' is discouraged in RHP. Use 'Request::get()' instead.",
      "file": "src/Controllers/AuthController.rhp",
      "line": 24,
      "column": 18,
      "span": {
        "start": 612,
        "end": 619
      }
    }
  ]
}
```

### CI/CD Integration Example

```yaml
# GitHub Actions Workflow
name: Lint & Typecheck
on: [push, pull_request]

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Run RHP Linter
        run: |
          rhp run lint src/ --check
```

