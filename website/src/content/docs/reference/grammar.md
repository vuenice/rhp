---
title: "Grammar & Syntax Reference"
description: "Formal EBNF grammar specification, custom Pratt parser operator precedence hierarchy, and strongly-typed Abstract Syntax Tree (AST) logical structure for RHP."
section: "Reference"
order: 1
---

## Overview & Compiler Parsing Architecture

RHP implements a high-throughput, two-phase frontend pipeline that transforms raw source text into a strongly-typed Abstract Syntax Tree (AST) with exact zero-copy string slicing and source span tracking:

```text
Source Code (&'s str)
         │
         ▼
┌───────────────────────────────┐
│   Frontend Lexer (Logos DFA)  │ ──► Zero-copy token slices referencing source lifetime 's
└───────────────────────────────┘
         │
         ▼
┌───────────────────────────────────────────────────────────────────────────┐
│                           RHP Parser Pipeline                             │
│                                                                           │
│  ┌─────────────────────────────┐        ┌──────────────────────────────┐  │
│  │   Combinator Parsing        │        │   Custom Pratt Parser        │  │
│  │   (src/parser/combinator.rs)│ ◄────► │   (src/parser/pratt.rs)      │  │
│  │   - Delimited & Separated   │        │   - Top-down binding power   │  │
│  │   - Declarations & Blocks   │        │   - Infix & Postfix led()    │  │
│  │   - Control flow & Typing   │        │   - Prefix nud() & Grouping  │  │
│  └─────────────────────────────┘        └──────────────────────────────┘  │
└───────────────────────────────────────────────────────────────────────────┘
         │
         ▼
┌───────────────────────────────────────────────────────────────────────────┐
│                     Strongly-Typed AST (src/ast/)                         │
│  - Program<'s>                                                            │
│  - Stmt<'s> (Functions, Classes, Interfaces, Enums, If, Loops, VarDecls) │
│  - Expr<'s> (Pratt expressions, Method/Static chains, Destructuring)      │
│  - Exact Source Span Tracking (Span { file_id, start, end, line, col })  │
└───────────────────────────────────────────────────────────────────────────┘
```

---

## 1. Lexical Grammar (Logos DFA Engine)

The lexical analyzer engine uses `logos` to generate a zero-copy deterministic finite automaton (DFA). Tokens borrow slices directly from the source buffer lifetime `'s`:

```text
Whitespace        ::= [ \t\r\n\f]+
CommentLine       ::= "//" [^\r\n]*
CommentBlock      ::= "/*" ( [^*] | ('*' [^/]) )* "*/"
DocComment        ::= "/**" ( [^*] | ('*' [^/]) )* "*/"
AttributeStart    ::= "#["

Ident             ::= [a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*
Variable          ::= "$" [a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*

DecimalInt        ::= [0-9]+ ("_" [0-9]+)*
HexInt            ::= "0" [xX] [0-9a-fA-F]+ ("_" [0-9a-fA-F]+)*
BinaryInt         ::= "0" [bB] [01]+ ("_" [01]+)*
OctalInt          ::= "0" [oO] [0-7]+ ("_" [0-7]+)*
FloatLiteral      ::= [0-9]+ ("_" [0-9]+)* "." [0-9]+ ("_" [0-9]+)* ([eE][+-]?[0-9]+ ("_" [0-9]+)*)?
                    | [0-9]+ ("_" [0-9]+)* [eE][+-]?[0-9]+ ("_" [0-9]+)*

SingleQuotedStr   ::= "'" ( [^'\\] | "\\" . )* "'"
DoubleQuotedStr   ::= '"' ( [^"\\] | "\\" . )* '"'
BacktickStr       ::= "`" ( [^`\\] | "\\" . )* "`"
```

