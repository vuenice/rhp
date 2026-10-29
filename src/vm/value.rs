use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use crate::ir::opcode::FunctionId;
use crate::type_system::types::{CollectionSubtype, RhpType, RustFloatKind, RustIntKind, TypeHierarchy};
use crate::vm::parent_process::FrozenValue;

/// Maximum byte length for an inlined Small String (SSO).
pub const SSO_CAPACITY: usize = 23;

/// Small-String-Optimized (SSO) binary-safe string representation.
///
/// Under the hood, RHP strings are raw byte buffers with a known length:
/// - Strings with length <= 23 bytes are stored inline on the stack with zero heap allocations (`Inline`).
/// - Larger strings are reference-counted byte buffers (`Rc<Vec<u8>>`) with Copy-on-Write semantics.
/// - 100% binary-safe: naturally accommodates arbitrary binary payloads (images, streams) and
///   legacy multi-byte character sets (Shift-JIS, EUC-JP, ISO-2022-JP) without triggering Rust UTF-8 panics.
#[derive(Clone)]
pub enum RhpString {
    Inline { len: u8, buf: [u8; SSO_CAPACITY] },
    Heap(Rc<Vec<u8>>),
}

impl RhpString {
    #[inline]
    pub fn new(s: &str) -> Self {
        Self::from_bytes(s.as_bytes())
    }

    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let len = bytes.len();
        if len <= SSO_CAPACITY {
            let mut buf = [0u8; SSO_CAPACITY];
            buf[..len].copy_from_slice(bytes);
            Self::Inline {
                len: len as u8,
                buf,
            }
        } else {
            Self::Heap(Rc::new(bytes.to_vec()))
        }
    }

    #[inline]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        let len = bytes.len();
        if len <= SSO_CAPACITY {
            let mut buf = [0u8; SSO_CAPACITY];
            buf[..len].copy_from_slice(&bytes);
            Self::Inline {
                len: len as u8,
                buf,
            }
        } else {
            Self::Heap(Rc::new(bytes))
        }
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Inline { len, buf } => &buf[..*len as usize],
            Self::Heap(rc) => rc.as_slice(),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Self::Inline { len, .. } => *len as usize,
            Self::Heap(rc) => rc.len(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Inline { len, buf } => {
                std::str::from_utf8(&buf[..*len as usize]).unwrap_or("")
            }
            Self::Heap(rc) => {
                std::str::from_utf8(rc.as_slice()).unwrap_or("")
            }
        }
    }

    #[inline]
    pub fn to_str_lossy(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(self.as_bytes())
    }

    #[inline]
    pub fn is_utf8(&self) -> bool {
        std::str::from_utf8(self.as_bytes()).is_ok()
    }

    #[inline]
    pub fn is_inline(&self) -> bool {
        matches!(self, Self::Inline { .. })
    }

    #[inline]
    pub fn is_heap(&self) -> bool {
        matches!(self, Self::Heap(_))
    }

    #[inline]
    pub fn ref_count(&self) -> usize {
        match self {
            Self::Inline { .. } => 1,
            Self::Heap(rc) => Rc::strong_count(rc),
        }
    }

    #[inline]
    pub fn is_shared(&self) -> bool {
        match self {
            Self::Inline { .. } => false,
            Self::Heap(rc) => Rc::strong_count(rc) > 1,
        }
    }

    /// Mutably access or promote string buffer using Copy-On-Write semantics.
    pub fn make_mut(&mut self) -> &mut Vec<u8> {
        match self {
            Self::Inline { len, buf } => {
                let v = buf[..*len as usize].to_vec();
                *self = Self::Heap(Rc::new(v));
                match self {
                    Self::Heap(rc) => Rc::make_mut(rc),
                    _ => unreachable!(),
                }
            }
            Self::Heap(rc) => Rc::make_mut(rc),
        }
    }

    /// Append a byte slice utilizing SSO when possible or Copy-On-Write heap mutation.
    pub fn push_bytes(&mut self, b: &[u8]) {
        match self {
            Self::Inline { len, buf } => {
                let current_len = *len as usize;
                let add_len = b.len();
                if current_len + add_len <= SSO_CAPACITY {
                    buf[current_len..current_len + add_len].copy_from_slice(b);
                    *len = (current_len + add_len) as u8;
                } else {
                    let mut owned = Vec::with_capacity(current_len + add_len);
                    owned.extend_from_slice(&buf[..current_len]);
                    owned.extend_from_slice(b);
                    *self = Self::Heap(Rc::new(owned));
                }
            }
            Self::Heap(rc) => {
                Rc::make_mut(rc).extend_from_slice(b);
            }
        }
    }

    /// Append a string slice utilizing SSO when possible or Copy-On-Write heap mutation.
    pub fn push_str(&mut self, s: &str) {
        self.push_bytes(s.as_bytes());
    }

    /// Append a single character with COW semantics.
    pub fn push(&mut self, ch: char) {
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        self.push_str(encoded);
    }

    /// Append a single raw byte.
    pub fn push_byte(&mut self, b: u8) {
        self.push_bytes(&[b]);
    }

    /// Mutate a byte at index using COW semantics.
    pub fn set_byte(&mut self, index: usize, b: u8) {
        if index < self.len() {
            self.make_mut()[index] = b;
        }
    }

    /// Mutate a character at a byte/char index using COW semantics.
    pub fn set_char(&mut self, index: usize, ch: char) {
        if index < self.len() {
            if self.is_utf8() {
                let mut chars: Vec<char> = self.as_str().chars().collect();
                if index < chars.len() {
                    chars[index] = ch;
                    let new_str: String = chars.into_iter().collect();
                    *self = Self::new(&new_str);
                }
            } else {
                let mut buf = [0u8; 4];
                let enc = ch.encode_utf8(&mut buf).as_bytes();
                if !enc.is_empty() {
                    self.set_byte(index, enc[0]);
                }
            }
        }
    }
}

