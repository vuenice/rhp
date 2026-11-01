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

pub mod class_table;
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

pub use class_table::{ClassInfo, ClassTable, MethodId, PropSlot};
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
    pub is_reference: bool,
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
    pub declaring_class_id: Option<ClassId>,
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
    pub trait_uses: Vec<CompiledTraitUse>,
    pub properties: Vec<CompiledProperty>,
    pub constants: Vec<CompiledClassConst>,
    pub methods: HashMap<String, FunctionId>,
    pub static_forward: Option<FunctionId>,
    pub span: Span,
}

/// Compiled trait use statement descriptor with conflict resolution and aliasing adaptations.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledTraitUse {
    pub traits: Vec<String>,
    pub adaptations: Vec<CompiledTraitAdaptation>,
    pub span: Span,
}

/// Compiled trait adaptation rule (`insteadof` or `as`).
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledTraitAdaptation {
    InsteadOf {
        span: Span,
        trait_name: Option<String>,
        method: String,
        instead_of: Vec<String>,
    },
    Alias {
        span: Span,
        trait_name: Option<String>,
        method: String,
        visibility: Option<Visibility>,
        alias: Option<String>,
    },
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
    /// Global runtime class table with flattened vtables and prop layouts.
    pub class_table: ClassTable,
    /// Read-only static metadata table emitted by the compiler (Reflection alternative).
    pub metadata: crate::metadata::ProgramMetadata,
    /// Optional primary file path of the compiled source code.
    pub file_path: Option<String>,
    /// Table of source file paths indexed by file_id for multi-file stack trace resolution.
    pub source_files: HashMap<usize, String>,
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
        let clean = name.strip_prefix('\\').unwrap_or(name);
        if let Some(id) = self.class_index.get(clean) {
            return self.classes.get(id.index());
        }
        if let Some(id) = self.class_index.get(name) {
            return self.classes.get(id.index());
        }
        None
    }

    /// Look up linked [`ClassInfo`] from the class table by [`ClassId`].
    #[inline]
    pub fn get_class_info(&self, id: ClassId) -> Option<&ClassInfo> {
        self.class_table.get(id)
    }

    /// Look up linked [`ClassInfo`] from the class table by name.
    #[inline]
    pub fn get_class_info_by_name(&self, name: &str) -> Option<&ClassInfo> {
        self.class_table.get_by_name(name)
    }

    /// Build and link the runtime [`ClassTable`] from compiled classes and interfaces.
    pub fn build_and_link_class_table(&mut self) -> Result<(), IrError> {
        let mut table = ClassTable::new();

        // 1. Populate classes and traits
        for cls in &self.classes {
            let mut info = ClassInfo::new(cls.id, cls.name.as_str(), cls.span);
            info.parent_name = cls.extends.clone();
            info.interface_names = cls.implements.clone();
            info.trait_names = cls.traits.clone();
            info.is_abstract = cls.modifiers.is_abstract;
            info.is_final = cls.modifiers.is_final;
            info.is_readonly = cls.modifiers.is_readonly;
            info.is_singleton = cls.modifiers.is_singleton;
            info.is_static_only = cls.modifiers.is_static_only;
            info.is_trait = cls.is_trait;
            info.is_interface = false;
            info.own_props = cls.properties.clone();
            for (m_name, &fn_id) in &cls.methods {
                info.own_methods.insert(m_name.as_str().into(), fn_id);
            }
            table.add_class(info);
        }

        // 2. Populate interfaces
        for (iface_name, iface) in &self.interfaces {
            let mut info = ClassInfo::new(ClassId(table.classes.len() as u32), iface_name.as_str(), iface.span);
            info.interface_names = iface.extends.clone();
            info.is_abstract = true;
            info.is_interface = true;
            for (m_name, &fn_id) in &iface.methods {
                info.own_methods.insert(m_name.as_str().into(), fn_id);
            }
            table.add_class(info);
        }

        // 3. Link the class table
        table.link(&mut self.functions, &mut self.function_index)?;

        // 4. Update CompiledClass methods and traits with final linked vtables
        for cls in &mut self.classes {
            if let Some(ci) = table.get(cls.id) {
                for (m_name, &mid) in &ci.vtable {
                    cls.methods.insert(m_name.to_string(), mid);
                }
                cls.static_forward = ci.static_forward;
            }
        }

        self.class_table = table;
        Ok(())
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
