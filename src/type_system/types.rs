use crate::ast::types::{TypeHint, TypeHintKind, Visibility};
use crate::lexer::span::Span;
use crate::type_system::error::{TypeError, TypeErrorKind};
use std::collections::HashMap;
use std::fmt;

/// Native Rust signed and unsigned integer types supported alongside default `int` (`i32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RustIntKind {
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
}

impl RustIntKind {
    /// Parse a Rust integer type name (`i8`..`i128`, `u8`..`u128`, `isize`, `usize`).
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "i8" => Some(Self::I8),
            "i16" => Some(Self::I16),
            "i32" => Some(Self::I32),
            "i64" => Some(Self::I64),
            "i128" => Some(Self::I128),
            "isize" => Some(Self::Isize),
            "u8" => Some(Self::U8),
            "u16" => Some(Self::U16),
            "u32" => Some(Self::U32),
            "u64" => Some(Self::U64),
            "u128" => Some(Self::U128),
            "usize" => Some(Self::Usize),
            _ => None,
        }
    }

    /// Canonical name of the Rust integer type.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::I128 => "i128",
            Self::Isize => "isize",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::U128 => "u128",
            Self::Usize => "usize",
        }
    }

    /// Returns true if this integer type is signed.
    pub fn is_signed(&self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::I128 | Self::Isize
        )
    }

    /// Bit width of the integer type (64 assumed for pointer-sized `isize`/`usize`).
    pub fn bit_width(&self) -> u32 {
        match self {
            Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 => 16,
            Self::I32 | Self::U32 => 32,
            Self::I64 | Self::U64 | Self::Isize | Self::Usize => 64,
            Self::I128 | Self::U128 => 128,
        }
    }
}

impl fmt::Display for RustIntKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Native Rust floating-point types supported alongside default `float`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RustFloatKind {
    F32,
    F64,
}

impl RustFloatKind {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "f32" => Some(Self::F32),
            "f64" => Some(Self::F64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }

    pub fn bit_width(&self) -> u32 {
        match self {
            Self::F32 => 32,
            Self::F64 => 64,
        }
    }
}

impl fmt::Display for RustFloatKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Explicit high-performance collection subtypes backed by Rust data structures (`Arr::*`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CollectionSubtype {
    /// `Arr::list()` -> `Box<Vec<PhpValue>>`
    List(Box<RhpType>),
    /// `Arr::dict()` -> `Box<IndexMap<String, PhpValue>>`
    Dict(Box<RhpType>),
    /// `Arr::set()` -> `Box<IndexSet<PhpValue>>`
    Set(Box<RhpType>),
    /// `Arr::hashMap()` -> `Box<HashMap<PhpValue, PhpValue>>`
    HashMap(Box<RhpType>, Box<RhpType>),
}

impl CollectionSubtype {
    /// Element value type yielded during iteration or functional transformations (`map`, `filter`).
    pub fn element_type(&self) -> &RhpType {
        match self {
            Self::List(t) | Self::Dict(t) | Self::Set(t) => t,
            Self::HashMap(_, v) => v,
        }
    }

    /// Key type yielded during `foreach ($col as $k => $v)`.
    pub fn key_type(&self) -> RhpType {
        match self {
            Self::List(_) | Self::Set(_) => RhpType::Int,
            Self::Dict(_) => RhpType::String,
            Self::HashMap(k, _) => (**k).clone(),
        }
    }

    /// Construct a new collection of the same subtype with an updated element value type.
    pub fn with_element_type(&self, new_elem: RhpType) -> Self {
        match self {
            Self::List(_) => Self::List(Box::new(new_elem)),
            Self::Dict(_) => Self::Dict(Box::new(new_elem)),
            Self::Set(_) => Self::Set(Box::new(new_elem)),
            Self::HashMap(k, _) => Self::HashMap(k.clone(), Box::new(new_elem)),
        }
    }
}

impl fmt::Display for CollectionSubtype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List(t) => {
                if **t == RhpType::Mixed || **t == RhpType::Dynamic {
                    write!(f, "List")
                } else {
                    write!(f, "List<{}>", t)
                }
            }
            Self::Dict(v) => {
                if **v == RhpType::Mixed || **v == RhpType::Dynamic {
                    write!(f, "Dict")
                } else {
                    write!(f, "Dict<string, {}>", v)
                }
            }
            Self::Set(t) => {
                if **t == RhpType::Mixed || **t == RhpType::Dynamic {
                    write!(f, "Set")
                } else {
                    write!(f, "Set<{}>", t)
                }
            }
            Self::HashMap(k, v) => {
                if (**k == RhpType::Mixed || **k == RhpType::Dynamic)
                    && (**v == RhpType::Mixed || **v == RhpType::Dynamic)
                {
                    write!(f, "HashMap")
                } else {
                    write!(f, "HashMap<{}, {}>", k, v)
                }
            }
        }
    }
}

