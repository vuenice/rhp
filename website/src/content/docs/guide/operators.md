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

RHP enforces **strict, type-safe equality** by design to eradicate subtle bugs caused by PHP's legacy implicit type coercion.

| Operator | Name | Syntax | Description | Example |
| :--- | :--- | :--- | :--- | :--- |
| `<` | **Less than** | `$a < $b` | `true` if `$a` is strictly less than `$b` | `5 < 10` $\rightarrow$ `true` |
| `>` | **Greater than** | `$a > $b` | `true` if `$a` is strictly greater than `$b` | `10 > 5` $\rightarrow$ `true` |
| `<=` | **Less than or equal** | `$a <= $b` | `true` if `$a` is less than or equal to `$b` | `5 <= 5` $\rightarrow$ `true` |
| `>=` | **Greater than or equal** | `$a >= $b` | `true` if `$a` is greater than or equal to `$b` | `10 >= 8` $\rightarrow$ `true` |
| `==` | **Strict Type-Safe Equality** | `$a == $b` | `true` if both operands share the same type and have equal values | `10 == 10` $\rightarrow$ `true` |
| `!=` or `<>` | **Strict Type-Safe Inequality** | `$a != $b` | `true` if values differ | `10 != 20` $\rightarrow$ `true` |
| `<=>` | **Spaceship** | `$a <=> $b` | Returns `-1` if `$a < $b`, `0` if `$a == $b`, `1` if `$a > $b` | `5 <=> 10` $\rightarrow$ `-1` |

> [!IMPORTANT]
> **No `===` or `!==` in RHP:** The compiler deliberately does not support `===` or `!==` (emits error `RHP0116`). Because `==` and `!=` in RHP are already 100% strict and type-safe, the triple-equals syntax is redundant. Running `rhp run lint` automatically converts all `===` to `==` and `!==` to `!=`.

> [!WARNING]
> **Mismatched Types Cause Compile-Time Errors:** If you attempt to compare operands of incompatible types using `==` or `!=` (e.g. `42 == "42"` or `true == 1`), the compiler emits a compile-time type error (`InvalidBinaryOperation`).

### Float Precision Comparison via `==`

Comparing floating-point numbers in traditional languages often suffers from IEEE-754 representation inaccuracies (e.g., `3.14f32` vs `3.14f64`).

In RHP, when comparing floats via `==` or `!=`:
1. The engine checks the **maximum decimal point (places)** across both sides:
   $$D_{\max} = \max(D_{\text{left}}, D_{\text{right}})$$
2. Both sides are formatted and compared up to that exact decimal point.

```php
// Both have 2 decimal points -> true
bool $match = (1.23 == 1.23); // true

// 1.23 (2 decimals) vs 1.234 (3 decimals) -> compared at 3 decimals (1.230 != 1.234)
bool $diff = (1.23 == 1.234); // false

// Float comparison with dynamic decimal precision
float $valA = 3.14;
float $valB = 3.140;
bool $equal = ($valA == $valB); // true (both verified to matching precision)
```

---

## Loose Comparison Helper Functions

When loose or cross-type comparison is explicitly intended, RHP provides dedicated static facade methods instead of unpredictable operator coercion:

```php
// Boolean truthiness comparison
Bool::compare(1, true);       // true (1 is truthy)
Bool::compare("0", false);    // true ("0" is falsy)
Bool::compare(null, false);   // true (null is falsy)

// String comparison with optional case-insensitivity
Str::compare("hello", "hello");        // true
Str::compare("hello", "HELLO");        // false
Str::compare("hello", "HELLO", true);  // true (case-insensitive)
Str::compare(42, "42");                // true (converts 42 to string)

// Integer loose comparison
Int::compare(42, "42");       // true
Int::compare(true, 1);        // true
Int::compare(false, 0);       // true
Int::compare(42, 43);         // false

// Float loose comparison with optional epsilon tolerance
Float::compare(3.14, "3.14");                  // true
Float::compare(1.00001, 1.00002);              // false
Float::compare(1.00001, 1.00002, 0.001);       // true (within epsilon)
```

---

## Array & Collection Comparison Functions

To compare collections across different underlying types (`array`, `list`, `set`, `dict`, `hashMap`), use the static comparison functions available on `Array` (or `Arr`):

