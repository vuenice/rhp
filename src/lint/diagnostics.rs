use std::collections::HashSet;

use crate::ast::{
    expr::{AssignOp, BinaryOp, Expr, ExprKind, Literal, UnaryOp},
    stmt::{ClassMemberKind, FunctionDecl, MethodDecl, Param, Stmt, StmtKind},
    types::{CastType, TypeHint, TypeHintKind},
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
            let code = match &parse_err.kind {
                crate::parser::error::ParseErrorKind::UnsupportedOperator { .. } => "unsupported-operator",
                crate::parser::error::ParseErrorKind::DynamicIncludeForbidden { .. } => "dynamic-include-forbidden",
                crate::parser::error::ParseErrorKind::DeclareDirectiveForbidden => "declare-directive-forbidden",
                _ => "parse-error",
            };
            issues.push(Issue::new(
                IssueLevel::Error,
                code,
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
    collect_redundant_void_returns(&program, file_name, &mut issues);

    // 3. Deprecated direct superglobals check ($_GET, $_POST, $_SESSION, $_COOKIE)
    collect_deprecated_superglobals(&program, file_name, &mut issues);

    // 3b. Division stored in an int may be a float at runtime.
    collect_division_stored_in_int(&program, file_name, &mut issues);

    // 4. Medium/Gradual Type Checker pass
    match check_program(&program) {
        Ok(result) => {
            for warning in result.warnings {
                issues.push(Issue::new(
                    IssueLevel::Warning,
                    warning.code,
                    warning.message,
                    file_name,
                    warning.span.line,
                    warning.span.col,
                    warning.span.start,
                    warning.span.end,
                ));
            }
        }
        Err(type_err) => {
            // The failing check drops `TypeCheckResult`, including for-loop warnings.
            collect_c_style_for_warnings(&program, file_name, &mut issues);
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

    // 5. Undetermined variable type diagnostics (e.g. untyped `$z = $x['key']` where $x is a dict)
    let type_info = crate::type_system::checker::TypeChecker::new().check_program(&program).ok();
    collect_undetermined_variable_diagnostics(&program.statements, type_info.as_ref(), file_name, &mut issues);

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
                    if !float_division_into_int(hint, init_expr, &found) {
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
                                    if !float_division_into_int(hint, default_val, &found) {
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
    let mut has_seen_optional = false;
    let mut first_opt: Option<&crate::ast::stmt::Param> = None;
    for param in &func.params {
        if param.default_value.is_some() {
            has_seen_optional = true;
            if first_opt.is_none() {
                first_opt = Some(param);
            }
        } else if !param.is_variadic && has_seen_optional {
            if let Some(opt) = first_opt {
                issues.push(Issue::new(
                    IssueLevel::Error,
                    "required-param-after-optional",
                    format!(
                        "Type Error: Required parameter '{}' cannot follow optional parameter '{}' in {} '{}'.",
                        param.name.name, opt.name.name, callable_kind, func.name.name
                    ),
                    file_name,
                    param.span.line,
                    param.span.col,
                    param.span.start,
                    param.span.end,
                ));
            }
        }

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
                if !float_division_into_int(hint, default_val, &found) {
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
    let mut has_seen_optional = false;
    let mut first_opt: Option<&crate::ast::stmt::Param> = None;
    for param in &method.function.params {
        if param.default_value.is_some() {
            has_seen_optional = true;
            if first_opt.is_none() {
                first_opt = Some(param);
            }
        } else if !param.is_variadic && has_seen_optional {
            if let Some(opt) = first_opt {
                issues.push(Issue::new(
                    IssueLevel::Error,
                    "required-param-after-optional",
                    format!(
                        "Type Error: Required parameter '{}' cannot follow optional parameter '{}' in method '{}'.",
                        param.name.name, opt.name.name, display_name
                    ),
                    file_name,
                    param.span.line,
                    param.span.col,
                    param.span.start,
                    param.span.end,
                ));
            }
        }

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
                if !float_division_into_int(hint, default_val, &found) {
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
    }

    if is_constructor || is_destructor {
        if let Some(ref ret) = method.function.return_type {
            if !is_explicit_void_return(ret) {
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
    }
}

const DIVISION_STORED_IN_INT: &str =
    "The division operation might result into float value instead of int.";

fn hint_is_int(hint: &TypeHint<'_>) -> bool {
    hint.simple_name() == Some("int")
}

/// True when `/` or `/=` feeds this expression and is not already wrapped in `int(...)`.
fn expr_stores_division(expr: &Expr<'_>) -> bool {
    struct Finder {
        found: bool,
    }

    impl<'s> AstVisitor<'s> for Finder {
        fn visit_expr(&mut self, expr: &Expr<'s>) {
            if self.found {
                return;
            }
            match &expr.kind {
                ExprKind::Cast { kind: CastType::Int, .. }
                | ExprKind::Closure { .. }
                | ExprKind::ArrowFunction { .. }
                | ExprKind::AnonymousClass { .. } => {}
                ExprKind::Binary { op: BinaryOp::Div, .. }
                | ExprKind::Assign { op: AssignOp::DivAssign, .. } => {
                    self.found = true;
                }
                _ => walk_expr(self, expr),
            }
        }
    }

    let mut finder = Finder { found: false };
    finder.visit_expr(expr);
    finder.found
}

/// The linter types `/` as float. That mismatch is reported as a warning, not a type error.
fn float_division_into_int(hint: &TypeHint<'_>, expr: &Expr<'_>, found: &str) -> bool {
    found == "float" && hint_is_int(hint) && expr_stores_division(expr)
}

fn collect_division_stored_in_int(program: &Program<'_>, file_name: &str, issues: &mut Vec<Issue>) {
    let mut visitor = DivisionIntoIntVisitor {
        file_name,
        issues,
        scopes: vec![HashSet::new()],
    };
    visitor.visit_program(program);
}

struct DivisionIntoIntVisitor<'a> {
    file_name: &'a str,
    issues: &'a mut Vec<Issue>,
    scopes: Vec<HashSet<String>>,
}

impl<'a> DivisionIntoIntVisitor<'a> {
    fn declare_int(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    fn is_int_var(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|scope| scope.contains(name))
    }

    fn warn(&mut self, span: crate::lexer::span::Span) {
        self.issues.push(Issue::new(
            IssueLevel::Warning,
            "division-stored-in-int",
            DIVISION_STORED_IN_INT,
            self.file_name,
            span.line,
            span.col,
            span.start,
            span.end,
        ));
    }

    fn enter_function<'b>(&mut self, params: &[Param<'b>], body: Option<&[Stmt<'b>]>) {
        self.scopes.push(HashSet::new());
        for param in params {
            if param.type_hint.as_ref().is_some_and(hint_is_int) {
                self.declare_int(param.name.name);
            }
        }
        for param in params {
            if let Some(default_val) = &param.default_value {
                if param.type_hint.as_ref().is_some_and(hint_is_int) && expr_stores_division(default_val) {
                    self.warn(default_val.span);
                }
                self.visit_expr(default_val);
            }
        }
        if let Some(body) = body {
            for stmt in body {
                self.visit_stmt(stmt);
            }
        }
        self.scopes.pop();
    }

    fn warn_int_assignment(&mut self, op: &AssignOp, target: &Expr<'_>, value: &Expr<'_>, span: crate::lexer::span::Span) {
        let ExprKind::Variable(var) = &target.kind else {
            return;
        };
        if !self.is_int_var(var.name) {
            return;
        }
        if *op == AssignOp::DivAssign || expr_stores_division(value) {
            self.warn(span);
        }
    }
}

impl<'a, 's> AstVisitor<'s> for DivisionIntoIntVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &Stmt<'s>) {
        match &stmt.kind {
            StmtKind::VarDecl { var, type_hint, init } => {
                if let Some(hint) = type_hint {
                    if hint_is_int(hint) {
                        if let Some(init_expr) = init {
                            if expr_stores_division(init_expr) {
                                self.warn(init_expr.span);
                            }
                        }
                        self.declare_int(var.name);
                    }
                }
                walk_stmt(self, stmt);
            }
            StmtKind::FunctionDecl(func) => {
                self.enter_function(&func.params, func.body.as_deref());
            }
            StmtKind::ClassDecl(cls) => {
                for member in &cls.members {
                    match &member.kind {
                        ClassMemberKind::Property(prop) => {
                            if let (Some(hint), Some(default_val)) = (&prop.type_hint, &prop.default) {
                                if hint_is_int(hint) && expr_stores_division(default_val) {
                                    self.warn(default_val.span);
                                }
                            }
                            if let Some(default_val) = &prop.default {
                                self.visit_expr(default_val);
                            }
                        }
                        ClassMemberKind::Method(method) => {
                            self.enter_function(&method.function.params, method.function.body.as_deref());
                        }
                        ClassMemberKind::Constant(co) => {
                            self.visit_expr(&co.value);
                        }
                        ClassMemberKind::TraitUse(_) => {}
                    }
                }
            }
            StmtKind::TraitDecl(trait_decl) => {
                for member in &trait_decl.members {
                    if let ClassMemberKind::Method(method) = &member.kind {
                        self.enter_function(&method.function.params, method.function.body.as_deref());
                    }
                }
            }
            StmtKind::EnumDecl(enum_decl) => {
                for method in &enum_decl.methods {
                    self.enter_function(&method.function.params, method.function.body.as_deref());
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &Expr<'s>) {
        match &expr.kind {
            ExprKind::Closure { params, body, .. } => {
                self.enter_function(params, Some(body.as_slice()));
            }
            ExprKind::Assign { op, target, value } => {
                self.warn_int_assignment(op, target, value, expr.span);
                walk_expr(self, expr);
            }
            _ => walk_expr(self, expr),
        }
    }
}

/// A written return type is void only when it is the named type `void` / `Void`.
fn is_explicit_void_return(hint: &TypeHint<'_>) -> bool {
    matches!(&hint.kind, TypeHintKind::Named(ident) if ident.name.eq_ignore_ascii_case("void"))
}

fn push_redundant_void_return(hint: &TypeHint<'_>, file_name: &str, issues: &mut Vec<Issue>) {
    issues.push(Issue::new(
        IssueLevel::Error,
        "redundant-void-return",
        "no need to mention return type for void",
        file_name,
        hint.span.line,
        hint.span.col,
        hint.span.start,
        hint.span.end,
    ));
}

fn note_function_void_return(func: &FunctionDecl<'_>, file_name: &str, issues: &mut Vec<Issue>) {
    if let Some(ret) = &func.return_type {
        if is_explicit_void_return(ret) {
            push_redundant_void_return(ret, file_name, issues);
        }
    }
}

fn collect_redundant_void_returns(program: &Program<'_>, file_name: &str, issues: &mut Vec<Issue>) {
    let mut visitor = RedundantVoidReturnVisitor { file_name, issues };
    for stmt in &program.statements {
        visitor.visit_stmt(stmt);
    }
}

struct RedundantVoidReturnVisitor<'a> {
    file_name: &'a str,
    issues: &'a mut Vec<Issue>,
}

impl<'a> RedundantVoidReturnVisitor<'a> {
    fn note_function(&mut self, func: &FunctionDecl<'_>) {
        note_function_void_return(func, self.file_name, self.issues);
    }
}

impl<'s> AstVisitor<'s> for RedundantVoidReturnVisitor<'_> {
    fn visit_stmt(&mut self, stmt: &Stmt<'s>) {
        match &stmt.kind {
            StmtKind::FunctionDecl(func) => self.note_function(func),
            StmtKind::ClassDecl(cls) => {
                for member in &cls.members {
                    if let ClassMemberKind::Method(method) = &member.kind {
                        self.note_function(&method.function);
                    }
                }
            }
            StmtKind::InterfaceDecl(iface) => {
                for method in &iface.methods {
                    self.note_function(&method.function);
                }
            }
            StmtKind::TraitDecl(tr) => {
                for member in &tr.members {
                    if let ClassMemberKind::Method(method) = &member.kind {
                        self.note_function(&method.function);
                    }
                }
            }
            StmtKind::EnumDecl(enm) => {
                for method in &enm.methods {
                    self.note_function(&method.function);
                }
            }
            _ => {}
        }
        walk_stmt(self, stmt);
        if let StmtKind::InterfaceDecl(iface) = &stmt.kind {
            for method in &iface.methods {
                if let Some(body) = &method.function.body {
                    for inner in body {
                        self.visit_stmt(inner);
                    }
                }
            }
        }
    }

    fn visit_expr(&mut self, expr: &Expr<'s>) {
        match &expr.kind {
            ExprKind::ArrowFunction { return_type, .. } | ExprKind::Closure { return_type, .. } => {
                if let Some(ret) = return_type {
                    if is_explicit_void_return(ret) {
                        push_redundant_void_return(ret, self.file_name, self.issues);
                    }
                }
            }
            ExprKind::AnonymousClass { class_decl, .. } => {
                for member in &class_decl.members {
                    if let ClassMemberKind::Method(method) = &member.kind {
                        self.note_function(&method.function);
                    }
                }
            }
            _ => {}
        }
        walk_expr(self, expr);
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
            Literal::Bytes { .. } => RhpType::Bytes,
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
                        "check" | "compare" | "digit" | "isDigit" | "alpha" | "isAlpha" | "alnum" | "isAlnum" | "contains" | "startsWith" | "endsWith" | "testRegex" | "regex" | "is" | "isUuid" | "isUlid" => RhpType::Bool,
                        "length" | "len" | "size" | "mbStrlen" | "mb_strlen" | "graphemeLen" | "grapheme_len" | "countOccurrences"
                        | "strcmp" | "cmp" | "compareBinary" | "binaryCompare"
                        | "strcasecmp" | "casecmp" | "caseCmp" | "caseCompare" | "compareCase"
                        | "strncmp" | "ncmp" | "nCmp" | "compareN" | "compareBinaryN"
                        | "strncasecmp" | "ncasecmp" | "nCaseCmp" | "caseNCmp" | "caseCompareN" => RhpType::Int,
                        "indexOf" | "lastIndexOf" => RhpType::Nullable(Box::new(RhpType::Int)),
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
                    "Duration" => match method.name {
                        "spec" | "format" | "forHumans" | "toString" | "toIso8601String" | "toDateInterval" => RhpType::String,
                        "total" | "totalYears" | "totalMonths" | "totalWeeks" | "totalDays" | "totalHours"
                        | "totalMinutes" | "totalSeconds" | "totalMilliseconds" | "totalMicroseconds" => RhpType::Float,
                        "compare" => RhpType::Int,
                        "eq" | "equalTo" | "gt" | "lt" | "gte" | "lte" | "isEmpty" | "isNegative" | "isPositive" => RhpType::Bool,
                        _ => RhpType::Class("Duration".to_string()),
                    },
                    "Time" => match method.name {
                        "toDateTimeString" | "toDateString" | "toDatString" | "toTimeString"
                        | "toISOString" | "toIso8601String" | "toAtomString" | "toRfc2822String"
                        | "toFormattedDateString" | "toDayDateTimeString" | "format" | "diffForHumans" | "date" => RhpType::String,
                        "getTimestamp" | "timestamp" | "year" | "month" | "day" | "hour" | "minute" | "second" => RhpType::Int,
                        "isToday" | "isYesterday" | "isTomorrow" | "isFuture" | "isPast" | "isWeekend" | "isWeekday" | "isLeapYear" => RhpType::Bool,
                        _ => RhpType::Class("Time".to_string()),
                    },
                    "Url" | "URL" => match method.name {
                        "parseUrl" | "parseQuery" => RhpType::Collection(CollectionSubtype::Dict(Box::new(RhpType::Mixed))),
                        _ => RhpType::String,
                    },
                    "Ip" => match method.name {
                        "toLong" => RhpType::Int,
                        _ => RhpType::String,
                    },
                    "Compress" => RhpType::String,
                    "Uniq" => match method.name {
                        "isValid" | "is_valid" => RhpType::Bool,
                        "version" => RhpType::Union(vec![RhpType::Int, RhpType::Null]),
                        "parse" => RhpType::Collection(CollectionSubtype::Dict(Box::new(RhpType::Mixed))),
                        _ => RhpType::String,
                    },
                    "Csv" => match method.name {
                        "generate" | "formatRow" | "stringify" => RhpType::String,
                        "close" | "feof" | "eof" | "rewind" | "writeFile" => RhpType::Bool,
                        "parse" | "readFile" => RhpType::Collection(CollectionSubtype::List(Box::new(
                            RhpType::Collection(CollectionSubtype::List(Box::new(RhpType::String))),
                        ))),
                        "parseRow" | "str_getcsv" => RhpType::Collection(CollectionSubtype::List(Box::new(RhpType::String))),
                        _ => RhpType::Dynamic,
                    },
                    "Sys" => match method.name {
                        "os" | "version" => RhpType::String,
                        "timestamp" => RhpType::Int,
                        "disk_free_space" | "diskFreeSpace" | "diskfreespace" | "disk_total_space" | "diskTotalSpace" => RhpType::Union(vec![RhpType::Float, RhpType::Bool]),
                        "print" | "echo" | "dd" | "dump" | "clearstatcache" | "clearStatCache" => RhpType::Void,
                        _ => RhpType::Dynamic,
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
            BinaryOp::Add => {
                let left_ty = infer_expr_rhp_type(left);
                let right_ty = infer_expr_rhp_type(right);
                if matches!(left_ty, RhpType::String) || matches!(right_ty, RhpType::String) {
                    RhpType::String
                } else if matches!(left_ty, RhpType::Float) || matches!(right_ty, RhpType::Float) {
                    RhpType::Float
                } else {
                    RhpType::Int
                }
            }
            BinaryOp::Sub | BinaryOp::Mul => {
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
            CastType::Bytes => RhpType::Bytes,
            CastType::Rune => RhpType::Rune,
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
        ExprKind::AnonymousClass { class_decl, .. } => {
            RhpType::Class(class_decl.name.name.to_string())
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
        TypeErrorKind::RequiredParameterAfterOptional { .. } => "required-param-after-optional",
        TypeErrorKind::CannotSkipRequiredParameter { .. } => "cannot-skip-required-parameter",
        TypeErrorKind::VoidAssignment { .. } => "void-assignment",
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
        TypeErrorKind::SharedInstantiationForbidden { .. } => "shared-instantiation-forbidden",
        TypeErrorKind::NonSharedExtendsSharedForbidden { .. } => "non-shared-extends-shared-forbidden",
        TypeErrorKind::SharedClassInheritedMutablePropertyForbidden { .. } => "shared-class-inherited-mutable-property",
        TypeErrorKind::SharedClassMutablePropertyForbidden { .. } => "shared-class-mutable-property",
        TypeErrorKind::SharedClassCapturesScopedService { .. } => "shared-class-captures-scoped-service",
        TypeErrorKind::SharedFieldClosureCaptureForbidden { .. } => "shared-field-closure-capture-forbidden",
        TypeErrorKind::SharedClassMutableStaticForbidden { .. } => "shared-class-mutable-static-forbidden",
        TypeErrorKind::TraitMethodCollision { .. } => "trait-method-collision",
        TypeErrorKind::TraitAdaptationError { .. } => "trait-adaptation-error",
        TypeErrorKind::CannotInstantiateAbstractClass { .. } => "cannot-instantiate-abstract-class",
        TypeErrorKind::CannotInstantiateInterface { .. } => "cannot-instantiate-interface",
        TypeErrorKind::CannotInstantiateTrait { .. } => "cannot-instantiate-trait",
        TypeErrorKind::AbstractMethodCannotBePrivate { .. } => "abstract-method-cannot-be-private",
        TypeErrorKind::UnimplementedAbstractMethod { .. } => "unimplemented-abstract-method",
        TypeErrorKind::TraitAbstractMethodUnimplemented { .. } => "trait-abstract-method-unimplemented",
        TypeErrorKind::RedundantIssetCheck { .. } => "redundant-isset-check",
        TypeErrorKind::CannotUnsetObject { .. } => "cannot-unset-object",
        TypeErrorKind::BareVariableIssetForbidden { .. } => "bare-variable-isset-forbidden",
        TypeErrorKind::DictPropertyAccessForbidden { .. } => "dict-property-access-forbidden",
        TypeErrorKind::Custom { .. } => "type-error",
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

fn collect_c_style_for_warnings<'s>(
    program: &Program<'s>,
    file_name: &str,
    issues: &mut Vec<Issue>,
) {
    let mut visitor = CStyleForVisitor { file_name, issues };
    visitor.visit_program(program);
}

struct CStyleForVisitor<'a> {
    file_name: &'a str,
    issues: &'a mut Vec<Issue>,
}

impl<'s, 'a> AstVisitor<'s> for CStyleForVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &Stmt<'s>) {
        if matches!(stmt.kind, StmtKind::For { .. }) {
            self.issues.push(Issue::new(
                IssueLevel::Warning,
                "deprecated-c-style-for",
                crate::type_system::checker::C_STYLE_FOR_DEPRECATION,
                self.file_name,
                stmt.span.line,
                stmt.span.col,
                stmt.span.start,
                stmt.span.end,
            ));
        }
        walk_stmt(self, stmt);
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

fn is_expr_dict(
    expr: &Expr<'_>,
    type_info: Option<&crate::type_system::TypeCheckResult>,
    dict_vars: &std::collections::HashSet<String>,
) -> bool {
    match &expr.kind {
        ExprKind::Variable(var) => dict_vars.contains(var.name),
        ExprKind::Array(elements) => {
            classify_array_literal(elements).as_deref() == Some("dict")
        }
        ExprKind::StaticCall { target, method, .. } => {
            if let ExprKind::Ident(target_id) = &target.kind {
                if target_id.name == "Arr" && method.name == "dict" {
                    return true;
                }
                if (target_id.name == "Url" || target_id.name == "URL") && method.name == "parseUrl" {
                    return true;
                }
            }
            false
        }
        _ => {
            if let Some(info) = type_info {
                if let Some(rhp_ty) = info.expr_types.get(&expr.span) {
                    return matches!(rhp_ty, RhpType::Collection(CollectionSubtype::Dict(_)));
                }
            }
            matches!(infer_expr_rhp_type(expr), RhpType::Collection(CollectionSubtype::Dict(_)))
        }
    }
}

fn is_dict_element_access(
    expr: &Expr<'_>,
    type_info: Option<&crate::type_system::TypeCheckResult>,
    dict_vars: &std::collections::HashSet<String>,
) -> bool {
    match &expr.kind {
        ExprKind::ArrayIndex { array, .. } => {
            is_expr_dict(array, type_info, dict_vars)
        }
        ExprKind::MethodCall { object, method, .. } => {
            method.name == "get" && is_expr_dict(object, type_info, dict_vars)
        }
        _ => false,
    }
}

/// Collect diagnostics for untyped variables initialized from dict access or undetermined types.
fn collect_undetermined_variable_diagnostics(
    stmts: &[Stmt<'_>],
    type_info: Option<&crate::type_system::TypeCheckResult>,
    file_name: &str,
    issues: &mut Vec<Issue>,
) {
    let mut typed_vars: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut dict_vars: std::collections::HashSet<String> = std::collections::HashSet::new();

    collect_scope_undetermined_variables(
        stmts,
        &mut typed_vars,
        &mut dict_vars,
        type_info,
        file_name,
        issues,
    );
}

fn collect_scope_undetermined_variables(
    stmts: &[Stmt<'_>],
    typed_vars: &mut std::collections::HashSet<String>,
    dict_vars: &mut std::collections::HashSet<String>,
    type_info: Option<&crate::type_system::TypeCheckResult>,
    file_name: &str,
    issues: &mut Vec<Issue>,
) {
    // Pass 1: Identify typed variables and dict variables in this scope
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::VarDecl { var, type_hint, init } => {
                let name = var.name.to_string();
                if let Some(hint) = type_hint {
                    typed_vars.insert(name.clone());
                    if let Some(s) = hint.simple_name() {
                        if s.eq_ignore_ascii_case("dict") {
                            dict_vars.insert(name.clone());
                        }
                    }
                }
                if let Some(init_expr) = init {
                    if is_expr_dict(init_expr, type_info, dict_vars) {
                        dict_vars.insert(name);
                    }
                }
            }
            StmtKind::Expr(expr) => {
                if let ExprKind::Assign { op: crate::ast::expr::AssignOp::Assign, target, value } = &expr.kind {
                    if let ExprKind::Variable(var) = &target.kind {
                        let name = var.name.to_string();
                        if is_expr_dict(value, type_info, dict_vars) {
                            dict_vars.insert(name);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Pass 2: Check statements in this scope
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Expr(expr) => {
                if let ExprKind::Assign { op: crate::ast::expr::AssignOp::Assign, target, value } = &expr.kind {
                    if let ExprKind::Variable(var) = &target.kind {
                        let name = var.name.to_string();
                        if !typed_vars.contains(&name) {
                            if is_dict_element_access(value, type_info, dict_vars) {
                                issues.push(Issue::new(
                                    IssueLevel::Error,
                                    "cannot-decide-variable-type",
                                    format!(
                                        "Type Error: Cannot decide what is type of '{}'. An explicit type annotation is required (e.g. 'string {} = ...').",
                                        name, name
                                    ),
                                    file_name,
                                    var.span.line,
                                    var.span.col,
                                    var.span.start,
                                    var.span.end,
                                ));
                                typed_vars.insert(name);
                            }
                        }
                    }
                }
            }
            StmtKind::FunctionDecl(func) => {
                if let Some(body) = &func.body {
                    let mut fn_typed = std::collections::HashSet::new();
                    let mut fn_dict = std::collections::HashSet::new();
                    for param in &func.params {
                        let p_name = param.name.name.to_string();
                        if let Some(ref hint) = param.type_hint {
                            fn_typed.insert(p_name.clone());
                            if let Some(s) = hint.simple_name() {
                                if s.eq_ignore_ascii_case("dict") {
                                    fn_dict.insert(p_name);
                                }
                            }
                        }
                    }
                    collect_scope_undetermined_variables(
                        body,
                        &mut fn_typed,
                        &mut fn_dict,
                        type_info,
                        file_name,
                        issues,
                    );
                }
            }
            StmtKind::ClassDecl(cls) => {
                for member in &cls.members {
                    if let ClassMemberKind::Method(method) = &member.kind {
                        if let Some(body) = &method.function.body {
                            let mut method_typed = std::collections::HashSet::new();
                            let mut method_dict = std::collections::HashSet::new();
                            method_typed.insert("$this".to_string());
                            method_typed.insert("this".to_string());
                            for param in &method.function.params {
                                let p_name = param.name.name.to_string();
                                if let Some(ref hint) = param.type_hint {
                                    method_typed.insert(p_name.clone());
                                    if let Some(s) = hint.simple_name() {
                                        if s.eq_ignore_ascii_case("dict") {
                                            method_dict.insert(p_name);
                                        }
                                    }
                                }
                            }
                            collect_scope_undetermined_variables(
                                body,
                                &mut method_typed,
                                &mut method_dict,
                                type_info,
                                file_name,
                                issues,
                            );
                        }
                    }
                }
            }
            StmtKind::Namespace { body: Some(body), .. } | StmtKind::Block(body) => {
                collect_scope_undetermined_variables(
                    body,
                    typed_vars,
                    dict_vars,
                    type_info,
                    file_name,
                    issues,
                );
            }
            _ => {}
        }
    }
}

