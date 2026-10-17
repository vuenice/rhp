use crate::lexer::span::Span;
use crate::type_system::{
    error::{TypeError, TypeErrorKind},
    types::{CollectionSubtype, RhpType, RustFloatKind, RustIntKind, TypeHierarchy},
};
use std::collections::HashMap;

/// Tracks a local variable's dynamically inferred type and optional locked type annotation (`$x: int`).
#[derive(Debug, Clone, PartialEq)]
pub struct VariableBinding {
    pub name: String,
    pub inferred_type: RhpType,
    pub locked_type: Option<RhpType>,
    pub span: Span,
}

impl VariableBinding {
    /// Effective current type of the variable in the local scope.
    pub fn effective_type(&self) -> &RhpType {
        self.locked_type.as_ref().unwrap_or(&self.inferred_type)
    }
}

/// Scoped variable symbol table for dynamic type inference within function/method bodies.
#[derive(Debug, Clone, Default)]
pub struct TypeEnvironment {
    vars: HashMap<String, VariableBinding>,
}

impl TypeEnvironment {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
        }
    }

    /// Declare a variable with an explicit locked type annotation (`$x: int = 10;` or parameter `int $x`).
    pub fn declare_typed(
        &mut self,
        name: impl Into<String>,
        declared_type: RhpType,
        init_type: Option<&RhpType>,
        span: Span,
        hierarchy: Option<&TypeHierarchy>,
    ) -> Result<(), TypeError> {
        let var_name = normalize_var_name(name.into());

        if let Some(actual) = init_type {
            if !actual.is_assignable_to(&declared_type, hierarchy) {
                return Err(TypeError::new(
                    TypeErrorKind::VariableTypeMismatch {
                        var_name,
                        expected: declared_type.to_string(),
                        found: actual.to_string(),
                    },
                    span,
                ));
            }
        }

        let inferred = init_type.cloned().unwrap_or_else(|| declared_type.clone());
        self.vars.insert(
            var_name.clone(),
            VariableBinding {
                name: var_name,
                inferred_type: inferred,
                locked_type: Some(declared_type),
                span,
            },
        );
        Ok(())
    }

    /// Assign or dynamically infer a variable's type within a function body (`$subtotal = $q * $p;`).
    ///
    /// - If the variable was declared with an explicit locked type (`$x: int` or a typed parameter),
    ///   enforces compatibility against the locked type.
    /// - If the variable is untyped, dynamically updates its inferred type to `value_type`.
    pub fn assign_inferred(
        &mut self,
        name: impl Into<String>,
        value_type: RhpType,
        span: Span,
        hierarchy: Option<&TypeHierarchy>,
    ) -> Result<RhpType, TypeError> {
        let var_name = normalize_var_name(name.into());

        if let Some(existing) = self.vars.get_mut(&var_name) {
            if let Some(ref locked) = existing.locked_type {
                if !value_type.is_assignable_to(locked, hierarchy) {
                    return Err(TypeError::new(
                        TypeErrorKind::VariableTypeMismatch {
                            var_name,
                            expected: locked.to_string(),
                            found: value_type.to_string(),
                        },
                        span,
                    ));
                }
                existing.inferred_type = value_type;
                return Ok(locked.clone());
            } else {
                existing.inferred_type = value_type.clone();
                existing.span = span;
                return Ok(value_type);
            }
        }

        self.vars.insert(
            var_name.clone(),
            VariableBinding {
                name: var_name,
                inferred_type: value_type.clone(),
                locked_type: None,
                span,
            },
        );
        Ok(value_type)
    }

    /// Look up the current type of a variable in the scope.
    pub fn get_type(&self, name: &str) -> Option<RhpType> {
        let key = normalize_var_name(name.to_string());
        self.vars.get(&key).map(|b| {
            // Return the most specific known type (inferred_type if more specific than locked_type, or locked_type)
            if let Some(ref locked) = b.locked_type {
                if matches!(b.inferred_type, RhpType::Dynamic | RhpType::Mixed) {
                    locked.clone()
                } else {
                    b.inferred_type.clone()
                }
            } else {
                b.inferred_type.clone()
            }
        })
    }

    /// Look up the full `VariableBinding` entry.
    pub fn get_binding(&self, name: &str) -> Option<&VariableBinding> {
        let key = normalize_var_name(name.to_string());
        self.vars.get(&key)
    }

    /// Export all variable bindings as a map of `variable_name -> RhpType`.
    pub fn snapshot_types(&self) -> HashMap<String, RhpType> {
        self.vars
            .iter()
            .map(|(k, v)| (k.clone(), v.inferred_type.clone()))
            .collect()
    }

    /// Merge variable inferences from two conditional execution branches (`if` / `else`).
    pub fn merge_branch(&mut self, other: &TypeEnvironment) {
        for (name, other_binding) in &other.vars {
            if let Some(existing) = self.vars.get_mut(name) {
                existing.inferred_type =
                    RhpType::unify(&existing.inferred_type, &other_binding.inferred_type);
            } else {
                self.vars.insert(name.clone(), other_binding.clone());
            }
        }
    }
}

