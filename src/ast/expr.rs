use crate::ast::{
    stmt::{Param, Stmt},
    types::{CastType, Ident, TypeHint, Variable},
};
use crate::lexer::span::Span;
use std::fmt;

/// Strongly-typed expression node in the Abstract Syntax Tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr<'s> {
    pub span: Span,
    pub kind: ExprKind<'s>,
}

impl<'s> Expr<'s> {
    #[inline]
    pub fn new(kind: ExprKind<'s>, span: Span) -> Self {
        Self { span, kind }
    }

    /// Construct a static facade or class method call (`Target::method(args)`).
    #[inline]
    pub fn static_call(
        target_name: &'s str,
        method_name: &'s str,
        args: Vec<Argument<'s>>,
        target_span: Span,
        full_span: Span,
    ) -> Self {
        Self::new(
            ExprKind::StaticCall {
                target: Box::new(Self::new(
                    ExprKind::Ident(Ident::new(target_name, target_span)),
                    target_span,
                )),
                method: Ident::new(method_name, target_span),
                args,
            },
            full_span,
        )
    }

    /// Construct a standard declared user function call (`callee(args)`).
    #[inline]
    pub fn user_function_call(
        callee_name: &'s str,
        args: Vec<Argument<'s>>,
        callee_span: Span,
        full_span: Span,
    ) -> Self {
        Self::new(
            ExprKind::Call {
                callee: Box::new(Self::new(
                    ExprKind::Ident(Ident::new(callee_name, callee_span)),
                    callee_span,
                )),
                args,
            },
            full_span,
        )
    }
}

