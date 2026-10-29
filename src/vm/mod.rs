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

pub mod arena;
pub mod encoding;
pub mod engine;
pub mod error;
pub mod facade;
pub mod fiber;
pub mod format;
pub mod memory;
pub mod parent_process;
pub mod rust_loader;
pub mod value;
pub mod worker;

use std::collections::HashMap;

use crate::ast::{
    expr::{BinaryOp, Expr, ExprKind, Literal, StringPart, UnaryOp},
    types::CastType,
    ASTNode,
};
use crate::ir::{compile_to_bytecode, lower_typed_program, CompilationError, IrModule};
use crate::type_system::{parse_and_check, parse_and_check_with_file_id};

pub use arena::{
    get_global_arena_stats, reset_global_arena_stats, ArenaMarker, RequestArena, RequestArenaStats,
    RequestDropStats,
};
pub use engine::{CallFrame, DEFAULT_MAX_CALL_DEPTH, VM};
pub use error::{RuntimeError, RuntimeErrorKind, StackTraceEntry};
pub use facade::{
    append_log, clear_log_history, clear_request_context, clear_runtime_closure_macros, clear_tier2_plugins,
    dispatch_facade_call, extract_array_elements, extract_dict_elements, extract_dict_map,
    extract_map_elements, flatten_elements, get_recorded_logs, get_runtime_closure_macro,
    has_runtime_closure_macro, has_tier2_plugin, init_cookie_data, init_request_params, init_session_data,
    parse_json_value, register_runtime_closure_macro, register_tier2_plugin, register_tier2_plugin_handler,
    FacadeResult, LogRecord, PluginFacade, Tier2PluginHandler,
};
pub use fiber::{
    current_fiber_id, current_fiber_name, current_fiber_state, current_fiber_stats, enter_fiber,
    get_fiber_local, is_current_fiber, record_fiber_io_finish, record_fiber_io_start,
    record_fiber_yield, set_fiber_local, with_current_fiber, Fiber, FiberContext, FiberId,
    FiberScheduler, FiberScopeGuard, FiberState, FiberStats, FiberStepResult, IoKind, IoWaitInfo,
    SchedulerStats,
};
pub use format::{format_rust_template, format_sys_args};
pub use indexmap::IndexMap;
pub use memory::{
    cow_detachments, current_usage, estimate_value_size, gc_pause_ms, gc_runs, get_memory_stats,
    is_gc_free, live_objects, peak_usage, record_alloc, record_cow_detachment, record_dealloc,
    record_live_object, release_live_object, reset_memory_tracker, reset_peak, MemoryScope,
    MemoryStats, OwnershipEnforcer,
};
pub use parent_process::{EngineState, FrozenValue, ParentProcess};
pub use rust_loader::{
    clear_rust_modules, extract_pub_functions, infer_facade_name, load_and_register_rust_file,
    resolve_rust_file_path, to_pascal_case, LoadedRustModule, RustCacheStore, RustLogEntry,
    RustLogStore,
};
pub use value::{FutureState, PhpValue, VmClosure, VmFuture, VmObject};
pub use worker::{
    deserialize_response, execute_isolated_request, serialize_response, WorkerError, WorkerHandle,
    WorkerModel, WorkerPayload, WorkerPool, WorkerPoolBuilder, WorkerPoolConfig, WorkerPoolStats,
    WorkerRequest, WorkerResponse, WorkerResponseStatus,
};

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
                BinaryOp::Eq => Ok(PhpValue::Bool(lhs.loose_eq(&rhs))),
                BinaryOp::NotEq => Ok(PhpValue::Bool(!lhs.loose_eq(&rhs))),
                BinaryOp::Identical => Ok(PhpValue::Bool(lhs.strict_eq(&rhs))),
                BinaryOp::NotIdentical => Ok(PhpValue::Bool(!lhs.strict_eq(&rhs))),
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
        ExprKind::Call { callee, args } => {
            if let ExprKind::Ident(ident) = &callee.kind {
                match ident.name {
                    "int" | "Int" => {
                        if let Some(first) = args.first() {
                            let v = eval_expr(&first.value, env)?;
                            return Ok(PhpValue::Int(v.to_i32().unwrap_or(0)));
                        }
                        return Ok(PhpValue::Int(0));
                    }
                    "float" | "Float" => {
                        if let Some(first) = args.first() {
                            let v = eval_expr(&first.value, env)?;
                            return Ok(PhpValue::Float(v.to_f32().unwrap_or(0.0)));
                        }
                        return Ok(PhpValue::Float(0.0));
                    }
                    "bool" | "Bool" => {
                        if let Some(first) = args.first() {
                            let v = eval_expr(&first.value, env)?;
                            return Ok(PhpValue::Bool(v.is_truthy()));
                        }
                        return Ok(PhpValue::Bool(false));
                    }
                    _ => {}
                }
            }
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
    run_source_with_vm_and_jit(source, false)
}