impl Deref for RhpString {
    type Target = str;
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for RhpString {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<[u8]> for RhpString {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl fmt::Display for RhpString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_str_lossy())
    }
}

impl fmt::Debug for RhpString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_str_lossy())
    }
}

impl PartialEq for RhpString {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<str> for RhpString {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<&str> for RhpString {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<[u8]> for RhpString {
    #[inline]
    fn eq(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }
}

impl PartialEq<&[u8]> for RhpString {
    #[inline]
    fn eq(&self, other: &&[u8]) -> bool {
        self.as_bytes() == *other
    }
}

impl Eq for RhpString {}

impl PartialOrd for RhpString {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RhpString {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl Hash for RhpString {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state);
    }
}

impl From<&str> for RhpString {
    #[inline]
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for RhpString {
    #[inline]
    fn from(s: String) -> Self {
        Self::from_vec(s.into_bytes())
    }
}

impl From<&String> for RhpString {
    #[inline]
    fn from(s: &String) -> Self {
        Self::from_bytes(s.as_bytes())
    }
}

impl From<&[u8]> for RhpString {
    #[inline]
    fn from(bytes: &[u8]) -> Self {
        Self::from_bytes(bytes)
    }
}

impl From<Vec<u8>> for RhpString {
    #[inline]
    fn from(bytes: Vec<u8>) -> Self {
        Self::from_vec(bytes)
    }
}

impl From<Rc<Vec<u8>>> for RhpString {
    #[inline]
    fn from(rc: Rc<Vec<u8>>) -> Self {
        if rc.len() <= SSO_CAPACITY {
            Self::from_bytes(&rc)
        } else {
            Self::Heap(rc)
        }
    }
}

impl From<Rc<String>> for RhpString {
    #[inline]
    fn from(rc: Rc<String>) -> Self {
        Self::from_bytes(rc.as_bytes())
    }
}

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

/// Runtime object representation with Shape-based slot indexing (Hidden Classes).
#[derive(Debug, Clone)]
pub struct VmObject {
    pub class_name: String,
    pub shape: Rc<RefCell<Shape>>,
    pub slots: Vec<PhpValue>,
    pub properties: HashMap<String, PhpValue>,
    pub readonly_props: HashMap<String, bool>,
    /// Optional weak reference target for breaking circular reference cycles.
    pub weak_target: Option<std::rc::Weak<RefCell<VmObject>>>,
}

impl VmObject {
    pub fn new(class_name: impl Into<String>) -> Self {
        Self {
            class_name: class_name.into(),
            shape: Rc::new(RefCell::new(Shape::new())),
            slots: Vec::new(),
            properties: HashMap::new(),
            readonly_props: HashMap::new(),
            weak_target: None,
        }
    }

