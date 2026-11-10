use crate::ast::types::{TypeHint, TypeHintKind, Visibility};
use crate::lexer::span::Span;
use crate::type_system::error::{TypeError, TypeErrorKind};
use std::collections::HashMap;
use std::fmt;


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
    /// The only integer (`int`), stored as `i64`.
    Int,
    /// Default float declaration (`float`). Runtime storage is exact `f32`, exact `f64`, or `Decimal`.
    Float,
    /// UTF-8 string primitive (`string`, Utf8 subtype).
    String,
    /// Raw binary data subtype (`bytes`).
    Bytes,
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
        Self::from_type_hint_with_resolver(hint, current_class, &|name| {
            let clean = name.strip_prefix('\\').unwrap_or(name);
            clean.to_string()
        })
    }

    /// Convert a parsed AST `TypeHint` into a semantic `RhpType`, using a custom class name resolver.
    pub fn from_type_hint_with_resolver<F>(
        hint: &TypeHint<'_>,
        current_class: Option<&str>,
        resolver: &F,
    ) -> Result<Self, TypeError>
    where
        F: Fn(&str) -> String,
    {
        match &hint.kind {
            TypeHintKind::Named(ident) => {
                let name = ident.name;
                match name {
                    "int" | "Int" | "i64" => Ok(RhpType::Int),
                    "i8" | "i16" | "i32" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
                    | "u128" | "usize" => Err(TypeError::new(
                        TypeErrorKind::InvalidTypeAnnotation {
                            type_name: name.to_string(),
                            reason: "the only integer type is int (i64)".to_string(),
                        },
                        hint.span,
                    )),
                    "float" | "Float" | "f32" | "f64" => Ok(RhpType::Float),
                    "string" => Ok(RhpType::String),
                    "bytes" | "Bytes" => Ok(RhpType::Bytes),
                    "bool" | "Bool" | "true" | "false" => Ok(RhpType::Bool),
                    "void" => Ok(RhpType::Void),
                    "never" => Ok(RhpType::Never),
                    "mixed" => Ok(RhpType::Mixed),
                    "null" | "Null" => Ok(RhpType::Null),
                    "object" => Ok(RhpType::Object),
                    "array" => Ok(RhpType::Array(Box::new(RhpType::Mixed))),
                    "iterable" => Ok(RhpType::Iterable(Box::new(RhpType::Mixed))),
                    "callable" => Ok(RhpType::Callable {
                        params: None,
                        return_type: None,
                    }),
                    "self" | "static" => {
                        if let Some(cls) = current_class {
                            let resolved = resolver(cls);
                            let clean = resolved.strip_prefix('\\').unwrap_or(&resolved);
                            Ok(RhpType::Class(clean.to_string()))
                        } else {
                            Ok(RhpType::Object)
                        }
                    }
                    "Future" => Ok(RhpType::Future(Box::new(RhpType::Mixed))),
                    "List" | "list" => Ok(RhpType::Collection(CollectionSubtype::List(Box::new(RhpType::Mixed)))),
                    "Dict" | "dict" => Ok(RhpType::Collection(CollectionSubtype::Dict(Box::new(RhpType::Mixed)))),
                    "Set" | "set" => Ok(RhpType::Collection(CollectionSubtype::Set(Box::new(RhpType::Mixed)))),
                    "HashMap" | "hashMap" | "hashmap" | "Map" | "map" => Ok(RhpType::Collection(CollectionSubtype::HashMap(
                        Box::new(RhpType::Mixed),
                        Box::new(RhpType::Mixed),
                    ))),
                    "String" | "Boolean" | "Integer" | "boolean" | "integer" => {
                        Err(TypeError::new(
                            TypeErrorKind::InvalidTypeAnnotation {
                                type_name: name.to_string(),
                                reason: "primitive types must be strictly lowercase (string, bool, int, float)".to_string(),
                            },
                            hint.span,
                        ))
                    }
                    other => {
                        let resolved = resolver(other);
                        let clean_resolved = resolved.strip_prefix('\\').unwrap_or(&resolved);
                        Ok(RhpType::Class(clean_resolved.to_string()))
                    }
                }
            }
            TypeHintKind::Nullable(inner) => {
                let resolved = Self::from_type_hint_with_resolver(inner, current_class, resolver)?;
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
                    let t = Self::from_type_hint_with_resolver(item, current_class, resolver)?;
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
                    let t = Self::from_type_hint_with_resolver(item, current_class, resolver)?;
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

    /// Returns true if this type is `int` (`i64`).
    pub fn is_integer(&self) -> bool {
        matches!(self, RhpType::Int)
    }

    /// Returns true if this type is the `float` declaration.
    pub fn is_float(&self) -> bool {
        matches!(self, RhpType::Float)
    }

    /// Returns true if this type is numeric (`int` or `float`).
    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    /// Direct native Rust transformation type name:
    /// - `Int` -> `i64`
    /// - `Float` -> `f64`
    /// - `Bool` -> `bool`
    /// - `Null` -> `None`
    /// - Primitives require zero `Rc<>` overhead.
    pub fn to_native_rust_type(&self) -> &'static str {
        match self {
            RhpType::Int => "i64",
            RhpType::Float => "f64",
            RhpType::Bool => "bool",
            RhpType::Null => "None",
            RhpType::Void => "()",
            _ => "PhpValue",
        }
    }

    /// Returns true if this type is a zero-cost primitive scalar (`int`, `float`, `bool`, `null`)
    /// that never requires `Rc<>`.
    pub fn is_scalar_primitive(&self) -> bool {
        matches!(
            self,
            RhpType::Int
                | RhpType::Float
                | RhpType::Bool
                | RhpType::Null
        )
    }

    /// Returns true if this type requires an `Rc<>` heap handle.
    /// Primitives (int, float, bool, null) return false: zero `Rc<>` overhead.
    pub fn requires_rc(&self) -> bool {
        !self.is_scalar_primitive()
    }

    /// Returns true if `null` is a valid inhabitant of this type.
    pub fn accepts_null(&self) -> bool {
        match self {
            RhpType::Null | RhpType::Nullable(_) | RhpType::Mixed | RhpType::Dynamic => true,
            RhpType::Union(types) => types.iter().any(|t| t.accepts_null()),
            _ => false,
        }
    }

    /// Returns true if this type represents an object instance or class type.
    pub fn is_object_type(&self) -> bool {
        match self {
            RhpType::Object | RhpType::Class(_) => true,
            RhpType::Nullable(inner) => inner.is_object_type(),
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
    /// - Safe numeric widening (`int` -> `float`).
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
            (RhpType::Int, RhpType::Float) => true,

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
            (RhpType::Class(sub), RhpType::Iterable(_)) => {
                let sub_clean = sub.strip_prefix('\\').unwrap_or(sub);
                if let Some(h) = hierarchy {
                    h.is_subtype_of(sub_clean, "Traversable")
                        || h.is_subtype_of(sub_clean, "IteratorAggregate")
                        || h.is_subtype_of(sub_clean, "Iterator")
                } else {
                    sub_clean == "ArrayIterator"
                        || sub_clean == "Traversable"
                        || sub_clean == "IteratorAggregate"
                        || sub_clean == "Iterator"
                        || sub_clean.ends_with("Collection")
                        || sub_clean.ends_with("Paginator")
                }
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
            (
                RhpType::Class(class_name),
                RhpType::Callable {
                    params: dst_p,
                    return_type: dst_r,
                },
            ) => {
                if let Some(h) = hierarchy {
                    if let Some(sig) = h.lookup_method(class_name, "__invoke") {
                        let params_ok = match dst_p {
                            None => true,
                            Some(dp) => {
                                sig.params.len() == dp.len()
                                    && sig
                                        .params
                                        .iter()
                                        .zip(dp.iter())
                                        .all(|(s, d)| d.is_assignable_to(&s.rhp_type, hierarchy))
                            }
                        };
                        let ret_ok = match dst_r {
                            None => true,
                            Some(dr) => sig.return_type.is_assignable_to(dr, hierarchy),
                        };
                        params_ok && ret_ok
                    } else {
                        false
                    }
                } else {
                    false
                }
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
                let sub_clean = sub.strip_prefix('\\').unwrap_or(sub);
                let sup_clean = sup.strip_prefix('\\').unwrap_or(sup);
                if sub_clean == sup_clean {
                    return true;
                }
                if let Some(h) = hierarchy {
                    h.is_subtype_of(sub_clean, sup_clean)
                } else {
                    false
                }
            }

            _ => false,
        }
    }

    /// Returns true if two types are strictly comparable (`==` and `!=`).
    /// In RHP, `==` and `!=` perform strict comparison without implicit type coercion.
    /// If both sides are known concrete types and are not the same type, comparison is rejected at compile time.
    pub fn is_strictly_comparable_to(
        &self,
        other: &RhpType,
        hierarchy: Option<&TypeHierarchy>,
    ) -> bool {
        if self == other {
            return true;
        }

        // Gradual typing: unconstrained dynamic or mixed can be anything at runtime
        if matches!(self, RhpType::Dynamic | RhpType::Mixed)
            || matches!(other, RhpType::Dynamic | RhpType::Mixed)
        {
            return true;
        }

        // Null comparison: a type can be compared to Null if it accepts null, or is a reference/object type
        if matches!(self, RhpType::Null) {
            return other.accepts_null()
                || matches!(
                    other,
                    RhpType::Class(_)
                        | RhpType::Object
                        | RhpType::Intersection(_)
                        | RhpType::Future(_)
                        | RhpType::Callable { .. }
                );
        }
        if matches!(other, RhpType::Null) {
            return self.accepts_null()
                || matches!(
                    self,
                    RhpType::Class(_)
                        | RhpType::Object
                        | RhpType::Intersection(_)
                        | RhpType::Future(_)
                        | RhpType::Callable { .. }
                );
        }

        // Nullable types: ?T can be compared with T or ?T
        if let RhpType::Nullable(inner) = self {
            return inner.is_strictly_comparable_to(other, hierarchy);
        }
        if let RhpType::Nullable(inner) = other {
            return self.is_strictly_comparable_to(inner, hierarchy);
        }

        // Union types: at least one variant must be strictly comparable
        if let RhpType::Union(variants) = self {
            return variants
                .iter()
                .any(|v| v.is_strictly_comparable_to(other, hierarchy));
        }
        if let RhpType::Union(variants) = other {
            return variants
                .iter()
                .any(|v| self.is_strictly_comparable_to(v, hierarchy));
        }

        // Class hierarchies:
        match (self, other) {
            (RhpType::Class(c1), RhpType::Class(c2)) => {
                let s1 = c1.strip_prefix('\\').unwrap_or(c1);
                let s2 = c2.strip_prefix('\\').unwrap_or(c2);
                if s1 == s2 {
                    return true;
                }
                if let Some(h) = hierarchy {
                    h.is_subtype_of(s1, s2)
                        || h.is_subtype_of(s2, s1)
                } else {
                    false
                }
            }
            (RhpType::Object, RhpType::Class(_)) | (RhpType::Class(_), RhpType::Object) => true,

            // Sequential Arrays / Collections:
            (RhpType::Array(e1), RhpType::Array(e2)) => {
                e1.is_strictly_comparable_to(e2, hierarchy)
            }
            (
                RhpType::Collection(CollectionSubtype::List(e1)),
                RhpType::Collection(CollectionSubtype::List(e2)),
            ) => e1.is_strictly_comparable_to(e2, hierarchy),
            (RhpType::Array(e1), RhpType::Collection(CollectionSubtype::List(e2)))
            | (RhpType::Collection(CollectionSubtype::List(e1)), RhpType::Array(e2)) => {
                e1.is_strictly_comparable_to(e2, hierarchy)
            }
            (RhpType::Tuple(_), RhpType::Array(_)) | (RhpType::Array(_), RhpType::Tuple(_)) => true,
            (RhpType::Tuple(_), RhpType::Collection(CollectionSubtype::List(_)))
            | (RhpType::Collection(CollectionSubtype::List(_)), RhpType::Tuple(_)) => true,
            (RhpType::Tuple(t1), RhpType::Tuple(t2)) => {
                t1.len() == t2.len()
                    && t1
                        .iter()
                        .zip(t2.iter())
                        .all(|(a, b)| a.is_strictly_comparable_to(b, hierarchy))
            }

            // Keyed collections:
            (
                RhpType::Collection(CollectionSubtype::Dict(v1)),
                RhpType::Collection(CollectionSubtype::Dict(v2)),
            ) => v1.is_strictly_comparable_to(v2, hierarchy),
            (
                RhpType::Collection(CollectionSubtype::Set(e1)),
                RhpType::Collection(CollectionSubtype::Set(e2)),
            ) => e1.is_strictly_comparable_to(e2, hierarchy),
            (
                RhpType::Collection(CollectionSubtype::HashMap(k1, v1)),
                RhpType::Collection(CollectionSubtype::HashMap(k2, v2)),
            ) => {
                k1.is_strictly_comparable_to(k2, hierarchy)
                    && v1.is_strictly_comparable_to(v2, hierarchy)
            }

            (RhpType::Int, RhpType::Int) => true,

            _ => false,
        }
    }
}

impl fmt::Display for RhpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RhpType::Int => write!(f, "int"),
            RhpType::Float => write!(f, "float"),
            RhpType::String => write!(f, "string"),
            RhpType::Bytes => write!(f, "bytes"),
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
                let s: Vec<String> = types
                    .iter()
                    .map(|t| {
                        if matches!(t, RhpType::Intersection(_)) {
                            format!("({})", t)
                        } else {
                            t.to_string()
                        }
                    })
                    .collect();
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
    pub is_reference: bool,
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
    pub is_override: bool,
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
    pub is_shared: bool,
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub traits: Vec<String>,
    pub trait_uses: Vec<TraitUseInfo>,
    pub properties: HashMap<String, PropertyTypeInfo>,
    pub constants: HashMap<String, RhpType>,
    pub methods: HashMap<String, FunctionSignature>,
    pub span: Span,
}

/// Trait use descriptor with conflict and aliasing adaptations in type system.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitUseInfo {
    pub traits: Vec<String>,
    pub adaptations: Vec<TraitAdaptationInfo>,
    pub span: Span,
}