#[inline]
fn normalize_var_name(name: String) -> String {
    if name.starts_with('$') {
        name
    } else {
        format!("${}", name)
    }
}

/// Resolve the return type and validate arguments for a built-in native static facade call (`Facade::method(args)`).
pub fn resolve_facade_static_call(
    facade: &str,
    method: &str,
    arg_types: &[RhpType],
    call_span: Span,
    hierarchy: Option<&TypeHierarchy>,
) -> Result<RhpType, TypeError> {
    // Allow runtime macro registration on any facade (`Facade::macro('name', callable)`)
    if method == "macro" {
        return Ok(RhpType::Void);
    }

    match facade {
        "Int" => match method {
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
            | "u128" | "usize" => {
                if let Some(first_arg) = arg_types.first() {
                    if !first_arg.is_assignable_to(&RhpType::Int, hierarchy) {
                        return Err(TypeError::new(
                            TypeErrorKind::ArgumentTypeMismatch {
                                callable_name: format!("Int::{}", method),
                                param_name: "$value".to_string(),
                                arg_index: 1,
                                expected: "int".to_string(),
                                found: first_arg.to_string(),
                            },
                            call_span,
                        ));
                    }
                }
                let kind = RustIntKind::from_name(method).unwrap_or(RustIntKind::I32);
                Ok(RhpType::RustInt(kind))
            }
            "random" | "max" | "min" | "abs" | "clamp" => Ok(RhpType::Int),
            _ => Ok(RhpType::Int),
        },

        "Float" => match method {
            "f32" => {
                if let Some(first_arg) = arg_types.first() {
                    if !first_arg.is_assignable_to(&RhpType::Float, hierarchy) {
                        return Err(TypeError::new(
                            TypeErrorKind::ArgumentTypeMismatch {
                                callable_name: "Float::f32".to_string(),
                                param_name: "$value".to_string(),
                                arg_index: 1,
                                expected: "float".to_string(),
                                found: first_arg.to_string(),
                            },
                            call_span,
                        ));
                    }
                }
                Ok(RhpType::RustFloat(RustFloatKind::F32))
            }
            "f64" | "i64" => {
                if let Some(first_arg) = arg_types.first() {
                    if !first_arg.is_assignable_to(&RhpType::Float, hierarchy) {
                        return Err(TypeError::new(
                            TypeErrorKind::ArgumentTypeMismatch {
                                callable_name: format!("Float::{}", method),
                                param_name: "$value".to_string(),
                                arg_index: 1,
                                expected: "float".to_string(),
                                found: first_arg.to_string(),
                            },
                            call_span,
                        ));
                    }
                }
                Ok(RhpType::RustFloat(RustFloatKind::F64))
            }
            _ => Ok(RhpType::Float),
        },

        "Arr" => match method {
            "list" => {
                let elem_ty = arg_types
                    .first()
                    .map(extract_collection_element_type)
                    .unwrap_or(RhpType::Mixed);
                Ok(RhpType::Collection(CollectionSubtype::List(Box::new(
                    elem_ty,
                ))))
            }
            "dict" => {
                let elem_ty = arg_types
                    .first()
                    .map(extract_collection_element_type)
                    .unwrap_or(RhpType::Mixed);
                Ok(RhpType::Collection(CollectionSubtype::Dict(Box::new(
                    elem_ty,
                ))))
            }
            "set" => {
                let elem_ty = arg_types
                    .first()
                    .map(extract_collection_element_type)
                    .unwrap_or(RhpType::Mixed);
                Ok(RhpType::Collection(CollectionSubtype::Set(Box::new(
                    elem_ty,
                ))))
            }
            "hashMap" | "map_collection" => {
                let elem_ty = arg_types
                    .first()
                    .map(extract_collection_element_type)
                    .unwrap_or(RhpType::Mixed);
                Ok(RhpType::Collection(CollectionSubtype::HashMap(
                    Box::new(RhpType::Mixed),
                    Box::new(elem_ty),
                )))
            }
            "range" => Ok(RhpType::Collection(CollectionSubtype::List(Box::new(
                RhpType::Int,
            )))),
            "wrap" | "repeat" | "from" | "flatten" | "chunk" | "pluck" => {
                Ok(RhpType::Collection(CollectionSubtype::List(Box::new(
                    RhpType::Mixed,
                ))))
            }
            "merge" | "keys" | "values" | "unique" | "slice" | "reverse" | "filter" | "map" => {
                Ok(RhpType::Array(Box::new(RhpType::Mixed)))
            }
            "count" | "push" | "unshift" => Ok(RhpType::Int),
            "contains" | "hasKey" => Ok(RhpType::Bool),
            "search" => Ok(RhpType::Union(vec![
                RhpType::Int,
                RhpType::String,
                RhpType::Bool,
            ])),
            "first" | "last" | "pop" | "shift" | "reduce" => Ok(RhpType::Mixed),
            _ => Ok(RhpType::Mixed),
        },

        "Str" => match method {
            "slug" => {
                check_arg_type(
                    "Str::slug",
                    "$title",
                    1,
                    arg_types.first(),
                    &RhpType::String,
                    call_span,
                    hierarchy,
                )?;
                Ok(RhpType::String)
            }
            "random" => {
                check_arg_type(
                    "Str::random",
                    "$length",
                    1,
                    arg_types.first(),
                    &RhpType::Int,
                    call_span,
                    hierarchy,
                )?;
                Ok(RhpType::String)
            }
            "contains" | "startsWith" | "endsWith" => {
                check_arg_type(
                    &format!("Str::{}", method),
                    "$haystack",
                    1,
                    arg_types.first(),
                    &RhpType::String,
                    call_span,
                    hierarchy,
                )?;
                check_arg_type(
                    &format!("Str::{}", method),
                    "$needle",
                    2,
                    arg_types.get(1),
                    &RhpType::String,
                    call_span,
                    hierarchy,
                )?;
                Ok(RhpType::Bool)
            }
            "upper" | "lower" | "trim" | "ltrim" | "rtrim" | "substr" | "replace" | "join"
            | "format" => {
                check_arg_type(
                    &format!("Str::{}", method),
                    "$string",
                    1,
                    arg_types.first(),
                    &RhpType::Union(vec![
                        RhpType::String,
                        RhpType::Array(Box::new(RhpType::Mixed)),
                    ]),
                    call_span,
                    hierarchy,
                )?;
                Ok(RhpType::String)
            }
            "length" => {
                check_arg_type(
                    "Str::length",
                    "$string",
                    1,
                    arg_types.first(),
                    &RhpType::String,
                    call_span,
                    hierarchy,
                )?;
                Ok(RhpType::Int)
            }
            "split" => Ok(RhpType::Array(Box::new(RhpType::String))),
            "pos" => Ok(RhpType::Union(vec![RhpType::Int, RhpType::Bool])),
            _ => Ok(RhpType::String),
        },

        "File" => match method {
            "read" => {
                check_arg_type(
                    "File::read",
                    "$path",
                    1,
                    arg_types.first(),
                    &RhpType::String,
                    call_span,
                    hierarchy,
                )?;
                Ok(RhpType::String)
            }
            "write" => {
                check_arg_type(
                    "File::write",
                    "$path",
                    1,
                    arg_types.first(),
                    &RhpType::String,
                    call_span,
                    hierarchy,
                )?;
                check_arg_type(
                    "File::write",
                    "$contents",
                    2,
                    arg_types.get(1),
                    &RhpType::String,
                    call_span,
                    hierarchy,
                )?;
                Ok(RhpType::Int)
            }
            "exists" | "delete" => {
                check_arg_type(
                    &format!("File::{}", method),
                    "$path",
                    1,
                    arg_types.first(),
                    &RhpType::String,
                    call_span,
                    hierarchy,
                )?;
                Ok(RhpType::Bool)
            }
            _ => Ok(RhpType::Mixed),
        },

        "Future" => match method {
            "run" => {
                let inner_ret = arg_types
                    .first()
                    .and_then(|t| match t {
                        RhpType::Callable {
                            return_type: Some(r),
                            ..
                        } => Some((**r).clone()),
                        _ => None,
                    })
                    .unwrap_or(RhpType::Mixed);
                Ok(RhpType::Future(Box::new(inner_ret)))
            }
            "delayed" => {
                check_arg_type(
                    "Future::delayed",
                    "$milliseconds",
                    1,
                    arg_types.first(),
                    &RhpType::Int,
                    call_span,
                    hierarchy,
                )?;
                let ret = arg_types
                    .get(1)
                    .and_then(|t| match t {
                        RhpType::Callable {
                            return_type: Some(r),
                            ..
                        } => Some((**r).clone()),
                        _ => None,
                    })
                    .unwrap_or(RhpType::Void);
                Ok(ret)
            }
            "value" => {
                let inner = arg_types.first().cloned().unwrap_or(RhpType::Mixed);
                Ok(RhpType::Future(Box::new(inner)))
            }
            "error" => Ok(RhpType::Future(Box::new(RhpType::Never))),
            "wait" => {
                // If passed a tuple of callables/futures `[fn() => T1, fn() => T2]`, resolve to `[T1, T2]`!
                if let Some(RhpType::Tuple(items)) = arg_types.first() {
                    let resolved: Vec<RhpType> = items
                        .iter()
                        .map(|item| match item {
                            RhpType::Callable {
                                return_type: Some(r),
                                ..
                            } => (**r).clone(),
                            RhpType::Future(inner) => (**inner).clone(),
                            other => other.clone(),
                        })
                        .collect();
                    return Ok(RhpType::Tuple(resolved));
                }
                Ok(RhpType::Array(Box::new(RhpType::Mixed)))
            }
            "any" => {
                if let Some(RhpType::Tuple(items)) = arg_types.first() {
                    let mut acc: Option<RhpType> = None;
                    for item in items {
                        let resolved = match item {
                            RhpType::Callable {
                                return_type: Some(r),
                                ..
                            } => (**r).clone(),
                            RhpType::Future(inner) => (**inner).clone(),
                            other => other.clone(),
                        };
                        acc = Some(match acc {
                            None => resolved,
                            Some(prev) => RhpType::unify(&prev, &resolved),
                        });
                    }
                    if let Some(unified) = acc {
                        return Ok(unified);
                    }
                }
                Ok(RhpType::Mixed)
            }
            "defer" => Ok(RhpType::Void),
            _ => Ok(RhpType::Future(Box::new(RhpType::Mixed))),
        },

        "HTTP" => Ok(RhpType::Class("HTTPResponse".to_string())),
        "Image" => Ok(RhpType::Class("ImageInstance".to_string())),
        "URL" => match method {
            "to" | "route" | "current" | "full" => Ok(RhpType::String),
            _ => Ok(RhpType::String),
        },
        "Session" => match method {
            "has" => Ok(RhpType::Bool),
            "put" | "forget" => Ok(RhpType::Void),
            "get" => Ok(RhpType::Mixed),
            _ => Ok(RhpType::Mixed),
        },
        "DB" => Ok(RhpType::Class("QueryBuilder".to_string())),
        "Log" => Ok(RhpType::Void),
        "Sys" => match method {
            "print" | "dd" => Ok(RhpType::Void),
            "env" => Ok(RhpType::Mixed),
            "os" => Ok(RhpType::String),
            _ => Ok(RhpType::Mixed),
        },
        "Glob" => match method {
            "getClass" => Ok(RhpType::String),
            "match" => Ok(RhpType::Bool),
            "customHelper" => Ok(RhpType::Dynamic),
            _ => Ok(RhpType::Dynamic),
        },
        "Fiber" => match method {
            "current" => Ok(RhpType::Collection(CollectionSubtype::Dict(Box::new(RhpType::Mixed)))),
            "isFiber" => Ok(RhpType::Bool),
            "yield" => Ok(RhpType::Bool),
            "stats" => Ok(RhpType::Collection(CollectionSubtype::Dict(Box::new(RhpType::Mixed)))),
            "context" => Ok(RhpType::Mixed),
            "all" => Ok(RhpType::Collection(CollectionSubtype::List(Box::new(
                RhpType::Collection(CollectionSubtype::Dict(Box::new(RhpType::Mixed))),
            )))),
            _ => Ok(RhpType::Mixed),
        },
        "Weak" => match method {
            "to" => Ok(RhpType::Class("WeakReference".to_string())),
            "get" => Ok(RhpType::Nullable(Box::new(RhpType::Mixed))),
            "isValid" => Ok(RhpType::Bool),
            _ => Ok(RhpType::Mixed),
        },
        "phpFun" => {
            if let Some(mapping) = crate::lexer::facade::get_legacy_php_function(method) {
                resolve_facade_static_call(
                    mapping.rhp_facade,
                    mapping.rhp_method,
                    arg_types,
                    call_span,
                    hierarchy,
                )
            } else {
                Ok(RhpType::Dynamic)
            }
        }
        _ => Ok(RhpType::Dynamic),
    }
}

