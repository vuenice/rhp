use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use parking_lot::RwLock;

use crate::ir::opcode::{ClassId, FunctionId};
use crate::ir::ClosureCategory;
use crate::type_system::types::{CollectionSubtype, RhpType, TypeHierarchy};
use crate::vm::parent_process::FrozenValue;

/// String value representation in RHP backed by `Rc<str>`.
pub type RhpString = Rc<str>;

/// Object Shape (Hidden Class) structure inspired by Bun & JavaScriptCore.
///
/// Maps property names to contiguous vector slot indices, allowing
/// predictable O(1) indexed property lookups instead of hash map bucket traversals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub id: u32,
    pub property_indices: HashMap<String, usize>,
    pub readonly_flags: HashMap<String, bool>,
}

impl Shape {
    pub fn new() -> Self {
        Self {
            id: 0,
            property_indices: HashMap::new(),
            readonly_flags: HashMap::new(),
        }
    }
}

impl Default for Shape {
    fn default() -> Self {
        Self::new()
    }
}

use crate::vm::exception::Throwable;

/// Runtime object representation with Shape-based slot indexing (Hidden Classes).
#[derive(Debug, Clone)]
pub struct VmObject {
    pub class_id: Option<ClassId>,
    pub class_name: String,
    pub shape: Rc<RefCell<Shape>>,
    pub slots: Vec<PhpValue>,
    pub properties: HashMap<String, PhpValue>,
    pub readonly_props: HashMap<String, bool>,
    /// Optional weak reference target for breaking circular reference cycles.
    pub weak_target: Option<std::rc::Weak<RefCell<VmObject>>>,
    /// Optional backing Throwable structure for PHP exception hierarchy instances.
    pub throwable: Option<Box<Throwable>>,
    /// Class from which this receiver was forwarded via __forward(), for typo diagnostics.
    pub forwarded_from: Option<String>,
    /// Indicates whether __destruct() has already been executed for this object.
    pub is_destructed: bool,
}

impl VmObject {
    pub fn new(class_name: impl Into<String>) -> Self {
        Self {
            class_id: None,
            class_name: class_name.into(),
            shape: Rc::new(RefCell::new(Shape::new())),
            slots: Vec::new(),
            properties: HashMap::new(),
            readonly_props: HashMap::new(),
            weak_target: None,
            throwable: None,
            forwarded_from: None,
            is_destructed: false,
        }
    }

    pub fn with_class_id(class_id: ClassId, class_name: impl Into<String>) -> Self {
        Self {
            class_id: Some(class_id),
            class_name: class_name.into(),
            shape: Rc::new(RefCell::new(Shape::new())),
            slots: Vec::new(),
            properties: HashMap::new(),
            readonly_props: HashMap::new(),
            weak_target: None,
            throwable: None,
            forwarded_from: None,
            is_destructed: false,
        }
    }

    /// Construct a weak reference object wrapper (`Weak::to($parent)`).
    pub fn weak_reference(target: &Rc<RefCell<VmObject>>) -> Self {
        Self {
            class_id: None,
            class_name: "WeakReference".to_string(),
            shape: Rc::new(RefCell::new(Shape::new())),
            slots: Vec::new(),
            properties: HashMap::new(),
            readonly_props: HashMap::new(),
            weak_target: Some(Rc::downgrade(target)),
            throwable: None,
            forwarded_from: None,
            is_destructed: false,
        }
    }

    #[inline]
    pub fn is_throwable(&self) -> bool {
        self.throwable.is_some()
    }

    #[inline]
    pub fn as_throwable(&self) -> Option<&Throwable> {
        self.throwable.as_deref()
    }

    #[inline]
    pub fn as_throwable_mut(&mut self) -> Option<&mut Throwable> {
        self.throwable.as_deref_mut()
    }

    #[inline]
    pub fn set_throwable(&mut self, t: Throwable) {
        self.throwable = Some(Box::new(t));
    }

    #[inline]
    pub fn get_property(&self, name: &str) -> Option<&PhpValue> {
        let shape = self.shape.borrow();
        if let Some(&idx) = shape.property_indices.get(name) {
            if idx < self.slots.len() {
                return self.slots.get(idx);
            }
        }
        self.properties.get(name)
    }

    pub fn set_property(&mut self, name: impl Into<String>, value: PhpValue) {
        let name_str = name.into();
        {
            let mut shape = self.shape.borrow_mut();
            if let Some(&idx) = shape.property_indices.get(&name_str) {
                if idx < self.slots.len() {
                    self.slots[idx] = value.clone();
                }
            } else {
                let next_idx = self.slots.len();
                shape.property_indices.insert(name_str.clone(), next_idx);
                self.slots.push(value.clone());
            }
        }
        self.properties.insert(name_str, value);
    }

    pub fn set_readonly(&mut self, name: impl Into<String>, is_ro: bool) {
        let name_str = name.into();
        self.shape.borrow_mut().readonly_flags.insert(name_str.clone(), is_ro);
        self.readonly_props.insert(name_str, is_ro);
    }

    /// Attach a native driver, connection lease, or client resource directly to this non-shared object instance.
    /// When this PHP object drops, the native resource Arc drops, triggering automatic RAII cleanup.
    pub fn set_native_resource(&mut self, name: impl Into<String>, resource: Arc<dyn std::any::Any + Send + Sync>) {
        self.set_property(name, PhpValue::Resource(resource));
    }

    /// Retrieve an attached native driver, connection lease, or client resource downcast to target type `T`.
    pub fn get_native_resource<T: Send + Sync + 'static>(&self, name: &str) -> Option<Arc<T>> {
        match self.get_property(name) {
            Some(PhpValue::Resource(arc)) => arc.clone().downcast::<T>().ok(),
            _ => None,
        }
    }
}

/// Thread-safe value representation for `shared class` state and concurrency primitives.
/// Guaranteed `Send + Sync` by the Rust compiler.
#[derive(Debug, Clone)]
pub enum SharedValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Rune(char),
    String(RhpString),
    Bytes(Arc<[u8]>),
    AtomicInt(Arc<AtomicI64>),
    AtomicBool(Arc<AtomicBool>),
    SharedObject(Arc<VmSharedObject>),
    SharedArray(Arc<RwLock<Vec<SharedValue>>>),
    SharedDict(Arc<RwLock<IndexMap<String, SharedValue>>>),
    Resource(Arc<dyn std::any::Any + Send + Sync>),
}

unsafe impl Send for SharedValue {}
unsafe impl Sync for SharedValue {}

impl SharedValue {
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Int(n) => *n != 0,
            Self::Float(f) => *f != 0.0 && !f.is_nan(),
            Self::Rune(c) => *c != '\0',
            Self::String(s) => !s.is_empty() && s.as_ref() != "0",
            Self::Bytes(b) => !b.is_empty() && &**b != b"0",
            Self::AtomicInt(a) => a.load(AtomicOrdering::SeqCst) != 0,
            Self::AtomicBool(b) => b.load(AtomicOrdering::SeqCst),
            Self::SharedObject(_) => true,
            Self::SharedArray(a) => !a.read().is_empty(),
            Self::SharedDict(d) => !d.read().is_empty(),
            Self::Resource(_) => true,
        }
    }
}

