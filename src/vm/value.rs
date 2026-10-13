use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;

use crate::ir::opcode::FunctionId;
use crate::type_system::types::{CollectionSubtype, RhpType, RustFloatKind, RustIntKind, TypeHierarchy};

/// Maximum byte length for an inlined Small String (SSO).
pub const SSO_CAPACITY: usize = 23;

/// Small-String-Optimized (SSO) string representation inspired by Bun and JavaScriptCore.
///
/// Strings with length <= 23 bytes are stored inline on the stack with zero heap allocations.
/// Larger strings fall back to reference-counted heap buffers (`Rc<String>`).
#[derive(Clone)]
pub enum RhpString {
    Inline { len: u8, buf: [u8; SSO_CAPACITY] },
    Heap(Rc<String>),
}

impl RhpString {
    #[inline]
    pub fn new(s: &str) -> Self {
        let len = s.len();
        if len <= SSO_CAPACITY {
            let mut buf = [0u8; SSO_CAPACITY];
            buf[..len].copy_from_slice(s.as_bytes());
            Self::Inline {
                len: len as u8,
                buf,
            }
        } else {
            Self::Heap(Rc::new(s.to_string()))
        }
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Inline { len, buf } => {
                // Safety: bytes originated from a valid UTF-8 &str
                unsafe { std::str::from_utf8_unchecked(&buf[..*len as usize]) }
            }
            Self::Heap(rc) => rc.as_str(),
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

impl fmt::Display for RhpString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl fmt::Debug for RhpString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl PartialEq for RhpString {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<str> for RhpString {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for RhpString {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
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
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for RhpString {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
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
        if s.len() <= SSO_CAPACITY {
            Self::new(&s)
        } else {
            Self::Heap(Rc::new(s))
        }
    }
}

impl From<Rc<String>> for RhpString {
    #[inline]
    fn from(rc: Rc<String>) -> Self {
        if rc.len() <= SSO_CAPACITY {
            Self::new(&rc)
        } else {
            Self::Heap(rc)
        }
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
}

impl VmObject {
    pub fn new(class_name: impl Into<String>) -> Self {
        Self {
            class_name: class_name.into(),
            shape: Rc::new(RefCell::new(Shape::new())),
            slots: Vec::new(),
            properties: HashMap::new(),
            readonly_props: HashMap::new(),
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
    Dict(Rc<RefCell<Vec<(String, PhpValue)>>>),
    Set(Rc<RefCell<Vec<PhpValue>>>),
    HashMap(Rc<RefCell<Vec<(PhpValue, PhpValue)>>>),
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

/// Runtime asynchronous Future handle supporting sync resolution and delayed evaluation.
#[derive(Debug, Clone)]
pub struct VmFuture {
    pub is_completed: bool,
    pub value: Option<PhpValue>,
    pub error: Option<String>,
}

impl VmFuture {
    pub fn resolved(val: PhpValue) -> Self {
        Self {
            is_completed: true,
            value: Some(val),
            error: None,
        }
    }

    pub fn rejected(err: impl Into<String>) -> Self {
        Self {
            is_completed: true,
            value: None,
            error: Some(err.into()),
        }
    }
}

impl PhpValue {
    // =========================================================================
    // Constructors & Factories
    // =========================================================================

    #[inline]
    pub fn string(s: impl Into<RhpString>) -> Self {
        Self::String(s.into())
    }

    #[inline]
    pub fn array(items: Vec<PhpValue>) -> Self {
        Self::Array(Rc::new(RefCell::new(items)))
    }

    #[inline]
    pub fn list(items: Vec<PhpValue>) -> Self {
        Self::List(Rc::new(RefCell::new(items)))
    }

    #[inline]
    pub fn dict(entries: Vec<(String, PhpValue)>) -> Self {
        Self::Dict(Rc::new(RefCell::new(entries)))
    }

    #[inline]
    pub fn set(items: Vec<PhpValue>) -> Self {
        let mut unique: Vec<PhpValue> = Vec::with_capacity(items.len());
        for item in items {
            if !unique.contains(&item) {
                unique.push(item);
            }
        }
        Self::Set(Rc::new(RefCell::new(unique)))
    }

    #[inline]
    pub fn hash_map(entries: Vec<(PhpValue, PhpValue)>) -> Self {
        Self::HashMap(Rc::new(RefCell::new(entries)))
    }

    #[inline]
    pub fn object(obj: VmObject) -> Self {
        Self::Object(Rc::new(RefCell::new(obj)))
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
            RhpType::Array(_) => matches!(self, Self::Array(_) | Self::List(_)),
            RhpType::Collection(col) => match (col, self) {
                (CollectionSubtype::List(_), Self::List(_) | Self::Array(_)) => true,
                (CollectionSubtype::Dict(_), Self::Dict(_)) => true,
                (CollectionSubtype::Set(_), Self::Set(_)) => true,
                (CollectionSubtype::HashMap(_, _), Self::HashMap(_)) => true,
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
    // COW Shallow Clone
    // =========================================================================

    pub fn cow_clone(&self) -> Self {
        match self {
            Self::Array(arr) => Self::Array(Rc::new(RefCell::new(arr.borrow().clone()))),
            Self::List(list) => Self::List(Rc::new(RefCell::new(list.borrow().clone()))),
            Self::Dict(dict) => Self::Dict(Rc::new(RefCell::new(dict.borrow().clone()))),
            Self::Set(set) => Self::Set(Rc::new(RefCell::new(set.borrow().clone()))),
            Self::HashMap(map) => Self::HashMap(Rc::new(RefCell::new(map.borrow().clone()))),
            Self::Object(obj) => {
                let inner = obj.borrow();
                Self::Object(Rc::new(RefCell::new(inner.clone())))
            }
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
                n.hash(state);
            }
            Self::Float(fl) => {
                3u8.hash(state);
                fl.to_bits().hash(state);
            }
            Self::RustInt { kind, value } => {
                4u8.hash(state);
                kind.hash(state);
                value.hash(state);
            }
            Self::RustFloat { kind, value } => {
                5u8.hash(state);
                kind.hash(state);
                value.to_bits().hash(state);
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
            _ => 9u8.hash(state),
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
