//! # Persistent Parent Process Architecture (`Arc<EngineState>`)
//!
//! Implements Technical Point #13 of the RHP Architecture Specification:
//! - **Persistent Parent Process Architecture**: Boot, parse, and compile application code
//!   inside a long-lived parent process into an immutable memory state (`Arc<EngineState>`)
//!   to avoid per-request bootstrap overhead.
//! - **Singleton Process Services**: If a developer needs to run something as part of the parent
//!   process, they first declare a class with the `singleton` keyword.
//! - **Singleton Invocation Invariants**: One can use both static and non-static methods of a
//!   singleton class within other singleton classes, but can only use static methods of a singleton
//!   class inside non-singleton classes.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use crate::ir::{lower_typed_program, lower_typed_program_with_base, CompilationError, IrModule};
use crate::lexer::span::Span;
use crate::type_system::types::{RustFloatKind, RustIntKind};
use crate::type_system::{
    parse_and_check, parse_and_check_with_file_id, parse_and_check_with_hierarchy, TypeHierarchy,
};
use crate::vm::engine::VM;
use crate::vm::error::RuntimeError;
use crate::vm::value::{PhpValue, VmObject};
use crate::vm::ExecutionError;

/// A thread-safe, immutable snapshot of a [`PhpValue`] stored in [`EngineState`].
///
/// Contains zero raw thread-local reference counters (`Rc`), making `EngineState`
/// safely shareable across multi-threaded worker pools and Tokio execution fibers (`Send + Sync`).
#[derive(Debug, Clone, PartialEq)]
pub enum FrozenValue {
    Null,
    Bool(bool),
    Int(i32),
    Float(f32),
    RustInt { kind: RustIntKind, value: i128 },
    RustFloat { kind: RustFloatKind, value: f64 },
    String(String),
    List(Vec<FrozenValue>),
    Dict(Vec<(String, FrozenValue)>),
    Set(Vec<FrozenValue>),
    HashMap(Vec<(FrozenValue, FrozenValue)>),
    Object {
        class_name: String,
        properties: Vec<(String, FrozenValue)>,
    },
}

impl FrozenValue {
    /// Convert an active execution [`PhpValue`] into a thread-safe [`FrozenValue`].
    pub fn from_php_value(val: &PhpValue) -> Self {
        match val {
            PhpValue::Null => FrozenValue::Null,
            PhpValue::Bool(b) => FrozenValue::Bool(*b),
            PhpValue::Int(n) => FrozenValue::Int(*n),
            PhpValue::Float(f) => FrozenValue::Float(*f),
            PhpValue::RustInt { kind, value } => FrozenValue::RustInt {
                kind: *kind,
                value: *value,
            },
            PhpValue::RustFloat { kind, value } => FrozenValue::RustFloat {
                kind: *kind,
                value: *value,
            },
            PhpValue::String(s) => FrozenValue::String(s.as_str().to_string()),
            PhpValue::Array(items) | PhpValue::List(items) => {
                let vec = items.borrow().iter().map(Self::from_php_value).collect();
                FrozenValue::List(vec)
            }
            PhpValue::Dict(d) => {
                let pairs = d
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::from_php_value(v)))
                    .collect();
                FrozenValue::Dict(pairs)
            }
            PhpValue::Set(s) => {
                let vec = s.borrow().iter().map(Self::from_php_value).collect();
                FrozenValue::Set(vec)
            }
            PhpValue::HashMap(m) => {
                let pairs = m
                    .borrow()
                    .iter()
                    .map(|(k, v)| (Self::from_php_value(k), Self::from_php_value(v)))
                    .collect();
                FrozenValue::HashMap(pairs)
            }
            PhpValue::Object(obj) => {
                let borrowed = obj.borrow();
                let class_name = borrowed.class_name.clone();
                let props = borrowed
                    .properties
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::from_php_value(v)))
                    .collect();
                FrozenValue::Object {
                    class_name,
                    properties: props,
                }
            }
            PhpValue::Future(fut) => {
                let mut b = fut.borrow_mut();
                b.poll_completion();
                if let Some(ref inner) = b.value {
                    Self::from_php_value(inner)
                } else {
                    FrozenValue::Null
                }
            }
            PhpValue::Closure(_) | PhpValue::Resource(_) => FrozenValue::Null,
        }
    }

    /// Rehydrate a [`FrozenValue`] into a thread-local [`PhpValue`].
    pub fn to_php_value(&self) -> PhpValue {
        match self {
            FrozenValue::Null => PhpValue::Null,
            FrozenValue::Bool(b) => PhpValue::Bool(*b),
            FrozenValue::Int(n) => PhpValue::Int(*n),
            FrozenValue::Float(f) => PhpValue::Float(*f),
            FrozenValue::RustInt { kind, value } => PhpValue::RustInt {
                kind: *kind,
                value: *value,
            },
            FrozenValue::RustFloat { kind, value } => PhpValue::RustFloat {
                kind: *kind,
                value: *value,
            },
            FrozenValue::String(s) => PhpValue::string(s.clone()),
            FrozenValue::List(items) => {
                PhpValue::list(items.iter().map(Self::to_php_value).collect())
            }
            FrozenValue::Dict(pairs) => {
                let mut map = indexmap::IndexMap::new();
                for (k, v) in pairs {
                    map.insert(k.clone(), v.to_php_value());
                }
                PhpValue::dict(map)
            }
            FrozenValue::Set(items) => {
                let mut set = std::collections::HashSet::new();
                for item in items {
                    set.insert(item.to_php_value());
                }
                PhpValue::set(set)
            }
            FrozenValue::HashMap(pairs) => {
                let mut map = std::collections::HashMap::new();
                for (k, v) in pairs {
                    map.insert(k.to_php_value(), v.to_php_value());
                }
                PhpValue::hash_map(map)
            }
            FrozenValue::Object {
                class_name,
                properties,
            } => {
                let mut obj = VmObject::new(class_name);
                for (k, v) in properties {
                    obj.set_property(k, v.to_php_value());
                }
                PhpValue::object(obj)
            }
        }
    }
}

