use crate::ast::{
    expr::{BinaryOp, Expr, ExprKind, Literal, UnaryOp},
    stmt::{ClassMemberKind, FunctionDecl, MethodDecl, Stmt, StmtKind},
    types::{CastType, TypeHint},
    visitor::{walk_expr, walk_stmt, AstVisitor},
    Program,
};
use crate::lint::autotype::classify_array_literal;
use crate::lint::reporter::{Issue, IssueLevel};
use crate::parser::parse;
use crate::type_system::{
    check_program,
    error::TypeErrorKind,
    types::{CollectionSubtype, RhpType},
};

/// Run typecheck on source code and collect all diagnostic issues.
pub fn run_typecheck(source: &str, file_name: &str) -> Vec<Issue> {
    let mut issues = Vec::new();

    // 1. Parse phase
    let program = match parse(source) {
        Ok(p) => p,
        Err(parse_err) => {
            issues.push(Issue::new(
                IssueLevel::Error,
                "parse-error",
                parse_err.to_string(),
                file_name,
                parse_err.span.line,
                parse_err.span.col,
                parse_err.span.start,
                parse_err.span.end,
            ));
            return issues;
        }
    };

    // 2. Static boundary signatures & typed declaration verification
    collect_declaration_diagnostics(&program.statements, file_name, &mut issues);

    // 3. Deprecated direct superglobals check ($_GET, $_POST, $_SESSION, $_COOKIE)
    collect_deprecated_superglobals(&program, file_name, &mut issues);

    // 4. Medium/Gradual Type Checker pass
    match check_program(&program) {
        Ok(_) => {}
        Err(type_err) => {
            let code = type_error_to_code(&type_err.kind);
            let err_msg = type_err.kind.to_string();
            let already_reported = issues.iter().any(|i| {
                (i.span.start == type_err.span.start && i.code == code)
                    || (i.code == code && (i.message == err_msg || i.message.ends_with(&err_msg)))
            });
            if !already_reported {
                issues.push(Issue::new(
                    IssueLevel::Error,
                    code,
                    err_msg,
                    file_name,
                    type_err.span.line,
                    type_err.span.col,
                    type_err.span.start,
                    type_err.span.end,
                ));
            }
        }
    }

    issues
}

/// Collect boundary signature issues and validate typed defaults across declarations.
fn collect_declaration_diagnostics(stmts: &[Stmt<'_>], file_name: &str, issues: &mut Vec<Issue>) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::VarDecl {
                var,
                type_hint: Some(hint),
                init: Some(init_expr),
            } => {
                // Check if the typed variable declaration has a mismatched initial value
                if let Err((expected, found)) = check_expr_assignable_to_hint(init_expr, hint, None) {
                    issues.push(Issue::new(
                        IssueLevel::Error,
                        "variable-type-mismatch",
                        format!(
                            "Type Error: Cannot assign value of type '{}' to typed variable '{}' of type '{}'.",
                            found, var.name, expected
                        ),
                        file_name,
                        init_expr.span.line,
                        init_expr.span.col,
                        init_expr.span.start,
                        init_expr.span.end,
                    ));
                }
            }
            StmtKind::FunctionDecl(func) => {
                validate_function_boundaries(func, "function", file_name, issues);
                if let Some(body) = &func.body {
                    collect_declaration_diagnostics(body, file_name, issues);
                }
            }
            StmtKind::ClassDecl(cls) => {
                for member in &cls.members {
                    match &member.kind {
                        ClassMemberKind::Property(prop) => {
                            if let (Some(hint), Some(default_val)) = (&prop.type_hint, &prop.default) {
                                if let Err((expected, found)) =
                                    check_expr_assignable_to_hint(default_val, hint, Some(&cls.name.name))
                                {
                                    issues.push(Issue::new(
                                        IssueLevel::Error,
                                        "property-default-type-mismatch",
                                        format!(
                                            "Type Error: Default value for property '{}' in class '{}' has incompatible type: expected '{}', found '{}'.",
                                            prop.name.name, cls.name.name, expected, found
                                        ),
                                        file_name,
                                        default_val.span.line,
                                        default_val.span.col,
                                        default_val.span.start,
                                        default_val.span.end,
                                    ));
                                }
                            }
                        }
                        ClassMemberKind::Method(method) => {
                            validate_method_boundaries(method, &cls.name.name, file_name, issues);
                            if let Some(body) = &method.function.body {
                                collect_declaration_diagnostics(body, file_name, issues);
                            }
                        }
                        _ => {}
                    }
                }
            }
            StmtKind::InterfaceDecl(iface) => {
                for method in &iface.methods {
                    validate_method_boundaries(method, &iface.name.name, file_name, issues);
                }
            }
            StmtKind::Namespace { body: Some(body), .. } => {
                collect_declaration_diagnostics(body, file_name, issues);
            }
            StmtKind::Block(body) => {
                collect_declaration_diagnostics(body, file_name, issues);
            }
            _ => {}
        }
    }
}

