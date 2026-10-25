---
title: "Lexical Analysis & Syntax Architecture"
description: "Refined language design specifications, lexical rules, and optimal technical implementations for the RHP compiler and lexer."
section: "Reference"
order: 2
---

# RHP Language Specification: Lexical Analysis & Syntax Architecture

This document outlines the refined language design specifications, syntax rules, and technical implementations for the **RHP** programming language compiler and lexer. These choices ensure clean developer ergonomics, predictable parsing, and high-performance Rust execution.

---

## 1. Keyword-as-Method-Name Resolution

### The Rule

Keywords like `match`, `default`, `from`, `for`, `class`, `new`, `static`, and `clone` are reserved as language keywords primarily at **statement-start** contexts.

### Technical Implementation

* **Logos Lexer Strategy:** Tokenize these keywords as strict keyword tokens when they appear at the start of expressions or statements.
* **Property / Method Access Handling:** When a keyword follows an object accessor (`->` or `.`), or when a method requires two parameters, disambiguate using explicit static facades (e.g., `Glob::getClass()`, `Glob::match()`) or handle contextual re-classification in the parser lookahead stage where an identifier is explicitly expected after an arrow operator (`->`, `?->`) or scope resolution operator (`::`).

### Code Examples

```php
// Valid: Contextual keywords as method names after object accessors
$result = $obj->match();
$defaultVal = $obj->default();
$source = $obj->from();
$iterator = $obj->for();
$className = $obj->class;
$instance = $obj->new();
$scoped = $obj->static();
$replica = $obj->clone();

// Disambiguation via explicit static facades
$classMeta = Glob::getClass($obj);
$isMatch = Glob::match('/pattern/', $subject);
```

---

## 2. Numeric Literal Edge Cases & Strict Validation

### The Rule

Malformed numeric literals must be caught instantly at the lexical phase with explicit diagnostic errors rather than causing silent corruption or undefined behavior.

### Technical Implementation

* **Trailing / Leading Underscores & Dots:** Explicitly reject numbers ending in an underscore (e.g., `1_`, `1_000_`) or trailing decimal points (`1.`).
  * *Error Message:* `"Numeric literal cannot end with a separator or decimal point."`
* **Leading Dots:** The pattern `.5` is invalid. Developers must write explicit leading zeros (`0.5`).
* **Incomplete Prefixes & Exponents:** Tokens like `0x`, `0b`, or `1e` without trailing digits must trigger an immediate lexical scan error:
  * *Error Message:* `SyntaxError: invalid syntax.`

### Validation Matrix

| Literal Pattern | Status | Lexer Action / Diagnostic Error |
| :--- | :--- | :--- |
| `42`, `1_000`, `1_2` | **Valid** | Tokenized as integer literal |
| `3.14`, `0.5`, `1.5e-3` | **Valid** | Tokenized as float literal |
| `0x1A_2F`, `0b1010_0101` | **Valid** | Tokenized as hex / binary literal |
| `1_`, `1_000_` | **Invalid** | `"Numeric literal cannot end with a separator or decimal point."` |
| `1.` | **Invalid** | `"Numeric literal cannot end with a separator or decimal point."` |
| `.5` | **Invalid** | `SyntaxError: invalid syntax.` *(Must write `0.5`)* |
| `0x`, `0b`, `1e` | **Invalid** | `SyntaxError: invalid syntax.` |

```php
// Valid numeric literals
int $a = 42;
int $b = 1_000_000;
int $c = 1_2;
float $pi = 3.14159;
float $half = 0.5; // Explicit leading zero required

// Invalid literals - caught at lexical scan phase:
// int $err1 = 1_;       // Error: Numeric literal cannot end with a separator or decimal point.
// float $err2 = 1.;     // Error: Numeric literal cannot end with a separator or decimal point.
// float $err3 = .5;     // Error: SyntaxError: invalid syntax.
// int $err4 = 0x;       // Error: SyntaxError: invalid syntax.
// float $err5 = 1e;     // Error: SyntaxError: invalid syntax.
```

---

## 3. String Edge Cases & Quoting Strategies

### The Rule

String literals support flexible escaping and alternative quote wrapping to prevent syntax fatigue.

### Technical Implementation

* **Standard Escape Sequences (`\'`, `\"`):** If a string is wrapped in a specific quote type, including that same quote character inside requires an explicit backslash escape (`\`).
* **Alternative Quoting (Nested Quotes):** Naturally nest quotes by wrapping strings in the opposing quote type (e.g., double quotes inside single quotes or vice versa) without requiring escape characters.
* **Raw Escape Quotes (Backticks `` ` ``):** Use backtick-delimited strings to include quotes freely without manual escaping overhead.
* **Line Ending Tracking:** The lexer and source mapper track `CRLF` (`\r\n`), `LF` (`\n`), and `CR`-only (`\r`) line breaks accurately to guarantee precise line and column span diagnostics across all platforms.

