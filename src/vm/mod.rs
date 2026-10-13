//! # RHP Virtual Machine Execution Engine
//!
//! Implements Technical Point #5 of the RHP Architecture Specification:
//! - **Native Rust Register-Based Bytecode VM Loop:** Executes flat linear [`OpCode`](crate::ir::OpCode)
//!   streams inside a pre-allocated contiguous register file (`Vec<PhpValue>`) for fast cold start
//!   and cache-friendly instruction dispatching.
//! - **Dual-Engine Execution Modes (`RunMode`):** Supports both Development Mode (`RunMode::Dev`,
//!   direct recursive AST evaluation) and Production / Build Mode (`RunMode::Build`, linear bytecode
//!   register VM with optional JIT flag).
//! - **Unified Dynamic Value (`PhpValue`):** Supports primitives, native Rust numeric subtypes
//!   (`Int::i64()`, `Float::f64()`, etc.), copy-on-write strings, explicit collection subtypes
//!   (`Arr::list`, `Arr::dict`, `Arr::set`, `Arr::hashMap`), closures, objects, and futures.

pub mod engine;
pub mod error;
pub mod facade;
pub mod value;

use std::collections::HashMap;

use crate::ast::{
    expr::{BinaryOp, Expr, ExprKind, Literal, StringPart, UnaryOp},
    types::CastType,
    ASTNode,
};
use crate::ir::{compile_to_bytecode, lower_typed_program, CompilationError, IrModule};
use crate::type_system::{parse_and_check, parse_and_check_with_file_id};

pub use engine::{CallFrame, DEFAULT_MAX_CALL_DEPTH, VM};
pub use error::{RuntimeError, RuntimeErrorKind, StackTraceEntry};
pub use facade::{dispatch_facade_call, FacadeResult};
pub use value::{PhpValue, VmClosure, VmFuture, VmObject};

/// Type alias matching the architecture specification sketch (`Result<Value, RuntimeError>`).
pub type Value = PhpValue;

/// Dual-engine execution mode selector (`rhp run dev` vs `rhp run build`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Development Mode: Direct recursive AST evaluation with instant cold start.
    Dev {
        hot_reload: bool,
        dap_port: Option<u16>,
    },
    /// Production / Build Mode: Register-based Bytecode VM with optional JIT compilation.
    Build { jit_enabled: bool, opt_level: u8 },
}

impl Default for RunMode {
    fn default() -> Self {
        Self::Build {
            jit_enabled: false,
            opt_level: 2,
        }
    }
}

/// Local variable scope environment used by the Development Mode direct AST evaluator (`eval_expr`).
#[derive(Debug, Clone, Default)]
pub struct Environment {
    vars: HashMap<String, PhpValue>,
}

impl Environment {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: impl Into<String>, value: PhpValue) {
        let raw = name.into();
        let norm = if raw.starts_with('$') {
            raw
        } else {
            format!("${}", raw)
        };
        self.vars.insert(norm, value);
    }

    pub fn get(&self, name: &str) -> Option<&PhpValue> {
        if name.starts_with('$') {
            self.vars.get(name)
        } else {
            let norm = format!("${}", name);
            self.vars.get(&norm)
        }
    }
}

/// Execute an [`ASTNode`] (`Expr`) under the specified [`RunMode`], matching the architecture
/// specification sketch:
///
/// ```ignore
/// pub fn execute(mode: RunMode, ast: &ASTNode, env: &mut Environment) -> Result<Value, RuntimeError> {
///     match mode {
///         RunMode::Dev { .. } => eval_expr(ast, env),
///         RunMode::Build { jit_enabled, .. } => {
///             let bytecode = compile_to_bytecode(ast)?;
///             VM::new(bytecode, jit_enabled).run()
///         }
///     }
/// }
/// ```
pub fn execute(
    mode: RunMode,
    ast: &ASTNode<'_>,
    env: &mut Environment,
) -> Result<Value, RuntimeError> {
    match mode {
        RunMode::Dev { .. } => eval_expr(ast, env),
        RunMode::Build {
            jit_enabled,
            opt_level,
        } => {
            let bytecode = compile_to_bytecode(ast).map_err(|ir_err| {
                RuntimeError::new(
                    RuntimeErrorKind::Custom {
                        message: ir_err.to_string(),
                    },
                    ir_err.span,
                )
            })?;
            let mut vm = VM::new(bytecode, jit_enabled);
            if jit_enabled {
                vm = vm.with_jit_config(crate::jit::JitConfig::new().with_opt_level(opt_level));
            }
            vm.run()
        }
    }
}