fn validate_function_boundaries(
    func: &FunctionDecl<'_>,
    callable_kind: &str,
    file_name: &str,
    issues: &mut Vec<Issue>,
) {
    for param in &func.params {
        if param.type_hint.is_none() {
            issues.push(Issue::new(
                IssueLevel::Error,
                "missing-parameter-type",
                format!(
                    "Type Error: Parameter '{}' in {} '{}' must declare an explicit type signature across boundaries.",
                    param.name.name, callable_kind, func.name.name
                ),
                file_name,
                param.span.line,
                param.span.col,
                param.span.start,
                param.span.end,
            ));
        } else if let (Some(hint), Some(default_val)) = (&param.type_hint, &param.default_value) {
            // Check if existing written type is wrong against the default parameter
            if let Err((expected, found)) = check_expr_assignable_to_hint(default_val, hint, None) {
                issues.push(Issue::new(
                    IssueLevel::Error,
                    "parameter-default-type-mismatch",
                    format!(
                        "Type Error: Default value for parameter '{}' in {} '{}' has incompatible type: expected '{}', found '{}'.",
                        param.name.name, callable_kind, func.name.name, expected, found
                    ),
                    file_name,
                    default_val.span.line,
                    default_val.span.col,
                    default_val.span.start,
                    default_val.span.end,
                ));
            }
        }
    }

    if func.return_type.is_none() {
        issues.push(Issue::new(
            IssueLevel::Error,
            "missing-return-type",
            format!(
                "Type Error: {} '{}' must declare an explicit return type signature across boundaries.",
                callable_kind, func.name.name
            ),
            file_name,
            func.name.span.line,
            func.name.span.col,
            func.name.span.start,
            func.name.span.end,
        ));
    }
}

fn validate_method_boundaries(
    method: &MethodDecl<'_>,
    class_name: &str,
    file_name: &str,
    issues: &mut Vec<Issue>,
) {
    let is_constructor = method.function.name.name == "__construct";
    let is_destructor = method.function.name.name == "__destruct";
    let display_name = format!("{}::{}", class_name, method.function.name.name);

    for param in &method.function.params {
        if param.type_hint.is_none() {
            issues.push(Issue::new(
                IssueLevel::Error,
                "missing-parameter-type",
                format!(
                    "Type Error: Parameter '{}' in method '{}' must declare an explicit type signature across boundaries.",
                    param.name.name, display_name
                ),
                file_name,
                param.span.line,
                param.span.col,
                param.span.start,
                param.span.end,
            ));
        } else if let (Some(hint), Some(default_val)) = (&param.type_hint, &param.default_value) {
            // Check if existing written type is wrong against the default parameter
            if let Err((expected, found)) = check_expr_assignable_to_hint(default_val, hint, Some(class_name)) {
                issues.push(Issue::new(
                    IssueLevel::Error,
                    "parameter-default-type-mismatch",
                    format!(
                        "Type Error: Default value for parameter '{}' in method '{}' has incompatible type: expected '{}', found '{}'.",
                        param.name.name, display_name, expected, found
                    ),
                    file_name,
                    default_val.span.line,
                    default_val.span.col,
                    default_val.span.start,
                    default_val.span.end,
                ));
            }
        }
    }

    if is_constructor || is_destructor {
        if let Some(ref ret) = method.function.return_type {
            if ret.simple_name() != Some("void") {
                issues.push(Issue::new(
                    IssueLevel::Error,
                    "constructor-return-type-forbidden",
                    format!(
                        "Type Error: {} in class '{}' cannot declare a return type.",
                        method.function.name.name, class_name
                    ),
                    file_name,
                    ret.span.line,
                    ret.span.col,
                    ret.span.start,
                    ret.span.end,
                ));
            }
        }
    } else if method.function.return_type.is_none() {
        issues.push(Issue::new(
            IssueLevel::Error,
            "missing-return-type",
            format!(
                "Type Error: Method '{}' must declare an explicit return type signature across boundaries.",
                display_name
            ),
            file_name,
            method.function.name.span.line,
            method.function.name.span.col,
            method.function.name.span.start,
            method.function.name.span.end,
        ));
    }
}