impl PartialEq for SharedValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => {
                if a.is_nan() && b.is_nan() {
                    true
                } else {
                    a == b
                }
            }
            (Self::Rune(a), Self::Rune(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::AtomicInt(a), Self::AtomicInt(b)) => {
                Arc::ptr_eq(a, b) || a.load(AtomicOrdering::SeqCst) == b.load(AtomicOrdering::SeqCst)
            }
            (Self::AtomicBool(a), Self::AtomicBool(b)) => {
                Arc::ptr_eq(a, b) || b.load(AtomicOrdering::SeqCst) == b.load(AtomicOrdering::SeqCst)
            }
            (Self::SharedObject(a), Self::SharedObject(b)) => a == b,
            (Self::SharedArray(a), Self::SharedArray(b)) => {
                Arc::ptr_eq(a, b) || *a.read() == *b.read()
            }
            (Self::SharedDict(a), Self::SharedDict(b)) => {
                Arc::ptr_eq(a, b) || *a.read() == *b.read()
            }
            (Self::Resource(a), Self::Resource(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for SharedValue {}

/// Shared Object representation for `shared class`.
/// Backed by `Arc` and `parking_lot::RwLock` for multi-threaded, zero-copy, lock-free concurrency.
#[derive(Debug, Clone)]
pub struct VmSharedObject {
    pub class_id: Option<ClassId>,
    pub class_name: String,
    pub properties: Arc<RwLock<IndexMap<String, SharedValue>>>,
    pub readonly_props: Arc<RwLock<HashMap<String, bool>>>,
}

unsafe impl Send for VmSharedObject {}
unsafe impl Sync for VmSharedObject {}

impl VmSharedObject {
    pub fn new(class_name: impl Into<String>) -> Self {
        Self {
            class_id: None,
            class_name: class_name.into(),
            properties: Arc::new(RwLock::new(IndexMap::new())),
            readonly_props: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_class_id(class_id: ClassId, class_name: impl Into<String>) -> Self {
        Self {
            class_id: Some(class_id),
            class_name: class_name.into(),
            properties: Arc::new(RwLock::new(IndexMap::new())),
            readonly_props: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_property(&self, name: &str) -> Option<PhpValue> {
        let guard = self.properties.read();
        guard.get(name).map(|v| PhpValue::from(v.clone()))
    }

    pub fn has_property(&self, name: &str) -> bool {
        self.properties.read().contains_key(name)
    }

    pub fn set_property(&self, name: impl Into<String>, value: PhpValue) -> Result<(), &'static str> {
        let shared_val = SharedValue::try_from(value)?;
        let name_str = name.into();
        self.properties.write().insert(name_str, shared_val);
        Ok(())
    }

    pub fn set_readonly(&self, name: impl Into<String>, is_ro: bool) {
        let name_str = name.into();
        self.readonly_props.write().insert(name_str, is_ro);
    }

    pub fn is_readonly(&self, name: &str) -> bool {
        self.readonly_props.read().get(name).copied().unwrap_or(false)
    }

    /// Attach a native driver, connection pool, or client resource directly to this shared object instance.
    pub fn set_native_resource(&self, name: impl Into<String>, resource: Arc<dyn std::any::Any + Send + Sync>) {
        self.properties.write().insert(name.into(), SharedValue::Resource(resource));
    }

    /// Retrieve an attached native driver, connection pool, or client resource downcast to target type `T`.
    pub fn get_native_resource<T: Send + Sync + 'static>(&self, name: &str) -> Option<Arc<T>> {
        let guard = self.properties.read();
        match guard.get(name) {
            Some(SharedValue::Resource(arc)) => arc.clone().downcast::<T>().ok(),
            _ => None,
        }
    }
}

impl PartialEq for VmSharedObject {
    fn eq(&self, other: &Self) -> bool {
        if self.class_name != other.class_name {
            return false;
        }
        if Arc::ptr_eq(&self.properties, &other.properties) {
            return true;
        }
        let g1 = self.properties.read();
        let g2 = other.properties.read();
        *g1 == *g2
    }
}

impl Eq for VmSharedObject {}

/// Unified core runtime value enum representing all primitive and complex RHP values.
///
/// Implements Technical Point #7 of the RHP Architecture Specification:
/// - Represents primitive scalars directly with zero heap indirection (`Null`, `Bool`, `Int`, `Float`).
/// - Standard integer is `i64` and standard float storage is exact `f32`, exact `f64`, or `Decimal`.
/// - Stored integer widths (`i8`, `i32`, `u64`, `i128`) are inline values, never `Rc`.
/// - Strings utilize Small-String-Optimization (`RhpString`) for zero heap allocations on <= 23 bytes.
/// - Standard arrays and subtyped collections are heap-allocated with reference-counting.
/// - Objects, Closures, and Futures pass by reference handles (`Rc<RefCell<...>>`).
/// - Shared Objects and Concurrency Primitives use `Arc` and `parking_lot::RwLock` (`Send + Sync`).
#[derive(Clone, Default)]
pub enum PhpValue {
    #[default]
    Null,
    Bool(bool),
    /// The only integer. Stored as `i64`.
    Int(i64),
    /// The only float. Stored as `f64`.
    Float(f64),
    /// A single Unicode scalar value (0..=0x10FFFF, excluding surrogates).
    Rune(char),
    String(Rc<str>),
    Bytes(Rc<[u8]>),
    /// Untyped `[]` before the first write. No heap buffer.
    /// A later read treats it as both an empty list and an empty dict.
    /// The first write promotes it to `List`, `Dict`, or `HashMap`.
    EmptyArray,
    Array(Rc<RefCell<Vec<PhpValue>>>),
    List(Rc<RefCell<Vec<PhpValue>>>),
    Dict(Rc<RefCell<IndexMap<String, PhpValue>>>),
    Set(Rc<RefCell<HashSet<PhpValue>>>),
    HashMap(Rc<RefCell<HashMap<PhpValue, PhpValue>>>),
    Object(Rc<RefCell<VmObject>>),
    Closure(Rc<VmClosure>),
    Future(Rc<RefCell<VmFuture>>),
    Resource(Arc<dyn std::any::Any + Send + Sync>),

    // 2. Shared Class / Concurrency Primitives (Arc-backed, Send + Sync)
    SharedObject(Arc<VmSharedObject>),
    SharedArray(Arc<RwLock<Vec<SharedValue>>>),
    SharedAtomic(Arc<AtomicI64>),
    SharedValue(SharedValue),
}

impl From<SharedValue> for PhpValue {
    fn from(val: SharedValue) -> Self {
        match val {
            SharedValue::Null => PhpValue::Null,
            SharedValue::Bool(b) => PhpValue::Bool(b),
            SharedValue::Int(n) => PhpValue::Int(n),
            SharedValue::Float(f) => PhpValue::float(f),
            SharedValue::Rune(c) => PhpValue::Rune(c),
            SharedValue::String(s) => PhpValue::String(s),
            SharedValue::Bytes(b) => PhpValue::bytes(b),
            SharedValue::AtomicInt(a) => PhpValue::SharedAtomic(a),
            SharedValue::AtomicBool(b) => PhpValue::SharedValue(SharedValue::AtomicBool(b)),
            SharedValue::SharedObject(o) => PhpValue::SharedObject(o),
            SharedValue::SharedArray(a) => PhpValue::SharedArray(a),
            SharedValue::SharedDict(d) => PhpValue::SharedValue(SharedValue::SharedDict(d)),
            SharedValue::Resource(r) => PhpValue::Resource(r),
        }
    }
}

impl TryFrom<PhpValue> for SharedValue {
    type Error = &'static str;

    fn try_from(val: PhpValue) -> Result<Self, Self::Error> {
        match val {
            PhpValue::Null => Ok(SharedValue::Null),
            PhpValue::Bool(b) => Ok(SharedValue::Bool(b)),
            PhpValue::Int(n) => Ok(SharedValue::Int(n)),
            PhpValue::Float(f) => Ok(SharedValue::Float(f)),
            PhpValue::Rune(c) => Ok(SharedValue::Rune(c)),
            PhpValue::String(s) => Ok(SharedValue::String(s)),
            PhpValue::SharedObject(o) => Ok(SharedValue::SharedObject(o)),
            PhpValue::SharedAtomic(a) => Ok(SharedValue::AtomicInt(a)),
            PhpValue::SharedArray(a) => Ok(SharedValue::SharedArray(a)),
            PhpValue::SharedValue(v) => Ok(v),
            PhpValue::Resource(r) => Ok(SharedValue::Resource(r)),
            PhpValue::EmptyArray => Ok(SharedValue::SharedArray(Arc::new(RwLock::new(Vec::new())))),
            PhpValue::Array(items) | PhpValue::List(items) => {
                let borrowed = items.borrow();
                let mut shared_items = Vec::with_capacity(borrowed.len());
                for item in borrowed.iter() {
                    shared_items.push(SharedValue::try_from(item.clone())?);
                }
                Ok(SharedValue::SharedArray(Arc::new(RwLock::new(shared_items))))
            }
            PhpValue::Dict(d) => {
                let borrowed = d.borrow();
                let mut map = IndexMap::with_capacity(borrowed.len());
                for (k, v) in borrowed.iter() {
                    map.insert(k.clone(), SharedValue::try_from(v.clone())?);
                }
                Ok(SharedValue::SharedDict(Arc::new(RwLock::new(map))))
            }
            _ => Err("Cannot store non-thread-safe request-scoped reference (such as standard Object, Closure, or Future) inside a shared class"),
        }
    }
}

/// Runtime closure and arrow-function handle referencing a compiled function and captured upvalues.
#[derive(Debug, Clone)]
pub struct VmClosure {
    pub func_id: FunctionId,
    pub name: String,
    pub upvalues: Vec<PhpValue>,
    pub is_static: bool,
    pub category: ClosureCategory,
}

impl VmClosure {
    #[inline]
    pub fn is_pure(&self) -> bool {
        self.category.is_pure()
    }

    #[inline]
    pub fn is_non_capturing(&self) -> bool {
        self.category.is_non_capturing()
    }

    #[inline]
    pub fn is_zero_allocation(&self) -> bool {
        self.category.is_zero_allocation()
    }
}

/// The execution state of an asynchronous Future, matching Rust's `Poll::Pending` / `Poll::Ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FutureState {
    Pending,
    Ready,
}

impl FutureState {
    pub fn as_str(&self) -> &'static str {
        match self {
            FutureState::Pending => "Pending",
            FutureState::Ready => "Ready",
        }
    }
}

/// Deadline guard created by `$future->timeout($ms, $fallback)`.
///
/// The clock starts when `timeout()` is called. `wait()` returns `$fallback`
/// (or calls it, when it is a closure) if the inner future is still pending.
#[derive(Clone)]
pub struct FutureTimeout {
    pub inner: Rc<RefCell<VmFuture>>,
    pub deadline: std::time::Instant,
    pub fallback: PhpValue,
}

/// Runtime asynchronous Future handle supporting non-blocking background task execution,
/// sync resolution, timers, parallel joins, and races.
#[derive(Clone)]
pub struct VmFuture {
    pub is_completed: bool,
    pub value: Option<PhpValue>,
    pub error: Option<String>,
    pub(crate) receiver: Option<Arc<Mutex<std::sync::mpsc::Receiver<Result<FrozenValue, String>>>>>,
    /// Set by `cancel()`. Later `wait()` returns the cancellation error.
    pub cancelled: bool,
    /// Closure for `Future::defer`. The engine runs it on `wait()` or at shutdown.
    pub lazy: Option<VmClosure>,
    /// Present when this handle is a `timeout()` wrapper around `inner`.
    pub timeout: Option<FutureTimeout>,
}

impl VmFuture {
    pub fn resolved(val: PhpValue) -> Self {
        Self {
            is_completed: true,
            value: Some(val),
            error: None,
            receiver: None,
            cancelled: false,
            lazy: None,
            timeout: None,
        }
    }

    pub fn rejected(err: impl Into<String>) -> Self {
        Self {
            is_completed: true,
            value: None,
            error: Some(err.into()),
            receiver: None,
            cancelled: false,
            lazy: None,
            timeout: None,
        }
    }

    pub fn pending(receiver: std::sync::mpsc::Receiver<Result<FrozenValue, String>>) -> Self {
        Self {
            is_completed: false,
            value: None,
            error: None,
            receiver: Some(Arc::new(Mutex::new(receiver))),
            cancelled: false,
            lazy: None,
            timeout: None,
        }
    }

    /// Lazy future. The closure runs on the calling fiber when `wait()` is used.
    pub fn lazy(closure: VmClosure) -> Self {
        Self {
            is_completed: false,
            value: None,
            error: None,
            receiver: None,
            cancelled: false,
            lazy: Some(closure),
            timeout: None,
        }
    }

    /// Wrapper that fails over to `fallback` once `deadline` has passed.
    pub fn with_timeout(inner: Rc<RefCell<VmFuture>>, deadline: std::time::Instant, fallback: PhpValue) -> Self {
        Self {
            is_completed: false,
            value: None,
            error: None,
            receiver: None,
            cancelled: false,
            lazy: None,
            timeout: Some(FutureTimeout {
                inner,
                deadline,
                fallback,
            }),
        }
    }

    /// Drop the pending result. A later `wait()` fails with a cancellation error.
    /// The worker thread is not killed; its result is discarded.
    pub fn cancel(&mut self) {
        if self.cancelled {
            return;
        }
        self.cancelled = true;
        self.receiver = None;
        self.lazy = None;
        self.timeout = None;
        self.is_completed = true;
        self.value = None;
        self.error = Some("Future cancelled".to_string());
    }

    /// Return the current state of this future (`FutureState::Pending` or `FutureState::Ready`).
    pub fn state(&mut self) -> FutureState {
        if self.poll_completion() {
            FutureState::Ready
        } else {
            FutureState::Pending
        }
    }

    /// Check if this future is still in the pending state without blocking.
    pub fn is_pending(&mut self) -> bool {
        !self.poll_completion()
    }

    /// Check if this future has reached the ready/completed state without blocking.
    pub fn is_ready(&mut self) -> bool {
        self.poll_completion()
    }

    /// Check if background execution has completed without blocking.
    pub fn poll_completion(&mut self) -> bool {
        if self.cancelled {
            return true;
        }
        if self.lazy.is_some() {
            return false;
        }
        if let Some(guard) = self.timeout.as_ref().map(|g| (g.deadline, g.inner.clone())) {
            if guard.0 <= std::time::Instant::now() {
                return true;
            }
            return guard.1.borrow_mut().poll_completion();
        }
        if self.is_completed && self.receiver.is_none() {
            return true;
        }
        let rx_mutex = match &self.receiver {
            Some(m) => m.clone(),
            None => return self.is_completed,
        };
        if let Ok(rx) = rx_mutex.lock() {
            match rx.try_recv() {
                Ok(Ok(frozen)) => {
                    self.value = Some(frozen.to_php_value());
                    self.is_completed = true;
                    self.error = None;
                    drop(rx);
                    self.receiver = None;
                    return true;
                }
                Ok(Err(err)) => {
                    self.error = Some(err);
                    self.is_completed = true;
                    self.value = None;
                    drop(rx);
                    self.receiver = None;
                    return true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    return false;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("Background future worker thread disconnected".to_string());
                    self.is_completed = true;
                    self.value = None;
                    drop(rx);
                    self.receiver = None;
                    return true;
                }
            }
        }
        self.is_completed
    }

    /// Block until completed and return the resolved [`PhpValue`] or error message.
    pub fn wait_result(&mut self) -> Result<PhpValue, String> {
        if self.cancelled {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "Future cancelled".to_string()));
        }
        if self.lazy.is_some() {
            return Err("Lazy future must be waited on by the VM".to_string());
        }
        if self.poll_completion() && self.receiver.is_none() && self.timeout.is_none() {
            if let Some(ref err) = self.error {
                return Err(err.clone());
            }
            return Ok(self.value.clone().unwrap_or(PhpValue::Null));
        }

        if let Some(rx_mutex) = self.receiver.take() {
            if let Ok(rx) = rx_mutex.lock() {
                match rx.recv() {
                    Ok(Ok(frozen)) => {
                        let val = frozen.to_php_value();
                        self.value = Some(val.clone());
                        self.is_completed = true;
                        self.error = None;
                        return Ok(val);
                    }
                    Ok(Err(err)) => {
                        self.error = Some(err.clone());
                        self.is_completed = true;
                        self.value = None;
                        return Err(err);
                    }
                    Err(_) => {
                        let err = "Background future worker thread disconnected".to_string();
                        self.error = Some(err.clone());
                        self.is_completed = true;
                        self.value = None;
                        return Err(err);
                    }
                }
            }
        }

        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        Ok(self.value.clone().unwrap_or(PhpValue::Null))
    }

    /// Block until completed with a maximum timeout duration.
    pub fn wait_timeout(&mut self, timeout: std::time::Duration) -> Result<PhpValue, String> {
        if self.cancelled {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "Future cancelled".to_string()));
        }
        if self.lazy.is_some() {
            return Err("Lazy future must be waited on by the VM".to_string());
        }
        if self.poll_completion() && self.receiver.is_none() && self.timeout.is_none() {
            if let Some(ref err) = self.error {
                return Err(err.clone());
            }
            return Ok(self.value.clone().unwrap_or(PhpValue::Null));
        }

        let rx_mutex = match &self.receiver {
            Some(m) => m.clone(),
            None => {
                if let Some(ref err) = self.error {
                    return Err(err.clone());
                }
                return Ok(self.value.clone().unwrap_or(PhpValue::Null));
            }
        };

        if let Ok(rx) = rx_mutex.lock() {
            match rx.recv_timeout(timeout) {
                Ok(Ok(frozen)) => {
                    let val = frozen.to_php_value();
                    drop(rx);
                    self.value = Some(val.clone());
                    self.is_completed = true;
                    self.error = None;
                    self.receiver = None;
                    return Ok(val);
                }
                Ok(Err(err)) => {
                    drop(rx);
                    self.error = Some(err.clone());
                    self.is_completed = true;
                    self.value = None;
                    self.receiver = None;
                    return Err(err);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err(format!("Future timed out after {:?}", timeout));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    drop(rx);
                    let err = "Background future worker thread disconnected".to_string();
                    self.error = Some(err.clone());
                    self.is_completed = true;
                    self.value = None;
                    self.receiver = None;
                    return Err(err);
                }
            }
        }

        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        Ok(self.value.clone().unwrap_or(PhpValue::Null))
    }
}