impl Default for FrozenValue {
    fn default() -> Self {
        FrozenValue::Null
    }
}

impl From<PhpValue> for FrozenValue {
    fn from(val: PhpValue) -> Self {
        FrozenValue::from_php_value(&val)
    }
}

impl From<&PhpValue> for FrozenValue {
    fn from(val: &PhpValue) -> Self {
        FrozenValue::from_php_value(val)
    }
}

impl From<i32> for FrozenValue {
    fn from(n: i32) -> Self {
        FrozenValue::Int(n)
    }
}

impl From<bool> for FrozenValue {
    fn from(b: bool) -> Self {
        FrozenValue::Bool(b)
    }
}

impl From<f32> for FrozenValue {
    fn from(f: f32) -> Self {
        FrozenValue::Float(f)
    }
}

impl From<String> for FrozenValue {
    fn from(s: String) -> Self {
        FrozenValue::String(s)
    }
}

impl From<&str> for FrozenValue {
    fn from(s: &str) -> Self {
        FrozenValue::String(s.to_string())
    }
}

/// Immutable pre-bootstrapped memory state of the persistent parent process.
///
/// Shared across concurrent worker threads or fibers (`Arc<EngineState>`), eliminating
/// repetitive lexing, parsing, AST generation, type-checking, and bytecode compilation overhead.
#[derive(Debug, Clone)]
pub struct EngineState {
    /// Pre-compiled immutable bytecode module containing application and framework code.
    pub module: Arc<IrModule>,
    /// Pre-analyzed nominal type hierarchy for instant subtype and boundary verification.
    pub type_hierarchy: Arc<TypeHierarchy>,
    /// Set of class names declared with the `singleton` keyword.
    pub singleton_classes: HashSet<String>,
    /// Static class properties evaluated during parent bootstrap.
    pub static_properties: HashMap<(String, String), FrozenValue>,
    /// Cached class constants and enum cases evaluated during parent bootstrap.
    pub class_constants: HashMap<(String, String), FrozenValue>,
    /// Top-level global variables initialized during parent bootstrap.
    pub globals: HashMap<String, FrozenValue>,
    /// Captured standard output emitted during the parent boot phase.
    pub boot_output: String,
    /// Result returned by the parent process `<main>` boot script.
    pub boot_value: FrozenValue,
    /// Instant when parent boot completed.
    pub booted_at: Instant,
}

// Guarantee Send + Sync for cross-thread worker pool dispatching.
unsafe impl Send for EngineState {}
unsafe impl Sync for EngineState {}

