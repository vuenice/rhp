use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use indexmap::IndexMap;

use crate::ast::expr::BinaryOp;
use crate::ast::types::CastType;
use crate::ir::opcode::{
    BytecodeChunk, Constant, FunctionId, IrCollectionKind, IrUnaryOp, OpCode, Reg,
};
use crate::ir::IrModule;
use crate::jit::{JitConfig, JitEngine};
use crate::lexer::facade::{
    has_custom_helper, is_legacy_php_function, is_reserved_intrinsic, is_static_facade,
    is_static_only, LEGACY_PHP_FUNCTIONS, NATIVE_FACADES, RESERVED_INTRINSICS,
};
use crate::lexer::span::Span;
use crate::type_system::types::{RhpType, TypeHierarchy};
use crate::vm::arena::{RequestArena, RequestDropStats};
use crate::vm::error::{RuntimeError, RuntimeErrorKind, StackTraceEntry};
use crate::vm::exception::{
    is_builtin_throwable, is_throwable_subtype, Frame, RawFrame, Throwable,
};
use crate::vm::facade::{
    arr_get_dot, arr_has_dot, dispatch_facade_call, extract_array_elements, extract_dict_elements,
    extract_dict_map, flatten_elements, get_item_column, get_runtime_closure_macro,
    php_value_natural_cmp, php_value_to_json, random_usize, register_runtime_closure_macro, shuffle_slice,
};
use crate::vm::parent_process::FrozenValue;
use crate::vm::value::{PhpValue, VmClosure, VmFuture, VmObject};

/// Default maximum call stack depth before throwing `StackOverflowError`.
pub const DEFAULT_MAX_CALL_DEPTH: usize = 512;

/// Staged argument pushed onto the VM argument stack by `OpCode::PushArg`.
#[derive(Debug, Clone)]
struct StagedArg {
    value: PhpValue,
    name: Option<String>,
    is_unpack: bool,
    is_reference: bool,
    is_skipped: bool,
    caller_reg: Option<usize>,
}

impl StagedArg {
    fn simple(value: PhpValue) -> Self {
        Self {
            value,
            name: None,
            is_unpack: false,
            is_reference: false,
            is_skipped: false,
            caller_reg: None,
        }
    }
}

/// Active iterator state held for `foreach` loops (`IterInit` / `IterNext`).
///
/// Implements Technical Points #8, #9, #10, and #11 of the RHP Architecture Specification:
/// - Maps sequential lists directly to underlying Rust `Vec<PhpValue>` structures (`ActiveIterator::List`),
///   enabling contiguous cache-line memory streaming during iteration with zero temporary vector allocations.
/// - Maps subtyped key-value dictionaries directly to underlying Rust `IndexMap<String, PhpValue>` structures (`ActiveIterator::Dict`),
///   guaranteeing strict insertion ordering and O(1) key indexing with zero temporary vector allocations.
/// - Maps unique collections directly to Rust `HashSet<PhpValue>` structures (`ActiveIterator::Set`).
/// - Universal general key-value dictionaries supporting complex keys map to `ActiveIterator::Entries`.
#[derive(Debug, Clone)]
pub(crate) enum ActiveIterator {
    /// Contiguous sequential list iterator referencing the underlying Rust `Vec<PhpValue>` directly.
    List {
        items: Rc<RefCell<Vec<PhpValue>>>,
        cursor: usize,
    },
    /// Subtyped key-value dictionary iterator referencing the underlying Rust `IndexMap<String, PhpValue>` directly.
    Dict {
        dict: Rc<RefCell<IndexMap<String, PhpValue>>>,
        cursor: usize,
    },
    /// Subtyped unique set iterator over underlying `HashSet<PhpValue>`.
    Set {
        items: Vec<PhpValue>,
        cursor: usize,
    },
    /// Associative dictionary / key-value map iterator.
    Entries {
        entries: Vec<(PhpValue, PhpValue)>,
        cursor: usize,
    },
    /// Object implementing Iterator interface (`rewind`, `valid`, `current`, `key`, `next`).
    IteratorObject {
        iterator: PhpValue,
        started: bool,
    },
}

#[derive(Debug)]
pub(crate) enum EnumStaticCallResult {
    Handled(PhpValue),
    DispatchedToCatch,
    NotAnEnumMethod,
}

/// Call frame on the register VM execution stack.
#[derive(Debug, Clone)]
pub struct CallFrame {
    /// Human-readable frame label (`<main>`, `calculateTotal`, `Order::total`, etc.).
    pub function_name: String,
    /// `None` for `<main>`, `Some(FunctionId)` for functions, methods, closures, and defer hooks.
    pub func_id: Option<FunctionId>,
    /// Current instruction pointer inside the frame's [`BytecodeChunk`].
    pub ip: usize,
    /// Base index into the VM's flat register file (`vm.registers[base_reg + r.index()]`).
    pub base_reg: usize,
    /// Number of virtual registers allocated to this frame.
    pub num_registers: usize,
    /// Destination register in the caller's frame where the return value should be stored.
    pub caller_dst: Option<usize>,
    /// Late-static-bound calling class for static calls and static:: resolution.
    pub called_class: Option<String>,
    /// Stack of active `try` handler IDs entered in this frame (`EnterTry` / `LeaveTry`).
    pub active_try_handlers: Vec<u16>,
    /// Per-frame iterator table keyed by `iter_reg.index()`.
    pub(crate) iterators: HashMap<usize, ActiveIterator>,
    /// Pass-by-reference bindings: (callee_abs_reg, caller_abs_reg)
    pub ref_bindings: Vec<(usize, usize)>,
}

/// High-Performance Native Rust Register-Based Bytecode Virtual Machine (`VM`).
///
/// Implements Technical Point #5 of the RHP Architecture Specification:
/// - **Flat Contiguous Register File:** All active call frames index into a pre-allocated
///   contiguous `Vec<PhpValue>` register window (`registers[frame.base_reg + reg.0 as usize]`),
///   avoiding per-call heap allocations and maximizing L1/L2 CPU cache locality.
/// - **Tight Fetch-Decode-Execute Loop:** Dispatches flat `Copy`-able [`OpCode`] instructions
///   with zero pointer chasing.
/// - **Gradual Type Boundary Enforcement:** Executes `OpCode::AssertType` at function/method
///   parameter and return boundaries as well as locked local declarations (`int $x`).
/// - **Zero-Cost Exception Table Unwinding:** Unwinds `try`/`catch`/`finally` regions via the
///   chunk's `ExceptionHandler` table when `Throw` or `ThrowMatchError` is executed.
/// - **Post-Response Defer Queue:** Collects `defer { ... }` and `Future::defer(...)` hooks and
///   executes them in LIFO order after the main script payload completes.
/// User-registered shutdown callback hook (`register_shutdown_function`).
#[derive(Debug, Clone)]
pub struct ShutdownHook {
    pub callback: PhpValue,
    pub args: Vec<PhpValue>,
}

/// User-registered error handler callback (`set_error_handler`).
#[derive(Debug, Clone)]
pub struct ErrorHandlerEntry {
    pub callback: PhpValue,
    pub error_levels: i32,
}

/// Default instruction budget (ticks) before cooperative preemption (10,000 instructions).
pub const DEFAULT_INSTRUCTION_BUDGET: u32 = 10_000;
/// Dev Mode loop iteration threshold for emitting a runaway loop warning (1,000 iterations).
pub const DEV_LOOP_WARN_THRESHOLD: u64 = 1_000;
/// Dev Mode loop iteration hard ceiling before raising a fatal InfiniteLoopLimitExceeded error (100,000 iterations / 1 lakh).
pub const DEV_LOOP_HARD_LIMIT: u64 = 100_000;

pub struct VM {
    module: IrModule,
    jit_enabled: bool,
    /// Contiguous virtual register file shared across all active [`CallFrame`]s.
    registers: Vec<PhpValue>,
    /// Active call stack frames.
    frames: Vec<CallFrame>,
    /// Argument staging buffer populated by `OpCode::PushArg`.
    arg_buffer: Vec<StagedArg>,
    /// Upvalue staging buffer populated by `OpCode::CaptureUpvalue`.
    upvalue_buffer: Vec<(String, PhpValue, bool)>,
    /// Global variables (`global $x;`).
    globals: HashMap<String, PhpValue>,
    /// Static class properties (`Class::$prop`).
    static_properties: HashMap<(String, String), PhpValue>,
    /// Cached evaluated class constants and enum cases (`Class::CONST`).
    class_constants: HashMap<(String, String), PhpValue>,
    /// Captured standard output from `echo`, `print()`, and `Sys::print()`.
    output_buffer: String,
    /// Post-response / post-execution deferred function hooks (`OpCode::Defer`).
    defer_queue: Vec<FunctionId>,
    /// Registered shutdown function callbacks (`register_shutdown_function`).
    pub shutdown_hooks: Vec<ShutdownHook>,
    /// Active error handler callbacks stack (`set_error_handler` / `restore_error_handler`).
    pub error_handlers: Vec<ErrorHandlerEntry>,
    /// Active exception handler callbacks stack (`set_exception_handler` / `restore_exception_handler`).
    pub exception_handlers: Vec<PhpValue>,
    /// Live instantiated user class instances for running `__destruct` at shutdown.
    pub live_objects: Vec<std::rc::Weak<RefCell<VmObject>>>,
    /// When true, process::exit() is suppressed on exit() signal, returning RuntimeError instead (useful for tests and embedding).
    pub suppress_process_exit: bool,
    /// Instance-level exit hook to intercept exit() and die() without depending on global state.
    pub exit_hook: Option<std::sync::Arc<dyn Fn(i32) + Send + Sync>>,
    /// Optional nominal type hierarchy for `instanceof` and `AssertType` subtype checks.
    type_hierarchy: Option<TypeHierarchy>,
    /// Maximum allowed call frame depth.
    max_call_depth: usize,
    /// Cranelift JIT compiler backend for hot loops and functions.
    jit_engine: Option<JitEngine>,
    /// Indicates whether this VM is running as the parent process during boot.
    pub is_parent_process: bool,
    /// Request-scoped bump arena providing instantaneous O(1) bulk deallocation.
    pub arena: RequestArena,
    /// IoC Dependency Injection container with compiler-generated factories.
    pub container: crate::metadata::Container,
    /// Primary file path of the executing source script.
    pub file_path: Option<String>,
    /// Table of source file paths indexed by file_id for stack traces.
    pub source_files: HashMap<usize, String>,
    /// When true, function arguments are not captured into exception stack frames (zend.exception_ignore_args).
    pub exception_ignore_args: bool,
    /// Configured instruction tick budget before cooperative preemption (default: 10,000).
    pub tick_budget: u32,
    /// Decrementing instruction tick counter for cooperative preemption.
    pub tick_counter: u32,
    /// Flag indicating whether the VM is executing in Development Mode (enabling loop warnings & hard limits).
    pub dev_mode: bool,
    /// Warning threshold for loop iterations in Dev Mode (default: 1,000).
    pub dev_loop_warn_threshold: u64,
    /// Hard limit for loop iterations in Dev Mode (default: 100,000).
    pub dev_loop_hard_limit: u64,
    /// When true, loop iteration warnings and hard limits are exempted/bypassed.
    pub exempt_loop_limits: bool,
    /// Active iteration counts per loop header: `(Option<FunctionId>, header_ip) -> count`.
    loop_iterations: HashMap<(Option<FunctionId>, u32), u64>,
    /// Set of loop headers that have already emitted a warning in Dev Mode.
    loop_warned: std::collections::HashSet<(Option<FunctionId>, u32)>,
    /// Total number of cooperative preemption events triggered.
    pub total_preemptions: u64,
}

impl VM {
    /// Construct a new Register VM from a compiled [`IrModule`] (or single [`BytecodeChunk`]).
    pub fn new(bytecode: impl Into<IrModule>, jit_enabled: bool) -> Self {
        let module = bytecode.into();
        let initial_regs = (module.main_chunk.num_registers as usize).max(64);
        let jit_engine = if jit_enabled {
            Some(JitEngine::new(JitConfig::new().with_enabled(true)))
        } else {
            None
        };
        let mut container = crate::metadata::Container::new();
        let _ = container.auto_wire(&module.metadata);
        let file_path = module.file_path.clone();
        let source_files = module.source_files.clone();
        Self {
            module,
            jit_enabled,
            registers: Vec::with_capacity(initial_regs * 4),
            frames: Vec::with_capacity(32),
            arg_buffer: Vec::with_capacity(16),
            upvalue_buffer: Vec::with_capacity(8),
            globals: HashMap::new(),
            static_properties: HashMap::new(),
            class_constants: HashMap::new(),
            output_buffer: String::new(),
            defer_queue: Vec::new(),
            shutdown_hooks: Vec::new(),
            error_handlers: Vec::new(),
            exception_handlers: Vec::new(),
            live_objects: Vec::new(),
            suppress_process_exit: false,
            exit_hook: None,
            type_hierarchy: None,
            max_call_depth: DEFAULT_MAX_CALL_DEPTH,
            jit_engine,
            is_parent_process: false,
            arena: RequestArena::new(),
            container,
            file_path,
            source_files,
            exception_ignore_args: true,
            tick_budget: DEFAULT_INSTRUCTION_BUDGET,
            tick_counter: DEFAULT_INSTRUCTION_BUDGET,
            dev_mode: false,
            dev_loop_warn_threshold: DEV_LOOP_WARN_THRESHOLD,
            dev_loop_hard_limit: DEV_LOOP_HARD_LIMIT,
            exempt_loop_limits: false,
            loop_iterations: HashMap::new(),
            loop_warned: std::collections::HashSet::new(),
            total_preemptions: 0,
        }
    }

    /// Attach an instance-level exit hook to intercept exit() and die() calls on this VM instance.
    pub fn with_exit_hook(mut self, hook: impl Fn(i32) + Send + Sync + 'static) -> Self {
        self.exit_hook = Some(std::sync::Arc::new(hook));
        self
    }

    /// Configure whether this VM instance runs in Development Mode (enabling loop warnings and hard limits).
    pub fn with_dev_mode(mut self, dev_mode: bool) -> Self {
        self.dev_mode = dev_mode;
        self
    }

    /// Set the instruction tick budget before cooperative preemption.
    pub fn with_instruction_budget(mut self, budget: u32) -> Self {
        self.tick_budget = budget.max(1);
        self.tick_counter = self.tick_budget;
        self
    }

    /// Set the loop warning and hard limit thresholds in Dev Mode.
    pub fn with_loop_limits(mut self, warn_threshold: u64, hard_limit: u64) -> Self {
        self.dev_loop_warn_threshold = warn_threshold;
        self.dev_loop_hard_limit = hard_limit;
        self
    }

    /// Exempt this VM instance from loop iteration limits and warnings.
    pub fn with_exempt_loop_limits(mut self, exempt: bool) -> Self {
        self.exempt_loop_limits = exempt;
        self
    }

    /// Reset the instruction budget tick counter back to `tick_budget`.
    pub fn reset_instruction_budget(&mut self) {
        self.tick_counter = self.tick_budget;
    }

    /// Attach a source file path to this VM instance for stack trace and __FILE__ resolution.
    pub fn with_file_path(mut self, path: impl Into<String>) -> Self {
        let p = path.into();
        self.file_path = Some(p.clone());
        self.source_files.insert(0, p);
        self
    }

    /// Register a source file path by its file_id for multi-file stack trace resolution.
    pub fn with_source_file(mut self, file_id: usize, path: impl Into<String>) -> Self {
        self.source_files.insert(file_id, path.into());
        self
    }

    /// Configure whether function arguments are ignored in exception stack traces (zend.exception_ignore_args).
    pub fn with_exception_ignore_args(mut self, ignore: bool) -> Self {
        self.exception_ignore_args = ignore;
        self
    }

    /// Resolve a file name from file_id for stack trace frames.
    pub fn resolve_source_file(&self, file_id: usize) -> String {
        if let Some(path) = self.source_files.get(&file_id) {
            return path.clone();
        }
        if let Some(ref path) = self.file_path {
            return path.clone();
        }
        if let Some(ref path) = self.module.file_path {
            return path.clone();
        }
        if file_id == 0 {
            String::new()
        } else {
            format!("file_{}.rhp", file_id)
        }
    }

    /// Capture cheap, unsymbolicated frame descriptors from the current VM call stack.
    /// Stores instruction pointers and function handles without reading span tables or interned strings.
    fn capture_raw_trace(&self) -> Vec<RawFrame> {
        let n = self.frames.len();
        if n <= 1 {
            return Vec::new();
        }
        let mut raw = Vec::with_capacity(n - 1);
        for i in (1..n).rev() {
            let callee_frame = &self.frames[i];
            let caller_frame = &self.frames[i - 1];
            let callee_func_id = callee_frame.func_id;
            let caller_func_id = caller_frame.func_id;
            let caller_ip = caller_frame.ip.saturating_sub(1);
            let caller_span = self
                .chunk_for_func(caller_func_id)
                .span_at(caller_ip)
                .unwrap_or_default();

            let (class_name, is_static) = if let Some(fid) = callee_func_id {
                if let Some(f) = self.module.get_function(fid) {
                    (f.enclosing_class.clone(), f.is_static)
                } else {
                    (None, false)
                }
            } else {
                (None, false)
            };

            let args = if !self.exception_ignore_args {
                if let Some(fid) = callee_func_id {
                    if let Some(f) = self.module.get_function(fid) {
                        f.params
                            .iter()
                            .filter_map(|p| {
                                let idx = callee_frame.base_reg + p.reg.index();
                                if idx < self.registers.len() {
                                    Some(self.registers[idx].clone())
                                } else {
                                    None
                                }
                            })
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            raw.push(RawFrame {
                callee_func_id,
                function_name: callee_frame.function_name.clone(),
                class_name,
                is_static,
                caller_func_id,
                caller_ip,
                caller_span,
                args,
            });
        }
        raw
    }

    /// Symbolicate a lazily-captured raw trace into fully resolved stack [`Frame`]s.
    /// Called on-demand when `getTrace()` or `getTraceAsString()` is first invoked.
    pub fn symbolicate_trace(&self, throwable: &mut Throwable) {
        if throwable.is_symbolicated {
            return;
        }
        let mut frames = Vec::with_capacity(throwable.raw_trace.len());
        for raw in &throwable.raw_trace {
            let caller_chunk = self.chunk_for_func(raw.caller_func_id);
            let span = caller_chunk.span_at(raw.caller_ip).unwrap_or(raw.caller_span);
            let file = self.resolve_source_file(span.file_id);
            let line = if span.line > 0 { span.line } else { 1 };

            let function = if let Some(pos) = raw.function_name.rfind("::") {
                raw.function_name[pos + 2..].to_string()
            } else {
                raw.function_name.clone()
            };

            let frame_type = if raw.class_name.is_some() {
                Some(if raw.is_static { "::".to_string() } else { "->".to_string() })
            } else {
                None
            };

            frames.push(Frame {
                file,
                line,
                function,
                class: raw.class_name.clone(),
                frame_type,
                args: raw.args.clone(),
            });
        }
        throwable.trace = frames;
        throwable.is_symbolicated = true;
    }

    /// Initialize a backing [`Throwable`] for a newly-instantiated Exception or Error object.
    fn init_throwable_for_object(&self, obj: &mut VmObject, span: Span) {
        let file = self.resolve_source_file(span.file_id);
        let line = if span.line > 0 { span.line } else { 1 };
        let raw_trace = self.capture_raw_trace();
        let mut throwable = Throwable::new("", 0, file, line);
        throwable.raw_trace = raw_trace;
        obj.set_property("message", PhpValue::string(""));
        obj.set_property("code", PhpValue::Int(0));
        obj.set_property("file", PhpValue::string(&throwable.file));
        obj.set_property("line", PhpValue::Int(throwable.line as i64));
        obj.throwable = Some(Box::new(throwable));
    }

    fn apply_throwable_args_from_slice(
        &self,
        obj: &mut VmObject,
        class_name: &str,
        args: &[PhpValue],
    ) {
        let mut throwable = match obj.throwable.take() {
            Some(t) => t,
            None => return,
        };
        let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
        if is_throwable_subtype(clean, "ErrorException") {
            if let Some(msg_arg) = args.first() {
                throwable.message = msg_arg.to_php_string();
                obj.set_property("message", PhpValue::string(&throwable.message));
            }
            if let Some(code_arg) = args.get(1) {
                throwable.code = code_arg.to_i64().unwrap_or(0);
                obj.set_property("code", PhpValue::Int(throwable.code as i64));
            }
            if let Some(sev_arg) = args.get(2) {
                let sev = sev_arg.to_i64().unwrap_or(1) as i32;
                throwable.severity = Some(sev);
                obj.set_property("severity", PhpValue::Int((sev) as i64));
            }
            if let Some(file_arg) = args.get(3) {
                if !matches!(file_arg, PhpValue::Null) {
                    throwable.file = file_arg.to_php_string();
                    obj.set_property("file", PhpValue::string(&throwable.file));
                }
            }
            if let Some(line_arg) = args.get(4) {
                if !matches!(line_arg, PhpValue::Null) {
                    throwable.line = line_arg.to_i64().unwrap_or(0) as usize;
                    obj.set_property("line", PhpValue::Int(throwable.line as i64));
                }
            }
            if let Some(prev_arg) = args.get(5) {
                if let PhpValue::Object(prev_rc) = prev_arg {
                    if let Some(ref prev_t) = prev_rc.borrow().throwable {
                        throwable.previous = Some(prev_t.clone());
                    }
                    throwable.previous_obj = Some(prev_arg.clone());
                    obj.set_property("previous", prev_arg.clone());
                }
            }
        } else {
            if let Some(msg_arg) = args.first() {
                throwable.message = msg_arg.to_php_string();
                obj.set_property("message", PhpValue::string(&throwable.message));
            }
            if let Some(code_arg) = args.get(1) {
                throwable.code = code_arg.to_i64().unwrap_or(0);
                obj.set_property("code", PhpValue::Int(throwable.code as i64));
            }
            if let Some(prev_arg) = args.get(2) {
                if let PhpValue::Object(prev_rc) = prev_arg {
                    if let Some(ref prev_t) = prev_rc.borrow().throwable {
                        throwable.previous = Some(prev_t.clone());
                    }
                    throwable.previous_obj = Some(prev_arg.clone());
                    obj.set_property("previous", prev_arg.clone());
                }
            }
        }
        obj.throwable = Some(throwable);
    }

    fn apply_throwable_constructor_args(
        &mut self,
        instance: &PhpValue,
        class_name: &str,
        staged: &[StagedArg],
    ) {
        if let PhpValue::Object(o) = instance {
            let flat: Vec<PhpValue> = staged.iter().map(|a| a.value.clone()).collect();
            self.apply_throwable_args_from_slice(&mut o.borrow_mut(), class_name, &flat);
        }
    }

    /// Complete a request lifecycle: instant O(1) arena drop and register file reset.
    pub fn finish_request(&mut self) -> RequestDropStats {
        self.registers.clear();
        self.frames.clear();
        self.arg_buffer.clear();
        self.upvalue_buffer.clear();
        self.defer_queue.clear();
        self.shutdown_hooks.clear();
        self.live_objects.clear();
        self.output_buffer.clear();
        crate::vm::facade::clear_request_context();
        self.arena.drop_request()
    }

    /// Mark this VM as running in persistent parent process boot mode.
    pub fn with_parent_process(mut self, is_parent: bool) -> Self {
        self.is_parent_process = is_parent;
        self
    }

    /// Look up the class name of the current call frame's enclosing method, if any.
    pub fn current_caller_class(&self) -> Option<String> {
        self.frames.last().and_then(|frame| {
            frame.func_id.and_then(|fid| {
                self.module.get_function(fid).and_then(|func| func.enclosing_class.clone())
            })
        })
    }

    /// Look up the late-static-bound called class (`static::class` or `get_called_class()`)
    /// of the current active frame, falling back to enclosing method class.
    pub fn current_called_class(&self) -> Option<String> {
        self.frames.last().and_then(|frame| {
            frame.called_class.clone().or_else(|| {
                frame.func_id.and_then(|fid| {
                    self.module.get_function(fid).and_then(|func| func.enclosing_class.clone())
                })
            })
        })
    }

    /// Configure the Cranelift JIT engine parameters for this VM instance.
    pub fn with_jit_config(mut self, config: JitConfig) -> Self {
        self.jit_enabled = config.enabled;
        self.jit_engine = if config.enabled {
            Some(JitEngine::new(config))
        } else {
            None
        };
        self
    }

    /// Access the Cranelift [`JitEngine`] reference if initialized.
    pub fn jit_engine(&self) -> Option<&JitEngine> {
        self.jit_engine.as_ref()
    }

    /// Access the Cranelift [`JitEngine`] mutable reference if initialized.
    pub fn jit_engine_mut(&mut self) -> Option<&mut JitEngine> {
        self.jit_engine.as_mut()
    }

    /// Attach a [`TypeHierarchy`] from the type checker for full class/interface subtype verification at runtime.
    pub fn with_type_hierarchy(mut self, hierarchy: TypeHierarchy) -> Self {
        self.type_hierarchy = Some(hierarchy);
        self
    }

    /// Set or replace the DI container on this VM instance.
    pub fn with_container(mut self, container: crate::metadata::Container) -> Self {
        self.container = container;
        self
    }


    /// Mutable reference to the underlying compiled [`IrModule`].
    pub fn module_mut(&mut self) -> &mut IrModule {
        &mut self.module
    }

    /// Returns whether JIT compilation is enabled for hot loops in this VM instance.
    #[inline]
    pub fn is_jit_enabled(&self) -> bool {
        self.jit_enabled
    }

    /// Access the captured output emitted by `echo` and `Sys::print` statements.
    #[inline]
    pub fn output(&self) -> &str {
        &self.output_buffer
    }

    /// Register a user-level shutdown callback function (`register_shutdown_function`).
    pub fn register_shutdown_function(&mut self, callback: PhpValue, args: Vec<PhpValue>) {
        self.shutdown_hooks.push(ShutdownHook { callback, args });
    }

    /// Register a user-level error handler callback function (`set_error_handler`).
    pub fn set_error_handler(&mut self, callback: PhpValue, error_levels: Option<i32>) -> PhpValue {
        if callback.is_null() {
            return self.error_handlers.pop().map(|e| e.callback).unwrap_or(PhpValue::Null);
        }
        let prev = self.error_handlers.last().map(|e| e.callback.clone()).unwrap_or(PhpValue::Null);
        let levels = error_levels.unwrap_or(32767); // E_ALL
        self.error_handlers.push(ErrorHandlerEntry {
            callback,
            error_levels: levels,
        });
        prev
    }

    /// Restore the previously defined error handler callback function (`restore_error_handler`).
    pub fn restore_error_handler(&mut self) -> bool {
        self.error_handlers.pop();
        true
    }

    /// Register a user-level exception handler callback function (`set_exception_handler`).
    pub fn set_exception_handler(&mut self, callback: PhpValue) -> PhpValue {
        if callback.is_null() {
            return self.exception_handlers.pop().unwrap_or(PhpValue::Null);
        }
        let prev = self.exception_handlers.last().cloned().unwrap_or(PhpValue::Null);
        self.exception_handlers.push(callback);
        prev
    }

    /// Restore the previously defined exception handler callback function (`restore_exception_handler`).
    pub fn restore_exception_handler(&mut self) -> bool {
        self.exception_handlers.pop();
        true
    }

    /// Trigger a user-level error, warning, or notice (`trigger_error`).
    pub fn trigger_error(
        &mut self,
        message: &str,
        error_level: Option<i32>,
        span: Span,
    ) -> Result<bool, RuntimeError> {
        let level = error_level.unwrap_or(1024); // E_USER_NOTICE
        let file = self.file_path.clone().unwrap_or_else(|| "<anonymous>".to_string());
        let line = span.line;

        if let Some(entry) = self.error_handlers.last().cloned() {
            if (entry.error_levels & level) != 0 {
                let args = vec![
                    PhpValue::Int((level) as i64),
                    PhpValue::string(message),
                    PhpValue::string(&file),
                    PhpValue::Int(line as i64),
                ];
                let res = self.invoke_callable_sync(&entry.callback, args, span)?;
                if !matches!(res, PhpValue::Bool(false)) {
                    return Ok(true);
                }
            }
        }

        if level == 256 {
            // E_USER_ERROR is fatal
            return Err(self.make_error(
                RuntimeErrorKind::Custom {
                    message: format!("Fatal error: {}", message),
                },
                span,
            ));
        }

        let prefix = match level {
            512 => "Warning",
            1024 => "Notice",
            8192 | 16384 => "Deprecated",
            _ => "Error",
        };
        self.output_buffer.push_str(&format!("\n{}: {} in {} on line {}\n", prefix, message, file, line));
        Ok(true)
    }

    /// Track a newly instantiated object for shutdown destructor cleanup.
    pub fn track_live_object(&mut self, val: &PhpValue) {
        if let PhpValue::Object(o) = val {
            self.live_objects.push(Rc::downgrade(o));
        }
    }

    /// Execute all registered shutdown functions (FIFO) and deferred hooks (LIFO).
    pub fn run_shutdown_functions(&mut self) {
        let hooks = std::mem::take(&mut self.shutdown_hooks);
        for hook in hooks {
            let _ = self.invoke_callable_sync(&hook.callback, hook.args, Span::default());
        }

        let deferred = std::mem::take(&mut self.defer_queue);
        for func_id in deferred.into_iter().rev() {
            let _ = self.invoke_compiled_function_sync(func_id, Vec::new(), Span::default());
        }
    }

    /// Execute destructors (`__destruct`) on all live user class objects in LIFO order.
    pub fn run_destructors(&mut self) {
        let mut passes = 0;
        while !self.live_objects.is_empty() && passes < 10 {
            passes += 1;
            let objects = std::mem::take(&mut self.live_objects);
            for weak_ref in objects.into_iter().rev() {
                if let Some(obj_rc) = weak_ref.upgrade() {
                    let (should_run, class_name) = {
                        let mut b = obj_rc.borrow_mut();
                        if b.is_destructed {
                            (false, String::new())
                        } else {
                            b.is_destructed = true;
                            (true, b.class_name.clone())
                        }
                    };

                    if should_run {
                        if let Some(destruct_id) = self.resolve_method_id(&class_name, "__destruct") {
                            let depth_before = self.frames.len();
                            if self
                                .push_compiled_call_frame(
                                    destruct_id,
                                    Some(PhpValue::Object(obj_rc.clone())),
                                    &[],
                                    &[],
                                    None,
                                    Some(class_name),
                                    Span::default(),
                                )
                                .is_ok()
                            {
                                let _ = self.dispatch_loop(depth_before);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Flush all output streams (stdout, stderr).
    pub fn flush_buffers(&mut self) {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
    }

    /// Read the value of a top-level `<main>` variable by name (`"$ans"` or `"ans"`) after execution.
    pub fn get_main_variable(&self, name: &str) -> Option<&PhpValue> {
        let reg = self.module.main_chunk.register_for_var(name)?;
        self.registers.get(reg.index())
    }

    /// Read a global variable by bare name (`"x"` or `"$x"`).
    pub fn get_global(&self, name: &str) -> Option<&PhpValue> {
        let bare = name.strip_prefix('$').unwrap_or(name);
        self.globals.get(bare)
    }

    /// Check whether a class exists (user-defined class, enum, native facade, or built-in standard class).
    pub fn is_class_exists(&self, class_name: &str) -> bool {
        let clean = class_name.strip_prefix('\\').unwrap_or(class_name);

        // 1. User-defined classes in the compiled module (traits excluded)
        if let Some(cls) = self.module.get_class_by_name(clean) {
            if !cls.is_trait {
                return true;
            }
        }
        if self.module.classes.iter().any(|c| !c.is_trait && c.name.eq_ignore_ascii_case(clean)) {
            return true;
        }

        // 2. Compiled enums
        if self.module.enums.contains_key(clean)
            || self.module.enums.values().any(|e| e.name.eq_ignore_ascii_case(clean))
        {
            return true;
        }

        // 3. Static facades (Tier 1, Tier 2, Tier 3)
        if is_static_facade(clean)
            || is_static_only(clean)
            || NATIVE_FACADES
                .iter()
                .any(|f| f.name.eq_ignore_ascii_case(clean))
        {
            return true;
        }

        // 4. Built-in exception and standard classes
        if clean.eq_ignore_ascii_case("stdClass")
            || clean.eq_ignore_ascii_case("Exception")
            || clean.eq_ignore_ascii_case("Error")
            || clean.eq_ignore_ascii_case("Throwable")
            || is_builtin_throwable(clean)
            || clean.eq_ignore_ascii_case("Closure")
            || clean.eq_ignore_ascii_case("Future")
            || clean.ends_with("Exception")
            || clean.ends_with("Error")
            || clean.eq_ignore_ascii_case("Traversable")
            || clean.eq_ignore_ascii_case("IteratorAggregate")
            || clean.eq_ignore_ascii_case("Iterator")
            || clean.eq_ignore_ascii_case("ArrayIterator")
            || clean.eq_ignore_ascii_case("Countable")
            || clean.eq_ignore_ascii_case("ArrayAccess")
            || clean.eq_ignore_ascii_case("LengthAwarePaginator")
            || clean.eq_ignore_ascii_case("Paginator")
            || clean.eq_ignore_ascii_case("Collection")
            || clean.eq_ignore_ascii_case("Illuminate\\Support\\Collection")
        {
            return true;
        }

        false
    }

    /// Check whether a standalone function exists (user-defined function, legacy PHP function, or intrinsic).
    pub fn is_function_exists(&self, fn_name: &str) -> bool {
        let clean = fn_name.strip_prefix('\\').unwrap_or(fn_name);

        // 1. User-defined standalone functions in the compiled module
        if let Some(id) = self.module.function_index.get(clean) {
            if let Some(func) = self.module.functions.get(id.index()) {
                if func.enclosing_class.is_none() && !func.is_closure && !func.name.starts_with("<anon") {
                    return true;
                }
            }
        }
        if self.module.functions.iter().any(|f| {
            f.enclosing_class.is_none()
                && !f.is_closure
                && !f.name.starts_with("<anon")
                && f.name.eq_ignore_ascii_case(clean)
        }) {
            return true;
        }

        // 2. Legacy PHP functions
        if is_legacy_php_function(clean)
            || LEGACY_PHP_FUNCTIONS
                .iter()
                .any(|f| f.php_name.eq_ignore_ascii_case(clean))
        {
            return true;
        }

        // 3. Reserved intrinsics (echo, print, env, dd, dump, etc.)
        if is_reserved_intrinsic(clean)
            || RESERVED_INTRINSICS
                .iter()
                .any(|&i| i.eq_ignore_ascii_case(clean))
            || clean.eq_ignore_ascii_case("ini_get")
            || clean.eq_ignore_ascii_case("ini_set")
        {
            return true;
        }

        // 4. Custom helpers
        if has_custom_helper(clean) {
            return true;
        }

        // 5. Canonical reflection functions
        if clean.eq_ignore_ascii_case("class_exists") || clean.eq_ignore_ascii_case("function_exists") {
            return true;
        }

        false
    }

    /// Reference to the underlying compiled [`IrModule`].
    pub fn module(&self) -> &IrModule {
        &self.module
    }

    /// Reference to the attached nominal [`TypeHierarchy`] if available.
    pub fn type_hierarchy(&self) -> Option<&TypeHierarchy> {
        self.type_hierarchy.as_ref()
    }

    /// Reference to evaluated static class properties (`(Class, Prop) -> Value`).
    pub fn static_properties(&self) -> &HashMap<(String, String), PhpValue> {
        &self.static_properties
    }

    /// Reference to cached class constants (`(Class, Const) -> Value`).
    pub fn class_constants(&self) -> &HashMap<(String, String), PhpValue> {
        &self.class_constants
    }

    /// Reference to top-level globals map.
    pub fn globals(&self) -> &HashMap<String, PhpValue> {
        &self.globals
    }

    /// Set static properties map (e.g. from persistent parent process state).
    pub fn set_static_properties(&mut self, props: HashMap<(String, String), PhpValue>) {
        self.static_properties = props;
    }

    /// Set class constants map (e.g. from persistent parent process state).
    pub fn set_class_constants(&mut self, consts: HashMap<(String, String), PhpValue>) {
        self.class_constants = consts;
    }

    /// Set globals map (e.g. from persistent parent process state).
    pub fn set_globals(&mut self, globals: HashMap<String, PhpValue>) {
        self.globals = globals;
    }

    /// Mutable reference to top-level globals map.
    pub fn globals_mut(&mut self) -> &mut HashMap<String, PhpValue> {
        &mut self.globals
    }

    /// Mutable reference to static class properties map.
    pub fn static_properties_mut(&mut self) -> &mut HashMap<(String, String), PhpValue> {
        &mut self.static_properties
    }

    /// Set or update a single global variable.
    pub fn set_global(&mut self, name: impl Into<String>, value: PhpValue) {
        let key = name.into();
        self.globals.insert(key, value);
    }

    /// Check whether a loop backedge has exceeded hotness thresholds and conditionally
    /// compile and execute the loop body natively via Cranelift.
    fn check_and_run_hot_loop(
        &mut self,
        frame_idx: usize,
        header_ip: u32,
        backedge_ip: u32,
    ) -> bool {
        if self.jit_engine.is_none() {
            return false;
        }
        let func_id = self.frames[frame_idx].func_id;
        let chunk = match func_id {
            Some(fid) => &self.module.functions[fid.index()].chunk,
            None => &self.module.main_chunk,
        };
        let maybe_loop = self
            .jit_engine
            .as_mut()
            .unwrap()
            .record_backedge(func_id, header_ip, backedge_ip, chunk);

        if let Some(compiled_loop) = maybe_loop {
            let base_reg = self.frames[frame_idx].base_reg;
            let num_regs = self.frames[frame_idx].num_registers;
            let reg_slice = &mut self.registers[base_reg..base_reg + num_regs];
            let next_ip = self
                .jit_engine
                .as_mut()
                .unwrap()
                .execute_loop(compiled_loop, reg_slice);
            self.frames[frame_idx].ip = next_ip as usize;

            if next_ip == header_ip {
                self.tick_counter = self.tick_budget;
                self.total_preemptions += 1;
                crate::vm::fiber::trigger_cooperative_yield(crate::vm::fiber::YieldReason::CooperativeYield);
                if self.dev_mode && !self.exempt_loop_limits {
                    let key = (func_id, header_ip);
                    let count = self.loop_iterations.entry(key).or_insert(0);
                    *count += 10_000;
                    if *count > self.dev_loop_warn_threshold && !self.loop_warned.contains(&key) {
                        self.loop_warned.insert(key);
                        let func_desc = self.frames[frame_idx].function_name.clone();
                        let msg = format!(
                            "Loop in {} at ip={} exceeded {} iterations in Dev Mode (possible infinite loop).",
                            func_desc, header_ip, self.dev_loop_warn_threshold
                        );
                        let _ = self.trigger_error(&msg, Some(512), Span::default());
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Check loop back-edge for CPU preemption (instruction budget) and Dev Mode iteration limits.
    fn handle_loop_backedge(
        &mut self,
        frame_idx: usize,
        header_ip: u32,
        _backedge_ip: u32,
        span: Span,
    ) -> Result<(), RuntimeError> {
        // 1. Cooperative Preemption & Instruction Budget (Tick Slicing)
        if self.tick_counter == 0 {
            self.tick_counter = self.tick_budget;
            self.total_preemptions += 1;
            crate::vm::fiber::trigger_cooperative_yield(crate::vm::fiber::YieldReason::CooperativeYield);
        } else {
            self.tick_counter = self.tick_counter.saturating_sub(1);
        }

        // 2. Dev Mode: Warning at 1,000 iterations & Hard Limit at 100,000 iterations
        if self.dev_mode && !self.exempt_loop_limits {
            let func_id = self.frames[frame_idx].func_id;
            let key = (func_id, header_ip);
            let count = self.loop_iterations.entry(key).or_insert(0);
            *count += 1;
            let current_count = *count;

            // Warning at > 1,000 iterations (emitted once per loop execution)
            if current_count > self.dev_loop_warn_threshold && !self.loop_warned.contains(&key) {
                self.loop_warned.insert(key);
                let func_desc = self.frames[frame_idx].function_name.clone();
                let msg = format!(
                    "Loop in {} at ip={} exceeded {} iterations in Dev Mode (possible infinite loop).",
                    func_desc, header_ip, self.dev_loop_warn_threshold
                );
                let _ = self.trigger_error(&msg, Some(512), span);
            }

            // Hard limit at 100,000 iterations (1 lakh)
            if current_count >= self.dev_loop_hard_limit {
                return Err(self.make_error(
                    RuntimeErrorKind::InfiniteLoopLimitExceeded {
                        limit: self.dev_loop_hard_limit,
                        header_ip,
                    },
                    span,
                ));
            }
        }

        Ok(())
    }

    /// Clear iteration accounting for loops that completed or jumped past in Dev Mode.
    fn handle_forward_jump(&mut self, frame_idx: usize, from_ip: u32, to_ip: u32) {
        if self.dev_mode && !self.exempt_loop_limits {
            let func_id = self.frames[frame_idx].func_id;
            self.loop_iterations.retain(|&(fid, header_ip), _| {
                !(fid == func_id && header_ip <= from_ip && to_ip > from_ip)
            });
            self.loop_warned.retain(|&(fid, header_ip)| {
                !(fid == func_id && header_ip <= from_ip && to_ip > from_ip)
            });
        }
    }

    /// Pre-initialize class constants, static property defaults, and enum cases, then execute
    /// `<main>` followed by any registered `defer` hooks.
    pub fn run(&mut self) -> Result<PhpValue, RuntimeError> {
        self.initialize_module_metadata()?;

        let main_num_regs = self.module.main_chunk.num_registers as usize;
        self.registers.clear();
        self.registers.resize(main_num_regs.max(1), PhpValue::Null);
        self.frames.clear();
        self.frames.push(CallFrame {
            function_name: "<main>".to_string(),
            func_id: None,
            ip: 0,
            base_reg: 0,
            num_registers: main_num_regs,
            caller_dst: None,
            called_class: None,
            active_try_handlers: Vec::new(),
            iterators: HashMap::new(),
            ref_bindings: Vec::new(),
        });

        let main_result = self.dispatch_loop(0);

        match main_result {
            Ok(val) => {
                self.run_shutdown_functions();
                self.run_destructors();
                self.flush_buffers();
                Ok(val)
            }
            Err(err) if matches!(err.kind, RuntimeErrorKind::Die(_)) => {
                let exit_code = match err.kind {
                    RuntimeErrorKind::Die(code) => code,
                    _ => unreachable!(),
                };
                if let Some(ref hook) = self.exit_hook {
                    hook(exit_code);
                    return Err(err);
                }
                if let Ok(guard) = crate::vm::facade::EXIT_HOOK.lock() {
                    if let Some(ref hook) = *guard {
                        hook(exit_code);
                        return Err(err);
                    }
                }
                if self.suppress_process_exit {
                    return Err(err);
                }
                std::process::exit(exit_code);
            }
            Err(err) if matches!(err.kind, RuntimeErrorKind::Exit(_)) => {
                let exit_code = match err.kind {
                    RuntimeErrorKind::Exit(code) => code,
                    _ => unreachable!(),
                };
                self.run_shutdown_functions();
                self.run_destructors();
                self.flush_buffers();

                if let Some(ref hook) = self.exit_hook {
                    hook(exit_code);
                    return Err(err);
                }
                if let Ok(guard) = crate::vm::facade::EXIT_HOOK.lock() {
                    if let Some(ref hook) = *guard {
                        hook(exit_code);
                        return Err(err);
                    }
                }
                if self.suppress_process_exit {
                    return Err(err);
                }
                if !self.output_buffer.is_empty() {
                    print!("{}", self.output_buffer);
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
                std::process::exit(exit_code);
            }
            Err(err) => {
                self.run_shutdown_functions();
                self.run_destructors();
                self.flush_buffers();
                Err(err)
            }
        }
    }

    /// Run the bytecode program inside an isolated local execution fiber, tracking
    /// sequential I/O calls (`File::read()`, `DB::table()`, etc.), cooperative yields,
    /// and elapsed CPU/IO durations.
    pub fn run_in_fiber(
        &mut self,
        fiber_name: impl Into<String>,
    ) -> Result<(PhpValue, crate::vm::fiber::FiberStats), RuntimeError> {
        let fiber = crate::vm::fiber::Fiber::new(fiber_name);
        let guard = crate::vm::fiber::enter_fiber(fiber);
        match self.run() {
            Ok(val) => {
                let finished = guard.finish(val.clone());
                Ok((val, finished.stats))
            }
            Err(err) => {
                let _ = guard.fail(err.to_string());
                Err(err)
            }
        }
    }

    /// Run the bytecode program inside a local execution fiber pre-populated with fiber-local context.
    pub fn run_in_fiber_with_context(
        &mut self,
        fiber_name: impl Into<String>,
        context: IndexMap<String, PhpValue>,
    ) -> Result<(PhpValue, crate::vm::fiber::FiberStats), RuntimeError> {
        let mut fiber = crate::vm::fiber::Fiber::new(fiber_name);
        for (k, v) in context {
            fiber.context.set(k, v);
        }
        let guard = crate::vm::fiber::enter_fiber(fiber);
        match self.run() {
            Ok(val) => {
                let finished = guard.finish(val.clone());
                Ok((val, finished.stats))
            }
            Err(err) => {
                let _ = guard.fail(err.to_string());
                Err(err)
            }
        }
    }

    // =========================================================================
    // Module Metadata Initialization (Class Constants, Static Props, Enums)
    // =========================================================================

    fn initialize_module_metadata(&mut self) -> Result<(), RuntimeError> {
        // 1. Evaluate class constants and static property defaults
        let classes = self.module.classes.clone();
        for cls in &classes {
            for c in &cls.constants {
                let val = self.eval_isolated_chunk(&c.value_chunk)?;
                self.class_constants
                    .insert((cls.name.clone(), c.name.clone()), val);
            }
            for prop in &cls.properties {
                if prop.is_static {
                    let val = if let Some(ref chunk) = prop.default_chunk {
                        self.eval_isolated_chunk(chunk)?
                    } else {
                        PhpValue::Null
                    };
                    self.static_properties
                        .insert((cls.name.clone(), prop.name.clone()), val);
                }
            }
        }

        // 2. Evaluate enum cases
        let enums: Vec<_> = self.module.enums.values().cloned().collect();
        for enm in &enums {
            for (idx, case) in enm.cases.iter().enumerate() {
                let key = (enm.name.clone(), case.name.clone());
                if self.class_constants.contains_key(&key) {
                    continue;
                }
                let mut case_obj = VmObject::new(enm.name.clone());
                case_obj.set_property("name", PhpValue::string(case.name.clone()));
                case_obj.readonly_props.insert("name".to_string(), true);
                if enm.backing_type.is_some() || case.value_chunk.is_some() {
                    let backing_val = if let Some(ref chunk) = case.value_chunk {
                        self.eval_isolated_chunk(chunk)?
                    } else {
                        PhpValue::Int(idx as i64)
                    };
                    case_obj.set_property("value", backing_val);
                    case_obj.readonly_props.insert("value".to_string(), true);
                }
                let obj_val = PhpValue::object(case_obj);
                self.class_constants.insert(key, obj_val.clone());
                if let Some(short) = enm.name.split('\\').last() {
                    if short != enm.name {
                        self.class_constants
                            .insert((short.to_string(), case.name.clone()), obj_val.clone());
                    }
                }
            }
        }

        Ok(())
    }

    /// Evaluate a small self-contained initializer [`BytecodeChunk`] (for property/constant defaults).
    fn eval_isolated_chunk(&mut self, chunk: &BytecodeChunk) -> Result<PhpValue, RuntimeError> {
        if chunk.is_empty() {
            return Ok(PhpValue::Null);
        }
        let mut regs = vec![PhpValue::Null; (chunk.num_registers as usize).max(1)];
        let mut ip = 0usize;
        while ip < chunk.instructions.len() {
            let op = chunk.instructions[ip];
            let span = chunk.span_at(ip).unwrap_or_default();
            ip += 1;
            match op {
                OpCode::LoadNull { dst } => regs[dst.index()] = PhpValue::Null,
                OpCode::LoadBool { dst, value } => regs[dst.index()] = PhpValue::Bool(value),
                OpCode::LoadInt { dst, value } => regs[dst.index()] = PhpValue::Int(value as i64),
                OpCode::LoadFloat { dst, bits } => regs[dst.index()] = PhpValue::float(f64::from_bits(bits)),
                OpCode::LoadConst { dst, const_id } => {
                    let c = chunk.get_constant(const_id).ok_or_else(|| {
                        RuntimeError::new(
                            RuntimeErrorKind::InvalidConstantIndex { index: const_id.0 },
                            span,
                        )
                    })?;
                    regs[dst.index()] = Self::constant_to_value(c);
                }
                OpCode::Move { dst, src } => {
                    regs[dst.index()] = regs[src.index()].clone();
                }
                OpCode::Unary { dst, op: uop, src } => {
                    let val = self.exec_unary(uop, &regs[src.index()], span)?;
                    regs[dst.index()] = val;
                }
                OpCode::Binary { dst, op: bop, lhs, rhs } => {
                    let val = self.exec_binary(bop, &regs[lhs.index()], &regs[rhs.index()], span)?;
                    regs[dst.index()] = val;
                }
                OpCode::NewArray { dst, capacity } => {
                    regs[dst.index()] = if capacity == 0 {
                        PhpValue::EmptyArray
                    } else {
                        PhpValue::Array(Rc::new(RefCell::new(Vec::with_capacity(capacity as usize))))
                    };
                }
                OpCode::ArrayPush { array, value } => {
                    let v = regs[value.index()].clone();
                    let target = &mut regs[array.index()];
                    if matches!(target, PhpValue::EmptyArray) {
                        *target = PhpValue::promote_empty_append(v);
                    } else {
                        target.ensure_array_mut();
                        match target {
                            PhpValue::Array(arr) | PhpValue::List(arr) => {
                                arr.borrow_mut().push(v);
                            }
                            _ => {}
                        }
                    }
                }
                OpCode::Return { value } => {
                    return Ok(value.map(|r| regs[r.index()].clone()).unwrap_or(PhpValue::Null));
                }
                _ => {}
            }
        }
        Ok(PhpValue::Null)
    }

    // =========================================================================
    // Core Register VM Dispatch Loop
    // =========================================================================

    /// Run the register bytecode dispatch loop until the frame at `stop_depth` returns.
    fn dispatch_loop(&mut self, stop_depth: usize) -> Result<PhpValue, RuntimeError> {
        loop {
            let frame_idx = self.frames.len() - 1;
            let ip = self.frames[frame_idx].ip;
            let func_id = self.frames[frame_idx].func_id;
            let base_reg = self.frames[frame_idx].base_reg;

            // Check instruction budget and cooperative yield at function prologue
            if ip == 0 {
                if self.tick_counter == 0 {
                    self.tick_counter = self.tick_budget;
                    self.total_preemptions += 1;
                    crate::vm::fiber::trigger_cooperative_yield(crate::vm::fiber::YieldReason::CooperativeYield);
                } else {
                    self.tick_counter = self.tick_counter.saturating_sub(1);
                }
            }

            // Check for hot function compilation and execution if entering a function frame
            if ip == 0 && func_id.is_some() && self.jit_engine.is_some() {
                if let Some(fid) = func_id {
                    let chunk = &self.module.functions[fid.index()].chunk;
                    let maybe_fn = self
                        .jit_engine
                        .as_mut()
                        .unwrap()
                        .record_function_call(fid, chunk);
                    if let Some(compiled_fn) = maybe_fn {
                        let num_regs = self.frames[frame_idx].num_registers;
                        let reg_slice = &mut self.registers[base_reg..base_reg + num_regs];
                        if let Some(ret_val) = self
                            .jit_engine
                            .as_mut()
                            .unwrap()
                            .execute_function(compiled_fn, reg_slice)
                        {
                            let popped = self.frames.pop().unwrap();
                            if popped.base_reg > 0 {
                                self.registers.truncate(popped.base_reg);
                            }
                            if self.frames.len() <= stop_depth {
                                return Ok(ret_val);
                            }
                            if let Some(caller_dst) = popped.caller_dst {
                                self.registers[caller_dst] = ret_val;
                            }
                            continue;
                        }
                    }
                }
            }

            let (op, span) = {
                let chunk = self.chunk_for_func(func_id);
                if ip >= chunk.instructions.len() {
                    (OpCode::Return { value: None }, Span::default())
                } else {
                    (
                        chunk.instructions[ip],
                        chunk.span_at(ip).unwrap_or_default(),
                    )
                }
            };

            self.frames[frame_idx].ip = ip + 1;

            match op {
                // -----------------------------------------------------------------
                // 1. Constants & Register Data Movement
                // -----------------------------------------------------------------
                OpCode::LoadNull { dst } => {
                    self.registers[base_reg + dst.index()] = PhpValue::Null;
                }

                OpCode::LoadBool { dst, value } => {
                    self.registers[base_reg + dst.index()] = PhpValue::Bool(value);
                }

                OpCode::LoadInt { dst, value } => {
                    self.registers[base_reg + dst.index()] = PhpValue::Int(value as i64);
                }

                OpCode::LoadFloat { dst, bits } => {
                    self.registers[base_reg + dst.index()] = PhpValue::float(f64::from_bits(bits));
                }

                OpCode::LoadConst { dst, const_id } => {
                    let val = {
                        let chunk = self.chunk_for_func(func_id);
                        let c = chunk.get_constant(const_id).ok_or_else(|| {
                            self.make_error(
                                RuntimeErrorKind::InvalidConstantIndex { index: const_id.0 },
                                span,
                            )
                        })?;
                        Self::constant_to_value(c)
                    };
                    self.registers[base_reg + dst.index()] = val;
                }

                OpCode::Move { dst, src } => {
                    if dst != src {
                        let val = self.registers[base_reg + src.index()].clone();
                        self.registers[base_reg + dst.index()] = val;
                    }
                }

                OpCode::Clone { dst, src } => {
                    let cloned = self.registers[base_reg + src.index()].deep_clone();
                    self.registers[base_reg + dst.index()] = cloned;
                }

                // -----------------------------------------------------------------
                // 2. Arithmetic, Bitwise, Comparison, Cast, & Type Contracts
                // -----------------------------------------------------------------
                OpCode::Unary { dst, op: uop, src } => {
                    let operand = self.registers[base_reg + src.index()].clone();
                    let res = self.exec_unary(uop, &operand, span)?;
                    self.registers[base_reg + dst.index()] = res;
                }

                OpCode::Binary {
                    dst,
                    op: bop,
                    lhs,
                    rhs,
                } => {
                    let left = self.registers[base_reg + lhs.index()].clone();
                    let right = self.registers[base_reg + rhs.index()].clone();
                    let res = self.exec_binary(bop, &left, &right, span)?;
                    self.registers[base_reg + dst.index()] = res;
                }

                OpCode::IncDec { dst, src, is_inc } => {
                    let delta = if is_inc { 1 } else { -1 };
                    let current = &self.registers[base_reg + src.index()];
                    let updated = self.exec_inc_dec(current, delta, span)?;
                    self.registers[base_reg + dst.index()] = updated;
                }

                OpCode::Cast { dst, kind, src } => {
                    let val = self.registers[base_reg + src.index()].clone();
                    let casted = match kind {
                        CastType::Int => PhpValue::Int(val.to_i64().unwrap_or(0)),
                        CastType::Float => PhpValue::float(val.to_f64().unwrap_or(0.0)),
                        CastType::String => PhpValue::string(val.to_php_string()),
                        CastType::Bool => PhpValue::Bool(val.is_truthy()),
                        CastType::Array => match val {
                            PhpValue::EmptyArray | PhpValue::Array(_) | PhpValue::List(_) => val,
                            PhpValue::Null => PhpValue::list(Vec::new()),
                            other => PhpValue::list(vec![other]),
                        },
                        CastType::Object => match val {
                            PhpValue::Object(_) => val,
                            other => {
                                let mut obj = VmObject::new("stdClass");
                                obj.set_property("scalar", other);
                                PhpValue::object(obj)
                            }
                        },
                    };
                    self.registers[base_reg + dst.index()] = casted;
                }

                OpCode::AssertType { reg, type_const } => {
                    let expected_ty = {
                        let chunk = self.chunk_for_func(func_id);
                        match chunk.get_constant(type_const) {
                            Some(Constant::Type(ty)) => ty.clone(),
                            _ => {
                                return Err(self.make_error(
                                    RuntimeErrorKind::InvalidConstantIndex {
                                        index: type_const.0,
                                    },
                                    span,
                                ))
                            }
                        }
                    };

                    let actual_val = &self.registers[base_reg + reg.index()];
                    let is_match = match (actual_val, &expected_ty) {
                        (PhpValue::Object(obj), RhpType::Class(target_cls)) => {
                            self.is_class_instance_of(&obj.borrow().class_name, target_cls)
                        }
                        _ => actual_val.matches_type_contract(&expected_ty, self.type_hierarchy.as_ref()),
                    };
                    if !is_match {
                        return Err(self.make_error(
                            RuntimeErrorKind::TypeContractViolation {
                                expected: expected_ty,
                                actual: self.runtime_type_label(actual_val),
                                value_preview: format!("{:?}", actual_val),
                            },
                            span,
                        ));
                    }

                    // Coerce `Int` into `Float` if the boundary explicitly expects `float`
                    if matches!(expected_ty, RhpType::Float) {
                        if actual_val.is_integer_value() {
                            if let Some(decimal) = crate::vm::numeric::php_to_decimal(actual_val) {
                                if let Ok(narrowed) = crate::vm::numeric::narrow_decimal(decimal) {
                                    self.registers[base_reg + reg.index()] = narrowed;
                                }
                            }
                        }
                    } else if let Some(width) = match &expected_ty {
                        RhpType::Int => Some("i64"),
                        _ => None,
                    } {
                        if let Some(stored) = crate::vm::numeric::fits_stored_width(actual_val, width) {
                            self.registers[base_reg + reg.index()] = stored;
                        }
                    }
                }

                // -----------------------------------------------------------------
                // 3. Control Flow & Linear Branching
                // -----------------------------------------------------------------
                OpCode::Jump { target } => {
                    if target <= (ip as u32) {
                        self.handle_loop_backedge(frame_idx, target, ip as u32, span)?;
                        if !self.check_and_run_hot_loop(frame_idx, target, ip as u32) {
                            self.frames[frame_idx].ip = target as usize;
                        }
                    } else {
                        self.handle_forward_jump(frame_idx, ip as u32, target);
                        self.frames[frame_idx].ip = target as usize;
                    }
                }

                OpCode::JumpIfTrue { cond, target } => {
                    if self.registers[base_reg + cond.index()].is_truthy() {
                        if target <= (ip as u32) {
                            self.handle_loop_backedge(frame_idx, target, ip as u32, span)?;
                            if !self.check_and_run_hot_loop(frame_idx, target, ip as u32) {
                                self.frames[frame_idx].ip = target as usize;
                            }
                        } else {
                            self.handle_forward_jump(frame_idx, ip as u32, target);
                            self.frames[frame_idx].ip = target as usize;
                        }
                    }
                }

                OpCode::JumpIfFalse { cond, target } => {
                    if !self.registers[base_reg + cond.index()].is_truthy() {
                        if target <= (ip as u32) {
                            self.handle_loop_backedge(frame_idx, target, ip as u32, span)?;
                            if !self.check_and_run_hot_loop(frame_idx, target, ip as u32) {
                                self.frames[frame_idx].ip = target as usize;
                            }
                        } else {
                            self.handle_forward_jump(frame_idx, ip as u32, target);
                            self.frames[frame_idx].ip = target as usize;
                        }
                    }
                }

                OpCode::JumpIfNull { cond, target } => {
                    if self.registers[base_reg + cond.index()].is_null() {
                        if target <= (ip as u32) {
                            self.handle_loop_backedge(frame_idx, target, ip as u32, span)?;
                            if !self.check_and_run_hot_loop(frame_idx, target, ip as u32) {
                                self.frames[frame_idx].ip = target as usize;
                            }
                        } else {
                            self.handle_forward_jump(frame_idx, ip as u32, target);
                            self.frames[frame_idx].ip = target as usize;
                        }
                    }
                }

                OpCode::JumpIfNotNull { cond, target } => {
                    if !self.registers[base_reg + cond.index()].is_null() {
                        if target <= (ip as u32) {
                            self.handle_loop_backedge(frame_idx, target, ip as u32, span)?;
                            if !self.check_and_run_hot_loop(frame_idx, target, ip as u32) {
                                self.frames[frame_idx].ip = target as usize;
                            }
                        } else {
                            self.handle_forward_jump(frame_idx, ip as u32, target);
                            self.frames[frame_idx].ip = target as usize;
                        }
                    }
                }

                OpCode::Return { value } => {
                    let ret_val = match value {
                        Some(r) => self.registers[base_reg + r.index()].clone(),
                        None => PhpValue::Null,
                    };

                    // Copy-out: Write back pass-by-reference parameter values to caller's registers
                    for &(callee_reg, caller_reg) in &self.frames[frame_idx].ref_bindings {
                        let final_val = self.registers[callee_reg].clone();
                        self.registers[caller_reg] = final_val;
                    }

                    let popped = self.frames.pop().unwrap();
                    if popped.base_reg > 0 {
                        self.registers.truncate(popped.base_reg);
                    }

                    if popped.function_name.ends_with("::__forward") {
                        if let Some(ref origin) = popped.called_class {
                            if let PhpValue::Object(ref rc) = ret_val {
                                rc.borrow_mut().forwarded_from = Some(origin.clone());
                            }
                        }
                    }

                    if self.frames.len() <= stop_depth {
                        return Ok(ret_val);
                    }

                    if let Some(caller_dst) = popped.caller_dst {
                        self.registers[caller_dst] = ret_val;
                    }
                }

                // -----------------------------------------------------------------
                // 4. Standard Arrays & Subtyped Collections
                // -----------------------------------------------------------------
                OpCode::NewArray { dst, capacity } => {
                    self.registers[base_reg + dst.index()] = if capacity == 0 {
                        PhpValue::EmptyArray
                    } else {
                        PhpValue::Array(Rc::new(RefCell::new(Vec::with_capacity(
                            capacity as usize,
                        ))))
                    };
                }

                OpCode::NewCollection { dst, kind, source } => {
                    let src_val = source.map(|s| self.registers[base_reg + s.index()].clone());
                    let col = match kind {
                        IrCollectionKind::List => {
                            let items = src_val
                                .as_ref()
                                .map(Self::extract_items)
                                .unwrap_or_default();
                            if let Some(first) = items.first() {
                                for item in &items[1..] {
                                    if !first.same_type_as(item) {
                                        return Err(self.make_error(
                                            RuntimeErrorKind::TypeError {
                                                message: format!(
                                                    "All elements in a list must have the same type, found {} and {}",
                                                    first.type_name(),
                                                    item.type_name()
                                                ),
                                            },
                                            span,
                                        ));
                                    }
                                }
                            }
                            PhpValue::list(items)
                        }
                        IrCollectionKind::Dict => {
                            let entries = src_val
                                .as_ref()
                                .map(Self::extract_dict_entries)
                                .unwrap_or_default();
                            PhpValue::dict_from_map(entries)
                        }
                        IrCollectionKind::Set => {
                            let items = src_val
                                .as_ref()
                                .map(Self::extract_items)
                                .unwrap_or_default();
                            if let Some(first) = items.first() {
                                for item in &items[1..] {
                                    if !first.same_type_as(item) {
                                        return Err(self.make_error(
                                            RuntimeErrorKind::TypeError {
                                                message: format!(
                                                    "All elements in a set must have the same type, found {} and {}",
                                                    first.type_name(),
                                                    item.type_name()
                                                ),
                                            },
                                            span,
                                        ));
                                    }
                                }
                            }
                            PhpValue::set(items)
                        }
                        IrCollectionKind::HashMap => {
                            let entries = src_val
                                .as_ref()
                                .map(Self::extract_map_entries)
                                .unwrap_or_default();
                            PhpValue::hash_map(entries)
                        }
                    };
                    self.registers[base_reg + dst.index()] = col;
                }

                OpCode::ArrayPush { array, value } => {
                    let val = self.registers[base_reg + value.index()].clone();
                    let target_idx = base_reg + array.index();
                    match &self.registers[target_idx] {
                        PhpValue::List(arr) => {
                            if let Some(first) = arr.borrow().first() {
                                if !first.same_type_as(&val) {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::TypeError {
                                            message: format!(
                                                "All elements in a list must have the same type, found {} and {}",
                                                first.type_name(),
                                                val.type_name()
                                            ),
                                        },
                                        span,
                                    ));
                                }
                            }
                            self.registers[target_idx].ensure_list_mut();
                            if let PhpValue::List(arr) = &self.registers[target_idx] {
                                arr.borrow_mut().push(val);
                            }
                        }
                        PhpValue::EmptyArray => {
                            self.registers[target_idx] = PhpValue::promote_empty_append(val);
                        }
                        PhpValue::Array(_) => {
                            self.registers[target_idx].ensure_array_mut();
                            if let PhpValue::Array(arr) = &self.registers[target_idx] {
                                arr.borrow_mut().push(val);
                            }
                        }
                        PhpValue::Set(set) => {
                            if let Some(first) = set.borrow().iter().next() {
                                if !first.same_type_as(&val) {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::TypeError {
                                            message: format!(
                                                "All elements in a set must have the same type, found {} and {}",
                                                first.type_name(),
                                                val.type_name()
                                            ),
                                        },
                                        span,
                                    ));
                                }
                            }
                            self.registers[target_idx].ensure_set_mut();
                            if let PhpValue::Set(set) = &self.registers[target_idx] {
                                set.borrow_mut().insert(val);
                            }
                        }
                        PhpValue::Dict(_) => {
                            self.registers[target_idx].ensure_dict_mut();
                            if let PhpValue::Dict(dict) = &self.registers[target_idx] {
                                let mut b = dict.borrow_mut();
                                let next_key = b.len().to_string();
                                b.insert(next_key, val);
                            }
                        }
                        PhpValue::HashMap(_) => {
                            self.registers[target_idx].ensure_hash_map_mut();
                            if let PhpValue::HashMap(map) = &self.registers[target_idx] {
                                let next_key = PhpValue::Int(map.borrow().len() as i64);
                                map.borrow_mut().insert(next_key, val);
                            }
                        }
                        other => {
                            let type_name = other.type_name().to_string();
                            return Err(self.make_error(
                                RuntimeErrorKind::InvalidIndexTarget { type_name },
                                span,
                            ));
                        }
                    }
                }

                OpCode::ArraySet { array, key, value } => {
                    let k = self.registers[base_reg + key.index()].clone();
                    let v = self.registers[base_reg + value.index()].clone();
                    let target_idx = base_reg + array.index();

                    // Check if setting a string key on a plain `Array` should upgrade it to `Dict`
                    let upgrade_to_dict = match (&self.registers[target_idx], &k) {
                        (PhpValue::Array(arr), PhpValue::String(_)) => {
                            let mut map = IndexMap::new();
                            for (i, item) in arr.borrow().iter().enumerate() {
                                map.insert(i.to_string(), item.clone());
                            }
                            Some(map)
                        }
                        (PhpValue::Null, _) => {
                            Some(IndexMap::new())
                        }
                        _ => None,
                    };

                    if let Some(mut map) = upgrade_to_dict {
                        let k_str = match &k {
                            PhpValue::String(s) => s.to_string(),
                            other => {
                                return Err(self.make_error(
                                    RuntimeErrorKind::TypeError {
                                        message: format!(
                                            "Dict key must be a string, got {}",
                                            other.type_name()
                                        ),
                                    },
                                    span,
                                ));
                            }
                        };
                        map.insert(k_str, v);
                        self.registers[target_idx] = PhpValue::dict_from_map(map);
                    } else {
                        match &self.registers[target_idx] {
                            PhpValue::List(arr) => {
                                let idx = match &k {
                                    PhpValue::Int(i) if *i >= 0 => *i,
                                    PhpValue::Int(i) => {
                                        return Err(self.make_error(
                                            RuntimeErrorKind::TypeError {
                                                message: format!("List index must be a non-negative integer, got negative integer: {}", i),
                                            },
                                            span,
                                        ));
                                    }
                                    other => {
                                        return Err(self.make_error(
                                            RuntimeErrorKind::TypeError {
                                                message: format!(
                                                    "List index must be a non-negative integer, got {}",
                                                    other.type_name()
                                                ),
                                            },
                                            span,
                                        ));
                                    }
                                };
                                if let Some(first) = arr.borrow().first() {
                                    if !first.same_type_as(&v) {
                                        return Err(self.make_error(
                                            RuntimeErrorKind::TypeError {
                                                message: format!(
                                                    "All elements in a list must have the same type, found {} and {}",
                                                    first.type_name(),
                                                    v.type_name()
                                                ),
                                            },
                                            span,
                                        ));
                                    }
                                }
                                self.registers[target_idx].ensure_list_mut();
                                if let PhpValue::List(arr) = &self.registers[target_idx] {
                                    let mut b = arr.borrow_mut();
                                    if (idx as usize) < b.len() {
                                        b[idx as usize] = v;
                                    } else if (idx as usize) == b.len() {
                                        b.push(v);
                                    } else {
                                        b.resize(idx as usize, PhpValue::Null);
                                        b.push(v);
                                    }
                                }
                            }
                            PhpValue::EmptyArray => {
                                self.registers[target_idx] = PhpValue::promote_empty_index(&k, v);
                            }
                            PhpValue::Array(_) => {
                                self.registers[target_idx].ensure_array_mut();
                                if let PhpValue::Array(arr) = &self.registers[target_idx] {
                                    let idx = k.to_i64().unwrap_or(0);
                                    let mut b = arr.borrow_mut();
                                    if idx >= 0 && (idx as usize) < b.len() {
                                        b[idx as usize] = v;
                                    } else if idx >= 0 && (idx as usize) == b.len() {
                                        b.push(v);
                                    } else if idx >= 0 {
                                        b.resize(idx as usize, PhpValue::Null);
                                        b.push(v);
                                    }
                                }
                            }
                            PhpValue::Dict(_) => {
                                let k_str = match &k {
                                    PhpValue::String(s) => s.to_string(),
                                    other => {
                                        return Err(self.make_error(
                                            RuntimeErrorKind::TypeError {
                                                message: format!(
                                                    "Dict key must be a string, got {}",
                                                    other.type_name()
                                                ),
                                            },
                                            span,
                                        ));
                                    }
                                };
                                self.registers[target_idx].ensure_dict_mut();
                                if let PhpValue::Dict(dict) = &self.registers[target_idx] {
                                    dict.borrow_mut().insert(k_str, v);
                                }
                            }
                            PhpValue::HashMap(_) => {
                                self.registers[target_idx].ensure_hash_map_mut();
                                if let PhpValue::HashMap(map) = &self.registers[target_idx] {
                                    map.borrow_mut().insert(k, v);
                                }
                            }
                            PhpValue::Set(set) => {
                                if v.is_truthy() {
                                    if let Some(first) = set.borrow().iter().next() {
                                        if !first.same_type_as(&k) {
                                            return Err(self.make_error(
                                                RuntimeErrorKind::TypeError {
                                                    message: format!(
                                                        "All elements in a set must have the same type, found {} and {}",
                                                        first.type_name(),
                                                        k.type_name()
                                                    ),
                                                },
                                                span,
                                            ));
                                        }
                                    }
                                    self.registers[target_idx].ensure_set_mut();
                                    if let PhpValue::Set(set) = &self.registers[target_idx] {
                                        set.borrow_mut().insert(k);
                                    }
                                } else {
                                    self.registers[target_idx].ensure_set_mut();
                                    if let PhpValue::Set(set) = &self.registers[target_idx] {
                                        set.borrow_mut().remove(&k);
                                    }
                                }
                            }
                            PhpValue::String(_) => {
                                let idx = k.to_i64().unwrap_or(-1);
                                if idx >= 0 {
                                    let repl_char = v.to_php_string().chars().next().unwrap_or('\0');
                                    if let PhpValue::String(s) = &mut self.registers[target_idx] {
                                        s.set_char(idx as usize, repl_char);
                                    }
                                }
                            }
                            other => {
                                let type_name = other.type_name().to_string();
                                return Err(self.make_error(
                                    RuntimeErrorKind::InvalidIndexTarget { type_name },
                                    span,
                                ));
                            }
                        }
                    }
                }

                OpCode::ArrayUnpack {
                    dst_array,
                    src_array,
                } => {
                    let src_val = self.registers[base_reg + src_array.index()].clone();
                    let items = Self::extract_items(&src_val);
                    let target_idx = base_reg + dst_array.index();
                    match &self.registers[target_idx] {
                        PhpValue::List(arr) => {
                            if let Some(first) = arr.borrow().first() {
                                for item in &items {
                                    if !first.same_type_as(item) {
                                        return Err(self.make_error(
                                            RuntimeErrorKind::TypeError {
                                                message: format!(
                                                    "All elements in a list must have the same type, found {} and {}",
                                                    first.type_name(),
                                                    item.type_name()
                                                ),
                                            },
                                            span,
                                        ));
                                    }
                                }
                            }
                            self.registers[target_idx].ensure_list_mut();
                            if let PhpValue::List(arr) = &self.registers[target_idx] {
                                arr.borrow_mut().extend(items);
                            }
                        }
                        PhpValue::EmptyArray => {
                            if !items.is_empty() {
                                self.registers[target_idx] = PhpValue::list(items);
                            }
                        }
                        PhpValue::Array(_) => {
                            self.registers[target_idx].ensure_array_mut();
                            if let PhpValue::Array(arr) = &self.registers[target_idx] {
                                arr.borrow_mut().extend(items);
                            }
                        }
                        PhpValue::Set(set) => {
                            if let Some(first) = set.borrow().iter().next() {
                                for item in &items {
                                    if !first.same_type_as(item) {
                                        return Err(self.make_error(
                                            RuntimeErrorKind::TypeError {
                                                message: format!(
                                                    "All elements in a set must have the same type, found {} and {}",
                                                    first.type_name(),
                                                    item.type_name()
                                                ),
                                            },
                                            span,
                                        ));
                                    }
                                }
                            }
                            self.registers[target_idx].ensure_set_mut();
                            if let PhpValue::Set(set) = &self.registers[target_idx] {
                                let mut b = set.borrow_mut();
                                for item in items {
                                    b.insert(item);
                                }
                            }
                        }
                        _ => {}
                    }
                }

                OpCode::ArrayGet { dst, array, index } => {
                    let arr_val = &self.registers[base_reg + array.index()];
                    let idx_val = &self.registers[base_reg + index.index()];
                    let fetched = match arr_val {
                        PhpValue::List(arr) => {
                            let i = match idx_val {
                                PhpValue::Int(n) if *n >= 0 => *n,
                                PhpValue::Int(n) => {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::TypeError {
                                            message: format!("List index must be a non-negative integer, got negative integer: {}", n),
                                        },
                                        span,
                                    ));
                                }
                                other => {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::TypeError {
                                            message: format!(
                                                "List index must be a non-negative integer, got {}",
                                                other.type_name()
                                            ),
                                        },
                                        span,
                                    ));
                                }
                            };
                            let b = arr.borrow();
                            if (i as usize) < b.len() {
                                b[i as usize].clone()
                            } else {
                                PhpValue::Null
                            }
                        }
                        PhpValue::EmptyArray => PhpValue::Null,
                        PhpValue::Array(arr) => {
                            let i = idx_val.to_i64().unwrap_or(-1);
                            let b = arr.borrow();
                            if i >= 0 && (i as usize) < b.len() {
                                b[i as usize].clone()
                            } else {
                                PhpValue::Null
                            }
                        }
                        PhpValue::Set(set) => {
                            PhpValue::Bool(set.borrow().contains(idx_val))
                        }
                        PhpValue::Dict(dict) => {
                            let key_str = match idx_val {
                                PhpValue::String(s) => s.to_string(),
                                other => {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::TypeError {
                                            message: format!(
                                                "Dict key must be a string, got {}",
                                                other.type_name()
                                            ),
                                        },
                                        span,
                                    ));
                                }
                            };
                            if let Some(v) = dict.borrow().get(&key_str) {
                                v.clone()
                            } else {
                                return Err(self.make_error(
                                    RuntimeErrorKind::UndefinedIndex { key: key_str },
                                    span,
                                ));
                            }
                        }
                        PhpValue::HashMap(map) => map
                            .borrow()
                            .get(idx_val)
                            .cloned()
                            .unwrap_or(PhpValue::Null),
                        PhpValue::String(s) => {
                            let i = idx_val.to_i64().unwrap_or(-1);
                            if i >= 0 {
                                s.chars()
                                    .nth(i as usize)
                                    .map(|c| PhpValue::string(c.to_string()))
                                    .unwrap_or(PhpValue::Null)
                            } else {
                                PhpValue::Null
                            }
                        }
                        other => {
                            return Err(self.make_error(
                                RuntimeErrorKind::InvalidIndexTarget {
                                    type_name: other.type_name().to_string(),
                                },
                                span,
                            ));
                        }
                    };
                    self.registers[base_reg + dst.index()] = fetched;
                }

                // -----------------------------------------------------------------
                // 5. Iteration (`foreach`)
                // -----------------------------------------------------------------
                OpCode::IterInit { iter_reg, iterable } => {
                    let iter_source = self.registers[base_reg + iterable.index()].clone();
                    let active_iter = self.build_active_iterator(&iter_source, span)?;

                    self.frames[frame_idx].iterators.insert(
                        iter_reg.index(),
                        active_iter,
                    );
                }

                OpCode::IterNext {
                    iter_reg,
                    key_dst,
                    val_dst,
                    jump_end,
                } => {
                    let mut iter_state = match self.frames[frame_idx].iterators.remove(&iter_reg.index()) {
                        Some(s) => s,
                        None => {
                            self.frames[frame_idx].ip = jump_end as usize;
                            continue;
                        }
                    };

                    enum IterYield {
                        Next { key: Option<PhpValue>, val: PhpValue },
                        Done,
                    }

                    let yield_result: Result<IterYield, RuntimeError> = match &mut iter_state {
                        ActiveIterator::List { items, cursor } => {
                            let b = items.borrow();
                            if *cursor < b.len() {
                                let val = b[*cursor].clone();
                                let curr = *cursor;
                                *cursor += 1;
                                Ok(IterYield::Next {
                                    key: Some(PhpValue::Int(curr as i64)),
                                    val,
                                })
                            } else {
                                Ok(IterYield::Done)
                            }
                        }
                        ActiveIterator::Dict { dict, cursor } => {
                            let b = dict.borrow();
                            if *cursor < b.len() {
                                if let Some((k, v)) = b.get_index(*cursor) {
                                    let key_val = PhpValue::string(k.clone());
                                    let val_val = v.clone();
                                    *cursor += 1;
                                    Ok(IterYield::Next {
                                        key: Some(key_val),
                                        val: val_val,
                                    })
                                } else {
                                    Ok(IterYield::Done)
                                }
                            } else {
                                Ok(IterYield::Done)
                            }
                        }
                        ActiveIterator::Set { items, cursor } => {
                            if *cursor < items.len() {
                                let val = items[*cursor].clone();
                                let curr = *cursor;
                                *cursor += 1;
                                Ok(IterYield::Next {
                                    key: Some(PhpValue::Int(curr as i64)),
                                    val,
                                })
                            } else {
                                Ok(IterYield::Done)
                            }
                        }
                        ActiveIterator::Entries { entries, cursor } => {
                            if *cursor < entries.len() {
                                let (k, v) = entries[*cursor].clone();
                                *cursor += 1;
                                Ok(IterYield::Next {
                                    key: Some(k),
                                    val: v,
                                })
                            } else {
                                Ok(IterYield::Done)
                            }
                        }
                        ActiveIterator::IteratorObject { iterator, started } => {
                            let iter_obj = iterator.clone();
                            if !*started {
                                self.invoke_method_sync(iter_obj.clone(), "rewind", vec![], span)?;
                                *started = true;
                            } else {
                                self.invoke_method_sync(iter_obj.clone(), "next", vec![], span)?;
                            }

                            let is_valid = self
                                .invoke_method_sync(iter_obj.clone(), "valid", vec![], span)?
                                .to_bool()
                                .unwrap_or(false);
                            if is_valid {
                                let val = self.invoke_method_sync(iter_obj.clone(), "current", vec![], span)?;
                                let key = if key_dst.is_some() {
                                    Some(self.invoke_method_sync(iter_obj.clone(), "key", vec![], span)?)
                                } else {
                                    None
                                };
                                Ok(IterYield::Next { key, val })
                            } else {
                                Ok(IterYield::Done)
                            }
                        }
                    };

                    match yield_result? {
                        IterYield::Next { key, val } => {
                            self.frames[frame_idx].iterators.insert(iter_reg.index(), iter_state);
                            if let Some(k_reg) = key_dst {
                                if let Some(k) = key {
                                    self.registers[base_reg + k_reg.index()] = k;
                                }
                            }
                            self.registers[base_reg + val_dst.index()] = val;
                        }
                        IterYield::Done => {
                            self.frames[frame_idx].ip = jump_end as usize;
                        }
                    }
                }

                // -----------------------------------------------------------------
                // 6. Function, Facade, Method, & Closure Invocations
                // -----------------------------------------------------------------
                OpCode::PushArg {
                    src,
                    name,
                    is_unpack,
                    is_reference,
                    is_skipped,
                } => {
                    let arg_name = if let Some(cid) = name {
                        let chunk = self.chunk_for_func(func_id);
                        chunk.get_string_constant(cid).map(|s| s.to_string())
                    } else {
                        None
                    };
                    let val = self.registers[base_reg + src.index()].clone();
                    let caller_reg = if is_reference {
                        Some(base_reg + src.index())
                    } else {
                        None
                    };
                    self.arg_buffer.push(StagedArg {
                        value: val,
                        name: arg_name,
                        is_unpack,
                        is_reference,
                        is_skipped,
                        caller_reg,
                    });
                }

                OpCode::Interpolate { dst, part_count } => {
                    let staged = self.drain_staged_args(part_count as usize);
                    let mut out = String::new();
                    for item in staged {
                        out.push_str(&item.value.to_php_string());
                    }
                    self.registers[base_reg + dst.index()] = PhpValue::string(out);
                }

                OpCode::CaptureUpvalue { src, name, is_by_ref } => {
                    let upval_name = {
                        let chunk = self.chunk_for_func(func_id);
                        chunk
                            .get_string_constant(name)
                            .unwrap_or_default()
                            .to_string()
                    };
                    let val = self.registers[base_reg + src.index()].clone();
                    self.upvalue_buffer.push((upval_name, val, is_by_ref));
                }

                OpCode::MakeClosure {
                    dst,
                    func_id: target_fn,
                    upvalue_count,
                    category,
                } => {
                    let count = upvalue_count as usize;
                    let start = self.upvalue_buffer.len().saturating_sub(count);
                    let captured: Vec<PhpValue> = self
                        .upvalue_buffer
                        .drain(start..)
                        .map(|(_, v, _)| v)
                        .collect();

                    let (fn_name, is_static) = self
                        .module
                        .get_function(target_fn)
                        .map(|f| (f.name.clone(), f.is_static))
                        .unwrap_or_else(|| (format!("fn#{}", target_fn.0), false));

                    self.registers[base_reg + dst.index()] =
                        PhpValue::closure(target_fn, fn_name, captured, is_static, category);
                }

                OpCode::CallFunction {
                    dst,
                    name,
                    arg_count,
                } => {
                    let staged = self.drain_staged_args(arg_count as usize);
                    let fn_name = {
                        let chunk = self.chunk_for_func(func_id);
                        chunk
                            .get_string_constant(name)
                            .unwrap_or_default()
                            .to_string()
                    };

                    if fn_name == "app" && !self.module.function_index.contains_key("app") {
                        if let Some(arg) = staged.first() {
                            let abs = arg.value.to_php_string();
                            let mut container = std::mem::take(&mut self.container);
                            let res = container.resolve(&abs, self);
                            self.container = container;
                            let val = res.map_err(|e| self.make_error(RuntimeErrorKind::Custom { message: e.to_string() }, span))?;
                            self.registers[base_reg + dst.index()] = val;
                            continue;
                        } else {
                            let app_obj = VmObject::new("Application");
                            self.registers[base_reg + dst.index()] = PhpValue::object(app_obj);
                            continue;
                        }
                    }

                    let clean_fn = fn_name.strip_prefix('\\').unwrap_or(&fn_name);
                    if (clean_fn.eq_ignore_ascii_case("now") || clean_fn.eq_ignore_ascii_case("date"))
                        && !self.module.function_index.contains_key(&fn_name)
                    {
                        let raw_args: Vec<PhpValue> = staged.into_iter().map(|a| a.value).collect();
                        let val = crate::vm::time::dispatch_time(clean_fn, &raw_args)
                            .map_err(|e| self.make_error(e, span))?;
                        self.registers[base_reg + dst.index()] = val;
                        continue;
                    }

                    if (clean_fn.eq_ignore_ascii_case("ini_get") || clean_fn.eq_ignore_ascii_case("ini_set"))
                        && !self.module.function_index.contains_key(&fn_name)
                    {
                        if clean_fn.eq_ignore_ascii_case("ini_get") {
                            let key = staged.first().map(|a| a.value.to_php_string()).unwrap_or_default();
                            let clean_key = key.trim_matches('"').trim_matches('\'');
                            if clean_key == "zend.exception_ignore_args" || key == "zend.exception_ignore_args" {
                                self.registers[base_reg + dst.index()] =
                                    PhpValue::string(if self.exception_ignore_args { "1" } else { "0" });
                            } else if clean_key == "max_loop_iterations" || key == "max_loop_iterations" {
                                self.registers[base_reg + dst.index()] =
                                    PhpValue::string(if self.exempt_loop_limits { "0".to_string() } else { self.dev_loop_hard_limit.to_string() });
                            } else if clean_key == "warn_loop_iterations" || key == "warn_loop_iterations" {
                                self.registers[base_reg + dst.index()] =
                                    PhpValue::string(self.dev_loop_warn_threshold.to_string());
                            } else if clean_key == "instruction_budget" || key == "instruction_budget" {
                                self.registers[base_reg + dst.index()] =
                                    PhpValue::string(self.tick_budget.to_string());
                            } else {
                                self.registers[base_reg + dst.index()] = PhpValue::string("");
                            }
                            continue;
                        } else if clean_fn.eq_ignore_ascii_case("ini_set") {
                            let key = staged.first().map(|a| a.value.to_php_string()).unwrap_or_default();
                            let val = staged.get(1).map(|a| a.value.to_php_string()).unwrap_or_default();
                            let clean_key = key.trim_matches('"').trim_matches('\'');
                            let clean_val = val.trim_matches('"').trim_matches('\'');
                            if clean_key == "zend.exception_ignore_args" || key == "zend.exception_ignore_args" {
                                let old = if self.exception_ignore_args { "1" } else { "0" };
                                self.exception_ignore_args =
                                    clean_val == "1" || clean_val.eq_ignore_ascii_case("true") || clean_val.eq_ignore_ascii_case("on")
                                    || val == "1" || val.eq_ignore_ascii_case("true") || val.eq_ignore_ascii_case("on");
                                self.registers[base_reg + dst.index()] = PhpValue::string(old);
                            } else if clean_key == "max_loop_iterations" || key == "max_loop_iterations" {
                                let old = if self.exempt_loop_limits { "0".to_string() } else { self.dev_loop_hard_limit.to_string() };
                                if let Ok(parsed) = clean_val.parse::<u64>() {
                                    if parsed == 0 {
                                        self.exempt_loop_limits = true;
                                    } else {
                                        self.exempt_loop_limits = false;
                                        self.dev_loop_hard_limit = parsed;
                                    }
                                }
                                self.registers[base_reg + dst.index()] = PhpValue::string(old);
                            } else if clean_key == "warn_loop_iterations" || key == "warn_loop_iterations" {
                                let old = self.dev_loop_warn_threshold.to_string();
                                if let Ok(parsed) = clean_val.parse::<u64>() {
                                    self.dev_loop_warn_threshold = parsed;
                                }
                                self.registers[base_reg + dst.index()] = PhpValue::string(old);
                            } else if clean_key == "instruction_budget" || key == "instruction_budget" {
                                let old = self.tick_budget.to_string();
                                if let Ok(parsed) = clean_val.parse::<u32>() {
                                    self.tick_budget = parsed.max(1);
                                    self.tick_counter = self.tick_budget;
                                }
                                self.registers[base_reg + dst.index()] = PhpValue::string(old);
                            } else {
                                self.registers[base_reg + dst.index()] = PhpValue::string("");
                            }
                            continue;
                        }
                    }

                    if (clean_fn.eq_ignore_ascii_case("set_error_handler")
                        || clean_fn.eq_ignore_ascii_case("restore_error_handler")
                        || clean_fn.eq_ignore_ascii_case("set_exception_handler")
                        || clean_fn.eq_ignore_ascii_case("restore_exception_handler")
                        || clean_fn.eq_ignore_ascii_case("trigger_error"))
                        && !self.module.function_index.contains_key(&fn_name)
                    {
                        if clean_fn.eq_ignore_ascii_case("set_error_handler") {
                            let cb = staged.first().map(|a| a.value.clone()).unwrap_or(PhpValue::Null);
                            let levels = staged.get(1).and_then(|a| match &a.value {
                                PhpValue::Int(i) => Some(*i as i32),
                                _ => None,
                            });
                            let prev = self.set_error_handler(cb, levels);
                            self.registers[base_reg + dst.index()] = prev;
                            continue;
                        } else if clean_fn.eq_ignore_ascii_case("restore_error_handler") {
                            let res = self.restore_error_handler();
                            self.registers[base_reg + dst.index()] = PhpValue::Bool(res);
                            continue;
                        } else if clean_fn.eq_ignore_ascii_case("set_exception_handler") {
                            let cb = staged.first().map(|a| a.value.clone()).unwrap_or(PhpValue::Null);
                            let prev = self.set_exception_handler(cb);
                            self.registers[base_reg + dst.index()] = prev;
                            continue;
                        } else if clean_fn.eq_ignore_ascii_case("restore_exception_handler") {
                            let res = self.restore_exception_handler();
                            self.registers[base_reg + dst.index()] = PhpValue::Bool(res);
                            continue;
                        } else if clean_fn.eq_ignore_ascii_case("trigger_error") {
                            let msg = staged.first().map(|a| a.value.to_php_string()).unwrap_or_default();
                            let level = staged.get(1).and_then(|a| match &a.value {
                                PhpValue::Int(i) => Some(*i as i32),
                                _ => None,
                            });
                            let res = self.trigger_error(&msg, level, span)?;
                            self.registers[base_reg + dst.index()] = PhpValue::Bool(res);
                            continue;
                        }
                    }

                    let target_id = self
                        .module
                        .function_index
                        .get(&fn_name)
                        .copied()
                        .ok_or_else(|| {
                            self.make_error(
                                RuntimeErrorKind::UndefinedFunction {
                                    name: fn_name.clone(),
                                },
                                span,
                            )
                        })?;

                    self.push_compiled_call_frame(
                        target_id,
                        None,
                        &staged,
                        &[],
                        Some(base_reg + dst.index()),
                        None,
                        span,
                    )?;
                }

                OpCode::CallDynamic {
                    dst,
                    callee,
                    arg_count,
                } => {
                    let staged = self.drain_staged_args(arg_count as usize);
                    let callee_val = self.registers[base_reg + callee.index()].clone();
                    match callee_val {
                        PhpValue::Closure(closure) => {
                            self.push_compiled_call_frame(
                                closure.func_id,
                                None,
                                &staged,
                                &closure.upvalues,
                                Some(base_reg + dst.index()),
                                None,
                                span,
                            )?;
                        }
                        PhpValue::Object(ref obj_rc) => {
                            let cls_name = obj_rc.borrow().class_name.clone();

                            if let Some(target_id) = self.resolve_method_id(&cls_name, "__invoke") {
                                self.push_compiled_call_frame(
                                    target_id,
                                    Some(callee_val),
                                    &staged,
                                    &[],
                                    Some(base_reg + dst.index()),
                                    Some(cls_name),
                                    span,
                                )?;
                            } else {
                                return Err(self.make_error(
                                    RuntimeErrorKind::NotCallable {
                                        type_name: cls_name,
                                    },
                                    span,
                                ));
                            }
                        }
                        PhpValue::String(fn_name) => {
                            let target_id = self
                                .module
                                .function_index
                                .get(fn_name.as_str())
                                .copied()
                                .ok_or_else(|| {
                                    self.make_error(
                                        RuntimeErrorKind::UndefinedFunction {
                                            name: (*fn_name).to_string(),
                                        },
                                        span,
                                    )
                                })?;
                            self.push_compiled_call_frame(
                                target_id,
                                None,
                                &staged,
                                &[],
                                Some(base_reg + dst.index()),
                                None,
                                span,
                            )?;
                        }
                        other => {
                            return Err(self.make_error(
                                RuntimeErrorKind::NotCallable {
                                    type_name: self.runtime_type_label(&other),
                                },
                                span,
                            ));
                        }
                    }
                }

                OpCode::FacadeCall {
                    dst,
                    facade,
                    method,
                    arg_count,
                } => {
                    let staged = self.drain_staged_args(arg_count as usize);
                    let (facade_name, method_name) = {
                        let chunk = self.chunk_for_func(func_id);
                        let f = chunk
                            .get_string_constant(facade)
                            .unwrap_or_default()
                            .to_string();
                        let m = chunk
                            .get_string_constant(method)
                            .unwrap_or_default()
                            .to_string();
                        (f, m)
                    };

                    let flat_args = Self::flatten_positional_args(&staged);
                    let res = match self.exec_facade_call(&facade_name, &method_name, &flat_args, span) {
                        Ok(v) => v,
                        Err(err) => {
                            if matches!(err.kind, RuntimeErrorKind::Exit(_) | RuntimeErrorKind::Die(_)) {
                                return Err(err);
                            }
                            let (cls, msg) = match &err.kind {
                                RuntimeErrorKind::UncaughtException { class, message } => (class.clone(), message.clone()),
                                RuntimeErrorKind::Custom { message } => ("Exception".to_string(), message.clone()),
                                RuntimeErrorKind::TypeError { message } => ("TypeError".to_string(), message.clone()),
                                _ => ("Exception".to_string(), err.kind.to_string()),
                            };
                            let mut obj = VmObject::new(&cls);
                            obj.set_property("message", PhpValue::string(msg));
                            if self.try_dispatch_to_catch(PhpValue::object(obj), stop_depth) {
                                continue;
                            } else {
                                return Err(err);
                            }
                        }
                    };
                    self.registers[base_reg + dst.index()] = res;
                }

                OpCode::StaticCall {
                    dst,
                    class,
                    method,
                    arg_count,
                } => {
                    let staged = self.drain_staged_args(arg_count as usize);
                    let (mut class_name, method_name) = {
                        let chunk = self.chunk_for_func(func_id);
                        let c = chunk
                            .get_string_constant(class)
                            .unwrap_or_default()
                            .to_string();
                        let m = chunk
                            .get_string_constant(method)
                            .unwrap_or_default()
                            .to_string();
                        (c, m)
                    };
                    if class_name == "static" {
                        class_name = self.current_called_class().unwrap_or(class_name);
                    }

                    // Dynamic Macro registration: `ClassName::macro('name', fn(...) => ...)`
                    if method_name == "macro" {
                        if let (Some(name_val), Some(arg1)) = (staged.first(), staged.get(1)) {
                            if let PhpValue::Closure(ref cb) = arg1.value {
                                let name_str = name_val.value.to_php_string();
                                crate::vm::facade::register_runtime_closure_macro(&class_name, &name_str, (**cb).clone());
                                self.registers[base_reg + dst.index()] = PhpValue::Bool(true);
                                continue;
                            }
                        }
                    }

                    // Dynamic Macro execution: `ClassName::customMacro(...)`
                    if let Some(macro_closure) = crate::vm::facade::get_runtime_closure_macro(&class_name, &method_name) {
                        let arg_values: Vec<PhpValue> = staged.into_iter().map(|a| a.value).collect();
                        let res = self.invoke_closure_sync(&macro_closure, arg_values, span)?;
                        self.registers[base_reg + dst.index()] = res;
                        continue;
                    }

                    let flat_args = Self::flatten_positional_args(&staged);

                    match self.exec_enum_static_method(&class_name, &method_name, &flat_args, stop_depth, span)? {
                        EnumStaticCallResult::Handled(res) => {
                            self.registers[base_reg + dst.index()] = res;
                            continue;
                        }
                        EnumStaticCallResult::DispatchedToCatch => {
                            continue;
                        }
                        EnumStaticCallResult::NotAnEnumMethod => {}
                    }

                    let is_parent_or_builtin_throwable = if method_name == "__construct" {
                        if class_name == "parent" {
                            let this_val = &self.registers[base_reg];
                            if let PhpValue::Object(o) = this_val {
                                o.borrow().throwable.is_some() || self.is_class_instance_of(&o.borrow().class_name, "Throwable")
                            } else {
                                false
                            }
                        } else {
                            is_builtin_throwable(&class_name)
                        }
                    } else {
                        false
                    };

                    if is_parent_or_builtin_throwable {
                        let this_val = self.registers[base_reg].clone();
                        if let PhpValue::Object(_) = this_val {
                            let _ = self.exec_builtin_object_method(
                                &class_name,
                                &this_val,
                                "__construct",
                                &flat_args,
                                span,
                            )?;
                            self.registers[base_reg + dst.index()] = PhpValue::Null;
                            continue;
                        }
                    }

                    if let Some(target_id) = self.resolve_method_id(&class_name, &method_name) {
                        self.push_compiled_call_frame(
                            target_id,
                            None,
                            &staged,
                            &[],
                            Some(base_reg + dst.index()),
                            Some(class_name.clone()),
                            span,
                        )?;
                    } else {
                        // Lookup order:
                        // 1. (Checked: real static method was None)
                        // 2. Instance method via parent:: or self:: if $this is compatible
                        let mut instance_dispatched = false;
                        if class_name == "parent" || class_name == "self" {
                            let this_val = self.registers[base_reg].clone();
                            if let PhpValue::Object(ref obj_rc) = this_val {
                                let obj_class = obj_rc.borrow().class_name.clone();
                                let target_cls = if class_name == "parent" {
                                    self.module.get_class_by_name(&obj_class).and_then(|c| c.extends.clone())
                                } else {
                                    Some(obj_class)
                                };
                                if let Some(ref t_cls) = target_cls {
                                    if let Some(mid) = self.resolve_method_id(t_cls, &method_name) {
                                        self.push_compiled_call_frame(
                                            mid,
                                            Some(this_val),
                                            &staged,
                                            &[],
                                            Some(base_reg + dst.index()),
                                            Some(t_cls.clone()),
                                            span,
                                        )?;
                                        instance_dispatched = true;
                                    }
                                }
                            }
                        }
                        if instance_dispatched {
                            continue;
                        }

                        // 3. __forward:
                        let fwd_mid = self.module.get_class_info_by_name(&class_name)
                            .and_then(|ci| ci.static_forward)
                            .or_else(|| self.resolve_method_id(&class_name, "__forward"));

                        if let Some(fwd_id) = fwd_mid {
                            let recv = self.invoke_compiled_function_sync_with_called_class(
                                fwd_id,
                                vec![],
                                Some(class_name.clone()),
                                span,
                            )?;
                            if let PhpValue::Object(ref rc) = recv {
                                rc.borrow_mut().forwarded_from = Some(class_name.clone());
                                let recv_cls = rc.borrow().class_name.clone();
                                if let Some(target_mid) = self.resolve_method_id(&recv_cls, &method_name) {
                                    self.push_compiled_call_frame(
                                        target_mid,
                                        Some(recv),
                                        &staged,
                                        &[],
                                        Some(base_reg + dst.index()),
                                        Some(recv_cls),
                                        span,
                                    )?;
                                    continue;
                                } else {
                                    match self.exec_builtin_object_method(
                                        &recv_cls,
                                        &recv,
                                        &method_name,
                                        &flat_args,
                                        span,
                                    ) {
                                        Ok(res) => {
                                            self.registers[base_reg + dst.index()] = res;
                                            continue;
                                        }
                                        Err(_) => {
                                            return Err(self.make_error(
                                                RuntimeErrorKind::UndefinedForwardedMethod {
                                                    class: recv_cls,
                                                    method: method_name,
                                                    origin_class: class_name,
                                                },
                                                span,
                                            ));
                                        }
                                    }
                                }
                            }
                        }

                        // 4. __callStatic:
                        if let Some(call_static_mid) = self.resolve_method_id(&class_name, "__callStatic") {
                            let call_static_args = vec![
                                StagedArg::simple(PhpValue::string(method_name.clone())),
                                StagedArg::simple(PhpValue::list(flat_args)),
                            ];
                            self.push_compiled_call_frame(
                                call_static_mid,
                                None,
                                &call_static_args,
                                &[],
                                Some(base_reg + dst.index()),
                                Some(class_name.clone()),
                                span,
                            )?;
                            continue;
                        }

                        // 5. UndefinedMethod error
                        return Err(self.make_error(
                            RuntimeErrorKind::UndefinedMethod {
                                class: class_name.clone(),
                                method: method_name.clone(),
                            },
                            span,
                        ));
                    }
                }

                OpCode::MethodCall {
                    dst,
                    object,
                    method,
                    arg_count,
                    is_nullsafe,
                } => {
                    let staged = self.drain_staged_args(arg_count as usize);
                    let receiver = self.registers[base_reg + object.index()].clone();
                    if is_nullsafe && receiver.is_null() {
                        self.registers[base_reg + dst.index()] = PhpValue::Null;
                        continue;
                    }

                    let method_name = {
                        let chunk = self.chunk_for_func(func_id);
                        chunk
                            .get_string_constant(method)
                            .unwrap_or_default()
                            .to_string()
                    };

                    if self.dispatch_instance_method(
                        receiver,
                        method_name,
                        staged,
                        base_reg,
                        dst,
                        stop_depth,
                        span,
                    )? {
                        continue;
                    }
                }

                OpCode::MethodCallDirect {
                    dst,
                    object,
                    method_id,
                    arg_count,
                    is_nullsafe,
                } => {
                    let staged = self.drain_staged_args(arg_count as usize);
                    let receiver = self.registers[base_reg + object.index()].clone();
                    if is_nullsafe && receiver.is_null() {
                        self.registers[base_reg + dst.index()] = PhpValue::Null;
                        continue;
                    }

                    let cls_name = match &receiver {
                        PhpValue::Object(obj_rc) => obj_rc.borrow().class_name.clone(),
                        _ => self
                            .module
                            .functions
                            .get(method_id.index())
                            .and_then(|f| f.enclosing_class.clone())
                            .unwrap_or_default(),
                    };

                    self.push_compiled_call_frame(
                        method_id,
                        Some(receiver),
                        &staged,
                        &[],
                        Some(base_reg + dst.index()),
                        Some(cls_name),
                        span,
                    )?;
                    continue;
                }

                OpCode::MethodCallDynamic {
                    dst,
                    object,
                    method_reg,
                    arg_count,
                    is_nullsafe,
                } => {
                    let staged = self.drain_staged_args(arg_count as usize);
                    let receiver = self.registers[base_reg + object.index()].clone();
                    if is_nullsafe && receiver.is_null() {
                        self.registers[base_reg + dst.index()] = PhpValue::Null;
                        continue;
                    }

                    let method_name =
                        self.registers[base_reg + method_reg.index()].to_php_string();

                    if self.dispatch_instance_method(
                        receiver,
                        method_name,
                        staged,
                        base_reg,
                        dst,
                        stop_depth,
                        span,
                    )? {
                        continue;
                    }
                }

                // -----------------------------------------------------------------
                // 7. Object-Oriented Operations & Properties
                // -----------------------------------------------------------------
                OpCode::NewObject {
                    dst,
                    class,
                    arg_count,
                } => {
                    let staged = self.drain_staged_args(arg_count as usize);
                    let class_name = {
                        let chunk = self.chunk_for_func(func_id);
                        chunk
                            .get_string_constant(class)
                            .unwrap_or_default()
                            .to_string()
                    };
                    let clean_name = if class_name == "static" {
                        self.current_called_class().unwrap_or(class_name)
                    } else {
                        class_name.strip_prefix('\\').unwrap_or(&class_name).to_string()
                    };

                    let instance = self.instantiate_class(&clean_name, span)?;

                    // If constructor arguments were passed to built-in Exception/Error, initialize message/code/previous
                    if (self.is_class_instance_of(&clean_name, "Throwable")
                        || clean_name.ends_with("Exception")
                        || clean_name.ends_with("Error"))
                        && self.resolve_method_id(&clean_name, "__construct").is_none()
                    {
                        self.apply_throwable_constructor_args(&instance, &clean_name, &staged);
                    } else if (clean_name == "ArrayIterator"
                        || clean_name == "Collection"
                        || clean_name == "Illuminate\\Support\\Collection")
                        && !self.module.class_index.contains_key(&clean_name)
                    {
                        if let Some(items_arg) = staged.first() {
                            if let PhpValue::Object(ref o) = instance {
                                o.borrow_mut().set_property("items", items_arg.value.clone());
                            }
                        }
                    } else if (clean_name == "LengthAwarePaginator" || clean_name == "Paginator")
                        && !self.module.class_index.contains_key(&clean_name)
                    {
                        if let PhpValue::Object(ref o) = instance {
                            if let Some(items_arg) = staged.first() {
                                o.borrow_mut().set_property("items", items_arg.value.clone());
                            }
                            if let Some(total_arg) = staged.get(1) {
                                o.borrow_mut().set_property("total", total_arg.value.clone());
                            }
                            if let Some(pp_arg) = staged.get(2) {
                                o.borrow_mut().set_property("perPage", pp_arg.value.clone());
                            }
                            if let Some(cp_arg) = staged.get(3) {
                                o.borrow_mut().set_property("currentPage", cp_arg.value.clone());
                            }
                        }
                    }

                    self.registers[base_reg + dst.index()] = instance.clone();

                    // If the class defines `__construct`, invoke it with `$this = instance`
                    if let Some(ctor_id) = self.resolve_method_id(&clean_name, "__construct") {
                        self.push_compiled_call_frame(
                            ctor_id,
                            Some(instance),
                            &staged,
                            &[],
                            None, // constructor return is void; `dst` retains `instance`
                            Some(clean_name.clone()),
                            span,
                        )?;
                    }
                }

                OpCode::NewObjectDynamic {
                    dst,
                    class_reg,
                    arg_count,
                } => {
                    let staged = self.drain_staged_args(arg_count as usize);
                    let class_name = self.registers[base_reg + class_reg.index()].to_php_string();
                    let clean_name = if class_name == "static" {
                        self.current_called_class().unwrap_or(class_name)
                    } else {
                        class_name.strip_prefix('\\').unwrap_or(&class_name).to_string()
                    };

                    let instance = self.instantiate_class(&clean_name, span)?;

                    if (self.is_class_instance_of(&clean_name, "Throwable")
                        || clean_name.ends_with("Exception")
                        || clean_name.ends_with("Error"))
                        && self.resolve_method_id(&clean_name, "__construct").is_none()
                    {
                        self.apply_throwable_constructor_args(&instance, &clean_name, &staged);
                    } else if (clean_name == "ArrayIterator"
                        || clean_name == "Collection"
                        || clean_name == "Illuminate\\Support\\Collection")
                        && !self.module.class_index.contains_key(&clean_name)
                    {
                        if let Some(items_arg) = staged.first() {
                            if let PhpValue::Object(ref o) = instance {
                                o.borrow_mut().set_property("items", items_arg.value.clone());
                            }
                        }
                    } else if (clean_name == "LengthAwarePaginator" || clean_name == "Paginator")
                        && !self.module.class_index.contains_key(&clean_name)
                    {
                        if let PhpValue::Object(ref o) = instance {
                            if let Some(items_arg) = staged.first() {
                                o.borrow_mut().set_property("items", items_arg.value.clone());
                            }
                            if let Some(total_arg) = staged.get(1) {
                                o.borrow_mut().set_property("total", total_arg.value.clone());
                            }
                            if let Some(pp_arg) = staged.get(2) {
                                o.borrow_mut().set_property("perPage", pp_arg.value.clone());
                            }
                            if let Some(cp_arg) = staged.get(3) {
                                o.borrow_mut().set_property("currentPage", cp_arg.value.clone());
                            }
                        }
                    }

                    self.registers[base_reg + dst.index()] = instance.clone();

                    if let Some(ctor_id) = self.resolve_method_id(&clean_name, "__construct") {
                        self.push_compiled_call_frame(
                            ctor_id,
                            Some(instance),
                            &staged,
                            &[],
                            None,
                            Some(clean_name.clone()),
                            span,
                        )?;
                    }
                }

                OpCode::GetProp { dst, object, prop } => {
                    let prop_name = {
                        let chunk = self.chunk_for_func(func_id);
                        chunk
                            .get_string_constant(prop)
                            .unwrap_or_default()
                            .to_string()
                    };
                    let obj_val = &self.registers[base_reg + object.index()];
                    let val = match obj_val {
                        PhpValue::Object(obj_rc) => obj_rc
                            .borrow()
                            .get_property(&prop_name)
                            .cloned()
                            .unwrap_or(PhpValue::Null),
                        PhpValue::Future(f_rc) => {
                            let mut fut = f_rc.borrow_mut();
                            match prop_name.as_str() {
                                "state" | "status" => PhpValue::string(fut.state().as_str()),
                                "is_completed" | "isCompleted" | "is_ready" | "isReady" => {
                                    PhpValue::Bool(fut.poll_completion())
                                }
                                "is_pending" | "isPending" => PhpValue::Bool(!fut.poll_completion()),
                                "value" => {
                                    fut.poll_completion();
                                    fut.value.clone().unwrap_or(PhpValue::Null)
                                }
                                "error" => {
                                    fut.poll_completion();
                                    fut.error
                                        .as_ref()
                                        .map(|e| PhpValue::string(e.clone()))
                                        .unwrap_or(PhpValue::Null)
                                }
                                _ => {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::UndefinedProperty {
                                            class: "Future".to_string(),
                                            property: prop_name,
                                        },
                                        span,
                                    ));
                                }
                            }
                        }
                        PhpValue::Null => PhpValue::Null,
                        PhpValue::Dict(_) => {
                            return Err(self.make_error(
                                RuntimeErrorKind::Custom {
                                    message: format!("Cannot access property '{}' on dict via '->'. Use bracket syntax '$x[\"{}\"]' or '$x->get(\"{}\")' instead.", prop_name, prop_name, prop_name),
                                },
                                span,
                            ));
                        }
                        other => {
                            return Err(self.make_error(
                                RuntimeErrorKind::UndefinedProperty {
                                    class: other.type_name().to_string(),
                                    property: prop_name,
                                },
                                span,
                            ));
                        }
                    };
                    self.registers[base_reg + dst.index()] = val;
                }

                OpCode::GetPropDynamic { dst, object, prop_reg } => {
                    let prop_name = self.registers[base_reg + prop_reg.index()].to_php_string();
                    let obj_val = &self.registers[base_reg + object.index()];
                    let val = match obj_val {
                        PhpValue::Object(obj_rc) => obj_rc
                            .borrow()
                            .get_property(&prop_name)
                            .cloned()
                            .unwrap_or(PhpValue::Null),
                        PhpValue::Future(f_rc) => {
                            let mut fut = f_rc.borrow_mut();
                            match prop_name.as_str() {
                                "state" | "status" => PhpValue::string(fut.state().as_str()),
                                "is_completed" | "isCompleted" | "is_ready" | "isReady" => {
                                    PhpValue::Bool(fut.poll_completion())
                                }
                                "is_pending" | "isPending" => PhpValue::Bool(!fut.poll_completion()),
                                "value" => {
                                    fut.poll_completion();
                                    fut.value.clone().unwrap_or(PhpValue::Null)
                                }
                                "error" => {
                                    fut.poll_completion();
                                    fut.error
                                        .as_ref()
                                        .map(|e| PhpValue::string(e.clone()))
                                        .unwrap_or(PhpValue::Null)
                                }
                                _ => {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::UndefinedProperty {
                                            class: "Future".to_string(),
                                            property: prop_name,
                                        },
                                        span,
                                    ));
                                }
                            }
                        }
                        PhpValue::Null => PhpValue::Null,
                        PhpValue::Dict(_) => {
                            return Err(self.make_error(
                                RuntimeErrorKind::Custom {
                                    message: format!("Cannot access property '{}' on dict via '->'. Use bracket syntax '$x[\"{}\"]' or '$x->get(\"{}\")' instead.", prop_name, prop_name, prop_name),
                                },
                                span,
                            ));
                        }
                        other => {
                            return Err(self.make_error(
                                RuntimeErrorKind::UndefinedProperty {
                                    class: other.type_name().to_string(),
                                    property: prop_name,
                                },
                                span,
                            ));
                        }
                    };
                    self.registers[base_reg + dst.index()] = val;
                }

                OpCode::SetProp {
                    object,
                    prop,
                    value,
                } => {
                    let prop_name = {
                        let chunk = self.chunk_for_func(func_id);
                        chunk
                            .get_string_constant(prop)
                            .unwrap_or_default()
                            .to_string()
                    };
                    let val = self.registers[base_reg + value.index()].clone();
                    let is_in_constructor = self.frames[frame_idx]
                        .function_name
                        .ends_with("::__construct");

                    match &self.registers[base_reg + object.index()] {
                        PhpValue::Object(obj_rc) => {
                            let mut obj = obj_rc.borrow_mut();
                            if let Some(&is_ro) = obj.readonly_props.get(&prop_name) {
                                if is_ro && !is_in_constructor && obj.properties.contains_key(&prop_name)
                                {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::ReadonlyPropertyModification {
                                            class: obj.class_name.clone(),
                                            property: prop_name,
                                        },
                                        span,
                                    ));
                                }
                            }
                            obj.set_property(prop_name, val);
                        }
                        PhpValue::Dict(_) => {
                            return Err(self.make_error(
                                RuntimeErrorKind::Custom {
                                    message: format!("Cannot set property '{}' on dict via '->'. Use bracket syntax '$x[\"{}\"] = ...' or '$x->set(\"{}\", ...)' instead.", prop_name, prop_name, prop_name),
                                },
                                span,
                            ));
                        }
                        other => {
                            return Err(self.make_error(
                                RuntimeErrorKind::UndefinedProperty {
                                    class: other.type_name().to_string(),
                                    property: prop_name,
                                },
                                span,
                            ));
                        }
                    }
                }

                OpCode::SetPropDynamic {
                    object,
                    prop_reg,
                    value,
                } => {
                    let prop_name = self.registers[base_reg + prop_reg.index()].to_php_string();
                    let val = self.registers[base_reg + value.index()].clone();
                    let is_in_constructor = self.frames[frame_idx]
                        .function_name
                        .ends_with("::__construct");

                    match &self.registers[base_reg + object.index()] {
                        PhpValue::Object(obj_rc) => {
                            let mut obj = obj_rc.borrow_mut();
                            if let Some(&is_ro) = obj.readonly_props.get(&prop_name) {
                                if is_ro && !is_in_constructor && obj.properties.contains_key(&prop_name)
                                {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::ReadonlyPropertyModification {
                                            class: obj.class_name.clone(),
                                            property: prop_name,
                                        },
                                        span,
                                    ));
                                }
                            }
                            obj.set_property(prop_name, val);
                        }
                        PhpValue::Dict(_) => {
                            return Err(self.make_error(
                                RuntimeErrorKind::Custom {
                                    message: format!("Cannot set property '{}' on dict via '->'. Use bracket syntax '$x[\"{}\"] = ...' or '$x->set(\"{}\", ...)' instead.", prop_name, prop_name, prop_name),
                                },
                                span,
                            ));
                        }
                        other => {
                            return Err(self.make_error(
                                RuntimeErrorKind::UndefinedProperty {
                                    class: other.type_name().to_string(),
                                    property: prop_name,
                                },
                                span,
                            ));
                        }
                    }
                }

                OpCode::GetStaticProp { dst, class, prop } => {
                    let (cls_name, prop_name) = {
                        let chunk = self.chunk_for_func(func_id);
                        (
                            chunk
                                .get_string_constant(class)
                                .unwrap_or_default()
                                .to_string(),
                            chunk
                                .get_string_constant(prop)
                                .unwrap_or_default()
                                .to_string(),
                        )
                    };
                    let resolved_cls = if cls_name == "static" {
                        self.current_called_class().unwrap_or(cls_name)
                    } else {
                        cls_name
                    };
                    let val = self
                        .static_properties
                        .get(&(resolved_cls, prop_name))
                        .cloned()
                        .unwrap_or(PhpValue::Null);
                    self.registers[base_reg + dst.index()] = val;
                }

                OpCode::SetStaticProp { class, prop, value } => {
                    let (cls_name, prop_name) = {
                        let chunk = self.chunk_for_func(func_id);
                        (
                            chunk
                                .get_string_constant(class)
                                .unwrap_or_default()
                                .to_string(),
                            chunk
                                .get_string_constant(prop)
                                .unwrap_or_default()
                                .to_string(),
                        )
                    };
                    let resolved_cls = if cls_name == "static" {
                        self.current_called_class().unwrap_or(cls_name)
                    } else {
                        cls_name
                    };
                    let val = self.registers[base_reg + value.index()].clone();
                    self.static_properties.insert((resolved_cls, prop_name), val);
                }

                OpCode::GetClassConst {
                    dst,
                    class,
                    const_name,
                } => {
                    let (cls_name, c_name) = {
                        let chunk = self.chunk_for_func(func_id);
                        (
                            chunk
                                .get_string_constant(class)
                                .unwrap_or_default()
                                .to_string(),
                            chunk
                                .get_string_constant(const_name)
                                .unwrap_or_default()
                                .to_string(),
                        )
                    };
                    let resolved_cls = if cls_name == "static" {
                        self.current_called_class().unwrap_or(cls_name)
                    } else {
                        cls_name
                    };
                    if c_name == "class" {
                        self.registers[base_reg + dst.index()] = PhpValue::string(resolved_cls);
                    } else if resolved_cls == "Time" || resolved_cls == "\\Time"
                        || resolved_cls == "Carbon" || resolved_cls == "\\Carbon" {
                        if let Some(val) = crate::vm::time::get_time_constant(&c_name) {
                            self.registers[base_reg + dst.index()] = val;
                        } else {
                            return Err(self.make_error(
                                RuntimeErrorKind::UndefinedClassConstant {
                                    class: resolved_cls,
                                    constant: c_name,
                                },
                                span,
                            ));
                        }
                    } else if resolved_cls == "Duration" || resolved_cls == "\\Duration"
                        || resolved_cls == "CarbonInterval" || resolved_cls == "\\CarbonInterval" {
                        if let Some(val) = crate::vm::duration::get_duration_constant(&c_name) {
                            self.registers[base_reg + dst.index()] = val;
                        } else {
                            return Err(self.make_error(
                                RuntimeErrorKind::UndefinedClassConstant {
                                    class: resolved_cls,
                                    constant: c_name,
                                },
                                span,
                            ));
                        }
                    } else if let Some(val) = self.class_constants.get(&(resolved_cls.clone(), c_name.clone())) {
                        self.registers[base_reg + dst.index()] = val.clone();
                    } else {
                        return Err(self.make_error(
                            RuntimeErrorKind::UndefinedClassConstant {
                                class: resolved_cls,
                                constant: c_name,
                            },
                            span,
                        ));
                    }
                }

                OpCode::Instanceof { dst, object, class } => {
                    let raw_cls = {
                        let chunk = self.chunk_for_func(func_id);
                        chunk
                            .get_string_constant(class)
                            .unwrap_or_default()
                            .to_string()
                    };
                    let target_cls = raw_cls.strip_prefix('\\').unwrap_or(&raw_cls).to_string();
                    let obj_val = &self.registers[base_reg + object.index()];
                    let is_inst = match obj_val {
                        PhpValue::Object(obj_rc) => {
                            let actual_cls = &obj_rc.borrow().class_name;
                            self.is_class_instance_of(actual_cls, &target_cls)
                        }
                        PhpValue::Future(_) => target_cls == "Future",
                        PhpValue::Closure(_) => target_cls == "Closure",
                        _ => false,
                    };
                    self.registers[base_reg + dst.index()] = PhpValue::Bool(is_inst);
                }

                OpCode::InstanceofDynamic { dst, object, class } => {
                    let raw_cls = match &self.registers[base_reg + class.index()] {
                        PhpValue::String(s) => s.as_str().to_string(),
                        PhpValue::Object(obj_rc) => obj_rc.borrow().class_name.clone(),
                        _ => String::new(),
                    };
                    let target_cls = raw_cls.strip_prefix('\\').unwrap_or(&raw_cls).to_string();
                    let obj_val = &self.registers[base_reg + object.index()];
                    let is_inst = match obj_val {
                        PhpValue::Object(obj_rc) => {
                            let actual_cls = &obj_rc.borrow().class_name;
                            self.is_class_instance_of(actual_cls, &target_cls)
                        }
                        PhpValue::Future(_) => target_cls == "Future",
                        PhpValue::Closure(_) => target_cls == "Closure",
                        _ => false,
                    };
                    self.registers[base_reg + dst.index()] = PhpValue::Bool(is_inst);
                }

                // -----------------------------------------------------------------
                // 8. Global Scope, Exceptions, Concurrency, Generators, & Output
                // -----------------------------------------------------------------
                OpCode::LoadGlobal { dst, name } => {
                    let g_name = {
                        let chunk = self.chunk_for_func(func_id);
                        chunk
                            .get_string_constant(name)
                            .unwrap_or_default()
                            .to_string()
                    };
                    let val = self.globals.get(&g_name).cloned().unwrap_or(PhpValue::Null);
                    self.registers[base_reg + dst.index()] = val;
                }

                OpCode::StoreGlobal { name, src } => {
                    let g_name = {
                        let chunk = self.chunk_for_func(func_id);
                        chunk
                            .get_string_constant(name)
                            .unwrap_or_default()
                            .to_string()
                    };
                    let val = self.registers[base_reg + src.index()].clone();
                    self.globals.insert(g_name, val);
                }

                OpCode::Echo { src } => {
                    let text = self.registers[base_reg + src.index()].to_php_string();
                    self.output_buffer.push_str(&text);
                }

                OpCode::Defer { func_id: def_id } => {
                    self.defer_queue.push(def_id);
                }

                OpCode::Await { dst, future } => {
                    let fut_val = self.registers[base_reg + future.index()].clone();
                    let resolved = match fut_val {
                        PhpValue::Future(f_rc) => {
                            let mut b = f_rc.borrow_mut();
                            match b.wait_result() {
                                Ok(val) => val,
                                Err(err_msg) => {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::Custom {
                                            message: err_msg,
                                        },
                                        span,
                                    ));
                                }
                            }
                        }
                        other => other,
                    };
                    self.registers[base_reg + dst.index()] = resolved;
                }

                OpCode::ImportRust { path, alias } => {
                    let chunk = self.chunk_for_func(func_id);
                    let path_str = chunk.get_string_constant(path).unwrap_or_default().to_string();
                    let alias_str = alias.and_then(|a| chunk.get_string_constant(a)).map(|s| s.to_string());
                    let _ = crate::vm::rust_loader::load_and_register_rust_file(&path_str, alias_str.as_deref(), None);
                }

                OpCode::Yield { dst, value, .. } => {
                    let yielded = value
                        .map(|r| self.registers[base_reg + r.index()].clone())
                        .unwrap_or(PhpValue::Null);
                    self.registers[base_reg + dst.index()] = yielded;
                }

                OpCode::EnterTry { handler_id } => {
                    self.frames[frame_idx].active_try_handlers.push(handler_id);
                }

                OpCode::LeaveTry { handler_id } => {
                    if let Some(pos) = self.frames[frame_idx]
                        .active_try_handlers
                        .iter()
                        .rposition(|&h| h == handler_id)
                    {
                        self.frames[frame_idx].active_try_handlers.remove(pos);
                    }
                }

                OpCode::Throw { exception } => {
                    let exc_val = self.registers[base_reg + exception.index()].clone();
                    self.unwind_exception(exc_val, stop_depth, span)?;
                }

                OpCode::ThrowMatchError { subject } => {
                    let subj_str = self.registers[base_reg + subject.index()].to_php_string();
                    let mut err_obj = VmObject::new("UnhandledMatchError");
                    err_obj.set_property(
                        "message",
                        PhpValue::string(format!("Unhandled match value '{}'", subj_str)),
                    );
                    if !self.try_dispatch_to_catch(PhpValue::object(err_obj), stop_depth) {
                        return Err(self.make_error(
                            RuntimeErrorKind::UnhandledMatchError { subject: subj_str },
                            span,
                        ));
                    }
                }
            }
        }
    }

    // =========================================================================
    // Call Frame Setup & Synchronous Re-Entrant Invocations
    // =========================================================================

    fn push_compiled_call_frame(
        &mut self,
        target_id: FunctionId,
        receiver: Option<PhpValue>,
        staged_args: &[StagedArg],
        upvalues: &[PhpValue],
        caller_dst: Option<usize>,
        called_class: Option<String>,
        span: Span,
    ) -> Result<(), RuntimeError> {
        if self.frames.len() >= self.max_call_depth {
            return Err(self.make_error(
                RuntimeErrorKind::StackOverflow {
                    max_depth: self.max_call_depth,
                },
                span,
            ));
        }

        let func = self
            .module
            .get_function(target_id)
            .cloned()
            .ok_or_else(|| {
                self.make_error(
                    RuntimeErrorKind::UndefinedFunction {
                        name: format!("fn#{}", target_id.0),
                    },
                    span,
                )
            })?;

        let new_base = self.registers.len();
        let num_regs = (func.chunk.num_registers as usize).max(1);
        self.registers
            .resize(new_base + num_regs, PhpValue::Null);

        // 1. Bind `$this` to `r0` for instance methods
        let has_explicit_receiver = receiver.is_some();
        if let Some(ref this_val) = receiver {
            if !func.is_static {
                self.registers[new_base] = this_val.clone();
            }
        }

        // 2. Bind positional, unpacked, and named arguments to parameter registers
        let mut positional_values: Vec<(PhpValue, bool, bool, Option<usize>)> = Vec::new();
        let mut named_values: Vec<(String, PhpValue, bool, Option<usize>)> = Vec::new();
        for arg in staged_args {
            if let Some(ref name) = arg.name {
                named_values.push((name.clone(), arg.value.clone(), arg.is_reference, arg.caller_reg));
            } else if arg.is_unpack {
                for item in Self::extract_items(&arg.value) {
                    positional_values.push((item, false, false, None));
                }
            } else {
                positional_values.push((arg.value.clone(), arg.is_reference, arg.is_skipped, arg.caller_reg));
            }
        }

        if !named_values.is_empty() {
            let (arg_name, _, _, _) = &named_values[0];
            return Err(self.make_error(
                RuntimeErrorKind::Custom {
                    message: format!(
                        "Named parameters are not supported in RHP (found '${}' in call to '{}()'). To skip optional parameters, use extra commas (e.g. `fn($req, , $opt)`) at call site.",
                        arg_name, func.name
                    ),
                },
                span,
            ));
        }

        let mut ref_bindings = Vec::new();
        let mut assigned_params = vec![false; func.params.len()];
        for (idx, param) in func.params.iter().enumerate() {
            if param.is_variadic {
                let rest: Vec<PhpValue> = if idx < positional_values.len() {
                    positional_values[idx..].iter().map(|(v, _, _, _)| v.clone()).collect()
                } else {
                    Vec::new()
                };
                self.registers[new_base + param.reg.index()] = PhpValue::list(rest);
                assigned_params[idx] = true;
                break;
            } else if idx < positional_values.len() {
                let (val, is_ref, is_skipped, caller_reg) = &positional_values[idx];
                if *is_skipped {
                    if !param.has_default {
                        return Err(self.make_error(
                            RuntimeErrorKind::Custom {
                                message: format!(
                                    "Cannot skip required parameter '${}' in call to '{}()'",
                                    param.name, func.name
                                ),
                            },
                            span,
                        ));
                    }
                    // Optional parameter skipped: leave register as Null (unassigned)
                    // so the bytecode prologue evaluates the default expression
                    assigned_params[idx] = true;
                    continue;
                }
                if param.is_reference {
                    if !*is_ref {
                        return Err(self.make_error(
                            RuntimeErrorKind::MissingReferenceArgument {
                                callable: func.name.clone(),
                                param_name: param.name.clone(),
                                arg_index: idx + 1,
                            },
                            span,
                        ));
                    }
                    if let Some(caller_r) = caller_reg {
                        ref_bindings.push((new_base + param.reg.index(), *caller_r));
                    }
                } else if *is_ref {
                    return Err(self.make_error(
                        RuntimeErrorKind::UnexpectedReferenceArgument {
                            callable: func.name.clone(),
                            param_name: param.name.clone(),
                            arg_index: idx + 1,
                        },
                        span,
                    ));
                }
                self.registers[new_base + param.reg.index()] = val.clone();
                assigned_params[idx] = true;
            }
        }

        // Verify required parameters were provided
        for (idx, param) in func.params.iter().enumerate() {
            if !assigned_params[idx] && !param.has_default && !param.is_variadic {
                let min_req = func
                    .params
                    .iter()
                    .filter(|p| !p.has_default && !p.is_variadic)
                    .count();
                return Err(self.make_error(
                    RuntimeErrorKind::TooFewArguments {
                        callable: func.name.clone(),
                        passed: positional_values.len(),
                        expected: min_req,
                    },
                    span,
                ));
            }
        }

        // 3. Bind captured closure/arrow upvalues (`use ($var)`)
        if !upvalues.is_empty() {
            for (upval_name, upval_val) in func.upvalues.iter().zip(upvalues.iter()) {
                if upval_name == "$this" && has_explicit_receiver {
                    continue;
                }
                if let Some(reg) = func.chunk.register_for_var(upval_name) {
                    self.registers[new_base + reg.index()] = upval_val.clone();
                }
            }
        }

        self.frames.push(CallFrame {
            function_name: func.name,
            func_id: Some(target_id),
            ip: 0,
            base_reg: new_base,
            num_registers: num_regs,
            caller_dst,
            called_class,
            active_try_handlers: Vec::new(),
            iterators: HashMap::new(),
            ref_bindings,
        });

        Ok(())
    }

    /// Invoke a compiled function/closure synchronously with an explicit called_class.
    pub fn invoke_compiled_function_sync_with_called_class(
        &mut self,
        func_id: FunctionId,
        args: Vec<PhpValue>,
        called_class: Option<String>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let staged: Vec<StagedArg> = args
            .into_iter()
            .map(StagedArg::simple)
            .collect();
        let depth_before = self.frames.len();
        self.push_compiled_call_frame(func_id, None, &staged, &[], None, called_class, span)?;
        self.dispatch_loop(depth_before)
    }

    /// Invoke a compiled function/closure synchronously and return its evaluated `PhpValue`
    /// (used by higher-order collection helpers `map`, `filter`, `reduce`, `Future::wait`, and `defer`).
    pub fn invoke_compiled_function_sync(
        &mut self,
        func_id: FunctionId,
        args: Vec<PhpValue>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        self.invoke_compiled_function_sync_with_called_class(func_id, args, None, span)
    }

    /// Invoke a compiled function or static method by name synchronously on this VM.
    pub fn invoke_function_by_name(
        &mut self,
        name: &str,
        args: Vec<PhpValue>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let func_id = self.module.function_index.get(name).copied().ok_or_else(|| {
            self.make_error(
                RuntimeErrorKind::UndefinedFunction {
                    name: name.to_string(),
                },
                span,
            )
        })?;
        self.invoke_compiled_function_sync(func_id, args, span)
    }


    /// Invoke a static method on a class synchronously with zero reflection.
    pub fn invoke_static_method_sync(
        &mut self,
        class_name: &str,
        method: &str,
        args: Vec<PhpValue>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
        if let Some(target_id) = self.resolve_method_id(clean, method) {
            let staged: Vec<StagedArg> = args
                .into_iter()
                .map(StagedArg::simple)
                .collect();

            let depth_before = self.frames.len();
            self.push_compiled_call_frame(target_id, None, &staged, &[], None, Some(clean.to_string()), span)?;
            return self.dispatch_loop(depth_before);
        }

        // Forwarder check:
        let fwd_mid = self.module.get_class_info_by_name(clean)
            .and_then(|ci| ci.static_forward)
            .or_else(|| self.resolve_method_id(clean, "__forward"));

        if let Some(fwd_id) = fwd_mid {
            let recv = self.invoke_compiled_function_sync_with_called_class(fwd_id, vec![], Some(clean.to_string()), span)?;
            if let PhpValue::Object(ref rc) = recv {
                rc.borrow_mut().forwarded_from = Some(clean.to_string());
                let recv_cls = rc.borrow().class_name.clone();
                if let Some(target_mid) = self.resolve_method_id(&recv_cls, method) {
                    let staged: Vec<StagedArg> = args.into_iter().map(StagedArg::simple).collect();
                    let depth_before = self.frames.len();
                    self.push_compiled_call_frame(target_mid, Some(recv), &staged, &[], None, Some(recv_cls), span)?;
                    return self.dispatch_loop(depth_before);
                } else {
                    return Err(self.make_error(
                        RuntimeErrorKind::UndefinedForwardedMethod {
                            class: recv_cls,
                            method: method.to_string(),
                            origin_class: clean.to_string(),
                        },
                        span,
                    ));
                }
            }
        }

        // __callStatic check:
        if let Some(call_static_mid) = self.resolve_method_id(clean, "__callStatic") {
            let call_static_args = vec![
                StagedArg::simple(PhpValue::string(method.to_string())),
                StagedArg::simple(PhpValue::list(args)),
            ];
            let depth_before = self.frames.len();
            self.push_compiled_call_frame(call_static_mid, None, &call_static_args, &[], None, Some(clean.to_string()), span)?;
            return self.dispatch_loop(depth_before);
        }

        Err(self.make_error(
            RuntimeErrorKind::UndefinedMethod {
                class: clean.to_string(),
                method: method.to_string(),
            },
            span,
        ))
    }

    /// Instantiate an object and invoke its `__construct` method if defined.
    pub fn instantiate_and_construct(
        &mut self,
        class_name: &str,
        args: Vec<PhpValue>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
        let instance = self.instantiate_class_raw(clean, span, true)?;

        if let Some(ctor_id) = self.resolve_method_id(clean, "__construct") {
            let staged: Vec<StagedArg> = args
                .into_iter()
                .map(StagedArg::simple)
                .collect();
            let depth_before = self.frames.len();
            self.push_compiled_call_frame(ctor_id, Some(instance.clone()), &staged, &[], None, Some(clean.to_string()), span)?;
            self.dispatch_loop(depth_before)?;
        }

        Ok(instance)
    }

    /// Invoke a [`VmClosure`] synchronously and return its evaluated `PhpValue`.
    pub fn invoke_closure_sync(
        &mut self,
        closure: &VmClosure,
        args: Vec<PhpValue>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let staged: Vec<StagedArg> = args
            .into_iter()
            .map(StagedArg::simple)
            .collect();
        let depth_before = self.frames.len();
        self.push_compiled_call_frame(
            closure.func_id,
            None,
            &staged,
            &closure.upvalues,
            None,
            None,
            span,
        )?;
        self.dispatch_loop(depth_before)
    }

    /// Invoke a [`VmClosure`] synchronously with `$this` bound to `this_val` and return its evaluated `PhpValue`.
    pub fn invoke_closure_sync_with_this(
        &mut self,
        closure: &VmClosure,
        this_val: Option<PhpValue>,
        args: Vec<PhpValue>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let staged: Vec<StagedArg> = args
            .into_iter()
            .map(StagedArg::simple)
            .collect();
        let called_cls = this_val.as_ref().and_then(|t| {
            if let PhpValue::Object(o) = t {
                Some(o.borrow().class_name.clone())
            } else {
                None
            }
        });
        let depth_before = self.frames.len();
        self.push_compiled_call_frame(
            closure.func_id,
            this_val,
            &staged,
            &closure.upvalues,
            None,
            called_cls,
            span,
        )?;
        self.dispatch_loop(depth_before)
    }

    /// Invoke any callable [`PhpValue`] (Closure, invokable Object, or function name string)
    /// synchronously and return its evaluated `PhpValue`.
    pub fn invoke_callable_sync(
        &mut self,
        callable: &PhpValue,
        args: Vec<PhpValue>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        match callable {
            PhpValue::Closure(closure) => self.invoke_closure_sync(closure, args, span),
            PhpValue::Object(obj_rc) => {
                let cls_name = obj_rc.borrow().class_name.clone();
                let staged: Vec<StagedArg> = args
                    .into_iter()
                    .map(StagedArg::simple)
                    .collect();
                if let Some(target_id) = self.resolve_method_id(&cls_name, "__invoke") {
                    let depth_before = self.frames.len();
                    self.push_compiled_call_frame(
                        target_id,
                        Some(callable.clone()),
                        &staged,
                        &[],
                        None,
                        Some(cls_name),
                        span,
                    )?;
                    self.dispatch_loop(depth_before)
                } else {
                    Err(self.make_error(
                        RuntimeErrorKind::NotCallable {
                            type_name: cls_name,
                        },
                        span,
                    ))
                }
            }
            PhpValue::String(fn_name) => {
                let staged: Vec<StagedArg> = args
                    .into_iter()
                    .map(StagedArg::simple)
                    .collect();
                let target_id = self
                    .module
                    .function_index
                    .get(fn_name.as_str())
                    .copied()
                    .ok_or_else(|| {
                        self.make_error(
                            RuntimeErrorKind::UndefinedFunction {
                                name: (*fn_name).to_string(),
                            },
                            span,
                        )
                    })?;
                let depth_before = self.frames.len();
                self.push_compiled_call_frame(target_id, None, &staged, &[], None, None, span)?;
                self.dispatch_loop(depth_before)
            }
            PhpValue::Array(arr) | PhpValue::List(arr) => {
                let items = arr.borrow();
                if items.len() >= 2 {
                    let method_name = items[1].to_php_string();
                    match &items[0] {
                        PhpValue::Object(o) => {
                            let obj = PhpValue::Object(o.clone());
                            drop(items);
                            return self.invoke_method_sync(obj, &method_name, args, span);
                        }
                        PhpValue::String(cls) => {
                            let cls_str = cls.to_string();
                            drop(items);
                            return self.invoke_static_method_sync(&cls_str, &method_name, args, span);
                        }
                        _ => {}
                    }
                }
                Err(self.make_error(
                    RuntimeErrorKind::NotCallable {
                        type_name: self.runtime_type_label(callable),
                    },
                    span,
                ))
            }
            other => Err(self.make_error(
                RuntimeErrorKind::NotCallable {
                    type_name: self.runtime_type_label(other),
                },
                span,
            )),
        }
    }

    /// Invoke an instance or built-in object method synchronously on this VM.
    pub fn invoke_method_sync(
        &mut self,
        receiver: PhpValue,
        method_name: &str,
        args: Vec<PhpValue>,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let flat_args = args.clone();
        let staged: Vec<StagedArg> = args
            .into_iter()
            .map(StagedArg::simple)
            .collect();

        match &receiver {
            PhpValue::Object(obj_rc) => {
                let cls_name = obj_rc.borrow().class_name.clone();

                if let Some(target_id) = self.resolve_method_id(&cls_name, method_name) {
                    let depth_before = self.frames.len();
                    self.push_compiled_call_frame(
                        target_id,
                        Some(receiver),
                        &staged,
                        &[],
                        None,
                        Some(cls_name),
                        span,
                    )?;
                    self.dispatch_loop(depth_before)
                } else {
                    self.exec_builtin_object_method(
                        &cls_name,
                        &receiver,
                        method_name,
                        &flat_args,
                        span,
                    )
                }
            }
            _ => self.exec_fluent_value_method(&receiver, method_name, &flat_args, span),
        }
    }

    fn build_active_iterator(
        &mut self,
        iter_source: &PhpValue,
        span: Span,
    ) -> Result<ActiveIterator, RuntimeError> {
        self.build_active_iterator_with_depth(iter_source, 0, span)
    }

    fn build_active_iterator_with_depth(
        &mut self,
        iter_source: &PhpValue,
        depth: usize,
        span: Span,
    ) -> Result<ActiveIterator, RuntimeError> {
        if depth > 20 {
            return Err(self.make_error(
                RuntimeErrorKind::NotIterable {
                    type_name: iter_source.type_name().to_string(),
                },
                span,
            ));
        }

        match iter_source {
            PhpValue::List(list) => Ok(ActiveIterator::List {
                items: list.clone(),
                cursor: 0,
            }),
            PhpValue::EmptyArray => Ok(ActiveIterator::List {
                items: Rc::new(RefCell::new(Vec::new())),
                cursor: 0,
            }),
            PhpValue::Array(arr) => Ok(ActiveIterator::List {
                items: arr.clone(),
                cursor: 0,
            }),
            PhpValue::Set(set) => {
                let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                Ok(ActiveIterator::Set { items, cursor: 0 })
            }
            PhpValue::Dict(dict) => Ok(ActiveIterator::Dict {
                dict: dict.clone(),
                cursor: 0,
            }),
            PhpValue::HashMap(map) => {
                let entries: Vec<(PhpValue, PhpValue)> = map
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Ok(ActiveIterator::Entries { entries, cursor: 0 })
            }
            PhpValue::Object(obj_rc) => {
                let cls_name = obj_rc.borrow().class_name.clone();

                // 1. ArrayIterator built-in: unwrap internal items directly
                if cls_name == "ArrayIterator" {
                    if let Some(items_val) = obj_rc.borrow().get_property("items").cloned() {
                        return self.build_active_iterator_with_depth(&items_val, depth + 1, span);
                    }
                }

                // 2. IteratorAggregate or objects implementing getIterator()
                let has_get_iterator = self.resolve_method_id(&cls_name, "getIterator").is_some()
                    || self.is_class_instance_of(&cls_name, "IteratorAggregate")
                    || cls_name == "LengthAwarePaginator"
                    || cls_name == "Paginator"
                    || cls_name.ends_with("Collection");

                if has_get_iterator {
                    let iterator = self.invoke_method_sync(
                        iter_source.clone(),
                        "getIterator",
                        vec![],
                        span,
                    )?;
                    return self.build_active_iterator_with_depth(&iterator, depth + 1, span);
                }

                // 3. Iterator interface (rewind, valid, current, key, next)
                let has_iterator_methods = (self.resolve_method_id(&cls_name, "valid").is_some()
                    && self.resolve_method_id(&cls_name, "current").is_some()
                    && self.resolve_method_id(&cls_name, "next").is_some())
                    || self.is_class_instance_of(&cls_name, "Iterator");

                if has_iterator_methods {
                    return Ok(ActiveIterator::IteratorObject {
                        iterator: iter_source.clone(),
                        started: false,
                    });
                }

                // 4. Plain object: iterate public/accessible properties
                let obj = obj_rc.borrow();
                let mut entries = Vec::new();
                let shape = obj.shape.borrow();
                let mut ordered_props: Vec<(usize, String)> = shape
                    .property_indices
                    .iter()
                    .map(|(k, &idx)| (idx, k.clone()))
                    .collect();
                ordered_props.sort_by_key(|(idx, _)| *idx);
                let mut seen = std::collections::HashSet::new();
                for (_, prop_name) in ordered_props {
                    if let Some(val) = obj.get_property(&prop_name) {
                        entries.push((PhpValue::string(prop_name.clone()), val.clone()));
                        seen.insert(prop_name);
                    }
                }
                for (prop_name, val) in &obj.properties {
                    if !seen.contains(prop_name) {
                        entries.push((PhpValue::string(prop_name.clone()), val.clone()));
                    }
                }
                Ok(ActiveIterator::Entries { entries, cursor: 0 })
            }
            other => Err(self.make_error(
                RuntimeErrorKind::NotIterable {
                    type_name: other.type_name().to_string(),
                },
                span,
            )),
        }
    }

    /// Spawn a background task executing a [`VmClosure`] concurrently on an OS thread pool,
    /// returning a channel receiver for the task's evaluated result.
    pub fn spawn_closure_future(
        &self,
        closure: &VmClosure,
    ) -> std::sync::mpsc::Receiver<Result<FrozenValue, String>> {
        let (tx, rx) = std::sync::mpsc::channel();
        let module = self.module.clone();
        let func_id = closure.func_id;
        let name = closure.name.clone();
        let is_static = closure.is_static;
        let category = closure.category;
        let frozen_upvalues: Vec<FrozenValue> = closure
            .upvalues
            .iter()
            .map(FrozenValue::from_php_value)
            .collect();
        let static_props: HashMap<(String, String), FrozenValue> = self
            .static_properties
            .iter()
            .map(|(k, v)| (k.clone(), FrozenValue::from_php_value(v)))
            .collect();
        let class_consts: HashMap<(String, String), FrozenValue> = self
            .class_constants
            .iter()
            .map(|(k, v)| (k.clone(), FrozenValue::from_php_value(v)))
            .collect();
        let hierarchy = self.type_hierarchy.clone();

        std::thread::Builder::new()
            .name(format!("rhp-future-{}", name))
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut child_vm = VM::new(module, false);
                    for ((cls, prop), val) in static_props {
                        child_vm.static_properties.insert((cls, prop), val.to_php_value());
                    }
                    for ((cls, cst), val) in class_consts {
                        child_vm.class_constants.insert((cls, cst), val.to_php_value());
                    }
                    if let Some(h) = hierarchy {
                        child_vm = child_vm.with_type_hierarchy(h);
                    }
                    let upvalues: Vec<PhpValue> = frozen_upvalues
                        .into_iter()
                        .map(|f| f.to_php_value())
                        .collect();
                    let vm_closure = VmClosure {
                        func_id,
                        name,
                        upvalues,
                        is_static,
                        category,
                    };
                    child_vm.invoke_closure_sync(&vm_closure, Vec::new(), Span::default())
                }));

                match outcome {
                    Ok(Ok(val)) => {
                        let _ = tx.send(Ok(FrozenValue::from_php_value(&val)));
                    }
                    Ok(Err(err)) => {
                        let _ = tx.send(Err(err.to_string()));
                    }
                    Err(panic_payload) => {
                        let msg = crate::vm::worker::extract_panic_message(&panic_payload);
                        let _ = tx.send(Err(format!("Future background task panicked: {}", msg)));
                    }
                }
            })
            .expect("Failed to spawn Future background worker thread");

        rx
    }

    // =========================================================================
    // Facade & Fluent Method Execution
    // =========================================================================

    fn exec_facade_call(
        &mut self,
        facade: &str,
        method: &str,
        args: &[PhpValue],
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        // 0. Boot Phase Invariant: Master process must stay strictly single-threaded before forking workers.
        // POSIX fork() duplicates ONLY the calling thread. If the parent process creates a Tokio runtime,
        // spawns background threads, or opens database pools prior to forking, child workers can deadlock
        // on inherited locks. Therefore, database connections and async threads are prohibited during boot.
        if self.is_parent_process
            && (facade == "DB"
                || facade == "ADB"
                || (facade == "Future" && (method == "run" || method == "delayed" || method == "wait" || method == "any")))
        {
            return Err(self.make_error(
                RuntimeErrorKind::Custom {
                    message: format!(
                        "BootPhaseViolation: Cannot invoke '{}::{}' during the master process boot phase. All boot-time execution must remain strictly single-threaded to prevent deadlocks across forked workers. Initialize database pools and async runtimes post-fork inside request handlers or worker lifecycle hooks.",
                        facade, method
                    ),
                },
                span,
            ));
        }

        if facade == "Glob" && method == "customHelper" {
            let helper_name = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let clean_helper = helper_name.strip_prefix('\\').unwrap_or(&helper_name);
            if clean_helper == "app" {
                if let Some(arg) = args.get(1) {
                    let abs = arg.to_php_string();
                    let mut container = std::mem::take(&mut self.container);
                    let res = container.resolve(&abs, self);
                    self.container = container;
                    return res.map_err(|e| self.make_error(RuntimeErrorKind::Custom { message: e.to_string() }, span));
                } else {
                    let app_obj = VmObject::new("Application");
                    return Ok(PhpValue::object(app_obj));
                }
            } else if clean_helper.eq_ignore_ascii_case("ini_get") {
                let key = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
                let clean_key = key.trim_matches('"').trim_matches('\'');
                if clean_key == "zend.exception_ignore_args" || key == "zend.exception_ignore_args" {
                    return Ok(PhpValue::string(if self.exception_ignore_args { "1" } else { "0" }));
                } else if clean_key == "max_loop_iterations" || key == "max_loop_iterations" {
                    return Ok(PhpValue::string(if self.exempt_loop_limits { "0".to_string() } else { self.dev_loop_hard_limit.to_string() }));
                } else if clean_key == "warn_loop_iterations" || key == "warn_loop_iterations" {
                    return Ok(PhpValue::string(self.dev_loop_warn_threshold.to_string()));
                } else if clean_key == "instruction_budget" || key == "instruction_budget" {
                    return Ok(PhpValue::string(self.tick_budget.to_string()));
                } else {
                    return Ok(PhpValue::string(""));
                }
            } else if clean_helper.eq_ignore_ascii_case("ini_set") {
                let key = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
                let val = args.get(2).map(|v| v.to_php_string()).unwrap_or_default();
                let clean_key = key.trim_matches('"').trim_matches('\'');
                let clean_val = val.trim_matches('"').trim_matches('\'');
                if clean_key == "zend.exception_ignore_args" || key == "zend.exception_ignore_args" {
                    let old = if self.exception_ignore_args { "1" } else { "0" };
                    self.exception_ignore_args =
                        clean_val == "1" || clean_val.eq_ignore_ascii_case("true") || clean_val.eq_ignore_ascii_case("on")
                        || val == "1" || val.eq_ignore_ascii_case("true") || val.eq_ignore_ascii_case("on");
                    return Ok(PhpValue::string(old));
                } else if clean_key == "max_loop_iterations" || key == "max_loop_iterations" {
                    let old = if self.exempt_loop_limits { "0".to_string() } else { self.dev_loop_hard_limit.to_string() };
                    if let Ok(parsed) = clean_val.parse::<u64>() {
                        if parsed == 0 {
                            self.exempt_loop_limits = true;
                        } else {
                            self.exempt_loop_limits = false;
                            self.dev_loop_hard_limit = parsed;
                        }
                    }
                    return Ok(PhpValue::string(old));
                } else if clean_key == "warn_loop_iterations" || key == "warn_loop_iterations" {
                    let old = self.dev_loop_warn_threshold.to_string();
                    if let Ok(parsed) = clean_val.parse::<u64>() {
                        self.dev_loop_warn_threshold = parsed;
                    }
                    return Ok(PhpValue::string(old));
                } else if clean_key == "instruction_budget" || key == "instruction_budget" {
                    let old = self.tick_budget.to_string();
                    if let Ok(parsed) = clean_val.parse::<u32>() {
                        self.tick_budget = parsed.max(1);
                        self.tick_counter = self.tick_budget;
                    }
                    return Ok(PhpValue::string(old));
                } else {
                    return Ok(PhpValue::string(""));
                }
            }
        }

        // 1. Dynamic Macro definition: `Facade::macro('name', fn(...) => ...)`
        if method == "macro" {
            if let (Some(name_val), Some(PhpValue::Closure(cb))) = (args.first(), args.get(1)) {
                let name_str = name_val.to_php_string();
                register_runtime_closure_macro(facade, &name_str, (**cb).clone());
                return Ok(PhpValue::Bool(true));
            }
        }

        // 2. Dynamic Macro invocation across Tier 1, Tier 2, and Tier 3 facades
        if let Some(macro_closure) = get_runtime_closure_macro(facade, method) {
            return self.invoke_closure_sync(&macro_closure, args.to_vec(), span);
        }

        // Intercept higher-order `Future::*` and `Arr::*` facade calls that invoke closures
        if facade == "Future" {
            match method {
                "run" => {
                    match args.first() {
                        Some(PhpValue::Closure(c)) => {
                            let rx = self.spawn_closure_future(c);
                            return Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::pending(rx)))));
                        }
                        Some(PhpValue::Future(f)) => {
                            return Ok(PhpValue::Future(f.clone()));
                        }
                        Some(other) => {
                            return Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::resolved(
                                other.clone(),
                            )))));
                        }
                        None => {
                            return Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::resolved(
                                PhpValue::Null,
                            )))));
                        }
                    }
                }
                "delayed" => {
                    let ms = args.first().and_then(|v| v.to_i64()).unwrap_or(0).max(0) as u64;
                    crate::vm::fiber::record_fiber_io_start(crate::vm::fiber::IoKind::Timer { milliseconds: ms });
                    crate::vm::fiber::record_fiber_yield();
                    crate::vm::fiber::record_fiber_io_finish();

                    match args.get(1) {
                        Some(PhpValue::Closure(c)) => {
                            if ms > 0 {
                                let (tx, rx) = std::sync::mpsc::channel();
                                let module = self.module.clone();
                                let func_id = c.func_id;
                                let name = c.name.clone();
                                let is_static = c.is_static;
                                let category = c.category;
                                let frozen_upvalues: Vec<FrozenValue> = c
                                    .upvalues
                                    .iter()
                                    .map(FrozenValue::from_php_value)
                                    .collect();
                                let static_props: HashMap<(String, String), FrozenValue> = self
                                    .static_properties
                                    .iter()
                                    .map(|(k, v)| (k.clone(), FrozenValue::from_php_value(v)))
                                    .collect();
                                let class_consts: HashMap<(String, String), FrozenValue> = self
                                    .class_constants
                                    .iter()
                                    .map(|(k, v)| (k.clone(), FrozenValue::from_php_value(v)))
                                    .collect();
                                let hierarchy = self.type_hierarchy.clone();

                                std::thread::Builder::new()
                                    .name(format!("rhp-delayed-{}", name))
                                    .spawn(move || {
                                        std::thread::sleep(std::time::Duration::from_millis(ms));
                                        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                            let mut child_vm = VM::new(module, false);
                                            for ((cls, prop), val) in static_props {
                                                child_vm.static_properties.insert((cls, prop), val.to_php_value());
                                            }
                                            for ((cls, cst), val) in class_consts {
                                                child_vm.class_constants.insert((cls, cst), val.to_php_value());
                                            }
                                            if let Some(h) = hierarchy {
                                                child_vm = child_vm.with_type_hierarchy(h);
                                            }
                                            let upvalues: Vec<PhpValue> = frozen_upvalues
                                                .into_iter()
                                                .map(|f| f.to_php_value())
                                                .collect();
                                            let vm_closure = VmClosure {
                                                func_id,
                                                name,
                                                upvalues,
                                                is_static,
                                                category,
                                            };
                                            child_vm.invoke_closure_sync(&vm_closure, Vec::new(), Span::default())
                                        }));

                                        match outcome {
                                            Ok(Ok(val)) => {
                                                let _ = tx.send(Ok(FrozenValue::from_php_value(&val)));
                                            }
                                            Ok(Err(err)) => {
                                                let _ = tx.send(Err(err.to_string()));
                                            }
                                            Err(panic_payload) => {
                                                let msg = crate::vm::worker::extract_panic_message(&panic_payload);
                                                let _ = tx.send(Err(format!("Future delayed task panicked: {}", msg)));
                                            }
                                        }
                                    })
                                    .expect("Failed to spawn Future delayed worker thread");

                                return Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::pending(rx)))));
                            } else {
                                let resolved = self.invoke_closure_sync(c, Vec::new(), span)?;
                                return Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::resolved(
                                    resolved,
                                )))));
                            }
                        }
                        Some(other) => {
                            if ms > 0 {
                                let (tx, rx) = std::sync::mpsc::channel();
                                let frozen = FrozenValue::from_php_value(other);
                                std::thread::spawn(move || {
                                    std::thread::sleep(std::time::Duration::from_millis(ms));
                                    let _ = tx.send(Ok(frozen));
                                });
                                return Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::pending(rx)))));
                            } else {
                                return Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::resolved(
                                    other.clone(),
                                )))));
                            }
                        }
                        None => {
                            if ms > 0 {
                                let (tx, rx) = std::sync::mpsc::channel();
                                std::thread::spawn(move || {
                                    std::thread::sleep(std::time::Duration::from_millis(ms));
                                    let _ = tx.send(Ok(FrozenValue::Null));
                                });
                                return Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::pending(rx)))));
                            } else {
                                return Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::resolved(
                                    PhpValue::Null,
                                )))));
                            }
                        }
                    }
                }
                "wait" | "all" => {
                    let mut handles: Vec<Rc<RefCell<VmFuture>>> = Vec::new();
                    if let Some(first) = args.first() {
                        for task in Self::extract_items(first) {
                            match task {
                                PhpValue::Closure(c) => {
                                    let rx = self.spawn_closure_future(&c);
                                    handles.push(Rc::new(RefCell::new(VmFuture::pending(rx))));
                                }
                                PhpValue::Future(f) => {
                                    handles.push(f.clone());
                                }
                                other => {
                                    handles.push(Rc::new(RefCell::new(VmFuture::resolved(other))));
                                }
                            }
                        }
                    }

                    let mut out = Vec::with_capacity(handles.len());
                    for handle in handles {
                        let mut b = handle.borrow_mut();
                        match b.wait_result() {
                            Ok(val) => out.push(val),
                            Err(err) => {
                                return Err(self.make_error(
                                    RuntimeErrorKind::Custom { message: err },
                                    span,
                                ));
                            }
                        }
                    }
                    return Ok(PhpValue::list(out));
                }
                "any" => {
                    let items = if let Some(first) = args.first() {
                        Self::extract_items(first)
                    } else {
                        Vec::new()
                    };

                    if items.is_empty() {
                        return Ok(PhpValue::Null);
                    }

                    let (tx, rx) = std::sync::mpsc::channel();
                    let mut immediate_result: Option<PhpValue> = None;

                    for (idx, task) in items.into_iter().enumerate() {
                        match task {
                            PhpValue::Closure(c) => {
                                let tx_clone = tx.clone();
                                let module = self.module.clone();
                                let func_id = c.func_id;
                                let name = c.name.clone();
                                let is_static = c.is_static;
                                let category = c.category;
                                let frozen_upvalues: Vec<FrozenValue> = c
                                    .upvalues
                                    .iter()
                                    .map(FrozenValue::from_php_value)
                                    .collect();
                                let static_props: HashMap<(String, String), FrozenValue> = self
                                    .static_properties
                                    .iter()
                                    .map(|(k, v)| (k.clone(), FrozenValue::from_php_value(v)))
                                    .collect();
                                let class_consts: HashMap<(String, String), FrozenValue> = self
                                    .class_constants
                                    .iter()
                                    .map(|(k, v)| (k.clone(), FrozenValue::from_php_value(v)))
                                    .collect();
                                let hierarchy = self.type_hierarchy.clone();

                                std::thread::Builder::new()
                                    .name(format!("rhp-any-race-{}", idx))
                                    .spawn(move || {
                                        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                            let mut child_vm = VM::new(module, false);
                                            for ((cls, prop), val) in static_props {
                                                child_vm.static_properties.insert((cls, prop), val.to_php_value());
                                            }
                                            for ((cls, cst), val) in class_consts {
                                                child_vm.class_constants.insert((cls, cst), val.to_php_value());
                                            }
                                            if let Some(h) = hierarchy {
                                                child_vm = child_vm.with_type_hierarchy(h);
                                            }
                                            let upvalues: Vec<PhpValue> = frozen_upvalues
                                                .into_iter()
                                                .map(|f| f.to_php_value())
                                                .collect();
                                            let vm_closure = VmClosure {
                                                func_id,
                                                name,
                                                upvalues,
                                                is_static,
                                                category,
                                            };
                                            child_vm.invoke_closure_sync(&vm_closure, Vec::new(), Span::default())
                                        }));

                                        let res = match outcome {
                                            Ok(Ok(val)) => Ok(FrozenValue::from_php_value(&val)),
                                            Ok(Err(e)) => Err(e.to_string()),
                                            Err(p) => Err(crate::vm::worker::extract_panic_message(&p)),
                                        };
                                        let _ = tx_clone.send((idx, res));
                                    })
                                    .expect("Failed to spawn race thread");
                            }
                            PhpValue::Future(f) => {
                                let rx_opt = {
                                    let mut b = f.borrow_mut();
                                    b.poll_completion();
                                    if b.is_completed && b.receiver.is_none() {
                                        if immediate_result.is_none() {
                                            if let Some(ref err) = b.error {
                                                return Err(self.make_error(
                                                    RuntimeErrorKind::Custom { message: err.clone() },
                                                    span,
                                                ));
                                            }
                                            immediate_result = Some(b.value.clone().unwrap_or(PhpValue::Null));
                                        }
                                        None
                                    } else {
                                        b.receiver.clone()
                                    }
                                };

                                if let Some(rx_arc) = rx_opt {
                                    let tx_clone = tx.clone();
                                    std::thread::Builder::new()
                                        .name(format!("rhp-any-wait-{}", idx))
                                        .spawn(move || {
                                            let res = if let Ok(rx) = rx_arc.lock() {
                                                match rx.recv() {
                                                    Ok(frozen_res) => frozen_res,
                                                    Err(_) => Err("Background future thread disconnected".to_string()),
                                                }
                                            } else {
                                                Err("Future mutex poisoned".to_string())
                                            };
                                            let _ = tx_clone.send((idx, res));
                                        })
                                        .expect("Failed to spawn race waiter thread");
                                }
                            }
                            other => {
                                if immediate_result.is_none() {
                                    immediate_result = Some(other);
                                }
                            }
                        }
                    }
                    drop(tx);

                    if let Some(res) = immediate_result {
                        return Ok(res);
                    }

                    let (_winner_idx, outcome) = match rx.recv() {
                        Ok(first) => {
                            let mut best = first;
                            // Settle window: if another task finishes in the same microsecond quantum,
                            // tie-break by submission order (lower index wins).
                            std::thread::yield_now();
                            std::thread::sleep(std::time::Duration::from_millis(1));
                            while let Ok(candidate) = rx.try_recv() {
                                if candidate.0 < best.0 {
                                    best = candidate;
                                }
                            }
                            best
                        }
                        Err(_) => return Ok(PhpValue::Null),
                    };

                    return match outcome {
                        Ok(frozen) => Ok(frozen.to_php_value()),
                        Err(err) => Err(self.make_error(
                            RuntimeErrorKind::Custom { message: err },
                            span,
                        )),
                    };
                }
                "value" => {
                    let val = args.first().cloned().unwrap_or(PhpValue::Null);
                    return Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::resolved(
                        val,
                    )))));
                }
                "error" => {
                    let msg = args
                        .first()
                        .map(|v| v.to_php_string())
                        .unwrap_or_else(|| "Future rejected".to_string());
                    return Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::rejected(
                        msg,
                    )))));
                }
                "defer" => {
                    if let Some(PhpValue::Closure(c)) = args.first() {
                        self.defer_queue.push(c.func_id);
                    }
                    return Ok(PhpValue::Null);
                }
                _ => {}
            }
        }

        if facade == "Memory" {
            match method {
                "scope" => {
                    if let Some(PhpValue::Closure(cb)) = args.first() {
                        let marker = self.arena.mark();
                        let _scope = crate::vm::memory::MemoryScope::new();
                        let res = self.invoke_closure_sync(cb, Vec::new(), span)?;
                        self.arena.reset_to_marker(marker);
                        return Ok(res);
                    }
                }
                "arenaStats" => {
                    return Ok(self.arena.stats().to_dict());
                }
                "resetArena" => {
                    let stats = self.arena.drop_request();
                    return Ok(PhpValue::Int(stats.bytes_reclaimed as i64));
                }
                "assertThreadSafe" => {
                    if let Some(val) = args.first() {
                        crate::vm::memory::OwnershipEnforcer::assert_thread_safe(val, span)?;
                        return Ok(PhpValue::Bool(true));
                    }
                    return Ok(PhpValue::Bool(true));
                }
                _ => {}
            }
        }

        if (facade == "Trace" || facade == "Telemetry") && method == "span" {
            if let Some(name_val) = args.first() {
                let name = name_val.to_php_string();
                if let Some(PhpValue::Closure(cb)) = args.get(1) {
                    let mut attrs = indexmap::IndexMap::new();
                    if let Some(PhpValue::Dict(d)) = args.get(2) {
                        for (k, v) in d.borrow().iter() {
                            attrs.insert(k.clone(), crate::telemetry::AttributeValue::from_php_value(v));
                        }
                    }
                    crate::telemetry::start_span(&name, crate::telemetry::SpanKind::Internal, None, attrs);
                    let res = self.invoke_closure_sync(cb, Vec::new(), span);
                    match res {
                        Ok(val) => {
                            crate::telemetry::end_active_span(Some(crate::telemetry::SpanStatus::ok()));
                            return Ok(val);
                        }
                        Err(err) => {
                            crate::telemetry::record_active_span_error(&err.to_string());
                            crate::telemetry::end_active_span(Some(crate::telemetry::SpanStatus::error(err.to_string())));
                            return Err(err);
                        }
                    }
                }
            }
        }

        if facade == "Sys" {
            match method {
                "echo" => {
                    let text = crate::vm::format::format_sys_args(args);
                    self.output_buffer.push_str(&text);
                    return Ok(PhpValue::Null);
                }
                "print" => {
                    let mut text = crate::vm::format::format_sys_args(args);
                    text.push('\n');
                    self.output_buffer.push_str(&text);
                    return Ok(PhpValue::Null);
                }
                "print_r" => {
                    if let Some(arg) = args.first() {
                        let return_output = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
                        let formatted = arg.to_print_r();
                        if return_output {
                            return Ok(PhpValue::string(formatted));
                        } else {
                            self.output_buffer.push_str(&formatted);
                            self.output_buffer.push('\n');
                            return Ok(PhpValue::Bool(true));
                        }
                    }
                    return Ok(PhpValue::Null);
                }
                "dump" | "dd" => {
                    for arg in args {
                        self.output_buffer.push_str(&arg.to_print_r());
                        self.output_buffer.push('\n');
                    }
                    return Ok(PhpValue::Null);
                }
                "exit" => {
                    let mut exit_code = 0;
                    if let Some(arg) = args.first() {
                        match arg {
                            PhpValue::String(s) => {
                                self.output_buffer.push_str(s);
                                exit_code = 0;
                            }
                            PhpValue::Int(i) => exit_code = *i,
                            _ => exit_code = 0,
                        }
                    }
                    return Err(self.make_error(RuntimeErrorKind::Exit((exit_code) as i32), span));
                }
                "die" => {
                    if !self.output_buffer.is_empty() {
                        print!("{}", self.output_buffer);
                        self.output_buffer.clear();
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                    }
                    if let Some(ref hook) = self.exit_hook {
                        let mut exit_code = 0;
                        if let Some(arg) = args.first() {
                            match arg {
                                PhpValue::String(s) => {
                                    print!("{}", s.as_str());
                                    use std::io::Write;
                                    let _ = std::io::stdout().flush();
                                    exit_code = 0;
                                }
                                PhpValue::Int(i) => exit_code = *i,
                                _ => exit_code = 0,
                            }
                        }
                        hook((exit_code) as i32);
                        return Err(self.make_error(RuntimeErrorKind::Die((exit_code) as i32), span));
                    }
                    return crate::vm::facade::execute_process_exit(args)
                        .map_err(|e| self.make_error(e, span));
                }
                "registerShutdownFunction" | "register_shutdown_function" => {
                    if let Some(callback) = args.first() {
                        let cb_args = if args.len() > 1 {
                            args[1..].to_vec()
                        } else {
                            Vec::new()
                        };
                        self.register_shutdown_function(callback.clone(), cb_args);
                    }
                    return Ok(PhpValue::Null);
                }
                "setErrorHandler" | "set_error_handler" => {
                    let callback = args.first().cloned().unwrap_or(PhpValue::Null);
                    let error_levels = args.get(1).and_then(|v| match v {
                        PhpValue::Int(i) => Some(*i as i32),
                        _ => None,
                    });
                    let prev = self.set_error_handler(callback, error_levels);
                    return Ok(prev);
                }
                "restoreErrorHandler" | "restore_error_handler" => {
                    let res = self.restore_error_handler();
                    return Ok(PhpValue::Bool(res));
                }
                "setExceptionHandler" | "set_exception_handler" => {
                    let callback = args.first().cloned().unwrap_or(PhpValue::Null);
                    let prev = self.set_exception_handler(callback);
                    return Ok(prev);
                }
                "restoreExceptionHandler" | "restore_exception_handler" => {
                    let res = self.restore_exception_handler();
                    return Ok(PhpValue::Bool(res));
                }
                "triggerError" | "trigger_error" => {
                    let msg = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let level = args.get(1).and_then(|v| match v {
                        PhpValue::Int(i) => Some(*i as i32),
                        _ => None,
                    });
                    let res = self.trigger_error(&msg, level, span)?;
                    return Ok(PhpValue::Bool(res));
                }
                _ => {}
            }
        }

        if facade == "phpFun" {
            match method {
                "exit" => {
                    let mut exit_code = 0;
                    if let Some(arg) = args.first() {
                        match arg {
                            PhpValue::String(s) => {
                                self.output_buffer.push_str(s);
                                exit_code = 0;
                            }
                            PhpValue::Int(i) => exit_code = *i,
                            _ => exit_code = 0,
                        }
                    }
                    return Err(self.make_error(RuntimeErrorKind::Exit((exit_code) as i32), span));
                }
                "die" => {
                    if !self.output_buffer.is_empty() {
                        print!("{}", self.output_buffer);
                        self.output_buffer.clear();
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                    }
                    if let Some(ref hook) = self.exit_hook {
                        let mut exit_code = 0;
                        if let Some(arg) = args.first() {
                            match arg {
                                PhpValue::String(s) => {
                                    print!("{}", s.as_str());
                                    use std::io::Write;
                                    let _ = std::io::stdout().flush();
                                    exit_code = 0;
                                }
                                PhpValue::Int(i) => exit_code = *i,
                                _ => exit_code = 0,
                            }
                        }
                        hook((exit_code) as i32);
                        return Err(self.make_error(RuntimeErrorKind::Die((exit_code) as i32), span));
                    }
                    return crate::vm::facade::execute_process_exit(args)
                        .map_err(|e| self.make_error(e, span));
                }
                "register_shutdown_function" | "registerShutdownFunction" => {
                    if let Some(callback) = args.first() {
                        let cb_args = if args.len() > 1 {
                            args[1..].to_vec()
                        } else {
                            Vec::new()
                        };
                        self.register_shutdown_function(callback.clone(), cb_args);
                    }
                    return Ok(PhpValue::Null);
                }
                "set_error_handler" | "setErrorHandler" => {
                    let callback = args.first().cloned().unwrap_or(PhpValue::Null);
                    let error_levels = args.get(1).and_then(|v| match v {
                        PhpValue::Int(i) => Some(*i as i32),
                        _ => None,
                    });
                    let prev = self.set_error_handler(callback, error_levels);
                    return Ok(prev);
                }
                "restore_error_handler" | "restoreErrorHandler" => {
                    let res = self.restore_error_handler();
                    return Ok(PhpValue::Bool(res));
                }
                "set_exception_handler" | "setExceptionHandler" => {
                    let callback = args.first().cloned().unwrap_or(PhpValue::Null);
                    let prev = self.set_exception_handler(callback);
                    return Ok(prev);
                }
                "restore_exception_handler" | "restoreExceptionHandler" => {
                    let res = self.restore_exception_handler();
                    return Ok(PhpValue::Bool(res));
                }
                "trigger_error" | "triggerError" => {
                    let msg = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let level = args.get(1).and_then(|v| match v {
                        PhpValue::Int(i) => Some(*i as i32),
                        _ => None,
                    });
                    let res = self.trigger_error(&msg, level, span)?;
                    return Ok(PhpValue::Bool(res));
                }
                _ => {}
            }
        }

        if facade == "Arr" || facade == "phpFun" {
            match method {
                "macro" => {
                    if let (Some(name_val), Some(PhpValue::Closure(cb))) = (args.first(), args.get(1)) {
                        let name_str = name_val.to_php_string();
                        register_runtime_closure_macro("Arr", &name_str, (**cb).clone());
                        return Ok(PhpValue::Bool(true));
                    }
                }
                "map" | "array_map" => {
                    let (cb_val, arr_opt) = match (args.first(), args.get(1)) {
                        (Some(cb), Some(arr)) if matches!(cb, PhpValue::Closure(_) | PhpValue::String(_)) => (Some(cb), Some(arr)),
                        (Some(arr), Some(cb)) if matches!(cb, PhpValue::Closure(_) | PhpValue::String(_)) => (Some(cb), Some(arr)),
                        _ => (None, None),
                    };
                    if let (Some(cb), Some(arr)) = (cb_val, arr_opt) {
                        let items = Self::extract_items(arr);
                        let mut mapped = Vec::with_capacity(items.len());
                        match cb {
                            PhpValue::Closure(c) => {
                                for (idx, item) in items.into_iter().enumerate() {
                                    mapped.push(self.invoke_closure_sync(
                                        c,
                                        vec![item, PhpValue::Int(idx as i64)],
                                        span,
                                    )?);
                                }
                            }
                            PhpValue::String(fn_name) => {
                                let name = fn_name.as_str();
                                for (_idx, item) in items.into_iter().enumerate() {
                                    let res = dispatch_facade_call("phpFun", name, &[item.clone()], span)?;
                                    mapped.push(res);
                                }
                            }
                            _ => {}
                        }
                        return Ok(PhpValue::list(mapped));
                    }
                }
                "filter" | "array_filter" => {
                    let (arr_opt, cb_opt) = match (args.first(), args.get(1)) {
                        (Some(PhpValue::Closure(cb)), Some(arr)) => (Some(arr), Some(cb)),
                        (Some(arr), Some(PhpValue::Closure(cb))) => (Some(arr), Some(cb)),
                        (Some(arr), _) => (Some(arr), None),
                        _ => (None, None),
                    };
                    if let Some(arr) = arr_opt {
                        let items = Self::extract_items(arr);
                        let mut filtered = Vec::new();
                        if let Some(cb) = cb_opt {
                            for (idx, item) in items.into_iter().enumerate() {
                                let keep = self.invoke_closure_sync(
                                    cb,
                                    vec![item.clone(), PhpValue::Int(idx as i64)],
                                    span,
                                )?;
                                if keep.is_truthy() {
                                    filtered.push(item);
                                }
                            }
                        } else {
                            filtered.extend(items.into_iter().filter(|i| i.is_truthy()));
                        }
                        return Ok(PhpValue::list(filtered));
                    }
                }
                "reduce" | "array_reduce" => {
                    if let (Some(arr), Some(PhpValue::Closure(cb))) = (args.first(), args.get(1)) {
                        let items = Self::extract_items(arr);
                        let mut acc = args.get(2).cloned().unwrap_or(PhpValue::Null);
                        for item in items {
                            acc = self.invoke_closure_sync(cb, vec![acc, item], span)?;
                        }
                        return Ok(acc);
                    }
                }
                "usort" | "uasort" => {
                    let (arr_opt, cb_opt) = match (args.first(), args.get(1)) {
                        (Some(arr), Some(PhpValue::Closure(cb))) => (Some(arr), Some(cb)),
                        (Some(PhpValue::Closure(cb)), Some(arr)) => (Some(arr), Some(cb)),
                        (Some(arr), _) => (Some(arr), None),
                        _ => (None, None),
                    };
                    if let Some(arr) = arr_opt {
                        let cb_args = cb_opt.map(|cb| vec![PhpValue::Closure(cb.clone())]).unwrap_or_default();
                        return self.exec_fluent_value_method(arr, method, &cb_args, span);
                    }
                }
                "uksort" => {
                    let (arr_opt, cb_opt) = match (args.first(), args.get(1)) {
                        (Some(arr), Some(PhpValue::Closure(cb))) => (Some(arr), Some(cb)),
                        (Some(PhpValue::Closure(cb)), Some(arr)) => (Some(arr), Some(cb)),
                        (Some(arr), _) => (Some(arr), None),
                        _ => (None, None),
                    };
                    if let Some(arr) = arr_opt {
                        let cb_args = cb_opt.map(|cb| vec![PhpValue::Closure(cb.clone())]).unwrap_or_default();
                        return self.exec_fluent_value_method(arr, "uksort", &cb_args, span);
                    }
                }
                "asort" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "asort", &[], span);
                    }
                }
                "natsort" | "natSort" | "sortNatural" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "natsort", &[], span);
                    }
                }
                "natcasesort" | "natCaseSort" | "sortNaturalCase" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "natcasesort", &[], span);
                    }
                }
                "ksort" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "ksort", &[], span);
                    }
                }
                "diff" | "array_diff" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "diff", &args[1..], span);
                    }
                }
                "diffKey" | "diffKeys" | "array_diff_key" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "diffKey", &args[1..], span);
                    }
                }
                "intersect" | "array_intersect" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "intersect", &args[1..], span);
                    }
                }
                "intersectKey" | "intersectKeys" | "array_intersect_key" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "intersectKey", &args[1..], span);
                    }
                }
                "column" | "array_column" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "column", &args[1..], span);
                    }
                }
                "chunk" | "array_chunk" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "chunk", &args[1..], span);
                    }
                }
                "flip" | "array_flip" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "flip", &[], span);
                    }
                }
                "combine" | "array_combine" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "combine", &args[1..], span);
                    }
                }
                "countValues" | "count_values" | "array_count_values" | "countBy" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "countValues", &args[1..], span);
                    }
                }
                "shuffle" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "shuffle", &[], span);
                    }
                }
                "randomKey" | "randomKeys" | "randKey" | "array_rand" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "randomKey", &args[1..], span);
                    }
                }
                "random" => {
                    if let Some(arr) = args.first() {
                        return self.exec_fluent_value_method(arr, "random", &args[1..], span);
                    }
                }
                _ => {
                    if let Some(macro_closure) = get_runtime_closure_macro("Arr", method) {
                        return self.invoke_closure_sync(&macro_closure, args.to_vec(), span);
                    }
                }
            }
        }

        if facade == "Reflect" || facade == "phpFun" {
            match method {
                "classExists" | "class_exists" => {
                    let class_name = match args.first() {
                        Some(PhpValue::String(s)) => s.to_string(),
                        Some(val) => val.to_php_string(),
                        None => return Ok(PhpValue::Bool(false)),
                    };
                    return Ok(PhpValue::Bool(self.is_class_exists(&class_name)));
                }
                "functionExists" | "function_exists" => {
                    let fn_name = match args.first() {
                        Some(PhpValue::String(s)) => s.to_string(),
                        Some(val) => val.to_php_string(),
                        None => return Ok(PhpValue::Bool(false)),
                    };
                    return Ok(PhpValue::Bool(self.is_function_exists(&fn_name)));
                }
                _ => {}
            }
        }

        if facade == "App" {
            match method {
                "bind" => {
                    let abs = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let conc = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
                    self.container.bind(abs, conc);
                    return Ok(PhpValue::Null);
                }
                "singleton" | "shared" => {
                    let abs = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let conc = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
                    self.container.shared(abs, conc);
                    return Ok(PhpValue::Null);
                }
                "instance" => {
                    let abs = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let val = args.get(1).cloned().unwrap_or(PhpValue::Null);
                    self.container.instance(abs, val);
                    return Ok(PhpValue::Null);
                }
                "swap" => {
                    let abs = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let fake = args.get(1).cloned().unwrap_or(PhpValue::Null);
                    self.container.swap(abs, fake);
                    return Ok(PhpValue::Null);
                }
                "has" => {
                    let abs = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    return Ok(PhpValue::Bool(self.container.has(&abs)));
                }
                "make" | "get" | "resolve" => {
                    let abs = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let mut container = std::mem::take(&mut self.container);
                    let res = container.resolve(&abs, self);
                    self.container = container;
                    return res.map_err(|e| self.make_error(RuntimeErrorKind::Custom { message: e.to_string() }, span));
                }
                _ => {}
            }
        }

        dispatch_facade_call(facade, method, args, span)
    }

    /// Execute fluent chaining methods on subtyped collections, numeric wrappers, futures, and strings.
    fn exec_fluent_value_method(
        &mut self,
        receiver: &PhpValue,
        method: &str,
        args: &[PhpValue],
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        match receiver {
            // 1. Subtyped Dictionary methods (`Arr::dict()->map()->filter()->keys()->get()`, etc.)
            PhpValue::Dict(dict) => {
                match method {
                    "check" => {
                        let ok = match args.first() {
                            None => true,
                            Some(sub) => {
                                let clean = sub.to_php_string().trim().to_lowercase().replace(['-', '_', ' '], "");
                                clean == "dict" || clean == "dictionary"
                            }
                        };
                        Ok(PhpValue::Bool(ok))
                    }
                    "keys" => {
                        let keys: Vec<PhpValue> = dict
                            .borrow()
                            .keys()
                            .map(|k| PhpValue::string(k.clone()))
                            .collect();
                        Ok(PhpValue::list(keys))
                    }
                    "values" => {
                        let vals: Vec<PhpValue> = dict.borrow().values().cloned().collect();
                        Ok(PhpValue::list(vals))
                    }
                    "get" => {
                        let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                        let default = args.get(1).cloned().unwrap_or(PhpValue::Null);
                        if key.contains('.') {
                            Ok(arr_get_dot(receiver, &key, default))
                        } else {
                            Ok(dict.borrow().get(&key).cloned().unwrap_or(default))
                        }
                    }
                    "has" | "hasKey" => {
                        let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                        if key.contains('.') {
                            Ok(PhpValue::Bool(arr_has_dot(receiver, &key)))
                        } else {
                            Ok(PhpValue::Bool(dict.borrow().contains_key(&key)))
                        }
                    }
                    "hasAny" => {
                        let keys = args.first().map(extract_array_elements).unwrap_or_default();
                        let any = keys.iter().any(|k| dict.borrow().contains_key(&k.to_php_string()));
                        Ok(PhpValue::Bool(any))
                    }
                    "put" | "set" => {
                        let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                        let val = args.get(1).cloned().unwrap_or(PhpValue::Null);
                        dict.borrow_mut().insert(key, val);
                        Ok(receiver.clone())
                    }
                    "pull" => {
                        let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                        let default = args.get(1).cloned().unwrap_or(PhpValue::Null);
                        Ok(dict.borrow_mut().shift_remove(&key).unwrap_or(default))
                    }
                    "forget" | "remove" | "delete" => {
                        let keys: Vec<String> = if let Some(PhpValue::Array(a) | PhpValue::List(a)) = args.first() {
                            a.borrow().iter().map(|v| v.to_php_string()).collect()
                        } else {
                            args.iter().map(|v| v.to_php_string()).collect()
                        };
                        for k in keys {
                            dict.borrow_mut().shift_remove(&k);
                        }
                        Ok(receiver.clone())
                    }
                    "map" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let mut out = IndexMap::new();
                            let entries: Vec<(String, PhpValue)> = dict
                                .borrow()
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            for (k, v) in entries {
                                let new_v = self.invoke_closure_sync(
                                    cb,
                                    vec![v, PhpValue::string(k.clone())],
                                    span,
                                )?;
                                out.insert(k, new_v);
                            }
                            Ok(PhpValue::dict_from_map(out))
                        } else {
                            Ok(receiver.clone())
                        }
                    }
                    "mapWithKeys" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let mut out = IndexMap::new();
                            let entries: Vec<(String, PhpValue)> = dict
                                .borrow()
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            for (k, v) in entries {
                                let res = self.invoke_closure_sync(
                                    cb,
                                    vec![v, PhpValue::string(k)],
                                    span,
                                )?;
                                for (pair_k, pair_v) in extract_dict_elements(&res) {
                                    out.insert(pair_k, pair_v);
                                }
                            }
                            Ok(PhpValue::dict_from_map(out))
                        } else {
                            Ok(receiver.clone())
                        }
                    }
                    "filter" => {
                        let mut out = IndexMap::new();
                        let entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (k, v) in entries {
                                let keep = self.invoke_closure_sync(
                                    cb,
                                    vec![v.clone(), PhpValue::string(k.clone())],
                                    span,
                                )?;
                                if keep.is_truthy() {
                                    out.insert(k, v);
                                }
                            }
                        } else {
                            for (k, v) in entries {
                                if v.is_truthy() {
                                    out.insert(k, v);
                                }
                            }
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "reject" => {
                        let mut out = IndexMap::new();
                        let entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (k, v) in entries {
                                let drop = self.invoke_closure_sync(
                                    cb,
                                    vec![v.clone(), PhpValue::string(k.clone())],
                                    span,
                                )?;
                                if !drop.is_truthy() {
                                    out.insert(k, v);
                                }
                            }
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "reduce" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let mut acc = args.get(1).cloned().unwrap_or(PhpValue::Null);
                            let entries: Vec<(String, PhpValue)> = dict
                                .borrow()
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            for (k, v) in entries {
                                acc = self.invoke_closure_sync(
                                    cb,
                                    vec![acc, v, PhpValue::string(k)],
                                    span,
                                )?;
                            }
                            Ok(acc)
                        } else {
                            Ok(PhpValue::Null)
                        }
                    }
                    "first" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let entries: Vec<(String, PhpValue)> = dict
                                .borrow()
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            for (k, v) in entries {
                                let m = self.invoke_closure_sync(
                                    cb,
                                    vec![v.clone(), PhpValue::string(k)],
                                    span,
                                )?;
                                if m.is_truthy() {
                                    return Ok(v);
                                }
                            }
                            Ok(PhpValue::Null)
                        } else {
                            Ok(dict.borrow().values().next().cloned().unwrap_or(PhpValue::Null))
                        }
                    }
                    "last" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let entries: Vec<(String, PhpValue)> = dict
                                .borrow()
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            let mut last_match = PhpValue::Null;
                            for (k, v) in entries {
                                let m = self.invoke_closure_sync(
                                    cb,
                                    vec![v.clone(), PhpValue::string(k)],
                                    span,
                                )?;
                                if m.is_truthy() {
                                    last_match = v;
                                }
                            }
                            Ok(last_match)
                        } else {
                            Ok(dict.borrow().values().last().cloned().unwrap_or(PhpValue::Null))
                        }
                    }
                    "firstKey" => Ok(dict
                        .borrow()
                        .keys()
                        .next()
                        .map(|k| PhpValue::string(k.clone()))
                        .unwrap_or(PhpValue::Null)),
                    "lastKey" => Ok(dict
                        .borrow()
                        .keys()
                        .last()
                        .map(|k| PhpValue::string(k.clone()))
                        .unwrap_or(PhpValue::Null)),
                    "only" => {
                        let keys: Vec<String> = if let Some(PhpValue::Array(a) | PhpValue::List(a)) = args.first() {
                            a.borrow().iter().map(|v| v.to_php_string()).collect()
                        } else {
                            args.iter().map(|v| v.to_php_string()).collect()
                        };
                        let mut out = IndexMap::new();
                        let b = dict.borrow();
                        for k in keys {
                            if let Some(val) = b.get(&k) {
                                out.insert(k, val.clone());
                            }
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "except" => {
                        let keys: Vec<String> = if let Some(PhpValue::Array(a) | PhpValue::List(a)) = args.first() {
                            a.borrow().iter().map(|v| v.to_php_string()).collect()
                        } else {
                            args.iter().map(|v| v.to_php_string()).collect()
                        };
                        let mut out = IndexMap::new();
                        for (k, v) in dict.borrow().iter() {
                            if !keys.contains(k) {
                                out.insert(k.clone(), v.clone());
                            }
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "flip" | "array_flip" => {
                        let mut out = IndexMap::new();
                        for (k, v) in dict.borrow().iter() {
                            let orig_val = if let Ok(n) = k.parse::<i32>() {
                                PhpValue::Int((n) as i64)
                            } else {
                                PhpValue::string(k.clone())
                            };
                            out.insert(v.to_php_string(), orig_val);
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "merge" => {
                        let mut merged = dict.borrow().clone();
                        for arg in args {
                            for (k, v) in extract_dict_elements(arg) {
                                merged.insert(k, v);
                            }
                        }
                        Ok(PhpValue::dict_from_map(merged))
                    }
                    "sortKeys" | "ksort" => {
                        let mut sorted = dict.borrow().clone();
                        sorted.sort_keys();
                        Ok(PhpValue::dict_from_map(sorted))
                    }
                    "sortKeysBy" | "uksort" => {
                        let mut entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let len = entries.len();
                            for i in 0..len {
                                for j in (i + 1)..len {
                                    let cmp_res = self.invoke_closure_sync(
                                        cb,
                                        vec![PhpValue::string(entries[i].0.clone()), PhpValue::string(entries[j].0.clone())],
                                        span,
                                    )?;
                                    if cmp_res.to_i64().unwrap_or(0) > 0 {
                                        entries.swap(i, j);
                                    }
                                }
                            }
                        } else {
                            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
                        }
                        let mut out = IndexMap::new();
                        for (k, v) in entries {
                            out.insert(k, v);
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "sortKeysDesc" => {
                        let mut sorted = dict.borrow().clone();
                        sorted.sort_by(|k1, _, k2, _| k2.cmp(k1));
                        Ok(PhpValue::dict_from_map(sorted))
                    }
                    "sort" | "asort" | "uasort" | "usort" => {
                        let mut entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let len = entries.len();
                            for i in 0..len {
                                for j in (i + 1)..len {
                                    let cmp_res = self.invoke_closure_sync(
                                        cb,
                                        vec![entries[i].1.clone(), entries[j].1.clone()],
                                        span,
                                    )?;
                                    if cmp_res.to_i64().unwrap_or(0) > 0 {
                                        entries.swap(i, j);
                                    }
                                }
                            }
                        } else {
                            entries.sort_by(|(_, a), (_, b)| {
                                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                            });
                        }
                        let mut out = IndexMap::new();
                        for (k, v) in entries {
                            out.insert(k, v);
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "natsort" | "natSort" | "sortNatural" => {
                        let mut entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        entries.sort_by(|(_, a), (_, b)| php_value_natural_cmp(a, b, false));
                        let mut out = IndexMap::new();
                        for (k, v) in entries {
                            out.insert(k, v);
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "natcasesort" | "natCaseSort" | "sortNaturalCase" => {
                        let mut entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        entries.sort_by(|(_, a), (_, b)| php_value_natural_cmp(a, b, true));
                        let mut out = IndexMap::new();
                        for (k, v) in entries {
                            out.insert(k, v);
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "sortBy" => {
                        let mut entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let len = entries.len();
                            for i in 0..len {
                                for j in (i + 1)..len {
                                    let ki = self.invoke_closure_sync(
                                        cb,
                                        vec![entries[i].1.clone()],
                                        span,
                                    )?;
                                    let kj = self.invoke_closure_sync(
                                        cb,
                                        vec![entries[j].1.clone()],
                                        span,
                                    )?;
                                    if ki.partial_cmp(&kj) == Some(std::cmp::Ordering::Greater) {
                                        entries.swap(i, j);
                                    }
                                }
                            }
                        } else if let Some(key_val) = args.first() {
                            let key_str = key_val.to_php_string();
                            entries.sort_by(|(_, a), (_, b)| {
                                let ka = match a {
                                    PhpValue::Object(o) => {
                                        o.borrow().get_property(&key_str).cloned().unwrap_or(PhpValue::Null)
                                    }
                                    PhpValue::Dict(d) => {
                                        d.borrow().get(&key_str).cloned().unwrap_or(PhpValue::Null)
                                    }
                                    _ => PhpValue::Null,
                                };
                                let kb = match b {
                                    PhpValue::Object(o) => {
                                        o.borrow().get_property(&key_str).cloned().unwrap_or(PhpValue::Null)
                                    }
                                    PhpValue::Dict(d) => {
                                        d.borrow().get(&key_str).cloned().unwrap_or(PhpValue::Null)
                                    }
                                    _ => PhpValue::Null,
                                };
                                ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
                            });
                        }
                        let mut out = IndexMap::new();
                        for (k, v) in entries {
                            out.insert(k, v);
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "reverse" => {
                        let mut rev = dict.borrow().clone();
                        rev.reverse();
                        Ok(PhpValue::dict_from_map(rev))
                    }
                    "count" | "size" | "length" | "len" => {
                        Ok(PhpValue::Int(dict.borrow().len() as i64))
                    }
                    "isEmpty" => Ok(PhpValue::Bool(dict.borrow().is_empty())),
                    "isNotEmpty" => Ok(PhpValue::Bool(!dict.borrow().is_empty())),
                    "all" | "toArray" => Ok(receiver.clone()),
                    "contains" | "hasValue" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (k, v) in dict.borrow().iter() {
                                let matched = self.invoke_closure_sync(
                                    cb,
                                    vec![v.clone(), PhpValue::string(k.clone())],
                                    span,
                                )?;
                                if matched.is_truthy() {
                                    return Ok(PhpValue::Bool(true));
                                }
                            }
                            Ok(PhpValue::Bool(false))
                        } else {
                            let needle = args.first().cloned().unwrap_or(PhpValue::Null);
                            let found = dict.borrow().values().any(|v| v == &needle);
                            Ok(PhpValue::Bool(found))
                        }
                    }
                    "each" | "forEach" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (k, v) in dict.borrow().iter() {
                                let res = self.invoke_closure_sync(
                                    cb,
                                    vec![v.clone(), PhpValue::string(k.clone())],
                                    span,
                                )?;
                                if res == PhpValue::Bool(false) {
                                    break;
                                }
                            }
                        }
                        Ok(receiver.clone())
                    }
                    "pluck" => {
                        let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                        let key_by = args.get(1).map(|v| v.to_php_string());
                        if let Some(kb) = key_by {
                            let mut map = IndexMap::new();
                            for v in dict.borrow().values() {
                                let k = match v {
                                    PhpValue::Object(o) => {
                                        o.borrow().get_property(&kb).map(|val| val.to_php_string())
                                    }
                                    PhpValue::Dict(d) => {
                                        d.borrow().get(&kb).map(|val| val.to_php_string())
                                    }
                                    _ => None,
                                }.unwrap_or_default();
                                let plucked_val = match v {
                                    PhpValue::Object(o) => {
                                        o.borrow().get_property(&key).cloned().unwrap_or(PhpValue::Null)
                                    }
                                    PhpValue::Dict(d) => {
                                        d.borrow().get(&key).cloned().unwrap_or(PhpValue::Null)
                                    }
                                    _ => PhpValue::Null,
                                };
                                map.insert(k, plucked_val);
                            }
                            Ok(PhpValue::dict_from_map(map))
                        } else {
                            let mut plucked = Vec::new();
                            for v in dict.borrow().values() {
                                match v {
                                    PhpValue::Object(o) => {
                                        plucked.push(o.borrow().get_property(&key).cloned().unwrap_or(PhpValue::Null))
                                    }
                                    PhpValue::Dict(d) => {
                                        plucked.push(d.borrow().get(&key).cloned().unwrap_or(PhpValue::Null))
                                    }
                                    _ => plucked.push(PhpValue::Null),
                                }
                            }
                            Ok(PhpValue::list(plucked))
                        }
                    }
                    "column" | "array_column" => {
                        let col_opt = args.first();
                        let is_col_null = col_opt.map_or(true, |v| matches!(v, PhpValue::Null));
                        let col_name = col_opt.map(|v| v.to_php_string()).unwrap_or_default();
                        let index_key_opt = args.get(1);

                        let mut map = IndexMap::new();
                        for (orig_k, v) in dict.borrow().iter() {
                            let k = if let Some(idx_key_val) = index_key_opt {
                                if !matches!(idx_key_val, PhpValue::Null) {
                                    let idx_name = idx_key_val.to_php_string();
                                    get_item_column(v, &idx_name).map(|val| val.to_php_string()).unwrap_or_default()
                                } else {
                                    orig_k.clone()
                                }
                            } else {
                                orig_k.clone()
                            };
                            let plucked_val = if is_col_null {
                                v.clone()
                            } else {
                                get_item_column(v, &col_name).unwrap_or(PhpValue::Null)
                            };
                            map.insert(k, plucked_val);
                        }
                        Ok(PhpValue::dict_from_map(map))
                    }
                    "combine" | "array_combine" => {
                        let vals_arg = args.first().unwrap_or(&PhpValue::Null);
                        let vals = Self::extract_items(vals_arg);
                        let dict_vals: Vec<PhpValue> = dict.borrow().values().cloned().collect();
                        if dict_vals.len() != vals.len() {
                            return Err(self.make_error(
                                RuntimeErrorKind::ValueError {
                                    message: "array_combine(): Argument #1 ($keys) and argument #2 ($values) must have the same number of elements".to_string(),
                                },
                                span,
                            ));
                        }
                        let mut map = IndexMap::new();
                        for (k, v) in dict_vals.into_iter().zip(vals) {
                            map.insert(k.to_php_string(), v);
                        }
                        Ok(PhpValue::dict_from_map(map))
                    }
                    "countValues" | "count_values" | "array_count_values" | "countBy" => {
                        let mut counts = IndexMap::new();
                        for v in dict.borrow().values() {
                            let key = if let Some(PhpValue::Closure(cb)) = args.first() {
                                self.invoke_closure_sync(cb, vec![v.clone()], span)?.to_php_string()
                            } else if let Some(prop) = args.first() {
                                let prop_str = prop.to_php_string();
                                get_item_column(v, &prop_str).unwrap_or(PhpValue::Null).to_php_string()
                            } else {
                                v.to_php_string()
                            };
                            let entry = counts.entry(key).or_insert(0i32);
                            *entry += 1;
                        }
                        let mut out = IndexMap::new();
                        for (k, cnt) in counts {
                            out.insert(k, PhpValue::Int((cnt) as i64));
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "shuffle" => {
                        let mut entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        shuffle_slice(&mut entries);
                        let mut out = IndexMap::new();
                        for (k, v) in entries {
                            out.insert(k, v);
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "randomKey" | "randomKeys" | "randKey" | "array_rand" => {
                        let keys: Vec<String> = dict.borrow().keys().cloned().collect();
                        if keys.is_empty() {
                            return Ok(PhpValue::Null);
                        }
                        let num = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as usize;
                        if num < 1 || num > keys.len() {
                            return Err(self.make_error(
                                RuntimeErrorKind::ValueError {
                                    message: "array_rand(): Argument #2 ($num) must be between 1 and the number of elements or greater in array".to_string(),
                                },
                                span,
                            ));
                        }
                        if num == 1 {
                            let idx = random_usize(keys.len());
                            Ok(PhpValue::string(keys[idx].clone()))
                        } else {
                            let mut indices: Vec<usize> = (0..keys.len()).collect();
                            shuffle_slice(&mut indices);
                            indices.truncate(num);
                            indices.sort();
                            let picked: Vec<PhpValue> = indices.into_iter().map(|i| PhpValue::string(keys[i].clone())).collect();
                            Ok(PhpValue::list(picked))
                        }
                    }
                    "random" => {
                        let entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        if entries.is_empty() {
                            return Ok(PhpValue::Null);
                        }
                        let num_opt = args.first().and_then(|v| if v.is_null() { None } else { v.to_i64() });
                        match num_opt {
                            None => {
                                let idx = random_usize(entries.len());
                                Ok(entries[idx].1.clone())
                            }
                            Some(num) => {
                                let n = num as usize;
                                if n < 1 || n > entries.len() {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::ValueError {
                                            message: format!("Arr::random(): requested {} elements, but array contains only {}", n, entries.len()),
                                        },
                                        span,
                                    ));
                                }
                                let mut indices: Vec<usize> = (0..entries.len()).collect();
                                shuffle_slice(&mut indices);
                                indices.truncate(n);
                                let mut map = IndexMap::new();
                                for i in indices {
                                    map.insert(entries[i].0.clone(), entries[i].1.clone());
                                }
                                Ok(PhpValue::dict_from_map(map))
                            }
                        }
                    }
                    "diff" | "array_diff" => {
                        let mut exclude = HashSet::new();
                        for arg in args {
                            for (_, v) in extract_dict_elements(arg) {
                                exclude.insert(v);
                            }
                        }
                        let mut out = IndexMap::new();
                        for (k, v) in dict.borrow().iter() {
                            if !exclude.contains(v) {
                                out.insert(k.clone(), v.clone());
                            }
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "diffKeys" | "diffKey" | "array_diff_key" => {
                        let mut exclude_keys = HashSet::new();
                        for arg in args {
                            for (k, _) in extract_dict_elements(arg) {
                                exclude_keys.insert(k);
                            }
                        }
                        let mut out = IndexMap::new();
                        for (k, v) in dict.borrow().iter() {
                            if !exclude_keys.contains(k) {
                                out.insert(k.clone(), v.clone());
                            }
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "intersect" | "array_intersect" => {
                        let other_val_sets: Vec<HashSet<PhpValue>> = args
                            .iter()
                            .map(|arg| extract_dict_elements(arg).into_iter().map(|(_, v)| v).collect())
                            .collect();
                        let mut out = IndexMap::new();
                        for (k, v) in dict.borrow().iter() {
                            if other_val_sets.iter().all(|set| set.contains(v)) {
                                out.insert(k.clone(), v.clone());
                            }
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "intersectKeys" | "intersectKey" | "array_intersect_key" => {
                        let other_key_sets: Vec<HashSet<String>> = args
                            .iter()
                            .map(|arg| extract_dict_elements(arg).into_iter().map(|(k, _)| k).collect())
                            .collect();
                        let mut out = IndexMap::new();
                        for (k, v) in dict.borrow().iter() {
                            if other_key_sets.iter().all(|set| set.contains(k)) {
                                out.insert(k.clone(), v.clone());
                            }
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "chunk" | "array_chunk" => {
                        let size = args.first().and_then(|v| v.to_i64()).unwrap_or(1).max(1) as usize;
                        let entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        let chunks: Vec<PhpValue> = entries
                            .chunks(size)
                            .map(|c| {
                                let mut m = IndexMap::new();
                                for (k, v) in c {
                                    m.insert(k.clone(), v.clone());
                                }
                                PhpValue::dict_from_map(m)
                            })
                            .collect();
                        Ok(PhpValue::list(chunks))
                    }
                    "take" => {
                        let limit = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
                        let entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        let taken: Vec<(String, PhpValue)> = if limit >= 0 {
                            entries.into_iter().take(limit as usize).collect()
                        } else {
                            let count = (-limit) as usize;
                            let skip_n = entries.len().saturating_sub(count);
                            entries.into_iter().skip(skip_n).collect()
                        };
                        let mut out = IndexMap::new();
                        for (k, v) in taken {
                            out.insert(k, v);
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "skip" => {
                        let count = args.first().and_then(|v| v.to_i64()).unwrap_or(0).max(0) as usize;
                        let entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        let mut out = IndexMap::new();
                        for (k, v) in entries.into_iter().skip(count) {
                            out.insert(k, v);
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "slice" => {
                        let offset = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
                        let len_req = args.get(1).and_then(|v| v.to_i64());
                        let entries: Vec<(String, PhpValue)> = dict
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        let total_len = entries.len() as i64;
                        let start_idx = if offset < 0 {
                            (total_len + offset).max(0) as usize
                        } else {
                            offset.min(total_len) as usize
                        };
                        let slice: Vec<(String, PhpValue)> = if let Some(l) = len_req {
                            let count = if l < 0 {
                                (total_len - start_idx as i64 + l).max(0) as usize
                            } else {
                                l as usize
                            };
                            entries.into_iter().skip(start_idx).take(count).collect()
                        } else {
                            entries.into_iter().skip(start_idx).collect()
                        };
                        let mut out = IndexMap::new();
                        for (k, v) in slice {
                            out.insert(k, v);
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "sum" => {
                        let mut is_float = false;
                        let mut float_sum = 0.0f64;
                        let mut int_sum = 0i64;
                        for item in dict.borrow().values() {
                            if let Some(fl) = item.to_f64() {
                                if item.to_i64().is_none()
                                    || matches!(item, PhpValue::Float(_))
                                {
                                    is_float = true;
                                }
                                float_sum += fl;
                            }
                            if let Some(i) = item.to_i64() {
                                int_sum += i;
                            }
                        }
                        if is_float {
                            Ok(PhpValue::float(float_sum))
                        } else {
                            Ok(PhpValue::Int(int_sum))
                        }
                    }
                    "avg" | "average" => {
                        let b = dict.borrow();
                        if b.is_empty() {
                            Ok(PhpValue::float(0.0))
                        } else {
                            let mut sum = 0.0f64;
                            for item in b.values() {
                                sum += item.to_f64().unwrap_or(0.0);
                            }
                            Ok(PhpValue::float(sum / b.len() as f64))
                        }
                    }
                    "min" => {
                        let mut min_val: Option<PhpValue> = None;
                        for item in dict.borrow().values() {
                            min_val = match min_val {
                                None => Some(item.clone()),
                                Some(prev) => {
                                    if item.partial_cmp(&prev) == Some(std::cmp::Ordering::Less) {
                                        Some(item.clone())
                                    } else {
                                        Some(prev)
                                    }
                                }
                            };
                        }
                        Ok(min_val.unwrap_or(PhpValue::Null))
                    }
                    "max" => {
                        let mut max_val: Option<PhpValue> = None;
                        for item in dict.borrow().values() {
                            max_val = match max_val {
                                None => Some(item.clone()),
                                Some(prev) => {
                                    if item.partial_cmp(&prev) == Some(std::cmp::Ordering::Greater) {
                                        Some(item.clone())
                                    } else {
                                        Some(prev)
                                    }
                                }
                            };
                        }
                        Ok(max_val.unwrap_or(PhpValue::Null))
                    }
                    "toJson" => Ok(PhpValue::string(php_value_to_json(receiver))),
                    other => {
                        if let Some(macro_closure) = get_runtime_closure_macro("Arr", other) {
                            self.invoke_closure_sync_with_this(
                                &macro_closure,
                                Some(receiver.clone()),
                                args.to_vec(),
                                span,
                            )
                        } else {
                            Err(self.make_error(
                                RuntimeErrorKind::UndefinedMethod {
                                    class: "Arr::dict".to_string(),
                                    method: other.to_string(),
                                },
                                span,
                            ))
                        }
                    }
                }
            }

            // 2. Subtyped Unique Set methods (`Arr::set()->contains()->add()->diff()`, etc.)
            PhpValue::Set(set) => {
                match method {
                    "check" => {
                        let ok = match args.first() {
                            None => true,
                            Some(sub) => {
                                let clean = sub.to_php_string().trim().to_lowercase().replace(['-', '_', ' '], "");
                                clean == "set"
                            }
                        };
                        Ok(PhpValue::Bool(ok))
                    }
                    "contains" | "has" | "hasKey" | "hasValue" => {
                        let target = match args.first() {
                            Some(v) => v,
                            None => return Ok(PhpValue::Bool(false)),
                        };
                        Ok(PhpValue::Bool(set.borrow().contains(target)))
                    }
                    "add" | "push" => {
                        let val = args.first().cloned().unwrap_or(PhpValue::Null);
                        let b = set.borrow();
                        if let Some(first) = b.iter().next() {
                            if !first.same_type_as(&val) {
                                return Err(self.make_error(
                                    RuntimeErrorKind::TypeError {
                                        message: format!(
                                            "All elements in a set must have the same type, found {} and {}",
                                            first.type_name(),
                                            val.type_name()
                                        ),
                                    },
                                    span,
                                ));
                            }
                        }
                        drop(b);
                        set.borrow_mut().insert(val);
                        Ok(receiver.clone())
                    }
                    "remove" | "delete" => {
                        let val = args.first().cloned().unwrap_or(PhpValue::Null);
                        let removed = set.borrow_mut().remove(&val);
                        Ok(PhpValue::Bool(removed))
                    }
                    "count" | "size" | "length" | "len" => Ok(PhpValue::Int(set.borrow().len() as i64)),
                    "isEmpty" => Ok(PhpValue::Bool(set.borrow().is_empty())),
                    "isNotEmpty" => Ok(PhpValue::Bool(!set.borrow().is_empty())),
                    "keys" => {
                        let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                        Ok(PhpValue::list(items))
                    }
                    "all" | "toArray" | "values" => {
                        let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                        Ok(PhpValue::list(items))
                    }
                    "map" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let mut out = HashSet::new();
                            let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                            for item in items {
                                let res = self.invoke_closure_sync(cb, vec![item], span)?;
                                out.insert(res);
                            }
                            Ok(PhpValue::set_from_hash_set(out))
                        } else {
                            Ok(receiver.clone())
                        }
                    }
                    "filter" => {
                        let mut out = HashSet::new();
                        let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for item in items {
                                let keep = self.invoke_closure_sync(cb, vec![item.clone()], span)?;
                                if keep.is_truthy() {
                                    out.insert(item);
                                }
                            }
                        } else {
                            for item in items {
                                if item.is_truthy() {
                                    out.insert(item);
                                }
                            }
                        }
                        Ok(PhpValue::set_from_hash_set(out))
                    }
                    "reject" => {
                        let mut out = HashSet::new();
                        let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for item in items {
                                let drop = self.invoke_closure_sync(cb, vec![item.clone()], span)?;
                                if !drop.is_truthy() {
                                    out.insert(item);
                                }
                            }
                        }
                        Ok(PhpValue::set_from_hash_set(out))
                    }
                    "reduce" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let mut acc = args.get(1).cloned().unwrap_or(PhpValue::Null);
                            let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                            for item in items {
                                acc = self.invoke_closure_sync(cb, vec![acc, item], span)?;
                            }
                            Ok(acc)
                        } else {
                            Ok(PhpValue::Null)
                        }
                    }
                    "sort" | "asort" | "usort" | "uasort" => {
                        let mut items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let len = items.len();
                            for i in 0..len {
                                for j in (i + 1)..len {
                                    let cmp_res = self.invoke_closure_sync(
                                        cb,
                                        vec![items[i].clone(), items[j].clone()],
                                        span,
                                    )?;
                                    if cmp_res.to_i64().unwrap_or(0) > 0 {
                                        items.swap(i, j);
                                    }
                                }
                            }
                        } else {
                            items.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                        }
                        let mut out = HashSet::new();
                        for it in items {
                            out.insert(it);
                        }
                        Ok(PhpValue::set_from_hash_set(out))
                    }
                    "natsort" | "natSort" | "sortNatural" | "natcasesort" | "natCaseSort" | "sortNaturalCase" => {
                        Ok(PhpValue::set_from_hash_set(set.borrow().clone()))
                    }
                    "diff" | "array_diff" => {
                        let mut exclude = HashSet::new();
                        for arg in args {
                            match arg {
                                PhpValue::Set(s) => {
                                    for it in s.borrow().iter() {
                                        exclude.insert(it.clone());
                                    }
                                }
                                other => {
                                    for it in Self::extract_items(other) {
                                        exclude.insert(it);
                                    }
                                }
                            }
                        }
                        let mut out = HashSet::new();
                        for item in set.borrow().iter() {
                            if !exclude.contains(item) {
                                out.insert(item.clone());
                            }
                        }
                        Ok(PhpValue::set_from_hash_set(out))
                    }
                    "diffKey" | "array_diff_key" => {
                        Ok(PhpValue::set_from_hash_set(set.borrow().clone()))
                    }
                    "intersect" | "array_intersect" => {
                        let other_sets: Vec<HashSet<PhpValue>> = args
                            .iter()
                            .map(|arg| match arg {
                                PhpValue::Set(s) => s.borrow().clone(),
                                other => Self::extract_items(other).into_iter().collect(),
                            })
                            .collect();
                        let mut out = HashSet::new();
                        for item in set.borrow().iter() {
                            if other_sets.iter().all(|s| s.contains(item)) {
                                out.insert(item.clone());
                            }
                        }
                        Ok(PhpValue::set_from_hash_set(out))
                    }
                    "intersectKey" | "array_intersect_key" => {
                        Ok(PhpValue::set_from_hash_set(set.borrow().clone()))
                    }
                    "column" | "array_column" => {
                        let col_opt = args.first();
                        let is_col_null = col_opt.map_or(true, |v| matches!(v, PhpValue::Null));
                        let col_name = col_opt.map(|v| v.to_php_string()).unwrap_or_default();
                        let mut out = HashSet::new();
                        for item in set.borrow().iter() {
                            let v = if is_col_null {
                                item.clone()
                            } else {
                                get_item_column(item, &col_name).unwrap_or(PhpValue::Null)
                            };
                            out.insert(v);
                        }
                        Ok(PhpValue::set_from_hash_set(out))
                    }
                    "chunk" | "array_chunk" => {
                        let size = args.first().and_then(|v| v.to_i64()).unwrap_or(1).max(1) as usize;
                        let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                        let chunks: Vec<PhpValue> = items
                            .chunks(size)
                            .map(|c| {
                                let mut hs = HashSet::new();
                                for it in c {
                                    hs.insert(it.clone());
                                }
                                PhpValue::set_from_hash_set(hs)
                            })
                            .collect();
                        Ok(PhpValue::list(chunks))
                    }
                    "flip" | "array_flip" => {
                        Ok(PhpValue::set_from_hash_set(set.borrow().clone()))
                    }
                    "combine" | "array_combine" => {
                        let vals_arg = args.first().unwrap_or(&PhpValue::Null);
                        let vals = Self::extract_items(vals_arg);
                        let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                        if items.len() != vals.len() {
                            return Err(self.make_error(
                                RuntimeErrorKind::ValueError {
                                    message: "array_combine(): Argument #1 ($keys) and argument #2 ($values) must have the same number of elements".to_string(),
                                },
                                span,
                            ));
                        }
                        let mut map = IndexMap::new();
                        for (k, v) in items.into_iter().zip(vals) {
                            map.insert(k.to_php_string(), v);
                        }
                        Ok(PhpValue::dict_from_map(map))
                    }
                    "countValues" | "count_values" | "array_count_values" | "countBy" => {
                        let mut counts = IndexMap::new();
                        for item in set.borrow().iter() {
                            let key = if let Some(PhpValue::Closure(cb)) = args.first() {
                                self.invoke_closure_sync(cb, vec![item.clone()], span)?.to_php_string()
                            } else if let Some(prop) = args.first() {
                                let prop_str = prop.to_php_string();
                                get_item_column(item, &prop_str).unwrap_or(PhpValue::Null).to_php_string()
                            } else {
                                item.to_php_string()
                            };
                            let entry = counts.entry(key).or_insert(0i32);
                            *entry += 1;
                        }
                        let mut out = IndexMap::new();
                        for (k, cnt) in counts {
                            out.insert(k, PhpValue::Int((cnt) as i64));
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "shuffle" => {
                        Ok(PhpValue::set_from_hash_set(set.borrow().clone()))
                    }
                    "randomKey" | "randomKeys" | "randKey" | "array_rand" => {
                        let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                        if items.is_empty() {
                            return Ok(PhpValue::Null);
                        }
                        let num = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as usize;
                        if num < 1 || num > items.len() {
                            return Err(self.make_error(
                                RuntimeErrorKind::ValueError {
                                    message: "array_rand(): Argument #2 ($num) must be between 1 and the number of elements or greater in array".to_string(),
                                },
                                span,
                            ));
                        }
                        if num == 1 {
                            let idx = random_usize(items.len());
                            Ok(items[idx].clone())
                        } else {
                            let mut indices: Vec<usize> = (0..items.len()).collect();
                            shuffle_slice(&mut indices);
                            indices.truncate(num);
                            let picked: HashSet<PhpValue> = indices.into_iter().map(|i| items[i].clone()).collect();
                            Ok(PhpValue::set_from_hash_set(picked))
                        }
                    }
                    "random" => {
                        let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                        if items.is_empty() {
                            return Ok(PhpValue::Null);
                        }
                        let num_opt = args.first().and_then(|v| if v.is_null() { None } else { v.to_i64() });
                        match num_opt {
                            None => {
                                let idx = random_usize(items.len());
                                Ok(items[idx].clone())
                            }
                            Some(num) => {
                                let n = num as usize;
                                if n < 1 || n > items.len() {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::ValueError {
                                            message: format!("Arr::random(): requested {} elements, but set contains only {}", n, items.len()),
                                        },
                                        span,
                                    ));
                                }
                                let mut indices: Vec<usize> = (0..items.len()).collect();
                                shuffle_slice(&mut indices);
                                indices.truncate(n);
                                let picked: HashSet<PhpValue> = indices.into_iter().map(|i| items[i].clone()).collect();
                                Ok(PhpValue::set_from_hash_set(picked))
                            }
                        }
                    }
                    "union" => {
                        let mut out = set.borrow().clone();
                        if let Some(other) = args.first() {
                            match other {
                                PhpValue::Set(s) => {
                                    for item in s.borrow().iter() {
                                        out.insert(item.clone());
                                    }
                                }
                                _ => {
                                    for item in Self::extract_items(other) {
                                        out.insert(item);
                                    }
                                }
                            }
                        }
                        Ok(PhpValue::set_from_hash_set(out))
                    }
                    "first" => Ok(set.borrow().iter().next().cloned().unwrap_or(PhpValue::Null)),
                    "each" | "forEach" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let items: Vec<PhpValue> = set.borrow().iter().cloned().collect();
                            for item in items {
                                self.invoke_closure_sync(cb, vec![item], span)?;
                            }
                        }
                        Ok(receiver.clone())
                    }
                    "toJson" => Ok(PhpValue::string(php_value_to_json(receiver))),
                    other => {
                        if let Some(macro_closure) = get_runtime_closure_macro("Arr", other) {
                            self.invoke_closure_sync_with_this(
                                &macro_closure,
                                Some(receiver.clone()),
                                args.to_vec(),
                                span,
                            )
                        } else {
                            Err(self.make_error(
                                RuntimeErrorKind::UndefinedMethod {
                                    class: "Arr::set".to_string(),
                                    method: other.to_string(),
                                },
                                span,
                            ))
                        }
                    }
                }
            }

            // 3. Subtyped Universal Hash Map methods (`Arr::hashMap()->get()->put()->keys()`, etc.)
            PhpValue::HashMap(map) => {
                match method {
                    "check" => {
                        let ok = match args.first() {
                            None => true,
                            Some(sub) => {
                                let clean = sub.to_php_string().trim().to_lowercase().replace(['-', '_', ' '], "");
                                clean == "hashmap" || clean == "map"
                            }
                        };
                        Ok(PhpValue::Bool(ok))
                    }
                    "get" => {
                        let key = args.first().cloned().unwrap_or(PhpValue::Null);
                        let default = args.get(1).cloned().unwrap_or(PhpValue::Null);
                        Ok(map.borrow().get(&key).cloned().unwrap_or(default))
                    }
                    "has" | "contains" | "hasKey" => {
                        let key = args.first().cloned().unwrap_or(PhpValue::Null);
                        Ok(PhpValue::Bool(map.borrow().contains_key(&key)))
                    }
                    "hasValue" => {
                        let target = args.first().cloned().unwrap_or(PhpValue::Null);
                        let found = map.borrow().values().any(|v| v == &target);
                        Ok(PhpValue::Bool(found))
                    }
                    "put" | "set" => {
                        let key = args.first().cloned().unwrap_or(PhpValue::Null);
                        let val = args.get(1).cloned().unwrap_or(PhpValue::Null);
                        map.borrow_mut().insert(key, val);
                        Ok(receiver.clone())
                    }
                    "pull" => {
                        let key = args.first().cloned().unwrap_or(PhpValue::Null);
                        let default = args.get(1).cloned().unwrap_or(PhpValue::Null);
                        Ok(map.borrow_mut().remove(&key).unwrap_or(default))
                    }
                    "forget" | "remove" => {
                        for arg in args {
                            map.borrow_mut().remove(arg);
                        }
                        Ok(receiver.clone())
                    }
                    "keys" => {
                        let keys: Vec<PhpValue> = map.borrow().keys().cloned().collect();
                        Ok(PhpValue::list(keys))
                    }
                    "values" => {
                        let vals: Vec<PhpValue> = map.borrow().values().cloned().collect();
                        Ok(PhpValue::list(vals))
                    }
                    "entries" | "all" | "toArray" => {
                        let entries: Vec<PhpValue> = map
                            .borrow()
                            .iter()
                            .map(|(k, v)| PhpValue::list(vec![k.clone(), v.clone()]))
                            .collect();
                        Ok(PhpValue::list(entries))
                    }
                    "count" | "size" | "length" | "len" => Ok(PhpValue::Int(map.borrow().len() as i64)),
                    "isEmpty" => Ok(PhpValue::Bool(map.borrow().is_empty())),
                    "isNotEmpty" => Ok(PhpValue::Bool(!map.borrow().is_empty())),
                    "map" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let mut out = HashMap::new();
                            let entries: Vec<(PhpValue, PhpValue)> = map
                                .borrow()
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            for (k, v) in entries {
                                let new_v = self.invoke_closure_sync(cb, vec![v, k.clone()], span)?;
                                out.insert(k, new_v);
                            }
                            Ok(PhpValue::hash_map_from_map(out))
                        } else {
                            Ok(receiver.clone())
                        }
                    }
                    "mapWithKeys" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let mut out = HashMap::new();
                            let entries: Vec<(PhpValue, PhpValue)> = map
                                .borrow()
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            for (k, v) in entries {
                                let res = self.invoke_closure_sync(cb, vec![v, k], span)?;
                                match res {
                                    PhpValue::HashMap(m) => {
                                        for (pair_k, pair_v) in m.borrow().iter() {
                                            out.insert(pair_k.clone(), pair_v.clone());
                                        }
                                    }
                                    PhpValue::Dict(d) => {
                                        for (pair_k, pair_v) in d.borrow().iter() {
                                            out.insert(PhpValue::string(pair_k.clone()), pair_v.clone());
                                        }
                                    }
                                    PhpValue::Array(a) | PhpValue::List(a) => {
                                        let b = a.borrow();
                                        if b.len() == 2 {
                                            out.insert(b[0].clone(), b[1].clone());
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            Ok(PhpValue::hash_map_from_map(out))
                        } else {
                            Ok(receiver.clone())
                        }
                    }
                    "filter" => {
                        let mut out = HashMap::new();
                        let entries: Vec<(PhpValue, PhpValue)> = map
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (k, v) in entries {
                                let keep = self.invoke_closure_sync(cb, vec![v.clone(), k.clone()], span)?;
                                if keep.is_truthy() {
                                    out.insert(k, v);
                                }
                            }
                        } else {
                            for (k, v) in entries {
                                if v.is_truthy() {
                                    out.insert(k, v);
                                }
                            }
                        }
                        Ok(PhpValue::hash_map_from_map(out))
                    }
                    "reject" => {
                        let mut out = HashMap::new();
                        let entries: Vec<(PhpValue, PhpValue)> = map
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (k, v) in entries {
                                let drop = self.invoke_closure_sync(cb, vec![v.clone(), k.clone()], span)?;
                                if !drop.is_truthy() {
                                    out.insert(k, v);
                                }
                            }
                        }
                        Ok(PhpValue::hash_map_from_map(out))
                    }
                    "reduce" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let mut acc = args.get(1).cloned().unwrap_or(PhpValue::Null);
                            let entries: Vec<(PhpValue, PhpValue)> = map
                                .borrow()
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            for (k, v) in entries {
                                acc = self.invoke_closure_sync(cb, vec![acc, v, k], span)?;
                            }
                            Ok(acc)
                        } else {
                            Ok(PhpValue::Null)
                        }
                    }
                    "diff" => {
                        let other_keys: HashSet<PhpValue> = match args.first() {
                            Some(PhpValue::HashMap(m)) => m.borrow().keys().cloned().collect(),
                            Some(PhpValue::Dict(d)) => d.borrow().keys().map(|k| PhpValue::string(k.clone())).collect(),
                            Some(PhpValue::Set(s)) => s.borrow().clone(),
                            Some(other) => Self::extract_items(other).into_iter().collect(),
                            None => HashSet::new(),
                        };
                        let mut out = HashMap::new();
                        for (k, v) in map.borrow().iter() {
                            if !other_keys.contains(k) {
                                out.insert(k.clone(), v.clone());
                            }
                        }
                        Ok(PhpValue::hash_map_from_map(out))
                    }
                    "intersect" => {
                        let other_keys: HashSet<PhpValue> = match args.first() {
                            Some(PhpValue::HashMap(m)) => m.borrow().keys().cloned().collect(),
                            Some(PhpValue::Dict(d)) => d.borrow().keys().map(|k| PhpValue::string(k.clone())).collect(),
                            Some(PhpValue::Set(s)) => s.borrow().clone(),
                            Some(other) => Self::extract_items(other).into_iter().collect(),
                            None => HashSet::new(),
                        };
                        let mut out = HashMap::new();
                        for (k, v) in map.borrow().iter() {
                            if other_keys.contains(k) {
                                out.insert(k.clone(), v.clone());
                            }
                        }
                        Ok(PhpValue::hash_map_from_map(out))
                    }
                    "merge" => {
                        let mut merged = map.borrow().clone();
                        for arg in args {
                            match arg {
                                PhpValue::HashMap(m) => {
                                    for (k, v) in m.borrow().iter() {
                                        merged.insert(k.clone(), v.clone());
                                    }
                                }
                                PhpValue::Dict(d) => {
                                    for (k, v) in d.borrow().iter() {
                                        merged.insert(PhpValue::string(k.clone()), v.clone());
                                    }
                                }
                                _ => {}
                            }
                        }
                        Ok(PhpValue::hash_map_from_map(merged))
                    }
                    "natsort" | "natSort" | "sortNatural" | "natcasesort" | "natCaseSort" | "sortNaturalCase" => {
                        Ok(PhpValue::hash_map_from_map(map.borrow().clone()))
                    }
                    "countValues" | "count_values" | "array_count_values" | "countBy" => {
                        let mut counts = IndexMap::new();
                        for v in map.borrow().values() {
                            let key = if let Some(PhpValue::Closure(cb)) = args.first() {
                                self.invoke_closure_sync(cb, vec![v.clone()], span)?.to_php_string()
                            } else if let Some(prop) = args.first() {
                                let prop_str = prop.to_php_string();
                                get_item_column(v, &prop_str).unwrap_or(PhpValue::Null).to_php_string()
                            } else {
                                v.to_php_string()
                            };
                            let entry = counts.entry(key).or_insert(0i32);
                            *entry += 1;
                        }
                        let mut out = IndexMap::new();
                        for (k, cnt) in counts {
                            out.insert(k, PhpValue::Int((cnt) as i64));
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "shuffle" => {
                        let mut entries: Vec<(PhpValue, PhpValue)> = map
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        shuffle_slice(&mut entries);
                        let mut out = HashMap::new();
                        for (k, v) in entries {
                            out.insert(k, v);
                        }
                        Ok(PhpValue::hash_map_from_map(out))
                    }
                    "randomKey" | "randomKeys" | "randKey" | "array_rand" => {
                        let keys: Vec<PhpValue> = map.borrow().keys().cloned().collect();
                        if keys.is_empty() {
                            return Ok(PhpValue::Null);
                        }
                        let num = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as usize;
                        if num < 1 || num > keys.len() {
                            return Err(self.make_error(
                                RuntimeErrorKind::ValueError {
                                    message: "array_rand(): Argument #2 ($num) must be between 1 and the number of elements or greater in array".to_string(),
                                },
                                span,
                            ));
                        }
                        if num == 1 {
                            let idx = random_usize(keys.len());
                            Ok(keys[idx].clone())
                        } else {
                            let mut indices: Vec<usize> = (0..keys.len()).collect();
                            shuffle_slice(&mut indices);
                            indices.truncate(num);
                            let picked: Vec<PhpValue> = indices.into_iter().map(|i| keys[i].clone()).collect();
                            Ok(PhpValue::list(picked))
                        }
                    }
                    "random" => {
                        let entries: Vec<(PhpValue, PhpValue)> = map
                            .borrow()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        if entries.is_empty() {
                            return Ok(PhpValue::Null);
                        }
                        let num_opt = args.first().and_then(|v| if v.is_null() { None } else { v.to_i64() });
                        match num_opt {
                            None => {
                                let idx = random_usize(entries.len());
                                Ok(entries[idx].1.clone())
                            }
                            Some(num) => {
                                let n = num as usize;
                                if n < 1 || n > entries.len() {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::ValueError {
                                            message: format!("Arr::random(): requested {} elements, but array contains only {}", n, entries.len()),
                                        },
                                        span,
                                    ));
                                }
                                let mut indices: Vec<usize> = (0..entries.len()).collect();
                                shuffle_slice(&mut indices);
                                indices.truncate(n);
                                let mut out = HashMap::new();
                                for i in indices {
                                    out.insert(entries[i].0.clone(), entries[i].1.clone());
                                }
                                Ok(PhpValue::hash_map_from_map(out))
                            }
                        }
                    }
                    "first" => Ok(map.borrow().values().next().cloned().unwrap_or(PhpValue::Null)),
                    "each" | "forEach" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let entries: Vec<(PhpValue, PhpValue)> = map
                                .borrow()
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            for (k, v) in entries {
                                self.invoke_closure_sync(cb, vec![v, k], span)?;
                            }
                        }
                        Ok(receiver.clone())
                    }
                    "toJson" => Ok(PhpValue::string(php_value_to_json(receiver))),
                    other => {
                        if let Some(macro_closure) = get_runtime_closure_macro("Arr", other) {
                            self.invoke_closure_sync_with_this(
                                &macro_closure,
                                Some(receiver.clone()),
                                args.to_vec(),
                                span,
                            )
                        } else {
                            Err(self.make_error(
                                RuntimeErrorKind::UndefinedMethod {
                                    class: "Arr::hashMap".to_string(),
                                    method: other.to_string(),
                                },
                                span,
                            ))
                        }
                    }
                }
            }

            // 4. Sequential list & generic collection methods (`Arr::list()->map()->filter()->reduce()`, etc.)
            PhpValue::EmptyArray
            | PhpValue::List(_)
            | PhpValue::Array(_) => {
                let items = Self::extract_items(receiver);
                match method {
                    "check" => {
                        let ok = match args.first() {
                            None => true,
                            Some(sub) => {
                                let clean = sub.to_php_string().trim().to_lowercase().replace(['-', '_', ' '], "");
                                if matches!(receiver, PhpValue::EmptyArray) {
                                    matches!(clean.as_str(), "list" | "dict" | "dictionary" | "array")
                                } else {
                                    clean == "list" || clean == "array"
                                }
                            }
                        };
                        Ok(PhpValue::Bool(ok))
                    }
                    "keys" => {
                        let keys: Vec<PhpValue> = (0..items.len())
                            .map(|i| PhpValue::Int(i as i64))
                            .collect();
                        Ok(PhpValue::list(keys))
                    }
                    "map" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let mut out = Vec::with_capacity(items.len());
                            for (idx, item) in items.into_iter().enumerate() {
                                out.push(self.invoke_closure_sync(
                                    cb,
                                    vec![item, PhpValue::Int(idx as i64)],
                                    span,
                                )?);
                            }
                            Ok(PhpValue::list(out))
                        } else {
                            Ok(PhpValue::list(items))
                        }
                    }
                    "filter" => {
                        let mut out = Vec::new();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (idx, item) in items.into_iter().enumerate() {
                                let keep = self.invoke_closure_sync(
                                    cb,
                                    vec![item.clone(), PhpValue::Int(idx as i64)],
                                    span,
                                )?;
                                if keep.is_truthy() {
                                    out.push(item);
                                }
                            }
                        } else {
                            out.extend(items.into_iter().filter(|i| i.is_truthy()));
                        }
                        Ok(PhpValue::list(out))
                    }
                    "reject" => {
                        let mut out = Vec::new();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (idx, item) in items.into_iter().enumerate() {
                                let drop = self.invoke_closure_sync(
                                    cb,
                                    vec![item.clone(), PhpValue::Int(idx as i64)],
                                    span,
                                )?;
                                if !drop.is_truthy() {
                                    out.push(item);
                                }
                            }
                        }
                        Ok(PhpValue::list(out))
                    }
                    "reduce" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let mut acc = args.get(1).cloned().unwrap_or(PhpValue::Int(0));
                            for item in items {
                                acc = self.invoke_closure_sync(cb, vec![acc, item], span)?;
                            }
                            Ok(acc)
                        } else {
                            Ok(PhpValue::Null)
                        }
                    }
                    "find" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (idx, item) in items.into_iter().enumerate() {
                                let matched = self.invoke_closure_sync(
                                    cb,
                                    vec![item.clone(), PhpValue::Int(idx as i64)],
                                    span,
                                )?;
                                if matched.is_truthy() {
                                    return Ok(item);
                                }
                            }
                        }
                        Ok(PhpValue::Null)
                    }
                    "first" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (idx, item) in items.into_iter().enumerate() {
                                let matched = self.invoke_closure_sync(
                                    cb,
                                    vec![item.clone(), PhpValue::Int(idx as i64)],
                                    span,
                                )?;
                                if matched.is_truthy() {
                                    return Ok(item);
                                }
                            }
                            Ok(PhpValue::Null)
                        } else {
                            Ok(items.into_iter().next().unwrap_or(PhpValue::Null))
                        }
                    }
                    "last" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let mut last_match = PhpValue::Null;
                            for (idx, item) in items.into_iter().enumerate() {
                                let matched = self.invoke_closure_sync(
                                    cb,
                                    vec![item.clone(), PhpValue::Int(idx as i64)],
                                    span,
                                )?;
                                if matched.is_truthy() {
                                    last_match = item;
                                }
                            }
                            Ok(last_match)
                        } else {
                            Ok(items.into_iter().last().unwrap_or(PhpValue::Null))
                        }
                    }
                    "count" | "size" | "length" | "len" => Ok(PhpValue::Int(items.len() as i64)),
                    "isEmpty" => Ok(PhpValue::Bool(items.is_empty())),
                    "isNotEmpty" => Ok(PhpValue::Bool(!items.is_empty())),
                    "all" | "toArray" | "values" => Ok(PhpValue::list(items)),
                    "contains" | "includes" | "hasValue" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (idx, item) in items.into_iter().enumerate() {
                                let matched = self.invoke_closure_sync(
                                    cb,
                                    vec![item, PhpValue::Int(idx as i64)],
                                    span,
                                )?;
                                if matched.is_truthy() {
                                    return Ok(PhpValue::Bool(true));
                                }
                            }
                            Ok(PhpValue::Bool(false))
                        } else {
                            let needle = args.first().cloned().unwrap_or(PhpValue::Null);
                            Ok(PhpValue::Bool(items.contains(&needle)))
                        }
                    }
                    "join" | "implode" => {
                        let sep = args
                            .first()
                            .map(|v| v.to_php_string())
                            .unwrap_or_else(|| ",".to_string());
                        let joined = items
                            .iter()
                            .map(|v| v.to_php_string())
                            .collect::<Vec<_>>()
                            .join(&sep);
                        Ok(PhpValue::string(joined))
                    }
                    "push" | "add" => {
                        let mut updated = items;
                        for a in args {
                            if let Some(first) = updated.first() {
                                if !first.same_type_as(a) {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::TypeError {
                                            message: format!(
                                                "All elements in a list must have the same type, found {} and {}",
                                                first.type_name(),
                                                a.type_name()
                                            ),
                                        },
                                        span,
                                    ));
                                }
                            }
                            updated.push(a.clone());
                        }
                        if let PhpValue::List(rc) | PhpValue::Array(rc) = receiver {
                            *rc.borrow_mut() = updated.clone();
                        }
                        Ok(PhpValue::list(updated))
                    }
                    "pop" => {
                        if let PhpValue::List(rc) | PhpValue::Array(rc) = receiver {
                            Ok(rc.borrow_mut().pop().unwrap_or(PhpValue::Null))
                        } else {
                            let mut it = items;
                            Ok(it.pop().unwrap_or(PhpValue::Null))
                        }
                    }
                    "remove" | "forget" | "delete" => {
                        let idx = args.first().and_then(|v| v.to_i64()).unwrap_or(-1);
                        if let PhpValue::List(rc) | PhpValue::Array(rc) = receiver {
                            let mut b = rc.borrow_mut();
                            if idx >= 0 && (idx as usize) < b.len() {
                                b.remove(idx as usize);
                            }
                        }
                        Ok(receiver.clone())
                    }
                    "shift" => {
                        if let PhpValue::List(rc) | PhpValue::Array(rc) = receiver {
                            let mut b = rc.borrow_mut();
                            if !b.is_empty() {
                                Ok(b.remove(0))
                            } else {
                                Ok(PhpValue::Null)
                            }
                        } else {
                            let mut it = items;
                            if !it.is_empty() {
                                Ok(it.remove(0))
                            } else {
                                Ok(PhpValue::Null)
                            }
                        }
                    }
                    "unshift" => {
                        for a in args {
                            if let Some(first) = items.first() {
                                if !first.same_type_as(a) {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::TypeError {
                                            message: format!(
                                                "All elements in a list must have the same type, found {} and {}",
                                                first.type_name(),
                                                a.type_name()
                                            ),
                                        },
                                        span,
                                    ));
                                }
                            }
                        }
                        let mut updated: Vec<PhpValue> = args.to_vec();
                        updated.extend(items);
                        if let PhpValue::List(rc) | PhpValue::Array(rc) = receiver {
                            *rc.borrow_mut() = updated.clone();
                        }
                        Ok(PhpValue::list(updated))
                    }
                    "slice" => {
                        let offset = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
                        let len_req = args.get(1).and_then(|v| v.to_i64());
                        let total_len = items.len() as i64;
                        let start_idx = if offset < 0 {
                            (total_len + offset).max(0) as usize
                        } else {
                            offset.min(total_len) as usize
                        };
                        let slice: Vec<PhpValue> = if let Some(l) = len_req {
                            let count = if l < 0 {
                                (total_len - start_idx as i64 + l).max(0) as usize
                            } else {
                                l as usize
                            };
                            items.into_iter().skip(start_idx).take(count).collect()
                        } else {
                            items.into_iter().skip(start_idx).collect()
                        };
                        Ok(PhpValue::list(slice))
                    }
                    "take" => {
                        let limit = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
                        let out: Vec<PhpValue> = if limit >= 0 {
                            items.into_iter().take(limit as usize).collect()
                        } else {
                            let count = (-limit) as usize;
                            let skip_n = items.len().saturating_sub(count);
                            items.into_iter().skip(skip_n).collect()
                        };
                        Ok(PhpValue::list(out))
                    }
                    "skip" => {
                        let count = args.first().and_then(|v| v.to_i64()).unwrap_or(0).max(0) as usize;
                        Ok(PhpValue::list(items.into_iter().skip(count).collect()))
                    }
                    "chunk" | "array_chunk" => {
                        let size = args.first().and_then(|v| v.to_i64()).unwrap_or(1).max(1) as usize;
                        let chunks: Vec<PhpValue> = items
                            .chunks(size)
                            .map(|c| PhpValue::list(c.to_vec()))
                            .collect();
                        Ok(PhpValue::list(chunks))
                    }
                    "diff" | "array_diff" => {
                        let mut exclude = HashSet::new();
                        for arg in args {
                            for item in Self::extract_items(arg) {
                                exclude.insert(item);
                            }
                        }
                        let diffed: Vec<PhpValue> = items
                            .into_iter()
                            .filter(|it| !exclude.contains(it))
                            .collect();
                        Ok(PhpValue::list(diffed))
                    }
                    "diffKey" | "diffKeys" | "array_diff_key" => {
                        let mut exclude_keys = HashSet::new();
                        for arg in args {
                            match arg {
                                PhpValue::Dict(d) => {
                                    for k in d.borrow().keys() {
                                        exclude_keys.insert(k.clone());
                                    }
                                }
                                PhpValue::List(l) | PhpValue::Array(l) => {
                                    for i in 0..l.borrow().len() {
                                        exclude_keys.insert(i.to_string());
                                    }
                                }
                                PhpValue::HashMap(m) => {
                                    for k in m.borrow().keys() {
                                        exclude_keys.insert(k.to_php_string());
                                    }
                                }
                                PhpValue::Set(s) => {
                                    for k in s.borrow().iter() {
                                        exclude_keys.insert(k.to_php_string());
                                    }
                                }
                                _ => {}
                            }
                        }
                        let diffed: Vec<PhpValue> = items
                            .into_iter()
                            .enumerate()
                            .filter(|(idx, _)| !exclude_keys.contains(&idx.to_string()))
                            .map(|(_, it)| it)
                            .collect();
                        Ok(PhpValue::list(diffed))
                    }
                    "intersect" | "array_intersect" => {
                        let other_sets: Vec<HashSet<PhpValue>> = args
                            .iter()
                            .map(|arg| Self::extract_items(arg).into_iter().collect())
                            .collect();
                        let intersected: Vec<PhpValue> = items
                            .into_iter()
                            .filter(|it| other_sets.iter().all(|set| set.contains(it)))
                            .collect();
                        Ok(PhpValue::list(intersected))
                    }
                    "intersectKey" | "intersectKeys" | "array_intersect_key" => {
                        let other_key_sets: Vec<HashSet<String>> = args
                            .iter()
                            .map(|arg| match arg {
                                PhpValue::Dict(d) => d.borrow().keys().cloned().collect(),
                                PhpValue::List(l) | PhpValue::Array(l) => {
                                    (0..l.borrow().len()).map(|i| i.to_string()).collect()
                                }
                                PhpValue::HashMap(m) => {
                                    m.borrow().keys().map(|k| k.to_php_string()).collect()
                                }
                                PhpValue::Set(s) => {
                                    s.borrow().iter().map(|k| k.to_php_string()).collect()
                                }
                                _ => HashSet::new(),
                            })
                            .collect();
                        let intersected: Vec<PhpValue> = items
                            .into_iter()
                            .enumerate()
                            .filter(|(idx, _)| {
                                let idx_str = idx.to_string();
                                other_key_sets.iter().all(|set| set.contains(&idx_str))
                            })
                            .map(|(_, it)| it)
                            .collect();
                        Ok(PhpValue::list(intersected))
                    }
                    "flatten" => {
                        let mut out = Vec::new();
                        for item in items {
                            flatten_elements(&item, &mut out);
                        }
                        Ok(PhpValue::list(out))
                    }
                    "concat" | "merge" => {
                        let mut merged = items;
                        for arg in args {
                            merged.extend(Self::extract_items(arg));
                        }
                        Ok(PhpValue::list(merged))
                    }
                    "get" => {
                        let idx = args.first().and_then(|v| v.to_i64()).unwrap_or(-1);
                        let default = args.get(1).cloned().unwrap_or(PhpValue::Null);
                        if idx >= 0 && (idx as usize) < items.len() {
                            Ok(items[idx as usize].clone())
                        } else {
                            Ok(default)
                        }
                    }
                    "has" | "hasKey" => {
                        let idx = args.first().and_then(|v| v.to_i64()).unwrap_or(-1);
                        Ok(PhpValue::Bool(idx >= 0 && (idx as usize) < items.len()))
                    }
                    "sum" => {
                        let mut is_float = false;
                        let mut float_sum = 0.0f64;
                        let mut int_sum = 0i64;
                        for item in &items {
                            if let Some(fl) = item.to_f64() {
                                if item.to_i64().is_none() || matches!(item, PhpValue::Float(_)) {
                                    is_float = true;
                                }
                                float_sum += fl;
                            }
                            if let Some(i) = item.to_i64() {
                                int_sum += i;
                            }
                        }
                        if is_float {
                            Ok(PhpValue::float(float_sum))
                        } else {
                            Ok(PhpValue::Int(int_sum))
                        }
                    }
                    "avg" | "average" => {
                        if items.is_empty() {
                            Ok(PhpValue::float(0.0))
                        } else {
                            let mut sum = 0.0f64;
                            for item in &items {
                                sum += item.to_f64().unwrap_or(0.0);
                            }
                            Ok(PhpValue::float(sum / items.len() as f64))
                        }
                    }
                    "min" => {
                        let mut min_val: Option<PhpValue> = None;
                        for item in items {
                            min_val = match min_val {
                                None => Some(item),
                                Some(prev) => {
                                    if item.partial_cmp(&prev) == Some(std::cmp::Ordering::Less) {
                                        Some(item)
                                    } else {
                                        Some(prev)
                                    }
                                }
                            };
                        }
                        Ok(min_val.unwrap_or(PhpValue::Null))
                    }
                    "max" => {
                        let mut max_val: Option<PhpValue> = None;
                        for item in items {
                            max_val = match max_val {
                                None => Some(item),
                                Some(prev) => {
                                    if item.partial_cmp(&prev) == Some(std::cmp::Ordering::Greater) {
                                        Some(item)
                                    } else {
                                        Some(prev)
                                    }
                                }
                            };
                        }
                        Ok(max_val.unwrap_or(PhpValue::Null))
                    }
                    "unique" => {
                        let mut unique_items: Vec<PhpValue> = Vec::with_capacity(items.len());
                        for item in items {
                            if !unique_items.contains(&item) {
                                unique_items.push(item);
                            }
                        }
                        Ok(PhpValue::list(unique_items))
                    }
                    "sort" | "asort" | "usort" | "uasort" => {
                        let mut sorted = items;
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let len = sorted.len();
                            for i in 0..len {
                                for j in (i + 1)..len {
                                    let cmp_res = self.invoke_closure_sync(
                                        cb,
                                        vec![sorted[i].clone(), sorted[j].clone()],
                                        span,
                                    )?;
                                    if cmp_res.to_i64().unwrap_or(0) > 0 {
                                        sorted.swap(i, j);
                                    }
                                }
                            }
                        } else {
                            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                        }
                        Ok(PhpValue::list(sorted))
                    }
                    "natsort" | "natSort" | "sortNatural" => {
                        let mut sorted = items;
                        sorted.sort_by(|a, b| php_value_natural_cmp(a, b, false));
                        Ok(PhpValue::list(sorted))
                    }
                    "natcasesort" | "natCaseSort" | "sortNaturalCase" => {
                        let mut sorted = items;
                        sorted.sort_by(|a, b| php_value_natural_cmp(a, b, true));
                        Ok(PhpValue::list(sorted))
                    }
                    "sortBy" => {
                        let mut sorted = items;
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            let len = sorted.len();
                            for i in 0..len {
                                for j in (i + 1)..len {
                                    let ki = self.invoke_closure_sync(cb, vec![sorted[i].clone()], span)?;
                                    let kj = self.invoke_closure_sync(cb, vec![sorted[j].clone()], span)?;
                                    if ki.partial_cmp(&kj) == Some(std::cmp::Ordering::Greater) {
                                        sorted.swap(i, j);
                                    }
                                }
                            }
                        } else if let Some(key_val) = args.first() {
                            let key_str = key_val.to_php_string();
                            sorted.sort_by(|a, b| {
                                let ka = match a {
                                    PhpValue::Object(o) => {
                                        o.borrow().get_property(&key_str).cloned().unwrap_or(PhpValue::Null)
                                    }
                                    PhpValue::Dict(d) => d
                                        .borrow()
                                        .get(&key_str)
                                        .cloned()
                                        .unwrap_or(PhpValue::Null),
                                    _ => PhpValue::Null,
                                };
                                let kb = match b {
                                    PhpValue::Object(o) => {
                                        o.borrow().get_property(&key_str).cloned().unwrap_or(PhpValue::Null)
                                    }
                                    PhpValue::Dict(d) => d
                                        .borrow()
                                        .get(&key_str)
                                        .cloned()
                                        .unwrap_or(PhpValue::Null),
                                    _ => PhpValue::Null,
                                };
                                ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
                            });
                        }
                        Ok(PhpValue::list(sorted))
                    }
                    "each" | "forEach" => {
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (idx, item) in items.iter().enumerate() {
                                self.invoke_closure_sync(
                                    cb,
                                    vec![item.clone(), PhpValue::Int(idx as i64)],
                                    span,
                                )?;
                            }
                        }
                        Ok(receiver.clone())
                    }
                    "pluck" => {
                        let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                        let mut plucked = Vec::with_capacity(items.len());
                        for item in items {
                            match item {
                                PhpValue::Object(obj) => {
                                    let val = obj
                                        .borrow()
                                        .get_property(&key)
                                        .cloned()
                                        .unwrap_or(PhpValue::Null);
                                    plucked.push(val);
                                }
                                PhpValue::Dict(d) => {
                                    let val = d.borrow().get(&key).cloned().unwrap_or(PhpValue::Null);
                                    plucked.push(val);
                                }
                                _ => plucked.push(PhpValue::Null),
                            }
                        }
                        Ok(PhpValue::list(plucked))
                    }
                    "column" | "array_column" => {
                        let col_opt = args.first();
                        let is_col_null = col_opt.map_or(true, |v| matches!(v, PhpValue::Null));
                        let col_name = col_opt.map(|v| v.to_php_string()).unwrap_or_default();
                        let index_key_opt = args.get(1);

                        if let Some(idx_key_val) = index_key_opt {
                            if !matches!(idx_key_val, PhpValue::Null) {
                                let idx_name = idx_key_val.to_php_string();
                                let mut map = IndexMap::new();
                                for item in items {
                                    let k = get_item_column(&item, &idx_name)
                                        .map(|v| v.to_php_string())
                                        .unwrap_or_default();
                                    let v = if is_col_null {
                                        item
                                    } else {
                                        get_item_column(&item, &col_name).unwrap_or(PhpValue::Null)
                                    };
                                    map.insert(k, v);
                                }
                                return Ok(PhpValue::dict_from_map(map));
                            }
                        }

                        let mut plucked = Vec::with_capacity(items.len());
                        for item in items {
                            let v = if is_col_null {
                                item
                            } else {
                                get_item_column(&item, &col_name).unwrap_or(PhpValue::Null)
                            };
                            plucked.push(v);
                        }
                        Ok(PhpValue::list(plucked))
                    }
                    "flip" | "array_flip" => {
                        let mut out = IndexMap::new();
                        for (idx, val) in items.into_iter().enumerate() {
                            out.insert(val.to_php_string(), PhpValue::Int(idx as i64));
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "combine" | "array_combine" => {
                        let vals_arg = args.first().unwrap_or(&PhpValue::Null);
                        let vals = Self::extract_items(vals_arg);
                        if items.len() != vals.len() {
                            return Err(self.make_error(
                                RuntimeErrorKind::ValueError {
                                    message: "array_combine(): Argument #1 ($keys) and argument #2 ($values) must have the same number of elements".to_string(),
                                },
                                span,
                            ));
                        }
                        let mut map = IndexMap::new();
                        for (k, v) in items.into_iter().zip(vals) {
                            map.insert(k.to_php_string(), v);
                        }
                        Ok(PhpValue::dict_from_map(map))
                    }
                    "countValues" | "count_values" | "array_count_values" | "countBy" => {
                        let mut counts = IndexMap::new();
                        for v in items {
                            let key = if let Some(PhpValue::Closure(cb)) = args.first() {
                                self.invoke_closure_sync(cb, vec![v.clone()], span)?.to_php_string()
                            } else if let Some(prop) = args.first() {
                                let prop_str = prop.to_php_string();
                                get_item_column(&v, &prop_str).unwrap_or(PhpValue::Null).to_php_string()
                            } else {
                                v.to_php_string()
                            };
                            let entry = counts.entry(key).or_insert(0i32);
                            *entry += 1;
                        }
                        let mut out = IndexMap::new();
                        for (k, cnt) in counts {
                            out.insert(k, PhpValue::Int((cnt) as i64));
                        }
                        Ok(PhpValue::dict_from_map(out))
                    }
                    "shuffle" => {
                        let mut shuffled = items;
                        shuffle_slice(&mut shuffled);
                        Ok(PhpValue::list(shuffled))
                    }
                    "randomKey" | "randomKeys" | "randKey" | "array_rand" => {
                        if items.is_empty() {
                            return Ok(PhpValue::Null);
                        }
                        let num = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as usize;
                        if num < 1 || num > items.len() {
                            return Err(self.make_error(
                                RuntimeErrorKind::ValueError {
                                    message: "array_rand(): Argument #2 ($num) must be between 1 and the number of elements or greater in array".to_string(),
                                },
                                span,
                            ));
                        }
                        if num == 1 {
                            let idx = random_usize(items.len());
                            Ok(PhpValue::Int(idx as i64))
                        } else {
                            let mut indices: Vec<usize> = (0..items.len()).collect();
                            shuffle_slice(&mut indices);
                            indices.truncate(num);
                            indices.sort();
                            let picked: Vec<PhpValue> = indices.into_iter().map(|i| PhpValue::Int(i as i64)).collect();
                            Ok(PhpValue::list(picked))
                        }
                    }
                    "random" => {
                        if items.is_empty() {
                            return Ok(PhpValue::Null);
                        }
                        let num_opt = args.first().and_then(|v| if v.is_null() { None } else { v.to_i64() });
                        match num_opt {
                            None => {
                                let idx = random_usize(items.len());
                                Ok(items[idx].clone())
                            }
                            Some(num) => {
                                let n = num as usize;
                                if n < 1 || n > items.len() {
                                    return Err(self.make_error(
                                        RuntimeErrorKind::ValueError {
                                            message: format!("Arr::random(): requested {} elements, but array contains only {}", n, items.len()),
                                        },
                                        span,
                                    ));
                                }
                                let mut indices: Vec<usize> = (0..items.len()).collect();
                                shuffle_slice(&mut indices);
                                indices.truncate(n);
                                let picked: Vec<PhpValue> = indices.into_iter().map(|i| items[i].clone()).collect();
                                Ok(PhpValue::list(picked))
                            }
                        }
                    }
                    "partition" => {
                        let mut matching = Vec::new();
                        let mut non_matching = Vec::new();
                        if let Some(PhpValue::Closure(cb)) = args.first() {
                            for (idx, item) in items.into_iter().enumerate() {
                                let res = self.invoke_closure_sync(
                                    cb,
                                    vec![item.clone(), PhpValue::Int(idx as i64)],
                                    span,
                                )?;
                                if res.is_truthy() {
                                    matching.push(item);
                                } else {
                                    non_matching.push(item);
                                }
                            }
                        }
                        Ok(PhpValue::list(vec![
                            PhpValue::list(matching),
                            PhpValue::list(non_matching),
                        ]))
                    }
                    "zip" => {
                        let other_lists: Vec<Vec<PhpValue>> =
                            args.iter().map(Self::extract_items).collect();
                        let mut zipped = Vec::with_capacity(items.len());
                        for (i, item) in items.into_iter().enumerate() {
                            let mut tuple = vec![item];
                            for other in &other_lists {
                                tuple.push(other.get(i).cloned().unwrap_or(PhpValue::Null));
                            }
                            zipped.push(PhpValue::list(tuple));
                        }
                        Ok(PhpValue::list(zipped))
                    }
                    "reverse" => {
                        let mut rev = items;
                        rev.reverse();
                        Ok(PhpValue::list(rev))
                    }
                    "toJson" => Ok(PhpValue::string(php_value_to_json(receiver))),
                    other => {
                        if let Some(macro_closure) = get_runtime_closure_macro("Arr", other) {
                            self.invoke_closure_sync_with_this(
                                &macro_closure,
                                Some(receiver.clone()),
                                args.to_vec(),
                                span,
                            )
                        } else {
                            Err(self.make_error(
                                RuntimeErrorKind::UndefinedMethod {
                                    class: receiver.type_name().to_string(),
                                    method: other.to_string(),
                                },
                                span,
                            ))
                        }
                    }
                }
            }

            // 2. Integer fluent methods. The result is always i64.
            value if value.is_integer_value() => {
                let val = crate::vm::numeric::int_operand(value)
                    .map_err(|fail| self.arith_error(fail, span))?;
                let rhs_int = |default: i64| -> Result<i64, RuntimeError> {
                    match args.first() {
                        Some(v) => crate::vm::numeric::int_operand(v)
                            .map_err(|fail| self.arith_error(fail, span)),
                        None => Ok(default),
                    }
                };
                let next_val = match method {
                    "round" | "floor" | "ceil" | "ciel" => Ok(val),
                    "add" => crate::vm::numeric::checked_int_add(val, rhs_int(0)?),
                    "subtract" | "sub" => crate::vm::numeric::checked_int_op(
                        BinaryOp::Sub,
                        val,
                        rhs_int(0)?,
                    ),
                    "multiply" | "mul" => crate::vm::numeric::checked_int_op(
                        BinaryOp::Mul,
                        val,
                        rhs_int(1)?,
                    ),
                    "divide" | "div" => crate::vm::numeric::checked_int_op(
                        BinaryOp::Div,
                        val,
                        rhs_int(0)?,
                    ),
                    "abs" => val.checked_abs().ok_or(crate::vm::numeric::ArithFail::Overflow),
                    "toInt" => return Ok(PhpValue::Int(val)),
                    "toFloat" => return Ok(PhpValue::float(val as f64)),
                    "toString" => return Ok(PhpValue::string(val.to_string())),
                    other => {
                        return Err(self.make_error(
                            RuntimeErrorKind::UndefinedMethod {
                                class: "int".to_string(),
                                method: other.to_string(),
                            },
                            span,
                        ))
                    }
                };
                let next_val = next_val.map_err(|fail| self.arith_error(fail, span))?;
                Ok(PhpValue::Int(next_val))
            }

            value if crate::vm::numeric::is_float_like(value) => {
                let val = value.to_f64().unwrap_or(0.0);
                let extract_f64 = |pv: &PhpValue| pv.to_f64().unwrap_or(0.0);
                match method {
                    // Binary & Decimal format operations (IEEE-754 64-bit float)
                    "bAdd" | "add" => {
                        let a = extract_f64(receiver);
                        let b = args.first().map(extract_f64).unwrap_or(0.0);
                        return Ok(PhpValue::float(a + b));
                    }
                    "bSub" | "subtract" | "sub" => {
                        let a = extract_f64(receiver);
                        let b = args.first().map(extract_f64).unwrap_or(0.0);
                        return Ok(PhpValue::float(a - b));
                    }
                    "bMul" | "multiply" | "mul" => {
                        let a = extract_f64(receiver);
                        let b = args.first().map(extract_f64).unwrap_or(1.0);
                        return Ok(PhpValue::float(a * b));
                    }
                    "bDiv" | "divide" | "div" => {
                        let a = extract_f64(receiver);
                        let b = args.first().map(extract_f64).unwrap_or(0.0);
                        if b == 0.0 {
                            return Err(self.make_error(RuntimeErrorKind::DivisionByZero, span));
                        }
                        return Ok(PhpValue::float(a / b));
                    }
                    "round" => {
                        let places = args.first().and_then(|v| v.to_i32()).unwrap_or(0).clamp(0, 15);
                        let factor = 10_f64.powi(places);
                        return Ok(PhpValue::float((val * factor).round() / factor));
                    }
                    "toDecimal" => {
                        let places = args.first().and_then(|v| v.to_i32()).unwrap_or(2).clamp(0, 15);
                        let factor = 10_f64.powi(places);
                        return Ok(PhpValue::float((val * factor).round() / factor));
                    }
                    "floor" => return Ok(PhpValue::float(val.floor())),
                    "ceil" | "ciel" => return Ok(PhpValue::float(val.ceil())),
                    "abs" => return Ok(PhpValue::float(val.abs())),
                    "toInt" => return Ok(PhpValue::Int(val as i64)),
                    "toFloat" => return Ok(PhpValue::float(val)),
                    "toString" => return Ok(PhpValue::string(val.to_string())),
                    other => {
                        return Err(self.make_error(
                            RuntimeErrorKind::UndefinedMethod {
                                class: "float".to_string(),
                                method: other.to_string(),
                            },
                            span,
                        ))
                    }
                }
            }

            // 4. Future fluent methods (`->wait()`, `->then()`, `->catch()`, `->isCompleted()`, etc.)
            PhpValue::Future(fut_rc) => match method {
                "wait" => {
                    let mut b = fut_rc.borrow_mut();
                    match b.wait_result() {
                        Ok(val) => Ok(val),
                        Err(err) => Err(self.make_error(
                            RuntimeErrorKind::Custom { message: err },
                            span,
                        )),
                    }
                }
                "then" => {
                    let mut b = fut_rc.borrow_mut();
                    let val = match b.wait_result() {
                        Ok(v) => v,
                        Err(err) => {
                            return Err(self.make_error(
                                RuntimeErrorKind::Custom { message: err },
                                span,
                            ));
                        }
                    };
                    drop(b);
                    if let Some(PhpValue::Closure(cb)) = args.first() {
                        let mapped = self.invoke_closure_sync(cb, vec![val], span)?;
                        Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::resolved(
                            mapped,
                        )))))
                    } else {
                        Ok(receiver.clone())
                    }
                }
                "catch" | "catchError" => {
                    let mut b = fut_rc.borrow_mut();
                    let outcome = b.wait_result();
                    drop(b);
                    match outcome {
                        Ok(_) => Ok(receiver.clone()),
                        Err(err_msg) => {
                            if let Some(PhpValue::Closure(cb)) = args.first() {
                                let recovered = self.invoke_closure_sync(cb, vec![PhpValue::string(err_msg)], span)?;
                                Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::resolved(recovered)))))
                            } else {
                                Ok(receiver.clone())
                            }
                        }
                    }
                }
                "isCompleted" => {
                    let mut b = fut_rc.borrow_mut();
                    let completed = b.poll_completion();
                    Ok(PhpValue::Bool(completed))
                }
                "state" | "getState" | "status" => {
                    let mut b = fut_rc.borrow_mut();
                    Ok(PhpValue::string(b.state().as_str()))
                }
                "isPending" => {
                    let mut b = fut_rc.borrow_mut();
                    Ok(PhpValue::Bool(!b.poll_completion()))
                }
                "isReady" => {
                    let mut b = fut_rc.borrow_mut();
                    Ok(PhpValue::Bool(b.poll_completion()))
                }
                "poll" => {
                    let mut b = fut_rc.borrow_mut();
                    let is_ready = b.poll_completion();
                    let mut dict = indexmap::IndexMap::new();
                    if is_ready {
                        dict.insert("state".to_string(), PhpValue::string("Ready"));
                        if let Some(ref err) = b.error {
                            dict.insert("error".to_string(), PhpValue::string(err.clone()));
                        } else {
                            let val = b.value.clone().unwrap_or(PhpValue::Null);
                            dict.insert("value".to_string(), val);
                        }
                    } else {
                        dict.insert("state".to_string(), PhpValue::string("Pending"));
                    }
                    Ok(PhpValue::dict_from_map(dict))
                }
                "hasError" => {
                    let mut b = fut_rc.borrow_mut();
                    b.poll_completion();
                    Ok(PhpValue::Bool(b.error.is_some()))
                }
                "value" => {
                    let mut b = fut_rc.borrow_mut();
                    b.poll_completion();
                    Ok(b.value.clone().unwrap_or(PhpValue::Null))
                }
                "error" => {
                    let mut b = fut_rc.borrow_mut();
                    b.poll_completion();
                    match &b.error {
                        Some(e) => Ok(PhpValue::string(e.clone())),
                        None => Ok(PhpValue::Null),
                    }
                }
                "timeout" => {
                    let ms = args.first().and_then(|v| v.to_i64()).unwrap_or(0).max(0) as u64;
                    let duration = std::time::Duration::from_millis(ms);
                    let mut b = fut_rc.borrow_mut();
                    let res = b.wait_timeout(duration);
                    drop(b);
                    match res {
                        Ok(val) => Ok(val),
                        Err(err) => {
                            if let Some(PhpValue::Closure(on_timeout)) = args.get(1) {
                                self.invoke_closure_sync(on_timeout, Vec::new(), span)
                            } else {
                                Err(self.make_error(
                                    RuntimeErrorKind::Custom { message: err },
                                    span,
                                ))
                            }
                        }
                    }
                }
                other => Err(self.make_error(
                    RuntimeErrorKind::UndefinedMethod {
                        class: "Future".to_string(),
                        method: other.to_string(),
                    },
                    span,
                )),
            },

            // 6. Binary-Safe String fluent methods ($text->trim()->upper(), etc.)
            // String helper methods are immutable by default, returning a new instance.
            PhpValue::String(s) if s.is_bytes() => match method {
                "len" | "length" => Ok(PhpValue::Int(s.len() as i64)),
                "slice" => {
                    let start = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
                    let length = args.get(1).and_then(|v| if v.is_null() { None } else { v.to_i64() });
                    let b = s.as_bytes();
                    let (st, en) = crate::vm::encoding::calc_mb_slice(b.len() as i64, start, length);
                    Ok(PhpValue::bytes(&b[st..en]))
                }
                "decode" => {
                    let encoding = args.first().and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) }).unwrap_or_else(|| "UTF-8".to_string());
                    let invalid = args.get(1).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) }).unwrap_or_else(|| "strict".to_string());
                    let decoded = crate::vm::encoding::decode_bytes(s.as_bytes(), &encoding, &invalid)
                        .map_err(|(msg, _)| self.make_error(RuntimeErrorKind::UncaughtException {
                            class: "InvalidEncodingException".to_string(),
                            message: msg,
                        }, span))?;
                    Ok(PhpValue::string(decoded))
                }
                "convert" => {
                    let to = args.first().ok_or_else(|| self.make_error(RuntimeErrorKind::TooFewArguments {
                        callable: "Bytes::convert".to_string(),
                        passed: args.len(),
                        expected: 2,
                    }, span))?.to_php_string();
                    let from = args.get(1).ok_or_else(|| self.make_error(RuntimeErrorKind::TooFewArguments {
                        callable: "Bytes::convert".to_string(),
                        passed: args.len(),
                        expected: 2,
                    }, span))?.to_php_string();
                    let converted = crate::vm::encoding::convert_encoding(s.as_bytes(), &to, Some(&from), "throw")
                        .map_err(|(cls, msg)| self.make_error(RuntimeErrorKind::UncaughtException { class: cls, message: msg }, span))?;
                    Ok(PhpValue::bytes(converted))
                }
                _ => Err(self.make_error(
                    RuntimeErrorKind::TypeError {
                        message: format!("Method {}() is not supported on bytes. Call ->decode() first to get a string.", method),
                    },
                    span,
                )),
            },

            PhpValue::String(s) => match method {
                "mbStrlen" | "mb_strlen" => {
                    let enc = args.first().and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
                    if enc.as_deref().map(|e| e.eq_ignore_ascii_case("UTF-8") || e.eq_ignore_ascii_case("UTF8")).unwrap_or(true) {
                        if s.is_ascii() {
                            Ok(PhpValue::Int(s.len() as i64))
                        } else {
                            Ok(PhpValue::Int(s.as_str().chars().count() as i64))
                        }
                    } else {
                        let count = crate::vm::encoding::mb_strlen_encoding(s.as_bytes(), &enc.unwrap())
                            .map_err(|(cls, msg)| self.make_error(RuntimeErrorKind::UncaughtException { class: cls, message: msg }, span))?;
                        Ok(PhpValue::Int(count as i64))
                    }
                }
                "graphemeLen" | "grapheme_len" => {
                    let count = crate::vm::encoding::grapheme_len(s.as_str());
                    Ok(PhpValue::Int(count as i64))
                }
                "mbSubstr" | "mb_substr" => {
                    let start = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
                    let len = args.get(1).and_then(|v| if v.is_null() { None } else { v.to_i64() });
                    let enc = args.get(2).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
                    let sub = crate::vm::encoding::mb_substr(s.as_str(), s.is_ascii(), start, len, enc.as_deref())
                        .map_err(|(cls, msg)| self.make_error(RuntimeErrorKind::UncaughtException { class: cls, message: msg }, span))?;
                    Ok(PhpValue::string(sub))
                }
                "len" | "byteLength" | "bytesCount" => {
                    Ok(PhpValue::Int(s.len() as i64))
                }
                "check" => {
                    if let Some(enc_val) = args.first() {
                        let enc_str = enc_val.to_php_string();
                        let is_valid = crate::vm::encoding::is_valid_encoding(s.as_bytes(), &enc_str);
                        Ok(PhpValue::Bool(is_valid))
                    } else {
                        Ok(PhpValue::Bool(true))
                    }
                }
                "compare" => {
                    let other = args.first().unwrap_or(&PhpValue::Null);
                    let case_insensitive = args.get(1).and_then(|v| v.to_bool()).unwrap_or(false);
                    let s1 = s.to_str_lossy();
                    let s2 = other.to_php_string();
                    let eq = if case_insensitive {
                        s1.eq_ignore_ascii_case(&s2)
                    } else {
                        s1 == s2
                    };
                    Ok(PhpValue::Bool(eq))
                }
                "strcmp" | "cmp" | "compareBinary" | "binaryCompare" => {
                    let other = args.first().ok_or_else(|| self.make_error(RuntimeErrorKind::TooFewArguments {
                        callable: "Str::strcmp".to_string(),
                        passed: 1,
                        expected: 2,
                    }, span))?;
                    let b1 = s.as_bytes();
                    let b2 = match other {
                        PhpValue::String(os) => std::borrow::Cow::Borrowed(os.as_bytes()),
                        val => std::borrow::Cow::Owned(val.to_php_string().into_bytes()),
                    };
                    Ok(PhpValue::Int((crate::vm::facade::binary_strcmp(b1, &b2)) as i64))
                }
                "strcasecmp" | "casecmp" | "caseCmp" | "caseCompare" | "compareCase" => {
                    let other = args.first().ok_or_else(|| self.make_error(RuntimeErrorKind::TooFewArguments {
                        callable: "Str::strcasecmp".to_string(),
                        passed: 1,
                        expected: 2,
                    }, span))?;
                    let b1 = s.as_bytes();
                    let b2 = match other {
                        PhpValue::String(os) => std::borrow::Cow::Borrowed(os.as_bytes()),
                        val => std::borrow::Cow::Owned(val.to_php_string().into_bytes()),
                    };
                    Ok(PhpValue::Int((crate::vm::facade::binary_strcasecmp(b1, &b2)) as i64))
                }
                "strncmp" | "ncmp" | "nCmp" | "compareN" | "compareBinaryN" => {
                    let other = args.first().ok_or_else(|| self.make_error(RuntimeErrorKind::TooFewArguments {
                        callable: "Str::strncmp".to_string(),
                        passed: 1,
                        expected: 3,
                    }, span))?;
                    let len_val = args.get(1).ok_or_else(|| self.make_error(RuntimeErrorKind::TooFewArguments {
                        callable: "Str::strncmp".to_string(),
                        passed: 2,
                        expected: 3,
                    }, span))?;
                    let len_i64 = len_val.to_i64().ok_or_else(|| self.make_error(RuntimeErrorKind::TypeError {
                        message: format!("Str::strncmp(): Argument #3 ($length) must be of type int, got {}", len_val.type_name()),
                    }, span))?;
                    if len_i64 < 0 {
                        return Err(self.make_error(RuntimeErrorKind::ValueError {
                            message: "Str::strncmp(): Argument #3 ($length) must be greater than or equal to 0".to_string(),
                        }, span));
                    }
                    let b1 = s.as_bytes();
                    let b2 = match other {
                        PhpValue::String(os) => std::borrow::Cow::Borrowed(os.as_bytes()),
                        val => std::borrow::Cow::Owned(val.to_php_string().into_bytes()),
                    };
                    Ok(PhpValue::Int((crate::vm::facade::binary_strncmp(b1, &b2, len_i64 as usize)) as i64))
                }
                "strncasecmp" | "ncasecmp" | "nCaseCmp" | "caseNCmp" | "caseCompareN" => {
                    let other = args.first().ok_or_else(|| self.make_error(RuntimeErrorKind::TooFewArguments {
                        callable: "Str::strncasecmp".to_string(),
                        passed: 1,
                        expected: 3,
                    }, span))?;
                    let len_val = args.get(1).ok_or_else(|| self.make_error(RuntimeErrorKind::TooFewArguments {
                        callable: "Str::strncasecmp".to_string(),
                        passed: 2,
                        expected: 3,
                    }, span))?;
                    let len_i64 = len_val.to_i64().ok_or_else(|| self.make_error(RuntimeErrorKind::TypeError {
                        message: format!("Str::strncasecmp(): Argument #3 ($length) must be of type int, got {}", len_val.type_name()),
                    }, span))?;
                    if len_i64 < 0 {
                        return Err(self.make_error(RuntimeErrorKind::ValueError {
                            message: "Str::strncasecmp(): Argument #3 ($length) must be greater than or equal to 0".to_string(),
                        }, span));
                    }
                    let b1 = s.as_bytes();
                    let b2 = match other {
                        PhpValue::String(os) => std::borrow::Cow::Borrowed(os.as_bytes()),
                        val => std::borrow::Cow::Owned(val.to_php_string().into_bytes()),
                    };
                    Ok(PhpValue::Int((crate::vm::facade::binary_strncasecmp(b1, &b2, len_i64 as usize)) as i64))
                }
                "digit" | "isDigit" | "is_digit" => {
                    let b = s.as_bytes();
                    Ok(PhpValue::Bool(!b.is_empty() && b.iter().all(|c| c.is_ascii_digit())))
                }
                "alpha" | "isAlpha" | "is_alpha" => {
                    let b = s.as_bytes();
                    Ok(PhpValue::Bool(!b.is_empty() && b.iter().all(|c| c.is_ascii_alphabetic())))
                }
                "alnum" | "isAlnum" | "is_alnum" | "alphaNum" | "alphanumeric" => {
                    let b = s.as_bytes();
                    Ok(PhpValue::Bool(!b.is_empty() && b.iter().all(|c| c.is_ascii_alphanumeric())))
                }
                "trim" => {
                    let str_val = s.to_str_lossy();
                    let chars = if let Some(arg1) = args.first() {
                        arg1.to_php_string()
                    } else {
                        " \t\n\r\0\x0B".to_string()
                    };
                    let trimmed = str_val.trim_matches(|c| chars.contains(c));
                    Ok(PhpValue::string(trimmed))
                }
                "ltrim" => {
                    let str_val = s.to_str_lossy();
                    let chars = if let Some(arg1) = args.first() {
                        arg1.to_php_string()
                    } else {
                        " \t\n\r\0\x0B".to_string()
                    };
                    let trimmed = str_val.trim_start_matches(|c| chars.contains(c));
                    Ok(PhpValue::string(trimmed))
                }
                "rtrim" => {
                    let str_val = s.to_str_lossy();
                    let chars = if let Some(arg1) = args.first() {
                        arg1.to_php_string()
                    } else {
                        " \t\n\r\0\x0B".to_string()
                    };
                    let trimmed = str_val.trim_end_matches(|c| chars.contains(c));
                    Ok(PhpValue::string(trimmed))
                }
                "upper" | "toUpper" => {
                    let str_val = s.to_str_lossy();
                    Ok(PhpValue::string(str_val.to_uppercase()))
                }
                "lower" | "toLower" => {
                    let str_val = s.to_str_lossy();
                    Ok(PhpValue::string(str_val.to_lowercase()))
                }
                "slug" => {
                    let str_val = s.to_str_lossy();
                    let mut slug = String::with_capacity(str_val.len());
                    let mut last_was_dash = false;
                    for c in str_val.chars() {
                        if c.is_alphanumeric() {
                            slug.extend(c.to_lowercase());
                            last_was_dash = false;
                        } else if !last_was_dash && !slug.is_empty() {
                            slug.push('-');
                            last_was_dash = true;
                        }
                    }
                    if slug.ends_with('-') {
                        slug.pop();
                    }
                    Ok(PhpValue::string(slug))
                }
                "contains" | "includes" => {
                    let needle = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    Ok(PhpValue::Bool(s.to_str_lossy().contains(&needle)))
                }
                "startsWith" | "starts_with" => {
                    let prefix = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    Ok(PhpValue::Bool(s.to_str_lossy().starts_with(&prefix)))
                }
                "endsWith" | "ends_with" => {
                    let suffix = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    Ok(PhpValue::Bool(s.to_str_lossy().ends_with(&suffix)))
                }
                "find" | "indexOf" | "index_of" => {
                    let needle = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let offset = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0);
                    let str_val = s.to_str_lossy();
                    let char_indices: Vec<(usize, char)> = str_val.char_indices().collect();
                    let total_chars = char_indices.len();
                    let start_char_idx = if offset < 0 {
                        (total_chars as i64 + offset).max(0) as usize
                    } else {
                        (offset as usize).min(total_chars)
                    };
                    let byte_offset = if start_char_idx < total_chars {
                        char_indices[start_char_idx].0
                    } else {
                        str_val.len()
                    };
                    if needle.is_empty() {
                        return Ok(PhpValue::Int(start_char_idx as i64));
                    }
                    if let Some(pos) = str_val[byte_offset..].find(&needle) {
                        let actual_byte_pos = byte_offset + pos;
                        let char_idx = str_val[..actual_byte_pos].chars().count();
                        Ok(PhpValue::Int(char_idx as i64))
                    } else {
                        Ok(PhpValue::Null)
                    }
                }
                "testRegex" | "regex" | "matchesPattern" => {
                    let str_val = s.to_str_lossy();
                    match args.first() {
                        None => {
                            Ok(PhpValue::Bool(crate::vm::facade::parse_regex_pattern(&str_val).is_ok()))
                        }
                        Some(pattern_val) => {
                            let pat_str = pattern_val.to_php_string();
                            let matched = if let Ok(re) = crate::vm::facade::parse_regex_pattern(&pat_str) {
                                re.is_match(&str_val)
                            } else if let Ok(re) = crate::vm::facade::parse_regex_pattern(&str_val) {
                                re.is_match(&pat_str)
                            } else {
                                false
                            };
                            Ok(PhpValue::Bool(matched))
                        }
                    }
                }
                "length" => {
                    Ok(PhpValue::Int(s.to_str_lossy().chars().count() as i64))
                }
                "split" => {
                    let sep = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let str_val = s.to_str_lossy();
                    let parts: Vec<PhpValue> = if sep.is_empty() {
                        str_val.chars().map(|c| PhpValue::string(c.to_string())).collect()
                    } else {
                        str_val.split(&sep).map(PhpValue::string).collect()
                    };
                    Ok(PhpValue::list(parts))
                }
                "replace" => {
                    let search = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let replace = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
                    let str_val = s.to_str_lossy();
                    Ok(PhpValue::string(str_val.replace(&search, &replace)))
                }
                "substr" => {
                    let start = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
                    let length = args.get(1).and_then(|v| v.to_i64());
                    let str_val = s.to_str_lossy();
                    let chars: Vec<char> = str_val.chars().collect();
                    let total_len = chars.len() as i64;
                    let actual_start = if start < 0 {
                        (total_len + start).max(0) as usize
                    } else {
                        start.min(total_len) as usize
                    };
                    let sub: String = if let Some(len) = length {
                        let actual_len = if len < 0 {
                            (total_len - actual_start as i64 + len).max(0) as usize
                        } else {
                            len as usize
                        };
                        chars.iter().skip(actual_start).take(actual_len).collect()
                    } else {
                        chars.iter().skip(actual_start).collect()
                    };
                    Ok(PhpValue::string(sub))
                }
                "reverse" => {
                    let str_val = s.to_str_lossy();
                    let rev: String = str_val.chars().rev().collect();
                    Ok(PhpValue::string(rev))
                }
                "repeat" => {
                    let times = args.first().and_then(|v| v.to_i64()).unwrap_or(0).max(0) as usize;
                    let str_val = s.to_str_lossy();
                    Ok(PhpValue::string(str_val.repeat(times)))
                }
                "toEncoding" => {
                    let encoding = args.first().map(|v| v.to_php_string()).unwrap_or_else(|| "UTF-8".to_string());
                    let encoded_bytes = crate::vm::encoding::encode_from_utf8(&s.to_str_lossy(), &encoding)
                        .map_err(|msg| self.make_error(RuntimeErrorKind::Custom { message: msg }, span))?;
                    Ok(PhpValue::string(crate::vm::value::RhpString::from_vec(encoded_bytes)))
                }
                "utf8" => {
                    let clean = String::from_utf8_lossy(s.as_bytes()).into_owned();
                    Ok(PhpValue::string(clean))
                }
                "bytes" => {
                    let byte_list: Vec<PhpValue> = s.as_bytes().iter().map(|&b| PhpValue::Int(b as i64)).collect();
                    Ok(PhpValue::list(byte_list))
                }
                "isUtf8" => {
                    Ok(PhpValue::Bool(s.is_utf8()))
                }
                "isEmpty" => {
                    Ok(PhpValue::Bool(s.is_empty()))
                }
                other => {
                    if let Some(macro_closure) = get_runtime_closure_macro("Str", other) {
                        self.invoke_closure_sync_with_this(
                            &macro_closure,
                            Some(receiver.clone()),
                            args.to_vec(),
                            span,
                        )
                    } else {
                        Err(self.make_error(
                            RuntimeErrorKind::UndefinedMethod {
                                class: "string".to_string(),
                                method: other.to_string(),
                            },
                            span,
                        ))
                    }
                }
            },

            other => {
                if method == "wait" {
                    return Ok(receiver.clone());
                }
                Err(self.make_error(
                    RuntimeErrorKind::UndefinedMethod {
                        class: other.type_name().to_string(),
                        method: method.to_string(),
                    },
                    span,
                ))
            }
        }
    }

    /// Execute built-in synthetic object methods (`HTTPResponse::json()`, `DBQuery::find()`, `Image::resize()`, etc.).
    fn exec_builtin_object_method(
        &mut self,
        class_name: &str,
        receiver: &PhpValue,
        method: &str,
        args: &[PhpValue],
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        if method == "toJson" {
            if class_name == "Time" || class_name == "Carbon" {
                if let PhpValue::Object(rc) = receiver {
                    if let Some(iso) = rc.borrow().get_property("iso") {
                        return Ok(PhpValue::string(format!("\"{}\"", iso.to_php_string())));
                    }
                }
            }
            return Ok(PhpValue::string(php_value_to_json(receiver)));
        }
        if class_name == "Time" || class_name == "Carbon" {
            return crate::vm::time::exec_time_instance_method(receiver, method, args, span);
        }
        if class_name == "Duration" || class_name == "CarbonInterval" {
            return crate::vm::duration::exec_duration_instance_method(receiver, method, args, span);
        }

        if class_name == "Application" || class_name == "Container"
            || class_name == "Illuminate\\Foundation\\Application"
            || class_name == "Illuminate\\Container\\Container"
        {
            match method {
                "bind" => {
                    let abs = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let conc = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
                    self.container.bind(abs, conc);
                    return Ok(PhpValue::Null);
                }
                "singleton" | "shared" => {
                    let abs = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let conc = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
                    self.container.shared(abs, conc);
                    return Ok(PhpValue::Null);
                }
                "instance" => {
                    let abs = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let val = args.get(1).cloned().unwrap_or(PhpValue::Null);
                    self.container.instance(abs, val);
                    return Ok(PhpValue::Null);
                }
                "make" | "get" | "resolve" => {
                    let abs = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    let mut container = std::mem::take(&mut self.container);
                    let res = container.resolve(&abs, self);
                    self.container = container;
                    return res.map_err(|e| self.make_error(RuntimeErrorKind::Custom { message: e.to_string() }, span));
                }
                "has" => {
                    let abs = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    return Ok(PhpValue::Bool(self.container.has(&abs)));
                }
                _ => {}
            }
        }

        match (class_name, method) {
            ("WeakReference", "get") => {
                if let PhpValue::Object(rc) = receiver {
                    let obj = rc.borrow();
                    if let Some(ref weak) = obj.weak_target {
                        if let Some(target_rc) = weak.upgrade() {
                            return Ok(PhpValue::Object(target_rc));
                        }
                    }
                }
                Ok(PhpValue::Null)
            }
            ("WeakReference", "isValid") => {
                if let PhpValue::Object(rc) = receiver {
                    let obj = rc.borrow();
                    if let Some(ref weak) = obj.weak_target {
                        return Ok(PhpValue::Bool(weak.upgrade().is_some()));
                    }
                }
                Ok(PhpValue::Bool(false))
            }
            ("UploadedFile", "path" | "getRealPath" | "getPath") => {
                if let PhpValue::Object(o) = receiver {
                    Ok(o.borrow().get_property("path").cloned().unwrap_or_else(|| PhpValue::string("")))
                } else {
                    Ok(PhpValue::string(""))
                }
            }
            ("UploadedFile", "clientName" | "getClientOriginalName") => {
                if let PhpValue::Object(o) = receiver {
                    Ok(o.borrow().get_property("clientName").cloned().unwrap_or_else(|| PhpValue::string("")))
                } else {
                    Ok(PhpValue::string(""))
                }
            }
            ("UploadedFile", "size" | "getSize") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(path_val) = o.borrow().get_property("path") {
                        let path_str = path_val.to_php_string();
                        if let Ok(meta) = std::fs::metadata(&path_str) {
                            return Ok(PhpValue::Int(meta.len() as i64));
                        }
                    }
                    Ok(o.borrow().get_property("size").cloned().unwrap_or(PhpValue::Int(0)))
                } else {
                    Ok(PhpValue::Int(0))
                }
            }
            ("UploadedFile", "mimeType" | "getMimeType") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(path_val) = o.borrow().get_property("path") {
                        let path_str = path_val.to_php_string();
                        if let Some(detected) = crate::vm::mime::detect_mime_type_for_path(std::path::Path::new(&path_str)) {
                            return Ok(PhpValue::string(detected));
                        }
                    }
                    Ok(o.borrow().get_property("mimeType").cloned().unwrap_or_else(|| PhpValue::string("application/octet-stream")))
                } else {
                    Ok(PhpValue::string("application/octet-stream"))
                }
            }
            ("UploadedFile", "clientMimeType" | "getClientMimeType") => {
                if let PhpValue::Object(o) = receiver {
                    Ok(o.borrow().get_property("clientMimeType").cloned().unwrap_or_else(|| PhpValue::string("application/octet-stream")))
                } else {
                    Ok(PhpValue::string("application/octet-stream"))
                }
            }
            ("UploadedFile", "extension" | "getClientOriginalExtension") => {
                if let PhpValue::Object(o) = receiver {
                    let name = o.borrow().get_property("clientName").map(|v| v.to_php_string()).unwrap_or_default();
                    let ext = std::path::Path::new(&name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_string();
                    Ok(PhpValue::string(ext))
                } else {
                    Ok(PhpValue::string(""))
                }
            }
            ("UploadedFile", "guessExtension") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(path_val) = o.borrow().get_property("path") {
                        let path_str = path_val.to_php_string();
                        if let Some(ext) = crate::vm::mime::guess_extension(&path_str) {
                            return Ok(PhpValue::string(ext));
                        }
                    }
                    if let Some(mime_val) = o.borrow().get_property("mimeType") {
                        let mime_str = mime_val.to_php_string();
                        if let Some(ext) = crate::vm::mime::guess_extension(&mime_str) {
                            return Ok(PhpValue::string(ext));
                        }
                    }
                }
                Ok(PhpValue::Null)
            }
            ("UploadedFile", "isValid") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(path_val) = o.borrow().get_property("path") {
                        let path_str = path_val.to_php_string();
                        return Ok(PhpValue::Bool(std::path::Path::new(&path_str).exists()));
                    }
                }
                Ok(PhpValue::Bool(false))
            }
            ("UploadedFile", "move" | "store") => {
                let target_dir = args.get(0).map(|v| v.to_php_string()).unwrap_or_else(|| "uploads".to_string());
                if let PhpValue::Object(o) = receiver {
                    let src_path = o.borrow().get_property("path").map(|v| v.to_php_string()).unwrap_or_default();
                    let target_name = if let Some(arg1) = args.get(1) {
                        arg1.to_php_string()
                    } else {
                        o.borrow().get_property("clientName").map(|v| v.to_php_string()).unwrap_or_else(|| "file".to_string())
                    };
                    let _ = std::fs::create_dir_all(&target_dir);
                    let dest = format!("{}/{}", target_dir.trim_end_matches('/'), target_name);
                    if std::fs::copy(&src_path, &dest).is_ok() {
                        let _ = std::fs::remove_file(&src_path);
                        o.borrow_mut().set_property("path", PhpValue::string(&dest));
                        return Ok(PhpValue::string(dest));
                    } else {
                        return Err(RuntimeError::new(RuntimeErrorKind::Custom {
                            message: format!("Failed to move uploaded file to '{}'", dest),
                        }, span));
                    }
                }
                Ok(PhpValue::Bool(false))
            }
            ("UploadedFile", "getContent" | "read") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(path_val) = o.borrow().get_property("path") {
                        let path_str = path_val.to_php_string();
                        if let Ok(content) = std::fs::read_to_string(&path_str) {
                            return Ok(PhpValue::string(content));
                        }
                    }
                }
                Ok(PhpValue::string(""))
            }
            ("Response", "header") => {
                if let PhpValue::Object(o) = receiver {
                    if args.len() >= 2 {
                        let key = args[0].to_php_string();
                        let val = args[1].to_php_string();
                        if key.eq_ignore_ascii_case("set-cookie") {
                            crate::vm::facade::add_active_cookie(val.clone());
                            if let Some(PhpValue::List(l)) = o.borrow().get_property("cookies") {
                                l.borrow_mut().push(PhpValue::string(&val));
                            } else {
                                o.borrow_mut().set_property("cookies", PhpValue::list(vec![PhpValue::string(&val)]));
                            }
                        } else {
                            if let Some(PhpValue::Dict(d)) = o.borrow().get_property("headers") {
                                d.borrow_mut().insert(key.clone(), PhpValue::string(&val));
                            } else {
                                let mut d = indexmap::IndexMap::new();
                                d.insert(key.clone(), PhpValue::string(&val));
                                o.borrow_mut().set_property("headers", PhpValue::dict_from_map(d));
                            }
                            crate::vm::facade::ACTIVE_RESPONSE.with(|cell| {
                                cell.borrow_mut().headers.insert(key, val);
                            });
                        }
                        Ok(receiver.clone())
                    } else if let Some(first) = args.first() {
                        let key = first.to_php_string();
                        if key.eq_ignore_ascii_case("set-cookie") {
                            if let Some(PhpValue::List(l)) = o.borrow().get_property("cookies") {
                                if let Some(first_cookie) = l.borrow().first() {
                                    return Ok(first_cookie.clone());
                                }
                            }
                        }
                        if let Some(PhpValue::Dict(d)) = o.borrow().get_property("headers") {
                            let borrowed = d.borrow();
                            for (k, v) in borrowed.iter() {
                                if k.eq_ignore_ascii_case(&key) {
                                    return Ok(v.clone());
                                }
                            }
                        }
                        Ok(PhpValue::Null)
                    } else {
                        Ok(receiver.clone())
                    }
                } else {
                    Ok(receiver.clone())
                }
            }
            ("Response", "withHeaders") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(first) = args.first() {
                        let map = extract_dict_map(first);
                        for (k, v) in &map {
                            let val_str = v.to_php_string();
                            if k.eq_ignore_ascii_case("set-cookie") {
                                crate::vm::facade::add_active_cookie(val_str.clone());
                                if let Some(PhpValue::List(l)) = o.borrow().get_property("cookies") {
                                    l.borrow_mut().push(PhpValue::string(&val_str));
                                } else {
                                    o.borrow_mut().set_property("cookies", PhpValue::list(vec![PhpValue::string(&val_str)]));
                                }
                            } else {
                                if let Some(PhpValue::Dict(d)) = o.borrow().get_property("headers") {
                                    d.borrow_mut().insert(k.clone(), PhpValue::string(&val_str));
                                } else {
                                    let mut d = indexmap::IndexMap::new();
                                    d.insert(k.clone(), PhpValue::string(&val_str));
                                    o.borrow_mut().set_property("headers", PhpValue::dict_from_map(d));
                                }
                                crate::vm::facade::ACTIVE_RESPONSE.with(|cell| {
                                    cell.borrow_mut().headers.insert(k.clone(), val_str);
                                });
                            }
                        }
                    }
                    Ok(receiver.clone())
                } else {
                    Ok(receiver.clone())
                }
            }
            ("Response", "status" | "setStatusCode") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(code_val) = args.first().and_then(|v| v.to_i64()) {
                        o.borrow_mut().set_property("status", PhpValue::Int(code_val as i64));
                        crate::vm::facade::ACTIVE_RESPONSE.with(|cell| {
                            cell.borrow_mut().status = code_val as u16;
                        });
                    }
                    Ok(receiver.clone())
                } else {
                    Ok(receiver.clone())
                }
            }
            ("Response", "getStatusCode") => {
                if let PhpValue::Object(o) = receiver {
                    let status = o.borrow().get_property("status").and_then(|v| v.to_i64()).unwrap_or(200);
                    Ok(PhpValue::Int(status as i64))
                } else {
                    Ok(PhpValue::Int(200))
                }
            }
            ("Response", "content" | "setContent") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(first) = args.first() {
                        let s = first.to_php_string();
                        o.borrow_mut().set_property("content", PhpValue::string(&s));
                        crate::vm::facade::ACTIVE_RESPONSE.with(|cell| {
                            cell.borrow_mut().content = Some(s);
                        });
                    }
                    Ok(receiver.clone())
                } else {
                    Ok(receiver.clone())
                }
            }
            ("Response", "getContent") => {
                if let PhpValue::Object(o) = receiver {
                    let content = o.borrow().get_property("content").cloned().unwrap_or_else(|| PhpValue::string(""));
                    Ok(content)
                } else {
                    Ok(PhpValue::string(""))
                }
            }
            ("Response", "getHeaders" | "headers") => {
                if let PhpValue::Object(o) = receiver {
                    let hdrs = o.borrow().get_property("headers").cloned().unwrap_or_else(|| PhpValue::dict_from_map(indexmap::IndexMap::new()));
                    Ok(hdrs)
                } else {
                    Ok(PhpValue::dict_from_map(indexmap::IndexMap::new()))
                }
            }
            ("Response", "cookie" | "withCookie") => {
                if let PhpValue::Object(o) = receiver {
                    let name = args.get(0).map(|v| v.to_php_string()).unwrap_or_default();
                    let val = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
                    if !name.is_empty() {
                        let cookie_str = crate::vm::facade::format_cookie_from_args(&name, &val, &args[2..]);
                        crate::vm::facade::add_active_cookie(cookie_str.clone());
                        if let Some(PhpValue::List(l)) = o.borrow().get_property("cookies") {
                            l.borrow_mut().push(PhpValue::string(&cookie_str));
                        } else {
                            o.borrow_mut().set_property("cookies", PhpValue::list(vec![PhpValue::string(&cookie_str)]));
                        }
                    }
                    Ok(receiver.clone())
                } else {
                    Ok(receiver.clone())
                }
            }
            ("Response", "withoutCookie" | "forgetCookie") => {
                if let PhpValue::Object(o) = receiver {
                    let name = args.get(0).map(|v| v.to_php_string()).unwrap_or_default();
                    let path = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| "/".to_string());
                    let domain = args.get(2).map(|v| v.to_php_string());
                    if !name.is_empty() {
                        let mut cookie_str = format!("{}=deleted; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path={}", name, path);
                        if let Some(d) = domain {
                            if !d.is_empty() {
                                cookie_str.push_str(&format!("; Domain={}", d));
                            }
                        }
                        crate::vm::facade::add_active_cookie(cookie_str.clone());
                        if let Some(PhpValue::List(l)) = o.borrow().get_property("cookies") {
                            l.borrow_mut().push(PhpValue::string(&cookie_str));
                        } else {
                            o.borrow_mut().set_property("cookies", PhpValue::list(vec![PhpValue::string(&cookie_str)]));
                        }
                    }
                    Ok(receiver.clone())
                } else {
                    Ok(receiver.clone())
                }
            }
            ("Response", "cookies" | "getCookies") => {
                if let PhpValue::Object(o) = receiver {
                    let cookies = o.borrow().get_property("cookies").cloned().unwrap_or_else(|| PhpValue::list(Vec::new()));
                    Ok(cookies)
                } else {
                    Ok(PhpValue::list(Vec::new()))
                }
            }
            ("Request", "header") => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                if let PhpValue::Object(o) = receiver {
                    if let Some(PhpValue::Dict(d)) = o.borrow().get_property("headers") {
                        for (k, v) in d.borrow().iter() {
                            if k.eq_ignore_ascii_case(&key) {
                                return Ok(v.clone());
                            }
                        }
                    }
                }
                crate::vm::facade::dispatch_facade_call("Request", "header", args, span)
            }
            ("Request", "hasHeader") => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                if let PhpValue::Object(o) = receiver {
                    if let Some(PhpValue::Dict(d)) = o.borrow().get_property("headers") {
                        for (k, _) in d.borrow().iter() {
                            if k.eq_ignore_ascii_case(&key) {
                                return Ok(PhpValue::Bool(true));
                            }
                        }
                    }
                }
                Ok(PhpValue::Bool(false))
            }
            ("Request", "headers") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(hdrs) = o.borrow().get_property("headers") {
                        return Ok(hdrs.clone());
                    }
                }
                crate::vm::facade::dispatch_facade_call("Request", "headers", args, span)
            }
            ("Request", "cookie") => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                if let PhpValue::Object(o) = receiver {
                    if let Some(PhpValue::Dict(d)) = o.borrow().get_property("cookies") {
                        for (k, v) in d.borrow().iter() {
                            if k == &key {
                                return Ok(v.clone());
                            }
                        }
                    }
                }
                crate::vm::facade::dispatch_facade_call("Request", "cookie", args, span)
            }
            ("Request", "cookies") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(cookies) = o.borrow().get_property("cookies") {
                        return Ok(cookies.clone());
                    }
                }
                crate::vm::facade::dispatch_facade_call("Request", "cookies", args, span)
            }
            ("Request", "server") => {
                if args.is_empty() {
                    if let PhpValue::Object(o) = receiver {
                        if let Some(srv) = o.borrow().get_property("server") {
                            return Ok(srv.clone());
                        }
                    }
                    return crate::vm::facade::dispatch_facade_call("Request", "server", args, span);
                }
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                if let PhpValue::Object(o) = receiver {
                    if let Some(PhpValue::Dict(d)) = o.borrow().get_property("server") {
                        for (k, v) in d.borrow().iter() {
                            if k.eq_ignore_ascii_case(&key) {
                                return Ok(v.clone());
                            }
                        }
                    }
                }
                crate::vm::facade::dispatch_facade_call("Request", "server", args, span)
            }
            ("Request", "bearerToken") => {
                crate::vm::facade::dispatch_facade_call("Request", "bearerToken", args, span)
            }
            ("Request", "method") => {
                crate::vm::facade::dispatch_facade_call("Request", "method", args, span)
            }
            ("Request", "isMethod") => {
                crate::vm::facade::dispatch_facade_call("Request", "isMethod", args, span)
            }
            ("Request", "all") => {
                crate::vm::facade::dispatch_facade_call("Request", "all", args, span)
            }
            ("Request", "get" | "input") => {
                crate::vm::facade::dispatch_facade_call("Request", "get", args, span)
            }
            ("Request", "has") => {
                crate::vm::facade::dispatch_facade_call("Request", "has", args, span)
            }
            ("Response", "send" | "sendHeaders") => Ok(receiver.clone()),
            ("HTTPResponse", "json") => {
                let mut entries = Vec::new();
                entries.push(("status".to_string(), PhpValue::string("ok")));
                Ok(PhpValue::dict(entries))
            }
            ("HTTPResponse", "body") => {
                if let PhpValue::Object(o) = receiver {
                    let b = o.borrow();
                    if let Some(raw_prop) = b.get_property("raw_bytes") {
                        let bytes: &[u8] = match &raw_prop {
                            PhpValue::String(s) => s.as_bytes(),
                            _ => b"",
                        };
                        let charset = b.get_property("headers").and_then(|h| {
                            if let PhpValue::Dict(d) = h {
                                for (k, v) in d.borrow().iter() {
                                    if k.eq_ignore_ascii_case("content-type") {
                                        let val = v.to_php_string();
                                        if let Some(idx) = val.to_ascii_lowercase().find("charset=") {
                                            let sub = &val[idx + 8..];
                                            let clean = sub.split(';').next().unwrap_or("").trim().trim_matches('"');
                                            return Some(clean.to_string());
                                        }
                                    }
                                }
                            }
                            None
                        });
                        let enc = charset.as_deref().unwrap_or("UTF-8");
                        let decoded = crate::vm::encoding::decode_bytes(bytes, enc, "strict")
                            .map(|s| s)
                            .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned());
                        Ok(PhpValue::string(decoded))
                    } else if let Some(body_prop) = b.get_property("body") {
                        Ok(body_prop.clone())
                    } else {
                        Ok(PhpValue::string(""))
                    }
                } else {
                    Ok(PhpValue::string(""))
                }
            }
            ("HTTPResponse", "bytes") => {
                if let PhpValue::Object(o) = receiver {
                    let b = o.borrow();
                    if let Some(raw_prop) = b.get_property("raw_bytes") {
                        Ok(raw_prop.clone())
                    } else if let Some(body_prop) = b.get_property("body") {
                        let bytes: &[u8] = match &body_prop {
                            PhpValue::String(s) => s.as_bytes(),
                            _ => b"",
                        };
                        Ok(PhpValue::bytes(bytes))
                    } else {
                        Ok(PhpValue::bytes(Vec::new()))
                    }
                } else {
                    Ok(PhpValue::bytes(Vec::new()))
                }
            }
            ("HTTPResponse", "status") => Ok(PhpValue::Int(200)),
            ("HTTPResponse", "ok") => {
                let status = if let PhpValue::Object(o) = receiver {
                    o.borrow()
                        .get_property("status")
                        .and_then(|v| v.to_i64())
                        .unwrap_or(200)
                } else {
                    200
                };
                Ok(PhpValue::Bool(status >= 200 && status < 300))
            }
            ("HTTPResponse", "headers") => {
                if let PhpValue::Object(o) = receiver {
                    Ok(o.borrow()
                        .get_property("headers")
                        .cloned()
                        .unwrap_or_else(|| PhpValue::dict(Vec::new())))
                } else {
                    Ok(PhpValue::dict(Vec::new()))
                }
            }
            ("HTTPClient", "get" | "post") => {
                let url = args
                    .first()
                    .map(|v| v.to_php_string())
                    .unwrap_or_default();
                let mut resp = VmObject::new("HTTPResponse");
                resp.set_property("url", PhpValue::string(url));
                resp.set_property("status", PhpValue::Int(200));
                resp.set_property("body", PhpValue::string("{\"status\":\"ok\"}"));
                Ok(PhpValue::object(resp))
            }
            ("AHTTPClient", "get" | "post" | "put" | "delete" | "patch" | "request") => {
                let url = args
                    .first()
                    .map(|v| v.to_php_string())
                    .unwrap_or_default();
                let body_payload = args.get(1).map(|v| v.to_php_string());
                let http_method = method.to_ascii_uppercase();
                let mut extra_headers: Vec<(String, String)> = Vec::new();
                if let PhpValue::Object(o) = receiver {
                    if let Some(PhpValue::Dict(hd)) = o.borrow().get_property("headers") {
                        for (k, v) in hd.borrow().iter() {
                            extra_headers.push((k.clone(), v.to_php_string()));
                        }
                    }
                }
                let rt = crate::vm::facade::tokio_runtime();
                let (tx, rx) = std::sync::mpsc::channel();
                rt.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    let mut resp = VmObject::new("HTTPResponse");
                    resp.set_property("url", PhpValue::string(url));
                    resp.set_property("status", PhpValue::Int(200));
                    let body_str = if let Some(b) = body_payload {
                        b
                    } else {
                        "{\"status\":\"ok\"}".to_string()
                    };
                    resp.set_property("body", PhpValue::string(body_str));
                    let mut headers_dict = IndexMap::new();
                    headers_dict.insert("Content-Type".to_string(), PhpValue::string("application/json"));
                    headers_dict.insert("X-Async-Engine".to_string(), PhpValue::string("tokio"));
                    headers_dict.insert("X-HTTP-Method".to_string(), PhpValue::string(http_method));
                    for (k, v) in extra_headers {
                        headers_dict.insert(k, PhpValue::string(v));
                    }
                    resp.set_property("headers", PhpValue::dict_from_map(headers_dict));
                    let frozen = crate::vm::parent_process::FrozenValue::from_php_value(&PhpValue::object(resp));
                    let _ = tx.send(Ok(frozen));
                });
                Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::pending(rx)))))
            }
            ("DBQuery", "find") => {
                let id = args.first().cloned().unwrap_or(PhpValue::Int(1));
                let mut record = VmObject::new("ModelRecord");
                record.set_property("id", id.clone());
                let tbl = if let PhpValue::Object(o) = receiver {
                    if let Some(t) = o.borrow().get_property("table") {
                        record.set_property("table", t.clone());
                        t.to_php_string()
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                };
                crate::vm::fiber::record_fiber_io_start(crate::vm::fiber::IoKind::DbFind {
                    table: tbl,
                    id: id.to_php_string(),
                });
                crate::vm::fiber::record_fiber_io_finish();
                Ok(PhpValue::object(record))
            }
            ("DBQuery", "first") => {
                let mut record = VmObject::new("ModelRecord");
                record.set_property("id", PhpValue::Int(1));
                let tbl = if let PhpValue::Object(o) = receiver {
                    if let Some(t) = o.borrow().get_property("table") {
                        record.set_property("table", t.clone());
                        t.to_php_string()
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                };
                crate::vm::fiber::record_fiber_io_start(crate::vm::fiber::IoKind::DbQuery {
                    sql: format!("SELECT * FROM {} LIMIT 1", tbl),
                });
                crate::vm::fiber::record_fiber_io_finish();
                Ok(PhpValue::object(record))
            }
            ("DBQuery", "where" | "orderBy" | "limit" | "offset" | "select") => {
                Ok(receiver.clone())
            }
            ("DBQuery", "get" | "all") => {
                let mut record = VmObject::new("ModelRecord");
                record.set_property("id", PhpValue::Int(1));
                let tbl = if let PhpValue::Object(o) = receiver {
                    if let Some(t) = o.borrow().get_property("table") {
                        record.set_property("table", t.clone());
                        t.to_php_string()
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                };
                crate::vm::fiber::record_fiber_io_start(crate::vm::fiber::IoKind::DbQuery {
                    sql: format!("SELECT * FROM {}", tbl),
                });
                crate::vm::fiber::record_fiber_io_finish();
                Ok(PhpValue::list(vec![PhpValue::object(record)]))
            }
            ("DBQuery", "count") => {
                crate::vm::fiber::record_fiber_io_start(crate::vm::fiber::IoKind::DbQuery {
                    sql: "SELECT COUNT(*) FROM table".to_string(),
                });
                crate::vm::fiber::record_fiber_io_finish();
                Ok(PhpValue::Int(1))
            }
            ("DBQuery", "insert") => {
                crate::vm::fiber::record_fiber_io_start(crate::vm::fiber::IoKind::DbQuery {
                    sql: "INSERT INTO table".to_string(),
                });
                crate::vm::fiber::record_fiber_io_finish();
                Ok(PhpValue::Int(1))
            }
            ("DBQuery", "update" | "delete") => {
                crate::vm::fiber::record_fiber_io_start(crate::vm::fiber::IoKind::DbQuery {
                    sql: format!("{} FROM table", method.to_uppercase()),
                });
                crate::vm::fiber::record_fiber_io_finish();
                Ok(PhpValue::Int(1))
            }
            ("DBQuery", "paginate") => {
                let per_page = args.first().and_then(|v| v.to_i64()).unwrap_or(15) as i32;
                let page = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1) as i32;
                let tbl = if let PhpValue::Object(o) = receiver {
                    if let Some(t) = o.borrow().get_property("table") {
                        t.to_php_string()
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                };
                crate::vm::fiber::record_fiber_io_start(crate::vm::fiber::IoKind::DbQuery {
                    sql: format!("SELECT * FROM {} LIMIT {} OFFSET {}", tbl, per_page, (page - 1) * per_page),
                });
                crate::vm::fiber::record_fiber_io_finish();

                let mut record = VmObject::new("ModelRecord");
                record.set_property("id", PhpValue::Int(1));
                record.set_property("table", PhpValue::string(tbl));
                let items = PhpValue::list(vec![PhpValue::object(record)]);

                let mut paginator = VmObject::new("LengthAwarePaginator");
                paginator.set_property("items", items);
                paginator.set_property("total", PhpValue::Int(1));
                paginator.set_property("perPage", PhpValue::Int((per_page) as i64));
                paginator.set_property("currentPage", PhpValue::Int((page) as i64));
                Ok(PhpValue::object(paginator))
            }
            ("ADBQuery", "where" | "orderBy" | "limit" | "offset" | "select") => {
                Ok(receiver.clone())
            }
            ("ADBQuery", "find") => {
                let id = args.first().cloned().unwrap_or(PhpValue::Int(1));
                let tbl = if let PhpValue::Object(o) = receiver {
                    if let Some(t) = o.borrow().get_property("table") {
                        t.to_php_string()
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                };
                let id_frozen = crate::vm::parent_process::FrozenValue::from_php_value(&id);
                let rt = crate::vm::facade::tokio_runtime();
                let (tx, rx) = std::sync::mpsc::channel();
                rt.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    let mut record = VmObject::new("ModelRecord");
                    record.set_property("id", id_frozen.to_php_value());
                    record.set_property("table", PhpValue::string(tbl));
                    let frozen = crate::vm::parent_process::FrozenValue::from_php_value(&PhpValue::object(record));
                    let _ = tx.send(Ok(frozen));
                });
                Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::pending(rx)))))
            }
            ("ADBQuery", "first") => {
                let tbl = if let PhpValue::Object(o) = receiver {
                    if let Some(t) = o.borrow().get_property("table") {
                        t.to_php_string()
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                };
                let rt = crate::vm::facade::tokio_runtime();
                let (tx, rx) = std::sync::mpsc::channel();
                rt.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    let mut record = VmObject::new("ModelRecord");
                    record.set_property("id", PhpValue::Int(1));
                    record.set_property("table", PhpValue::string(tbl));
                    let frozen = crate::vm::parent_process::FrozenValue::from_php_value(&PhpValue::object(record));
                    let _ = tx.send(Ok(frozen));
                });
                Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::pending(rx)))))
            }
            ("ADBQuery", "get" | "all") => {
                let tbl = if let PhpValue::Object(o) = receiver {
                    if let Some(t) = o.borrow().get_property("table") {
                        t.to_php_string()
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                };
                let rt = crate::vm::facade::tokio_runtime();
                let (tx, rx) = std::sync::mpsc::channel();
                rt.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    let mut record = VmObject::new("ModelRecord");
                    record.set_property("id", PhpValue::Int(1));
                    record.set_property("table", PhpValue::string(tbl));
                    let items = vec![crate::vm::parent_process::FrozenValue::from_php_value(&PhpValue::object(record))];
                    let _ = tx.send(Ok(crate::vm::parent_process::FrozenValue::List(items)));
                });
                Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::pending(rx)))))
            }
            ("ADBQuery", "count") => {
                let rt = crate::vm::facade::tokio_runtime();
                let (tx, rx) = std::sync::mpsc::channel();
                rt.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    let _ = tx.send(Ok(crate::vm::parent_process::FrozenValue::Int(1)));
                });
                Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::pending(rx)))))
            }
            ("ADBQuery", "insert" | "update" | "delete") => {
                let rt = crate::vm::facade::tokio_runtime();
                let (tx, rx) = std::sync::mpsc::channel();
                rt.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    let _ = tx.send(Ok(crate::vm::parent_process::FrozenValue::Int(1)));
                });
                Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::pending(rx)))))
            }
            ("ADBQuery", "paginate") => {
                let per_page = args.first().and_then(|v| v.to_i64()).unwrap_or(15) as i32;
                let page = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1) as i32;
                let tbl = if let PhpValue::Object(o) = receiver {
                    if let Some(t) = o.borrow().get_property("table") {
                        t.to_php_string()
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                };
                let rt = crate::vm::facade::tokio_runtime();
                let (tx, rx) = std::sync::mpsc::channel();
                rt.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    let mut record = VmObject::new("ModelRecord");
                    record.set_property("id", PhpValue::Int(1));
                    record.set_property("table", PhpValue::string(tbl));
                    let items = vec![crate::vm::parent_process::FrozenValue::from_php_value(&PhpValue::object(record))];

                    let mut paginator = VmObject::new("LengthAwarePaginator");
                    paginator.set_property(
                        "items",
                        PhpValue::List(Rc::new(RefCell::new(
                            items.into_iter().map(|f| f.to_php_value()).collect(),
                        ))),
                    );
                    paginator.set_property("total", PhpValue::Int(1));
                    paginator.set_property("perPage", PhpValue::Int((per_page) as i64));
                    paginator.set_property("currentPage", PhpValue::Int((page) as i64));
                    let frozen = crate::vm::parent_process::FrozenValue::from_php_value(&PhpValue::object(paginator));
                    let _ = tx.send(Ok(frozen));
                });
                Ok(PhpValue::Future(Rc::new(RefCell::new(VmFuture::pending(rx)))))
            }
            ("ArrayIterator", "count") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(items) = o.borrow().get_property("items") {
                        let cnt = match items {
                            PhpValue::List(l) => l.borrow().len(),
                            PhpValue::Array(a) => a.borrow().len(),
                            PhpValue::Dict(d) => d.borrow().len(),
                            PhpValue::Set(s) => s.borrow().len(),
                            _ => 0,
                        };
                        return Ok(PhpValue::Int(cnt as i64));
                    }
                }
                Ok(PhpValue::Int(0))
            }
            ("ArrayIterator", "getArrayCopy") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(items) = o.borrow().get_property("items") {
                        return Ok(items.clone());
                    }
                }
                Ok(PhpValue::list(vec![]))
            }
            ("ArrayIterator", "rewind") => {
                if let PhpValue::Object(o) = receiver {
                    o.borrow_mut().set_property("cursor", PhpValue::Int(0));
                }
                Ok(PhpValue::Null)
            }
            ("ArrayIterator", "valid") => {
                if let PhpValue::Object(o) = receiver {
                    let b = o.borrow();
                    let cursor = b.get_property("cursor").and_then(|v| v.to_i64()).unwrap_or(0);
                    if let Some(items) = b.get_property("items") {
                        let len = match items {
                            PhpValue::List(l) => l.borrow().len(),
                            PhpValue::Array(a) => a.borrow().len(),
                            PhpValue::Dict(d) => d.borrow().len(),
                            PhpValue::Set(s) => s.borrow().len(),
                            _ => 0,
                        };
                        return Ok(PhpValue::Bool(cursor >= 0 && (cursor as usize) < len));
                    }
                }
                Ok(PhpValue::Bool(false))
            }
            ("ArrayIterator", "current") => {
                if let PhpValue::Object(o) = receiver {
                    let b = o.borrow();
                    let cursor = b.get_property("cursor").and_then(|v| v.to_i64()).unwrap_or(0) as usize;
                    if let Some(items) = b.get_property("items") {
                        match items {
                            PhpValue::List(l) => {
                                let lb = l.borrow();
                                if cursor < lb.len() {
                                    return Ok(lb[cursor].clone());
                                }
                            }
                            PhpValue::Array(a) => {
                                let ab = a.borrow();
                                if cursor < ab.len() {
                                    return Ok(ab[cursor].clone());
                                }
                            }
                            PhpValue::Dict(d) => {
                                let db = d.borrow();
                                if cursor < db.len() {
                                    if let Some((_, v)) = db.get_index(cursor) {
                                        return Ok(v.clone());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ok(PhpValue::Null)
            }
            ("ArrayIterator", "key") => {
                if let PhpValue::Object(o) = receiver {
                    let b = o.borrow();
                    let cursor = b.get_property("cursor").and_then(|v| v.to_i64()).unwrap_or(0) as usize;
                    if let Some(items) = b.get_property("items") {
                        match items {
                            PhpValue::List(_) | PhpValue::Array(_) => {
                                return Ok(PhpValue::Int(cursor as i64));
                            }
                            PhpValue::Dict(d) => {
                                let db = d.borrow();
                                if cursor < db.len() {
                                    if let Some((k, _)) = db.get_index(cursor) {
                                        return Ok(PhpValue::string(k.clone()));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ok(PhpValue::Null)
            }
            ("ArrayIterator", "next") => {
                if let PhpValue::Object(o) = receiver {
                    let mut b = o.borrow_mut();
                    let cursor = b.get_property("cursor").and_then(|v| v.to_i64()).unwrap_or(0);
                    b.set_property("cursor", PhpValue::Int((cursor + 1) as i64));
                }
                Ok(PhpValue::Null)
            }
            ("ArrayIterator", "getIterator") => Ok(receiver.clone()),
            ("Collection" | "Illuminate\\Support\\Collection", "all" | "toArray" | "items") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(items) = o.borrow().get_property("items") {
                        return Ok(items.clone());
                    }
                }
                Ok(PhpValue::list(vec![]))
            }
            ("Collection" | "Illuminate\\Support\\Collection", "count") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(items) = o.borrow().get_property("items") {
                        let cnt = match items {
                            PhpValue::List(l) => l.borrow().len(),
                            PhpValue::Array(a) => a.borrow().len(),
                            PhpValue::Dict(d) => d.borrow().len(),
                            PhpValue::Set(s) => s.borrow().len(),
                            _ => 0,
                        };
                        return Ok(PhpValue::Int(cnt as i64));
                    }
                }
                Ok(PhpValue::Int(0))
            }
            ("Collection" | "Illuminate\\Support\\Collection", "getIterator") => {
                let items = if let PhpValue::Object(o) = receiver {
                    o.borrow().get_property("items").cloned().unwrap_or_else(|| PhpValue::list(vec![]))
                } else {
                    PhpValue::list(vec![])
                };
                let mut iter_obj = VmObject::new("ArrayIterator");
                iter_obj.set_property("items", items);
                iter_obj.set_property("cursor", PhpValue::Int(0));
                Ok(PhpValue::object(iter_obj))
            }
            ("LengthAwarePaginator" | "Paginator" | "Illuminate\\Pagination\\LengthAwarePaginator" | "Illuminate\\Pagination\\Paginator", "items") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(items) = o.borrow().get_property("items") {
                        return Ok(items.clone());
                    }
                }
                Ok(PhpValue::list(vec![]))
            }
            ("LengthAwarePaginator" | "Paginator" | "Illuminate\\Pagination\\LengthAwarePaginator" | "Illuminate\\Pagination\\Paginator", "total") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(tot) = o.borrow().get_property("total") {
                        return Ok(tot.clone());
                    }
                }
                Ok(PhpValue::Int(0))
            }
            ("LengthAwarePaginator" | "Paginator" | "Illuminate\\Pagination\\LengthAwarePaginator" | "Illuminate\\Pagination\\Paginator", "perPage") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(pp) = o.borrow().get_property("perPage") {
                        return Ok(pp.clone());
                    }
                }
                Ok(PhpValue::Int(15))
            }
            ("LengthAwarePaginator" | "Paginator" | "Illuminate\\Pagination\\LengthAwarePaginator" | "Illuminate\\Pagination\\Paginator", "currentPage") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(cp) = o.borrow().get_property("currentPage") {
                        return Ok(cp.clone());
                    }
                }
                Ok(PhpValue::Int(1))
            }
            ("LengthAwarePaginator" | "Paginator" | "Illuminate\\Pagination\\LengthAwarePaginator" | "Illuminate\\Pagination\\Paginator", "count") => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(items) = o.borrow().get_property("items") {
                        let cnt = match items {
                            PhpValue::List(l) => l.borrow().len(),
                            PhpValue::Array(a) => a.borrow().len(),
                            PhpValue::Dict(d) => d.borrow().len(),
                            PhpValue::Set(s) => s.borrow().len(),
                            _ => 0,
                        };
                        return Ok(PhpValue::Int(cnt as i64));
                    }
                }
                Ok(PhpValue::Int(0))
            }
            ("LengthAwarePaginator" | "Paginator" | "Illuminate\\Pagination\\LengthAwarePaginator" | "Illuminate\\Pagination\\Paginator", "getIterator") => {
                let items = if let PhpValue::Object(o) = receiver {
                    o.borrow().get_property("items").cloned().unwrap_or_else(|| PhpValue::list(vec![]))
                } else {
                    PhpValue::list(vec![])
                };
                let mut iter_obj = VmObject::new("ArrayIterator");
                iter_obj.set_property("items", items);
                iter_obj.set_property("cursor", PhpValue::Int(0));
                Ok(PhpValue::object(iter_obj))
            }
            ("LoggerChannel", "info" | "error" | "debug" | "warning" | "warn" | "notice"
            | "alert" | "critical" | "emergency") => {
                let ch = if let PhpValue::Object(o) = receiver {
                    o.borrow()
                        .get_property("channel")
                        .map(|v| v.to_php_string())
                        .unwrap_or_else(|| "default".to_string())
                } else {
                    "default".to_string()
                };
                let msg = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                let ctx = args.get(1).cloned();
                let lvl = if method == "warn" { "warning" } else { method };
                crate::vm::facade::append_log(lvl, &msg, ctx, &ch);
                Ok(PhpValue::Null)
            }
            ("Image", "resize" | "crop") => Ok(receiver.clone()),
            ("Image", "save") => Ok(PhpValue::Bool(true)),
            (_, "__construct") if is_builtin_throwable(class_name) || class_name == "parent" => {
                if let PhpValue::Object(o) = receiver {
                    self.apply_throwable_args_from_slice(&mut o.borrow_mut(), class_name, args);
                }
                Ok(PhpValue::Null)
            }
            (_, "getFile") => {
                if let PhpValue::Object(o) = receiver {
                    let b = o.borrow();
                    if let Some(ref t) = b.throwable {
                        return Ok(PhpValue::string(&t.file));
                    }
                    if let Some(f) = b.get_property("file") {
                        return Ok(f.clone());
                    }
                }
                Ok(PhpValue::string(""))
            }
            (_, "getLine") => {
                if let PhpValue::Object(o) = receiver {
                    let b = o.borrow();
                    if let Some(ref t) = b.throwable {
                        return Ok(PhpValue::Int(t.line as i64));
                    }
                    if let Some(l) = b.get_property("line") {
                        return Ok(l.clone());
                    }
                }
                Ok(PhpValue::Int(0))
            }
            (_, "getTrace") => {
                if let PhpValue::Object(o) = receiver {
                    let mut b = o.borrow_mut();
                    if let Some(ref mut t) = b.throwable {
                        if !t.is_symbolicated {
                            self.symbolicate_trace(t);
                        }
                        return Ok(t.to_trace_php_value(self.exception_ignore_args));
                    }
                }
                Ok(PhpValue::list(vec![]))
            }
            (_, "getTraceAsString") => {
                if let PhpValue::Object(o) = receiver {
                    let mut b = o.borrow_mut();
                    if let Some(ref mut t) = b.throwable {
                        if !t.is_symbolicated {
                            self.symbolicate_trace(t);
                        }
                        return Ok(PhpValue::string(t.get_trace_as_string()));
                    }
                }
                Ok(PhpValue::string("#0 {main}"))
            }
            (_, "source") => {
                if let PhpValue::Object(o) = receiver {
                    let b = o.borrow();
                    if let Some(ref t) = b.throwable {
                        if let Some(ref prev_obj) = t.previous_obj {
                            return Ok(prev_obj.clone());
                        }
                        if let Some(prev) = t.source() {
                            let mut prev_o = VmObject::new("Exception");
                            prev_o.set_property("message", PhpValue::string(&prev.message));
                            prev_o.set_property("code", PhpValue::Int(prev.code as i64));
                            prev_o.set_property("file", PhpValue::string(&prev.file));
                            prev_o.set_property("line", PhpValue::Int(prev.line as i64));
                            prev_o.throwable = Some(Box::new(prev.clone()));
                            return Ok(PhpValue::object(prev_o));
                        }
                    }
                    if let Some(prev) = b.get_property("previous") {
                        return Ok(prev.clone());
                    }
                }
                Ok(PhpValue::Null)
            }
            (_, "getPrevious") => {
                // PHP/Laravel compatibility alias delegating to primary source()
                self.exec_builtin_object_method(class_name, receiver, "source", args, span)
            }
            (_, "rootCause") => {
                // Primary method returning the deepest cause in the chain
                let mut current = receiver.clone();
                loop {
                    let next = self.exec_builtin_object_method(class_name, &current, "source", &[], span)?;
                    if next.is_null() {
                        break;
                    }
                    current = next;
                }
                Ok(current)
            }
            (_, "getRootCause") => {
                // Alias delegating to primary rootCause()
                self.exec_builtin_object_method(class_name, receiver, "rootCause", args, span)
            }
            (_, "chain") => {
                // Primary method returning a list of all chained exceptions from receiver down to rootCause
                let mut list = Vec::new();
                let mut current = receiver.clone();
                while !current.is_null() {
                    list.push(current.clone());
                    current = self.exec_builtin_object_method(class_name, &current, "source", &[], span)?;
                }
                Ok(PhpValue::list(list))
            }
            (_, "getChain") => {
                // Alias delegating to primary chain()
                self.exec_builtin_object_method(class_name, receiver, "chain", args, span)
            }
            (_, "getMessage") => {
                if let PhpValue::Object(o) = receiver {
                    let b = o.borrow();
                    if let Some(ref t) = b.throwable {
                        return Ok(PhpValue::string(&t.message));
                    }
                    if let Some(m) = b.get_property("message") {
                        return Ok(m.clone());
                    }
                }
                Ok(PhpValue::string(""))
            }
            (_, "getCode") => {
                if let PhpValue::Object(o) = receiver {
                    let b = o.borrow();
                    if let Some(ref t) = b.throwable {
                        return Ok(PhpValue::Int(t.code as i64));
                    }
                    if let Some(c) = b.get_property("code") {
                        return Ok(c.clone());
                    }
                }
                Ok(PhpValue::Int(0))
            }
            (_, "getSeverity") => {
                if let PhpValue::Object(o) = receiver {
                    let b = o.borrow();
                    if let Some(ref t) = b.throwable {
                        if let Some(sev) = t.severity {
                            return Ok(PhpValue::Int((sev) as i64));
                        }
                    }
                    if let Some(s) = b.get_property("severity") {
                        return Ok(s.clone());
                    }
                }
                Ok(PhpValue::Int(1))
            }
            (_, "__toString") => {
                if let PhpValue::Object(o) = receiver {
                    let mut b = o.borrow_mut();
                    let cls = b.class_name.clone();
                    if let Some(ref mut t) = b.throwable {
                        if !t.is_symbolicated {
                            self.symbolicate_trace(t);
                        }
                        return Ok(PhpValue::string(t.to_string_representation(&cls)));
                    }
                    return Ok(PhpValue::string(format!("{}: in unknown:0", cls)));
                }
                Ok(PhpValue::string(""))
            }
            _ => {
                if let PhpValue::Object(o) = receiver {
                    if let Some(ref origin) = o.borrow().forwarded_from {
                        return Err(self.make_error(
                            RuntimeErrorKind::UndefinedForwardedMethod {
                                class: class_name.to_string(),
                                method: method.to_string(),
                                origin_class: origin.clone(),
                            },
                            span,
                        ));
                    }
                }
                Err(self.make_error(
                    RuntimeErrorKind::UndefinedMethod {
                        class: class_name.to_string(),
                        method: method.to_string(),
                    },
                    span,
                ))
            }
        }
    }

    // =========================================================================
    // Object Instantiation, Inheritance Resolution, & Exception Unwinding
    // =========================================================================

    fn instantiate_class(&mut self, class_name: &str, span: Span) -> Result<PhpValue, RuntimeError> {
        self.instantiate_class_raw(class_name, span, false)
    }

    fn instantiate_class_raw(
        &mut self,
        class_name: &str,
        span: Span,
        allow_shared: bool,
    ) -> Result<PhpValue, RuntimeError> {
        let clean_name = class_name.strip_prefix('\\').unwrap_or(class_name);

        // Engine-Level Instantiation Guard:
        // Halts illegal instantiation attempts on static facades (Tier 1, Tier 2, Tier 3)
        // or any class marked `is_static_only: true` with a fatal error.
        if is_static_facade(clean_name) || is_static_only(clean_name) {
            return Err(self.make_error(
                RuntimeErrorKind::StaticFacadeInstantiationForbidden {
                    class: clean_name.to_string(),
                },
                span,
            ));
        }

        if let Some(cls) = self.module.get_class_by_name(clean_name) {
            if cls.modifiers.is_static_only {
                return Err(self.make_error(
                    RuntimeErrorKind::StaticFacadeInstantiationForbidden {
                        class: clean_name.to_string(),
                    },
                    span,
                ));
            }

            if cls.modifiers.is_shared && !allow_shared {
                return Err(self.make_error(
                    RuntimeErrorKind::SharedInstantiationForbidden {
                        shared_class: clean_name.to_string(),
                    },
                    span,
                ));
            }
        }

        let mut obj = VmObject::new(clean_name);
        if let Some(ci) = self.module.get_class_info_by_name(clean_name) {
            obj.class_id = Some(ci.id);
        }

        // Built-in Exception / RuntimeException / Throwable classes
        if self.is_class_instance_of(clean_name, "Throwable") {
            self.init_throwable_for_object(&mut obj, span);
            if !self.module.class_index.contains_key(clean_name) {
                let instance = PhpValue::object(obj);
                self.track_live_object(&instance);
                return Ok(instance);
            }
        } else if clean_name == "stdClass" {
            if !self.module.class_index.contains_key(clean_name) {
                let instance = PhpValue::object(obj);
                self.track_live_object(&instance);
                return Ok(instance);
            }
        }

        // Built-in Iterator / Collection / Paginator classes
        if clean_name == "ArrayIterator"
            || clean_name == "Collection"
            || clean_name == "Illuminate\\Support\\Collection"
            || clean_name == "LengthAwarePaginator"
            || clean_name == "Paginator"
            || clean_name == "Illuminate\\Pagination\\LengthAwarePaginator"
            || clean_name == "Illuminate\\Pagination\\Paginator"
        {
            if !self.module.class_index.contains_key(clean_name) {
                obj.set_property("items", PhpValue::list(vec![]));
                obj.set_property("cursor", PhpValue::Int(0));
                let instance = PhpValue::object(obj);
                self.track_live_object(&instance);
                return Ok(instance);
            }
        }

        // Collect inheritance chain from root ancestor down to `class_name`
        let mut hierarchy_chain = Vec::new();
        let mut cursor = Some(clean_name.to_string());
        while let Some(ref current_cls_name) = cursor {
            if let Some(cls) = self.module.get_class_by_name(current_cls_name).cloned() {
                if let Some(ref parent_name) = cls.extends {
                    if let Some(parent_cls) = self.module.get_class_by_name(parent_name) {
                        if parent_cls.modifiers.is_shared && !cls.modifiers.is_shared {
                            return Err(self.make_error(
                                RuntimeErrorKind::NonSharedExtendsSharedForbidden {
                                    class_name: current_cls_name.to_string(),
                                    parent_name: parent_name.to_string(),
                                },
                                span,
                            ));
                        }
                    }
                }
                let parent = cls.extends.clone();
                hierarchy_chain.push(cls);
                cursor = parent;
            } else if current_cls_name == clean_name {
                return Err(self.make_error(
                    RuntimeErrorKind::UndefinedClass {
                        name: clean_name.to_string(),
                    },
                    span,
                ));
            } else {
                break;
            }
        }

        for cls in hierarchy_chain.into_iter().rev() {
            // Initialize trait properties
            for trait_name in &cls.traits {
                if let Some(tr) = self.module.get_class_by_name(trait_name).cloned() {
                    for prop in &tr.properties {
                        if !prop.is_static {
                            let val = if let Some(ref chunk) = prop.default_chunk {
                                self.eval_isolated_chunk(chunk)?
                            } else {
                                PhpValue::Null
                            };
                            obj.set_property(prop.name.clone(), val);
                            obj.readonly_props.insert(prop.name.clone(), prop.is_readonly);
                        }
                    }
                }
            }

            // Initialize class instance properties
            for prop in &cls.properties {
                if !prop.is_static {
                    let val = if let Some(ref chunk) = prop.default_chunk {
                        self.eval_isolated_chunk(chunk)?
                    } else {
                        PhpValue::Null
                    };
                    obj.set_property(prop.name.clone(), val);
                    obj.readonly_props.insert(prop.name.clone(), prop.is_readonly);
                }
            }
        }

        let instance = PhpValue::object(obj);
        self.track_live_object(&instance);
        Ok(instance)
    }

    fn exec_enum_static_method(
        &mut self,
        class_name: &str,
        method_name: &str,
        args: &[PhpValue],
        stop_depth: usize,
        span: Span,
    ) -> Result<EnumStaticCallResult, RuntimeError> {
        let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
        let enm_opt = self
            .module
            .enums
            .get(clean)
            .or_else(|| self.module.enums.get(class_name))
            .or_else(|| {
                self.module
                    .enums
                    .values()
                    .find(|e| e.name.eq_ignore_ascii_case(clean))
            })
            .cloned();

        let enm = match enm_opt {
            Some(e) => e,
            None => return Ok(EnumStaticCallResult::NotAnEnumMethod),
        };

        match method_name.to_ascii_lowercase().as_str() {
            "cases" => {
                let mut list = Vec::with_capacity(enm.cases.len());
                for case in &enm.cases {
                    let case_val = self
                        .class_constants
                        .get(&(enm.name.clone(), case.name.clone()))
                        .or_else(|| self.class_constants.get(&(clean.to_string(), case.name.clone())))
                        .or_else(|| {
                            let short = enm.name.split('\\').last().unwrap_or(&enm.name);
                            self.class_constants.get(&(short.to_string(), case.name.clone()))
                        })
                        .cloned();
                    if let Some(val) = case_val {
                        list.push(val);
                    }
                }
                Ok(EnumStaticCallResult::Handled(PhpValue::list(list)))
            }
            "from" => {
                if enm.backing_type.is_none() {
                    return Err(self.make_error(
                        RuntimeErrorKind::UndefinedMethod {
                            class: class_name.to_string(),
                            method: "from".to_string(),
                        },
                        span,
                    ));
                }
                if args.is_empty() {
                    return Err(self.make_error(
                        RuntimeErrorKind::TooFewArguments {
                            callable: format!("{}::from", class_name),
                            passed: 0,
                            expected: 1,
                        },
                        span,
                    ));
                }
                let target_val = &args[0];

                if let Some(ref back_ty) = enm.backing_type {
                    if !target_val.matches_type_contract(back_ty, self.type_hierarchy.as_ref()) {
                        return Err(self.make_error(
                            RuntimeErrorKind::TypeContractViolation {
                                expected: back_ty.clone(),
                                actual: self.runtime_type_label(target_val),
                                value_preview: format!("{:?}", target_val),
                            },
                            span,
                        ));
                    }
                }

                let mut matched = None;
                for case in &enm.cases {
                    let case_val = self
                        .class_constants
                        .get(&(enm.name.clone(), case.name.clone()))
                        .or_else(|| self.class_constants.get(&(clean.to_string(), case.name.clone())))
                        .or_else(|| {
                            let short = enm.name.split('\\').last().unwrap_or(&enm.name);
                            self.class_constants.get(&(short.to_string(), case.name.clone()))
                        });
                    if let Some(val) = case_val {
                        if let PhpValue::Object(rc) = val {
                            let obj = rc.borrow();
                            if let Some(backing_val) = obj.get_property("value") {
                                if backing_val == target_val {
                                    matched = Some(val.clone());
                                    break;
                                }
                            }
                        }
                    }
                }

                if let Some(case_val) = matched {
                    Ok(EnumStaticCallResult::Handled(case_val))
                } else {
                    let val_preview = match target_val {
                        PhpValue::String(s) => format!("\"{}\"", s),
                        other => other.to_php_string(),
                    };
                    let msg = format!(
                        "{} is not a valid backing value for enum {}",
                        val_preview, enm.name
                    );
                    let mut err_obj = VmObject::new("ValueError");
                    err_obj.set_property("message", PhpValue::string(msg.clone()));
                    if self.try_dispatch_to_catch(PhpValue::object(err_obj), stop_depth) {
                        Ok(EnumStaticCallResult::DispatchedToCatch)
                    } else {
                        Err(self.make_error(
                            RuntimeErrorKind::ValueError { message: msg },
                            span,
                        ))
                    }
                }
            }
            "tryfrom" => {
                if enm.backing_type.is_none() {
                    return Err(self.make_error(
                        RuntimeErrorKind::UndefinedMethod {
                            class: class_name.to_string(),
                            method: "tryFrom".to_string(),
                        },
                        span,
                    ));
                }
                if args.is_empty() {
                    return Err(self.make_error(
                        RuntimeErrorKind::TooFewArguments {
                            callable: format!("{}::tryFrom", class_name),
                            passed: 0,
                            expected: 1,
                        },
                        span,
                    ));
                }
                let target_val = &args[0];

                if !target_val.is_null() {
                    if let Some(ref back_ty) = enm.backing_type {
                        if !target_val.matches_type_contract(back_ty, self.type_hierarchy.as_ref()) {
                            return Err(self.make_error(
                                RuntimeErrorKind::TypeContractViolation {
                                    expected: back_ty.clone(),
                                    actual: self.runtime_type_label(target_val),
                                    value_preview: format!("{:?}", target_val),
                                },
                                span,
                            ));
                        }
                    }
                }

                let mut matched = None;
                for case in &enm.cases {
                    let case_val = self
                        .class_constants
                        .get(&(enm.name.clone(), case.name.clone()))
                        .or_else(|| self.class_constants.get(&(clean.to_string(), case.name.clone())))
                        .or_else(|| {
                            let short = enm.name.split('\\').last().unwrap_or(&enm.name);
                            self.class_constants.get(&(short.to_string(), case.name.clone()))
                        });
                    if let Some(val) = case_val {
                        if let PhpValue::Object(rc) = val {
                            let obj = rc.borrow();
                            if let Some(backing_val) = obj.get_property("value") {
                                if backing_val == target_val {
                                    matched = Some(val.clone());
                                    break;
                                }
                            }
                        }
                    }
                }

                Ok(EnumStaticCallResult::Handled(
                    matched.unwrap_or(PhpValue::Null),
                ))
            }
            _ => Ok(EnumStaticCallResult::NotAnEnumMethod),
        }
    }

    fn dispatch_instance_method(
        &mut self,
        receiver: PhpValue,
        method_name: String,
        staged: Vec<StagedArg>,
        base_reg: usize,
        dst: Reg,
        stop_depth: usize,
        span: Span,
    ) -> Result<bool, RuntimeError> {
        let flat_args = Self::flatten_positional_args(&staged);

        match &receiver {
            PhpValue::Object(obj_rc) => {
                let cls_name = obj_rc.borrow().class_name.clone();

                let is_throwable_final_method = match method_name.as_str() {
                    "getFile" | "getLine" | "getTrace" | "getPrevious" | "source"
                    | "rootCause" | "getRootCause" | "chain" | "getChain"
                    | "getMessage" | "getCode" | "getTraceAsString" | "getSeverity" => {
                        if let PhpValue::Object(o) = &receiver {
                            o.borrow().throwable.is_some() || self.is_class_instance_of(&cls_name, "Throwable")
                        } else {
                            false
                        }
                    }
                    _ => false,
                };

                if is_throwable_final_method {
                    let res = self.exec_builtin_object_method(
                        &cls_name,
                        &receiver,
                        &method_name,
                        &flat_args,
                        span,
                    )?;
                    self.registers[base_reg + dst.index()] = res;
                    return Ok(false);
                }

                if let Some(target_id) = self.resolve_method_id(&cls_name, &method_name) {
                    self.push_compiled_call_frame(
                        target_id,
                        Some(receiver),
                        &staged,
                        &[],
                        Some(base_reg + dst.index()),
                        Some(cls_name),
                        span,
                    )?;
                    return Ok(true);
                } else {
                    // Dynamic Magic Fallback: Check for instance __call
                    let call_mid = self
                        .module
                        .get_class_info_by_name(&cls_name)
                        .and_then(|ci| ci.call_slot)
                        .or_else(|| self.resolve_method_id(&cls_name, "__call"));

                    if let Some(call_mid) = call_mid {
                        let call_args = vec![
                            StagedArg::simple(PhpValue::string(method_name.clone())),
                            StagedArg::simple(PhpValue::list(flat_args)),
                        ];
                        self.push_compiled_call_frame(
                            call_mid,
                            Some(receiver),
                            &call_args,
                            &[],
                            Some(base_reg + dst.index()),
                            Some(cls_name),
                            span,
                        )?;
                        return Ok(true);
                    }

                    match self.exec_enum_static_method(
                        &cls_name,
                        &method_name,
                        &flat_args,
                        stop_depth,
                        span,
                    )? {
                        EnumStaticCallResult::Handled(res) => {
                            self.registers[base_reg + dst.index()] = res;
                            return Ok(false);
                        }
                        EnumStaticCallResult::DispatchedToCatch => {
                            return Ok(true);
                        }
                        EnumStaticCallResult::NotAnEnumMethod => {}
                    }
                    let res = self.exec_builtin_object_method(
                        &cls_name,
                        &receiver,
                        &method_name,
                        &flat_args,
                        span,
                    )?;
                    self.registers[base_reg + dst.index()] = res;
                    return Ok(false);
                }
            }
            _ => {
                if let PhpValue::String(ref cls_str) = receiver {
                    let cls_name = cls_str.as_str();
                    match self.exec_enum_static_method(
                        cls_name,
                        &method_name,
                        &flat_args,
                        stop_depth,
                        span,
                    )? {
                        EnumStaticCallResult::Handled(res) => {
                            self.registers[base_reg + dst.index()] = res;
                            return Ok(false);
                        }
                        EnumStaticCallResult::DispatchedToCatch => {
                            return Ok(true);
                        }
                        EnumStaticCallResult::NotAnEnumMethod => {}
                    }

                    if let Some(target_id) = self.resolve_method_id(cls_name, &method_name) {
                        self.push_compiled_call_frame(
                            target_id,
                            None,
                            &staged,
                            &[],
                            Some(base_reg + dst.index()),
                            Some(cls_name.to_string()),
                            span,
                        )?;
                        return Ok(true);
                    }

                    // Check __forward:
                    let fwd_mid = self
                        .module
                        .get_class_info_by_name(cls_name)
                        .and_then(|ci| ci.static_forward)
                        .or_else(|| self.resolve_method_id(cls_name, "__forward"));

                    if let Some(fwd_id) = fwd_mid {
                        let recv = self.invoke_compiled_function_sync_with_called_class(
                            fwd_id,
                            vec![],
                            Some(cls_name.to_string()),
                            span,
                        )?;
                        if let PhpValue::Object(ref rc) = recv {
                            rc.borrow_mut().forwarded_from = Some(cls_name.to_string());
                            let recv_cls = rc.borrow().class_name.clone();
                            if let Some(target_mid) = self.resolve_method_id(&recv_cls, &method_name) {
                                self.push_compiled_call_frame(
                                    target_mid,
                                    Some(recv),
                                    &staged,
                                    &[],
                                    Some(base_reg + dst.index()),
                                    Some(recv_cls),
                                    span,
                                )?;
                                return Ok(true);
                            } else {
                                match self.exec_builtin_object_method(
                                    &recv_cls,
                                    &recv,
                                    &method_name,
                                    &flat_args,
                                    span,
                                ) {
                                    Ok(res) => {
                                        self.registers[base_reg + dst.index()] = res;
                                        return Ok(false);
                                    }
                                    Err(_) => {
                                        return Err(self.make_error(
                                            RuntimeErrorKind::UndefinedForwardedMethod {
                                                class: recv_cls,
                                                method: method_name,
                                                origin_class: cls_name.to_string(),
                                            },
                                            span,
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    // Check __callStatic:
                    if let Some(call_static_mid) = self.resolve_method_id(cls_name, "__callStatic") {
                        let call_static_args = vec![
                            StagedArg::simple(PhpValue::string(method_name.clone())),
                            StagedArg::simple(PhpValue::list(flat_args)),
                        ];
                        self.push_compiled_call_frame(
                            call_static_mid,
                            None,
                            &call_static_args,
                            &[],
                            Some(base_reg + dst.index()),
                            Some(cls_name.to_string()),
                            span,
                        )?;
                        return Ok(true);
                    }
                }
                let res = match self.exec_fluent_value_method(
                    &receiver,
                    &method_name,
                    &flat_args,
                    span,
                ) {
                    Ok(v) => v,
                    Err(err) => {
                        if matches!(err.kind, RuntimeErrorKind::Exit(_) | RuntimeErrorKind::Die(_)) {
                            return Err(err);
                        }
                        let (cls, msg) = match &err.kind {
                            RuntimeErrorKind::DivisionByZero => (
                                "DivisionByZeroError".to_string(),
                                "Division by zero".to_string(),
                            ),
                            RuntimeErrorKind::UncaughtException { class, message } => {
                                (class.clone(), message.clone())
                            }
                            RuntimeErrorKind::Custom { message } => {
                                ("Exception".to_string(), message.clone())
                            }
                            RuntimeErrorKind::TypeError { message } => {
                                ("TypeError".to_string(), message.clone())
                            }
                            _ => ("Exception".to_string(), err.kind.to_string()),
                        };
                        let mut obj = VmObject::new(&cls);
                        obj.set_property("message", PhpValue::string(msg));
                        if self.try_dispatch_to_catch(PhpValue::object(obj), stop_depth) {
                            return Ok(true);
                        } else {
                            return Err(err);
                        }
                    }
                };
                self.registers[base_reg + dst.index()] = res;
                Ok(false)
            }
        }
    }

    fn resolve_method_id(&self, class_name: &str, method_name: &str) -> Option<FunctionId> {
        let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
        // Fast Tier 2 path: O(1) lookup from the linked runtime class table flat vtable
        if let Some(ci) = self.module.get_class_info_by_name(clean) {
            if let Some(&mid) = ci.vtable.get(method_name) {
                return Some(mid);
            }
        }
        let mut cursor = Some(clean);
        while let Some(cls_name) = cursor {
            if let Some(cls) = self.module.get_class_by_name(cls_name) {
                if let Some(&fn_id) = cls.methods.get(method_name) {
                    return Some(fn_id);
                }
                for trait_name in &cls.traits {
                    if let Some(tr) = self.module.get_class_by_name(trait_name) {
                        if let Some(&fn_id) = tr.methods.get(method_name) {
                            return Some(fn_id);
                        }
                    }
                }
                cursor = cls.extends.as_deref();
            } else if let Some(enm) = self.module.enums.get(cls_name) {
                return enm.methods.get(method_name).copied();
            } else {
                break;
            }
        }
        None
    }

    fn is_class_instance_of(&self, actual_cls: &str, target_cls: &str) -> bool {
        let actual_clean = actual_cls.strip_prefix('\\').unwrap_or(actual_cls);
        let target_clean = target_cls.strip_prefix('\\').unwrap_or(target_cls);
        if actual_clean == target_clean || target_clean == "object" {
            return true;
        }
        if (actual_clean == "Duration" && target_clean == "CarbonInterval")
            || (actual_clean == "CarbonInterval" && target_clean == "Duration")
        {
            return true;
        }
        if let Some(ref h) = self.type_hierarchy {
            if h.is_subtype_of(actual_clean, target_clean) || h.is_subtype_of(actual_cls, target_cls) {
                return true;
            }
        }
        // Walk compiled class extends / implements / traits
        let mut cursor = Some(actual_clean);
        while let Some(cls_name) = cursor {
            if cls_name == target_clean {
                return true;
            }
            if is_throwable_subtype(cls_name, target_clean) {
                return true;
            }
            if let Some(cls) = self.module.get_class_by_name(cls_name) {
                if cls.implements.iter().any(|i| {
                    let ic = i.strip_prefix('\\').unwrap_or(i);
                    ic == target_clean
                        || i == target_cls
                        || (target_clean == "Traversable"
                            && (ic == "IteratorAggregate" || ic == "Iterator" || ic == "SeekableIterator"))
                }) || cls.traits.iter().any(|t| {
                    let tc = t.strip_prefix('\\').unwrap_or(t);
                    tc == target_clean || t == target_cls
                }) {
                    return true;
                }
                cursor = cls.extends.as_deref();
            } else if let Some(enm) = self.module.enums.get(cls_name) {
                if target_clean == "UnitEnum" {
                    return true;
                }
                if enm.backing_type.is_some() && (target_clean == "BackedEnum" || target_clean == "UnitEnum") {
                    return true;
                }
                if enm.implements.iter().any(|i| {
                    let ic = i.strip_prefix('\\').unwrap_or(i);
                    ic == target_clean || i == target_cls
                }) {
                    return true;
                }
                break;
            } else {
                break;
            }
        }

        let is_target_traversable = target_clean == "Traversable";
        let is_target_iter_agg = target_clean == "IteratorAggregate";
        let is_target_iterator = target_clean == "Iterator" || target_clean == "SeekableIterator";
        let is_target_array_access = target_clean == "ArrayAccess";
        let is_target_countable = target_clean == "Countable";

        if actual_clean == "ArrayIterator" {
            if is_target_traversable || is_target_iterator || is_target_array_access || is_target_countable {
                return true;
            }
        }
        if actual_clean == "Collection" || actual_clean == "Illuminate\\Support\\Collection" {
            if is_target_traversable || is_target_iter_agg || is_target_array_access || is_target_countable {
                return true;
            }
        }
        if actual_clean == "LengthAwarePaginator"
            || actual_clean == "Paginator"
            || actual_clean == "Illuminate\\Pagination\\LengthAwarePaginator"
            || actual_clean == "Illuminate\\Pagination\\Paginator"
        {
            if is_target_traversable || is_target_iter_agg || is_target_array_access || is_target_countable {
                return true;
            }
        }

        if is_throwable_subtype(actual_clean, target_clean) {
            return true;
        }
        false
    }

    fn unwind_exception(
        &mut self,
        exc_val: PhpValue,
        stop_depth: usize,
        span: Span,
    ) -> Result<(), RuntimeError> {
        if self.try_dispatch_to_catch(exc_val.clone(), stop_depth) {
            return Ok(());
        }

        let (exc_class, exc_msg) = match &exc_val {
            PhpValue::Object(o) => {
                let b = o.borrow();
                let msg = b
                    .get_property("message")
                    .map(|v| v.to_php_string())
                    .unwrap_or_default();
                (b.class_name.clone(), msg)
            }
            other => ("Exception".to_string(), other.to_php_string()),
        };

        if let Some(handler) = self.exception_handlers.last().cloned() {
            let res = self.invoke_callable_sync(&handler, vec![exc_val], span);
            if let Err(handler_err) = res {
                return Err(handler_err);
            }
            return Err(self.make_error(
                RuntimeErrorKind::UncaughtExceptionHandled {
                    class: exc_class,
                    message: exc_msg,
                },
                span,
            ));
        }

        Err(self.make_error(
            RuntimeErrorKind::UncaughtException {
                class: exc_class,
                message: exc_msg,
            },
            span,
        ))
    }

    fn try_dispatch_to_catch(&mut self, exc_val: PhpValue, stop_depth: usize) -> bool {
        let exc_class = match &exc_val {
            PhpValue::Object(o) => o.borrow().class_name.clone(),
            _ => "Exception".to_string(),
        };

        while self.frames.len() > stop_depth {
            let frame_idx = self.frames.len() - 1;
            let func_id = self.frames[frame_idx].func_id;
            let base_reg = self.frames[frame_idx].base_reg;

            while let Some(handler_id) = self.frames[frame_idx].active_try_handlers.pop() {
                let handler_opt = self
                    .chunk_for_func(func_id)
                    .exception_handlers
                    .get(handler_id as usize)
                    .cloned();

                if let Some(handler) = handler_opt {
                    for catch in &handler.catches {
                        let matches = catch.catch_types.is_empty()
                            || catch.catch_types.iter().any(|ct| {
                                self.is_class_instance_of(&exc_class, ct)
                            });
                        if matches {
                            self.registers[base_reg + catch.exception_reg.index()] = exc_val;
                            self.frames[frame_idx].ip = catch.target_ip as usize;
                            return true;
                        }
                    }
                }
            }

            let popped = self.frames.pop().unwrap();
            self.registers.truncate(popped.base_reg);
        }

        false
    }

    // =========================================================================
    // Pure Unary & Binary Instruction Execution
    // =========================================================================

    fn exec_unary(
        &self,
        op: IrUnaryOp,
        src: &PhpValue,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        match op {
            IrUnaryOp::Plus => match src {
                PhpValue::Int(_) | PhpValue::Float(_) => Ok(src.clone()),
                other => {
                    if let Some(i) = other.to_i64() {
                        Ok(PhpValue::Int(i))
                    } else if let Some(f) = other.to_f64() {
                        Ok(PhpValue::float(f))
                    } else {
                        Err(self.make_error(
                            RuntimeErrorKind::InvalidUnaryOperand {
                                op: "+".to_string(),
                                operand_type: other.type_name().to_string(),
                            },
                            span,
                        ))
                    }
                }
            },
            IrUnaryOp::Neg => match src {
                value if value.is_integer_value() || matches!(value, PhpValue::Bool(_)) => {
                    let n = crate::vm::numeric::int_operand(value)
                        .map_err(|fail| self.arith_error(fail, span))?;
                    let negated = crate::vm::numeric::checked_int_neg(n)
                        .map_err(|fail| self.arith_error(fail, span))?;
                    Ok(PhpValue::Int(negated))
                }
                value if crate::vm::numeric::is_float_like(value) => {
                    let zero = PhpValue::Int(0);
                    crate::vm::numeric::float_binary(BinaryOp::Sub, &zero, value)
                        .map_err(|fail| self.arith_error(fail, span))
                }
                other => {
                    if let Some(i) = other.to_i64() {
                        let negated = crate::vm::numeric::checked_int_neg(i)
                            .map_err(|fail| self.arith_error(fail, span))?;
                        Ok(PhpValue::Int(negated))
                    } else {
                        Err(self.make_error(
                            RuntimeErrorKind::InvalidUnaryOperand {
                                op: "-".to_string(),
                                operand_type: other.type_name().to_string(),
                            },
                            span,
                        ))
                    }
                }
            },
            IrUnaryOp::Not => Ok(PhpValue::Bool(!src.is_truthy())),
            IrUnaryOp::BitNot => {
                if let Some(n) = src.to_i64() {
                    Ok(PhpValue::Int(!n))
                } else {
                    Err(self.make_error(
                        RuntimeErrorKind::InvalidUnaryOperand {
                            op: "~".to_string(),
                            operand_type: src.type_name().to_string(),
                        },
                        span,
                    ))
                }
            }
            IrUnaryOp::Silence => Ok(src.clone()),
        }
    }

    fn exec_binary(
        &self,
        op: BinaryOp,
        lhs: &PhpValue,
        rhs: &PhpValue,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        match op {
            BinaryOp::Concat => {
                if lhs.is_bytes() && rhs.is_bytes() {
                    return Err(self.make_error(
                        RuntimeErrorKind::TypeError {
                            message: "Cannot concatenate bytes using the '.' operator. Use BytesBuffer or decode to string first.".to_string(),
                        },
                        span,
                    ));
                }
                if lhs.is_bytes() || rhs.is_bytes() {
                    return Err(self.make_error(
                        RuntimeErrorKind::TypeError {
                            message: "Cannot concatenate string and bytes. Decode the bytes or convert the string explicitly.".to_string(),
                        },
                        span,
                    ));
                }
                let mut s = lhs.to_php_string();
                s.push_str(&rhs.to_php_string());
                Ok(PhpValue::string(s))
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
                self.exec_numeric_binary(op, lhs, rhs, span)
            }

            // Bitwise operators
            BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr => {
                let a = lhs.to_i64().unwrap_or(0);
                let b = rhs.to_i64().unwrap_or(0);
                let res = match op {
                    BinaryOp::BitAnd => a & b,
                    BinaryOp::BitOr => a | b,
                    BinaryOp::BitXor => a ^ b,
                    BinaryOp::Shl => a.wrapping_shl(b as u32),
                    BinaryOp::Shr => a.wrapping_shr(b as u32),
                    _ => unreachable!(),
                };
                Ok(PhpValue::Int(res))
            }

            // Equality & Identity comparisons (strict equality)
            BinaryOp::Eq => Ok(PhpValue::Bool(Self::strict_eq(lhs, rhs))),
            BinaryOp::NotEq => Ok(PhpValue::Bool(!Self::strict_eq(lhs, rhs))),
            BinaryOp::Identical => Ok(PhpValue::Bool(Self::strict_eq(lhs, rhs))),
            BinaryOp::NotIdentical => Ok(PhpValue::Bool(!Self::strict_eq(lhs, rhs))),

            // Relational & Spaceship comparisons
            BinaryOp::Lt => Ok(PhpValue::Bool(lhs < rhs)),
            BinaryOp::LtEq => Ok(PhpValue::Bool(lhs <= rhs)),
            BinaryOp::Gt => Ok(PhpValue::Bool(lhs > rhs)),
            BinaryOp::GtEq => Ok(PhpValue::Bool(lhs >= rhs)),
            BinaryOp::Spaceship => {
                let cmp = match lhs.partial_cmp(rhs) {
                    Some(std::cmp::Ordering::Less) => -1,
                    Some(std::cmp::Ordering::Equal) => 0,
                    Some(std::cmp::Ordering::Greater) => 1,
                    None => 0,
                };
                Ok(PhpValue::Int(cmp))
            }

            // Logical XOR (`xor`)
            BinaryOp::WordXor => Ok(PhpValue::Bool(lhs.is_truthy() ^ rhs.is_truthy())),
            BinaryOp::LogicalAnd | BinaryOp::WordAnd => {
                Ok(PhpValue::Bool(lhs.is_truthy() && rhs.is_truthy()))
            }
            BinaryOp::LogicalOr | BinaryOp::WordOr => {
                Ok(PhpValue::Bool(lhs.is_truthy() || rhs.is_truthy()))
            }
            BinaryOp::Coalesce => {
                if !lhs.is_null() {
                    Ok(lhs.clone())
                } else {
                    Ok(rhs.clone())
                }
            }
            BinaryOp::Range => {
                dispatch_facade_call("Arr", "range", &[lhs.clone(), rhs.clone()], span)
            }
        }
    }

    #[inline]
    fn strict_eq(a: &PhpValue, b: &PhpValue) -> bool {
        a.strict_eq(b)
    }


    // =========================================================================
    // Internal Utilities
    // =========================================================================

    #[inline]
    fn chunk_for_func(&self, func_id: Option<FunctionId>) -> &BytecodeChunk {
        match func_id {
            Some(id) => &self.module.functions[id.index()].chunk,
            None => &self.module.main_chunk,
        }
    }

    fn arith_error(&self, fail: crate::vm::numeric::ArithFail, span: Span) -> RuntimeError {
        let kind = match fail {
            crate::vm::numeric::ArithFail::Overflow => RuntimeErrorKind::IntegerOverflow,
            crate::vm::numeric::ArithFail::DecimalOverflow => RuntimeErrorKind::DecimalOverflow {
                digits: crate::vm::numeric::DECIMAL_MAX_DIGITS,
            },
            crate::vm::numeric::ArithFail::DivideByZero => RuntimeErrorKind::DivisionByZero,
            crate::vm::numeric::ArithFail::NotNumeric => RuntimeErrorKind::Custom {
                message: "numeric operation expected a number".to_string(),
            },
        };
        self.make_error(kind, span)
    }

    fn exec_inc_dec(&self, current: &PhpValue, delta: i64, span: Span) -> Result<PhpValue, RuntimeError> {
        if matches!(current, PhpValue::Null) {
            return Ok(PhpValue::Int(if delta > 0 { 1 } else { 0 }));
        }
        if crate::vm::numeric::is_float_like(current) {
            return crate::vm::numeric::float_binary(BinaryOp::Add, current, &PhpValue::Int(delta))
                .map_err(|fail| self.arith_error(fail, span));
        }
        let n = crate::vm::numeric::int_operand(current).map_err(|fail| self.arith_error(fail, span))?;
        let updated = crate::vm::numeric::checked_int_add(n, delta).map_err(|fail| self.arith_error(fail, span))?;
        Ok(PhpValue::Int(updated))
    }

    fn exec_numeric_binary(
        &self,
        op: BinaryOp,
        lhs: &PhpValue,
        rhs: &PhpValue,
        span: Span,
    ) -> Result<PhpValue, RuntimeError> {
        let operand_err = || {
            self.make_error(
                RuntimeErrorKind::InvalidBinaryOperands {
                    op: op.to_string(),
                    lhs_type: lhs.type_name().to_string(),
                    rhs_type: rhs.type_name().to_string(),
                },
                span,
            )
        };
        if crate::vm::numeric::is_float_like(lhs) || crate::vm::numeric::is_float_like(rhs) {
            return crate::vm::numeric::float_binary(op, lhs, rhs).map_err(|fail| match fail {
                crate::vm::numeric::ArithFail::NotNumeric => operand_err(),
                other => self.arith_error(other, span),
            });
        }
        let a = match crate::vm::numeric::int_operand(lhs) {
            Ok(n) => n,
            Err(crate::vm::numeric::ArithFail::NotNumeric) => return Err(operand_err()),
            Err(fail) => return Err(self.arith_error(fail, span)),
        };
        let b = match crate::vm::numeric::int_operand(rhs) {
            Ok(n) => n,
            Err(crate::vm::numeric::ArithFail::NotNumeric) => return Err(operand_err()),
            Err(fail) => return Err(self.arith_error(fail, span)),
        };
        match crate::vm::numeric::checked_int_op(op, a, b) {
            Ok(n) => Ok(PhpValue::Int(n)),
            Err(crate::vm::numeric::ArithFail::NotNumeric) => {
                crate::vm::numeric::float_binary(op, lhs, rhs).map_err(|fail| match fail {
                    crate::vm::numeric::ArithFail::NotNumeric => operand_err(),
                    other => self.arith_error(other, span),
                })
            }
            Err(fail) => Err(self.arith_error(fail, span)),
        }
    }

    fn constant_to_value(c: &Constant) -> PhpValue {
        match c {
            Constant::Null => PhpValue::Null,
            Constant::Bool(b) => PhpValue::Bool(*b),
            Constant::Int(n) => PhpValue::Int(*n),
            Constant::Float(f) => PhpValue::float(*f),
            Constant::Decimal(raw) => raw
                .parse::<f64>()
                .map(PhpValue::float)
                .unwrap_or_else(|_| PhpValue::string(raw.clone())),
            Constant::String(s) => PhpValue::string(s.clone()),
            Constant::Bytes(b) => PhpValue::bytes(b.clone()),
            Constant::Type(ty) => PhpValue::string(ty.to_string()),
        }
    }

    fn drain_staged_args(&mut self, count: usize) -> Vec<StagedArg> {
        let start = self.arg_buffer.len().saturating_sub(count);
        self.arg_buffer.drain(start..).collect()
    }

    fn flatten_positional_args(staged: &[StagedArg]) -> Vec<PhpValue> {
        let mut out = Vec::with_capacity(staged.len());
        for arg in staged {
            if arg.is_unpack {
                out.extend(Self::extract_items(&arg.value));
            } else {
                out.push(arg.value.clone());
            }
        }
        out
    }

    fn extract_items(val: &PhpValue) -> Vec<PhpValue> {
        match val {
            PhpValue::EmptyArray => Vec::new(),
            PhpValue::Array(a) | PhpValue::List(a) => a.borrow().clone(),
            PhpValue::Set(s) => s.borrow().iter().cloned().collect(),
            PhpValue::Dict(d) => d.borrow().values().cloned().collect(),
            PhpValue::HashMap(m) => m.borrow().values().cloned().collect(),
            PhpValue::Null => Vec::new(),
            other => vec![other.clone()],
        }
    }

    fn extract_dict_entries(val: &PhpValue) -> IndexMap<String, PhpValue> {
        extract_dict_map(val)
    }

    fn extract_map_entries(val: &PhpValue) -> Vec<(PhpValue, PhpValue)> {
        match val {
            PhpValue::HashMap(m) => m
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            PhpValue::Dict(d) => d
                .borrow()
                .iter()
                .map(|(k, v)| (PhpValue::string(k.clone()), v.clone()))
                .collect(),
            PhpValue::Array(a) | PhpValue::List(a) => {
                let b = a.borrow();
                let all_pairs = !b.is_empty()
                    && b.iter().all(|item| match item {
                        PhpValue::Array(sub) | PhpValue::List(sub) => sub.borrow().len() == 2,
                        _ => false,
                    });
                if all_pairs {
                    b.iter()
                        .map(|item| match item {
                            PhpValue::Array(sub) | PhpValue::List(sub) => {
                                let sub_b = sub.borrow();
                                (sub_b[0].clone(), sub_b[1].clone())
                            }
                            _ => unreachable!(),
                        })
                        .collect()
                } else {
                    b.iter()
                        .enumerate()
                        .map(|(i, v)| (PhpValue::Int(i as i64), v.clone()))
                        .collect()
                }
            }
            _ => Vec::new(),
        }
    }

    fn runtime_type_label(&self, val: &PhpValue) -> String {
        match val {
            PhpValue::Object(o) => o.borrow().class_name.clone(),
            other => other.type_name().to_string(),
        }
    }

    fn make_error(&self, kind: RuntimeErrorKind, span: Span) -> RuntimeError {
        let mut trace = Vec::with_capacity(self.frames.len());
        for frame in self.frames.iter().rev() {
            let ip = frame.ip.saturating_sub(1);
            let frame_span = self
                .chunk_for_func(frame.func_id)
                .span_at(ip)
                .unwrap_or(span);
            trace.push(StackTraceEntry {
                function_name: frame.function_name.clone(),
                ip,
                span: frame_span,
            });
        }
        RuntimeError::with_stack_trace(kind, span, trace)
    }
}

impl From<BytecodeChunk> for IrModule {
    fn from(chunk: BytecodeChunk) -> Self {
        Self {
            main_chunk: chunk,
            ..Default::default()
        }
    }
}

