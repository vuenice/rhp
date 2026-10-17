use std::collections::{HashMap, HashSet};

use crate::ast::{
    expr::{ArrayElement, AssignOp, BinaryOp, Expr, ExprKind, Literal, UnaryOp},
    stmt::{ClassMemberKind, Param, Stmt, StmtKind},
    types::CastType,
};
use crate::parser::parse;
use crate::type_system::{
    checker::TypeChecker,
    types::{CollectionSubtype, RhpType},
    TypeCheckResult,
};

/// Automatically adds types before untyped variables wherever statically determinable.
///
/// Implements requirements:
/// - Recognizes `list`, `dict`, or `hashMap` when array is defined at the same time of declaration and not empty:
///   - Sequential integer indexed elements: `list $arr = [1, 2, 3];`
///   - String-keyed key-value pairs: `dict $arr = ['name' => 'Alice'];`
///   - Complex / non-string keyed pairs: `hashMap $arr = [1 => 'first'];`
///   - Empty array: `array $arr = [];`
/// - Infers types for variables defined at declaration time (`$x = '';` -> `string $x = '';`, `$y = 1;` -> `int $y = 1;`)
/// - Infers types for function/method parameters with default values (`function foo($x = 1)` -> `function foo(int $x = 1)`)
/// - Infers types for class properties with defaults (`public $user = ['a' => 1];` -> `public dict $user = ['a' => 1];`)
/// - If types are already written, DOES NOT change them (type mismatches will be reported as errors in typecheck)
/// - Leaves untyped when ambiguous, dynamic, or reassigned to conflicting types ("whenever not possible no issue")
pub fn add_variable_types(source: &str) -> String {
    let program = match parse(source) {
        Ok(p) => p,
        Err(_) => return source.to_string(),
    };

    let type_info = TypeChecker::new().check_program(&program).ok();
    let mut edits: Vec<(usize, String)> = Vec::new();

    // 1. Process top-level script statements
    collect_scope_edits(
        &program.statements,
        &HashSet::new(),
        &HashMap::new(),
        type_info.as_ref(),
        None,
        &mut edits,
        source,
    );

    // 2. Process functions and class methods recursively
    collect_nested_declarations(&program.statements, type_info.as_ref(), &mut edits, source);

    if edits.is_empty() {
        return source.to_string();
    }

    // Sort edits by byte offset in descending order to apply from back to front
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    // Deduplicate any overlapping insertion points
    edits.dedup_by(|a, b| a.0 == b.0);

    let mut result = source.to_string();
    for (offset, text) in edits {
        if offset <= result.len() {
            result.insert_str(offset, &text);
        }
    }

    result
}

