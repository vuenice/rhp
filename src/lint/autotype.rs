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
/// - Recognizes `list`, `dict`, `set`, or `hashMap` from an untyped array:
///   - Sequential integer indexed elements: `list $arr = [1, 2, 3];`
///   - String-keyed key-value pairs: `dict $arr = ['name' => 'Alice'];`
///   - Complex / non-string keyed pairs: `hashMap $arr = [1 => 'first'];`
///   - `Arr::set(...)`: `set`
///   - Empty array with no write: `array $arr = [];`
///   - Empty array, first write `$arr[] =` or `$arr[0] =`: `list`
///   - Empty array, first write `$arr['name'] =`: `dict`
///   - Empty array, first write `$arr[999] =` or another non-sequential key: `hashMap`
/// - Infers types for variables defined at declaration time (`$x = '';` -> `string $x = '';`, `$y = 1;` -> `int $y = 1;`)
/// - Infers types for function/method parameters with default values (`function foo($x = 1)` -> `function foo(int $x = 1)`)
/// - Infers return types from returned values (`function foo() { return [1]; }` -> `function foo(): list`)
/// - A function with no return value becomes `: void`
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
                collect_param_edits(&func.params, func.body.as_deref(), type_info, edits, source);
                collect_return_type_edit(func, type_info, edits, source);

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
                            collect_param_edits(
                                &method.function.params,
                                method.function.body.as_deref(),
                                type_info,
                                edits,
                                source,
                            );
                            collect_return_type_edit(&method.function, type_info, edits, source);

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
            StmtKind::For { body, .. }
            | StmtKind::Foreach { body, .. }
            | StmtKind::ForIn { body, .. }
            | StmtKind::Loop { body } => {
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
    body: Option<&[Stmt<'_>]>,
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
                let ty_str = refine_untyped_array(body.unwrap_or(&[]), param.name.name, &ty_str);
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

/// Insert `: list` / `: dict` / `: array` / `: void` when the function has no return type.
fn collect_return_type_edit(
    func: &crate::ast::stmt::FunctionDecl<'_>,
    type_info: Option<&TypeCheckResult>,
    edits: &mut Vec<(usize, String)>,
    source: &str,
) {
    if func.return_type.is_some() || matches!(func.name.name, "__construct" | "__destruct") {
        return;
    }
    let Some(body) = func.body.as_deref() else {
        return;
    };
    let Some(offset) = closing_paren_offset(source, func.name.span.end) else {
        return;
    };
    let mut known = HashMap::new();
    for param in &func.params {
        if let Some(ref hint) = param.type_hint {
            if let Some(simple) = hint.simple_name() {
                known.insert(param.name.name.to_string(), simple.to_string());
            }
        } else if let Some(ref def) = param.default_value {
            if let Some(t) = infer_expression_type_name(def, type_info, &HashMap::new()) {
                known.insert(param.name.name.to_string(), t);
            }
        }
    }
    remember_local_types(body, type_info, &mut known);
    let Some(ty) = infer_function_return_type(body, type_info, &known) else {
        return;
    };
    edits.push((offset, format!(": {ty}")));
}

fn closing_paren_offset(source: &str, from: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = from;
    let mut depth = 0i32;
    let mut seen = false;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => {
                depth += 1;
                seen = true;
            }
            b')' => {
                depth -= 1;
                if seen && depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn remember_local_types(
    stmts: &[Stmt<'_>],
    type_info: Option<&TypeCheckResult>,
    known: &mut HashMap<String, String>,
) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::VarDecl { var, init: Some(init), .. } => {
                if let Some(t) = infer_expression_type_name(init, type_info, known) {
                    known.insert(var.name.to_string(), t);
                }
            }
            StmtKind::Expr(expr) => {
                if let ExprKind::Assign { target, value, .. } = &expr.kind {
                    if let ExprKind::Variable(var) = &target.kind {
                        if let Some(t) = infer_expression_type_name(value, type_info, known) {
                            known.insert(var.name.to_string(), t);
                        }
                    }
                }
            }
            StmtKind::Block(body) | StmtKind::Namespace { body: Some(body), .. } => {
                remember_local_types(body, type_info, known);
            }
            StmtKind::If { then_branch, elseif_branches, else_branch, .. } => {
                remember_local_types(std::slice::from_ref(then_branch.as_ref()), type_info, known);
                for branch in elseif_branches {
                    remember_local_types(std::slice::from_ref(&branch.body), type_info, known);
                }
                if let Some(else_branch) = else_branch {
                    remember_local_types(std::slice::from_ref(else_branch.as_ref()), type_info, known);
                }
            }
            StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } | StmtKind::Loop { body } => {
                remember_local_types(std::slice::from_ref(body.as_ref()), type_info, known);
            }
            StmtKind::For { body, .. } | StmtKind::Foreach { body, .. } | StmtKind::ForIn { body, .. } => {
                remember_local_types(std::slice::from_ref(body.as_ref()), type_info, known);
            }
            _ => {}
        }
    }
}

fn infer_function_return_type(
    body: &[Stmt<'_>],
    type_info: Option<&TypeCheckResult>,
    known: &HashMap<String, String>,
) -> Option<String> {
    let mut returns: Vec<Option<String>> = Vec::new();
    collect_return_types(body, type_info, known, &mut returns);
    if returns.is_empty() {
        return Some("void".to_string());
    }
    if !always_returns(body) {
        return None;
    }
    let first = returns.first()?.clone()?;
    if returns.iter().all(|ty| ty.as_deref() == Some(first.as_str())) {
        Some(first)
    } else {
        None
    }
}

fn collect_return_types(
    stmts: &[Stmt<'_>],
    type_info: Option<&TypeCheckResult>,
    known: &HashMap<String, String>,
    out: &mut Vec<Option<String>>,
) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Return(expr) => {
                out.push(expr.as_ref().and_then(|e| infer_expression_type_name(e, type_info, known)));
            }
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {}
            StmtKind::Block(body) | StmtKind::Namespace { body: Some(body), .. } => {
                collect_return_types(body, type_info, known, out);
            }
            StmtKind::If { then_branch, elseif_branches, else_branch, .. } => {
                collect_return_types(std::slice::from_ref(then_branch.as_ref()), type_info, known, out);
                for branch in elseif_branches {
                    collect_return_types(std::slice::from_ref(&branch.body), type_info, known, out);
                }
                if let Some(else_branch) = else_branch {
                    collect_return_types(std::slice::from_ref(else_branch.as_ref()), type_info, known, out);
                }
            }
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::Foreach { body, .. }
            | StmtKind::ForIn { body, .. }
            | StmtKind::Loop { body } => {
                collect_return_types(std::slice::from_ref(body.as_ref()), type_info, known, out);
            }
            StmtKind::TryCatch { try_block, catches, finally_block } => {
                collect_return_types(try_block, type_info, known, out);
                for catch in catches {
                    collect_return_types(&catch.body, type_info, known, out);
                }
                if let Some(finally_block) = finally_block {
                    collect_return_types(finally_block, type_info, known, out);
                }
            }
            StmtKind::Switch { cases, .. } => {
                for case in cases {
                    collect_return_types(&case.body, type_info, known, out);
                }
            }
            _ => {}
        }
    }
}