/// All expression variants supported by the RHP grammar.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind<'s> {
    /// Primitive literal value.
    Literal(Literal<'s>),

    /// Variable identifier (e.g., `$userId`).
    Variable(Variable<'s>),

    /// Standalone identifier (e.g., class name, function name, constant).
    Ident(Ident<'s>),

    /// Unary operation (e.g., `- $x`, `! $flag`, `++ $count`, `$count++`).
    Unary {
        op: UnaryOp,
        expr: Box<Expr<'s>>,
        is_prefix: bool,
    },

    /// Binary operation (e.g., `$a + $b`, `$a * $b`, `$x == $y`).
    Binary {
        op: BinaryOp,
        left: Box<Expr<'s>>,
        right: Box<Expr<'s>>,
    },

    /// Assignment operation (e.g., `$x = 10`, `$x += 5`).
    Assign {
        op: AssignOp,
        target: Box<Expr<'s>>,
        value: Box<Expr<'s>>,
    },

    /// Ternary or Elvis expression (`$cond ? $then : $else` or `$cond ?: $else`).
    Ternary {
        condition: Box<Expr<'s>>,
        if_true: Option<Box<Expr<'s>>>,
        if_false: Box<Expr<'s>>,
    },

    /// Function call (e.g., `calculateTotal($a, $b)`).
    Call {
        callee: Box<Expr<'s>>,
        args: Vec<Argument<'s>>,
    },

    /// Object instance method call (`$user->getName()` or nullsafe `$user?->getName()`).
    MethodCall {
        object: Box<Expr<'s>>,
        method: Ident<'s>,
        args: Vec<Argument<'s>>,
        is_nullsafe: bool,
    },

    /// Static method invocation on class or facade (`Arr::list(...)`, `Int::i64(...)`).
    StaticCall {
        target: Box<Expr<'s>>,
        method: Ident<'s>,
        args: Vec<Argument<'s>>,
    },

    /// Object property fetch (`$user->name` or nullsafe `$user?->name`).
    PropertyFetch {
        object: Box<Expr<'s>>,
        property: Ident<'s>,
        is_nullsafe: bool,
    },

    /// Static property access (`User::$instanceCount`).
    StaticPropertyFetch {
        class: Ident<'s>,
        property: Variable<'s>,
    },

    /// Class constant fetch or class reference (`User::STATUS_ACTIVE`, `User::class`).
    ClassConstFetch {
        class: Ident<'s>,
        const_name: Ident<'s>,
    },

    /// Array literal definition (`[1, 2, 3]` or `['key' => 'value']`).
    Array(Vec<ArrayElement<'s>>),

    /// Array subscript access (`$items[0]` or append `$items[]`).
    ArrayIndex {
        array: Box<Expr<'s>>,
        index: Option<Box<Expr<'s>>>,
    },

    /// Destructuring target list (`[$user, $posts]`).
    List(Vec<Option<ArrayElement<'s>>>),

    /// Arrow function expression (`fn($n) => $n * 2`).
    ArrowFunction {
        params: Vec<Param<'s>>,
        return_type: Option<TypeHint<'s>>,
        body: Box<Expr<'s>>,
    },

    /// Anonymous closure expression (`function($x) use ($y) { ... }`).
    Closure {
        is_static: bool,
        params: Vec<Param<'s>>,
        uses: Vec<ClosureUse<'s>>,
        return_type: Option<TypeHint<'s>>,
        body: Vec<Stmt<'s>>,
    },

    /// Match expression (`match ($status) { 200 => 'OK', default => 'Err' }`).
    Match {
        condition: Box<Expr<'s>>,
        arms: Vec<MatchArm<'s>>,
    },

    /// Object instantiation (`new User($name)`).
    New {
        class: Box<Expr<'s>>,
        args: Vec<Argument<'s>>,
    },

    /// Object cloning (`clone $user`).
    Clone(Box<Expr<'s>>),

    /// Instanceof type check (`$obj instanceof User`).
    Instanceof {
        expr: Box<Expr<'s>>,
        class: Box<Expr<'s>>,
    },

    /// Type casting / conversion (e.g., `int($string)`, `string($x)`).
    Cast {
        kind: CastType,
        expr: Box<Expr<'s>>,
    },

    /// Generator yield expression (`yield $item`, `yield $k => $v`, `yield from $gen`).
    Yield {
        key: Option<Box<Expr<'s>>>,
        value: Option<Box<Expr<'s>>>,
        is_from: bool,
    },

    /// Asynchronous await expression (`await $future`).
    Await(Box<Expr<'s>>),

    /// Parenthesized grouping `(expr)`.
    Grouping(Box<Expr<'s>>),
}

/// Primitive literal representations.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal<'s> {
    Int {
        raw: &'s str,
        value: i64,
    },
    Float {
        raw: &'s str,
        value: f64,
    },
    String {
        raw: &'s str,
        value: String,
    },
    InterpolatedString {
        raw: &'s str,
        parts: Vec<StringPart<'s>>,
    },
    Bool(bool),
    Null,
}

/// Segment of an interpolated double-quoted string.
#[derive(Debug, Clone, PartialEq)]
pub enum StringPart<'s> {
    Literal(String),
    Expr(Expr<'s>),
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    // Arithmetic
    Add,       // +
    Sub,       // -
    Mul,       // *
    Div,       // /
    Mod,       // %
    Pow,       // **
    // String
    Concat,    // .
    // Bitwise
    BitAnd,    // &
    BitOr,     // |
    BitXor,    // ^
    Shl,       // <<
    Shr,       // >>
    // Comparison
    Eq,            // ==
    Identical,     // ===
    NotEq,         // != or <>
    NotIdentical,  // !==
    Lt,            // <
    Gt,            // >
    LtEq,          // <=
    GtEq,          // >=
    Spaceship,     // <=>
    // Logical
    LogicalAnd,    // &&
    LogicalOr,     // ||
    WordAnd,       // and
    WordOr,        // or
    WordXor,       // xor
    // Null Coalescing
    Coalesce,      // ??
}

impl fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op = match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::Pow => "**",
            BinaryOp::Concat => ".",
            BinaryOp::BitAnd => "&",
            BinaryOp::BitOr => "|",
            BinaryOp::BitXor => "^",
            BinaryOp::Shl => "<<",
            BinaryOp::Shr => ">>",
            BinaryOp::Eq => "==",
            BinaryOp::Identical => "===",
            BinaryOp::NotEq => "!=",
            BinaryOp::NotIdentical => "!==",
            BinaryOp::Lt => "<",
            BinaryOp::Gt => ">",
            BinaryOp::LtEq => "<=",
            BinaryOp::GtEq => ">=",
            BinaryOp::Spaceship => "<=>",
            BinaryOp::LogicalAnd => "&&",
            BinaryOp::LogicalOr => "||",
            BinaryOp::WordAnd => "and",
            BinaryOp::WordOr => "or",
            BinaryOp::WordXor => "xor",
            BinaryOp::Coalesce => "??",
        };
        write!(f, "{}", op)
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Plus,     // +
    Neg,      // -
    Not,      // !
    BitNot,   // ~
    PreInc,   // ++$x
    PreDec,   // --$x
    PostInc,  // $x++
    PostDec,  // $x--
    Silence,  // @
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op = match self {
            UnaryOp::Plus => "+",
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
            UnaryOp::BitNot => "~",
            UnaryOp::PreInc | UnaryOp::PostInc => "++",
            UnaryOp::PreDec | UnaryOp::PostDec => "--",
            UnaryOp::Silence => "@",
        };
        write!(f, "{}", op)
    }
}

/// Assignment operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssignOp {
    Assign,          // =
    AddAssign,       // +=
    SubAssign,       // -=
    MulAssign,       // *=
    DivAssign,       // /=
    ModAssign,       // %=
    PowAssign,       // **=
    ConcatAssign,    // .=
    CoalesceAssign,  // ??=
    BitAndAssign,    // &=
    BitOrAssign,     // |=
    BitXorAssign,    // ^=
    ShlAssign,       // <<=
    ShrAssign,       // >>=
}

impl fmt::Display for AssignOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op = match self {
            AssignOp::Assign => "=",
            AssignOp::AddAssign => "+=",
            AssignOp::SubAssign => "-=",
            AssignOp::MulAssign => "*=",
            AssignOp::DivAssign => "/=",
            AssignOp::ModAssign => "%=",
            AssignOp::PowAssign => "**=",
            AssignOp::ConcatAssign => ".=",
            AssignOp::CoalesceAssign => "??=",
            AssignOp::BitAndAssign => "&=",
            AssignOp::BitOrAssign => "|=",
            AssignOp::BitXorAssign => "^=",
            AssignOp::ShlAssign => "<<=",
            AssignOp::ShrAssign => ">>=",
        };
        write!(f, "{}", op)
    }
}