/// Recursively collect declarations from functions, classes, interfaces, traits.
fn collect_nested_declarations(
    stmts: &[Stmt<'_>],
    type_info: Option<&TypeCheckResult>,
    edits: &mut Vec<(usize, String)>,
    source: &str,
) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::FunctionDecl(func) => {
                // Infer types for untyped parameters with default values
                collect_param_edits(&func.params, type_info, edits, source);

                if let Some(body) = &func.body {
                    let mut pre_declared = HashSet::new();
                    let mut param_types = HashMap::new();
                    for param in &func.params {
                        pre_declared.insert(param.name.name.to_string());
                        if let Some(ref hint) = param.type_hint {
                            if let Some(simple) = hint.simple_name() {
                                param_types.insert(param.name.name.to_string(), simple.to_string());
                            }
                        } else if let Some(ref def) = param.default_value {
                            if let Some(t) = infer_expression_type_name(def, type_info, &HashMap::new()) {
                                param_types.insert(param.name.name.to_string(), t);
                            }
                        }
                    }
                    collect_scope_edits(
                        body,
                        &pre_declared,
                        &param_types,
                        type_info,
                        Some(func.name.name),
                        edits,
                        source,
                    );
                    collect_nested_declarations(body, type_info, edits, source);
                }
            }
            StmtKind::ClassDecl(cls) => {
                for member in &cls.members {
                    match &member.kind {
                        ClassMemberKind::Property(prop) => {
                            // If type is already written, DO NOT change it!
                            if prop.type_hint.is_none() {
                                if let Some(init) = &prop.default {
                                    if let Some(ty_str) = infer_expression_type_name(init, type_info, &HashMap::new()) {
                                        let offset = prop.name.span.start;
                                        if should_insert_type_before(offset, &ty_str, source) {
                                            edits.push((offset, format!("{} ", ty_str)));
                                        }
                                    }
                                }
                            }
                        }
                        ClassMemberKind::Method(method) => {
                            // Infer types for untyped parameters with default values
                            collect_param_edits(&method.function.params, type_info, edits, source);

                            if let Some(body) = &method.function.body {
                                let mut pre_declared = HashSet::new();
                                pre_declared.insert("$this".to_string());
                                let mut param_types = HashMap::new();
                                for param in &method.function.params {
                                    pre_declared.insert(param.name.name.to_string());
                                    if let Some(ref hint) = param.type_hint {
                                        if let Some(simple) = hint.simple_name() {
                                            param_types.insert(param.name.name.to_string(), simple.to_string());
                                        }
                                    } else if let Some(ref def) = param.default_value {
                                        if let Some(t) = infer_expression_type_name(def, type_info, &HashMap::new()) {
                                            param_types.insert(param.name.name.to_string(), t);
                                        }
                                    }
                                }
                                let method_key = format!("{}::{}", cls.name.name, method.function.name.name);
                                collect_scope_edits(
                                    body,
                                    &pre_declared,
                                    &param_types,
                                    type_info,
                                    Some(&method_key),
                                    edits,
                                    source,
                                );
                                collect_nested_declarations(body, type_info, edits, source);
                            }
                        }
                        _ => {}
                    }
                }
            }
            StmtKind::Namespace { body: Some(body), .. } => {
                collect_nested_declarations(body, type_info, edits, source);
            }
            StmtKind::Block(body) => {
                collect_nested_declarations(body, type_info, edits, source);
            }
            StmtKind::If { then_branch, elseif_branches, else_branch, .. } => {
                collect_nested_stmt(then_branch, type_info, edits, source);
                for branch in elseif_branches {
                    collect_nested_stmt(&branch.body, type_info, edits, source);
                }
                if let Some(else_b) = else_branch {
                    collect_nested_stmt(else_b, type_info, edits, source);
                }
            }
            StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } => {
                collect_nested_stmt(body, type_info, edits, source);
            }
            StmtKind::For { body, .. } | StmtKind::Foreach { body, .. } => {
                collect_nested_stmt(body, type_info, edits, source);
            }
            _ => {}
        }
    }
}

/// Infer types for parameters with default values if they are untyped.
/// If already typed, DOES NOT touch them.
fn collect_param_edits(
    params: &[Param<'_>],
    type_info: Option<&TypeCheckResult>,
    edits: &mut Vec<(usize, String)>,
    source: &str,
) {
    for param in params {
        // If the type is already written, we don't change it!
        if param.type_hint.is_some() {
            continue;
        }

        // Defined at declaration time with a default value: infer the parameter type
        if let Some(ref def_val) = param.default_value {
            if let Some(ty_str) = infer_expression_type_name(def_val, type_info, &HashMap::new()) {
                let offset = param.name.span.start;
                if should_insert_type_before(offset, &ty_str, source) {
                    edits.push((offset, format!("{} ", ty_str)));
                }
            }
        }
    }
}

fn collect_nested_stmt(
    stmt: &Stmt<'_>,
    type_info: Option<&TypeCheckResult>,
    edits: &mut Vec<(usize, String)>,
    source: &str,
) {
    if let StmtKind::Block(stmts) = &stmt.kind {
        collect_nested_declarations(stmts, type_info, edits, source);
    }
}

/// A variable assignment occurrence in a scope.
struct VarAssignment {
    offset: usize,
    inferred_type: Option<String>,
}

/// Analyze statements in a local scope to identify candidates for variable typing.
fn collect_scope_edits(
    stmts: &[Stmt<'_>],
    pre_declared: &HashSet<String>,
    initial_known_types: &HashMap<String, String>,
    type_info: Option<&TypeCheckResult>,
    _scope_key: Option<&str>,
    edits: &mut Vec<(usize, String)>,
    source: &str,
) {
    let mut assignments: HashMap<String, Vec<VarAssignment>> = HashMap::new();
    let mut already_typed_vars: HashSet<String> = HashSet::new();
    let mut known_types: HashMap<String, String> = initial_known_types.clone();

    // Pass 1: Identify all variable declarations and assignments in this scope
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::VarDecl { var, type_hint, init } => {
                let var_name = var.name.to_string();
                if type_hint.is_some() {
                    // Already typed: DO NOT change it!
                    already_typed_vars.insert(var_name);
                } else if let Some(init_expr) = init {
                    let ty = infer_expression_type_name(init_expr, type_info, &known_types);
                    if let Some(ref t) = ty {
                        known_types.insert(var_name.clone(), t.clone());
                    }
                    assignments
                        .entry(var_name)
                        .or_default()
                        .push(VarAssignment {
                            offset: var.span.start,
                            inferred_type: ty,
                        });
                }
            }
            StmtKind::Expr(expr) => {
                if let ExprKind::Assign { op: AssignOp::Assign, target, value } = &expr.kind {
                    if let ExprKind::Variable(var) = &target.kind {
                        let var_name = var.name.to_string();
                        let ty = infer_expression_type_name(value, type_info, &known_types);
                        if let Some(ref t) = ty {
                            known_types.insert(var_name.clone(), t.clone());
                        }
                        assignments
                            .entry(var_name)
                            .or_default()
                            .push(VarAssignment {
                                offset: var.span.start,
                                inferred_type: ty,
                            });
                    }
                }
            }
            _ => {}
        }
    }

    // Pass 2: Decide whether to add type prefix to initial assignments
    for (var_name, var_assigns) in assignments {
        // Skip variables already declared with explicit types or parameters
        if pre_declared.contains(&var_name) || already_typed_vars.contains(&var_name) {
            continue;
        }

        if var_assigns.is_empty() {
            continue;
        }

        let first = &var_assigns[0];
        let initial_type = match &first.inferred_type {
            Some(t) => t,
            None => continue, // Cannot infer type
        };

        // Check if any subsequent assignments conflict with the initial type
        let mut has_type_conflict = false;
        for next_assign in &var_assigns[1..] {
            if let Some(next_ty) = &next_assign.inferred_type {
                if next_ty != initial_type {
                    has_type_conflict = true;
                    break;
                }
            }
        }

        if has_type_conflict {
            // Reassigned to different types: keep dynamic ("whenever not possible no issue")
            continue;
        }

        // Safe to add type to the first assignment!
        if should_insert_type_before(first.offset, initial_type, source) {
            edits.push((first.offset, format!("{} ", initial_type)));
        }
    }
}

