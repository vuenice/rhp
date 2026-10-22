---
title: "Operators"
description: "Reference guide for arithmetic, logical, null-coalescing, and comparison operators in RHP."
section: "Language Guide"
order: 2
---

## Arithmetic Operators

RHP supports standard mathematical operators with compile-time type validation:

| Operator | Name | Example | Description |
| :--- | :--- | :--- | :--- |
| `+` | Addition | `$a + $b` | Sum of operands |
| `-` | Subtraction | `$a - $b` | Difference of operands |
| `*` | Multiplication | `$a * $b` | Product of operands |
| `/` | Division | `$a / $b` | Quotient of operands |
| `%` | Modulo | `$a % $b` | Remainder of division |
| `**` | Exponentiation | `$a ** $b` | Base raised to exponent |

---

## Assignment Operators

```php
int $x = 10;
$x += 5;   // 15
$x *= 2;   // 30
$x -= 10;  // 20
$x /= 4;   // 5
```

---

## Comparison Operators

RHP distinguishes between value equivalence (`==`) and strict identity (`===`):

```php
int $a = 10;
int $b = 10;

// Strict identity check (type and value)
if ($a === $b) {
    print("Values and types match exactly");
}

// Spaceship operator (<=>) for sorting
int $cmp = $a <=> 5; // returns 1
```

---

## Null-Coalescing and Ternary Operators

The null-coalescing operator (`??`), ternary operator (`?:`), and null-coalescing assignment (`??=`) provide concise handling of optional or fallback values:

```php
// Null coalescing fallback
dict $config = Arr::dict(['env' => 'production']);
string $mode = $config->get('debug') ?? 'false';

// Null coalescing assignment
string $cache_dir = null;
$cache_dir ??= '/tmp/rhp_cache';

// Ternary expression
bool $is_admin = true;
string $role = $is_admin ? 'SuperAdmin' : 'Member';
```

### Pratt Parser Precedence Hierarchy

RHP parses expressions using an explicit **Pratt parser** with strictly calibrated binding powers (BP):

| Operator | Syntax | Binding Power | Associativity | Description |
| :--- | :--- | :--- | :--- | :--- |
| **Null Coalesce** | `??` | **30** | Right | Higher precedence than ternary; evaluated first |
| **Ternary** | `? :` | **25** | Right | Evaluates conditions and branches |
| **Null-Coalescing Assign** | `??=` | **20** | Right | Lowest compound precedence; assigns if target is null |

#### Disambiguation Examples

1. **`??` vs `?:` Precedence:**
   ```php
   // Evaluates as: ($a ?? $b) ? ($a ?? $b) : $c
   $result = $a ?? $b ?: $c;
   ```
   Because `??` has binding power **30** and `?:` has binding power **25**, the null-coalescing sub-expression binds tighter.

2. **`??` vs `??=` Precedence:**
   ```php
   // Evaluates as: $a ??= ($b ?? $c)
   $a ??= $b ?? $c;
   ```
   `??=` (binding power **20**) yields to `??` (binding power **30**), ensuring right-hand expressions resolve completely before assignment.

---

## String Concatenation Operator (`.`)

Consistent with PHP syntax, strings are concatenated using the dot (`.`) operator:

```php
string $first = "Fast";
string $second = "Backend";
string $full = $first . " " . $second;
```

---

## Operator Ambiguity & Maximal Munch

When consecutive operator characters appear, RHP's lexer applies the **Maximal Munch Rule**: the longest possible valid token is consumed unconditionally.

### Dot-Family Operators

| Token | Symbol | Length | Semantic Role |
| :--- | :--- | :--- | :--- |
| `Ellipsis` | `...` | 3 characters | Variadic parameters and array/collection unpacking |
| `Range` | `..` | 2 characters | Sequential range iteration (`1..10`) |
| `Dot` | `.` | 1 character | String concatenation |

### Disambiguation Behavior

```php
// Lexed as Variadic/Ellipsis (longest match):
function sum(...$values) {}

// Lexed as Range:
foreach (1..5 as $i) {}

// Lexed as Dot concatenation:
$label = "Item" . $id;

// Whitespace-free maximal munch:
// $a...$b is lexed as: [$a, ..., $b] (Ellipsis), NOT [$a, .., ., $b]
```

To avoid subtle bugs, always format multi-character expressions with clean whitespace (e.g., `$first . $second`).

---

## Type Conversion (Functional Syntax, No `(type)` Casts)

In RHP, type conversion is not performed using C-style or PHP-style prefix cast operators like `(int)$x` or `(string)$x`. Instead, use functional conversion syntax:

```php
int $numericId  = int($request->get('id'));
string $textVal = string(12345);
float $amount   = float("49.99");
bool $isEnabled = bool($config->get('active'));
```

> [!NOTE]
> Attempting to use parenthesized casts like `(int)$val` or `(string)$val` triggers a compile-time `ParseError`. Always use `int(...)`, `string(...)`, `float(...)`, or `bool(...)`.