impl EngineState {
    /// Create a fast, isolated worker [`VM`] pre-seeded with this immutable parent memory state.
    ///
    /// The worker VM immediately inherits all pre-compiled functions, classes, interfaces,
    /// and static properties without performing any parsing or compilation.
    pub fn create_worker_vm(&self) -> VM {
        let mut vm = VM::new((*self.module).clone(), false)
            .with_type_hierarchy((*self.type_hierarchy).clone());
        vm.set_static_properties(self.rehydrate_static_properties());
        vm.set_class_constants(self.rehydrate_class_constants());
        vm.set_globals(self.rehydrate_globals());
        vm
    }

    /// Check if a class was declared with the `singleton` keyword.
    #[inline]
    pub fn is_singleton_class(&self, class_name: &str) -> bool {
        self.singleton_classes.contains(class_name)
    }

    /// Execute a per-request script against the pre-compiled parent state.
    ///
    /// The request code is parsed and type-checked against the pre-existing parent [`TypeHierarchy`],
    /// and lowered into a request entry chunk linked with the parent's pre-compiled [`IrModule`].
    pub fn execute_request(&self, request_source: &str) -> Result<PhpValue, ExecutionError> {
        let (mut vm, result) = self.execute_request_with_vm(request_source)?;
        let _ = vm.finish_request();
        Ok(result)
    }

    /// Execute a per-request script, returning both the worker [`VM`] (to inspect variables)
    /// and the evaluated return value.
    pub fn execute_request_with_vm(
        &self,
        request_source: &str,
    ) -> Result<(VM, PhpValue), ExecutionError> {
        let typed = parse_and_check_with_hierarchy(
            request_source,
            (*self.type_hierarchy).clone(),
        )
        .map_err(CompilationError::from)?;

        let request_module = lower_typed_program_with_base(
            &typed,
            (*self.module).clone(),
        )
        .map_err(CompilationError::from)?;

        let mut vm = VM::new(request_module, false)
            .with_type_hierarchy(typed.type_info.hierarchy);
        vm.set_static_properties(self.rehydrate_static_properties());
        vm.set_class_constants(self.rehydrate_class_constants());
        vm.set_globals(self.rehydrate_globals());

        let result = vm.run()?;
        Ok((vm, result))
    }

    /// Invoke an already-compiled function or static method directly on a fresh worker VM
    /// with zero parsing and zero bytecode compilation overhead.
    pub fn call_function(
        &self,
        func_name: &str,
        args: Vec<PhpValue>,
    ) -> Result<PhpValue, RuntimeError> {
        let mut vm = self.create_worker_vm();
        let res = vm.invoke_function_by_name(func_name, args, Span::default());
        let _ = vm.finish_request();
        res
    }

    /// Rehydrate static properties map into active [`PhpValue`]s.
    pub fn rehydrate_static_properties(&self) -> HashMap<(String, String), PhpValue> {
        self.static_properties
            .iter()
            .map(|(k, v)| (k.clone(), v.to_php_value()))
            .collect()
    }

    /// Rehydrate class constants map into active [`PhpValue`]s.
    pub fn rehydrate_class_constants(&self) -> HashMap<(String, String), PhpValue> {
        self.class_constants
            .iter()
            .map(|(k, v)| (k.clone(), v.to_php_value()))
            .collect()
    }

    /// Rehydrate globals map into active [`PhpValue`]s.
    pub fn rehydrate_globals(&self) -> HashMap<String, PhpValue> {
        self.globals
            .iter()
            .map(|(k, v)| (k.clone(), v.to_php_value()))
            .collect()
    }

    /// Spawn a concurrent multi-threaded worker pool pre-seeded with this immutable parent memory state.
    pub fn spawn_worker_pool(
        self: &Arc<Self>,
        config: crate::vm::worker::WorkerPoolConfig,
    ) -> Result<crate::vm::worker::WorkerPool, crate::vm::worker::WorkerError> {
        crate::vm::worker::WorkerPool::new(Arc::clone(self), config)
    }

    /// Spawn a multi-threaded worker pool with `num_workers` threads.
    pub fn spawn_worker_threads(
        self: &Arc<Self>,
        num_workers: usize,
    ) -> Result<crate::vm::worker::WorkerPool, crate::vm::worker::WorkerError> {
        let config = crate::vm::worker::WorkerPoolConfig::new(num_workers)
            .with_model(crate::vm::worker::WorkerModel::MultiThreaded);
        self.spawn_worker_pool(config)
    }

    /// Spawn a process-level copy-on-write worker pool with `num_workers` workers.
    pub fn spawn_cow_workers(
        self: &Arc<Self>,
        num_workers: usize,
    ) -> Result<crate::vm::worker::WorkerPool, crate::vm::worker::WorkerError> {
        let config = crate::vm::worker::WorkerPoolConfig::new(num_workers)
            .with_model(crate::vm::worker::WorkerModel::ProcessCow);
        self.spawn_worker_pool(config)
    }

