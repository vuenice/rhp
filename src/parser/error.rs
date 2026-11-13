use crate::lexer::{error::LexError, span::Span};
use std::fmt;

/// Strongly-typed parser error containing source span location and specific failure cause.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub span: Span,
    pub kind: ParseErrorKind,
    pub secondary_label: Option<(Span, String)>,
    pub expected_tokens: Option<Vec<String>>,
    pub found_token: Option<String>,
}

impl ParseError {
    #[inline]
    pub fn new(kind: ParseErrorKind, span: Span) -> Self {
        Self {
            span,
            kind,
            secondary_label: None,
            expected_tokens: None,
            found_token: None,
        }
    }

    pub fn with_secondary_label(mut self, span: Span, text: impl Into<String>) -> Self {
        self.secondary_label = Some((span, text.into()));
        self
    }

    pub fn with_tokens(mut self, expected: Vec<String>, found: Option<String>) -> Self {
        self.expected_tokens = Some(expected);
        self.found_token = found;
        self
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

    #[error("Unsupported operator '{op}'. RHP does not support '{op}'. Use '{suggested}' instead for strict comparison.")]
    UnsupportedOperator {
        op: String,
        suggested: String,
    },

    #[error("Compile Error: isset() cannot be used on bare variable '{var_name}'. In RHP, undefined variables are compile errors; restrict isset() to array access, property access, or nullable expressions.")]
    BareVariableIssetForbidden {
        var_name: String,
    },

    #[error("Compile Error: Cannot unset object variable '{var_name}': removing objects via unset() is forbidden.")]
    CannotUnsetObject {
        var_name: String,
    },

    #[error("Compile Error: Invalid unset target: {reason}")]
    InvalidUnsetTarget {
        reason: String,
    },

    #[error("Compile Error: Dynamic '{construct}' is not supported in RHP. Use static 'import' or 'use' with literal paths or namespaces. Dynamic file inclusion violates compile-time type safety and standalone binary compilation.")]
    DynamicIncludeForbidden {
        construct: String,
    },

    #[error("Compile Error: Directive statement 'declare(...)' is not supported in RHP. Strict typing is enabled project-wide by default; run 'rhp migrate' to remove declare() directives.")]
    DeclareDirectiveForbidden,

    #[error("Lexer error: {0}")]
    Lex(#[from] LexError),
}