impl fmt::Debug for VmFuture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VmFuture")
            .field("is_completed", &self.is_completed)
            .field("value", &self.value)
            .field("error", &self.error)
            .field("has_receiver", &self.receiver.is_some())
            .finish()
    }
}

/// Count the number of fractional decimal digits (decimal point precision) in a float's decimal representation.
pub fn count_float_decimal_places(s: &str) -> usize {
    let s = s.trim();
    if let Some(pos) = s.find(['e', 'E']) {
        let (mantissa, exp_str) = s.split_at(pos);
        let exp: i32 = exp_str[1..].parse().unwrap_or(0);
        let mantissa_dec = mantissa
            .find('.')
            .map_or(0, |dot| mantissa.len().saturating_sub(dot + 1));
        if exp < 0 {
            mantissa_dec + (exp.unsigned_abs() as usize)
        } else {
            mantissa_dec.saturating_sub(exp as usize)
        }
    } else if let Some(dot) = s.find('.') {
        s.len().saturating_sub(dot + 1)
    } else {
        0
    }
}

/// Compare two float values by checking the maximum decimal point on both sides
/// and then comparing both values to that maximum decimal point.
pub fn compare_floats_by_max_decimal_point(
    v1: f64,
    s1: &str,
    v2: f64,
    s2: &str,
) -> bool {
    if v1.is_nan() || v2.is_nan() {
        return false;
    }
    if v1.is_infinite() || v2.is_infinite() {
        return v1 == v2;
    }
    // +0.0 and -0.0 are strictly equal
    if v1 == 0.0 && v2 == 0.0 {
        return true;
    }

    let d1 = count_float_decimal_places(s1);
    let d2 = count_float_decimal_places(s2);
    let max_decimals = d1.max(d2).min(100);

    // Format both numbers to `max_decimals` decimal places
    let str1 = format!("{:.prec$}", v1, prec = max_decimals);
    let str2 = format!("{:.prec$}", v2, prec = max_decimals);

    // Normalize "-0.00..." vs "0.00..."
    let clean1 = if str1.starts_with('-') && str1[1..].chars().all(|c| c == '0' || c == '.') {
        &str1[1..]
    } else {
        &str1
    };
    let clean2 = if str2.starts_with('-') && str2[1..].chars().all(|c| c == '0' || c == '.') {
        &str2[1..]
    } else {
        &str2
    };

    clean1 == clean2
}

impl PhpValue {
    // =========================================================================
    // Constructors & Factories
    // =========================================================================

    #[inline]
    pub fn float(val: f64) -> Self {
        let v = if val.is_nan() {
            f64::NAN
        } else if val == 0.0 {
            0.0
        } else {
            val
        };
        Self::Float(v)
    }

