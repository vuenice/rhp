use std::fmt;
use thiserror::Error;

use crate::lexer::span::{SourceMap, Span};
use crate::type_system::types::RhpType;

/// Stack trace frame captured when a [`RuntimeError`] occurs inside the VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackTraceEntry {
    /// Name of the active function, method, closure, or `<main>`.
    pub function_name: String,
    /// Instruction pointer index within the frame's [`BytecodeChunk`](crate::ir::BytecodeChunk).
    pub ip: usize,
    /// Exact source code span mapped to `ip`.
    pub span: Span,
}

impl fmt::Display for StackTraceEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "at {} (ip={}, line {}, col {})",
            self.function_name, self.ip, self.span.line, self.span.col
        )
    }
}

/// Specific runtime error categories produced by the RHP Register Virtual Machine.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum RuntimeErrorKind {
    #[error(
        "TypeError: Boundary contract violation: expected value of type '{expected}', got '{actual}' ({value_preview})"
    )]
    TypeContractViolation {
        expected: RhpType,
        actual: String,
        value_preview: String,
    },

    #[error("DivisionByZeroError: Division or modulo by zero")]
    DivisionByZero,

    #[error("TypeError: Unsupported operand types for '{op}': {lhs_type} and {rhs_type}")]
    InvalidBinaryOperands {
        op: String,
        lhs_type: String,
        rhs_type: String,
    },

    #[error("TypeError: Unsupported operand type for unary '{op}': {operand_type}")]
    InvalidUnaryOperand { op: String, operand_type: String },

    #[error("Error: Call to undefined function '{name}()'")]
    UndefinedFunction { name: String },

    #[error("Error: Call to undefined method '{class}::{method}()'")]
    UndefinedMethod { class: String, method: String },

    #[error("Error: Call to undefined facade method '{facade}::{method}()'")]
    UndefinedFacadeMethod { facade: String, method: String },

    #[error("Error: Class '{name}' not found")]
    UndefinedClass { name: String },

    #[error("Error: Undefined property '{class}::${property}'")]
    UndefinedProperty { class: String, property: String },

    #[error("Error: Undefined class constant '{class}::{constant}'")]
    UndefinedClassConstant { class: String, constant: String },

    #[error("Error: Cannot modify readonly property '{class}::${property}'")]
    ReadonlyPropertyModification { class: String, property: String },

    #[error("TypeError: Value of type '{type_name}' is not callable")]
    NotCallable { type_name: String },

    #[error("TypeError: Value of type '{type_name}' is not iterable in foreach")]
    NotIterable { type_name: String },

    #[error("TypeError: Cannot index into value of type '{type_name}'")]
    InvalidIndexTarget { type_name: String },

    #[error("OutOfBoundsError: Undefined index or key '{key}'")]
    UndefinedIndex { key: String },

    #[error(
        "ArgumentCountError: Too few arguments to function '{callable}()', {passed} passed and at least {expected} expected"
    )]
    TooFewArguments {
        callable: String,
        passed: usize,
        expected: usize,
    },

    #[error("Error: Unknown named parameter '${name}' in call to '{callable}()'")]
    UnknownNamedArgument { callable: String, name: String },

    #[error("UncaughtException ({class}): {message}")]
    UncaughtException { class: String, message: String },

    #[error("UnhandledMatchError: Unhandled match value '{subject}'")]
    UnhandledMatchError { subject: String },

    #[error("StackOverflowError: Maximum VM call stack depth ({max_depth}) exceeded")]
    StackOverflow { max_depth: usize },

    #[error("VMError: Invalid constant pool index c{index}")]
    InvalidConstantIndex { index: u32 },

    #[error("VMError: Invalid register access r{reg}")]
    InvalidRegisterAccess { reg: u16 },

    #[error("Fatal Error: Cannot instantiate static facade class '{class}'. Use static methods directly.")]
    StaticFacadeInstantiationForbidden { class: String },

    #[error("Fatal Error: Cannot extend static facade class '{class}'. Use '{class}::macro()' to register custom methods dynamically.")]
    StaticFacadeInheritanceForbidden { class: String },

    #[error(
        "Fatal Error: Cannot call non-static method '{method}' of singleton class '{singleton_class}' inside non-singleton class '{caller_class}'."
    )]
    NonStaticSingletonMethodCallForbidden {
        singleton_class: String,
        method: String,
        caller_class: String,
    },

    #[error(
        "Fatal Error: Cannot instantiate singleton class '{singleton_class}' inside non-singleton class '{caller_class}'. Only static methods of singleton classes can be used in non-singleton classes."
    )]
    SingletonInstantiationForbidden {
        singleton_class: String,
        caller_class: String,
    },

    #[error(
        "Fatal Error: Cannot extend singleton class '{parent_name}' from non-singleton class '{class_name}'."
    )]
    NonSingletonExtendsSingletonForbidden {
        class_name: String,
        parent_name: String,
    },

    #[error("RuntimeError: {message}")]
    Custom { message: String },
}

/// Rich runtime error with source [`Span`] and full VM call stack trace.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("{kind} at line {}, column {}", span.line, span.col)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub span: Span,
    pub stack_trace: Vec<StackTraceEntry>,
}

impl RuntimeError {
    pub fn new(kind: RuntimeErrorKind, span: Span) -> Self {
        Self {
            kind,
            span,
            stack_trace: Vec::new(),
        }
    }

    pub fn with_stack_trace(
        kind: RuntimeErrorKind,
        span: Span,
        stack_trace: Vec<StackTraceEntry>,
    ) -> Self {
        Self {
            kind,
            span,
            stack_trace,
        }
    }

    /// Format a detailed diagnostic string with source code snippet and stack trace using a [`SourceMap`].
    pub fn format_with_source_map(&self, source_map: &SourceMap) -> String {
        let mut out = source_map.format_diagnostic(&self.span, &self.kind.to_string());
        if !self.stack_trace.is_empty() {
            out.push_str("\nStack trace:\n");
            for (idx, frame) in self.stack_trace.iter().enumerate() {
                out.push_str(&format!("  #{}: {}\n", idx, frame));
            }
        }
        out
    }
}