/// Check if expression is assignable to a type hint, returning (expected, found) if incompatible.
fn check_expr_assignable_to_hint(
    expr: &Expr<'_>,
    hint: &TypeHint<'_>,
    current_class: Option<&str>,
) -> Result<(), (String, String)> {
    let target_ty = match RhpType::from_type_hint(hint, current_class) {
        Ok(t) => t,
        Err(_) => return Ok(()),
    };
    let expr_ty = infer_expr_rhp_type(expr);
    if expr_ty == RhpType::Dynamic || expr_ty == RhpType::Mixed {
        return Ok(());
    }
    if !expr_ty.is_assignable_to(&target_ty, None) {
        return Err((target_ty.to_string(), expr_ty.to_string()));
    }
    Ok(())
}

/// Infer semantic RhpType directly from an AST expression.
pub fn infer_expr_rhp_type(expr: &Expr<'_>) -> RhpType {
    match &expr.kind {
        ExprKind::Literal(lit) => match lit {
            Literal::Int { .. } => RhpType::Int,
            Literal::Float { .. } => RhpType::Float,
            Literal::String { .. } | Literal::InterpolatedString { .. } => RhpType::String,
            Literal::Bool(_) => RhpType::Bool,
            Literal::Null => RhpType::Null,
        },
        ExprKind::Array(elements) => {
            if elements.is_empty() {
                RhpType::Array(Box::new(RhpType::Mixed))
            } else {
                let classification = classify_array_literal(elements);
                match classification.as_deref() {
                    Some("list") => RhpType::Collection(CollectionSubtype::List(Box::new(RhpType::Mixed))),
                    Some("dict") => RhpType::Collection(CollectionSubtype::Dict(Box::new(RhpType::Mixed))),
                    Some("hashMap") | Some("hashmap") => {
                        RhpType::Collection(CollectionSubtype::HashMap(
                            Box::new(RhpType::Mixed),
                            Box::new(RhpType::Mixed),
                        ))
                    }
                    _ => RhpType::Array(Box::new(RhpType::Mixed)),
                }
            }
        }
        ExprKind::StaticCall { target, method, .. } => {
            if let ExprKind::Ident(target_id) = &target.kind {
                match target_id.name {
                    "Arr" => match method.name {
                        "list" => RhpType::Collection(CollectionSubtype::List(Box::new(RhpType::Mixed))),
                        "dict" => RhpType::Collection(CollectionSubtype::Dict(Box::new(RhpType::Mixed))),
                        "set" => RhpType::Collection(CollectionSubtype::Set(Box::new(RhpType::Mixed))),
                        "map" | "hashMap" => RhpType::Collection(CollectionSubtype::HashMap(
                            Box::new(RhpType::Mixed),
                            Box::new(RhpType::Mixed),
                        )),
                        _ => RhpType::Array(Box::new(RhpType::Mixed)),
                    },
                    "Str" => match method.name {
                        "contains" | "includes" | "startsWith" | "endsWith" | "testRegex" | "regex" | "is" | "isUuid" | "isUlid" => RhpType::Bool,
                        "length" | "indexOf" => RhpType::Int,
                        _ => RhpType::String,
                    },
                    "Regex" => match method.name {
                        "isValid" | "testString" | "matches" | "isMatch" => RhpType::Bool,
                        _ => RhpType::String,
                    },
                    "Crypt" => match method.name {
                        "hashAlgos" | "opensslCipherMethods" => RhpType::Collection(CollectionSubtype::List(Box::new(RhpType::String))),
                        _ => RhpType::String,
                    },
                    "Html" => RhpType::String,
                    "Url" | "URL" => match method.name {
                        "parseUrl" => RhpType::Collection(CollectionSubtype::Dict(Box::new(RhpType::Mixed))),
                        _ => RhpType::String,
                    },
                    "Int" => RhpType::Int,
                    "Float" => RhpType::Float,
                    _ => RhpType::Dynamic,
                }
            } else {
                RhpType::Dynamic
            }
        }
        ExprKind::Binary { op, left, right } => match op {
            BinaryOp::Concat => RhpType::String,
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Identical
            | BinaryOp::NotIdentical
            | BinaryOp::Lt
            | BinaryOp::LtEq
            | BinaryOp::Gt
            | BinaryOp::GtEq
            | BinaryOp::Spaceship
            | BinaryOp::LogicalAnd
            | BinaryOp::LogicalOr
            | BinaryOp::WordAnd
            | BinaryOp::WordOr
            | BinaryOp::WordXor => RhpType::Bool,
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                let left_ty = infer_expr_rhp_type(left);
                let right_ty = infer_expr_rhp_type(right);
                if matches!(left_ty, RhpType::Float) || matches!(right_ty, RhpType::Float) {
                    RhpType::Float
                } else {
                    RhpType::Int
                }
            }
            BinaryOp::Div => RhpType::Float,
            BinaryOp::Mod => RhpType::Int,
            _ => RhpType::Dynamic,
        },
        ExprKind::Unary { op, .. } => match op {
            UnaryOp::Not => RhpType::Bool,
            UnaryOp::PreInc | UnaryOp::PostInc | UnaryOp::PreDec | UnaryOp::PostDec => RhpType::Int,
            _ => RhpType::Dynamic,
        },
        ExprKind::Cast { kind, .. } => match kind {
            CastType::Int => RhpType::Int,
            CastType::Float => RhpType::Float,
            CastType::String => RhpType::String,
            CastType::Bool => RhpType::Bool,
            CastType::Array => RhpType::Array(Box::new(RhpType::Mixed)),
            CastType::Object => RhpType::Object,
        },
        ExprKind::New { class, .. } => {
            if let ExprKind::Ident(ident) = &class.kind {
                RhpType::Class(ident.name.to_string())
            } else {
                RhpType::Object
            }
        }
        ExprKind::Grouping(inner) => infer_expr_rhp_type(inner),
        _ => RhpType::Dynamic,
    }
}

