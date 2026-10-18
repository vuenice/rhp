---
title: "Auto-Formatter"
description: "AST-guided code formatting, 4-space indentation rules, and brace normalization in RHP."
section: "Tooling"
order: 2
---

## Overview

RHP ships with an integrated code formatter accessible via:

```bash
rhp run lint [PATH]
```

Inspired by Carthage Software Mago, the formatter parses code into a concrete syntax tree, normalizes whitespace, formats braces, aligns operators, and outputs clean, idiomatic code while preserving comments and string literals verbatim.

---

## Formatting Invariants

1. **4-Space Indentation:** All blocks, class bodies, functions, and array literals use exactly 4 spaces. Tabs are automatically converted.
2. **K&R / 1TBS Brace Style:** Opening curly braces stay on the same line for control structures (`if`, `while`, `match`) and functions, while classes align according to PSR-12 standard.
3. **Operator Spacing:** Strict single spacing surrounding binary operators (`$a + $b`, `$x === $y`, `$dict->get('key')`).
4. **Trailing Commas:** Enforced on multi-line array literals and parameter lists for clean git diffs.

---

## Dry-Run Mode

To check if files comply with the formatter without modifying them:

```bash
rhp run lint src/ --check
```

Returns exit code `0` if all files match formatting standards, or `1` with diff diagnostics if formatting changes are required.