/// Semantic type representation in the RHP Medium/Gradual Type System.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RhpType {
    /// Default integer primitive (`int`, standard `i32` semantics).
    Int,
    /// Default float primitive (`float`).
    Float,
    /// Explicit native Rust integer type (`i8`, `i16`, `i32`, `i64`, `i128`, `isize`, `u8`..`usize`).
    RustInt(RustIntKind),
    /// Explicit native Rust float type (`f32`, `f64`).
    RustFloat(RustFloatKind),
    /// UTF-8 string primitive (`string`, Copy-On-Write semantics).
    String,
    /// Boolean primitive (`bool`).
    Bool,
    /// Null value (`null`).
    Null,
    /// Void return type (`void`).
    Void,
    /// Bottom type (`never`) for functions/expressions that never return normally.
    Never,
    /// Universal top type (`mixed`) accepting any value at boundary contracts.
    Mixed,
    /// Gradually inferred dynamic type inside function bodies when static type is unconstrained.
    Dynamic,
    /// Generic object type (`object`).
    Object,
    /// Standard array type (`array`) with inferred element value type.
    Array(Box<RhpType>),
    /// Fixed-length tuple array literal (`[T1, T2, ...]`), supporting positional destructuring.
    Tuple(Vec<RhpType>),
    /// Explicit Rust-backed collection subtype (`Arr::list`, `Arr::dict`, `Arr::set`, `Arr::hashMap`).
    Collection(CollectionSubtype),
    /// Iterable interface/contract (`iterable`).
    Iterable(Box<RhpType>),
    /// Callable type (`callable`, closures, arrow functions).
    Callable {
        params: Option<Vec<RhpType>>,
        return_type: Option<Box<RhpType>>,
    },
    /// Asynchronous future handle (`Future<T>`).
    Future(Box<RhpType>),
    /// User-defined class, interface, trait, or enum (`User`, `OrderService`, etc.).
    Class(String),
    /// Nullable type (`?T`).
    Nullable(Box<RhpType>),
    /// Union of multiple types (`T1 | T2`).
    Union(Vec<RhpType>),
    /// Intersection of multiple types (`T1 & T2`).
    Intersection(Vec<RhpType>),
}

