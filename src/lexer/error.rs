use std::fmt;
use thiserror::Error;

use super::span::Span;

/// Lexical analysis error variants.
#[derive(Error, Debug, Clone, PartialEq, Eq, Default)]
pub enum LexError {
    #[default]
    #[error("unrecognized token")]
    UnrecognizedToken,

    #[error("unterminated string literal")]
    UnterminatedString,

    #[error("unterminated block comment")]
    UnterminatedComment,

    #[error("invalid escape sequence: {0}")]
    InvalidEscape(String),

    #[error("{0}")]
    InvalidNumericLiteral(String),

    #[error("{0}")]
    InvalidVariableIdentifier(String),
}

/// A lexical error bundled with its source span for precise diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpannedLexError {
    pub error: LexError,
    pub span: Span,
}

impl SpannedLexError {
    pub fn new(error: LexError, span: Span) -> Self {
        Self { error, span }
    }
}

impl fmt::Display for SpannedLexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Lexer error: {} at line {}, col {} (offset {}..{})",
            self.error, self.span.line, self.span.col, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for SpannedLexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