    /// Construct a weak reference object wrapper (`Weak::to($parent)`).
    pub fn weak_reference(target: &Rc<RefCell<VmObject>>) -> Self {
        Self {
            class_name: "WeakReference".to_string(),
            shape: Rc::new(RefCell::new(Shape::new())),
            slots: Vec::new(),
            properties: HashMap::new(),
            readonly_props: HashMap::new(),
            weak_target: Some(Rc::downgrade(target)),
        }
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
}

/// Unified core runtime value enum representing all primitive and complex RHP values.
///
/// Implements Technical Point #7 of the RHP Architecture Specification:
/// - Represents primitive scalars directly with zero heap indirection (`Null`, `Bool`, `Int`, `Float`).
/// - Standard integer defaults to `i32` and standard float defaults to `f32`.
/// - Native Rust numeric subtypes (`i8`..`u128`, `f32`, `f64`) are held with exact bit-level precision.
/// - Strings utilize Small-String-Optimization (`RhpString`) for zero heap allocations on <= 23 bytes.
/// - Standard arrays and subtyped collections are heap-allocated with reference-counting.
/// - Objects, Closures, and Futures pass by reference handles (`Rc<RefCell<...>>`).
#[derive(Clone, Default)]
pub enum PhpValue {
    #[default]
    Null,
    Bool(bool),
    Int(i32),
    Float(f32),
    RustInt {
        kind: RustIntKind,
        value: i128,
    },
    RustFloat {
        kind: RustFloatKind,
        value: f64,
    },
    String(RhpString),
    Array(Rc<RefCell<Vec<PhpValue>>>),
    List(Rc<RefCell<Vec<PhpValue>>>),
    Dict(Rc<RefCell<IndexMap<String, PhpValue>>>),
    Set(Rc<RefCell<HashSet<PhpValue>>>),
    HashMap(Rc<RefCell<HashMap<PhpValue, PhpValue>>>),
    Object(Rc<RefCell<VmObject>>),
    Closure(Rc<VmClosure>),
    Future(Rc<RefCell<VmFuture>>),
    Resource(Arc<dyn std::any::Any + Send + Sync>),
}

/// Runtime closure and arrow-function handle referencing a compiled function and captured upvalues.
#[derive(Debug, Clone)]
pub struct VmClosure {
    pub func_id: FunctionId,
    pub name: String,
    pub upvalues: Vec<PhpValue>,
    pub is_static: bool,
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

/// Runtime asynchronous Future handle supporting non-blocking background task execution,
/// sync resolution, timers, parallel joins, and races.
#[derive(Clone)]
pub struct VmFuture {
    pub is_completed: bool,
    pub value: Option<PhpValue>,
    pub error: Option<String>,
    pub(crate) receiver: Option<Arc<Mutex<std::sync::mpsc::Receiver<Result<FrozenValue, String>>>>>,
}

impl VmFuture {
    pub fn resolved(val: PhpValue) -> Self {
        Self {
            is_completed: true,
            value: Some(val),
            error: None,
            receiver: None,
        }
    }