```php
$listA = [1, 2, 3];
$listB = [1, 2, 3];
$listC = [3, 2, 1];

// 1. JSON Stringify and Compare
// Serializes both structures to canonical JSON and compares representations
Array::compareJson($listA, $listB); // true
Array::compareJson($listA, $listC); // false

// 2. Memory Address / Reference Comparison
// Checks if two variables refer to the exact same memory allocation (pointer equality)
$ref1 = [10, 20];
$ref2 = $ref1;
$ref3 = [10, 20];
Array::compareRef($ref1, $ref2); // true (same memory allocation)
Array::compareRef($ref1, $ref3); // false (distinct allocations)

// 3. Delimited Joined String Comparison
// Converts elements to a delimited string (e.g. [1, 2, 3] -> '1,2,3') and compares
Array::compareJoined([1, 2, 3], ['1', '2', '3']);        // true (default separator ',')
Array::compareJoined([1, 2, 3], ['1', '2', '3'], '-');    // true (custom separator '-')

// 4. Unordered Value Comparison (Strictly supports list and set only)
// Compares elements irrespective of order; rejects dict/hashmap with runtime error
set $setA = [1, 2, 3];
set $setB = [3, 1, 2];
Array::compareUnordered($listA, $listC); // true (list vs list, order ignored)
Array::compareUnordered($listA, $setB);  // true (list vs set, order ignored)
Array::compareUnordered($setA, $setB);   // true (set vs set, order ignored)
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
| `+` | **Checked Addition** | `$a + $b` | Sum of operands. **Always checked for overflow** in all build modes (traps on overflow). |
| `+%` | **Wrapping Addition** | `$a +% $b` | Two's-complement modular addition (wrapping on overflow). |
| `+\|` | **Saturating Addition** | `$a +\| $b` | Clamps result to `MIN` / `MAX` bound on overflow. |
| `-` | **Subtraction** | `$a - $b` | Difference of operands (checked for underflow). |
| `*` | **Multiplication** | `$a * $b` | Product of operands (checked for overflow). |
| `/` | **Division** | `$a / $b` | Quotient of operands. Integer division truncates; division by zero traps. |
| `%` | **Modulo** | `$a % $b` | Integer remainder of division. |
| `**` | **Exponentiation** | `$a ** $b` | Base raised to exponent (**Right-associative**). |

```php
int $a = 15;
int $b = 4;

int $sum  = $a + $b; // 19 (Checked: traps if $a + $b overflows)
int $diff = $a - $b; // 11
int $prod = $a * $b; // 60
int $rem  = $a % $b; // 3
int $pow  = 2 ** 3;  // 8

// Overflow-handling operators:
u8 $wrapped = 255u8 +% 1u8; // 0u8 (wrapping)
u8 $clamped = 250u8 +| 20u8; // 255u8 (saturating)
```

> [!IMPORTANT]
> **No Implicit Coercion & No Silent Overflow:**
> - `+` supports **strictly typed addition and concatenation**:
>   - Numeric addition: operands must be numeric (`int + int`, `float + float`).
>   - String concatenation: both operands must be strings (`string + string`).
> - Mixed-type addition/concatenation (`string + int`, `int + string`, `null + int`, `bool + int`) is rejected with actionable compile-time errors.
> - String concatenation assignment uses `+=` (`$msg += " world"`).
> - Array merging **must** use `array_merge()` or spread `[...$a, ...$b]`.
> - The legacy `.` operator for string concatenation has been **removed** from RHP.
> - Constant expressions that overflow are flagged as **compile-time errors**, never deferred to runtime.
> - For full specifications, LLVM codegen details, and custom `Add` traits, see [Safe Addition & Arithmetic Semantics](/docs/advanced/safe-addition-semantics).

---

## Assignment Operators

RHP supports simple assignment and compound assignment shortcuts:

| Operator | Syntax | Equivalent | Description |
| :--- | :--- | :--- | :--- |
| `=` | `$x = $y` | `$x = $y` | Assigns value of right operand to left target |
| `+=` | `$x += $y` | `$x = $x + $y` | Addition / string append assignment |
| `-=` | `$x -= $y` | `$x = $x - $y` | Subtraction assignment |
| `*=` | `$x *= $y` | `$x = $x * $y` | Multiplication assignment |
| `/=` | `$x /= $y` | `$x = $x / $y` | Division assignment |
| `%=` | `$x %= $y` | `$x = $x % $y` | Modulo assignment |
| `**=` | `$x **= $y` | `$x = $x ** $y` | Exponentiation assignment |
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

## String Concatenation Operator (`+`)

In RHP, strings are concatenated using the `+` operator with strict type checking (both operands must be strings):

```php
string $first = "Fast";
string $second = "Backend";
string $full = $first + " " + $second; // "Fast Backend"