impl RhpType {
    /// Convert a parsed AST `TypeHint` into a semantic `RhpType`.
    pub fn from_type_hint(
        hint: &TypeHint<'_>,
        current_class: Option<&str>,
    ) -> Result<Self, TypeError> {
        match &hint.kind {
            TypeHintKind::Named(ident) => {
                let name = ident.name;
                match name {
                    "int" => Ok(RhpType::Int),
                    "float" => Ok(RhpType::Float),
                    "string" => Ok(RhpType::String),
                    "bool" | "true" | "false" => Ok(RhpType::Bool),
                    "void" => Ok(RhpType::Void),
                    "never" => Ok(RhpType::Never),
                    "mixed" => Ok(RhpType::Mixed),
                    "null" => Ok(RhpType::Null),
                    "object" => Ok(RhpType::Object),
                    "array" => Ok(RhpType::Array(Box::new(RhpType::Mixed))),
                    "iterable" => Ok(RhpType::Iterable(Box::new(RhpType::Mixed))),
                    "callable" => Ok(RhpType::Callable {
                        params: None,
                        return_type: None,
                    }),
                    "self" | "static" => {
                        if let Some(cls) = current_class {
                            Ok(RhpType::Class(cls.to_string()))
                        } else {
                            Ok(RhpType::Object)
                        }
                    }
                    "Future" => Ok(RhpType::Future(Box::new(RhpType::Mixed))),
                    "List" | "list" => Ok(RhpType::Collection(CollectionSubtype::List(Box::new(RhpType::Mixed)))),
                    "Dict" | "dict" => Ok(RhpType::Collection(CollectionSubtype::Dict(Box::new(RhpType::Mixed)))),
                    "Set" | "set" => Ok(RhpType::Collection(CollectionSubtype::Set(Box::new(RhpType::Mixed)))),
                    "HashMap" | "hashmap" | "Map" | "map" => Ok(RhpType::Collection(CollectionSubtype::HashMap(
                        Box::new(RhpType::Mixed),
                        Box::new(RhpType::Mixed),
                    ))),
                    "String" | "Boolean" | "Integer" | "boolean" | "integer" | "Float" => {
                        Err(TypeError::new(
                            TypeErrorKind::InvalidTypeAnnotation {
                                type_name: name.to_string(),
                                reason: "primitive types must be strictly lowercase (string, bool, int, float)".to_string(),
                            },
                            hint.span,
                        ))
                    }
                    other => {
                        if let Some(int_kind) = RustIntKind::from_name(other) {
                            return Ok(RhpType::RustInt(int_kind));
                        }
                        if let Some(float_kind) = RustFloatKind::from_name(other) {
                            return Ok(RhpType::RustFloat(float_kind));
                        }
                        Ok(RhpType::Class(other.to_string()))
                    }
                }
            }
            TypeHintKind::Nullable(inner) => {
                let resolved = Self::from_type_hint(inner, current_class)?;
                if matches!(resolved, RhpType::Void | RhpType::Never | RhpType::Mixed) {
                    return Err(TypeError::new(
                        TypeErrorKind::InvalidTypeAnnotation {
                            type_name: format!("?{}", resolved),
                            reason: format!("'{}' cannot be marked nullable", resolved),
                        },
                        hint.span,
                    ));
                }
                Ok(RhpType::Nullable(Box::new(resolved)))
            }
            TypeHintKind::Union(items) => {
                let mut resolved_items = Vec::with_capacity(items.len());
                for item in items {
                    let t = Self::from_type_hint(item, current_class)?;
                    if matches!(t, RhpType::Void) {
                        return Err(TypeError::new(
                            TypeErrorKind::InvalidTypeAnnotation {
                                type_name: hint.to_string(),
                                reason: "'void' cannot be part of a union type".to_string(),
                            },
                            item.span,
                        ));
                    }
                    if !resolved_items.contains(&t) {
                        resolved_items.push(t);
                    }
                }
                if resolved_items.len() == 1 {
                    Ok(resolved_items.into_iter().next().unwrap())
                } else {
                    Ok(RhpType::Union(resolved_items))
                }
            }
            TypeHintKind::Intersection(items) => {
                let mut resolved_items = Vec::with_capacity(items.len());
                for item in items {
                    let t = Self::from_type_hint(item, current_class)?;
                    if !matches!(t, RhpType::Class(_) | RhpType::Object) {
                        return Err(TypeError::new(
                            TypeErrorKind::InvalidTypeAnnotation {
                                type_name: hint.to_string(),
                                reason: format!(
                                    "intersection type member '{}' must be a class or interface",
                                    t
                                ),
                            },
                            item.span,
                        ));
                    }
                    if !resolved_items.contains(&t) {
                        resolved_items.push(t);
                    }
                }
                Ok(RhpType::Intersection(resolved_items))
            }
            TypeHintKind::Callable => Ok(RhpType::Callable {
                params: None,
                return_type: None,
            }),
            TypeHintKind::Array => Ok(RhpType::Array(Box::new(RhpType::Mixed))),
            TypeHintKind::Iterable => Ok(RhpType::Iterable(Box::new(RhpType::Mixed))),
        }
    }

