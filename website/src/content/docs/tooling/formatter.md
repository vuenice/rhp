---
title: "Auto-Formatter"
description: "AST-guided code formatting, 4-space indentation rules, and brace normalization in RHP."
section: "Tooling"
order: 2
---

## Overview

RHP ships with an integrated code formatter accessible via the unified tooling command:

```bash
rhp run lint [PATH] [OPTIONS]
```

Inspired by [Carthage Software Mago](https://github.com/carthage-software/mago), the formatter operates directly on concrete syntax trees and token streams. It normalizes indentation, formats braces, aligns operators, and outputs clean, idiomatic code adhering to the PER Coding Style (PER-CS 2.0) while preserving comments, docblocks, and string literals verbatim.

---

## Formatting Invariants

1. **4-Space Indentation:** All blocks, class bodies, functions, match arms, and array literals use exactly 4 spaces per nesting level. Tab characters (`\t`) are automatically converted.
2. **K&R / 1TBS Brace Style:** Opening curly braces stay on the same line for control structures (`if`, `while`, `for`, `match`) and functions, while class declarations align according to the PSR-12 standard.
3. **Operator Spacing:** Strict single spacing surrounding binary operators (`$a + $b`, `$x === $y`, `$x => $y`).
4. **Clean Commas:** Consistent comma placement across multi-line array literals and function parameter lists for clean git diffs.
5. **Verbatim Literal Preservation:** Single-quoted, double-quoted, heredoc/nowdoc strings, and inline/docblock comments are preserved with byte-exact precision.

---

## Execution Modes

### Apply Formatting & Fixes

```bash
# Format current directory
rhp run lint

# Format specific directory or file
rhp run lint src/
rhp run lint app/Models/Order.rhp
```

### Dry-Run Check Mode

To check if files comply with the formatter without modifying files on disk:

```bash
rhp run lint src/ --check
# or
rhp run lint src/ --no-fix
```

Returns exit code `0` if all files match formatting standards, or `1` with diagnostics if formatting or type errors exist.

---

## Integration with Linter & Type Inferencer

The formatter runs as the first stage of the `rhp run lint` pipeline, immediately followed by:
1. **Automated Type Inference:** Inferring scalar types, `list`, `dict`, `hashMap`, and parameter defaults.
2. **Casing Normalization:** Enforcing `snake_case` for variables, `PascalCase` for classes, `camelCase` for functions, with `snake_case` for `test*` functions.
3. **Static Typecheck:** Emitting machine-readable diagnostics in Mago-compatible JSON format.

For full details on type inference and casing rules, see the [Linter and Static Typing](/tooling/linter) documentation.