    pub fn rejected(err: impl Into<String>) -> Self {
        Self {
            is_completed: true,
            value: None,
            error: Some(err.into()),
            receiver: None,
        }
    }

    pub fn pending(receiver: std::sync::mpsc::Receiver<Result<FrozenValue, String>>) -> Self {
        Self {
            is_completed: false,
            value: None,
            error: None,
            receiver: Some(Arc::new(Mutex::new(receiver))),
        }
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
        if self.poll_completion() && self.receiver.is_none() {
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
        if self.poll_completion() && self.receiver.is_none() {
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

impl PhpValue {
    // =========================================================================
    // Constructors & Factories
    // =========================================================================

    #[inline]
    pub fn string(s: impl Into<RhpString>) -> Self {
        let rs = s.into();
        if rs.is_heap() {
            crate::vm::memory::record_alloc(rs.len() + 24);
        }
        Self::String(rs)
    }

    #[inline]
    pub fn bytes(b: impl Into<Vec<u8>>) -> Self {
        Self::string(b.into())
    }

    #[inline]
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
    pub fn closure(func_id: FunctionId, name: impl Into<String>, upvalues: Vec<PhpValue>, is_static: bool) -> Self {
        Self::Closure(Rc::new(VmClosure {
            func_id,
            name: name.into(),
            upvalues,
            is_static,
        }))
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
            Self::RustInt { kind, .. } => kind.as_str(),
            Self::RustFloat { kind, .. } => kind.as_str(),
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::List(_) => "Arr::list",
            Self::Dict(_) => "Arr::dict",
            Self::Set(_) => "Arr::set",
            Self::HashMap(_) => "Arr::hashMap",
            Self::Object(_) => "object",
            Self::Closure(_) => "Closure",
            Self::Future(_) => "Future",
            Self::Resource(_) => "resource",
        }
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
            Self::RustInt { value, .. } => *value != 0,
            Self::RustFloat { value, .. } => *value != 0.0 && !value.is_nan(),
            Self::String(s) => !s.is_empty() && s.as_str() != "0",
            Self::Array(arr) => !arr.borrow().is_empty(),
            Self::List(list) => !list.borrow().is_empty(),
            Self::Dict(dict) => !dict.borrow().is_empty(),
            Self::Set(set) => !set.borrow().is_empty(),
            Self::HashMap(map) => !map.borrow().is_empty(),
            Self::Object(_) | Self::Closure(_) | Self::Future(_) | Self::Resource(_) => true,
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

    /// Check if this value is a zero-cost primitive scalar (`int`, `float`, `bool`, `null`)
    /// that is stored unboxed without any `Rc<>` reference counting overhead.
    #[inline]
    pub fn is_scalar_primitive(&self) -> bool {
        matches!(
            self,
            Self::Null
                | Self::Bool(_)
                | Self::Int(_)
                | Self::Float(_)
                | Self::RustInt { .. }
                | Self::RustFloat { .. }
        )
    }

    /// Check if this value requires an `Rc<>` reference counted heap handle.
    /// Primitives (`int`, `float`, `bool`, `null`) return false: zero `Rc<>` overhead.
    #[inline]
    pub fn requires_rc(&self) -> bool {
        !self.is_scalar_primitive()
    }

    pub fn to_i32(&self) -> Option<i32> {
        match self {
            Self::Int(n) => Some(*n),
            Self::Bool(b) => Some(if *b { 1 } else { 0 }),
            Self::Float(f) => Some(*f as i32),
            Self::RustInt { value, .. } => Some(*value as i32),
            Self::RustFloat { value, .. } => Some(*value as i32),
            Self::String(s) => s.trim().parse::<i32>().ok(),
            _ => None,
        }
    }

    pub fn to_i64(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n as i64),
            Self::Bool(b) => Some(if *b { 1 } else { 0 }),
            Self::Float(f) => Some(*f as i64),
            Self::RustInt { value, .. } => Some(*value as i64),
            Self::RustFloat { value, .. } => Some(*value as i64),
            Self::String(s) => s.trim().parse::<i64>().ok(),
            _ => None,
        }
    }

    pub fn to_f32(&self) -> Option<f32> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Int(n) => Some(*n as f32),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Self::RustInt { value, .. } => Some(*value as f32),
            Self::RustFloat { value, .. } => Some(*value as f32),
            Self::String(s) => s.trim().parse::<f32>().ok(),
            _ => None,
        }
    }