/// Compile and execute an RHP source program inside the Register Bytecode VM with optional JIT,
/// returning both the [`VM`] instance and the `<main>` return value.
pub fn run_source_with_vm_and_jit(
    source: &str,
    jit_enabled: bool,
) -> Result<(VM, PhpValue), ExecutionError> {
    let typed = parse_and_check(source).map_err(CompilationError::from)?;
    let hierarchy = typed.type_info.hierarchy.clone();
    let module = lower_typed_program(&typed).map_err(CompilationError::from)?;
    let mut vm = VM::new(module, jit_enabled).with_type_hierarchy(hierarchy);
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

/// Compile and execute an RHP source program with an explicit file path for `__FILE__` and `__DIR__` resolution.
pub fn run_source_with_path(source: &str, path: impl Into<String>) -> Result<PhpValue, ExecutionError> {
    let (_, val) = run_source_with_path_and_vm(source, path)?;
    Ok(val)
}

/// Compile and execute an RHP source program with an explicit file path, returning both VM and return value.
pub fn run_source_with_path_and_vm(
    source: &str,
    path: impl Into<String>,
) -> Result<(VM, PhpValue), ExecutionError> {
    run_source_with_path_vm_and_jit(source, path, false)
}

/// Compile and execute an RHP source program with an explicit file path and JIT option.
pub fn run_source_with_path_vm_and_jit(
    source: &str,
    path: impl Into<String>,
    jit_enabled: bool,
) -> Result<(VM, PhpValue), ExecutionError> {
    let typed = crate::type_system::parse_and_check_with_path(source, path).map_err(CompilationError::from)?;
    let hierarchy = typed.type_info.hierarchy.clone();
    let module = lower_typed_program(&typed).map_err(CompilationError::from)?;
    let mut vm = VM::new(module, jit_enabled).with_type_hierarchy(hierarchy);
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

/// Compile and execute an RHP source program inside a local execution fiber, returning
/// both the final return value and the collected fiber execution telemetry and statistics.
pub fn run_source_in_fiber(
    source: &str,
    fiber_name: &str,
) -> Result<(PhpValue, FiberStats), ExecutionError> {
    let typed = parse_and_check(source).map_err(CompilationError::from)?;
    let hierarchy = typed.type_info.hierarchy.clone();
    let module = lower_typed_program(&typed).map_err(CompilationError::from)?;
    let mut vm = VM::new(module, false).with_type_hierarchy(hierarchy);
    let (result, stats) = vm.run_in_fiber(fiber_name)?;
    Ok((result, stats))
}

/// Compile and execute an RHP source program inside a local execution fiber with pre-populated
/// fiber-local context storage.
pub fn run_source_in_fiber_with_context(
    source: &str,
    fiber_name: &str,
    context: IndexMap<String, PhpValue>,
) -> Result<(PhpValue, FiberStats), ExecutionError> {
    let typed = parse_and_check(source).map_err(CompilationError::from)?;
    let hierarchy = typed.type_info.hierarchy.clone();
    let module = lower_typed_program(&typed).map_err(CompilationError::from)?;
    let mut vm = VM::new(module, false).with_type_hierarchy(hierarchy);
    let (result, stats) = vm.run_in_fiber_with_context(fiber_name, context)?;
    Ok((result, stats))
}