    #[inline]
    pub fn same_type_as(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    #[inline]
    pub fn rune(c: char) -> Self {
        Self::Rune(c)
    }

    #[inline]
    pub fn string(s: impl AsRef<str>) -> Self {
        Self::String(Rc::from(s.as_ref()))
    }

    #[inline]
    pub fn bytes(b: impl AsRef<[u8]>) -> Self {
        Self::Bytes(Rc::from(b.as_ref()))
    }

    /// Empty untyped array literal. Promotion happens on the first write.
    #[inline]
    pub fn empty_array() -> Self {
        Self::EmptyArray
    }

    /// First indexed write into an empty array.
    /// `0` becomes a list, a string key becomes a dict, anything else becomes a hash map.
    pub fn promote_empty_index(key: &PhpValue, value: PhpValue) -> Self {
        match key {
            Self::Int(0) => Self::list(vec![value]),
            Self::String(s) => Self::dict([(s.to_string(), value)]),
            other => Self::hash_map([(other.clone(), value)]),
        }
    }

    /// First append (`$items[] = $val`) into an empty array. Always a list.
    #[inline]
    pub fn promote_empty_append(value: PhpValue) -> Self {
        Self::list(vec![value])
    }

    pub fn array(items: Vec<PhpValue>) -> Self {
        let bytes = 32 + items.capacity() * std::mem::size_of::<PhpValue>();
        crate::vm::memory::record_alloc(bytes);
        crate::vm::memory::record_live_object();
        Self::Array(Rc::new(RefCell::new(items)))
    }

    #[inline]
    pub fn list(items: Vec<PhpValue>) -> Self {
        let bytes = 32 + items.capacity() * std::mem::size_of::<PhpValue>();
        crate::vm::memory::record_alloc(bytes);
        crate::vm::memory::record_live_object();
        Self::List(Rc::new(RefCell::new(items)))
    }

    #[inline]
    pub fn dict(entries: impl IntoIterator<Item = (String, PhpValue)>) -> Self {
        let mut map = IndexMap::new();
        for (k, v) in entries {
            map.insert(k, v);
        }
        Self::dict_from_map(map)
    }

    #[inline]
    pub fn dict_from_map(map: IndexMap<String, PhpValue>) -> Self {
        let bytes = 48 + map.capacity() * 48;
        crate::vm::memory::record_alloc(bytes);
        crate::vm::memory::record_live_object();
        Self::Dict(Rc::new(RefCell::new(map)))
    }

    #[inline]
    pub fn set(items: impl IntoIterator<Item = PhpValue>) -> Self {
        let set: HashSet<PhpValue> = items.into_iter().collect();
        Self::set_from_hash_set(set)
    }

    #[inline]
    pub fn set_from_hash_set(set: HashSet<PhpValue>) -> Self {
        let bytes = 48 + set.capacity() * 32;
        crate::vm::memory::record_alloc(bytes);
        crate::vm::memory::record_live_object();
        Self::Set(Rc::new(RefCell::new(set)))
    }

    #[inline]
    pub fn hash_map(entries: impl IntoIterator<Item = (PhpValue, PhpValue)>) -> Self {
        let mut map = HashMap::new();
        for (k, v) in entries {
            map.insert(k, v);
        }
        Self::hash_map_from_map(map)
    }

    #[inline]
    pub fn hash_map_from_map(map: HashMap<PhpValue, PhpValue>) -> Self {
        let bytes = 48 + map.capacity() * 48;
        crate::vm::memory::record_alloc(bytes);
        crate::vm::memory::record_live_object();
        Self::HashMap(Rc::new(RefCell::new(map)))
    }

    #[inline]
    pub fn object(obj: VmObject) -> Self {
        let bytes = 64 + obj.properties.len() * 32;
        crate::vm::memory::record_alloc(bytes);
        crate::vm::memory::record_live_object();
        Self::Object(Rc::new(RefCell::new(obj)))
    }

    #[inline]
    pub fn weak_ref(target: &Rc<RefCell<VmObject>>) -> Self {
        Self::Object(Rc::new(RefCell::new(VmObject::weak_reference(target))))
    }

    #[inline]
    pub fn closure(
        func_id: FunctionId,
        name: impl Into<String>,
        upvalues: Vec<PhpValue>,
        is_static: bool,
        category: ClosureCategory,
    ) -> Self {
        Self::Closure(Rc::new(VmClosure {
            func_id,
            name: name.into(),
            upvalues,
            is_static,
            category,
        }))
    }

    #[inline]
    pub fn shared_object(obj: VmSharedObject) -> Self {
        Self::SharedObject(Arc::new(obj))
    }

    #[inline]
    pub fn shared_atomic(val: i64) -> Self {
        Self::SharedAtomic(Arc::new(AtomicI64::new(val)))
    }

    #[inline]
    pub fn shared_array(items: Vec<SharedValue>) -> Self {
        Self::SharedArray(Arc::new(RwLock::new(items)))
    }

    // =========================================================================
    // Type Inspection & Conversions
    // =========================================================================

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Rune(_) => "rune",
            Self::String(_) => "string",
            Self::Bytes(_) => "bytes",
            Self::EmptyArray => "array",
            Self::Array(_) => "array",
            Self::List(_) => "Arr::list",
            Self::Dict(_) => "Arr::dict",
            Self::Set(_) => "Arr::set",
            Self::HashMap(_) => "Arr::hashMap",
            Self::Object(_) | Self::SharedObject(_) => "object",
            Self::Closure(_) => "Closure",
            Self::Future(_) => "Future",
            Self::Resource(_) => "resource",
            Self::SharedAtomic(_) => "int",
            Self::SharedArray(_) => "array",
            Self::SharedValue(sv) => match sv {
                SharedValue::Null => "null",
                SharedValue::Bool(_) => "bool",
                SharedValue::Int(_) | SharedValue::AtomicInt(_) => "int",
                SharedValue::Float(_) => "float",
                SharedValue::Rune(_) => "rune",
                SharedValue::String(_) => "string",
                SharedValue::Bytes(_) => "bytes",
                SharedValue::AtomicBool(_) => "bool",
                SharedValue::SharedObject(_) => "object",
                SharedValue::SharedArray(_) => "array",
                SharedValue::SharedDict(_) => "Arr::dict",
                SharedValue::Resource(_) => "resource",
            },
        }
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    pub fn is_utf8(&self) -> bool {
        matches!(self, Self::String(_))
    }

    pub fn is_bytes(&self) -> bool {
        matches!(self, Self::Bytes(_))
    }