    /// Returns true if this type is any integer type (`int` or `RustInt`).
    pub fn is_integer(&self) -> bool {
        matches!(self, RhpType::Int | RhpType::RustInt(_))
    }

    /// Returns true if this type is any floating-point type (`float` or `RustFloat`).
    pub fn is_float(&self) -> bool {
        matches!(self, RhpType::Float | RhpType::RustFloat(_))
    }

    /// Returns true if this type is numeric (`int`, `float`, `RustInt`, or `RustFloat`).
    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    /// Returns true if `null` is a valid inhabitant of this type.
    pub fn accepts_null(&self) -> bool {
        match self {
            RhpType::Null | RhpType::Nullable(_) | RhpType::Mixed | RhpType::Dynamic => true,
            RhpType::Union(types) => types.iter().any(|t| t.accepts_null()),
            _ => false,
        }
    }

    /// Strip `Null` / `Nullable` wrapper to obtain the non-null inner type (used in `?:`, `??`, `?->`).
    pub fn non_null_type(&self) -> RhpType {
        match self {
            RhpType::Nullable(inner) => (**inner).clone(),
            RhpType::Union(types) => {
                let filtered: Vec<RhpType> = types
                    .iter()
                    .filter(|t| !matches!(t, RhpType::Null))
                    .map(|t| t.non_null_type())
                    .collect();
                match filtered.len() {
                    0 => RhpType::Mixed,
                    1 => filtered.into_iter().next().unwrap(),
                    _ => RhpType::Union(filtered),
                }
            }
            other => other.clone(),
        }
    }

    /// Wrap this type as nullable (`?T`) if it doesn't already accept `null`.
    pub fn into_nullable(self) -> RhpType {
        if self.accepts_null() {
            self
        } else {
            RhpType::Nullable(Box::new(self))
        }
    }

    /// Combine two inferred types into a unified type (e.g., across `if`/`else` branches, ternary, or `match` arms).
    pub fn unify(a: &RhpType, b: &RhpType) -> RhpType {
        if a == b {
            return a.clone();
        }
        if matches!(a, RhpType::Never) {
            return b.clone();
        }
        if matches!(b, RhpType::Never) {
            return a.clone();
        }
        if matches!(a, RhpType::Dynamic) || matches!(b, RhpType::Dynamic) {
            return RhpType::Dynamic;
        }
        if matches!(a, RhpType::Mixed) || matches!(b, RhpType::Mixed) {
            return RhpType::Mixed;
        }

        // T + Null -> ?T
        if matches!(a, RhpType::Null) {
            return b.clone().into_nullable();
        }
        if matches!(b, RhpType::Null) {
            return a.clone().into_nullable();
        }

        // Numeric promotion: int + float -> float
        if (matches!(a, RhpType::Int) && matches!(b, RhpType::Float))
            || (matches!(a, RhpType::Float) && matches!(b, RhpType::Int))
        {
            return RhpType::Float;
        }

        // Flatten unions
        let mut members = Vec::new();
        let mut push_unique = |t: &RhpType| {
            if let RhpType::Union(inner) = t {
                for item in inner {
                    if !members.contains(item) {
                        members.push(item.clone());
                    }
                }
            } else if !members.contains(t) {
                members.push(t.clone());
            }
        };
        push_unique(a);
        push_unique(b);

        if members.len() == 1 {
            members.into_iter().next().unwrap()
        } else {
            RhpType::Union(members)
        }
    }

