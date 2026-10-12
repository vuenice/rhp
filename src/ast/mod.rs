pub mod expr;
pub mod stmt;
pub mod types;
pub mod visitor;

pub use expr::{
    Argument, ArrayElement, AssignOp, BinaryOp, ClosureUse, Expr, ExprKind, Literal, MatchArm,
    StringPart, UnaryOp,
};
pub use stmt::{
    Attribute, CatchClause, ClassConstDecl, ClassDecl, ClassMember, ClassMemberKind, ElseIfBranch,
    EnumCase, EnumDecl, FunctionDecl, InterfaceDecl, MethodDecl, Param, PropertyDecl, Stmt,
    StmtKind, SwitchCase, TraitDecl, TraitUseDecl, UseImport,
};
pub use types::{
    CastType, ClassModifiers, Ident, TypeHint, TypeHintKind, Variable, Visibility,
};
pub use visitor::{walk_expr, walk_program, walk_stmt, AstVisitor};

use crate::lexer::span::Span;

/// Root program node representing a parsed RHP script or compilation unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Program<'s> {
    pub span: Span,
    pub statements: Vec<Stmt<'s>>,
}

impl<'s> Program<'s> {
    #[inline]
    pub fn new(statements: Vec<Stmt<'s>>, span: Span) -> Self {
        Self { span, statements }
    }
}

/// Compatibility alias to support the architecture specification evaluator sketch:
/// `pub fn eval_expr(node: &ASTNode, env: &mut Environment) -> Result<Value, RuntimeError>`
pub type ASTNode<'s> = Expr<'s>;

/// Compatibility alias for `ASTKind`
pub type ASTKind<'s> = ExprKind<'s>;
