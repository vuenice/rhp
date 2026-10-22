---
title: "Numeric Types"
description: "Complete reference for int, float, Int::, and Float:: — RHP's numeric primitives and supporting facade classes."
section: "Language Guide"
order: 2
---

## Overview

RHP has a deliberate, simple numeric story:

| Item | Category | Default Rust storage | Notes |
| :--- | :--- | :--- | :--- |
| `int` | Primitive keyword | **`i32`** | Base datatype. Strict lowercase. |
| `float` | Primitive keyword | **`i32`** | Base datatype. Strict lowercase. Defaults to 32-bit, **not** `f64`. |
| `Int` | Static facade class | — | Utility class. Cannot be instantiated. |
| `Float` | Static facade class | — | Utility class. Cannot be instantiated. |

> [!IMPORTANT]
> **Both `int` and `float` default to `i32` (32-bit) storage.**
> They are separate types with different semantic intent (`int` for whole numbers, `float` for fractional numbers), but both are backed by `i32` internally at runtime unless you opt into a wider type via `Int::i64(...)` or `Float::f64(...)`.

---

## `int` — The Integer Primitive

`int` is a reserved lowercase keyword representing a **signed 32-bit integer** (`i32`).

```php
int $count = 42;
int $negative = -1000;
int $hex   = 0xFF;         // Hex literal
int $bin   = 0b1010_0101;  // Binary literal with separator
int $oct   = 0o755;        // Octal literal
int $big   = 1_000_000;    // Underscore separator for readability
```

### Arithmetic

```php
int $a = 10;
int $b = 3;

int $sum  = $a + $b;   // 13
int $diff = $a - $b;   // 7
int $prod = $a * $b;   // 30
int $div  = $a / $b;   // 3  (integer division — truncates)
int $mod  = $a % $b;   // 1
int $pow  = $a ** $b;  // 1000
```

### Conversion

```php
int $parsed  = int("42");      // 42
int $fromBool = int(true);     // 1
int $fromFloat = int(3.99);    // 3  (truncates — does NOT round)
```

---

## `float` — The Float Primitive

`float` is a reserved lowercase keyword representing a **32-bit floating-point number** (`i32` storage) by default.

> [!NOTE]
> Despite being called "float", the default precision is 32-bit (`i32`), not 64-bit (`f64`). If you need full IEEE-754 double precision, use `Float::f64(...)`.

```php
float $rate    = 9;       // i32 storage — whole numbers are fine
float $score   = 100;     // i32 storage
float $partial = 3;       // i32 — fractional values need Float::f32/f64 for precision
```

### Arithmetic

```php
float $a = 10;
float $b = 3;

float $sum  = $a + $b;   // 13
float $prod = $a * $b;   // 30
float $div  = $a / $b;   // 3 (integer result since both are i32)
```

### When to upgrade to `Float::f64`

Use the `Float` facade whenever you need:
- Fractional precision (e.g. `3.14`, `99.99`)
- Large scientific ranges
- IEEE-754 rounding semantics

```php
// Instead of:
float $pi = 3;  // loses precision — just stores 3

// Use:
$pi = Float::f64(3.141592653589793);  // Full 64-bit IEEE-754
```

### Conversion

```php
float $f = float("3");     // 3.0 (i32 storage)
float $g = float(true);    // 1
float $h = float("abc");   // 0 (non-numeric string → 0)
```

---

## Numeric Literal Edge Cases & Strict Validation

RHP's lexer enforces rigorous syntax verification on numeric literals to prevent ambiguities with string concatenation (`.`) and member access (`.`):

### 1. No Trailing Separators or Decimal Points
Numeric literals cannot end with an underscore (`_`) or a bare decimal point (`.`):
```php
// ❌ Rejected by lexer:
// int $a = 10_; // Error: Numeric literal cannot end with a separator or decimal point.
// float $b = 10.; // Error: Numeric literal cannot end with a separator or decimal point.

// ✅ Valid:
int $a = 10;
float $b = 10.0;
int $c = 1_000_000;
```
When violated, the compiler halts with:
```text
ParseError: Numeric literal cannot end with a separator or decimal point.
```

### 2. Leading Dots Disallowed
Omitting the leading zero in fractional numbers is strictly prohibited. You must write `0.5`, not `.5`:
```php
// ❌ Rejected by compiler:
// float $half = .5; // Syntax error

// ✅ Valid:
float $half = 0.5;
```
This eliminates parsing collisions where a leading dot could be conflated with the string concatenation operator (`$str . 5`).

### 3. Incomplete Base Prefixes & Exponents
Base prefixes (`0x`, `0b`, `0o`) and scientific exponent markers (`1e`, `2E`) must be followed by valid digits:
```php
// ❌ Rejected:
// $badHex = 0x;  // SyntaxError: invalid syntax.
// $badExp = 1e;  // SyntaxError: invalid syntax.

// ✅ Valid:
$hex = 0xFF;
$bin = 0b1010;
$sci = 1e6;
```

---

## `Int` — Static Facade Class

`Int` is an **identifier** (capitalized), not a keyword. It is a **static facade class** — meaning it can never be instantiated with `new`. All access is through static method calls (`Int::method(...)`).

