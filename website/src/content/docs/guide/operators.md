---
title: "Operators"
description: "Comprehensive reference guide for arithmetic, comparison, logical, bitwise, assignment, ternary, Elvis, and null-coalescing operators in RHP."
section: "Language Guide"
order: 2
---

# Operators in RHP

RHP supports standard PHP-compatible operators for arithmetic, comparison, logic, bit manipulation, and string manipulation. Expressions are evaluated in linear time $O(N)$ using a custom Pratt parser (Top-Down Operator Precedence) with deterministic binding powers and zero-cost compile-time type validation.

---

## Comparison & Relational Operators

RHP provides a full suite of comparison and relational operators. Comparisons evaluate to boolean `true` or `false` (or `-1`, `0`, `1` in the case of the spaceship operator).

| Operator | Name | Syntax | Description | Example |
| :--- | :--- | :--- | :--- | :--- |
| `<` | **Less than** | `$a < $b` | `true` if `$a` is strictly strictly less than `$b` | `5 < 10` $\rightarrow$ `true` |
| `>` | **Greater than** | `$a > $b` | `true` if `$a` is strictly greater than `$b` | `10 > 5` $\rightarrow$ `true` |
| `<=` | **Less than or equal** | `$a <= $b` | `true` if `$a` is less than or equal to `$b` | `5 <= 5` $\rightarrow$ `true` |
| `>=` | **Greater than or equal** | `$a >= $b` | `true` if `$a` is greater than or equal to `$b` | `10 >= 8` $\rightarrow$ `true` |
| `==` | **Loose Equality** | `$a == $b` | `true` if values are equivalent after loose coercion | `10 == "10"` $\rightarrow$ `true` |
| `!=` or `<>` | **Loose Inequality** | `$a != $b` | `true` if values are not equivalent | `10 != 20` $\rightarrow$ `true` |
| `===` | **Strict Identity** | `$a === $b` | `true` if values match and share the identical datatype | `10 === 10` $\rightarrow$ `true` |
| `!==` | **Strict Non-identity** | `$a !== $b` | `true` if values differ or datatypes do not match | `10 !== "10"` $\rightarrow$ `true` |
| `<=>` | **Spaceship** | `$a <=> $b` | Returns `-1` if `$a < $b`, `0` if `$a == $b`, `1` if `$a > $b` | `5 <=> 10` $\rightarrow$ `-1` |

### Comparison Examples

```php
int $count = 42;
int $limit = 50;

// Relational bounds checking (< and >)
if ($count > 0 && $count < $limit) {
    print("Count is within valid bounds.\n");
}

// Relational boundary checking (<= and >=)
if ($count >= 42) {
    print("Reached or exceeded quota.\n");
}

// Strict identity check (=== vs ==)
int $id = 100;
string $rawId = "100";

if ($id == $rawId) {
    print("Equivalent values\n"); // Prints
}

if ($id === $rawId) {
    // Will not execute: int does not strictly match string
}

// Spaceship operator (<=>) for custom sorting
int $cmp = $count <=> 100; // -1 because 42 < 100
```

---

## Conditional, Ternary & Null-Coalescing Operators

RHP provides concise operators for branching, fallbacks, and null-safe evaluations.

| Operator | Name | Syntax | Description |
| :--- | :--- | :--- | :--- |
| `? :` | **Ternary** | `$cond ? $then : $else` | Evaluates `$then` if `$cond` is truthy, otherwise `$else` |
| `?:` | **Elvis (Short Ternary)** | `$expr ?: $fallback` | Returns `$expr` if truthy, otherwise evaluates `$fallback` |
| `??` | **Null-Coalescing** | `$expr ?? $fallback` | Returns `$expr` if not `null`, otherwise evaluates `$fallback` |
| `??=` | **Null-Coalescing Assign** | `$target ??= $val` | Assigns `$val` into `$target` only if `$target` is currently `null` |
| `?->` | **Nullsafe Member Access** | `$obj?->prop`, `$obj?->method()` | Chains method/property access safely; evaluates to `null` if `$obj` is null |

### Ternary Operator (`? :`)

The standard ternary operator allows inline conditional branch selection:

```php
bool $is_admin = true;
string $role = $is_admin ? 'SuperAdmin' : 'Member';

int $score = 85;
string $grade = ($score >= 90) ? 'A' : (($score >= 80) ? 'B' : 'C');
```

### Elvis Operator (`?:`)

The **Elvis operator** (or short ternary) evaluates the condition once. If truthy, the condition value itself is returned; otherwise, the fallback value is returned:

```php
string $user_input = "";
string $display_name = $user_input ?: 'Anonymous'; // 'Anonymous' (empty string is falsy)

string $configured_host = "db.internal";
string $host = $configured_host ?: 'localhost';     // 'db.internal'
```

> [!TIP]
> **Difference between `??` and `?:`:**
> * `??` (Null-Coalesce) **only** triggers fallback when the value is strictly `null`. Values like `""` (empty string), `0`, or `false` are preserved.
> * `?:` (Elvis) triggers fallback on **any falsy** value (`null`, `false`, `0`, `""`, `[]`).

```php
string $title = "";

// Elvis treats empty string as falsy:
string $a = $title ?: 'Default Title'; // 'Default Title'

// Null-coalesce preserves empty string:
string $b = $title ?? 'Default Title'; // ""
```

### Null-Coalescing Assignment (`??=`)

```php
string $cache_dir = null;
$cache_dir ??= '/tmp/rhp_cache'; // Assigned: '/tmp/rhp_cache'

$cache_dir ??= '/var/cache';     // Ignored: already non-null
```

### Nullsafe Operator (`?->`)

Avoid nested null checks with fluent nullsafe property and method calls:

```php
class User {
    public function getProfile(): Profile|null { return null; }
}

User|null $user = null;
string|null $bio = $user?->getProfile()?->bio; // Resolves safely to null without errors
```

---

## Arithmetic Operators

RHP supports standard mathematical operators with compile-time type validation:

| Operator | Name | Syntax | Description |
| :--- | :--- | :--- | :--- |
| `+` | **Addition** | `$a + $b` | Sum of operands |
| `-` | **Subtraction** | `$a - $b` | Difference of operands |
| `*` | **Multiplication** | `$a * $b` | Product of operands |
| `/` | **Division** | `$a / $b` | Quotient of operands (returns float if non-integer) |
| `%` | **Modulo** | `$a % $b` | Integer remainder of division |
| `**` | **Exponentiation** | `$a ** $b` | Base raised to exponent (**Right-associative**) |

```php
int $a = 15;
int $b = 4;

int $sum  = $a + $b; // 19
int $diff = $a - $b; // 11
int $prod = $a * $b; // 60
int $rem  = $a % $b; // 3
int $pow  = 2 ** 3;  // 8
```

> [!IMPORTANT]
> In RHP, `+` is strictly mathematical and requires numeric operands (`int`, `float`, `i64`, `f64`). Using `+` for array unions or string concatenations is rejected at compile time.

---

## Assignment Operators

RHP supports simple assignment and compound assignment shortcuts:

| Operator | Syntax | Equivalent | Description |
| :--- | :--- | :--- | :--- |
| `=` | `$x = $y` | `$x = $y` | Assigns value of right operand to left target |
| `+=` | `$x += $y` | `$x = $x + $y` | Addition assignment |
| `-=` | `$x -= $y` | `$x = $x - $y` | Subtraction assignment |
| `*=` | `$x *= $y` | `$x = $x * $y` | Multiplication assignment |
| `/=` | `$x /= $y` | `$x = $x / $y` | Division assignment |
| `%=` | `$x %= $y` | `$x = $x % $y` | Modulo assignment |
| `**=` | `$x **= $y` | `$x = $x ** $y` | Exponentiation assignment |
| `.=` | `$x .= $y` | `$x = $x . $y` | String concatenation assignment |
| `??=` | `$x ??= $y` | `$x = $x ?? $y` | Null-coalescing assignment |
| `&=` | `$x &= $y` | `$x = $x & $y` | Bitwise AND assignment |
| `\|=` | `$x \|= $y` | `$x = $x \| $y` | Bitwise OR assignment |
| `^=` | `$x ^= $y` | `$x = $x ^ $y` | Bitwise XOR assignment |
| `<<=` | `$x <<= $y` | `$x = $x << $y` | Shift left assignment |
| `>>=` | `$x >>= $y` | `$x = $x >> $y` | Shift right assignment |

Assignment expressions are right-associative, allowing chaining:

```php
int $a = 0;
int $b = 0;
$a = $b = 100; // $a and $b are both 100
```

---

## Logical Operators

RHP supports both symbolic short-circuit operators and low-precedence word operators:

| Operator | Syntax | Associativity | Precedence | Description |
| :--- | :--- | :--- | :--- | :--- |
| `!` | `!$a` | Right | Prefix | Logical NOT (inverts truthiness) |
| `&&` | `$a && $b` | Left | High | Short-circuit logical AND |
| `\|\|` | `$a \|\| $b` | Left | High | Short-circuit logical OR |
| `and` | `$a and $b` | Left | Low | Low-precedence logical AND |
| `or` | `$a or $b` | Left | Low | Low-precedence logical OR |
| `xor` | `$a xor $b` | Left | Low | Logical XOR (`true` if exactly one operand is truthy) |