### Code Examples

```php
// Standard escape sequences
string $escapedSingle = 'It\'s high performance';
string $escapedDouble = "She said, \"Hello, RHP!\"";

// Alternative quote wrapping (no escapes required)
string $nestedDouble = 'She said, "Hello, RHP!"';
string $nestedSingle = "It's high performance";

// Raw escape quotes using backticks
string $rawQuotes = `You can freely mix "double" and 'single' quotes without escaping!`;
```

---

## 4. Operator Ambiguity & Maximal Munch

### The Rule

Compound operators and multi-character punctuation must not collide with shorter prefix operators.

### Technical Implementation

* **Maximal Munch Rule:** The Logos lexer enforces greedy matching. When scanning an ellipsis (`...`), the lexer consumes all three characters immediately rather than stopping short at two dots (`..`) or a single dot (`.`).
* **Pratt Parser Precedence:** Assign strict binding powers for compound operators in the expression parser:
  1. **Null-coalesce level (`??`):** Binding power `30` (right-associative)
  2. **Elvis / Conditional level (`?:`, `? :`):** Binding power `25` (right-associative)
  3. **Assignment level (`??=`, `=`):** Binding power `20` (right-associative)

### Operator Hierarchy Interaction

Because `??` binds tighter than `?:`, which binds tighter than `??=`:
* `$a ?? $b ?: $c` correctly parses as `($a ?? $b) ?: $c`
* `$x ??= $a ?: $b` correctly parses as `$x ??= ($a ?: $b)`

```php
// Maximal munch: ... is parsed as a single variadic/spread token
function collectValues(int ...$numbers): list {
    return Arr::list([...$numbers]);
}

// Pratt parser precedence hierarchy:
// 1. ?? (null-coalesce) evaluated first
// 2. ?: (elvis) evaluated second
// 3. ??= (assignment) evaluated last
$target ??= $primary ?? $fallback ?: 'default_value';
```

---

## 5. Case Sensitivity & Type Keywords

### The Rule

RHP enforces strict case-sensitivity for core primitive types and literals to maintain compiler performance and eliminate ambiguity.

### Technical Implementation

* **Valid Literals & Types (Strictly Lowercase):**
  * Booleans & Null: `true`, `false`, `null`
  * Primitives: `string`, `bool`, `int`, `float`
* **Invalid Equivalents:** Capitalized variants (`String`, `Boolean`, `Integer`) or legacy aliases (`boolean`, `integer`) are rejected by the type checker and parser, enforcing uniform codebase conventions.

### Comparison Table

| Language Construct | Strictly Valid (Lowercase) | Invalid (Compiler Error) |
| :--- | :--- | :--- |
| **Boolean Literals** | `true`, `false` | `TRUE`, `FALSE`, `True`, `False` |
| **Null Literal** | `null` | `NULL`, `Null` |
| **String Primitive** | `string` | `String` |
| **Integer Primitive** | `int` | `Integer`, `integer` |
| **Boolean Primitive** | `bool` | `Boolean`, `boolean` |
| **Float Primitive** | `float` | `Float`, `double` |

```php
// Valid: strictly lowercase primitive types and literals
int $count = 10;
string $name = "Alice";
bool $flag = true;
$emptyVal = null;

// Rejected by type checker:
// function process(String $s): Boolean {}  // Error: Invalid primitive type 'String'
// function run(integer $x): boolean {}     // Error: Invalid primitive type 'integer'
```

> [!NOTE]
> Capitalized names like `Int` and `Float` exist only as **Tier 1 static facade classes** (`Int::i64()`, `Float::f64()`), never as primitive type hints.

---

## 6. Comments & Attribute Syntax Boundaries

### The Rule

Clear separation between documentation, code comments, and metadata attributes.

### Technical Implementation

* **Standard Comments:** Use `//` for line comments and `/* ... */` for block/doc comments (`/** ... */`).
* **Attributes:** The hash symbol (`#`) is strictly reserved for metadata attributes (e.g., `#[Route]`). **Never use `#` for comments.** When the lexer encounters `#`, it must immediately look for an opening square bracket `[` to initialize an attribute token. A bare `#` or `#` followed by whitespace/text triggers an immediate lexical scan error.

### Code Examples

```php
// Valid: Line comment using double slashes
/* Valid: Multi-line block comment */
/** Valid: Documentation comment */

// Valid: Metadata attribute declaration using #[...]
#[Route('/api/v1/orders')]
#[Authorize('admin')]
class OrderController {
    #[Inject]
    public OrderService $orders;
}

// Invalid: Never use '#' for comments (triggers compile-time Lexer error):
// # This is not a comment in RHP!
```