    pub fn is_rune(&self) -> bool {
        matches!(self, Self::Rune(_))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Int(n) => *n != 0,
            Self::Float(fl) => *fl != 0.0 && !fl.is_nan(),
            Self::Rune(c) => *c != '\0',
            Self::String(s) => !s.is_empty() && &**s != "0",
            Self::Bytes(b) => !b.is_empty() && &**b != b"0",
            Self::EmptyArray => false,
            Self::Array(arr) => !arr.borrow().is_empty(),
            Self::List(list) => !list.borrow().is_empty(),
            Self::Dict(dict) => !dict.borrow().is_empty(),
            Self::Set(set) => !set.borrow().is_empty(),
            Self::HashMap(map) => !map.borrow().is_empty(),
            Self::Object(_) | Self::SharedObject(_) | Self::SharedAtomic(_) | Self::Closure(_) | Self::Future(_) | Self::Resource(_) => true,
            Self::SharedArray(arr) => !arr.read().is_empty(),
            Self::SharedValue(sv) => sv.is_truthy(),
        }
    }

    /// Extract boolean value natively if this value is a boolean scalar.
    #[inline]
    pub fn to_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Extract char value natively if this value is a rune scalar.
    #[inline]
    pub fn to_rune(&self) -> Option<char> {
        match self {
            Self::Rune(c) => Some(*c),
            _ => None,
        }
    }

    /// Convert `PhpValue` to an `Option<&PhpValue>`, mapping `PhpValue::Null` directly to `None`.
    #[inline]
    pub fn as_none(&self) -> Option<&Self> {
        if matches!(self, Self::Null) {
            None
        } else {
            Some(self)
        }
    }

    /// Convert `PhpValue` into `Option<PhpValue>`, mapping `PhpValue::Null` directly to `None`.
    #[inline]
    pub fn into_option(self) -> Option<Self> {
        if matches!(self, Self::Null) {
            None
        } else {
            Some(self)
        }
    }

    /// Check if this value is a zero-cost primitive scalar (`int`, `float`, `bool`, `rune`, `null`)
    /// that is stored unboxed without any `Rc<>` reference counting overhead.
    #[inline]
    pub fn is_scalar_primitive(&self) -> bool {
        matches!(
            self,
            Self::Null | Self::Bool(_) | Self::Int(_) | Self::Float(_) | Self::Rune(_)
        )
    }

    /// Two's-complement bits of an `int` (`i64`).
    pub fn signed_bits(&self) -> Option<i128> {
        match self {
            Self::Int(n) => Some(*n as i128),
            Self::SharedAtomic(a) => Some(a.load(AtomicOrdering::SeqCst) as i128),
            Self::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    pub fn is_integer_value(&self) -> bool {
        matches!(
            self,
            Self::Int(_) | Self::SharedAtomic(_)
        )
    }

    /// Check if this value requires an `Rc<>` reference counted heap handle.
    /// Primitives (`int`, `float`, `bool`, `null`) return false: zero `Rc<>` overhead.
    #[inline]
    pub fn requires_rc(&self) -> bool {
        !self.is_scalar_primitive()
    }

    pub fn to_i32(&self) -> Option<i32> {
        if let Some(bits) = self.signed_bits() {
            return i32::try_from(bits).ok();
        }
        match self {
            Self::Float(f) => Some(*f as i32),
            Self::String(s) => s.trim().parse::<i32>().ok(),
            _ => None,
        }
    }

    pub fn to_i64(&self) -> Option<i64> {
        if let Some(bits) = self.signed_bits() {
            return i64::try_from(bits).ok();
        }
        match self {
            Self::Float(f) => Some(*f as i64),
            Self::String(s) => s.trim().parse::<i64>().ok(),
            Self::Bytes(b) => std::str::from_utf8(b).ok().and_then(|s| s.trim().parse::<i64>().ok()),
            _ => None,
        }
    }

    pub fn to_f32(&self) -> Option<f32> {
        match self {
            Self::Float(f) => Some(*f as f32),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            other => other.to_i64().map(|n| n as f32).or_else(|| match other {
                Self::String(s) => s.trim().parse::<f32>().ok(),
                Self::Bytes(b) => std::str::from_utf8(b).ok().and_then(|s| s.trim().parse::<f32>().ok()),
                _ => None,
            }),
        }
    }

    pub fn to_f64(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            other => other.to_i64().map(|n| n as f64).or_else(|| match other {
                Self::String(s) => s.trim().parse::<f64>().ok(),
                Self::Bytes(b) => std::str::from_utf8(b).ok().and_then(|s| s.trim().parse::<f64>().ok()),
                _ => None,
            }),
        }
    }

    pub fn to_php_string(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Bool(b) => (if *b { "1" } else { "" }).to_string(),
            Self::Int(n) => n.to_string(),
            Self::Float(fl) => fl.to_string(),
            Self::Rune(c) => c.to_string(),
            Self::String(s) => s.to_string(),
            Self::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            Self::EmptyArray => "Array".to_string(),
            Self::Array(_) => "Array".to_string(),
            Self::List(_) => "Arr::list".to_string(),
            Self::Dict(_) => "Arr::dict".to_string(),
            Self::Set(_) => "Arr::set".to_string(),
            Self::HashMap(_) => "Arr::hashMap".to_string(),
            Self::Object(obj) => {
                let b = obj.borrow();
                if let Some(ref t) = b.throwable {
                    return t.to_string_representation(&b.class_name);
                }
                if b.class_name == "Time" || b.class_name == "Duration" {
                    if let Some(formatted) = b.get_property("formatted") {
                        return formatted.to_php_string();
                    }
                }
                if b.class_name == "Response" {
                    if let Some(content) = b.get_property("content") {
                        return content.to_php_string();
                    }
                }
                format!("class@{}", b.class_name)
            }
            Self::SharedObject(obj) => format!("class@{}", obj.class_name),
            Self::SharedAtomic(a) => a.load(AtomicOrdering::SeqCst).to_string(),
            Self::SharedArray(_) => "Array".to_string(),
            Self::SharedValue(sv) => match sv {
                SharedValue::Null => String::new(),
                SharedValue::Bool(b) => (if *b { "1" } else { "" }).to_string(),
                SharedValue::Int(n) => n.to_string(),
                SharedValue::Float(f) => f.to_string(),
                SharedValue::Rune(c) => c.to_string(),
                SharedValue::String(s) => s.to_string(),
                SharedValue::Bytes(b) => String::from_utf8_lossy(b).to_string(),
                SharedValue::AtomicInt(a) => a.load(AtomicOrdering::SeqCst).to_string(),
                SharedValue::AtomicBool(b) => (if b.load(AtomicOrdering::SeqCst) { "1" } else { "" }).to_string(),
                SharedValue::SharedObject(o) => format!("class@{}", o.class_name),
                SharedValue::SharedArray(_) => "Array".to_string(),
                SharedValue::SharedDict(_) => "Arr::dict".to_string(),
                SharedValue::Resource(_) => "Resource".to_string(),
            },
            Self::Closure(c) => format!("Closure({})", c.name),
            Self::Future(_) => "Future".to_string(),
            Self::Resource(_) => "Resource".to_string(),
        }
    }

    /// Recursively formats this value matching standard PHP `print_r` semantics.
    pub fn to_print_r(&self) -> String {
        self.format_print_r(0)
    }

    fn format_print_r(&self, indent: usize) -> String {
        if indent > 32 {
            return "*RECURSION*".to_string();
        }
        let pad = "    ".repeat(indent);
        let inner_pad = "    ".repeat(indent + 1);
        match self {
            Self::Null => String::new(),
            Self::Bool(b) => (if *b { "1" } else { "" }).to_string(),
            Self::Int(n) => n.to_string(),
            Self::Float(fl) => fl.to_string(),
            Self::Rune(c) => c.to_string(),
            Self::String(s) => s.to_string(),
            Self::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            Self::EmptyArray => "Array\n(\n)".to_string(),
            Self::Array(arr) => {
                let borrowed = arr.borrow();
                if borrowed.is_empty() {
                    return "Array\n(\n)".to_string();
                }
                let mut out = String::from("Array\n") + &pad + "(\n";
                for (idx, val) in borrowed.iter().enumerate() {
                    out.push_str(&format!("{}[{}] => {}\n", inner_pad, idx, val.format_print_r(indent + 1)));
                }
                out.push_str(&pad);
                out.push(')');
                out
            }
            Self::Dict(dict) => {
                let borrowed = dict.borrow();
                if borrowed.is_empty() {
                    return "Array\n(\n)".to_string();
                }
                let mut out = String::from("Array\n") + &pad + "(\n";
                for (k, val) in borrowed.iter() {
                    out.push_str(&format!("{}[{}] => {}\n", inner_pad, k, val.format_print_r(indent + 1)));
                }
                out.push_str(&pad);
                out.push(')');
                out
            }
            Self::List(list) => {
                let borrowed = list.borrow();
                if borrowed.is_empty() {
                    return "Arr::list\n(\n)".to_string();
                }
                let mut out = String::from("Arr::list\n") + &pad + "(\n";
                for (idx, val) in borrowed.iter().enumerate() {
                    out.push_str(&format!("{}[{}] => {}\n", inner_pad, idx, val.format_print_r(indent + 1)));
                }
                out.push_str(&pad);
                out.push(')');
                out
            }
            Self::HashMap(hm) => {
                let borrowed = hm.borrow();
                if borrowed.is_empty() {
                    return "Arr::hashMap\n(\n)".to_string();
                }
                let mut out = String::from("Arr::hashMap\n") + &pad + "(\n";
                for (k, val) in borrowed.iter() {
                    out.push_str(&format!("{}[{}] => {}\n", inner_pad, k.to_php_string(), val.format_print_r(indent + 1)));
                }
                out.push_str(&pad);
                out.push(')');
                out
            }
            Self::Set(set) => {
                let borrowed = set.borrow();
                if borrowed.is_empty() {
                    return "Arr::set\n(\n)".to_string();
                }
                let mut out = String::from("Arr::set\n") + &pad + "(\n";
                for val in borrowed.iter() {
                    out.push_str(&format!("{}{}\n", inner_pad, val.to_php_string()));
                }
                out.push_str(&pad);
                out.push(')');
                out
            }
            Self::Object(obj) => {
                let borrowed = obj.borrow();
                let mut out = format!("{} Object\n{}(\n", borrowed.class_name, pad);
                for (k, val) in &borrowed.properties {
                    out.push_str(&format!("{}[{}] => {}\n", inner_pad, k, val.format_print_r(indent + 1)));
                }
                out.push_str(&pad);
                out.push(')');
                out
            }
            Self::SharedObject(obj) => {
                let guard = obj.properties.read();
                let mut out = format!("{} Object\n{}(\n", obj.class_name, pad);
                for (k, val) in guard.iter() {
                    let php_val = PhpValue::from(val.clone());
                    out.push_str(&format!("{}[{}] => {}\n", inner_pad, k, php_val.format_print_r(indent + 1)));
                }
                out.push_str(&pad);
                out.push(')');
                out
            }
            Self::SharedAtomic(a) => a.load(AtomicOrdering::SeqCst).to_string(),
            Self::SharedArray(arr) => {
                let guard = arr.read();
                let mut out = String::from("Array\n") + &pad + "(\n";
                for (idx, val) in guard.iter().enumerate() {
                    let php_val = PhpValue::from(val.clone());
                    out.push_str(&format!("{}[{}] => {}\n", inner_pad, idx, php_val.format_print_r(indent + 1)));
                }
                out.push_str(&pad);
                out.push(')');
                out
            }
            Self::SharedValue(sv) => match sv {
                SharedValue::Null => String::new(),
                SharedValue::Bool(b) => (if *b { "1" } else { "" }).to_string(),
                SharedValue::Int(n) => n.to_string(),
                SharedValue::Float(f) => f.to_string(),
                SharedValue::String(s) => s.to_string(),
                SharedValue::Bytes(b) => String::from_utf8_lossy(b).to_string(),
                SharedValue::Rune(c) => c.to_string(),
                SharedValue::AtomicInt(a) => a.load(AtomicOrdering::SeqCst).to_string(),
                SharedValue::AtomicBool(b) => (if b.load(AtomicOrdering::SeqCst) { "1" } else { "" }).to_string(),
                SharedValue::SharedObject(o) => PhpValue::SharedObject(o.clone()).format_print_r(indent),
                SharedValue::SharedArray(a) => PhpValue::SharedArray(a.clone()).format_print_r(indent),
                SharedValue::SharedDict(d) => {
                    let guard = d.read();
                    let mut out = String::from("Arr::dict\n") + &pad + "(\n";
                    for (k, val) in guard.iter() {
                        let php_val = PhpValue::from(val.clone());
                        out.push_str(&format!("{}[{}] => {}\n", inner_pad, k, php_val.format_print_r(indent + 1)));
                    }
                    out.push_str(&pad);
                    out.push(')');
                    out
                }
                SharedValue::Resource(_) => "Resource".to_string(),
            },
            Self::Closure(c) => format!("Closure({})", c.name),
            Self::Future(fut) => format!("Future(completed={})", fut.borrow().is_completed),
            Self::Resource(_) => "Resource".to_string(),
        }
    }

    /// Access raw byte slice if this value is bytes or string.
    #[inline]
    pub fn raw_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(b) => Some(b.as_ref()),
            Self::String(s) => Some(s.as_bytes()),
            _ => None,
        }
    }

    /// Lossy UTF-8 conversion for string/bytes or general php values.
    #[inline]
    pub fn to_str_lossy(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::String(s) => std::borrow::Cow::Borrowed(s.as_ref()),
            Self::Bytes(b) => String::from_utf8_lossy(b.as_ref()),
            _ => std::borrow::Cow::Owned(self.to_php_string()),
        }
    }

    // =========================================================================
    // Strict & Loose Equality
    // =========================================================================
    // Strict Equality (StrictEq)
    // =========================================================================

    /// Strict equality comparison (`==` and `!=`):
    /// Returns `true` if and only if types and values strictly match without type coercion.
    /// When comparing floats, checks the maximum decimal point on both sides and compares
    /// both sides to that maximum decimal point for accurate representation.
    pub fn strict_eq(&self, other: &Self) -> bool {
        self.strict_eq_depth(other, 0)
    }

    pub fn strict_eq_depth(&self, other: &Self, depth: usize) -> bool {
        if depth > 256 {
            return false;
        }
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(x), Self::Bool(y)) => x == y,
            (Self::Int(x), Self::Int(y)) => x == y,
            (Self::Float(x), Self::Float(y)) => {
                if x.is_nan() && y.is_nan() {
                    true
                } else if *x == 0.0 && *y == 0.0 {
                    true
                } else {
                    compare_floats_by_max_decimal_point(*x, &x.to_string(), *y, &y.to_string())
                }
            }
            (Self::String(x), Self::String(y)) => x == y,
            (Self::Bytes(x), Self::Bytes(y)) => x == y,
            (Self::Rune(x), Self::Rune(y)) => x == y,
            (Self::EmptyArray, Self::EmptyArray) => true,
            (Self::Array(x) | Self::List(x), Self::Array(y) | Self::List(y)) => {
                if Rc::ptr_eq(x, y) {
                    return true;
                }
                let a = x.borrow().clone();
                let b = y.borrow().clone();
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(v1, v2)| v1.strict_eq_depth(v2, depth + 1))
            }
            (Self::Dict(x), Self::Dict(y)) => {
                if Rc::ptr_eq(x, y) {
                    return true;
                }
                let a = x.borrow().clone();
                let b = y.borrow().clone();
                a.len() == b.len()
                    && a.iter()
                        .all(|(k, v1)| b.get(k).map_or(false, |v2| v1.strict_eq_depth(v2, depth + 1)))
            }
            (Self::Set(x), Self::Set(y)) => {
                if Rc::ptr_eq(x, y) {
                    return true;
                }
                let a = x.borrow().clone();
                let b = y.borrow().clone();
                a == b
            }
            (Self::HashMap(x), Self::HashMap(y)) => {
                if Rc::ptr_eq(x, y) {
                    return true;
                }
                let a = x.borrow().clone();
                let b = y.borrow().clone();
                a == b
            }
            (Self::Object(x), Self::Object(y)) => Rc::ptr_eq(x, y),
            (Self::SharedObject(x), Self::SharedObject(y)) => Arc::ptr_eq(x, y),
            (Self::SharedAtomic(x), Self::SharedAtomic(y)) => {
                Arc::ptr_eq(x, y) || x.load(AtomicOrdering::SeqCst) == y.load(AtomicOrdering::SeqCst)
            }
            (Self::SharedArray(x), Self::SharedArray(y)) => Arc::ptr_eq(x, y),
            (Self::SharedValue(x), Self::SharedValue(y)) => x == y,
            (Self::Closure(x), Self::Closure(y)) => Rc::ptr_eq(x, y),
            (Self::Future(x), Self::Future(y)) => Rc::ptr_eq(x, y),
            (Self::Resource(x), Self::Resource(y)) => Arc::ptr_eq(x, y),
            _ => false,
        }
    }


    // =========================================================================
    // Type Contract Assertion against RhpType
    // =========================================================================

    pub fn matches_type_contract(&self, expected: &RhpType, hierarchy: Option<&TypeHierarchy>) -> bool {
        match expected {
            RhpType::Mixed | RhpType::Dynamic => true,
            RhpType::Void | RhpType::Never => false,
            RhpType::Null => matches!(self, Self::Null),
            RhpType::Bool => matches!(self, Self::Bool(_)),
            RhpType::Int => self.to_i64().is_some() && self.is_integer_value(),
            RhpType::Float => matches!(self, Self::Float(_) | Self::Int(_)),
            RhpType::String => matches!(self, Self::String(_)),
            RhpType::Bytes => matches!(self, Self::Bytes(_)),
            RhpType::Rune => matches!(self, Self::Rune(_)),
            RhpType::Array(inner) => {
                if **inner == RhpType::Mixed || **inner == RhpType::Dynamic {
                    matches!(
                        self,
                        Self::EmptyArray
                            | Self::Array(_)
                            | Self::List(_)
                            | Self::Dict(_)
                            | Self::Set(_)
                            | Self::HashMap(_)
                    )
                } else {
                    match self {
                        Self::EmptyArray => true,
                        Self::Array(a) | Self::List(a) => {
                            a.borrow().iter().all(|v| v.matches_type_contract(inner, hierarchy))
                        }
                        Self::Set(s) => {
                            s.borrow().iter().all(|v| v.matches_type_contract(inner, hierarchy))
                        }
                        Self::Dict(d) => {
                            d.borrow().values().all(|v| v.matches_type_contract(inner, hierarchy))
                        }
                        Self::HashMap(m) => {
                            m.borrow().values().all(|v| v.matches_type_contract(inner, hierarchy))
                        }
                        _ => false,
                    }
                }
            }
            RhpType::Collection(col) => match (col, self) {
                (CollectionSubtype::List(_), Self::EmptyArray)
                | (CollectionSubtype::Dict(_), Self::EmptyArray) => true,
                (CollectionSubtype::List(inner), Self::List(arr) | Self::Array(arr)) => {
                    if **inner == RhpType::Mixed || **inner == RhpType::Dynamic {
                        true
                    } else {
                        arr.borrow().iter().all(|v| v.matches_type_contract(inner, hierarchy))
                    }
                }
                (CollectionSubtype::Dict(inner), Self::Dict(dict)) => {
                    if **inner == RhpType::Mixed || **inner == RhpType::Dynamic {
                        true
                    } else {
                        dict.borrow().values().all(|v| v.matches_type_contract(inner, hierarchy))
                    }
                }
                (CollectionSubtype::Set(inner), Self::Set(set)) => {
                    if **inner == RhpType::Mixed || **inner == RhpType::Dynamic {
                        true
                    } else {
                        set.borrow().iter().all(|v| v.matches_type_contract(inner, hierarchy))
                    }
                }
                (CollectionSubtype::HashMap(k_type, v_type), Self::HashMap(map)) => {
                    let k_ok = **k_type == RhpType::Mixed || **k_type == RhpType::Dynamic;
                    let v_ok = **v_type == RhpType::Mixed || **v_type == RhpType::Dynamic;
                    if k_ok && v_ok {
                        true
                    } else {
                        map.borrow().iter().all(|(k, v)| {
                            (k_ok || k.matches_type_contract(k_type, hierarchy))
                                && (v_ok || v.matches_type_contract(v_type, hierarchy))
                        })
                    }
                }
                _ => false,
            },
            RhpType::Iterable(_) => match self {
                Self::EmptyArray
                | Self::Array(_)
                | Self::List(_)
                | Self::Dict(_)
                | Self::Set(_)
                | Self::HashMap(_) => true,
                Self::Object(obj) => {
                    let obj_cls = &obj.borrow().class_name;
                    if let Some(h) = hierarchy {
                        h.is_subtype_of(obj_cls, "Traversable")
                            || h.is_subtype_of(obj_cls, "IteratorAggregate")
                            || h.is_subtype_of(obj_cls, "Iterator")
                    } else {
                        let clean = obj_cls.strip_prefix('\\').unwrap_or(obj_cls);
                        clean == "ArrayIterator"
                            || clean == "LengthAwarePaginator"
                            || clean == "Paginator"
                            || clean == "Collection"
                            || clean.ends_with("Collection")
                            || clean.ends_with("Paginator")
                    }
                }
                _ => false,
            },
            RhpType::Callable { .. } => match self {
                Self::Closure(_) => true,
                Self::Object(obj) => {
                    if let Some(h) = hierarchy {
                        h.lookup_method(&obj.borrow().class_name, "__invoke").is_some()
                    } else {
                        false
                    }
                }
                _ => false,
            },
            RhpType::Future(_) => matches!(self, Self::Future(_)),
            RhpType::Object => matches!(self, Self::Object(_) | Self::SharedObject(_) | Self::Closure(_) | Self::Future(_)),
            RhpType::Class(class_name) => match self {
                Self::Object(obj) => {
                    let obj_cls = &obj.borrow().class_name;
                    let obj_clean = obj_cls.strip_prefix('\\').unwrap_or(obj_cls);
                    let cls_clean = class_name.strip_prefix('\\').unwrap_or(class_name);
                    if obj_clean == cls_clean {
                        return true;
                    }
                    if let Some(h) = hierarchy {
                        h.is_subtype_of(obj_clean, cls_clean)
                    } else {
                        if cls_clean == "Traversable" {
                            obj_clean == "ArrayIterator"
                                || obj_clean == "LengthAwarePaginator"
                                || obj_clean == "Paginator"
                                || obj_clean == "Collection"
                                || obj_clean.ends_with("Collection")
                                || obj_clean.ends_with("Paginator")
                        } else if cls_clean == "Iterator" {
                            obj_clean == "ArrayIterator"
                        } else if cls_clean == "IteratorAggregate" {
                            obj_clean == "LengthAwarePaginator"
                                || obj_clean == "Paginator"
                                || obj_clean == "Collection"
                        } else {
                            false
                        }
                    }
                }
                Self::SharedObject(obj) => {
                    let obj_cls = &obj.class_name;
                    let obj_clean = obj_cls.strip_prefix('\\').unwrap_or(obj_cls);
                    let cls_clean = class_name.strip_prefix('\\').unwrap_or(class_name);
                    if obj_clean == cls_clean {
                        return true;
                    }
                    if let Some(h) = hierarchy {
                        h.is_subtype_of(obj_clean, cls_clean)
                    } else {
                        false
                    }
                }
                Self::Future(_) if class_name == "Future" => true,
                _ => false,
            },
            RhpType::Nullable(inner) => matches!(self, Self::Null) || self.matches_type_contract(inner, hierarchy),
            RhpType::Union(variants) => variants.iter().any(|v| self.matches_type_contract(v, hierarchy)),
            RhpType::Intersection(members) => members.iter().all(|m| self.matches_type_contract(m, hierarchy)),
            RhpType::Tuple(items) => match self {
                Self::Array(arr) | Self::List(arr) => {
                    let b = arr.borrow();
                    b.len() == items.len()
                        && b.iter()
                            .zip(items.iter())
                            .all(|(val, ty)| val.matches_type_contract(ty, hierarchy))
                }
                _ => false,
            },
        }
    }

    // =========================================================================
    // Copy-On-Write (COW) Semantics & Detachment Primitives
    // =========================================================================

    /// Returns the strong reference count of the underlying buffer or handle.
    /// Primitives, scalars, and inline SSO strings always return 1.
    pub fn ref_count(&self) -> usize {
        match self {
            Self::String(s) => Rc::strong_count(s),
            Self::Bytes(b) => Rc::strong_count(b),
            Self::Array(rc) | Self::List(rc) => Rc::strong_count(rc),
            Self::Dict(rc) => Rc::strong_count(rc),
            Self::Set(rc) => Rc::strong_count(rc),
            Self::HashMap(rc) => Rc::strong_count(rc),
            Self::Object(rc) => Rc::strong_count(rc),
            Self::SharedObject(arc) => Arc::strong_count(arc),
            Self::SharedAtomic(arc) => Arc::strong_count(arc),
            Self::SharedArray(arc) => Arc::strong_count(arc),
            Self::Closure(rc) => Rc::strong_count(rc),
            Self::Future(rc) => Rc::strong_count(rc),
            _ => 1,
        }
    }

    /// Returns `true` if this value references a shared heap structure (ref_count > 1).
    pub fn is_shared(&self) -> bool {
        self.ref_count() > 1
    }

    /// Detaches the sequential array buffer if shared (`strong_count > 1`) (Copy-On-Write).
    /// Returns `true` if this value is an Array or List, `false` otherwise.
    pub fn ensure_array_mut(&mut self) -> bool {
        match self {
            Self::Array(rc) => {
                if Rc::strong_count(rc) > 1 {
                    let cloned = rc.borrow().clone();
                    *rc = Rc::new(RefCell::new(cloned));
                    crate::vm::memory::record_cow_detachment();
                }
                true
            }
            Self::List(rc) => {
                if Rc::strong_count(rc) > 1 {
                    let cloned = rc.borrow().clone();
                    *rc = Rc::new(RefCell::new(cloned));
                    crate::vm::memory::record_cow_detachment();
                }
                true
            }
            _ => false,
        }
    }

    /// Detaches the sequential list buffer if shared (`strong_count > 1`) (Copy-On-Write).
    /// Returns `true` if this value is a List, `false` otherwise.
    pub fn ensure_list_mut(&mut self) -> bool {
        if let Self::List(rc) = self {
            if Rc::strong_count(rc) > 1 {
                let cloned = rc.borrow().clone();
                *rc = Rc::new(RefCell::new(cloned));
                crate::vm::memory::record_cow_detachment();
            }
            true
        } else {
            false
        }
    }

    /// Returns `true` if this value is an array or collection variant (`Array`, `List`, `Dict`, `Set`, or `HashMap`).
    #[inline]
    pub fn is_array(&self) -> bool {
        matches!(
            self,
            Self::EmptyArray | Self::Array(_) | Self::List(_) | Self::Dict(_) | Self::Set(_) | Self::HashMap(_)
        )
    }

    /// Returns `true` if this value is an explicit subtyped sequential list (`Arr::list`).
    #[inline]
    pub fn is_list(&self) -> bool {
        matches!(self, Self::List(_))
    }

    /// Borrows the underlying Rust `Vec<PhpValue>` sequential list structure.
    #[inline]
    pub fn as_list(&self) -> Option<std::cell::Ref<'_, Vec<PhpValue>>> {
        match self {
            Self::List(rc) | Self::Array(rc) => Some(rc.borrow()),
            _ => None,
        }
    }

    /// Mutably borrows the underlying Rust `Vec<PhpValue>` sequential list structure.
    #[inline]
    pub fn as_list_mut(&self) -> Option<std::cell::RefMut<'_, Vec<PhpValue>>> {
        match self {
            Self::List(rc) | Self::Array(rc) => Some(rc.borrow_mut()),
            _ => None,
        }
    }

    /// Returns the length of the sequential list if this value is a List or Array.
    #[inline]
    pub fn list_len(&self) -> Option<usize> {
        match self {
            Self::List(rc) | Self::Array(rc) => Some(rc.borrow().len()),
            _ => None,
        }
    }

    /// Accesses an element by 0-based index from the contiguous Rust `Vec<PhpValue>` structure.
    #[inline]
    pub fn list_get(&self, index: usize) -> Option<PhpValue> {
        match self {
            Self::List(rc) | Self::Array(rc) => rc.borrow().get(index).cloned(),
            _ => None,
        }
    }

    /// Detaches the dictionary buffer if shared (`strong_count > 1`) (Copy-On-Write).
    /// Returns `true` if this value is a Dict, `false` otherwise.
    pub fn ensure_dict_mut(&mut self) -> bool {
        if let Self::Dict(rc) = self {
            if Rc::strong_count(rc) > 1 {
                let cloned = rc.borrow().clone();
                *rc = Rc::new(RefCell::new(cloned));
                crate::vm::memory::record_cow_detachment();
            }
            true
        } else {
            false
        }
    }

    /// Lists, dicts, and untyped arrays cannot be set elements or hashmap keys.
    pub fn is_unhashable_collection(&self) -> bool {
        matches!(
            self,
            Self::EmptyArray | Self::Array(_) | Self::List(_) | Self::Dict(_)
        )
    }

    /// Returns `true` if this value is an explicit subtyped dictionary (`Arr::dict`).
    #[inline]
    pub fn is_dict(&self) -> bool {
        matches!(self, Self::Dict(_))
    }

    /// Borrows the underlying Rust `IndexMap<String, PhpValue>` structure.
    #[inline]
    pub fn as_dict(&self) -> Option<std::cell::Ref<'_, IndexMap<String, PhpValue>>> {
        match self {
            Self::Dict(rc) => Some(rc.borrow()),
            _ => None,
        }
    }

    /// Mutably borrows the underlying Rust `IndexMap<String, PhpValue>` structure.
    #[inline]
    pub fn as_dict_mut(&self) -> Option<std::cell::RefMut<'_, IndexMap<String, PhpValue>>> {
        match self {
            Self::Dict(rc) => Some(rc.borrow_mut()),
            _ => None,
        }
    }

    /// Returns the length of the dictionary if this value is a Dict.
    #[inline]
    pub fn dict_len(&self) -> Option<usize> {
        match self {
            Self::Dict(rc) => Some(rc.borrow().len()),
            _ => None,
        }
    }

    /// Accesses an entry by string key from the `IndexMap<String, PhpValue>`.
    #[inline]
    pub fn dict_get(&self, key: &str) -> Option<PhpValue> {
        match self {
            Self::Dict(rc) => rc.borrow().get(key).cloned(),
            _ => None,
        }
    }

    /// Accesses an entry by 0-based insertion index from the `IndexMap<String, PhpValue>`.
    #[inline]
    pub fn dict_get_index(&self, index: usize) -> Option<(String, PhpValue)> {
        match self {
            Self::Dict(rc) => rc.borrow().get_index(index).map(|(k, v)| (k.clone(), v.clone())),
            _ => None,
        }
    }

    /// Checks if a string key exists in the `IndexMap<String, PhpValue>`.
    #[inline]
    pub fn dict_contains_key(&self, key: &str) -> bool {
        match self {
            Self::Dict(rc) => rc.borrow().contains_key(key),
            _ => false,
        }
    }

    /// Detaches the unique set buffer if shared (`strong_count > 1`) (Copy-On-Write).
    /// Returns `true` if this value is a Set, `false` otherwise.
    pub fn ensure_set_mut(&mut self) -> bool {
        if let Self::Set(rc) = self {
            if Rc::strong_count(rc) > 1 {
                let cloned = rc.borrow().clone();
                *rc = Rc::new(RefCell::new(cloned));
                crate::vm::memory::record_cow_detachment();
            }
            true
        } else {
            false
        }
    }

    /// Returns `true` if this value is an explicit subtyped set (`Arr::set`).
    #[inline]
    pub fn is_set(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    /// Borrows the underlying Rust `HashSet<PhpValue>` structure.
    #[inline]
    pub fn as_set(&self) -> Option<std::cell::Ref<'_, HashSet<PhpValue>>> {
        match self {
            Self::Set(rc) => Some(rc.borrow()),
            _ => None,
        }
    }

    /// Mutably borrows the underlying Rust `HashSet<PhpValue>` structure.
    #[inline]
    pub fn as_set_mut(&self) -> Option<std::cell::RefMut<'_, HashSet<PhpValue>>> {
        match self {
            Self::Set(rc) => Some(rc.borrow_mut()),
            _ => None,
        }
    }

    /// Returns the length of the set if this value is a Set.
    #[inline]
    pub fn set_len(&self) -> Option<usize> {
        match self {
            Self::Set(rc) => Some(rc.borrow().len()),
            _ => None,
        }
    }

    /// Checks if a value exists in the `HashSet<PhpValue>`.
    #[inline]
    pub fn set_contains(&self, val: &PhpValue) -> bool {
        match self {
            Self::Set(rc) => rc.borrow().contains(val),
            _ => false,
        }
    }

    /// Detaches the map buffer if shared (`strong_count > 1`) (Copy-On-Write).
    /// Returns `true` if this value is a HashMap, `false` otherwise.
    pub fn ensure_hash_map_mut(&mut self) -> bool {
        if let Self::HashMap(rc) = self {
            if Rc::strong_count(rc) > 1 {
                let cloned = rc.borrow().clone();
                *rc = Rc::new(RefCell::new(cloned));
                crate::vm::memory::record_cow_detachment();
            }
            true
        } else {
            false
        }
    }

    /// Returns `true` if this value is an explicit subtyped universal hash map (`Arr::hashMap`).
    #[inline]
    pub fn is_hash_map(&self) -> bool {
        matches!(self, Self::HashMap(_))
    }

    /// Borrows the underlying Rust `HashMap<PhpValue, PhpValue>` structure.
    #[inline]
    pub fn as_hash_map(&self) -> Option<std::cell::Ref<'_, HashMap<PhpValue, PhpValue>>> {
        match self {
            Self::HashMap(rc) => Some(rc.borrow()),
            _ => None,
        }
    }

    /// Mutably borrows the underlying Rust `HashMap<PhpValue, PhpValue>` structure.
    #[inline]
    pub fn as_hash_map_mut(&self) -> Option<std::cell::RefMut<'_, HashMap<PhpValue, PhpValue>>> {
        match self {
            Self::HashMap(rc) => Some(rc.borrow_mut()),
            _ => None,
        }
    }

    /// Returns the number of entries in the hash map if this value is a HashMap.
    #[inline]
    pub fn hash_map_len(&self) -> Option<usize> {
        match self {
            Self::HashMap(rc) => Some(rc.borrow().len()),
            _ => None,
        }
    }

    /// Accesses an entry by complex key from the `HashMap<PhpValue, PhpValue>`.
    #[inline]
    pub fn hash_map_get(&self, key: &PhpValue) -> Option<PhpValue> {
        match self {
            Self::HashMap(rc) => rc.borrow().get(key).cloned(),
            _ => None,
        }
    }

    /// Checks if a key exists in the `HashMap<PhpValue, PhpValue>`.
    #[inline]
    pub fn hash_map_contains_key(&self, key: &PhpValue) -> bool {
        match self {
            Self::HashMap(rc) => rc.borrow().contains_key(key),
            _ => false,
        }
    }

    /// Explicit deep clone of collections and objects (used by language `clone $obj`).
    /// Objects are cloned into fresh instances, while arrays are recursively cloned.
    pub fn deep_clone(&self) -> Self {
        match self {
            Self::Array(arr) => {
                let items: Vec<PhpValue> = arr.borrow().iter().map(|v| v.deep_clone()).collect();
                Self::Array(Rc::new(RefCell::new(items)))
            }
            Self::List(list) => {
                let items: Vec<PhpValue> = list.borrow().iter().map(|v| v.deep_clone()).collect();
                Self::List(Rc::new(RefCell::new(items)))
            }
            Self::Dict(dict) => {
                let mut map = IndexMap::with_capacity(dict.borrow().len());
                for (k, v) in dict.borrow().iter() {
                    map.insert(k.clone(), v.deep_clone());
                }
                Self::Dict(Rc::new(RefCell::new(map)))
            }
            Self::Set(set) => {
                let mut new_set = HashSet::with_capacity(set.borrow().len());
                for item in set.borrow().iter() {
                    new_set.insert(item.deep_clone());
                }
                Self::Set(Rc::new(RefCell::new(new_set)))
            }
            Self::HashMap(map) => {
                let mut new_map = HashMap::with_capacity(map.borrow().len());
                for (k, v) in map.borrow().iter() {
                    new_map.insert(k.deep_clone(), v.deep_clone());
                }
                Self::HashMap(Rc::new(RefCell::new(new_map)))
            }
            Self::Object(obj) => {
                let inner = obj.borrow();
                Self::Object(Rc::new(RefCell::new(inner.clone())))
            }
            Self::SharedObject(so) => Self::SharedObject(Arc::clone(so)),
            Self::SharedAtomic(sa) => Self::SharedAtomic(Arc::clone(sa)),
            Self::SharedArray(sa) => Self::SharedArray(Arc::clone(sa)),
            Self::SharedValue(sv) => Self::SharedValue(sv.clone()),
            other => other.clone(),
        }
    }

    /// Clones the collection buffer on-demand, while preserving reference handle
    /// identity for objects, closures, and futures.
    pub fn cow_clone(&self) -> Self {
        match self {
            Self::Array(arr) => Self::Array(Rc::new(RefCell::new(arr.borrow().clone()))),
            Self::List(list) => Self::List(Rc::new(RefCell::new(list.borrow().clone()))),
            Self::Dict(dict) => Self::Dict(Rc::new(RefCell::new(dict.borrow().clone()))),
            Self::Set(set) => Self::Set(Rc::new(RefCell::new(set.borrow().clone()))),
            Self::HashMap(map) => Self::HashMap(Rc::new(RefCell::new(map.borrow().clone()))),
            // Objects, Closures, and Futures ALWAYS preserve their reference handles
            other => other.clone(),
        }
    }
}