    pub fn to_f64(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f as f64),
            Self::Int(n) => Some(*n as f64),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Self::RustInt { value, .. } => Some(*value as f64),
            Self::RustFloat { value, .. } => Some(*value),
            Self::String(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    }

    pub fn to_php_string(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Bool(b) => (if *b { "1" } else { "" }).to_string(),
            Self::Int(n) => n.to_string(),
            Self::Float(fl) => {
                if fl.fract() == 0.0 && !fl.is_infinite() && !fl.is_nan() {
                    fl.to_string()
                } else {
                    fl.to_string()
                }
            }
            Self::RustInt { value, .. } => value.to_string(),
            Self::RustFloat { value, .. } => value.to_string(),
            Self::String(s) => s.to_string(),
            Self::Array(_) => "Array".to_string(),
            Self::List(_) => "Arr::list".to_string(),
            Self::Dict(_) => "Arr::dict".to_string(),
            Self::Set(_) => "Arr::set".to_string(),
            Self::HashMap(_) => "Arr::hashMap".to_string(),
            Self::Object(obj) => format!("class@{}", obj.borrow().class_name),
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
            Self::RustInt { value, .. } => value.to_string(),
            Self::RustFloat { value, .. } => value.to_string(),
            Self::String(s) => s.to_string(),
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
            Self::Closure(c) => format!("Closure({})", c.name),
            Self::Future(fut) => format!("Future(completed={})", fut.borrow().is_completed),
            Self::Resource(_) => "Resource".to_string(),
        }
    }

    /// Access raw byte slice if this value is a binary string.
    #[inline]
    pub fn raw_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::String(s) => Some(s.as_bytes()),
            _ => None,
        }
    }

    // =========================================================================
    // Strict & Loose Equality
    // =========================================================================

