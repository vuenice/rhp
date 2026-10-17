use crate::lexer::span::Span;
use std::fmt;

/// An identifier with its source span.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident<'s> {
    pub name: &'s str,
    pub span: Span,
}

impl<'s> Ident<'s> {
    #[inline]
    pub fn new(name: &'s str, span: Span) -> Self {
        Self { name, span }
    }

    #[inline]
    pub fn as_str(&self) -> &'s str {
        self.name
    }
}

impl<'s> fmt::Display for Ident<'s> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// A variable token (including the leading `$`) with its source span.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Variable<'s> {
    pub name: &'s str,
    pub span: Span,
}

impl<'s> Variable<'s> {
    #[inline]
    pub fn new(name: &'s str, span: Span) -> Self {
        Self { name, span }
    }

    /// Variable name without the leading `$` symbol.
    #[inline]
    pub fn bare_name(&self) -> &'s str {
        if self.name.starts_with('$') {
            &self.name[1..]
        } else {
            self.name
        }
    }
}

impl<'s> fmt::Display for Variable<'s> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// Member visibility modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

impl fmt::Display for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Visibility::Public => write!(f, "public"),
            Visibility::Protected => write!(f, "protected"),
            Visibility::Private => write!(f, "private"),
        }
    }
}

/// Class-level modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ClassModifiers {
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_readonly: bool,
    pub is_static_only: bool,
    pub is_singleton: bool,
}

/// Cast type targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastType {
    Int,
    Float,
    String,
    Bool,
    Array,
    Object,
}

/// Strongly-typed type hint attached to parameters, returns, and variables.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeHint<'s> {
    pub span: Span,
    pub kind: TypeHintKind<'s>,
}

impl<'s> TypeHint<'s> {
    #[inline]
    pub fn new(kind: TypeHintKind<'s>, span: Span) -> Self {
        Self { span, kind }
    }

    /// Checks if this type hint is marked nullable (`?Type`).
    pub fn is_nullable(&self) -> bool {
        matches!(self.kind, TypeHintKind::Nullable(_))
    }

    /// Returns the simple type name if this is a single named type.
    pub fn simple_name(&self) -> Option<&'s str> {
        match &self.kind {
            TypeHintKind::Named(ident) => Some(ident.name),
            TypeHintKind::Nullable(inner) => inner.simple_name(),
            _ => None,
        }
    }
}

/// Specific variant of a type hint.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeHintKind<'s> {
    /// Named type: primitives (`int`, `float`, `String`, `bool`, `void`, `mixed`),
    /// native Rust numeric types (`i64`, `f64`, `u32`, etc.), or user classes/interfaces.
    Named(Ident<'s>),
    /// Nullable type prefix (`?Type`).
    Nullable(Box<TypeHint<'s>>),
    /// Union types (`Type1 | Type2`).
    Union(Vec<TypeHint<'s>>),
    /// Intersection types (`Type1 & Type2`).
    Intersection(Vec<TypeHint<'s>>),
    /// Callable type keyword.
    Callable,
    /// Array type keyword.
    Array,
    /// Iterable type keyword.
    Iterable,
}

impl<'s> fmt::Display for TypeHint<'s> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            TypeHintKind::Named(ident) => write!(f, "{}", ident.name),
            TypeHintKind::Nullable(inner) => write!(f, "?{}", inner),
            TypeHintKind::Union(types) => {
                let s: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "{}", s.join("|"))
            }
            TypeHintKind::Intersection(types) => {
                let s: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "{}", s.join("&"))
            }
            TypeHintKind::Callable => write!(f, "callable"),
            TypeHintKind::Array => write!(f, "array"),
            TypeHintKind::Iterable => write!(f, "iterable"),
        }
    }
}