/// Trait representing strict equality comparison where types and values must strictly match.
pub trait StrictEq {
    /// Returns `true` if and only if types and values strictly match without type coercion.
    fn strict_eq(&self, other: &Self) -> bool;
}

impl StrictEq for PhpValue {
    #[inline]
    fn strict_eq(&self, other: &Self) -> bool {
        self.strict_eq(other)
    }
}

impl PartialEq for PhpValue {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.strict_eq(other)
    }
}

impl PartialOrd for PhpValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a.partial_cmp(b),
            (Self::Float(a), Self::Float(b)) => a.partial_cmp(b),
            (Self::Rune(a), Self::Rune(b)) => a.partial_cmp(b),
            (Self::String(a), Self::String(b)) => a.partial_cmp(b),
            (Self::Bytes(a), Self::Bytes(b)) => a.partial_cmp(b),
            (Self::Bool(a), Self::Bool(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

impl Eq for PhpValue {}

impl Hash for PhpValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Null => 0u8.hash(state),
            Self::Bool(b) => {
                1u8.hash(state);
                b.hash(state);
            }
            Self::Int(n) => {
                2u8.hash(state);
                (*n as i128).hash(state);
            }
            Self::Float(fl) => {
                3u8.hash(state);
                let normalized = if fl.is_nan() {
                    f64::NAN
                } else if *fl == 0.0 {
                    0.0f64
                } else {
                    *fl
                };
                normalized.to_bits().hash(state);
            }
            Self::Rune(c) => {
                17u8.hash(state);
                (*c as u32).hash(state);
            }
            Self::String(s) => {
                6u8.hash(state);
                s.hash(state);
            }
            Self::Bytes(b) => {
                16u8.hash(state);
                b.hash(state);
            }
            Self::EmptyArray => {
                15u8.hash(state);
            }
            Self::Object(obj) => {
                7u8.hash(state);
                Rc::as_ptr(obj).hash(state);
            }
            Self::Closure(c) => {
                8u8.hash(state);
                Rc::as_ptr(c).hash(state);
            }
            Self::Array(a) | Self::List(a) => {
                9u8.hash(state);
                if let Ok(b) = a.try_borrow() {
                    b.len().hash(state);
                    for item in b.iter() {
                        item.hash(state);
                    }
                }
            }
            Self::Dict(d) => {
                10u8.hash(state);
                if let Ok(b) = d.try_borrow() {
                    b.len().hash(state);
                    for (k, v) in b.iter() {
                        k.hash(state);
                        v.hash(state);
                    }
                }
            }
            Self::Set(s) => {
                11u8.hash(state);
                if let Ok(b) = s.try_borrow() {
                    b.len().hash(state);
                    let mut sum: u64 = 0;
                    for item in b.iter() {
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        item.hash(&mut h);
                        sum = sum.wrapping_add(h.finish());
                    }
                    sum.hash(state);
                }
            }
            Self::HashMap(m) => {
                12u8.hash(state);
                if let Ok(b) = m.try_borrow() {
                    b.len().hash(state);
                    let mut sum: u64 = 0;
                    for (k, v) in b.iter() {
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        k.hash(&mut h);
                        v.hash(&mut h);
                        sum = sum.wrapping_add(h.finish());
                    }
                    sum.hash(state);
                }
            }
            Self::Future(fut) => {
                13u8.hash(state);
                Rc::as_ptr(fut).hash(state);
            }
            Self::Resource(r) => {
                14u8.hash(state);
                Arc::as_ptr(r).hash(state);
            }
            Self::SharedObject(so) => {
                16u8.hash(state);
                Arc::as_ptr(so).hash(state);
            }
            Self::SharedAtomic(sa) => {
                17u8.hash(state);
                Arc::as_ptr(sa).hash(state);
            }
            Self::SharedArray(sa) => {
                18u8.hash(state);
                Arc::as_ptr(sa).hash(state);
            }
            Self::SharedValue(sv) => {
                19u8.hash(state);
                format!("{:?}", sv).hash(state);
            }
        }
    }
}