/// Adaptation rule in type system (`insteadof` or `as`).
#[derive(Debug, Clone, PartialEq)]
pub enum TraitAdaptationInfo {
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
    pub properties: HashMap<String, PropertyTypeInfo>,
    pub span: Span,
}

/// Complete nominal class, interface, trait, and enum hierarchy for subtype checking.
#[derive(Debug, Clone)]
pub struct TypeHierarchy {
    pub classes: HashMap<String, ClassTypeInfo>,
    pub interfaces: HashMap<String, InterfaceTypeInfo>,
    pub traits: HashMap<String, ClassTypeInfo>,
    pub enums: HashMap<String, EnumTypeInfo>,
}

fn default_standard_interfaces() -> HashMap<String, InterfaceTypeInfo> {
    let mut interfaces = HashMap::new();

    // 1. Traversable
    interfaces.insert(
        "Traversable".to_string(),
        InterfaceTypeInfo {
            name: "Traversable".to_string(),
            extends: Vec::new(),
            methods: HashMap::new(),
            span: Span::default(),
        },
    );

    // 2. IteratorAggregate
    let mut iter_agg_methods = HashMap::new();
    iter_agg_methods.insert(
        "getIterator".to_string(),
        FunctionSignature {
            name: "getIterator".to_string(),
            params: Vec::new(),
            return_type: RhpType::Class("Traversable".to_string()),
            is_async: false,
            is_static: false,
            is_abstract: true,
            is_final: false,
            is_override: false,
            visibility: Visibility::Public,
            span: Span::default(),
        },
    );
    interfaces.insert(
        "IteratorAggregate".to_string(),
        InterfaceTypeInfo {
            name: "IteratorAggregate".to_string(),
            extends: vec!["Traversable".to_string()],
            methods: iter_agg_methods,
            span: Span::default(),
        },
    );

    // 3. Iterator
    let mut iter_methods = HashMap::new();
    for (m_name, ret_ty) in [
        ("current", RhpType::Mixed),
        ("next", RhpType::Void),
        ("key", RhpType::Mixed),
        ("valid", RhpType::Bool),
        ("rewind", RhpType::Void),
    ] {
        iter_methods.insert(
            m_name.to_string(),
            FunctionSignature {
                name: m_name.to_string(),
                params: Vec::new(),
                return_type: ret_ty,
                is_async: false,
                is_static: false,
                is_abstract: true,
                is_final: false,
                is_override: false,
                visibility: Visibility::Public,
                span: Span::default(),
            },
        );
    }
    interfaces.insert(
        "Iterator".to_string(),
        InterfaceTypeInfo {
            name: "Iterator".to_string(),
            extends: vec!["Traversable".to_string()],
            methods: iter_methods,
            span: Span::default(),
        },
    );

    // 4. Countable
    let mut countable_methods = HashMap::new();
    countable_methods.insert(
        "count".to_string(),
        FunctionSignature {
            name: "count".to_string(),
            params: Vec::new(),
            return_type: RhpType::Int,
            is_async: false,
            is_static: false,
            is_abstract: true,
            is_final: false,
            is_override: false,
            visibility: Visibility::Public,
            span: Span::default(),
        },
    );
    interfaces.insert(
        "Countable".to_string(),
        InterfaceTypeInfo {
            name: "Countable".to_string(),
            extends: Vec::new(),
            methods: countable_methods,
            span: Span::default(),
        },
    );

    // 5. ArrayAccess
    let mut array_access_methods = HashMap::new();
    array_access_methods.insert(
        "offsetExists".to_string(),
        FunctionSignature {
            name: "offsetExists".to_string(),
            params: vec![ParamSignature {
                name: "offset".to_string(),
                rhp_type: RhpType::Mixed,
                has_default: false,
                is_variadic: false,
                is_reference: false,
                span: Span::default(),
            }],
            return_type: RhpType::Bool,
            is_async: false,
            is_static: false,
            is_abstract: true,
            is_final: false,
            is_override: false,
            visibility: Visibility::Public,
            span: Span::default(),
        },
    );
    array_access_methods.insert(
        "offsetGet".to_string(),
        FunctionSignature {
            name: "offsetGet".to_string(),
            params: vec![ParamSignature {
                name: "offset".to_string(),
                rhp_type: RhpType::Mixed,
                has_default: false,
                is_variadic: false,
                is_reference: false,
                span: Span::default(),
            }],
            return_type: RhpType::Mixed,
            is_async: false,
            is_static: false,
            is_abstract: true,
            is_final: false,
            is_override: false,
            visibility: Visibility::Public,
            span: Span::default(),
        },
    );
    array_access_methods.insert(
        "offsetSet".to_string(),
        FunctionSignature {
            name: "offsetSet".to_string(),
            params: vec![
                ParamSignature {
                    name: "offset".to_string(),
                    rhp_type: RhpType::Mixed,
                    has_default: false,
                    is_variadic: false,
                    is_reference: false,
                    span: Span::default(),
                },
                ParamSignature {
                    name: "value".to_string(),
                    rhp_type: RhpType::Mixed,
                    has_default: false,
                    is_variadic: false,
                    is_reference: false,
                    span: Span::default(),
                },
            ],
            return_type: RhpType::Void,
            is_async: false,
            is_static: false,
            is_abstract: true,
            is_final: false,
            is_override: false,
            visibility: Visibility::Public,
            span: Span::default(),
        },
    );
    array_access_methods.insert(
        "offsetUnset".to_string(),
        FunctionSignature {
            name: "offsetUnset".to_string(),
            params: vec![ParamSignature {
                name: "offset".to_string(),
                rhp_type: RhpType::Mixed,
                has_default: false,
                is_variadic: false,
                is_reference: false,
                span: Span::default(),
            }],
            return_type: RhpType::Void,
            is_async: false,
            is_static: false,
            is_abstract: true,
            is_final: false,
            is_override: false,
            visibility: Visibility::Public,
            span: Span::default(),
        },
    );
    interfaces.insert(
        "ArrayAccess".to_string(),
        InterfaceTypeInfo {
            name: "ArrayAccess".to_string(),
            extends: Vec::new(),
            methods: array_access_methods,
            span: Span::default(),
        },
    );

    // 6. RouteBindable
    let mut route_bindable_methods = HashMap::new();
    route_bindable_methods.insert(
        "resolveRouteBinding".to_string(),
        FunctionSignature {
            name: "resolveRouteBinding".to_string(),
            params: vec![ParamSignature {
                name: "value".to_string(),
                rhp_type: RhpType::String,
                has_default: false,
                is_variadic: false,
                is_reference: false,
                span: Span::default(),
            }],
            return_type: RhpType::Nullable(Box::new(RhpType::Mixed)),
            is_async: false,
            is_static: true,
            is_abstract: true,
            is_final: false,
            is_override: false,
            visibility: Visibility::Public,
            span: Span::default(),
        },
    );
    interfaces.insert(
        "RouteBindable".to_string(),
        InterfaceTypeInfo {
            name: "RouteBindable".to_string(),
            extends: Vec::new(),
            methods: route_bindable_methods,
            span: Span::default(),
        },
    );

    interfaces
}