pub fn type_error_to_code(kind: &TypeErrorKind) -> &'static str {
    match kind {
        TypeErrorKind::MissingParameterType { .. } => "missing-parameter-type",
        TypeErrorKind::MissingReturnType { .. } => "missing-return-type",
        TypeErrorKind::PassByReferenceForbidden { .. } => "pass-by-reference-forbidden",
        TypeErrorKind::MissingReferenceArgument { .. } => "missing-reference-argument",
        TypeErrorKind::UnexpectedReferenceArgument { .. } => "unexpected-reference-argument",
        TypeErrorKind::ReferenceArgumentMustBeVariable { .. } => "reference-argument-must-be-variable",
        TypeErrorKind::ClosureReferenceCaptureForbidden { .. } => "closure-reference-capture-forbidden",
        TypeErrorKind::CyclicClosureCaptureForbidden { .. } => "cyclic-closure-capture-forbidden",
        TypeErrorKind::CyclicReferenceRisk { .. } => "cyclic-reference-risk",
        TypeErrorKind::ConstructorReturnTypeForbidden { .. } => "constructor-return-type-forbidden",
        TypeErrorKind::ReturnTypeMismatch { .. } => "return-type-mismatch",
        TypeErrorKind::VoidReturnWithValue { .. } => "void-return-with-value",
        TypeErrorKind::NeverFunctionReturns { .. } => "never-function-returns",
        TypeErrorKind::MissingReturnStatement { .. } => "missing-return-statement",
        TypeErrorKind::ArgumentTypeMismatch { .. } => "argument-type-mismatch",
        TypeErrorKind::ArityMismatch { .. } => "arity-mismatch",
        TypeErrorKind::UnknownNamedArgument { .. } => "unknown-named-argument",
        TypeErrorKind::VariableTypeMismatch { .. } => "variable-type-mismatch",
        TypeErrorKind::ParameterDefaultTypeMismatch { .. } => "parameter-default-type-mismatch",
        TypeErrorKind::PropertyDefaultTypeMismatch { .. } => "property-default-type-mismatch",
        TypeErrorKind::PropertyAssignmentTypeMismatch { .. } => "property-assignment-type-mismatch",
        TypeErrorKind::ReadonlyPropertyMutation { .. } => "readonly-property-mutation",
        TypeErrorKind::InterfaceContractViolation { .. } => "interface-contract-violation",
        TypeErrorKind::MethodOverrideContractViolation { .. } => "method-override-contract-violation",
        TypeErrorKind::InvalidBinaryOperation { .. } => "invalid-binary-operation",
        TypeErrorKind::InvalidUnaryOperation { .. } => "invalid-unary-operation",
        TypeErrorKind::InvalidTypeAnnotation { .. } => "invalid-type-annotation",
        TypeErrorKind::StaticFacadeInstantiationForbidden { .. } => "static-facade-instantiation-forbidden",
        TypeErrorKind::StaticFacadeInheritanceForbidden { .. } => "static-facade-inheritance-forbidden",
        TypeErrorKind::NonStaticSingletonMethodCallForbidden { .. } => "non-static-singleton-method-call-forbidden",
        TypeErrorKind::SingletonInstantiationForbidden { .. } => "singleton-instantiation-forbidden",
        TypeErrorKind::NonSingletonExtendsSingletonForbidden { .. } => "non-singleton-extends-singleton-forbidden",
        TypeErrorKind::TraitMethodCollision { .. } => "trait-method-collision",
        TypeErrorKind::TraitAdaptationError { .. } => "trait-adaptation-error",
    }
}