    /// Execute a set of requests concurrently across workers against the shared parent state.
    pub fn execute_concurrent(
        self: &Arc<Self>,
        requests: Vec<&str>,
    ) -> Result<Vec<crate::vm::worker::WorkerResponse>, crate::vm::worker::WorkerError> {
        let pool = self.spawn_worker_threads(requests.len().max(1))?;
        let worker_requests = requests
            .into_iter()
            .enumerate()
            .map(|(i, src)| crate::vm::worker::WorkerRequest::new(src).with_id(format!("req_{}", i)))
            .collect();
        Ok(pool.execute_batch(worker_requests))
    }
}

/// Long-lived Parent Process orchestrator managing framework boot and immutable [`EngineState`].
#[derive(Debug, Default)]
pub struct ParentProcess {
    state: Option<Arc<EngineState>>,
}

impl ParentProcess {
    /// Construct a new unbooted [`ParentProcess`] instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Current cached [`EngineState`] if booted.
    pub fn state(&self) -> Option<&Arc<EngineState>> {
        self.state.as_ref()
    }

    /// Spawn a worker pool from the current booted parent process state.
    pub fn spawn_worker_pool(
        &self,
        config: crate::vm::worker::WorkerPoolConfig,
    ) -> Result<crate::vm::worker::WorkerPool, crate::vm::worker::WorkerError> {
        let state = self.state.as_ref().ok_or(crate::vm::worker::WorkerError::PoolShutdown)?;
        state.spawn_worker_pool(config)
    }

    /// Boot, parse, and compile application code inside the long-lived parent process
    /// into an immutable memory state (`Arc<EngineState>`), avoiding per-request bootstrap overhead.
    pub fn boot(source: &str) -> Result<Arc<EngineState>, ExecutionError> {
        let typed = parse_and_check(source).map_err(CompilationError::from)?;
        Self::boot_typed(typed)
    }

    /// Boot with an explicit `file_id` for source map diagnostics.
    pub fn boot_with_file_id(source: &str, file_id: usize) -> Result<Arc<EngineState>, ExecutionError> {
        let typed = parse_and_check_with_file_id(source, file_id).map_err(CompilationError::from)?;
        Self::boot_typed(typed)
    }

    /// Boot with an explicit file path for `__FILE__` and `__DIR__` resolution.
    pub fn boot_with_path(source: &str, path: impl Into<String>) -> Result<Arc<EngineState>, ExecutionError> {
        let typed = crate::type_system::parse_and_check_with_path(source, path).map_err(CompilationError::from)?;
        Self::boot_typed(typed)
    }

    fn boot_typed(typed: crate::type_system::TypedProgram<'_>) -> Result<Arc<EngineState>, ExecutionError> {
        let mut singleton_classes = HashSet::new();
        for (name, class_info) in &typed.type_info.hierarchy.classes {
            if class_info.is_singleton {
                singleton_classes.insert(name.clone());
            }
        }

        let hierarchy = typed.type_info.hierarchy.clone();
        let module = lower_typed_program(&typed).map_err(CompilationError::from)?;

        let mut vm = VM::new(module, false)
            .with_type_hierarchy(hierarchy.clone())
            .with_parent_process(true);

        let boot_value = vm.run()?;
        let boot_output = vm.output().to_string();

        let static_properties: HashMap<(String, String), FrozenValue> = vm
            .static_properties()
            .iter()
            .map(|(k, v)| (k.clone(), FrozenValue::from_php_value(v)))
            .collect();

        let class_constants: HashMap<(String, String), FrozenValue> = vm
            .class_constants()
            .iter()
            .map(|(k, v)| (k.clone(), FrozenValue::from_php_value(v)))
            .collect();

        let globals: HashMap<String, FrozenValue> = vm
            .globals()
            .iter()
            .map(|(k, v)| (k.clone(), FrozenValue::from_php_value(v)))
            .collect();

        let compiled_module = vm.module().clone();

        let state = Arc::new(EngineState {
            module: Arc::new(compiled_module),
            type_hierarchy: Arc::new(hierarchy),
            singleton_classes,
            static_properties,
            class_constants,
            globals,
            boot_output,
            boot_value: FrozenValue::from_php_value(&boot_value),
            booted_at: Instant::now(),
        });

        Ok(state)
    }
}