> [!IMPORTANT]
> **Strict Lexical Boundaries:**
> * **Attributes vs. Comments:** The hash symbol (`#`) is strictly reserved for metadata attributes (`#[Attribute]`). **`#` is never used for comments.** Encountering `#` without `[` triggers an immediate lexical scan error.
> * **Strict Numeric Validation:** Numbers cannot end with an underscore (e.g. `1_`, `1_000_`) or decimal point (`1.`)—these trigger `"Numeric literal cannot end with a separator or decimal point."`. Leading dots (e.g. `.5`) are invalid and require explicit leading zeros (`0.5`). Incomplete prefixes/exponents (`0x`, `0b`, `1e`) trigger `SyntaxError: invalid syntax.`.
> * **Variable Validation:** Bare `$`, double-dollar (`$$`), or digit-prefixed variable names (`$0`) are strictly rejected with `"Variable names must start with a letter or underscore, followed by alphanumeric characters (e.g., $varName)."`.
> * For full details, see the [Lexical Analysis & Syntax Architecture Reference](/docs/reference/syntax-architecture).

---

## 2. Custom Pratt Parser & Operator Precedence

Expressions are parsed in linear time $O(N)$ using a custom Pratt parser (Top-Down Operator Precedence). Each operator has explicit left and right binding powers (`left_bp`, `right_bp`) guaranteeing mathematical and logical associativity:

| Precedence Category | Binding Power (L, R) | Operators | Associativity | Description |
| :--- | :--- | :--- | :--- | :--- |
| **Member Access & Calls** | `(100, 101)` | `->`, `?->`, `::`, `( )`, `[ ]` | Left-to-right | Property fetch, nullsafe fetch, static call, invoke, array index |
| **Postfix Operators** | `95` | `++`, `--` | Left-to-right | Post-increment and post-decrement (`$x++`, `$x--`) |
| **Prefix Unary** | `93` | `+`, `-`, `!`, `~`, `++`, `--`, `@`, `clone`, `await` | Right-to-left | Unary signs, bitwise/logical NOT, pre-inc/dec, error suppression |
| **Exponentiation** | `(91, 90)` | `**` | **Right-to-left** | Power operator (`2 ** 3 ** 4` $\rightarrow$ `2 ** (3 ** 4)`) |
| **Type Instance Check** | `(85, 86)` | `instanceof` | Left-to-right | Class and interface verification (`$obj instanceof User`) |
| **Multiplicative** | `(80, 81)` | `*`, `/`, `%` | Left-to-right | Multiplication, division, modulo |
| **Additive & String** | `(75, 76)` | `+`, `-`, `.` | Left-to-right | Addition, subtraction, string concatenation (`.`) |
| **Bitwise Shift** | `(70, 71)` | `<<`, `>>` | Left-to-right | Bitwise left and right shift |
| **Relational Comparison** | `(65, 66)` | `<`, `<=`, `>`, `>=`, `<=>` | Left-to-right | Less, greater, spaceship comparison |
| **Equality Check** | `(60, 61)` | `==`, `!=`, `===`, `!==`, `<>` | Left-to-right | Strict equivalence and identity |
| **Bitwise AND** | `(55, 56)` | `&` | Left-to-right | Bitwise AND |
| **Bitwise XOR** | `(50, 51)` | `^` | Left-to-right | Bitwise XOR |
| **Bitwise OR** | `(45, 46)` | `\|` | Left-to-right | Bitwise OR |
| **Logical AND** | `(40, 41)` | `&&` | Left-to-right | Short-circuit boolean AND |
| **Logical OR** | `(35, 36)` | `\|\|` | Left-to-right | Short-circuit boolean OR |
| **Null Coalescing** | `(30, 29)` | `??` | **Right-to-left** | Fallback chaining (`$a ?? $b ?? $default`) |
| **Ternary & Elvis** | `(25, 24)` | `? :`, `?:` | **Right-to-left** | Conditional expression and Elvis fallback |
| **Assignment** | `(20, 19)` | `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `.=`, `??=`, `&=`, `\|=`, `^=`, `<<=`, `>>=` | **Right-to-left** | Variable assignment (`$a = $b = 10` $\rightarrow$ `$a = ($b = 10)`) |
| **Logical Word AND** | `(9, 10)` | `and` | Left-to-right | Low-precedence logical AND |
| **Logical Word XOR** | `(7, 8)` | `xor` | Left-to-right | Low-precedence logical XOR |
| **Logical Word OR** | `(5, 6)` | `or` | Left-to-right | Low-precedence logical OR |
| **Generator Yield** | `(1, 1)` | `yield`, `yield from` | Non-associative | Yield value or delegate to generator |
| **Arrow Closures** | `0` (Lowest) | `fn` | Non-associative | Single-expression arrow function (`fn($n) => $n * 2`) |

### Right-Associative Operator Proofs

1. **Exponentiation (`**`):**
   ```php
   $res = 2 ** 3 ** 4; // Parsed as 2 ** (3 ** 4) = 2 ** 81 = 2417851639229258349412352
   ```
2. **Chained Assignment (`=`):**
   ```php
   $a = $b = 10; // Parsed as $a = ($b = 10)
   ```
3. **Null-Coalescing (`??`):**
   ```php
   $mode = $primary ?? $fallback ?? 'default'; // Parsed as $primary ?? ($fallback ?? 'default')
   ```

---

## 3. Special Expression Grammar Constructs

### Fluent Facade & Method Chaining
Static facade calls followed by chained instance methods parse seamlessly with high operator precedence:

```php
$numbers = Arr::list([1, 2, 3, 4, 5])
    ->map(fn($n) => $n * 2)
    ->filter(fn($n) => $n > 4);