```php
bool $authenticated = true;
bool $has_permission = false;

// High-precedence short-circuit:
if ($authenticated && $has_permission) {
    print("Granted");
}

// Low-precedence word operators:
$result = $authenticated or $has_permission;
```

---

## Bitwise Operators

Bitwise operators manipulate integer bits directly:

| Operator | Name | Syntax | Description |
| :--- | :--- | :--- | :--- |
| `&` | **Bitwise AND** | `$a & $b` | Bits set in both operands |
| `\|` | **Bitwise OR** | `$a \| $b` | Bits set in either operand |
| `^` | **Bitwise XOR** | `$a ^ $b` | Bits set in one but not both |
| `~` | **Bitwise NOT** | `~$a` | Inverts all bits |
| `<<` | **Shift Left** | `$a << $b` | Shifts bits left by `$b` places |
| `>>` | **Shift Right** | `$a >> $b` | Shifts bits right by `$b` places |

```php
int $flags = 0b0010;
int $mask  = 0b0011;

int $and = $flags & $mask; // 0b0010 (2)
int $or  = $flags | 0b0100; // 0b0110 (6)
int $shl = 1 << 4;         // 16
```

---

## Increment & Decrement Operators

RHP supports pre-increment, post-increment, pre-decrement, and post-decrement:

```php
int $x = 5;

// Post-increment: returns original value, then increments
int $post = $x++; // $post is 5, $x is now 6

// Pre-increment: increments first, then returns new value
int $pre = ++$x;  // $pre is 7, $x is 7

// Decrement operators
$x--; // 6
--$x; // 5
```

---

## String Concatenation Operator (`.`)

Consistent with PHP syntax, strings are concatenated using the dot (`.`) operator:

```php
string $first = "Fast";
string $second = "Backend";
string $full = $first . " " . $second; // "Fast Backend"

string $buffer = "Log: ";
$buffer .= "Operation complete.";
```

---

## Type Testing & Error Control Operators

### `instanceof` (Type Instance Verification)

Tests whether an object implements an interface or derives from a class:

```php
if ($handler instanceof RequestHandler) {
    $handler->handle($request);
}
```

### `@` (Error Suppression Operator)

RHP parses the unary `@` operator as an expression silence modifier:

```php
$val = @$service->optionalOperation();
```

