//! # RHP Medium / Gradual Type System Engine
//!
//! Implements Technical Point #3 of the RHP Architecture Specification:
//! - **Function & Method Boundaries:** Enforces strict parameter and return type signatures across
//!   function, method, interface, trait, and enum boundaries to guarantee API predictability,
//!   IDE intelligence, and compile-time boundary safety.
//! - **Function Bodies:** Evaluates local variables and expressions using dynamic type inference
//!   and flow-sensitive assignment tracking, preserving clean developer ergonomics and rapid prototyping.
//! - **Reference Mechanics:** Forbids explicit pass-by-reference (`&$var`) and closure reference
//!   captures (`use (&$var)`) in favor of explicit returns, object handles, and Copy-On-Write (COW) collections.

pub mod checker;
pub mod env;
pub mod error;
pub mod types;

pub use checker::{TypeCheckResult, TypeChecker};
pub use env::{
    resolve_builtin_fluent_method, resolve_facade_static_call, TypeEnvironment, VariableBinding,
};
pub use error::{TypeError, TypeErrorKind};
pub use types::{
    ClassTypeInfo, CollectionSubtype, EnumTypeInfo, FunctionSignature, InterfaceTypeInfo,
    ParamSignature, PropertyTypeInfo, RhpType, RustFloatKind, RustIntKind, TypeHierarchy,
};

use crate::ast::Program;
use crate::lexer::span::Span;
use crate::parser::{error::ParseError, parse, parse_with_file_id};
use std::fmt;

/// A parsed and type-checked RHP program ready for Intermediate Representation (IR) lowering
/// or Development Mode AST evaluation.
#[derive(Debug, Clone)]
pub struct TypedProgram<'s> {
    pub program: Program<'s>,
    pub type_info: TypeCheckResult,
}

/// Unified frontend compilation error (combining Lex/Parse errors and Type System errors).
#[derive(Debug, Clone, PartialEq)]
pub enum FrontendError {
    Parse(ParseError),
    Type(TypeError),
}

impl FrontendError {
    pub fn span(&self) -> Span {
        match self {
            FrontendError::Parse(e) => e.span,
            FrontendError::Type(e) => e.span,
        }
    }
}

impl fmt::Display for FrontendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrontendError::Parse(e) => write!(f, "{}", e),
            FrontendError::Type(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for FrontendError {}

impl From<ParseError> for FrontendError {
    fn from(err: ParseError) -> Self {
        FrontendError::Parse(err)
    }
}

impl From<TypeError> for FrontendError {
    fn from(err: TypeError) -> Self {
        FrontendError::Type(err)
    }
}

/// Type-check an already-parsed AST `Program`, enforcing strict boundary signatures
/// and performing dynamic type inference across function and script bodies.
pub fn check_program(program: &Program<'_>) -> Result<TypeCheckResult, TypeError> {
    TypeChecker::new().check_program(program)
}

/// Parse source code and run the Medium/Gradual Type Checker in one pass.
pub fn parse_and_check<'s>(source: &'s str) -> Result<TypedProgram<'s>, FrontendError> {
    let program = parse(source)?;
    let type_info = check_program(&program)?;
    Ok(TypedProgram { program, type_info })
}

/// Parse source code with an explicit `file_id` and run the Medium/Gradual Type Checker.
pub fn parse_and_check_with_file_id<'s>(
    source: &'s str,
    file_id: usize,
) -> Result<TypedProgram<'s>, FrontendError> {
    let program = parse_with_file_id(source, file_id)?;
    let type_info = check_program(&program)?;
    Ok(TypedProgram { program, type_info })
}