$id = Int::i64(9223372036854775807);
$price = Float::f64(99.99)->toDecimal(2);
Int::i64()->random()->add()->subtract();
```

### Static Facade Instantiation Defense
Instantiation of static facades (`new Arr()`, `new Str()`, `new Int()`, `new Float()`, `new HTTP()`, `new Future()`, `new DB()`, `new Log()`, `new Sys()`) is strictly prohibited at the parser level:

```php
// Fatal Compile-Time Error:
$bad = new Int(); 
// Diagnostic: "Fatal Error: Cannot instantiate static facade class 'Int'. Use static methods directly."
```

### Array Destructuring Assignment
Arrays can be unpacked directly into variables on the left side of an assignment:

```php
[$user, $posts] = Future::wait([
    fn() => DB::table('users')->find($id),
    fn() => HTTP::get("https://api.example.com/posts?user={$id}")->json()
]);
```

### Double-Quoted String Interpolation
Double-quoted strings containing `$var` or `{$expr}` are broken down by the interpolation mini-parser into discrete AST parts:

```php
$url = "https://api.example.com/posts?user={$id}&limit={$limit + 10}";
```
* Generates `Literal::InterpolatedString` with alternating `StringPart::Literal` and `StringPart::Expr` nodes, avoiding runtime regex evaluation.

---

## 4. Combinator Statement & Declaration Grammar

Structural grammar rules are parsed using combinatorial functions (`delimited`, `separated_by`, `parse_attributes`, `parse_type_hint`):

```text
Program         ::= PhpTag? Statement* PhpClose?

Statement       ::= BlockStmt
                  | VarDeclStmt
                  | FunctionDecl
                  | ClassDecl
                  | InterfaceDecl
                  | TraitDecl
                  | EnumDecl
                  | IfStmt
                  | WhileStmt
                  | DoWhileStmt
                  | ForStmt
                  | ForeachStmt
                  | SwitchStmt
                  | TryCatchStmt
                  | NamespaceStmt
                  | UseStmt
                  | GlobalStmt
                  | EchoStmt
                  | DeferStmt
                  | ReturnStmt
                  | BreakStmt
                  | ContinueStmt
                  | ThrowStmt
                  | ExprStmt
                  | ";"