/// Direct recursive AST expression evaluator for Development Mode (`rhp run dev`).
pub fn eval_expr(node: &ASTNode<'_>, env: &mut Environment) -> Result<Value, RuntimeError> {
    match &node.kind {
        ExprKind::Grouping(inner) => eval_expr(inner, env),
        ExprKind::Literal(lit) => match lit {
            Literal::Null => Ok(PhpValue::Null),
            Literal::Bool(b) => Ok(PhpValue::Bool(*b)),
            Literal::Int { value, .. } => {
                if let Ok(narrow) = i32::try_from(*value) {
                    Ok(PhpValue::Int(narrow))
                } else {
                    Ok(PhpValue::RustInt {
                        kind: crate::type_system::types::RustIntKind::I64,
                        value: *value as i128,
                    })
                }
            }
            Literal::Float { value, .. } => Ok(PhpValue::Float(*value as f32)),
            Literal::String { value, .. } => Ok(PhpValue::string(value.clone())),
            Literal::InterpolatedString { parts, .. } => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        StringPart::Literal(s) => out.push_str(s),
                        StringPart::Expr(sub) => {
                            let val = eval_expr(sub, env)?;
                            out.push_str(&val.to_php_string());
                        }
                    }
                }
                Ok(PhpValue::string(out))
            }
        },
        ExprKind::Variable(var) => Ok(env.get(var.name).cloned().unwrap_or(PhpValue::Null)),
        ExprKind::Unary { op, expr: inner, .. } => {
            let v = eval_expr(inner, env)?;
            match op {
                UnaryOp::Plus => Ok(PhpValue::Int(v.to_i32().unwrap_or(0))),
                UnaryOp::Neg => Ok(PhpValue::Int(-v.to_i32().unwrap_or(0))),
                UnaryOp::Not => Ok(PhpValue::Bool(!v.is_truthy())),
                UnaryOp::BitNot => Ok(PhpValue::Int(!v.to_i32().unwrap_or(0))),
                _ => Ok(v),
            }
        }
        ExprKind::Binary { op, left, right } => {
            let lhs = eval_expr(left, env)?;
            let rhs = eval_expr(right, env)?;
            match op {
                BinaryOp::Add => Ok(PhpValue::Int(
                    lhs.to_i32().unwrap_or(0).wrapping_add(rhs.to_i32().unwrap_or(0)),
                )),
                BinaryOp::Sub => Ok(PhpValue::Int(
                    lhs.to_i32().unwrap_or(0).wrapping_sub(rhs.to_i32().unwrap_or(0)),
                )),
                BinaryOp::Mul => Ok(PhpValue::Int(
                    lhs.to_i32().unwrap_or(0).wrapping_mul(rhs.to_i32().unwrap_or(0)),
                )),
                BinaryOp::Div => {
                    let b = rhs.to_i32().unwrap_or(0);
                    if b == 0 {
                        return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero, node.span));
                    }
                    if lhs.to_i32().unwrap_or(0) % b == 0 {
                        Ok(PhpValue::Int(lhs.to_i32().unwrap_or(0) / b))
                    } else {
                        Ok(PhpValue::Float((lhs.to_i32().unwrap_or(0) as f32) / (b as f32)))
                    }
                }
                BinaryOp::Concat => Ok(PhpValue::string(format!(
                    "{}{}",
                    lhs.to_php_string(),
                    rhs.to_php_string()
                ))),
                BinaryOp::Eq | BinaryOp::Identical => Ok(PhpValue::Bool(lhs == rhs)),
                BinaryOp::NotEq | BinaryOp::NotIdentical => Ok(PhpValue::Bool(lhs != rhs)),
                BinaryOp::Lt => Ok(PhpValue::Bool(lhs < rhs)),
                BinaryOp::LtEq => Ok(PhpValue::Bool(lhs <= rhs)),
                BinaryOp::Gt => Ok(PhpValue::Bool(lhs > rhs)),
                BinaryOp::GtEq => Ok(PhpValue::Bool(lhs >= rhs)),
                _ => {
                    let chunk = compile_to_bytecode(node).map_err(|e| {
                        RuntimeError::new(
                            RuntimeErrorKind::Custom {
                                message: e.to_string(),
                            },
                            node.span,
                        )
                    })?;
                    VM::new(chunk, false).run()
                }
            }
        }
        ExprKind::Cast { kind, expr: inner } => {
            let v = eval_expr(inner, env)?;
            match kind {
                CastType::Int => Ok(PhpValue::Int(v.to_i32().unwrap_or(0))),
                CastType::Float => Ok(PhpValue::Float(v.to_f32().unwrap_or(0.0))),
                CastType::String => Ok(PhpValue::string(v.to_php_string())),
                CastType::Bool => Ok(PhpValue::Bool(v.is_truthy())),
                _ => Ok(v),
            }
        }
        _ => {
            let chunk = compile_to_bytecode(node).map_err(|e| {
                RuntimeError::new(
                    RuntimeErrorKind::Custom {
                        message: e.to_string(),
                    },
                    node.span,
                )
            })?;
            VM::new(chunk, false).run()
        }
    }
}