fn always_returns(stmts: &[Stmt<'_>]) -> bool {
    stmts.iter().any(stmt_always_returns)
}

fn stmt_always_returns(stmt: &Stmt<'_>) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) => true,
        StmtKind::Block(body) => always_returns(body),
        StmtKind::If { then_branch, elseif_branches, else_branch, .. } => {
            let else_ok = else_branch.as_ref().is_some_and(|b| stmt_always_returns(b));
            stmt_always_returns(then_branch)
                && else_ok
                && elseif_branches.iter().all(|b| stmt_always_returns(&b.body))
        }
        _ => false,
    }
}

/// A variable assignment occurrence in a scope.
struct VarAssignment {
    offset: usize,
    inferred_type: Option<String>,
    references_self: bool,
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
                    let refs_self = expr_references_variable(init_expr, &var_name);
                    if let Some(ref t) = ty {
                        known_types.insert(var_name.clone(), t.clone());
                    }
                    assignments
                        .entry(var_name)
                        .or_default()
                        .push(VarAssignment {
                            offset: var.span.start,
                            inferred_type: ty,
                            references_self: refs_self,
                        });
                }
            }
            StmtKind::Expr(expr) => {
                if let ExprKind::Assign { op: AssignOp::Assign, target, value } = &expr.kind {
                    if let ExprKind::Variable(var) = &target.kind {
                        let var_name = var.name.to_string();
                        let ty = infer_expression_type_name(value, type_info, &known_types);
                        let refs_self = expr_references_variable(value, &var_name);
                        if let Some(ref t) = ty {
                            known_types.insert(var_name.clone(), t.clone());
                        }
                        assignments
                            .entry(var_name)
                            .or_default()
                            .push(VarAssignment {
                                offset: var.span.start,
                                inferred_type: ty,
                                references_self: refs_self,
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
        // If the assignment references itself (e.g. $z = Arr::list($z)->map();), it is an update of an existing var, not a declaration
        if first.references_self {
            continue;
        }

        let initial_owned = match &first.inferred_type {
            Some(t) => refine_untyped_array(stmts, &var_name, t),
            None => continue, // Cannot infer type
        };
        let initial_type = initial_owned.as_str();

        // Check if any subsequent assignments conflict with the initial type
        let mut has_type_conflict = false;
        for next_assign in &var_assigns[1..] {
            if let Some(next_ty) = &next_assign.inferred_type {
                if next_ty.as_str() != initial_type {
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

/// A write that reveals what an empty untyped array should become.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EmptyWrite {
    /// `$items[] = $val`
    Append,
    /// `$items[0] = $val`
    IndexZero,
    /// `$items['name'] = $val`
    StringKey,
    /// `$items[999] = $val`. A list can absorb this only after it is already a list.
    GappedInt,
    /// Bool, float, null, or another non-string literal key. Always a hash map.
    Complex,
    /// Key is not a literal, so the collection cannot be named.
    Dynamic,
}

/// Apply the empty-array and set rules on top of a type inferred from the initializer.
fn refine_untyped_array(stmts: &[Stmt<'_>], var_name: &str, inferred: &str) -> String {
    if inferred == "array" {
        if let Some(promoted) = classify_empty_array_writes(stmts, var_name) {
            return promoted;
        }
    }
    inferred.to_string()
}

fn classify_empty_array_writes(stmts: &[Stmt<'_>], var_name: &str) -> Option<String> {
    let mut writes = Vec::new();
    collect_empty_writes(stmts, var_name, &mut writes);
    let first = writes.first().copied()?;
    let kind = match first {
        EmptyWrite::Append | EmptyWrite::IndexZero => "list",
        EmptyWrite::StringKey => "dict",
        EmptyWrite::GappedInt | EmptyWrite::Complex => "hashMap",
        EmptyWrite::Dynamic => return None,
    };
    let compatible = writes.iter().all(|write| match (kind, write) {
        ("list", EmptyWrite::Append | EmptyWrite::IndexZero | EmptyWrite::GappedInt) => true,
        ("dict", EmptyWrite::StringKey | EmptyWrite::Append) => true,
        ("hashMap", _) => true,
        _ => false,
    });
    if compatible {
        Some(kind.to_string())
    } else {
        None
    }
}

fn collect_empty_writes(stmts: &[Stmt<'_>], var_name: &str, out: &mut Vec<EmptyWrite>) {
    for stmt in stmts {
        walk_stmt_for_writes(stmt, var_name, out);
    }
}

fn walk_stmt_for_writes(stmt: &Stmt<'_>, var_name: &str, out: &mut Vec<EmptyWrite>) {
    match &stmt.kind {
        StmtKind::Expr(expr) => walk_expr_for_writes(expr, var_name, out),
        StmtKind::VarDecl { init, .. } => {
            if let Some(init) = init {
                walk_expr_for_writes(init, var_name, out);
            }
        }
        StmtKind::Block(body)
        | StmtKind::Namespace { body: Some(body), .. }
        | StmtKind::TryCatch { try_block: body, .. } => {
            collect_empty_writes(body, var_name, out);
            if let StmtKind::TryCatch { catches, finally_block, .. } = &stmt.kind {
                for catch in catches {
                    collect_empty_writes(&catch.body, var_name, out);
                }
                if let Some(finally_block) = finally_block {
                    collect_empty_writes(finally_block, var_name, out);
                }
            }
        }
        StmtKind::If { condition, then_branch, elseif_branches, else_branch } => {
            walk_expr_for_writes(condition, var_name, out);
            walk_stmt_for_writes(then_branch, var_name, out);
            for branch in elseif_branches {
                walk_expr_for_writes(&branch.condition, var_name, out);
                walk_stmt_for_writes(&branch.body, var_name, out);
            }
            if let Some(else_branch) = else_branch {
                walk_stmt_for_writes(else_branch, var_name, out);
            }
        }
        StmtKind::While { condition, body } | StmtKind::DoWhile { body, condition } => {
            walk_expr_for_writes(condition, var_name, out);
            walk_stmt_for_writes(body, var_name, out);
        }
        StmtKind::For { init, condition, update, body } => {
            for expr in init.iter().chain(condition).chain(update) {
                walk_expr_for_writes(expr, var_name, out);
            }
            walk_stmt_for_writes(body, var_name, out);
        }
        StmtKind::Foreach { expr, body, .. } | StmtKind::ForIn { expr, body, .. } => {
            walk_expr_for_writes(expr, var_name, out);
            walk_stmt_for_writes(body, var_name, out);
        }
        StmtKind::Loop { body } => walk_stmt_for_writes(body, var_name, out),
        StmtKind::Switch { condition, cases } => {
            walk_expr_for_writes(condition, var_name, out);
            for case in cases {
            if let Some(test) = &case.value {
                walk_expr_for_writes(test, var_name, out);
            }
                collect_empty_writes(&case.body, var_name, out);
            }
        }
        StmtKind::Return(Some(expr)) | StmtKind::Throw(expr) => walk_expr_for_writes(expr, var_name, out),
        StmtKind::Unset(exprs) | StmtKind::Echo(exprs) => {
            for expr in exprs {
                walk_expr_for_writes(expr, var_name, out);
            }
        }
        _ => {}
    }
}

fn walk_expr_for_writes(expr: &Expr<'_>, var_name: &str, out: &mut Vec<EmptyWrite>) {
    if let ExprKind::Assign { target, value, .. } = &expr.kind {
        if let Some(write) = write_kind(target, var_name) {
            out.push(write);
        }
        walk_expr_for_writes(value, var_name, out);
        if let ExprKind::ArrayIndex { array, index } = &target.kind {
            walk_expr_for_writes(array, var_name, out);
            if let Some(index) = index {
                walk_expr_for_writes(index, var_name, out);
            }
        }
        return;
    }
    match &expr.kind {
        ExprKind::ArrayIndex { array, index } => {
            walk_expr_for_writes(array, var_name, out);
            if let Some(index) = index {
                walk_expr_for_writes(index, var_name, out);
            }
        }
        ExprKind::Array(elements) => {
            for element in elements {
                if let Some(key) = &element.key {
                    walk_expr_for_writes(key, var_name, out);
                }
                walk_expr_for_writes(&element.value, var_name, out);
            }
        }
        ExprKind::Binary { left, right, .. } | ExprKind::Ternary { condition: left, if_false: right, .. } => {
            walk_expr_for_writes(left, var_name, out);
            walk_expr_for_writes(right, var_name, out);
        }
        ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } => {
            walk_expr_for_writes(expr, var_name, out);
        }
        ExprKind::Call { callee, args } => {
            walk_expr_for_writes(callee, var_name, out);
            for arg in args {
                walk_expr_for_writes(&arg.value, var_name, out);
            }
        }
        ExprKind::MethodCall { object, args, .. } | ExprKind::StaticCall { target: object, args, .. } => {
            walk_expr_for_writes(object, var_name, out);
            for arg in args {
                walk_expr_for_writes(&arg.value, var_name, out);
            }
        }
        _ => {}
    }
}

fn write_kind(target: &Expr<'_>, var_name: &str) -> Option<EmptyWrite> {
    let ExprKind::ArrayIndex { array, index } = &target.kind else {
        return None;
    };
    let ExprKind::Variable(var) = &array.kind else {
        return None;
    };
    if var.name != var_name {
        return None;
    }
    let Some(index) = index else {
        return Some(EmptyWrite::Append);
    };
    Some(match &index.kind {
        ExprKind::Literal(Literal::Int { value: 0, .. }) => EmptyWrite::IndexZero,
        ExprKind::Literal(Literal::Int { .. }) => EmptyWrite::GappedInt,
        ExprKind::Literal(Literal::String { .. }) | ExprKind::Literal(Literal::InterpolatedString { .. }) => {
            EmptyWrite::StringKey
        }
        ExprKind::Literal(_) => EmptyWrite::Complex,
        _ => EmptyWrite::Dynamic,
    })
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
            Literal::Bytes { .. } => Some("bytes".to_string()),
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
                        "check" | "compare" | "digit" | "isDigit" | "alpha" | "isAlpha" | "alnum" | "isAlnum" | "contains" | "includes" | "startsWith" | "endsWith" | "testRegex" | "regex" | "is" | "isUuid" | "isUlid" => Some("bool".to_string()),
                        "length" | "len" | "mbStrlen" | "mb_strlen" | "graphemeLen" | "grapheme_len" | "countOccurrences"
                        | "strcmp" | "cmp" | "compareBinary" | "binaryCompare"
                        | "strcasecmp" | "casecmp" | "caseCmp" | "caseCompare" | "compareCase"
                        | "strncmp" | "ncmp" | "nCmp" | "compareN" | "compareBinaryN"
                        | "strncasecmp" | "ncasecmp" | "nCaseCmp" | "caseNCmp" | "caseCompareN" => Some("int".to_string()),
                        "find" | "indexOf" => Some("?int".to_string()),
                        _ => Some("string".to_string()),
                    },
                    "Regex" => match method.name {
                        "isValid" | "testString" | "matches" | "isMatch" => Some("bool".to_string()),
                        _ => Some("string".to_string()),
                    },
                    "Crypt" => match method.name {
                        "hashAlgos" | "opensslCipherMethods" => Some("list".to_string()),
                        _ => Some("string".to_string()),
                    },
                    "Html" => Some("string".to_string()),
                    "Duration" | "CarbonInterval" => match method.name {
                        "spec" | "format" | "forHumans" | "toString" | "toIso8601String" | "toDateInterval" => {
                            Some("string".to_string())
                        }
                        "total" | "totalYears" | "totalMonths" | "totalWeeks" | "totalDays" | "totalHours"
                        | "totalMinutes" | "totalSeconds" | "totalMilliseconds" | "totalMicroseconds" => {
                            Some("float".to_string())
                        }
                        "compare" => Some("int".to_string()),
                        "eq" | "equalTo" | "gt" | "lt" | "gte" | "lte" | "isEmpty" | "isNegative" | "isPositive" => {
                            Some("bool".to_string())
                        }
                        _ => Some("Duration".to_string()),
                    },
                    "Time" | "Carbon" => match method.name {
                        "toDateTimeString" | "toDateString" | "toDatString" | "toTimeString"
                        | "toISOString" | "toIso8601String" | "toAtomString" | "toRfc2822String"
                        | "toFormattedDateString" | "toDayDateTimeString" | "format" | "diffForHumans" | "date" => Some("string".to_string()),
                        "getTimestamp" | "timestamp" | "year" | "month" | "day" | "hour" | "minute" | "second" => Some("int".to_string()),
                        "isToday" | "isYesterday" | "isTomorrow" | "isFuture" | "isPast" | "isWeekend" | "isWeekday" | "isLeapYear" => Some("bool".to_string()),
                        _ => Some("Time".to_string()),
                    },
                    "Url" | "URL" => match method.name {
                        "parseUrl" | "parseQuery" => Some("dict".to_string()),
                        _ => Some("string".to_string()),
                    },
                    "Ip" => match method.name {
                        "toLong" => Some("int".to_string()),
                        _ => Some("string".to_string()),
                    },
                    "Compress" => Some("string".to_string()),
                    "Uniq" => match method.name {
                        "isValid" | "is_valid" => Some("bool".to_string()),
                        "version" => Some("int".to_string()),
                        "parse" => Some("dict".to_string()),
                        _ => Some("string".to_string()),
                    },
                    "Csv" => match method.name {
                        "generate" | "formatRow" | "stringify" => Some("string".to_string()),
                        "close" | "feof" | "eof" | "rewind" | "writeFile" => Some("bool".to_string()),
                        "parse" | "readFile" | "parseRow" | "str_getcsv" => Some("list".to_string()),
                        _ => None,
                    },
                    "Sys" => match method.name {
                        "os" | "version" => Some("string".to_string()),
                        "timestamp" => Some("int".to_string()),
                        "disk_free_space" | "diskFreeSpace" | "diskfreespace" | "disk_total_space" | "diskTotalSpace" => Some("float".to_string()),
                        "print" | "echo" | "dd" | "dump" | "clearstatcache" | "clearStatCache" => Some("void".to_string()),
                        _ => None,
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
            BinaryOp::Add => {
                let left_ty = infer_expression_type_name(left, type_info, known_vars);
                let right_ty = infer_expression_type_name(right, type_info, known_vars);
                if left_ty.as_deref() == Some("string") || right_ty.as_deref() == Some("string") {
                    Some("string".to_string())
                } else if left_ty.as_deref() == Some("float") || right_ty.as_deref() == Some("float") {
                    Some("float".to_string())
                } else {
                    Some("int".to_string())
                }
            }
            BinaryOp::Sub | BinaryOp::Mul => {
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

        ExprKind::AnonymousClass { class_decl, .. } => {
            if let Some(ref ext) = class_decl.extends {
                Some(ext.name.to_string())
            } else if let Some(iface) = class_decl.implements.first() {
                Some(iface.name.to_string())
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
        _ => None,
    }
}

/// Recursively checks if an expression references a variable by name.
fn expr_references_variable(expr: &Expr<'_>, var_name: &str) -> bool {
    match &expr.kind {
        ExprKind::Variable(v) => v.name == var_name,
        ExprKind::ArrayIndex { array, index } => {
            expr_references_variable(array, var_name)
                || index
                    .as_ref()
                    .is_some_and(|idx| expr_references_variable(idx, var_name))
        }
        ExprKind::PropertyFetch { object, .. } => expr_references_variable(object, var_name),
        ExprKind::Unary { expr, .. } => expr_references_variable(expr, var_name),
        ExprKind::Binary { left, right, .. } => {
            expr_references_variable(left, var_name) || expr_references_variable(right, var_name)
        }
        ExprKind::Ternary {
            condition,
            if_true,
            if_false,
        } => {
            expr_references_variable(condition, var_name)
                || if_true
                    .as_ref()
                    .is_some_and(|t| expr_references_variable(t, var_name))
                || expr_references_variable(if_false, var_name)
        }
        ExprKind::Call { callee, args } => {
            expr_references_variable(callee, var_name)
                || args
                    .iter()
                    .any(|a| expr_references_variable(&a.value, var_name))
        }
        ExprKind::MethodCall { object, args, .. } => {
            expr_references_variable(object, var_name)
                || args
                    .iter()
                    .any(|a| expr_references_variable(&a.value, var_name))
        }
        ExprKind::StaticCall { args, .. } => args
            .iter()
            .any(|a| expr_references_variable(&a.value, var_name)),
        ExprKind::Array(elements) => elements.iter().any(|el| {
            el.key
                .as_ref()
                .is_some_and(|k| expr_references_variable(k, var_name))
                || expr_references_variable(&el.value, var_name)
        }),
        _ => false,
    }
}

