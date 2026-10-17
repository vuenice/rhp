//! # RHP Intermediate Representation (IR) & Bytecode Lowering Engine
//!
//! Implements Technical Point #4 of the RHP Architecture Specification:
//! - **Intermediate Representation (IR) Lowering:** Translates the typed AST into flat,
//!   compact linear register-based bytecode instructions (`OpCode`) optimized for execution
//!   within the RHP Register Virtual Machine and Cranelift JIT pipeline.
//! - **Register Allocation:** Maps local variables, parameters, and expression temporaries
//!   directly to contiguous 16-bit virtual registers (`r0`..`r65535`).
//! - **Boundary Type Guards:** Emits explicit `AssertType` instructions at function/method
//!   parameter boundaries, return boundaries, and locked local variable declarations.
//! - **Facade & Subtyped Collection Lowering:** Lowers `Arr::list`, `Arr::dict`, `Arr::set`,
//!   and `Arr::hashMap` directly into `NewCollection` opcodes, native Rust numeric wrappers
//!   into `WrapRustInt` / `WrapRustFloat`, and `phpFun::*` legacy calls into canonical RHP facades.

pub mod error;
pub mod lower;
pub mod opcode;

use std::collections::HashMap;
use std::fmt;

use crate::ast::{
    expr::Expr,
    types::{ClassModifiers, Visibility},
    Program,
};
use crate::lexer::span::Span;
use crate::type_system::{
    parse_and_check, parse_and_check_with_file_id, types::RhpType, TypeCheckResult, TypedProgram,
};

pub use error::{CompilationError, IrError, IrErrorKind};
pub use lower::IrLowerer;
pub use opcode::{
    BytecodeChunk, CatchTarget, ClassId, Constant, ConstantId, ExceptionHandler, FunctionId,
    IrCollectionKind, IrUnaryOp, OpCode, Reg, RegisterInfo,
};

/// Formal parameter descriptor in a [`CompiledFunction`].
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledParam {
    pub name: String,
    pub reg: Reg,
    pub rhp_type: RhpType,
    pub has_default: bool,
    pub is_variadic: bool,
    pub is_readonly: bool,
    pub visibility: Option<Visibility>,
    pub span: Span,
}

/// Compiled function, method, closure, arrow function, or deferred hook.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFunction {
    pub id: FunctionId,
    pub name: String,
    pub params: Vec<CompiledParam>,
    pub return_type: RhpType,
    pub upvalues: Vec<String>,
    pub is_async: bool,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_closure: bool,
    pub visibility: Visibility,
    pub enclosing_class: Option<String>,
    pub chunk: BytecodeChunk,
    pub span: Span,
}

/// Compiled class or trait property descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledProperty {
    pub name: String,
    pub rhp_type: RhpType,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_readonly: bool,
    pub default_chunk: Option<BytecodeChunk>,
    pub span: Span,
}

/// Compiled class constant descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledClassConst {
    pub name: String,
    pub visibility: Visibility,
    pub is_final: bool,
    pub value_chunk: BytecodeChunk,
    pub span: Span,
}

/// Compiled class or trait metadata and method dispatch table.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledClass {
    pub id: ClassId,
    pub name: String,
    pub is_trait: bool,
    pub modifiers: ClassModifiers,
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub traits: Vec<String>,
    pub properties: Vec<CompiledProperty>,
    pub constants: Vec<CompiledClassConst>,
    pub methods: HashMap<String, FunctionId>,
    pub span: Span,
}

/// Compiled interface contract descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledInterface {
    pub name: String,
    pub extends: Vec<String>,
    pub methods: HashMap<String, FunctionId>,
    pub span: Span,
}

/// Compiled enum case descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEnumCase {
    pub name: String,
    pub value_chunk: Option<BytecodeChunk>,
    pub span: Span,
}

/// Compiled enum descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEnum {
    pub name: String,
    pub backing_type: Option<RhpType>,
    pub implements: Vec<String>,
    pub cases: Vec<CompiledEnumCase>,
    pub methods: HashMap<String, FunctionId>,
    pub span: Span,
}