/// Error returned by the end-to-end compile-and-run pipeline (`run_source`).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExecutionError {
    #[error(transparent)]
    Compilation(#[from] CompilationError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

/// Compile and execute an RHP source program inside the Register Bytecode VM, returning the
/// final return value of `<main>`.
pub fn run_source(source: &str) -> Result<PhpValue, ExecutionError> {
    let (_, val) = run_source_with_vm(source)?;
    Ok(val)
}

/// Compile and execute an RHP source program inside the Register Bytecode VM, returning both
/// the [`VM`] instance (to inspect variables, globals, or output) and the `<main>` return value.
pub fn run_source_with_vm(source: &str) -> Result<(VM, PhpValue), ExecutionError> {
    let typed = parse_and_check(source).map_err(CompilationError::from)?;
    let hierarchy = typed.type_info.hierarchy.clone();
    let module = lower_typed_program(&typed).map_err(CompilationError::from)?;
    let mut vm = VM::new(module, false).with_type_hierarchy(hierarchy);
    let result = vm.run()?;
    Ok((vm, result))
}

/// Compile and execute an RHP source program with an explicit `file_id` for [`SourceMap`](crate::lexer::span::SourceMap) diagnostics.
pub fn run_source_with_file_id(
    source: &str,
    file_id: usize,
) -> Result<(VM, PhpValue), ExecutionError> {
    let typed = parse_and_check_with_file_id(source, file_id).map_err(CompilationError::from)?;
    let hierarchy = typed.type_info.hierarchy.clone();
    let module = lower_typed_program(&typed).map_err(CompilationError::from)?;
    let mut vm = VM::new(module, false).with_type_hierarchy(hierarchy);
    let result = vm.run()?;
    Ok((vm, result))
}

/// Execute an already-compiled [`IrModule`] in a fresh [`VM`] instance.
pub fn run_module(module: IrModule, jit_enabled: bool) -> Result<PhpValue, RuntimeError> {
    let mut vm = VM::new(module, jit_enabled);
    vm.run()
}

/// Compile and execute a standalone [`Expr`] inside the Register Bytecode VM.
pub fn run_expr(expr: &Expr<'_>) -> Result<PhpValue, ExecutionError> {
    let chunk = compile_to_bytecode(expr).map_err(CompilationError::from)?;
    let val = VM::new(chunk, false).run()?;
    Ok(val)
}