impl fmt::Debug for PhpValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "null"),
            Self::Bool(b) => write!(f, "{}", b),
            Self::Int(n) => write!(f, "{}", n),
            Self::Float(fl) => write!(f, "{:?}", fl),
            Self::Rune(c) => write!(f, "Rune({:?})", c),
            Self::String(s) => write!(f, "\"{}\"", s),
            Self::Bytes(b) => write!(f, "b\"{}\"", String::from_utf8_lossy(b)),
            Self::EmptyArray => write!(f, "EmptyArray"),
            Self::Array(arr) => write!(f, "Array({:?})", arr.borrow()),
            Self::List(list) => write!(f, "Arr::list({:?})", list.borrow()),
            Self::Dict(dict) => write!(f, "Arr::dict({:?})", dict.borrow()),
            Self::Set(set) => write!(f, "Arr::set({:?})", set.borrow()),
            Self::HashMap(map) => write!(f, "Arr::hashMap({:?})", map.borrow()),
            Self::Object(obj) => write!(f, "Object({:?})", obj.borrow()),
            Self::Closure(c) => write!(f, "Closure({}, upvals={})", c.name, c.upvalues.len()),
            Self::Future(fut) => write!(f, "Future(completed={})", fut.borrow().is_completed),
            Self::Resource(_) => write!(f, "Resource(<dyn>)"),
            Self::SharedObject(obj) => write!(f, "SharedObject({:?})", obj),
            Self::SharedAtomic(a) => write!(f, "SharedAtomic({})", a.load(AtomicOrdering::SeqCst)),
            Self::SharedArray(a) => write!(f, "SharedArray({:?})", a.read()),
            Self::SharedValue(v) => write!(f, "SharedValue({:?})", v),
        }
    }
}