    /// Strict equality comparison (`===`):
    /// Returns `true` if and only if types and values strictly match without type coercion.
    pub fn strict_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(x), Self::Bool(y)) => x == y,
            (Self::Int(x), Self::Int(y)) => x == y,
            (Self::Float(x), Self::Float(y)) => x == y,
            (
                Self::RustInt {
                    kind: k1,
                    value: v1,
                },
                Self::RustInt {
                    kind: k2,
                    value: v2,
                },
            ) => k1 == k2 && v1 == v2,
            (
                Self::RustFloat {
                    kind: k1,
                    value: v1,
                },
                Self::RustFloat {
                    kind: k2,
                    value: v2,
                },
            ) => k1 == k2 && v1 == v2,
            (Self::String(x), Self::String(y)) => x == y,
            (Self::Array(x), Self::Array(y)) | (Self::List(x), Self::List(y)) => {
                let a = x.borrow();
                let b = y.borrow();
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(v1, v2)| v1.strict_eq(v2))
            }
            (Self::Dict(x), Self::Dict(y)) => {
                let a = x.borrow();
                let b = y.borrow();
                a.len() == b.len() && a.iter().all(|(k, v1)| b.get(k).map_or(false, |v2| v1.strict_eq(v2)))
            }
            (Self::Set(x), Self::Set(y)) => *x.borrow() == *y.borrow(),
            (Self::HashMap(x), Self::HashMap(y)) => *x.borrow() == *y.borrow(),
            (Self::Object(x), Self::Object(y)) => Rc::ptr_eq(x, y),
            (Self::Closure(x), Self::Closure(y)) => Rc::ptr_eq(x, y),
            (Self::Future(x), Self::Future(y)) => Rc::ptr_eq(x, y),
            (Self::Resource(x), Self::Resource(y)) => Arc::ptr_eq(x, y),
            _ => false,
        }
    }

    /// Loose equality comparison (`==`):
    /// First performs strict type checking. If that succeeds (returns `true`),
    /// it returns `true`. If strict type checking fails or returns `false`,
    /// it converts both sides to string and compares them again.
    pub fn loose_eq(&self, other: &Self) -> bool {
        if self.strict_eq(other) {
            return true;
        }
        match (self, other) {
            (Self::Array(x), Self::Array(y)) | (Self::List(x), Self::List(y)) => {
                let a = x.borrow();
                let b = y.borrow();
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(v1, v2)| v1.loose_eq(v2))
            }
            (Self::Dict(x), Self::Dict(y)) => {
                let a = x.borrow();
                let b = y.borrow();
                a.len() == b.len() && a.iter().all(|(k, v1)| b.get(k).map_or(false, |v2| v1.loose_eq(v2)))
            }
            (Self::Array(_) | Self::List(_) | Self::Dict(_) | Self::Set(_) | Self::HashMap(_), _)
            | (_, Self::Array(_) | Self::List(_) | Self::Dict(_) | Self::Set(_) | Self::HashMap(_)) => {
                false
            }
            _ => self.to_php_string() == other.to_php_string(),
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
            RhpType::Int => matches!(self, Self::Int(_) | Self::RustInt { .. }),
            RhpType::Float => matches!(self, Self::Float(_) | Self::RustFloat { .. } | Self::Int(_)),
            RhpType::RustInt(kind) => match self {
                Self::RustInt { kind: k, .. } => k == kind,
                Self::Int(_) => true,
                _ => false,
            },
            RhpType::RustFloat(kind) => match self {
                Self::RustFloat { kind: k, .. } => k == kind,
                Self::Float(_) | Self::Int(_) => true,
                _ => false,
            },
            RhpType::String => matches!(self, Self::String(_)),
            RhpType::Array(inner) => {
                if **inner == RhpType::Mixed || **inner == RhpType::Dynamic {
                    matches!(
                        self,
                        Self::Array(_)
                            | Self::List(_)
                            | Self::Dict(_)
                            | Self::Set(_)
                            | Self::HashMap(_)
                    )
                } else {
                    match self {
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
            RhpType::Iterable(_) => matches!(
                self,
                Self::Array(_) | Self::List(_) | Self::Dict(_) | Self::Set(_) | Self::HashMap(_)
            ),
            RhpType::Callable { .. } => matches!(self, Self::Closure(_)),
            RhpType::Future(_) => matches!(self, Self::Future(_)),
            RhpType::Object => matches!(self, Self::Object(_) | Self::Closure(_) | Self::Future(_)),
            RhpType::Class(class_name) => match self {
                Self::Object(obj) => {
                    let obj_cls = &obj.borrow().class_name;
                    if obj_cls == class_name {
                        return true;
                    }
                    if let Some(h) = hierarchy {
                        h.is_subtype_of(obj_cls, class_name)
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
            Self::String(s) => s.ref_count(),
            Self::Array(rc) | Self::List(rc) => Rc::strong_count(rc),
            Self::Dict(rc) => Rc::strong_count(rc),
            Self::Set(rc) => Rc::strong_count(rc),
            Self::HashMap(rc) => Rc::strong_count(rc),
            Self::Object(rc) => Rc::strong_count(rc),
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
            Self::Array(_) | Self::List(_) | Self::Dict(_) | Self::Set(_) | Self::HashMap(_)
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

impl PartialEq for PhpValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Int(a), Self::Float(b)) => (*a as f32) == *b,
            (Self::Float(a), Self::Int(b)) => *a == (*b as f32),
            (Self::RustInt { value: a, .. }, Self::RustInt { value: b, .. }) => a == b,
            (Self::RustInt { value: a, .. }, Self::Int(b)) => *a == (*b as i128),
            (Self::Int(a), Self::RustInt { value: b, .. }) => (*a as i128) == *b,
            (Self::RustFloat { value: a, .. }, Self::RustFloat { value: b, .. }) => a == b,
            (Self::RustFloat { value: a, .. }, Self::Float(b)) => (*a as f32) == *b,
            (Self::Float(a), Self::RustFloat { value: b, .. }) => *a == (*b as f32),
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Array(a) | Self::List(a), Self::Array(b) | Self::List(b)) => {
                *a.borrow() == *b.borrow()
            }
            (Self::Dict(a), Self::Dict(b)) => *a.borrow() == *b.borrow(),
            (Self::Set(a), Self::Set(b)) => *a.borrow() == *b.borrow(),
            (Self::HashMap(a), Self::HashMap(b)) => *a.borrow() == *b.borrow(),
            (Self::Object(a), Self::Object(b)) => Rc::ptr_eq(a, b),
            (Self::Closure(a), Self::Closure(b)) => Rc::ptr_eq(a, b),
            (Self::Future(a), Self::Future(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl PartialOrd for PhpValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a.partial_cmp(b),
            (Self::Float(a), Self::Float(b)) => a.partial_cmp(b),
            (Self::Int(a), Self::Float(b)) => (*a as f32).partial_cmp(b),
            (Self::Float(a), Self::Int(b)) => a.partial_cmp(&(*b as f32)),
            (Self::RustInt { value: a, .. }, Self::RustInt { value: b, .. }) => a.partial_cmp(b),
            (Self::RustInt { value: a, .. }, Self::Int(b)) => a.partial_cmp(&(*b as i128)),
            (Self::Int(a), Self::RustInt { value: b, .. }) => (*a as i128).partial_cmp(b),
            (Self::RustFloat { value: a, .. }, Self::RustFloat { value: b, .. }) => a.partial_cmp(b),
            (Self::String(a), Self::String(b)) => a.partial_cmp(b),
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
            Self::RustInt { value, .. } => {
                2u8.hash(state);
                value.hash(state);
            }
            Self::Float(fl) => {
                3u8.hash(state);
                fl.to_bits().hash(state);
            }
            Self::RustFloat { value, .. } => {
                3u8.hash(state);
                (*value as f32).to_bits().hash(state);
            }
            Self::String(s) => {
                6u8.hash(state);
                s.hash(state);
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
            Self::RustInt { kind, value } => write!(f, "{}_{}", value, kind),
            Self::RustFloat { kind, value } => write!(f, "{:?}_{}", value, kind),
            Self::String(s) => write!(f, "\"{}\"", s),
            Self::Array(arr) => write!(f, "Array({:?})", arr.borrow()),
            Self::List(list) => write!(f, "Arr::list({:?})", list.borrow()),
            Self::Dict(dict) => write!(f, "Arr::dict({:?})", dict.borrow()),
            Self::Set(set) => write!(f, "Arr::set({:?})", set.borrow()),
            Self::HashMap(map) => write!(f, "Arr::hashMap({:?})", map.borrow()),
            Self::Object(obj) => write!(f, "Object({:?})", obj.borrow()),
            Self::Closure(c) => write!(f, "Closure({}, upvals={})", c.name, c.upvalues.len()),
            Self::Future(fut) => write!(f, "Future(completed={})", fut.borrow().is_completed),
            Self::Resource(_) => write!(f, "Resource(<dyn>)"),
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

impl From<i32> for PhpValue {
    #[inline]
    fn from(n: i32) -> Self {
        PhpValue::Int(n)
    }
}

impl From<f32> for PhpValue {
    #[inline]
    fn from(fl: f32) -> Self {
        PhpValue::Float(fl)
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

