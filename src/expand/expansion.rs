use crate::ast::stmt::{ClassMember, Stmt};

/// Structured output returned by an [`Expander`](crate::expand::Expander) handler.
///
/// Handlers never mutate the input AST directly. Instead, they return structured
/// AST transformations that the expansion engine incorporates into the surrounding AST.
#[derive(Debug, Clone, PartialEq)]
pub enum Expansion {
    /// Add members (properties, methods, constants) to the enclosing class.
    /// Used by attributes like `#[fillable]`, `#[guarded]`, `#[table]`.
    AddClassMembers(Vec<ClassMember<'static>>),

    /// Replace class members with transformed versions.
    ReplaceClassMembers(Vec<ClassMember<'static>>),

    /// Emit statements replacing the expansion site (e.g. for template directives like `@if`, `@for`).
    Statements(Vec<Stmt<'static>>),

    /// Add top-level statements to the enclosing program or block.
    AddStatements(Vec<Stmt<'static>>),

    /// Empty expansion (the attribute was purely declarative or validated metadata).
    Empty,
}

impl Expansion {
    /// Helper to construct an empty expansion.
    pub fn empty() -> Self {
        Self::Empty
    }

    /// Helper to construct an expansion that adds class members.
    pub fn add_members(members: Vec<ClassMember<'static>>) -> Self {
        Self::AddClassMembers(members)
    }

    /// Helper to construct an expansion that emits statements.
    pub fn statements(stmts: Vec<Stmt<'static>>) -> Self {
        Self::Statements(stmts)
    }
}