impl fmt::Display for PhpValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_php_string())
    }
}

impl From<bool> for PhpValue {
    #[inline]
    fn from(b: bool) -> Self {
        PhpValue::Bool(b)
    }
}

impl From<char> for PhpValue {
    #[inline]
    fn from(c: char) -> Self {
        PhpValue::Rune(c)
    }
}

impl From<i32> for PhpValue {
    #[inline]
    fn from(n: i32) -> Self {
        PhpValue::Int((n) as i64)
    }
}

impl From<f32> for PhpValue {
    #[inline]
    fn from(fl: f32) -> Self {
        PhpValue::float(fl as f64)
    }
}

impl From<f64> for PhpValue {
    #[inline]
    fn from(fl: f64) -> Self {
        PhpValue::float(fl)
    }
}

impl From<&str> for PhpValue {
    #[inline]
    fn from(s: &str) -> Self {
        PhpValue::string(s)
    }
}

impl From<String> for PhpValue {
    #[inline]
    fn from(s: String) -> Self {
        PhpValue::string(s)
    }
}

impl From<Rc<str>> for PhpValue {
    #[inline]
    fn from(s: Rc<str>) -> Self {
        PhpValue::String(s)
    }
}

impl From<Vec<u8>> for PhpValue {
    #[inline]
    fn from(b: Vec<u8>) -> Self {
        PhpValue::bytes(b)
    }
}

impl From<&[u8]> for PhpValue {
    #[inline]
    fn from(b: &[u8]) -> Self {
        PhpValue::bytes(b)
    }
}

impl From<Rc<[u8]>> for PhpValue {
    #[inline]
    fn from(b: Rc<[u8]>) -> Self {
        PhpValue::Bytes(b)
    }
}

impl<T: Into<PhpValue>> From<Option<T>> for PhpValue {
    #[inline]
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(val) => val.into(),
            None => PhpValue::Null,
        }
    }
}

