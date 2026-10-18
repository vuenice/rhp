---
title: "Linter and Static Typing"
description: "Static type checker, variable auto-typing, casing enforcement, and JSON diagnostics in RHP."
section: "Tooling"
order: 3
---

## Overview

The `rhp run lint` command combines static analysis, boundary type checking, and automated type inference into a unified pipeline:

```bash
rhp run lint [PATH] [OPTIONS]
```

---

## Automated Variable Type Inference

When variables are declared without explicit type annotations, the linter analyzes the initializer AST expression and inserts optimal type hints:

```php
// BEFORE LINTING:
$message = "Hello";
$count = 100;
$items = [1, 2, 3];
$payload = ['status' => 'ok'];

// AFTER 'rhp run lint':
string $message = "Hello";
int $count = 100;
list $items = [1, 2, 3];
dict $payload = ['status' => 'ok'];
```

---

## Casing Normalization Rules

RHP enforces consistent casing across codebases:

- **Variables:** Normalized to `snake_case` (`$userAccount` $\rightarrow$ `$user_account`).
- **Classes / Interfaces / Enums:** Normalized to `PascalCase` (`class user_account` $\rightarrow$ `class UserAccount`).
- **Methods & Functions:** Normalized to `camelCase` (`function process_order()` $\rightarrow$ `function processOrder()`).
- **Test Exception:** Functions beginning with `test` are preserved in `snake_case` (`function test_user_login()`).

To disable automated casing transformations, pass `--no-case-change`:

```bash
rhp run lint src/ --no-case-change
```

---

## Machine-Readable JSON Output

In CI/CD environments, export diagnostics directly:

```bash
rhp run lint src/ --check
```

Outputs diagnostic arrays formatted for GitHub Actions, GitLab CI, and IDE problem matchers.