/// Check if the source code immediately preceding `offset` already contains this type annotation.
fn should_insert_type_before(offset: usize, ty_str: &str, source: &str) -> bool {
    let prefix = if offset > 0 {
        let start = offset.saturating_sub(40);
        &source[start..offset]
    } else {
        ""
    };

    let trimmed_prefix = prefix.trim_end();
    // Check if prefix already ends with the type name or any type keyword
    if trimmed_prefix.ends_with(ty_str) {
        return false;
    }

    let type_keywords = [
        "int", "float", "string", "bool", "void", "mixed", "array", "list", "dict", "set", "hashmap", "hashMap", "object",
    ];
    for kw in &type_keywords {
        if trimmed_prefix.ends_with(kw) {
            return false;
        }
    }

    true
}

/// Classify an array literal into `list`, `dict`, or `hashMap` when defined and not empty.
/// - Sequential elements (no keys or contiguous 0, 1, 2... indices): `list`
/// - String-keyed key-value pairs (`'name' => ...`): `dict`
/// - Non-string keys (integers, booleans, objects, complex keys): `hashMap`
/// - Empty array `[]`: `array`
pub fn classify_array_literal(elements: &[ArrayElement<'_>]) -> Option<String> {
    if elements.is_empty() {
        return Some("array".to_string());
    }

    let mut has_keys = false;
    let mut all_string_keys = true;
    let mut is_sequential_int = true;
    let mut expected_idx = 0;

    for el in elements {
        if let Some(ref k) = el.key {
            has_keys = true;
            match &k.kind {
                ExprKind::Literal(Literal::String { .. }) | ExprKind::Literal(Literal::InterpolatedString { .. }) => {
                    is_sequential_int = false;
                }
                ExprKind::Literal(Literal::Int { value, .. }) => {
                    all_string_keys = false;
                    if *value != expected_idx as i64 {
                        is_sequential_int = false;
                    }
                }
                _ => {
                    all_string_keys = false;
                    is_sequential_int = false;
                }
            }
        } else {
            all_string_keys = false;
        }
        expected_idx += 1;
    }

    if !has_keys || is_sequential_int {
        Some("list".to_string())
    } else if all_string_keys {
        Some("dict".to_string())
    } else {
        Some("hashMap".to_string())
    }
}

/// Infer canonical type name string from an expression.
pub fn infer_expression_type_name(
    expr: &Expr<'_>,
    type_info: Option<&TypeCheckResult>,
    known_vars: &HashMap<String, String>,
) -> Option<String> {
    // 1. Direct AST Pattern Matching
    match &expr.kind {
        ExprKind::Literal(lit) => match lit {
            Literal::String { .. } | Literal::InterpolatedString { .. } => Some("string".to_string()),
            Literal::Int { .. } => Some("int".to_string()),
            Literal::Float { .. } => Some("float".to_string()),
            Literal::Bool(_) => Some("bool".to_string()),
            Literal::Null => None,
        },

        // Recognize list, dict, or hashMap if array is defined at the same time and not empty
        ExprKind::Array(elements) => classify_array_literal(elements),

        ExprKind::StaticCall { target, method, .. } => {
            if let ExprKind::Ident(target_id) = &target.kind {
                match target_id.name {
                    "Arr" => match method.name {
                        "list" => Some("list".to_string()),
                        "dict" => Some("dict".to_string()),
                        "set" => Some("set".to_string()),
                        "map" | "hashMap" => Some("hashMap".to_string()),
                        _ => Some("array".to_string()),
                    },
                    "Str" => match method.name {
                        "contains" | "startsWith" | "endsWith" | "is" | "isUuid" | "isUlid" => Some("bool".to_string()),
                        "length" => Some("int".to_string()),
                        _ => Some("string".to_string()),
                    },
                    "Int" => Some("int".to_string()),
                    "Float" => Some("float".to_string()),
                    _ => None,
                }
            } else {
                None
            }
        }

        ExprKind::Binary { op, left, right } => match op {
            BinaryOp::Concat => Some("string".to_string()),
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
            | BinaryOp::WordXor => Some("bool".to_string()),
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                let left_ty = infer_expression_type_name(left, type_info, known_vars);
                let right_ty = infer_expression_type_name(right, type_info, known_vars);
                if left_ty.as_deref() == Some("float") || right_ty.as_deref() == Some("float") {
                    Some("float".to_string())
                } else if left_ty.as_deref() == Some("int") && right_ty.as_deref() == Some("int") {
                    Some("int".to_string())
                } else {
                    Some("int".to_string())
                }
            }
            BinaryOp::Div => Some("float".to_string()),
            BinaryOp::Mod => Some("int".to_string()),
            _ => None,
        },

        ExprKind::Unary { op, .. } => match op {
            UnaryOp::Not => Some("bool".to_string()),
            UnaryOp::PreInc | UnaryOp::PostInc | UnaryOp::PreDec | UnaryOp::PostDec => Some("int".to_string()),
            _ => None,
        },

        ExprKind::Cast { kind, .. } => match kind {
            CastType::Int => Some("int".to_string()),
            CastType::Float => Some("float".to_string()),
            CastType::String => Some("string".to_string()),
            CastType::Bool => Some("bool".to_string()),
            CastType::Array => Some("array".to_string()),
            CastType::Object => Some("object".to_string()),
        },

        ExprKind::New { class, .. } => {
            if let ExprKind::Ident(ident) = &class.kind {
                Some(ident.name.to_string())
            } else {
                Some("object".to_string())
            }
        }

        ExprKind::Variable(var) => known_vars.get(var.name).cloned(),

        ExprKind::Grouping(inner) => infer_expression_type_name(inner, type_info, known_vars),

        _ => {
            // Check TypeCheckResult fallback if available
            if let Some(info) = type_info {
                if let Some(rhp_ty) = info.expr_type(&expr.span) {
                    return rhp_type_to_syntax_name(rhp_ty);
                }
            }
            None
        }
    }
}

/// Convert RhpType to standard RHP type syntax string.
fn rhp_type_to_syntax_name(ty: &RhpType) -> Option<String> {
    match ty {
        RhpType::Int => Some("int".to_string()),
        RhpType::Float => Some("float".to_string()),
        RhpType::String => Some("string".to_string()),
        RhpType::Bool => Some("bool".to_string()),
        RhpType::Array(_) => Some("array".to_string()),
        RhpType::Collection(CollectionSubtype::List(_)) => Some("list".to_string()),
        RhpType::Collection(CollectionSubtype::Dict(_)) => Some("dict".to_string()),
        RhpType::Collection(CollectionSubtype::Set(_)) => Some("set".to_string()),
        RhpType::Collection(CollectionSubtype::HashMap(_, _)) => Some("hashMap".to_string()),
        RhpType::Class(cls) => Some(cls.clone()),
        RhpType::RustInt(kind) => Some(kind.as_str().to_string()),
        RhpType::RustFloat(kind) => Some(kind.as_str().to_string()),
        _ => None,
    }
}