impl Default for TypeHierarchy {
    fn default() -> Self {
        Self {
            classes: HashMap::new(),
            interfaces: default_standard_interfaces(),
            traits: HashMap::new(),
            enums: HashMap::new(),
        }
    }
}

impl TypeHierarchy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if `sub` is a nominal subtype of `sup` via `extends`, `implements`, or `use` traits.
    pub fn is_subtype_of(&self, sub: &str, sup: &str) -> bool {
        let sub_clean = sub.strip_prefix('\\').unwrap_or(sub);
        let sup_clean = sup.strip_prefix('\\').unwrap_or(sup);
        if sub_clean == sup_clean {
            return true;
        }

        // Built-in Iterator / Traversable / Collection relationships
        if sup_clean == "Traversable" {
            if sub_clean == "Iterator"
                || sub_clean == "IteratorAggregate"
                || sub_clean == "ArrayIterator"
                || sub_clean == "LengthAwarePaginator"
                || sub_clean == "Paginator"
                || sub_clean == "Collection"
                || sub_clean == "Illuminate\\Support\\Collection"
                || sub_clean.ends_with("Collection")
                || sub_clean.ends_with("Paginator")
            {
                return true;
            }
        }
        if sup_clean == "Iterator" && sub_clean == "ArrayIterator" {
            return true;
        }
        if (sup_clean == "IteratorAggregate" || sup_clean == "Countable")
            && (sub_clean == "LengthAwarePaginator"
                || sub_clean == "Paginator"
                || sub_clean == "Collection"
                || sub_clean == "Illuminate\\Support\\Collection")
        {
            return true;
        }
        if (sup_clean == "ArrayAccess" || sup_clean == "Countable") && sub_clean == "ArrayIterator" {
            return true;
        }

        if let Some(cls) = self.classes.get(sub_clean).or_else(|| self.classes.get(sub)) {
            if let Some(parent) = &cls.extends {
                let p_clean = parent.strip_prefix('\\').unwrap_or(parent);
                if p_clean == sup_clean || self.is_subtype_of(p_clean, sup_clean) {
                    return true;
                }
            }
            for iface in &cls.implements {
                let i_clean = iface.strip_prefix('\\').unwrap_or(iface);
                if i_clean == sup_clean || self.is_subtype_of(i_clean, sup_clean) {
                    return true;
                }
            }
            for tr in &cls.traits {
                let t_clean = tr.strip_prefix('\\').unwrap_or(tr);
                if t_clean == sup_clean || self.is_subtype_of(t_clean, sup_clean) {
                    return true;
                }
            }
        }

        if let Some(iface) = self.interfaces.get(sub_clean).or_else(|| self.interfaces.get(sub)) {
            for parent_iface in &iface.extends {
                let pi_clean = parent_iface.strip_prefix('\\').unwrap_or(parent_iface);
                if pi_clean == sup_clean || self.is_subtype_of(pi_clean, sup_clean) {
                    return true;
                }
            }
        }

        if let Some(enm) = self.enums.get(sub_clean).or_else(|| self.enums.get(sub)) {
            if sup_clean == "UnitEnum" {
                return true;
            }
            if enm.backing_type.is_some() && (sup_clean == "BackedEnum" || sup_clean == "UnitEnum") {
                return true;
            }
            for iface in &enm.implements {
                let i_clean = iface.strip_prefix('\\').unwrap_or(iface);
                if i_clean == sup_clean || self.is_subtype_of(i_clean, sup_clean) {
                    return true;
                }
            }
        }

        false
    }

    /// Look up a method signature on a class (walking up its parent chain and traits), interface, or enum.
    pub fn lookup_method(&self, class_name: &str, method_name: &str) -> Option<&FunctionSignature> {
        let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
        if let Some(cls) = self.classes.get(clean) {
            if let Some(sig) = cls.methods.get(method_name) {
                return Some(sig);
            }
            for tr in &cls.traits {
                let clean_tr = tr.strip_prefix('\\').unwrap_or(tr);
                if let Some(trait_info) = self.traits.get(clean_tr) {
                    if let Some(sig) = trait_info.methods.get(method_name) {
                        return Some(sig);
                    }
                }
            }
            if let Some(parent) = &cls.extends {
                return self.lookup_method(parent, method_name);
            }
        }
        if let Some(iface) = self.interfaces.get(clean) {
            if let Some(sig) = iface.methods.get(method_name) {
                return Some(sig);
            }
            for parent in &iface.extends {
                if let Some(sig) = self.lookup_method(parent, method_name) {
                    return Some(sig);
                }
            }
        }
        if let Some(enm) = self.enums.get(clean) {
            if let Some(sig) = enm.methods.get(method_name) {
                return Some(sig);
            }
        }
        if let Some(tr) = self.traits.get(clean) {
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
        if let Some(enm) = self.enums.get(class_name) {
            if let Some(p) = enm
                .properties
                .get(&with_dollar)
                .or_else(|| enm.properties.get(bare))
            {
                return Some(p);
            }
        }
        None
    }
}