/// Call invocation argument.
#[derive(Debug, Clone, PartialEq)]
pub struct Argument<'s> {
    pub span: Span,
    pub name: Option<Ident<'s>>,
    pub value: Expr<'s>,
    pub is_unpack: bool,
}

impl<'s> Argument<'s> {
    #[inline]
    pub fn new(value: Expr<'s>, span: Span) -> Self {
        Self {
            span,
            name: None,
            value,
            is_unpack: false,
        }
    }

    #[inline]
    pub fn named(name: Ident<'s>, value: Expr<'s>, span: Span) -> Self {
        Self {
            span,
            name: Some(name),
            value,
            is_unpack: false,
        }
    }

    #[inline]
    pub fn unpacked(value: Expr<'s>, span: Span) -> Self {
        Self {
            span,
            name: None,
            value,
            is_unpack: true,
        }
    }
}

/// Element in an array literal.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayElement<'s> {
    pub span: Span,
    pub key: Option<Expr<'s>>,
    pub value: Expr<'s>,
    pub is_unpack: bool,
}

impl<'s> ArrayElement<'s> {
    #[inline]
    pub fn value(value: Expr<'s>, span: Span) -> Self {
        Self {
            span,
            key: None,
            value,
            is_unpack: false,
        }
    }

    #[inline]
    pub fn key_value(key: Expr<'s>, value: Expr<'s>, span: Span) -> Self {
        Self {
            span,
            key: Some(key),
            value,
            is_unpack: false,
        }
    }

    #[inline]
    pub fn unpacked(value: Expr<'s>, span: Span) -> Self {
        Self {
            span,
            key: None,
            value,
            is_unpack: true,
        }
    }
}

/// Closure `use ($var, &$ref)` capture entry.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosureUse<'s> {
    pub span: Span,
    pub var: Variable<'s>,
    pub by_ref: bool,
}

/// Match expression branch arm.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm<'s> {
    pub span: Span,
    pub patterns: Vec<Expr<'s>>,
    pub is_default: bool,
    pub body: Expr<'s>,
}
