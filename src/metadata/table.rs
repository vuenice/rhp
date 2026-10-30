//! # Compile-Time Metadata Table (Alternative to PHP Reflection)
//!
//! In RHP, the compiler inspects every class, constructor, parameter type, and attribute
//! at build time. This module defines the read-only static metadata table emitted by the
//! compiler. The runtime, DI container, and router read this table directly with zero
//! runtime reflection overhead.

use std::collections::HashMap;
use std::fmt;

use crate::ast::types::Visibility;
use crate::type_system::types::RhpType;
use crate::metadata::registry::AttributeRegistry;

/// Evaluated value of an attribute argument.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeArgValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Array(Vec<AttributeArgValue>),
    Map(Vec<(String, AttributeArgValue)>),
    Symbol(String),
    Null,
}

impl AttributeArgValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) | Self::Symbol(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[AttributeArgValue]> {
        match self {
            Self::Array(arr) => Some(arr.as_slice()),
            _ => None,
        }
    }

    pub fn to_string_list(&self) -> Vec<String> {
        match self {
            Self::String(s) | Self::Symbol(s) => vec![s.clone()],
            Self::Array(arr) => arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect(),
            _ => Vec::new(),
        }
    }
}

impl fmt::Display for AttributeArgValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(s) => write!(f, "\"{}\"", s),
            Self::Symbol(s) => write!(f, "{}", s),
            Self::Int(n) => write!(f, "{}", n),
            Self::Float(n) => write!(f, "{}", n),
            Self::Bool(b) => write!(f, "{}", b),
            Self::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Self::Map(pairs) => {
                write!(f, "[")?;
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "'{}' => {}", k, v)?;
                }
                write!(f, "]")
            }
            Self::Null => write!(f, "null"),
        }
    }
}

/// A positional or named argument in a compiled attribute annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeArg {
    pub name: Option<String>,
    pub value: AttributeArgValue,
}

/// A compiled language attribute (e.g., `#[Route('/orders', methods: ['GET'])]`).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledAttribute {
    pub name: String,
    pub args: Vec<AttributeArg>,
}

impl CompiledAttribute {
    pub fn new(name: impl Into<String>, args: Vec<AttributeArg>) -> Self {
        Self {
            name: name.into(),
            args,
        }
    }

    /// Retrieve the first positional argument or an argument by name.
    pub fn get_arg(&self, name_or_first: &str) -> Option<&AttributeArgValue> {
        // Try matching named argument first
        for arg in &self.args {
            if let Some(ref name) = arg.name {
                if name.eq_ignore_ascii_case(name_or_first) {
                    return Some(&arg.value);
                }
            }
        }
        // Fallback to first argument if requested
        if name_or_first == "0" || name_or_first.is_empty() {
            return self.args.first().map(|a| &a.value);
        }
        None
    }

    /// Retrieve the first string argument value, if present.
    pub fn first_str(&self) -> Option<&str> {
        self.args.first().and_then(|a| a.value.as_str())
    }
}

/// Formal parameter metadata on a function or method.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamMetadata {
    pub name: String,
    pub rhp_type: RhpType,
    pub has_default: bool,
    pub is_variadic: bool,
    pub attributes: Vec<CompiledAttribute>,
}

impl ParamMetadata {
    pub fn has_attribute(&self, attr_name: &str) -> bool {
        self.attributes.iter().any(|a| a.name == attr_name)
    }

    pub fn get_attribute(&self, attr_name: &str) -> Option<&CompiledAttribute> {
        self.attributes.iter().find(|a| a.name == attr_name)
    }
}

/// Function or method metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodMetadata {
    pub name: String,
    pub params: Vec<ParamMetadata>,
    pub return_type: RhpType,
    pub attributes: Vec<CompiledAttribute>,
    pub is_static: bool,
    pub is_abstract: bool,
    pub visibility: Visibility,
}

impl MethodMetadata {
    pub fn has_attribute(&self, attr_name: &str) -> bool {
        self.attributes.iter().any(|a| a.name == attr_name)
    }

    pub fn get_attribute(&self, attr_name: &str) -> Option<&CompiledAttribute> {
        self.attributes.iter().find(|a| a.name == attr_name)
    }

    pub fn param(&self, name: &str) -> Option<&ParamMetadata> {
        let clean = name.strip_prefix('$').unwrap_or(name);
        self.params.iter().find(|p| p.name.strip_prefix('$').unwrap_or(&p.name) == clean)
    }
}

/// Standalone function metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionMetadata {
    pub name: String,
    pub params: Vec<ParamMetadata>,
    pub return_type: RhpType,
    pub attributes: Vec<CompiledAttribute>,
    pub is_async: bool,
}

/// Class property metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyMetadata {
    pub name: String,
    pub rhp_type: RhpType,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_readonly: bool,
    pub attributes: Vec<CompiledAttribute>,
}

/// Complete compile-time class metadata table.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassMetadata {
    pub name: String,
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub constructor: Option<MethodMetadata>,
    pub methods: HashMap<String, MethodMetadata>,
    pub properties: HashMap<String, PropertyMetadata>,
    pub attributes: Vec<CompiledAttribute>,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_singleton: bool,
}

impl ClassMetadata {
    pub fn has_attribute(&self, attr_name: &str) -> bool {
        self.attributes.iter().any(|a| a.name == attr_name)
    }

    pub fn get_attribute(&self, attr_name: &str) -> Option<&CompiledAttribute> {
        self.attributes.iter().find(|a| a.name == attr_name)
    }

    pub fn implements_interface(&self, iface: &str) -> bool {
        let clean = iface.strip_prefix('\\').unwrap_or(iface);
        self.implements.iter().any(|i| i.strip_prefix('\\').unwrap_or(i) == clean)
    }
}

/// Top-level static metadata table for an entire compiled RHP module.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProgramMetadata {
    /// Classes and traits indexed by canonical name.
    pub classes: HashMap<String, ClassMetadata>,
    /// Standalone functions indexed by name.
    pub functions: HashMap<String, FunctionMetadata>,
    /// Pre-indexed attribute registry for fast route, command, listener, and policy lookups.
    pub attributes: AttributeRegistry,
    /// Explicit string-to-RhpType registry for dynamic resolution (`app($className)`).
    pub string_to_type: HashMap<String, RhpType>,
}

impl ProgramMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retrieve class metadata by name.
    pub fn get_class(&self, name: &str) -> Option<&ClassMetadata> {
        let clean = name.strip_prefix('\\').unwrap_or(name);
        self.classes.get(clean).or_else(|| self.classes.get(name))
    }

    /// Retrieve function metadata by name.
    pub fn get_function(&self, name: &str) -> Option<&FunctionMetadata> {
        self.functions.get(name)
    }

    /// Find all classes tagged with a specific attribute.
    pub fn classes_with_attribute(&self, attr_name: &str) -> Vec<&ClassMetadata> {
        self.classes
            .values()
            .filter(|c| c.has_attribute(attr_name))
            .collect()
    }

    /// Find all methods across all classes tagged with a specific attribute.
    pub fn methods_with_attribute(&self, attr_name: &str) -> Vec<(&ClassMetadata, &MethodMetadata)> {
        let mut results = Vec::new();
        for class in self.classes.values() {
            for method in class.methods.values() {
                if method.has_attribute(attr_name) {
                    results.push((class, method));
                }
            }
        }
        results
    }
}
