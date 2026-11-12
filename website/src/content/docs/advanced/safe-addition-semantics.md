---
title: "Arithmetic Addition & Safe Operator Semantics"
description: "In-depth specification of RHP's safe addition operator (+) and overflow-handling operators (+%, +|), compile-time checking, constant folding, and LLVM codegen."
section: "Advanced"
order: 4
---

# Safe Addition Semantics & Arithmetic Operators

RHP is a statically compiled programming language with PHP-familiar syntax (`$var`, object arrows `->`), but its execution model is built upon Rust and LLVM/Cranelift. To prevent silent security vulnerabilities, runtime crashes, and catastrophic data corruption, RHP's arithmetic semantics reject legacy PHP implicit coercions and silent overflows.

---

## 1. PHP Traps Deliberately Eliminated

Legacy PHP arithmetic exhibits several notorious behaviors that cause silent bugs, precision loss, and data destruction. RHP eliminates all of them at compile time:

| Legacy PHP Behavior | What PHP Does | Why It Fails | RHP Semantic Invariant |
|:---|:---|:---|:---|
| `"5 apples" + 3` | Silently coerces string to `8` | Hidden type bugs; masking corrupted data payloads | ❌ **Compile Error** (`StringConcatForbidden`). Use `.` for concat, or `int($s)` for explicit conversion. |
| `[1, 2] + [3, 4, 5]` | Array union (left keys win: `[1, 2, 5]`) | Silently drops data when integer indexes clash | ❌ **Compile Error** (`ArrayUnionForbidden`). Use `array_merge()` or spread `[...$a, ...$b]`. |
| `PHP_INT_MAX + 1` | Silently converts integer to `float` | Loses exact precision in 64-bit IDs and finances | ❌ **Checked Trap / Const Overflow**. Never converts to float. Traps with diagnostic context. |
| `null + 1` / `true + 2` | Silently evaluates to `1` and `3` | Null and boolean values masked as numbers | ❌ **Compile Error** (`NonNumericOperand`). Strict type requirements. |
| `$int + $float` | Silently coerces int to float | Implicit precision loss and unexpected promotion | ❌ **Compile Error** (`TypeMismatch`). Requires `$int as float` or `float($int)`. |

---

## 2. Operator Specifications: `+`, `+%`, `+|`

RHP offers three distinct addition operators plus standard library helper methods:

```text
┌──────────────┬───────────────────┬─────────────────────────────────────────────────────────────┐
│ Operator     │ Flavor            │ Overflow Behavior                                           │
├──────────────┼───────────────────┼─────────────────────────────────────────────────────────────┤
│ $a + $b      │ Checked (Default) │ Traps with file, line, and operand values in ALL builds.    │
│ $a +% $b     │ Wrapping          │ Two's-complement modular wrapping (fast inner loops/crypto).│
│ $a +| $b     │ Saturating        │ Clamps at target type's MIN / MAX bounds (DSP/audio/games). │
└──────────────┴───────────────────┴─────────────────────────────────────────────────────────────┘
```

### 2.1 Checked Addition (`+`) — Always Checked in All Build Modes

In both **debug** and **release/production** builds, `+` emits checked arithmetic using target architecture overflow intrinsics (such as `llvm.sadd.with.overflow`). If an overflow occurs at runtime, the execution scope traps with an informative diagnostic message:

```php
int $a = 9_223_372_036_854_775_807; // i64::MAX
int $b = 1;

int $c = $a + $b; 
// Runtime Trap:
// panic: integer overflow at src/payments.rhp:42:15 (lhs: 9223372036854775807, rhs: 1)
```

> [!IMPORTANT]
> **Why Checked in Release Mode?**
> Rust defaults to wrapping in release builds to maintain benchmark throughput. RHP chooses **checked addition by default across all build modes**. In high-concurrency backend services, silent integer overflow causes authentication bypasses, corrupted financial ledgers, and out-of-bounds memory slices. RHP's compiler mitigates branch overhead through Value-Range Propagation (VRP) optimization passes.

### 2.2 Wrapping Addition (`+%`)

When two's-complement modular arithmetic is explicitly desired (e.g. hashing, cryptography, ring buffers):

