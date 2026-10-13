use std::fmt;

use crate::lexer::span::Span;
use crate::parser::error::ParseError;
use crate::type_system::{error::TypeError, FrontendError};

/// Diagnostic error emitted during Intermediate Representation (IR) bytecode lowering.
#[derive(Debug, Clone, PartialEq)]
pub struct IrError {
    pub span: Span,
    pub kind: IrErrorKind,
}

impl IrError {
    #[inline]
    pub fn new(kind: IrErrorKind, span: Span) -> Self {
        Self { span, kind }
    }
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IR lowering error at {}: {}", self.span, self.kind)
    }
}

impl std::error::Error for IrError {}

/// Specific failure modes encountered during AST-to-Bytecode IR lowering.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum IrErrorKind {
    #[error("IR Error: '{statement}' statement is not permitted outside of a loop or switch block.")]
    ControlFlowOutsideLoop { statement: String },

    #[error(
        "IR Error: '{statement} {requested_level}' exceeds active loop/switch nesting depth of {available_depth}."
    )]
    InvalidBreakContinueLevel {
        statement: String,
        requested_level: usize,
        available_depth: usize,
    },

    #[error("IR Error: '{statement}' nesting level must be a positive integer literal (>= 1).")]
    NonPositiveBreakContinueLevel { statement: String },

    #[error("IR Error: Invalid assignment target in expression lowering: {reason}")]
    InvalidAssignmentTarget { reason: String },

    #[error(
        "IR Error: Virtual register limit (65,535 registers) exceeded while compiling '{scope_name}'."
    )]
    RegisterOverflow { scope_name: String },
}

/// Unified end-to-end compilation error encompassing Lexing/Parsing, Type Checking, and IR Lowering.
#[derive(Debug, Clone, PartialEq)]
pub enum CompilationError {
    Parse(ParseError),
    Type(TypeError),
    Ir(IrError),
}

impl CompilationError {
    pub fn span(&self) -> Span {
        match self {
            Self::Parse(e) => e.span,
            Self::Type(e) => e.span,
            Self::Ir(e) => e.span,
        }
    }
}

impl fmt::Display for CompilationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "{}", e),
            Self::Type(e) => write!(f, "{}", e),
            Self::Ir(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for CompilationError {}

impl From<ParseError> for CompilationError {
    fn from(err: ParseError) -> Self {
        Self::Parse(err)
    }
}

impl From<TypeError> for CompilationError {
    fn from(err: TypeError) -> Self {
        Self::Type(err)
    }
}

impl From<FrontendError> for CompilationError {
    fn from(err: FrontendError) -> Self {
        match err {
            FrontendError::Parse(p) => Self::Parse(p),
            FrontendError::Type(t) => Self::Type(t),
        }
    }
}

impl From<IrError> for CompilationError {
    fn from(err: IrError) -> Self {
        Self::Ir(err)
    }
}