> [!NOTE]
> In RHP, `@` is strictly restricted to expressions. Template directives starting with `@` (such as Blade's `@if` or `@foreach`) in RHP code files will trigger a parse error indicating they are not permitted in raw RHP source.

---

## Pratt Parser Precedence Hierarchy

Expressions are parsed in linear time $O(N)$ using a custom Pratt parser (Top-Down Operator Precedence). Each operator possesses strictly calibrated left and right binding powers (`left_bp`, `right_bp`) guaranteeing mathematical and logical associativity:

| Precedence Category | Binding Power (L, R) | Operators | Associativity | Description |
| :--- | :--- | :--- | :--- | :--- |
| **Member Access & Calls** | `(100, 101)` | `->`, `?->`, `::`, `( )`, `[ ]` | Left-to-right | Property fetch, nullsafe, static access, call, index |
| **Postfix Operators** | `95` | `++`, `--` | Left-to-right | Post-increment and post-decrement |
| **Prefix Unary** | `93` | `+`, `-`, `!`, `~`, `++`, `--`, `@`, `clone`, `await` | Right-to-left | Unary signs, bitwise/logical NOT, pre-inc/dec, silence |
| **Exponentiation** | `(91, 90)` | `**` | **Right-to-left** | Power operator (`2 ** 3 ** 4` $\rightarrow$ `2 ** (3 ** 4)`) |
| **Type Check** | `(85, 86)` | `instanceof` | Left-to-right | Class and interface verification |
| **Multiplicative** | `(80, 81)` | `*`, `/`, `%` | Left-to-right | Multiplication, division, modulo |
| **Additive & String** | `(75, 76)` | `+`, `-`, `.` | Left-to-right | Addition, subtraction, string concatenation |
| **Bitwise Shift** | `(70, 71)` | `<<`, `>>` | Left-to-right | Bitwise left and right shift |
| **Relational Comparison** | `(65, 66)` | `<`, `<=`, `>`, `>=`, `<=>` | Left-to-right | Less, greater, bound, spaceship comparison |
| **Equality Check** | `(60, 61)` | `==`, `!=`, `===`, `!==`, `<>` | Left-to-right | Loose equivalence and strict identity |
| **Bitwise AND** | `(55, 56)` | `&` | Left-to-right | Bitwise AND |
| **Bitwise XOR** | `(50, 51)` | `^` | Left-to-right | Bitwise XOR |
| **Bitwise OR** | `(45, 46)` | `\|` | Left-to-right | Bitwise OR |
| **Logical AND** | `(40, 41)` | `&&` | Left-to-right | Short-circuit boolean AND |
| **Logical OR** | `(35, 36)` | `\|\|` | Left-to-right | Short-circuit boolean OR |
| **Null Coalescing** | `(30, 29)` | `??` | **Right-to-left** | Fallback chaining (`$a ?? $b ?? $default`) |
| **Ternary & Elvis** | `(25, 24)` | `? :`, `?:` | **Right-to-left** | Conditional expression and Elvis fallback |
| **Assignment** | `(20, 19)` | `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `.=`, `??=`, `&=`, `\|=`, `^=`, `<<=`, `>>=` | **Right-to-left** | Variable assignment (`$a = $b = 10`) |
| **Logical Word AND** | `(9, 10)` | `and` | Left-to-right | Low-precedence logical AND |
| **Logical Word XOR** | `(7, 8)` | `xor` | Left-to-right | Low-precedence logical XOR |
| **Logical Word OR** | `(5, 6)` | `or` | Left-to-right | Low-precedence logical OR |
| **Generator Yield** | `(1, 1)` | `yield`, `yield from` | Non-associative | Yield value or delegate to generator |
| **Arrow Closures** | `0` (Lowest) | `fn` | Non-associative | Single-expression arrow function (`fn($n) => $n * 2`) |

### Disambiguation Examples

1. **`??` vs `?:` Precedence:**
   ```php
   // Evaluates as: ($a ?? $b) ?: $c
   $result = $a ?? $b ?: $c;
   ```
   Because `??` has binding power **30** and `?:` has binding power **25**, the null-coalescing sub-expression binds tighter.

2. **`??` vs `??=` Precedence:**
   ```php
   // Evaluates as: $a ??= ($b ?? $c)
   $a ??= $b ?? $c;
   ```
   `??=` (binding power **20**) yields to `??` (binding power **30**), ensuring right-hand expressions resolve completely before assignment.

3. **Compound Precedence Chaining:**
   ```php
   // Pratt parser resolves this strictly without parentheses:
   // 1. ?? binds first
   // 2. ?: binds second
   // 3. ??= binds last
   $target ??= $primary ?? $fallback ?: 'default_value';
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
function sum(int ...$values): int {}

// Lexed as Range:
foreach (1..5 as $i) {}

// Lexed as Dot concatenation:
$label = "Item" . $id;

// Whitespace-free maximal munch:
// $a...$b is lexed as: [$a, ..., $b] (Ellipsis), NOT [$a, .., ., $b]
```

To avoid ambiguity, format multi-character expressions with clean whitespace (e.g., `$first . $second`).

---

## Key Differences from PHP (Unsupported Operators)

RHP intentionally removes or replaces specific legacy PHP operator behaviors:

| PHP Operator Feature | Status in RHP | Reason | Modern RHP Alternative |
| :--- | :--- | :--- | :--- |
| **Type Casts `(int)`, `(string)`, `(float)`, `(bool)`** | ❌ **Rejected** (`ParseError`) | Ambiguous syntax with parentheses; lack of strict type inference | Use functional conversion: `int($x)`, `string($x)`, `float($x)`, `bool($x)` |
| **Array Union Operator `+` (`$a + $b`)** | ❌ **Rejected** (`TypeError`) | High risk of unintended key overwrites and non-intuitive coercion | Use `Arr::merge($a, $b)` or `dict` methods |
| **Execution Operator (Backticks `` `ls` ``)** | ❌ **String literal only** | Security hazard and hidden subprocess spawning | Use `Process::run(['ls'])` or `System::exec(...)` |
| **Reference Assignment `=&` and `&$var`** | ❌ **Rejected** | Violates memory safety and zero-GC ownership invariants | Use value returns or encapsulate in class/object containers |
| **Double Dollar `$$var`** | ❌ **Rejected** (`LexError`) | Dynamic variable variables prevent compile-time optimizations | Use explicit dictionaries (`$dict[$key]`) |

### Example: Cast Syntax Migration

```php
// ❌ Rejected in RHP:
// int $num = (int)$request->get('id'); // Compile-time ParseError

// ✅ Modern RHP Syntax:
int $num = int($request->get('id'));
string $str = string(12345);
float $amt = float("49.99");
bool $active = bool($config->get('enabled'));
```