    /// Check if a value of type `self` can be assigned to or passed as a target of type `target`.
    ///
    /// Implements RHP's Medium/Gradual Type System rules:
    /// - Strict verification when static types are known (`int`, `float`, `string`, `bool`, classes, collections).
    /// - Gradual compatibility when source or target is `Mixed` or `Dynamic`.
    /// - Safe numeric widening (`int` -> `float`, literal `int` -> `RustInt`/`RustFloat`).
    /// - Subtype polymorphism for `Collection` -> `array`/`iterable`, `Tuple` -> `array`/`iterable`,
    ///   and class/interface inheritance hierarchies.
    pub fn is_assignable_to(&self, target: &RhpType, hierarchy: Option<&TypeHierarchy>) -> bool {
        if self == target {
            return true;
        }

        // Bottom type `never` is assignable to everything
        if matches!(self, RhpType::Never) {
            return true;
        }

        // Gradual typing & top type `mixed`:
        // Any non-void value is assignable to `mixed` or `Dynamic`.
        if matches!(target, RhpType::Mixed | RhpType::Dynamic) {
            return !matches!(self, RhpType::Void);
        }

        // Under gradual typing, a dynamically inferred unconstrained value (`Dynamic` or `Mixed`)
        // can cross into a typed boundary (enforced at the boundary at runtime).
        if matches!(self, RhpType::Dynamic | RhpType::Mixed) {
            return !matches!(target, RhpType::Void | RhpType::Never);
        }

        // Void cannot be assigned to non-void
        if matches!(self, RhpType::Void) || matches!(target, RhpType::Void) {
            return false;
        }

        // Source is Union: every variant in the union must be assignable to `target`
        if let RhpType::Union(source_variants) = self {
            return source_variants
                .iter()
                .all(|s| s.is_assignable_to(target, hierarchy));
        }

        // Source is Nullable(?T): both Null and T must be assignable to `target`
        if let RhpType::Nullable(inner) = self {
            return RhpType::Null.is_assignable_to(target, hierarchy)
                && inner.is_assignable_to(target, hierarchy);
        }

        // Target is Nullable(?T): accepts Null or anything assignable to T
        if let RhpType::Nullable(target_inner) = target {
            if matches!(self, RhpType::Null) {
                return true;
            }
            return self.is_assignable_to(target_inner, hierarchy);
        }

        // Target is Union: `self` must be assignable to at least one member of the union
        if let RhpType::Union(target_variants) = target {
            return target_variants
                .iter()
                .any(|t| self.is_assignable_to(t, hierarchy));
        }

        // Target is Intersection: `self` must be assignable to ALL members of the intersection
        if let RhpType::Intersection(target_members) = target {
            return target_members
                .iter()
                .all(|t| self.is_assignable_to(t, hierarchy));
        }

        // Source is Intersection: at least one member of the intersection satisfies `target`
        if let RhpType::Intersection(source_members) = self {
            return source_members
                .iter()
                .any(|s| s.is_assignable_to(target, hierarchy));
        }

        match (self, target) {
            // Numeric compatibility:
            // 1. Standard `int` widens to `float`, `RustInt`, or `RustFloat`
            (RhpType::Int, RhpType::Float | RhpType::RustInt(_) | RhpType::RustFloat(_)) => true,
            // 2. `RustInt` is compatible with `int`, `float`, `RustFloat`, or widening same-signedness `RustInt`
            (RhpType::RustInt(_), RhpType::Int | RhpType::Float | RhpType::RustFloat(_)) => true,
            (RhpType::RustInt(src_k), RhpType::RustInt(dst_k)) => {
                src_k == dst_k
                    || (src_k.is_signed() == dst_k.is_signed()
                        && src_k.bit_width() <= dst_k.bit_width())
            }
            // 3. `Float` and `RustFloat` are mutually compatible
            (RhpType::Float, RhpType::RustFloat(_)) | (RhpType::RustFloat(_), RhpType::Float) => {
                true
            }
            (RhpType::RustFloat(src_f), RhpType::RustFloat(dst_f)) => {
                src_f == dst_f || src_f.bit_width() <= dst_f.bit_width()
            }

            // Array, Tuple, Collection, and Iterable compatibility:
            (RhpType::Array(src_el), RhpType::Array(dst_el)) => {
                src_el.is_assignable_to(dst_el, hierarchy)
            }
            (RhpType::Tuple(elems), RhpType::Array(dst_el) | RhpType::Iterable(dst_el)) => elems
                .iter()
                .all(|e| e.is_assignable_to(dst_el, hierarchy)),
            (RhpType::Collection(sub), RhpType::Array(dst_el) | RhpType::Iterable(dst_el)) => {
                sub.element_type().is_assignable_to(dst_el, hierarchy)
            }
            (RhpType::Array(src_el), RhpType::Iterable(dst_el)) => {
                src_el.is_assignable_to(dst_el, hierarchy)
            }
            (RhpType::Array(src_el), RhpType::Collection(CollectionSubtype::List(dst_el))) => {
                src_el.is_assignable_to(dst_el, hierarchy)
            }
            (RhpType::Array(src_el), RhpType::Collection(CollectionSubtype::Dict(_dst_el))) => {
                // Allow empty array literal `[]` to initialize a Dict
                **src_el == RhpType::Mixed || **src_el == RhpType::Dynamic
            }
            (RhpType::Array(src_el), RhpType::Collection(CollectionSubtype::Set(dst_el))) => {
                src_el.is_assignable_to(dst_el, hierarchy)
            }
            (RhpType::Array(src_el), RhpType::Collection(CollectionSubtype::HashMap(_, _dst_v))) => {
                // Allow empty array literal `[]` to initialize a HashMap
                **src_el == RhpType::Mixed || **src_el == RhpType::Dynamic
            }
            (RhpType::Tuple(elems), RhpType::Collection(CollectionSubtype::List(dst_el))) => {
                elems.iter().all(|e| e.is_assignable_to(dst_el, hierarchy))
            }
            (RhpType::Collection(sub1), RhpType::Collection(sub2)) => match (sub1, sub2) {
                (CollectionSubtype::List(a), CollectionSubtype::List(b))
                | (CollectionSubtype::Dict(a), CollectionSubtype::Dict(b))
                | (CollectionSubtype::Set(a), CollectionSubtype::Set(b)) => {
                    a.is_assignable_to(b, hierarchy)
                }
                (CollectionSubtype::HashMap(k1, v1), CollectionSubtype::HashMap(k2, v2)) => {
                    k1.is_assignable_to(k2, hierarchy) && v1.is_assignable_to(v2, hierarchy)
                }
                _ => false,
            },

            // Callable compatibility:
            (
                RhpType::Callable {
                    params: src_p,
                    return_type: src_r,
                },
                RhpType::Callable {
                    params: dst_p,
                    return_type: dst_r,
                },
            ) => {
                let params_ok = match (src_p, dst_p) {
                    (_, None) => true,
                    (None, Some(_)) => true,
                    (Some(sp), Some(dp)) => {
                        sp.len() == dp.len()
                            && sp
                                .iter()
                                .zip(dp.iter())
                                .all(|(s, d)| d.is_assignable_to(s, hierarchy))
                    }
                };
                let ret_ok = match (src_r, dst_r) {
                    (_, None) => true,
                    (None, Some(_)) => true,
                    (Some(sr), Some(dr)) => sr.is_assignable_to(dr, hierarchy),
                };
                params_ok && ret_ok
            }

            // Future compatibility:
            (RhpType::Future(src_inner), RhpType::Future(dst_inner)) => {
                src_inner.is_assignable_to(dst_inner, hierarchy)
            }
            (RhpType::Future(_), RhpType::Class(name)) if name == "Future" => true,
            (RhpType::Class(name), RhpType::Future(_)) if name == "Future" => true,

            // Object compatibility:
            (
                RhpType::Class(_) | RhpType::Future(_) | RhpType::Collection(_),
                RhpType::Object,
            ) => true,

            // Class & Interface subtyping:
            (RhpType::Class(sub), RhpType::Class(sup)) => {
                if sub == sup {
                    return true;
                }
                if let Some(h) = hierarchy {
                    h.is_subtype_of(sub, sup)
                } else {
                    false
                }
            }

            _ => false,
        }
    }
}

