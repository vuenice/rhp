use crate::ast::{
    expr::Argument,
    stmt::{Attribute, ClassDecl, MethodDecl, PropertyDecl},
};
use crate::lexer::span::Span;

/// Target and contextual metadata for an AST expansion site.
///
/// Immutable view passed to an [`Expander`](crate::expand::Expander) handler.
#[derive(Debug, Clone)]
pub enum ExpansionSite<'a, 's> {
    /// Class-level attribute: `#[Table('users')] class User { ... }`
    Class {
        attribute: &'a Attribute<'s>,
        class: &'a ClassDecl<'s>,
    },
    /// Method-level attribute: `#[Route('/api')] public function index() { ... }`
    Method {
        attribute: &'a Attribute<'s>,
        class: &'a ClassDecl<'s>,
        method: &'a MethodDecl<'s>,
    },
    /// Property-level attribute: `#[Column('email')] public string $email;`
    Property {
        attribute: &'a Attribute<'s>,
        class: &'a ClassDecl<'s>,
        property: &'a PropertyDecl<'s>,
    },
    /// Directive in a template file: `@if($cond) ... @endif` or `@datetime($x)`
    Directive {
        name: &'a str,
        args: &'a [Argument<'s>],
        raw_args: &'a str,
        span: Span,
        body: &'a [crate::expand::template::TemplateNode<'s>],
    },
}

impl<'a, 's> ExpansionSite<'a, 's> {
    /// Return the primary source span of the expansion site (the attribute or directive span).
    pub fn span(&self) -> Span {
        match self {
            Self::Class { attribute, .. } => attribute.span,
            Self::Method { attribute, .. } => attribute.span,
            Self::Property { attribute, .. } => attribute.span,
            Self::Directive { span, .. } => *span,
        }
    }

    /// Return the name of the attribute or directive being expanded.
    pub fn name(&self) -> &'a str {
        match self {
            Self::Class { attribute, .. } => attribute.name.as_str(),
            Self::Method { attribute, .. } => attribute.name.as_str(),
            Self::Property { attribute, .. } => attribute.name.as_str(),
            Self::Directive { name, .. } => name,
        }
    }

    /// Return the parsed arguments passed to the attribute or directive.
    pub fn args(&self) -> &'a [Argument<'s>] {
        match self {
            Self::Class { attribute, .. } => &attribute.args,
            Self::Method { attribute, .. } => &attribute.args,
            Self::Property { attribute, .. } => &attribute.args,
            Self::Directive { args, .. } => args,
        }
    }

    /// Return true if this expansion site is on a class.
    pub fn is_class(&self) -> bool {
        matches!(self, Self::Class { .. })
    }

    /// Return true if this expansion site is on a method.
    pub fn is_method(&self) -> bool {
        matches!(self, Self::Method { .. })
    }

    /// Return true if this expansion site is on a property.
    pub fn is_property(&self) -> bool {
        matches!(self, Self::Property { .. })
    }

    /// Return true if this expansion site is a template directive.
    pub fn is_directive(&self) -> bool {
        matches!(self, Self::Directive { .. })
    }
}