---

## 7. Control Flow Normalization (`elseif`)

### The Rule

Simplify conditional branching syntax by standardizing on a single keyword token.

### Technical Implementation

* **Keyword Design:** Use the single monolithic keyword `elseif` for conditional chaining to keep grammar rules clean and predictable without managing ambiguous multi-token `else if` parsing sequences.
* **Parser Enforcement:** The parser explicitly rejects `else if` (two separate tokens) with a clear diagnostic directing the developer to write `elseif`.

### Code Examples

```php
int $score = 85;

// Valid: Monolithic 'elseif' keyword
if ($score >= 90) {
    print("Grade A");
} elseif ($score >= 80) {
    print("Grade B");
} elseif ($score >= 70) {
    print("Grade C");
} else {
    print("Grade F");
}

// Invalid: Multi-token 'else if' is rejected
// if ($score >= 90) { ... } else if ($score >= 80) { ... }
// Error: Use the single keyword 'elseif' instead of 'else if' for conditional chaining.
```

---

## 8. Variable Identifier Validation

### The Rule

Dynamic variable interpolation paradigms (such as PHP's `$$var`) are completely omitted to ensure memory safety, predictable scopes, and clean VM execution.

### Technical Implementation

* **Strict Lexing Rules:** Any bare `$`, double-dollar `$$`, or digit-prefixed variable name (e.g., `$0`) is intercepted at the lexer and parser boundary.
* **Diagnostic Error:**
  * *Error Message:* `"Variable names must start with a letter or underscore, followed by alphanumeric characters (e.g., $varName)."`

### Validation Matrix

| Variable Pattern | Status | Diagnostic Result |
| :--- | :--- | :--- |
| `$user`, `$orderId`, `$_count` | **Valid** | Tokenized as `Token::Variable` |
| `$$`, `$$var` | **Invalid** | `"Variable names must start with a letter or underscore, followed by alphanumeric characters (e.g., $varName)."` |
| `$` *(bare)* | **Invalid** | `"Variable names must start with a letter or underscore, followed by alphanumeric characters (e.g., $varName)."` |
| `$0`, `$9abc` | **Invalid** | `"Variable names must start with a letter or underscore, followed by alphanumeric characters (e.g., $varName)."` |

```php
// Valid variable identifiers
int $userId = 1001;
string $_tempToken = "abc";
float $discount2026 = 15.5;

// Invalid variable identifiers - caught at compilation boundary:
// $$dynamicVar = 10; // Error: Variable names must start with a letter or underscore...
// $0 = "invalid";    // Error: Variable names must start with a letter or underscore...
// $ = 42;            // Error: Variable names must start with a letter or underscore...
```

---

## 9. Magic Constants & Compile-Time Scope Lowering

### The Rule

RHP supports standard PHP magic constants (`__LINE__`, `__FILE__`, `__DIR__`, `__CLASS__`, `__METHOD__`, `__FUNCTION__`, `__TRAIT__`, `__NAMESPACE__`) as **compile-time and scope-resolved placeholders** that fold directly into literal AST nodes without runtime performance penalties.

### Technical Implementation

* **Compile-Time File Bounds (`__LINE__`, `__FILE__`, `__DIR__`):**
  * `__LINE__`: Replaced with an integer literal (`Literal::Int`) containing the 1-based line number from the token span (`Span.line`).
  * `__FILE__`: Replaced with a string literal (`Literal::String`) containing the canonical absolute file path.
  * `__DIR__`: Replaced with a string literal (`Literal::String`) containing the parent directory of `__FILE__`.
* **Lexical Scope Resolution (`__CLASS__`, `__METHOD__`, `__FUNCTION__`, `__TRAIT__`, `__NAMESPACE__`):**
  * During Pratt and Combinator parsing, `CombinatorStream` maintains a `ScopeContext` with class, trait, and function scope stacks.
  * `__FUNCTION__`: Pulls the active function or method identifier (or `"{closure}"` inside closures).
  * `__CLASS__`: Pulls the active enclosing class or enum name (including namespace).
  * `__METHOD__`: Constructs the method FQN (e.g. `App\Services\Worker::handle`).
  * `__TRAIT__`: Pulls the active enclosing trait name.
  * `__NAMESPACE__`: Pulls the active namespace.
* **Zero Runtime Overhead:** Because all magic constants are folded into static `ExprKind::Literal` values during parsing, bytecode generation and the Register Bytecode VM dispatch them as standard pre-allocated literals with no stack-trace or reflection overhead.