impl fmt::Display for RhpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RhpType::Int => write!(f, "int"),
            RhpType::Float => write!(f, "float"),
            RhpType::RustInt(k) => write!(f, "{}", k),
            RhpType::RustFloat(k) => write!(f, "{}", k),
            RhpType::String => write!(f, "string"),
            RhpType::Bool => write!(f, "bool"),
            RhpType::Null => write!(f, "null"),
            RhpType::Void => write!(f, "void"),
            RhpType::Never => write!(f, "never"),
            RhpType::Mixed => write!(f, "mixed"),
            RhpType::Dynamic => write!(f, "dynamic"),
            RhpType::Object => write!(f, "object"),
            RhpType::Array(inner) => {
                if matches!(**inner, RhpType::Mixed | RhpType::Dynamic) {
                    write!(f, "array")
                } else {
                    write!(f, "array<{}>", inner)
                }
            }
            RhpType::Tuple(items) => {
                let s: Vec<String> = items.iter().map(|t| t.to_string()).collect();
                write!(f, "[{}]", s.join(", "))
            }
            RhpType::Collection(sub) => write!(f, "{}", sub),
            RhpType::Iterable(inner) => {
                if matches!(**inner, RhpType::Mixed | RhpType::Dynamic) {
                    write!(f, "iterable")
                } else {
                    write!(f, "iterable<{}>", inner)
                }
            }
            RhpType::Callable {
                params,
                return_type,
            } => match (params, return_type) {
                (Some(ps), Some(ret)) => {
                    let p_str: Vec<String> = ps.iter().map(|p| p.to_string()).collect();
                    write!(f, "callable({}): {}", p_str.join(", "), ret)
                }
                _ => write!(f, "callable"),
            },
            RhpType::Future(inner) => write!(f, "Future<{}>", inner),
            RhpType::Class(name) => write!(f, "{}", name),
            RhpType::Nullable(inner) => write!(f, "?{}", inner),
            RhpType::Union(types) => {
                let s: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "{}", s.join("|"))
            }
            RhpType::Intersection(types) => {
                let s: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "{}", s.join("&"))
            }
        }
    }
}