/// Resolve fluent instance method calls on built-in types (numeric wrappers, collections, futures, HTTP/Image/DB builders).
pub fn resolve_builtin_fluent_method(
    receiver_type: &RhpType,
    method: &str,
    arg_types: &[RhpType],
) -> Option<RhpType> {
    match receiver_type {
        // Fluent numeric APIs (`Int::i64()->random()->add()->subtract()`, `Float::f64()->toDecimal(2)`)
        RhpType::Int | RhpType::RustInt(_) => match method {
            "random" | "add" | "subtract" | "mul" | "div" | "abs" | "clamp" => {
                Some(receiver_type.clone())
            }
            "toDecimal" => Some(RhpType::RustFloat(RustFloatKind::F64)),
            "toString" => Some(RhpType::String),
            _ => None,
        },
        RhpType::Float | RhpType::RustFloat(_) => match method {
            "toDecimal" | "round" | "floor" | "ceil" | "add" | "subtract" | "mul" | "div"
            | "abs" | "random" => Some(receiver_type.clone()),
            "toInt" => Some(RhpType::Int),
            "toString" => Some(RhpType::String),
            _ => None,
        },

        // Fluent Collection subtype chaining (`Arr::list([1, 2, 3])->map(...)->filter(...)`)
        RhpType::Collection(sub) => match method {
            "map" => {
                let mapped_elem = arg_types
                    .first()
                    .and_then(|cb| match cb {
                        RhpType::Callable {
                            return_type: Some(r),
                            ..
                        } => Some((**r).clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| sub.element_type().clone());
                Some(RhpType::Collection(sub.with_element_type(mapped_elem)))
            }
            "filter" | "reject" | "unique" | "reverse" | "slice" | "sort" | "sortBy"
            | "sortKeys" | "sortKeysDesc" => Some(RhpType::Collection(sub.clone())),
            "reduce" => {
                let reduced = arg_types
                    .first()
                    .and_then(|cb| match cb {
                        RhpType::Callable {
                            return_type: Some(r),
                            ..
                        } => Some((**r).clone()),
                        _ => None,
                    })
                    .or_else(|| arg_types.get(1).cloned())
                    .unwrap_or_else(|| sub.element_type().clone());
                Some(reduced)
            }
            "find" | "first" | "last" => Some(sub.element_type().clone().into_nullable()),
            "count" => Some(RhpType::Int),
            "contains" | "isEmpty" | "isNotEmpty" => Some(RhpType::Bool),
            "all" | "toArray" => Some(RhpType::Array(Box::new(sub.element_type().clone()))),
            _ => {
                // Check if registered as a runtime macro on `Arr`
                if crate::lexer::facade::has_macro("Arr", method) {
                    Some(RhpType::Dynamic)
                } else {
                    Some(RhpType::Dynamic)
                }
            }
        },

        // Fluent array helper methods (`$arr->sort()`, `array $arr->filter(...)`, etc.)
        RhpType::Array(inner) => match method {
            "map" => {
                let mapped_elem = arg_types
                    .first()
                    .and_then(|cb| match cb {
                        RhpType::Callable {
                            return_type: Some(r),
                            ..
                        } => Some((**r).clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| (**inner).clone());
                Some(RhpType::Array(Box::new(mapped_elem)))
            }
            "filter" | "reject" | "unique" | "reverse" | "slice" | "sort" | "sortBy"
            | "sortKeys" | "sortKeysDesc" => Some(RhpType::Array(inner.clone())),
            "reduce" => {
                let reduced = arg_types
                    .first()
                    .and_then(|cb| match cb {
                        RhpType::Callable {
                            return_type: Some(r),
                            ..
                        } => Some((**r).clone()),
                        _ => None,
                    })
                    .or_else(|| arg_types.get(1).cloned())
                    .unwrap_or_else(|| (**inner).clone());
                Some(reduced)
            }
            "find" | "first" | "last" => Some((**inner).clone().into_nullable()),
            "count" => Some(RhpType::Int),
            "contains" | "isEmpty" | "isNotEmpty" | "hasKey" => Some(RhpType::Bool),
            "all" | "toArray" | "values" => Some(RhpType::Array(inner.clone())),
            "keys" => Some(RhpType::Array(Box::new(RhpType::Mixed))),
            "sum" | "avg" | "min" | "max" => Some(RhpType::Mixed),
            "get" => Some(RhpType::Mixed),
            _ => {
                if crate::lexer::facade::has_macro("Arr", method) {
                    Some(RhpType::Dynamic)
                } else {
                    Some(RhpType::Dynamic)
                }
            }
        },

        // Future handle (`$future->wait()`, `$future->then(...)`, etc.)
        RhpType::Future(inner) => match method {
            "wait" => Some((**inner).clone()),
            "then" => {
                let next_ty = arg_types
                    .first()
                    .and_then(|cb| match cb {
                        RhpType::Callable {
                            return_type: Some(r),
                            ..
                        } => Some((**r).clone()),
                        _ => None,
                    })
                    .unwrap_or(RhpType::Mixed);
                Some(RhpType::Future(Box::new(next_ty)))
            }
            "catch" | "catchError" => Some(RhpType::Future(inner.clone())),
            "isCompleted" => Some(RhpType::Bool),
            "hasError" => Some(RhpType::Bool),
            "value" => Some((**inner).clone()),
            "error" => Some(RhpType::String),
            "timeout" => Some((**inner).clone()),
            _ => Some(RhpType::Dynamic),
        },

        // Built-in fluent facade return objects (`HTTPResponse`, `ImageInstance`, `QueryBuilder`)
        RhpType::Class(name) => match name.as_str() {
            "HTTPResponse" => match method {
                "post" | "get" | "put" | "delete" | "withHeaders" | "timeout" => {
                    Some(RhpType::Class("HTTPResponse".to_string()))
                }
                "json" => Some(RhpType::Array(Box::new(RhpType::Mixed))),
                "body" => Some(RhpType::String),
                "status" => Some(RhpType::Int),
                "ok" => Some(RhpType::Bool),
                _ => Some(RhpType::Dynamic),
            },
            "ImageInstance" => match method {
                "resize" | "crop" | "rotate" | "watermark" => {
                    Some(RhpType::Class("ImageInstance".to_string()))
                }
                "save" => Some(RhpType::Bool),
                _ => Some(RhpType::Dynamic),
            },
            "QueryBuilder" => match method {
                "where" | "orderBy" | "limit" | "select" => {
                    Some(RhpType::Class("QueryBuilder".to_string()))
                }
                "get" => Some(RhpType::Collection(CollectionSubtype::List(Box::new(
                    RhpType::Mixed,
                )))),
                "find" | "first" => Some(RhpType::Dynamic),
                "count" => Some(RhpType::Int),
                _ => Some(RhpType::Dynamic),
            },
            "WeakReference" => match method {
                "get" => Some(RhpType::Nullable(Box::new(RhpType::Mixed))),
                "isValid" => Some(RhpType::Bool),
                _ => Some(RhpType::Dynamic),
            },
            _ => None,
        },

        _ if method == "wait" => Some(receiver_type.clone()),
        _ => None,
    }
}

fn extract_collection_element_type(arg: &RhpType) -> RhpType {
    match arg {
        RhpType::Array(inner) | RhpType::Iterable(inner) => (**inner).clone(),
        RhpType::Collection(sub) => sub.element_type().clone(),
        RhpType::Tuple(items) => {
            let mut acc: Option<RhpType> = None;
            for item in items {
                acc = Some(match acc {
                    None => item.clone(),
                    Some(prev) => RhpType::unify(&prev, item),
                });
            }
            acc.unwrap_or(RhpType::Mixed)
        }
        _ => RhpType::Mixed,
    }
}

fn check_arg_type(
    callable_name: &str,
    param_name: &str,
    arg_index: usize,
    actual: Option<&RhpType>,
    expected: &RhpType,
    call_span: Span,
    hierarchy: Option<&TypeHierarchy>,
) -> Result<(), TypeError> {
    if let Some(arg_ty) = actual {
        if !arg_ty.is_assignable_to(expected, hierarchy) {
            return Err(TypeError::new(
                TypeErrorKind::ArgumentTypeMismatch {
                    callable_name: callable_name.to_string(),
                    param_name: param_name.to_string(),
                    arg_index,
                    expected: expected.to_string(),
                    found: arg_ty.to_string(),
                },
                call_span,
            ));
        }
    }
    Ok(())
}
