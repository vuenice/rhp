use crate::lexer::{error::LexError, span::Span};
use std::fmt;

/// Strongly-typed parser error containing source span location and specific failure cause.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub span: Span,
    pub kind: ParseErrorKind,
}

impl ParseError {
    #[inline]
    pub fn new(kind: ParseErrorKind, span: Span) -> Self {
        Self { span, kind }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Parse error at {}: {}", self.span, self.kind)
    }
}

impl std::error::Error for ParseError {}

/// Category and details of a parser failure.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ParseErrorKind {
    #[error("Unexpected token: expected {expected}, found {found}")]
    UnexpectedToken {
        expected: String,
        found: String,
    },

    #[error("Unexpected end of input: expected {expected}")]
    UnexpectedEof {
        expected: String,
    },

    #[error("Invalid assignment target: left-hand side is not assignable")]
    InvalidAssignmentTarget,

    #[error("Fatal Error: Cannot instantiate static facade class '{class_name}'. Use static methods directly.")]
    StaticFacadeInstantiationForbidden {
        class_name: String,
    },

    #[error("Fatal Error: Cannot extend static facade class '{class_name}'. Use '{class_name}::macro()' to register custom methods dynamically.")]
    StaticFacadeInheritanceForbidden {
        class_name: String,
    },

    #[error("Fatal Error: Invalid macro registration on facade '{class_name}': {reason}")]
    InvalidFacadeMacroRegistration {
        class_name: String,
        reason: String,
    },

    #[error("Invalid syntax: {message}")]
    Custom {
        message: String,
    },

    #[error("Lexer error: {0}")]
    Lex(#[from] LexError),
}
