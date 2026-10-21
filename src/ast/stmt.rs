use crate::ast::{
    expr::{Argument, Expr},
    types::{ClassModifiers, Ident, TypeHint, Variable, Visibility},
};
use crate::lexer::span::Span;

/// Strongly-typed statement node in the Abstract Syntax Tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt<'s> {
    pub span: Span,
    pub kind: StmtKind<'s>,
}

impl<'s> Stmt<'s> {
    #[inline]
    pub fn new(kind: StmtKind<'s>, span: Span) -> Self {
        Self { span, kind }
    }
}

/// All statement variants supported by the RHP grammar.
#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind<'s> {
    /// Expression statement (e.g. `$obj->run();`, `$x = 10;`).
    Expr(Expr<'s>),

    /// Strongly-typed variable declaration statement (e.g., `int $x = 10;`).
    VarDecl {
        var: Variable<'s>,
        type_hint: Option<TypeHint<'s>>,
        init: Option<Expr<'s>>,
    },

    /// Block of statements `{ ... }`.
    Block(Vec<Stmt<'s>>),

    /// If conditional branch with optional elseif chains and else fallback.
    If {
        condition: Expr<'s>,
        then_branch: Box<Stmt<'s>>,
        elseif_branches: Vec<ElseIfBranch<'s>>,
        else_branch: Option<Box<Stmt<'s>>>,
    },

    /// While loop.
    While {
        condition: Expr<'s>,
        body: Box<Stmt<'s>>,
    },

    /// Do-While loop.
    DoWhile {
        body: Box<Stmt<'s>>,
        condition: Expr<'s>,
    },

    /// For loop (`for ($i = 0; $i < 10; $i++)`).
    For {
        init: Vec<Expr<'s>>,
        condition: Vec<Expr<'s>>,
        update: Vec<Expr<'s>>,
        body: Box<Stmt<'s>>,
    },

    /// Foreach loop (`foreach ($items as $item)` or `foreach ($items as $k => $v)`).
    Foreach {
        expr: Expr<'s>,
        key_var: Option<Variable<'s>>,
        value_var: Box<Expr<'s>>,
        body: Box<Stmt<'s>>,
    },

    /// Switch statement with cases and default.
    Switch {
        condition: Expr<'s>,
        cases: Vec<SwitchCase<'s>>,
    },

    /// Break statement with optional nesting level.
    Break(Option<Expr<'s>>),

    /// Continue statement with optional nesting level.
    Continue(Option<Expr<'s>>),

    /// Return statement with optional return value.
    Return(Option<Expr<'s>>),

    /// Throw exception statement.
    Throw(Expr<'s>),

    /// Try-catch-finally exception handling.
    TryCatch {
        try_block: Vec<Stmt<'s>>,
        catches: Vec<CatchClause<'s>>,
        finally_block: Option<Vec<Stmt<'s>>>,
    },

    /// Function definition.
    FunctionDecl(FunctionDecl<'s>),

    /// Class definition.
    ClassDecl(ClassDecl<'s>),

    /// Interface definition.
    InterfaceDecl(InterfaceDecl<'s>),

    /// Trait definition.
    TraitDecl(TraitDecl<'s>),

    /// Enum definition.
    EnumDecl(EnumDecl<'s>),

    /// Namespace declaration (`namespace App\Services;`).
    Namespace {
        name: Option<Ident<'s>>,
        body: Option<Vec<Stmt<'s>>>,
    },

    /// Use import statement (`use App\Models\User as UserModel;`).
    Use(Vec<UseImport<'s>>),

    /// Rust file import statement (`import "./Cache.rs";` or `use "./Cache.rs" as Cache;`).
    RustImport {
        path: String,
        alias: Option<Ident<'s>>,
    },

    /// Global variables declaration (`global $config, $db;`).
    Global(Vec<Variable<'s>>),

    /// Echo output statement (`echo $msg, "\n";`).
    Echo(Vec<Expr<'s>>),

    /// Post-response or scope defer hook statement (`defer { ... };`).
    Defer(Box<Stmt<'s>>),

    /// Empty statement (stray semicolon `;`).
    Empty,
}

/// Function or method formal parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param<'s> {
    pub span: Span,
    pub attributes: Vec<Attribute<'s>>,
    pub visibility: Option<Visibility>,
    pub is_readonly: bool,
    pub type_hint: Option<TypeHint<'s>>,
    pub is_variadic: bool,
    pub is_reference: bool,
    pub name: Variable<'s>,
    pub default_value: Option<Expr<'s>>,
}

/// Standalone or method function declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl<'s> {
    pub span: Span,
    pub attributes: Vec<Attribute<'s>>,
    pub is_async: bool,
    pub name: Ident<'s>,
    pub params: Vec<Param<'s>>,
    pub return_type: Option<TypeHint<'s>>,
    pub body: Option<Vec<Stmt<'s>>>,
}

/// Class declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl<'s> {
    pub span: Span,
    pub attributes: Vec<Attribute<'s>>,
    pub modifiers: ClassModifiers,
    pub name: Ident<'s>,
    pub extends: Option<Ident<'s>>,
    pub implements: Vec<Ident<'s>>,
    pub members: Vec<ClassMember<'s>>,
}

/// Class member wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassMember<'s> {
    pub span: Span,
    pub kind: ClassMemberKind<'s>,
}

/// Class member variants.
#[derive(Debug, Clone, PartialEq)]
pub enum ClassMemberKind<'s> {
    Property(PropertyDecl<'s>),
    Method(MethodDecl<'s>),
    Constant(ClassConstDecl<'s>),
    TraitUse(TraitUseDecl<'s>),
}

/// Class property definition.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyDecl<'s> {
    pub attributes: Vec<Attribute<'s>>,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_readonly: bool,
    pub type_hint: Option<TypeHint<'s>>,
    pub name: Variable<'s>,
    pub default: Option<Expr<'s>>,
}

/// Class method definition.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodDecl<'s> {
    pub attributes: Vec<Attribute<'s>>,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub function: FunctionDecl<'s>,
}

/// Class constant definition.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassConstDecl<'s> {
    pub attributes: Vec<Attribute<'s>>,
    pub visibility: Visibility,
    pub is_final: bool,
    pub name: Ident<'s>,
    pub value: Expr<'s>,
}

/// Trait use declaration inside a class.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitUseDecl<'s> {
    pub traits: Vec<Ident<'s>>,
}

/// Interface declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceDecl<'s> {
    pub span: Span,
    pub attributes: Vec<Attribute<'s>>,
    pub name: Ident<'s>,
    pub extends: Vec<Ident<'s>>,
    pub methods: Vec<MethodDecl<'s>>,
}

/// Trait declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl<'s> {
    pub span: Span,
    pub attributes: Vec<Attribute<'s>>,
    pub name: Ident<'s>,
    pub members: Vec<ClassMember<'s>>,
}

/// Enum declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl<'s> {
    pub span: Span,
    pub attributes: Vec<Attribute<'s>>,
    pub name: Ident<'s>,
    pub backing_type: Option<TypeHint<'s>>,
    pub implements: Vec<Ident<'s>>,
    pub cases: Vec<EnumCase<'s>>,
    pub methods: Vec<MethodDecl<'s>>,
}

/// Enum case definition.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumCase<'s> {
    pub span: Span,
    pub attributes: Vec<Attribute<'s>>,
    pub name: Ident<'s>,
    pub value: Option<Expr<'s>>,
}

/// Language attribute annotation (e.g., `#[Route('/api', methods: ['GET'])]`).
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute<'s> {
    pub span: Span,
    pub name: Ident<'s>,
    pub args: Vec<Argument<'s>>,
}

/// `elseif` branch node in an if statement.
#[derive(Debug, Clone, PartialEq)]
pub struct ElseIfBranch<'s> {
    pub span: Span,
    pub condition: Expr<'s>,
    pub body: Stmt<'s>,
}

/// Switch case node.
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase<'s> {
    pub span: Span,
    pub value: Option<Expr<'s>>, // None for default
    pub body: Vec<Stmt<'s>>,
}

/// Catch clause in a try-catch block.
#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause<'s> {
    pub span: Span,
    pub types: Vec<Ident<'s>>,
    pub var: Variable<'s>,
    pub body: Vec<Stmt<'s>>,
}

/// Imported symbol in a `use` declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct UseImport<'s> {
    pub span: Span,
    pub name: Ident<'s>,
    pub alias: Option<Ident<'s>>,
}
