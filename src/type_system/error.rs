use crate::lexer::span::Span;
use std::fmt;

/// Strongly-typed diagnostic emitted by the RHP Medium/Gradual Type Checker.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeError {
    pub span: Span,
    pub kind: TypeErrorKind,
    pub secondary_label: Option<(Span, String)>,
}

impl TypeError {
    #[inline]
    pub fn new(kind: TypeErrorKind, span: Span) -> Self {
        Self {
            span,
            kind,
            secondary_label: None,
        }
    }

    pub fn with_secondary_label(mut self, span: Span, text: impl Into<String>) -> Self {
        self.secondary_label = Some((span, text.into()));
        self
    }
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Type error at {}: {}", self.span, self.kind)
    }
}

impl std::error::Error for TypeError {}

/// Specific boundary contract or type inference violation.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TypeErrorKind {
    #[error(
        "Type Error: Parameter '{param_name}' in {callable_kind} '{callable_name}' must declare an explicit type signature across boundaries."
    )]
    MissingParameterType {
        callable_kind: String,
        callable_name: String,
        param_name: String,
    },

    #[error(
        "Type Error: {callable_kind} '{callable_name}' must declare an explicit return type signature across boundaries."
    )]
    MissingReturnType {
        callable_kind: String,
        callable_name: String,
    },

    #[error(
        "Type Error: Explicit pass-by-reference ('&{param_name}') in {callable_kind} '{callable_name}' is forbidden. Use explicit returns or mutable collection wrappers instead."
    )]
    PassByReferenceForbidden {
        callable_kind: String,
        callable_name: String,
        param_name: String,
    },

    #[error(
        "Type Error: Argument #{arg_index} ('{param_name}') of '{callable_name}()' is declared as pass-by-reference ('&') and must be passed as a reference ('&${var_name}')."
    )]
    MissingReferenceArgument {
        callable_name: String,
        param_name: String,
        arg_index: usize,
        var_name: String,
    },

    #[error(
        "Type Error: Argument #{arg_index} passed to '{callable_name}()' uses pass-by-reference ('&'), but parameter '{param_name}' does not accept references."
    )]
    UnexpectedReferenceArgument {
        callable_name: String,
        param_name: String,
        arg_index: usize,
    },

    #[error(
        "Type Error: Pass-by-reference argument #{arg_index} passed to '{callable_name}()' must be a variable."
    )]
    ReferenceArgumentMustBeVariable {
        callable_name: String,
        arg_index: usize,
    },

    #[error(
        "Type Error: Closure capture-by-reference ('&{var_name}') is forbidden. Objects pass by handle and arrays/strings use Copy-On-Write (COW) semantics."
    )]
    ClosureReferenceCaptureForbidden {
        var_name: String,
    },

    #[error(
        "Type Error: Closure capture of '$this' into property '{target}' is forbidden due to cyclic reference risk. Use 'Weak::to($this)' or a static closure ('static fn() => ...')."
    )]
    CyclicClosureCaptureForbidden {
        target: String,
    },

    #[error(
        "Type Error: Bidirectional object link on '{target}' is forbidden due to cyclic reference risk. Use an explicit weak reference wrapper ('Weak::to(...)') to prevent memory cycles."
    )]
    CyclicReferenceRisk {
        target: String,
    },

    #[error(
        "Type Error: {method_name} in class '{class_name}' cannot declare a return type ('{found_type}')."
    )]
    ConstructorReturnTypeForbidden {
        class_name: String,
        method_name: String,
        found_type: String,
    },

    #[error(
        "Type Error: Return type mismatch in {callable_kind} '{callable_name}': expected '{expected}', found '{found}'."
    )]
    ReturnTypeMismatch {
        callable_kind: String,
        callable_name: String,
        expected: String,
        found: String,
    },

    #[error(
        "Type Error: {callable_kind} '{callable_name}' with return type 'void' cannot return a value of type '{found}'."
    )]
    VoidReturnWithValue {
        callable_kind: String,
        callable_name: String,
        found: String,
    },

    #[error(
        "Type Error: {callable_kind} '{callable_name}' with return type 'never' must not return."
    )]
    NeverFunctionReturns {
        callable_kind: String,
        callable_name: String,
    },

    #[error(
        "Type Error: {callable_kind} '{callable_name}' declares return type '{expected}' but not all code paths return a value."
    )]
    MissingReturnStatement {
        callable_kind: String,
        callable_name: String,
        expected: String,
    },

    #[error(
        "Type Error: Argument #{arg_index} ('{param_name}') passed to {callable_name} has incompatible type: expected '{expected}', found '{found}'."
    )]
    ArgumentTypeMismatch {
        callable_name: String,
        param_name: String,
        arg_index: usize,
        expected: String,
        found: String,
    },

    #[error(
        "Type Error: {callable_name} expects between {min_args} and {max_args} arguments, but {found_args} were provided."
    )]
    ArityMismatch {
        callable_name: String,
        min_args: usize,
        max_args: usize,
        found_args: usize,
    },

    #[error(
        "Type Error: Unknown named argument '{arg_name}' in call to {callable_name}."
    )]
    UnknownNamedArgument {
        callable_name: String,
        arg_name: String,
    },

    #[error(
        "Type Error: Required parameter '{param_name}' cannot follow optional parameter '{optional_param_name}' in {callable_name}."
    )]
    RequiredParameterAfterOptional {
        callable_name: String,
        param_name: String,
        optional_param_name: String,
    },

    #[error(
        "Type Error: Cannot skip required parameter '{param_name}' in call to {callable_name}."
    )]
    CannotSkipRequiredParameter {
        callable_name: String,
        param_name: String,
    },

    #[error(
        "Type Error: Cannot assign value of type '{found}' to typed variable '{var_name}' of type '{expected}'."
    )]
    VariableTypeMismatch {
        var_name: String,
        expected: String,
        found: String,
    },

    #[error(
        "Type Error: Default value for parameter '{param_name}' in {callable_name} has incompatible type: expected '{expected}', found '{found}'."
    )]
    ParameterDefaultTypeMismatch {
        callable_name: String,
        param_name: String,
        expected: String,
        found: String,
    },

    #[error(
        "Type Error: Default value for property '{property_name}' in class '{class_name}' has incompatible type: expected '{expected}', found '{found}'."
    )]
    PropertyDefaultTypeMismatch {
        class_name: String,
        property_name: String,
        expected: String,
        found: String,
    },

    #[error(
        "Type Error: Cannot assign value of type '{found}' to property '{class_name}::{property_name}' of type '{expected}'."
    )]
    PropertyAssignmentTypeMismatch {
        class_name: String,
        property_name: String,
        expected: String,
        found: String,
    },

    #[error(
        "Type Error: Cannot modify readonly property '{class_name}::{property_name}' outside of constructor."
    )]
    ReadonlyPropertyMutation {
        class_name: String,
        property_name: String,
    },

    #[error(
        "Type Error: Class '{class_name}' fails to implement interface '{interface_name}': {reason}"
    )]
    InterfaceContractViolation {
        class_name: String,
        interface_name: String,
        reason: String,
    },

    #[error(
        "Type Error: Method '{class_name}::{method_name}' violates parent contract in '{parent_name}': {reason}"
    )]
    MethodOverrideContractViolation {
        class_name: String,
        parent_name: String,
        method_name: String,
        reason: String,
    },

    #[error(
        "Type Error: Invalid binary operation '{op}' between '{left_type}' and '{right_type}'."
    )]
    InvalidBinaryOperation {
        op: String,
        left_type: String,
        right_type: String,
    },

    #[error(
        "Type Error: Invalid unary operation '{op}' on operand of type '{operand_type}'."
    )]
    InvalidUnaryOperation {
        op: String,
        operand_type: String,
    },

    #[error(
        "Type Error: Invalid type annotation '{type_name}': {reason}"
    )]
    InvalidTypeAnnotation {
        type_name: String,
        reason: String,
    },

    #[error(
        "Fatal Error: Cannot instantiate static facade class '{class_name}'. Use static methods directly."
    )]
    StaticFacadeInstantiationForbidden {
        class_name: String,
    },

    #[error(
        "Fatal Error: Cannot extend static facade class '{class_name}'. Use '{class_name}::macro()' to register custom methods dynamically."
    )]
    StaticFacadeInheritanceForbidden {
        class_name: String,
    },

    #[error(
        "Type Error: Cannot instantiate shared class '{shared_class}'. Shared instances are created by the container at boot."
    )]
    SharedInstantiationForbidden {
        shared_class: String,
    },

    #[error(
        "Type Error: Cannot extend shared class '{parent_name}' from non-shared class '{class_name}'."
    )]
    NonSharedExtendsSharedForbidden {
        class_name: String,
        parent_name: String,
    },

    #[error(
        "Type Error: Shared class '{shared_class}' cannot inherit mutable property '${property}' from non-shared class '{parent_class}'. All inherited properties must be readonly."
    )]
    SharedClassInheritedMutablePropertyForbidden {
        shared_class: String,
        parent_class: String,
        property: String,
    },

    #[error(
        "Type Error: Shared class '{shared_class}' cannot declare mutable property '${property}'. Shared classes must enforce deep immutability."
    )]
    SharedClassMutablePropertyForbidden {
        shared_class: String,
        property: String,
    },

    #[error(
        "Type Error: Shared class '{shared_class}' cannot inject request-scoped service '{service_type}' in constructor parameter '${param}'. Shared instances cannot capture request-scoped state."
    )]
    SharedClassCapturesScopedService {
        shared_class: String,
        param: String,
        service_type: String,
    },

    #[error(
        "Type Error: Shared class '{shared_class}' cannot store non-static closure in property '${property}'. Closures in shared fields must be static/pure to prevent request state leakage."
    )]
    SharedFieldClosureCaptureForbidden {
        shared_class: String,
        property: String,
    },

    #[error(
        "Type Error: Shared class '{shared_class}' cannot declare mutable static property '${property}'. Static properties in shared classes must be readonly or concurrency-safe primitives (Atomic, Pool)."
    )]
    SharedClassMutableStaticForbidden {
        shared_class: String,
        property: String,
    },

    #[error(
        "Type Error: Trait method '{trait_name}::{method_name}' has not been applied, because there are collisions with '{conflicting_trait}::{method_name}' on '{class_name}'. Use 'insteadof' to resolve the conflict."
    )]
    TraitMethodCollision {
        class_name: String,
        trait_name: String,
        conflicting_trait: String,
        method_name: String,
    },

    #[error(
        "Type Error: Trait adaptation error on '{class_name}': {reason}"
    )]
    TraitAdaptationError {
        class_name: String,
        reason: String,
    },

    #[error("Fatal Error: Cannot instantiate abstract class '{class_name}'")]
    CannotInstantiateAbstractClass { class_name: String },

    #[error("Fatal Error: Cannot instantiate interface '{interface_name}'")]
    CannotInstantiateInterface { interface_name: String },

    #[error("Fatal Error: Cannot instantiate trait '{trait_name}'")]
    CannotInstantiateTrait { trait_name: String },

    #[error("Fatal Error: Abstract method '{class_name}::{method_name}()' cannot be declared private")]
    AbstractMethodCannotBePrivate {
        class_name: String,
        method_name: String,
    },

    #[error("Fatal Error: Class '{class_name}' contains abstract method '{declaring_class}::{method_name}()' and must therefore be declared abstract or implement it")]
    UnimplementedAbstractMethod {
        class_name: String,
        method_name: String,
        declaring_class: String,
    },

    #[error("Fatal Error: Class '{class_name}' must implement abstract method '{trait_name}::{method_name}()'")]
    TraitAbstractMethodUnimplemented {
        class_name: String,
        trait_name: String,
        method_name: String,
    },

    #[error("Type Error: Redundant isset() check: operand of type '{operand_type}' is non-nullable and can never be null or missing.")]
    RedundantIssetCheck {
        operand_type: String,
    },

    #[error("Compile Error: Cannot unset object variable '{var_name}': removing objects via unset() is forbidden.")]
    CannotUnsetObject {
        var_name: String,
    },

    #[error("Compile Error: isset() cannot be used on bare variable '{var_name}'. In RHP, undefined variables are compile errors; restrict isset() to array access, property access, or nullable expressions.")]
    BareVariableIssetForbidden {
        var_name: String,
    },

    #[error(
        "Type Error: Cannot access property '{property_name}' on dict via '->'. Use bracket syntax '$x[\"{property_name}\"]' or '$x->get(\"{property_name}\")' instead."
    )]
    DictPropertyAccessForbidden {
        property_name: String,
    },

    #[error("TypeError: {message}")]
    Custom {
        message: String,
    },
}