struct DeprecatedSuperglobalsVisitor<'a> {
    file_name: &'a str,
    issues: &'a mut Vec<Issue>,
}

impl<'s, 'a> AstVisitor<'s> for DeprecatedSuperglobalsVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &Stmt<'s>) {
        if let StmtKind::Global(vars) = &stmt.kind {
            for var in vars {
                if let Some(msg) = deprecated_superglobal_message(var.name) {
                    self.issues.push(Issue::new(
                        IssueLevel::Warning,
                        "deprecated-superglobal",
                        msg,
                        self.file_name,
                        var.span.line,
                        var.span.col,
                        var.span.start,
                        var.span.end,
                    ));
                }
            }
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &Expr<'s>) {
        if let ExprKind::Variable(var) = &expr.kind {
            if let Some(msg) = deprecated_superglobal_message(var.name) {
                self.issues.push(Issue::new(
                    IssueLevel::Warning,
                    "deprecated-superglobal",
                    msg,
                    self.file_name,
                    var.span.line,
                    var.span.col,
                    var.span.start,
                    var.span.end,
                ));
            }
        }
        walk_expr(self, expr);
    }
}

fn deprecated_superglobal_message(name: &str) -> Option<String> {
    match name {
        "$_GET" | "_GET" => Some("Direct use of '$_GET' is discouraged in RHP. Use 'Request::get()' instead.".to_string()),
        "$_POST" | "_POST" => Some("Direct use of '$_POST' is discouraged in RHP. Use 'Request::get()' instead.".to_string()),
        "$_SESSION" | "_SESSION" => Some("Direct use of '$_SESSION' is discouraged in RHP. Use 'Session::get()' / 'Session::set()' instead.".to_string()),
        "$_COOKIE" | "_COOKIE" => Some("Direct use of '$_COOKIE' is discouraged in RHP. Use 'Cookie::get()' / 'Cookie::set()' instead.".to_string()),
        _ => None,
    }
}

fn collect_deprecated_superglobals<'s>(
    program: &Program<'s>,
    file_name: &str,
    issues: &mut Vec<Issue>,
) {
    let mut visitor = DeprecatedSuperglobalsVisitor { file_name, issues };
    visitor.visit_program(program);
}