/// Typed formal parameter contract in a function or method signature.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSignature {
    pub name: String,
    pub rhp_type: RhpType,
    pub has_default: bool,
    pub is_variadic: bool,
    pub span: Span,
}

/// Enforced boundary contract for a standalone function or class/interface/trait/enum method.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionSignature {
    pub name: String,
    pub params: Vec<ParamSignature>,
    pub return_type: RhpType,
    pub is_async: bool,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub visibility: Visibility,
    pub span: Span,
}

impl FunctionSignature {
    /// Minimum number of positional arguments required at a call site.
    pub fn min_arity(&self) -> usize {
        self.params
            .iter()
            .filter(|p| !p.has_default && !p.is_variadic)
            .count()
    }

    /// Maximum number of positional arguments accepted (`None` if variadic).
    pub fn max_arity(&self) -> Option<usize> {
        if self.params.iter().any(|p| p.is_variadic) {
            None
        } else {
            Some(self.params.len())
        }
    }
}

/// Typed property metadata on a class or trait.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyTypeInfo {
    pub name: String,
    pub rhp_type: RhpType,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_readonly: bool,
    pub has_default: bool,
    pub span: Span,
}

/// Complete boundary contract for a declared class or trait.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassTypeInfo {
    pub name: String,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_readonly: bool,
    pub is_static_only: bool,
    pub is_singleton: bool,
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub traits: Vec<String>,
    pub properties: HashMap<String, PropertyTypeInfo>,
    pub constants: HashMap<String, RhpType>,
    pub methods: HashMap<String, FunctionSignature>,
    pub span: Span,
}

/// Complete boundary contract for a declared interface.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceTypeInfo {
    pub name: String,
    pub extends: Vec<String>,
    pub methods: HashMap<String, FunctionSignature>,
    pub span: Span,
}