string $buffer = "Log: ";
$buffer += "Operation complete.";
```

> [!NOTE]
> The legacy PHP dot operator (`.`) and dot-assign (`.=`) have been removed from RHP. Using `.` or `.=` results in a compile-time parse error directing developers to `+` and `+=`.

---

## Type Testing & Error Control Operators

### `instanceof` (Type Instance Verification)

Tests whether an object implements an interface or derives from a class:

```php
if ($handler instanceof RequestHandler) {
    $handler->handle($request);
}
```

### `@` (Error Suppression & Safe Evaluation Operator)

In RHP, the unary prefix `@` operator evaluates fallible expressions safely without throwing uncaught exceptions. Instead of indiscriminately silencing errors or altering engine globals, `@expr` produces a structured 2-element tuple:

$$\text{@expr} \longrightarrow (T \mid \text{null},\; \text{Throwable} \mid \text{null})$$

```php
[$result, $err] = @helperFun();
```

#### Key Semantics:
1. **Failure slot (`$err`)**: Populated with the thrown `Throwable` or `Exception` object on failure; evaluates to `null` on success.
2. **Success slot (`$result`)**: Contains the evaluated value of the expression on success; evaluates to `null` on failure.
3. **Void return type**: Functions returning `void` return `null` as their result type:
   ```php
   [$result, $err] = @saveLogs(); // [$result: null, $err: null] on success
   ```
4. **Single-slot destructuring**: Checking the second slot is optional:
   ```php
   [$result] = @unlink("cache.tmp");
   ```
5. **Checked exceptions only**: `@` captures thrown exceptions and throwables. It does not swallow or capture notices, warnings, or compile-time type errors as the second slot (second slot remains `null` in those cases). If a function returns `false` or `null` on failure without throwing an exception, it is the caller's responsibility to check `$result` accordingly.
6. **Direct tuple assignment**:
   ```php
   $tuple = @compute();
   if ($tuple[1] != null) {
       echo "Failed: " + $tuple[1]->getMessage();
   } else {
       echo "Result: " + $tuple[0];
   }
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
| **Additive & String** | `(75, 76)` | `+`, `-` | Left-to-right | Addition, subtraction, string concatenation (`+`) |
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
| **Assignment** | `(20, 19)` | `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `??=`, `&=`, `\|=`, `^=`, `<<=`, `>>=` | **Right-to-left** | Variable assignment (`$a = $b = 10`) |
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
| `Dot` | `.` | 1 character | Removed operator (fails compilation; use `+` for strings) |

### Disambiguation Behavior

```php
// Lexed as Variadic/Ellipsis (longest match):
function sum(int ...$values): int {}

// Lexed as Range:
foreach (1..5 as $i) {}

// String concatenation uses '+' (dot operator is rejected):
$label = "Item" + $idStr;

// Whitespace-free maximal munch:
// $a...$b is lexed as: [$a, ..., $b] (Ellipsis), NOT [$a, .., ., $b]
```

To avoid ambiguity, format multi-character expressions with clean whitespace (e.g., `$first + $second`).

---

## Key Differences from PHP (Unsupported Operators)

RHP intentionally removes or replaces specific legacy PHP operator behaviors:

| PHP Operator Feature | Status in RHP | Reason | Modern RHP Alternative |
| :--- | :--- | :--- | :--- |
| **String Concatenation `.` and `.=`** | ❌ **Rejected** (`ParseError`) | Replaced by `+` and `+=` with strict type checking (disallowing implicit string/numeric coercion) | Use `+` for string concatenation, `+=` for append, or backticks/double-quotes interpolation |
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
