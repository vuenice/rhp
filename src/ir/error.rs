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

    #[error("Fatal Error: Class '{class_name}' cannot extend itself or form an inheritance cycle.")]
    InheritanceCycle { class_name: String },

    #[error("Fatal Error: Class '{class_name}' contains abstract method '{declaring_class}::{method_name}()' and must therefore be declared abstract or implement it.")]
    UnimplementedAbstractMethod {
        class_name: String,
        method_name: String,
        declaring_class: String,
    },

    #[error("Fatal Error: Class '{class_name}' must implement abstract method '{trait_name}::{method_name}()'.")]
    TraitAbstractMethodUnimplemented {
        class_name: String,
        trait_name: String,
        method_name: String,
    },

    #[error("Fatal Error: Method '{class_name}::{method_name}()' cannot override final method '{parent_name}::{method_name}()'.")]
    CannotOverrideFinalMethod {
        class_name: String,
        parent_name: String,
        method_name: String,
    },

    #[error("Fatal Error: Access level to '{class_name}::{method_name}()' must be {parent_visibility} (as in class '{parent_name}') or weaker.")]
    NarrowedVisibility {
        class_name: String,
        parent_name: String,
        method_name: String,
        parent_visibility: String,
    },

    #[error("Fatal Error: Class '{class_name}' cannot extend final class '{parent_name}'.")]
    CannotExtendFinalClass {
        class_name: String,
        parent_name: String,
    },

    #[error("Fatal Error: Class '{class_name}' must implement interface method '{interface_name}::{method_name}()'.")]
    InterfaceNotImplemented {
        class_name: String,
        interface_name: String,
        method_name: String,
    },

    #[error("Fatal Error: Abstract method '{class_name}::{method_name}()' cannot be declared private.")]
    AbstractMethodPrivate {
        class_name: String,
        method_name: String,
    },

    #[error("Fatal Error: Class '{class_name}' cannot extend sealed class '{parent_name}'.")]
    CannotExtendSealedClass {
        class_name: String,
        parent_name: String,
    },

    #[error("Fatal Error: Method '{class_name}::{method_name}()' has '#[Override]' or 'override' modifier but no matching parent or interface method was found.")]
    MethodDoesNotOverride {
        class_name: String,
        method_name: String,
    },

    #[error("Fatal Error: Trait method collision: '{trait_a}::{method_name}()' and '{trait_b}::{method_name}()' collide in class '{class_name}'. Explicitly declare '{class_name}::{method_name}()' to resolve the conflict.")]
    TraitMethodCollision {
        class_name: String,
        method_name: String,
        trait_a: String,
        trait_b: String,
    },
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