/// Complete compiled bytecode module produced by lowering a [`TypedProgram`] or [`Program`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IrModule {
    /// Entry-point bytecode chunk for top-level script execution.
    pub main_chunk: BytecodeChunk,
    /// Table of all compiled functions, methods, closures, arrow functions, and defer blocks.
    pub functions: Vec<CompiledFunction>,
    /// Lookup index mapping standalone function names and `Class::method` keys to [`FunctionId`].
    pub function_index: HashMap<String, FunctionId>,
    /// Table of all compiled classes and traits.
    pub classes: Vec<CompiledClass>,
    /// Lookup index mapping class/trait names to [`ClassId`].
    pub class_index: HashMap<String, ClassId>,
    /// Compiled interfaces by name.
    pub interfaces: HashMap<String, CompiledInterface>,
    /// Compiled enums by name.
    pub enums: HashMap<String, CompiledEnum>,
}

impl IrModule {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a compiled function or `Class::method` by name.
    pub fn get_function_by_name(&self, name: &str) -> Option<&CompiledFunction> {
        let id = self.function_index.get(name)?;
        self.functions.get(id.index())
    }

    /// Look up a compiled function by [`FunctionId`].
    #[inline]
    pub fn get_function(&self, id: FunctionId) -> Option<&CompiledFunction> {
        self.functions.get(id.index())
    }

    /// Look up a compiled class or trait by name.
    pub fn get_class_by_name(&self, name: &str) -> Option<&CompiledClass> {
        let id = self.class_index.get(name)?;
        self.classes.get(id.index())
    }

    /// Disassemble the entire compiled module (top-level script and all functions/methods).
    pub fn disassemble(&self) -> String {
        let mut out = self.main_chunk.disassemble("<main>");
        for func in &self.functions {
            out.push('\n');
            out.push_str(&func.chunk.disassemble(&format!("fn {} ({})", func.name, func.id)));
        }
        out
    }
}

impl fmt::Display for IrModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.disassemble())
    }
}

/// Lower a type-checked [`TypedProgram`] into a linear bytecode [`IrModule`].
pub fn lower_typed_program(typed: &TypedProgram<'_>) -> Result<IrModule, IrError> {
    IrLowerer::new(Some(&typed.type_info)).lower_program(&typed.program)
}

/// Lower a type-checked [`TypedProgram`] seeded with an existing parent [`IrModule`].
pub fn lower_typed_program_with_base(
    typed: &TypedProgram<'_>,
    base_module: IrModule,
) -> Result<IrModule, IrError> {
    IrLowerer::with_base_module(Some(&typed.type_info), base_module).lower_program(&typed.program)
}

/// Lower an AST [`Program`] (with optional [`TypeCheckResult`]) into a linear bytecode [`IrModule`].
pub fn lower_program(
    program: &Program<'_>,
    type_info: Option<&TypeCheckResult>,
) -> Result<IrModule, IrError> {
    IrLowerer::new(type_info).lower_program(program)
}

/// Lower a single standalone [`Expr`] (`ASTNode`) into a [`BytecodeChunk`].
///
/// Supports the architecture specification sketch: `let bytecode = compile_to_bytecode(ast)?;`
pub fn compile_to_bytecode(node: &Expr<'_>) -> Result<BytecodeChunk, IrError> {
    IrLowerer::new(None).lower_expr_standalone(node)
}

/// Parse, type-check, and lower RHP source code into a complete [`IrModule`] in one pass.
pub fn compile_source(source: &str) -> Result<IrModule, CompilationError> {
    let typed = parse_and_check(source)?;
    let module = lower_typed_program(&typed)?;
    Ok(module)
}

/// Parse, type-check, and lower RHP source code with an explicit `file_id` into an [`IrModule`].
pub fn compile_source_with_file_id(
    source: &str,
    file_id: usize,
) -> Result<IrModule, CompilationError> {
    let typed = parse_and_check_with_file_id(source, file_id)?;
    let module = lower_typed_program(&typed)?;
    Ok(module)
}