```php
u8 $val = 255u8;
u8 $next = $val +% 1u8; // Evaluates to 0u8 (no trap)

i8 $signed = 127i8;
i8 $wrapped = $signed +% 1i8; // Evaluates to -128i8
```

### 2.3 Saturating Addition (`+|`)

For DSP, clamped meters, saturation volumes, and score counters:

```php
u8 $health = 250u8;
u8 $restored = $health +| 20u8; // Clamps to 255u8 (u8::MAX)

i8 $level = -120i8;
i8 $dropped = $level +| -20i8;  // Clamps to -128i8 (i8::MIN)
```

### 2.4 Recoverable Arithmetic via Methods

For workflows where overflow is an expected possibility rather than a critical failure, primitive types provide explicit checked methods:

```php
// Returns Option<int> (nullable / Some($v) | null)
$maybeSum = $a->checked_add($b);

if ($maybeSum !== null) {
    print("Sum: {$maybeSum}\n");
} else {
    print("Overflow avoided!\n");
}

// Returns tuple [int $result, bool $overflowed]
[$result, $overflowed] = $a->overflowing_add($b);
```

---

## 3. Strict Type Rules & Literal Inference

Both operands of `+`, `+%`, and `+|` must evaluate to the **exact same concrete numeric type**.

### 3.1 Supported Numeric Types

- **Fixed-width Integers:** `i8`, `i16`, `i32`, `i64` (default `int` is `i64`), `u8`, `u16`, `u32`, `u64`.
- **Floating-point:** `f32`, `f64` (IEEE 754 compliant).
- **Exact Decimal:** `decimal` (exact base-10 for currencies and billing).
- **Arbitrary Precision:** `BigInt` (helper class backed by arbitrary-precision integers).

```php
// ❌ Rejected at compile time:
int $x = 10;
float $y = 2.5;
$res = $x + $y; 
// error: cannot add `i64` and `f64`: mismatched types
//  = help: operands must have identical types; cast explicitly using `$x as float` or `float($x)`
```

### 3.2 Context-Dependent Literal Inference

Unsuffixed integer and float literals adopt the type of the opposing operand, provided the literal value fits within the target type's boundaries:

```php
u8 $counter = 200u8;

// ✅ Valid: 1 is inferred as u8 because it fits in [0, 255]
u8 $next = $counter + 1;

// ❌ Compile-time error: 300 cannot fit into u8
u8 $invalid = $counter + 300;
// error: integer literal `300` out of range for `u8`
//  = help: ensure the literal fits within u8's bounds or declare it with a wider type suffix
```

Explicit suffixes are supported when literal ambiguity must be avoided: `1u8`, `42i16`, `1000u32`, `2.5f32`, `99.99d` (decimal).

---

## 4. Compile-Time Constant Folding

If both operands are known compile-time constants, expressions are folded during compiler optimization. If a constant arithmetic operation overflows, it triggers a **compile-time error**, never deferring the fault to runtime:

```php
const MAX_I64 = 9_223_372_036_854_775_807;

// ❌ Compile-time error:
const OVERFLOWED = MAX_I64 + 1;
// error: attempt to checked add with overflow in constant expression: `9223372036854775807 + 1`
//  = help: constant additions that overflow will fail at compile time; use a wider type or wrapping operator if intentional
```

---

## 5. Floating-Point, Decimal, and BigInt Semantics

### 5.1 Floats (`f32`, `f64`)
- Strictly IEEE 754 compliant.
- Operations that exceed representation limits do not panic or trap; they yield `+inf`, `-inf`, or `NaN`.
- `f32 + f64` requires explicit casting (`$f32 as f64 + $f64`).

### 5.2 `decimal` (Exact Base-10 Arithmetic)
- For monetary transactions and ledger systems where binary floating-point representation drift (e.g. `0.1 + 0.2 = 0.30000000000000004`) is impermissible.
- Evaluates addition with exact decimal precision:
  ```php
  decimal $price = 19.99d;
  decimal $tax = 1.60d;
  decimal $total = $price + $tax; // Exactly 21.59d
  ```
- Adding `decimal + float` or `decimal + int` is a compile error without explicit conversion.