/// Complete boundary contract for a declared enum.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumTypeInfo {
    pub name: String,
    pub backing_type: Option<RhpType>,
    pub implements: Vec<String>,
    pub cases: Vec<String>,
    pub methods: HashMap<String, FunctionSignature>,
    pub span: Span,
}

/// Nominal class, interface, trait, and enum hierarchy for subtype checking.
#[derive(Debug, Clone, Default)]
pub struct TypeHierarchy {
    pub classes: HashMap<String, ClassTypeInfo>,
    pub interfaces: HashMap<String, InterfaceTypeInfo>,
    pub traits: HashMap<String, ClassTypeInfo>,
    pub enums: HashMap<String, EnumTypeInfo>,
}

impl TypeHierarchy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if `sub` is a nominal subtype of `sup` via `extends`, `implements`, or `use` traits.
    pub fn is_subtype_of(&self, sub: &str, sup: &str) -> bool {
        if sub == sup {
            return true;
        }

        if let Some(cls) = self.classes.get(sub) {
            if let Some(parent) = &cls.extends {
                if parent == sup || self.is_subtype_of(parent, sup) {
                    return true;
                }
            }
            for iface in &cls.implements {
                if iface == sup || self.is_subtype_of(iface, sup) {
                    return true;
                }
            }
            for tr in &cls.traits {
                if tr == sup || self.is_subtype_of(tr, sup) {
                    return true;
                }
            }
        }

        if let Some(iface) = self.interfaces.get(sub) {
            for parent_iface in &iface.extends {
                if parent_iface == sup || self.is_subtype_of(parent_iface, sup) {
                    return true;
                }
            }
        }

        if let Some(enm) = self.enums.get(sub) {
            for iface in &enm.implements {
                if iface == sup || self.is_subtype_of(iface, sup) {
                    return true;
                }
            }
        }

        false
    }

    /// Look up a method signature on a class (walking up its parent chain and traits), interface, or enum.
    pub fn lookup_method(&self, class_name: &str, method_name: &str) -> Option<&FunctionSignature> {
        if let Some(cls) = self.classes.get(class_name) {
            if let Some(sig) = cls.methods.get(method_name) {
                return Some(sig);
            }
            for tr in &cls.traits {
                if let Some(trait_info) = self.traits.get(tr) {
                    if let Some(sig) = trait_info.methods.get(method_name) {
                        return Some(sig);
                    }
                }
            }
            if let Some(parent) = &cls.extends {
                return self.lookup_method(parent, method_name);
            }
        }
        if let Some(iface) = self.interfaces.get(class_name) {
            if let Some(sig) = iface.methods.get(method_name) {
                return Some(sig);
            }
            for parent in &iface.extends {
                if let Some(sig) = self.lookup_method(parent, method_name) {
                    return Some(sig);
                }
            }
        }
        if let Some(enm) = self.enums.get(class_name) {
            if let Some(sig) = enm.methods.get(method_name) {
                return Some(sig);
            }
        }
        if let Some(tr) = self.traits.get(class_name) {
            if let Some(sig) = tr.methods.get(method_name) {
                return Some(sig);
            }
        }
        None
    }

    /// Look up a property definition on a class (walking up its parent chain and traits).
    pub fn lookup_property(&self, class_name: &str, prop_name: &str) -> Option<&PropertyTypeInfo> {
        let bare = prop_name.strip_prefix('$').unwrap_or(prop_name);
        let with_dollar = format!("${}", bare);

        if let Some(cls) = self.classes.get(class_name) {
            if let Some(p) = cls
                .properties
                .get(&with_dollar)
                .or_else(|| cls.properties.get(bare))
            {
                return Some(p);
            }
            for tr in &cls.traits {
                if let Some(trait_info) = self.traits.get(tr) {
                    if let Some(p) = trait_info
                        .properties
                        .get(&with_dollar)
                        .or_else(|| trait_info.properties.get(bare))
                    {
                        return Some(p);
                    }
                }
            }
            if let Some(parent) = &cls.extends {
                return self.lookup_property(parent, prop_name);
            }
        }
        None
    }
}