VarDeclStmt     ::= Variable ":" TypeHint ("=" Expression)? ";"
ExprStmt        ::= Expression ";"
BlockStmt       ::= "{" Statement* "}"
```

### Surface-Level Strict & Gradual Type Hints

Type hints support primitives, native Rust numeric types, nullable types, unions, and intersections:

```text
TypeHint        ::= "?" BaseTypeHint
                  | BaseTypeHint ("|" BaseTypeHint)+
                  | BaseTypeHint ("&" BaseTypeHint)+
                  | BaseTypeHint

BaseTypeHint    ::= "int" | "float" | "string" | "bool" | "void" | "mixed" | "never"
                  | "callable" | "array" | "iterable"
                  | "i64" | "f64" | "u32" | "i32" | Ident
```

> [!NOTE]
> Primitive types must be strictly lowercase (`string`, `bool`, `int`, `float`). Capitalized variants (`String`, `Boolean`, `Integer`) and legacy aliases (`boolean`, `integer`) are rejected by the type checker.

#### Examples:
```php
function calculateTotal(int $quantity, float $unitPrice, string $currency): float {
    $subtotal = $quantity * $unitPrice;
    return $subtotal;
}

// Nullable type hint
function findUser(?int $id): ?User { ... }

// Union type hint
function processPayload(int|float $val): string|bool { ... }

// Intersection type hint
function handleBatch(Countable&ArrayAccess $items): void { ... }
```

### Object-Oriented Declarations

```text
ClassDecl       ::= AttributeList? ClassModifiers? "class" Ident ("extends" Ident)? ("implements" Ident ("," Ident)*)? "{" ClassMember* "}"
ClassModifiers  ::= ("abstract" | "final" | "readonly")*

ClassMember     ::= AttributeList? Visibility? "static"? "readonly"? TypeHint? Variable ("=" Expression)? ";"
                  | AttributeList? Visibility? "static"? "abstract"? "final"? "function" Ident "(" ParamList? ")" (":" TypeHint)? (BlockStmt | ";")
                  | AttributeList? Visibility? "final"? "const" Ident "=" Expression ";"
                  | "use" Ident ("," Ident)* ";"

Visibility      ::= "public" | "protected" | "private"
```

#### Example with Attributes and Constructor Promotion:
```php
#[Route('/api/orders')]
final class OrderService extends BaseService implements IOrderService {
    public const MAX_RETRIES = 3;
    private readonly String $apiKey;
    public static int $instances = 0;

    public function __construct(public int $id, private String $secretKey) {
        $this->apiKey = $secretKey;
        self::$instances++;
    }

    #[Authorize('admin')]
    public function process(int $orderId): bool {
        return true;
    }
}
```

---

## 5. Strongly-Typed AST Representation (`src/ast/`)

Every AST node holds an exact source span (`Span { file_id, start, end, line, col }`) for compiler diagnostics, language servers (LSP), and Debug Adapter Protocol (DAP) breakpoints:

```rust
pub struct Program<'s> {
    pub span: Span,
    pub statements: Vec<Stmt<'s>>,
}

pub struct Stmt<'s> {
    pub span: Span,
    pub kind: StmtKind<'s>,
}

pub struct Expr<'s> {
    pub span: Span,
    pub kind: ExprKind<'s>,
}
```

### AST Visitor Pattern (`src/ast/visitor.rs`)

The compiler implements the `AstVisitor<'s>` trait for immutable recursive tree walking across expressions, statements, and declarations:

```rust
pub trait AstVisitor<'s> {
    fn visit_program(&mut self, program: &Program<'s>) { walk_program(self, program); }
    fn visit_stmt(&mut self, stmt: &Stmt<'s>) { walk_stmt(self, stmt); }
    fn visit_expr(&mut self, expr: &Expr<'s>) { walk_expr(self, expr); }
}
```
This visitor powers the Development Mode AST interpreter (`rhp run dev`), linting passes, type checking, and IR lowering.