### 5.3 `BigInt`
- Designed for cryptography and arbitrary-magnitude integers.
- `+` on `BigInt + BigInt` allocates arbitrary precision and **never overflows**.
- `BigInt + int` is disallowed without explicit promotion (`BigInt($int)`).

---

## 6. User-Defined Overloading: The `Add` Interface

User-defined classes (e.g. `Vector2D`, `Matrix`, `Money`) can participate in `+` expressions by explicitly implementing the `Add` interface:

```php
interface Add<Rhs = Self> {
    type Output;
    public function add(Rhs $rhs): Output;
}

class Vector2D implements Add<Vector2D> {
    public function __construct(
        public float $x,
        public float $y
    ) {}

    public function add(Vector2D $rhs): Vector2D {
        return new Vector2D($this->x + $rhs->x, $this->y + $rhs->y);
    }
}

Vector2D $v1 = new Vector2D(1.0, 2.0);
Vector2D $v2 = new Vector2D(3.0, 4.0);
Vector2D $v3 = $v1 + $v2; // Invocates Vector2D::add($v2)
```

**Rules for Operator Overloading:**
1. Must be declared explicitly via `implements Add`.
2. Operand types must be strictly specified; no runtime dynamic fallback.
3. Overloading does not introduce implicit coercions.

---

## 7. Actionable Diagnostic Errors

All diagnostic messages emitted by the type checker provide exact code spans and remediation advice:

```text
error: cannot add `string` and `int`
  --> src/checkout.rhp:14:21
   |
14 |     $total = $items + " items: " + $count;
   |                                  ^
   = help: '+' concatenates strings or adds numbers; convert with `string($count)` or use string interpolation

error: cannot add `i32` and `f64`: mismatched types
  --> src/calc.rhp:8:12
   |
 8 |     return $width + $offset;
   |            ^^^^^^^^^^^^^^^^
   = help: operands must have identical types; cast explicitly using `$width as f64`

error: attempt to checked add with overflow in constant expression: `255 + 1`
  --> src/network.rhp:3:16
   |
 3 | const PORT = 255u8 + 1;
   |              ^^^^^^^^^
   = help: constant additions that overflow will fail at compile time; use a wider type or wrapping operator if intentional
```

---

## 8. Compiler Implementation Architecture (Rust & LLVM)

The RHP compiler implements arithmetic checking through four coordinated stages:

```text
Source (.rhp) ──► [Lexer / Parser] ──► [Type Checker] ──► [Const Folder] ──► [LLVM Codegen]
                  '+', '+%', '+|'     Strict equality     Compile error     llvm.sadd.with.overflow
                                      Literal inference   on overflow       or llvm.sadd.sat
```

### 8.1 LLVM Lowering Strategy (via Inkwell)

When lowering checked additions, RHP invokes LLVM overflow intrinsics paired with branch emission:

```rust
// Codegen snippet for checked integer addition
let intrinsic = match (is_signed, bit_width) {
    (true, 64) => "llvm.sadd.with.overflow.i64",
    (false, 64) => "llvm.uadd.with.overflow.i64",
    (true, 32) => "llvm.sadd.with.overflow.i32",
    // ...
};

// Returns struct { iN result, i1 overflow_bit }
let call_result = builder.build_call(intrinsic_fn, &[lhs, rhs], "ovf_call");
let sum = builder.build_extract_value(call_result, 0, "sum");
let ovf_flag = builder.build_extract_value(call_result, 1, "ovf");

// Conditional branch to panic trap on overflow
builder.build_conditional_branch(ovf_flag, trap_bb, normal_bb);
```

### 8.2 Optimization & Overflow Check Elimination

To avoid performance degradation in compute-heavy tight loops:
1. **Value Range Propagation (VRP):** The compiler analyzes iteration bounds (e.g. `for ($i = 0; $i < 1000; $i++)`). When operand ranges mathematically guarantee that bounds cannot exceed type limits, the intrinsic check is optimized away into a naked, zero-cost `add nsw`.
2. **Explicit Opt-in:** Developers working on inner cryptographic or graphics loops can use `+%` (wrapping) or `+|` (saturating), which emit single-instruction hardware instructions without branching.