> [!CAUTION]
> `new Int()` is a **fatal compile-time error**:
> ```
> Fatal Error: Cannot instantiate static facade class 'Int'. Use static methods directly.
> ```

### Factory Methods — Extended Rust Integer Types

| Method | Rust Type | Range |
| :--- | :--- | :--- |
| `Int::i8($val)` | `i8` | −128 to 127 |
| `Int::i16($val)` | `i16` | −32,768 to 32,767 |
| `Int::i32($val)` | `i32` | −2,147,483,648 to 2,147,483,647 |
| `Int::i64($val)` | `i64` | −9,223,372,036,854,775,808 to 9,223,372,036,854,775,807 |
| `Int::i128($val)` | `i128` | ±170 undecillion |
| `Int::u8($val)` | `u8` | 0 to 255 |
| `Int::u16($val)` | `u16` | 0 to 65,535 |
| `Int::u32($val)` | `u32` | 0 to 4,294,967,295 |
| `Int::u64($val)` | `u64` | 0 to 18,446,744,073,709,551,615 |
| `Int::u128($val)` | `u128` | 0 to ±340 undecillion |
| `Int::usize($val)` | `usize` | Platform pointer-width |
| `Int::isize($val)` | `isize` | Platform pointer-width, signed |

### Usage Examples

```php
// 64-bit integer for database IDs, timestamps, crypto
$id        = Int::i64(9_223_372_036_854_775_807);
$timestamp = Int::i64(1_700_000_000);

// Unsigned types for bitmasks, network ports, memory sizes
$port    = Int::u16(8080);
$flags   = Int::u32(0xDEAD_BEEF);
$memSize = Int::usize(1024 * 1024);  // 1 MiB

// Fluent chaining
$result = Int::i64()
    ->random()      // generate random i64
    ->add(10)
    ->subtract(3)
    ->abs();

// Formatting
$hex = Int::i64(255)->toHex();  // "ff"
$bin = Int::i32(5)->toBin();    // "101"
$oct = Int::u32(8)->toOct();    // "10"
```

---

## `Float` — Static Facade Class

`Float` is an **identifier** (capitalized), not a keyword. Like `Int`, it is a **static facade class** — never instantiated with `new`.

> [!CAUTION]
> `new Float()` is a **fatal compile-time error**:
> ```
> Fatal Error: Cannot instantiate static facade class 'Float'. Use static methods directly.
> ```

### Factory Methods — Extended Rust Float Types

| Method | Rust Type | Precision |
| :--- | :--- | :--- |
| `Float::f32($val)` | `f32` | 32-bit IEEE-754 single precision (~7 decimal digits) |
| `Float::f64($val)` | `f64` | 64-bit IEEE-754 double precision (~15 decimal digits) |

### Usage Examples

```php
// Single precision (f32) — good for graphics, game physics
$angle  = Float::f32(1.5707963);   // π/2
$red    = Float::f32(0.93);

// Double precision (f64) — good for finance, science, GPS coordinates
$price   = Float::f64(99.99);
$pi      = Float::f64(3.141592653589793);
$lat     = Float::f64(37.774929);

// Fluent formatting & rounding
$formatted = Float::f64(1234567.891)->toDecimal(2);   // "1234567.89"
$pct       = Float::f64(0.856)->toPercent();           // "85.60%"
$sci       = Float::f64(0.000012)->toScientific();     // "1.2e-5"

// Arithmetic chaining
$result = Float::f64(3.14)
    ->multiply(2.0)
    ->add(1.0)
    ->round(4);
```

---

## Choosing the Right Numeric Type

```
Simple whole numbers (counters, IDs up to ~2 billion)  →  int
Simple quantities that may need rounding later          →  float  (upgrade to Float::f64)
Large IDs, timestamps, 64-bit crypto                   →  Int::i64
Financial values, GPS, scientific computation          →  Float::f64
Bitmasks, network, buffer sizes                        →  Int::u32 / Int::usize
Graphics, game physics (OK with ~7 sig figs)           →  Float::f32
```

---

## Int / Float as Type Annotations vs. Identifiers

The lexer disambiguates `int`/`float` (keywords) from `Int`/`Float` (identifiers) purely by capitalization:

```php
// Type annotation — keywords (lowercase)
function sum(int $a, float $b): float {
    return $a + $b;   // both are i32 internally
}

// Static facade — identifiers (PascalCase)
$precise = Float::f64(sum(10, 3))->toDecimal(2);
```

Attempting to use `Int` or `Float` as a type annotation triggers a parse error:

```
ParseError: 'Int' is not a valid type keyword. Use lowercase 'int' for the primitive type,
or 'Int::' for static facade methods.
```

---

## Numeric Literal Syntax

RHP supports the following numeric literal forms:

```php
// Decimal integers (with optional underscore separators)
$million = 1_000_000;
$negative = -42;

// Hexadecimal
$hex = 0xFF;       // 255
$hex2 = 0xDEAD_BEEF;

// Binary
$bits = 0b1010_0101;   // 165

// Octal
$perm = 0o755;         // 493

// Float literals (require Float:: for full precision)
// Plain float keyword stores these as i32 — use Int::i32() or Float::f64() instead
$pi = Float::f64(3.14159);
$exp = Float::f64(1.5e10);    // Scientific notation
$neg = Float::f64(-0.001);
```
