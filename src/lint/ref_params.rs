use std::collections::{HashMap, HashSet};

use crate::ast::{
    expr::{Expr, ExprKind, UnaryOp},
    stmt::{ClassMemberKind, Param, Stmt, StmtKind},
    types::{TypeHint, TypeHintKind},
};
use crate::parser::parse;

/// Built-in PHP array functions that mutate their array argument in-place.
const MUTATING_ARRAY_FUNCTIONS: &[&str] = &[
    "array_push",
    "array_pop",
    "array_shift",
    "array_unshift",
    "array_splice",
    "sort",
    "rsort",
    "asort",
    "arsort",
    "ksort",
    "krsort",
    "usort",
    "uasort",
    "uksort",
    "shuffle",
    "array_multisort",
    "array_walk",
    "array_walk_recursive",
];

/// In-place mutating method names on collections/objects.
const MUTATING_METHOD_NAMES: &[&str] = &[
    "push", "pop", "shift", "unshift", "add", "put", "remove", "clear", "delete",
];

/// Automatically modifies array parameters in function/method declarations to copy-by-reference (`&$param`)
/// if the array is mutated inside the function.
///
/// Also updates call sites in the same source file where variables are passed to reference parameters.
pub fn convert_mutated_array_params_to_ref(source: &str) -> String {
    let program = match parse(source) {
        Ok(p) => p,
        Err(_) => return source.to_string(),
    };

    let mut edits: Vec<(usize, String)> = Vec::new();
    let mut ref_param_indices_by_fn: HashMap<String, HashSet<usize>> = HashMap::new();

    // 1. Scan function and method declarations for mutated array parameters
    collect_declaration_ref_edits(
        &program.statements,
        source,
        &mut edits,
        &mut ref_param_indices_by_fn,
    );

    // 2. Scan call sites and add '&' if passing variables to reference parameters
    if !ref_param_indices_by_fn.is_empty() {
        collect_call_site_ref_edits(&program.statements, source, &ref_param_indices_by_fn, &mut edits);
    }

    if edits.is_empty() {
        return source.to_string();
    }

    // Apply edits from back to front by byte offset
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    edits.dedup_by(|a, b| a.0 == b.0);

    let mut result = source.to_string();
    for (offset, text) in edits {
        if offset <= result.len() {
            result.insert_str(offset, &text);
        }
    }

    result
}

/// Recursively traverses statements looking for function, method, and closure declarations.
fn collect_declaration_ref_edits(
    stmts: &[Stmt<'_>],
    source: &str,
    edits: &mut Vec<(usize, String)>,
    ref_param_indices_by_fn: &mut HashMap<String, HashSet<usize>>,
) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::FunctionDecl(func) => {
                let fn_name = func.name.name.to_string();
                if let Some(body) = &func.body {
                    inspect_params_and_body(
                        Some(&fn_name),
                        &func.params,
                        body,
                        source,
                        edits,
                        ref_param_indices_by_fn,
                    );
                    collect_declaration_ref_edits(body, source, edits, ref_param_indices_by_fn);
                }
            }
            StmtKind::ClassDecl(cls) => {
                for member in &cls.members {
                    if let ClassMemberKind::Method(method) = &member.kind {
                        let method_name = method.function.name.name.to_string();
                        if let Some(body) = &method.function.body {
                            inspect_params_and_body(
                                Some(&method_name),
                                &method.function.params,
                                body,
                                source,
                                edits,
                                ref_param_indices_by_fn,
                            );
                            collect_declaration_ref_edits(body, source, edits, ref_param_indices_by_fn);
                        }
                    }
                }
            }
            StmtKind::TraitDecl(tr) => {
                for member in &tr.members {
                    if let ClassMemberKind::Method(method) = &member.kind {
                        let method_name = method.function.name.name.to_string();
                        if let Some(body) = &method.function.body {
                            inspect_params_and_body(
                                Some(&method_name),
                                &method.function.params,
                                body,
                                source,
                                edits,
                                ref_param_indices_by_fn,
                            );
                            collect_declaration_ref_edits(body, source, edits, ref_param_indices_by_fn);
                        }
                    }
                }
            }
            StmtKind::Block(stmts) => {
                collect_declaration_ref_edits(stmts, source, edits, ref_param_indices_by_fn);
            }
            StmtKind::If {
                then_branch,
                elseif_branches,
                else_branch,
                ..
            } => {
                collect_declaration_in_stmt(then_branch, source, edits, ref_param_indices_by_fn);
                for branch in elseif_branches {
                    collect_declaration_in_stmt(&branch.body, source, edits, ref_param_indices_by_fn);
                }
                if let Some(eb) = else_branch {
                    collect_declaration_in_stmt(eb, source, edits, ref_param_indices_by_fn);
                }
            }
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::Foreach { body, .. }
            | StmtKind::ForIn { body, .. }
            | StmtKind::Loop { body } => {
                collect_declaration_in_stmt(body, source, edits, ref_param_indices_by_fn);
            }
            StmtKind::TryCatch {
                try_block,
                catches,
                finally_block,
            } => {
                collect_declaration_ref_edits(try_block, source, edits, ref_param_indices_by_fn);
                for c in catches {
                    collect_declaration_ref_edits(&c.body, source, edits, ref_param_indices_by_fn);
                }
                if let Some(fb) = finally_block {
                    collect_declaration_ref_edits(fb, source, edits, ref_param_indices_by_fn);
                }
            }
            StmtKind::Expr(expr) => {
                collect_closure_declaration_ref_edits(expr, source, edits, ref_param_indices_by_fn);
            }
            StmtKind::Namespace {
                body: Some(body), ..
            } => {
                collect_declaration_ref_edits(body, source, edits, ref_param_indices_by_fn);
            }
            _ => {}
        }
    }
}

fn collect_declaration_in_stmt(
    stmt: &Stmt<'_>,
    source: &str,
    edits: &mut Vec<(usize, String)>,
    ref_param_indices_by_fn: &mut HashMap<String, HashSet<usize>>,
) {
    if let StmtKind::Block(stmts) = &stmt.kind {
        collect_declaration_ref_edits(stmts, source, edits, ref_param_indices_by_fn);
    } else {
        collect_declaration_ref_edits(std::slice::from_ref(stmt), source, edits, ref_param_indices_by_fn);
    }
}

fn collect_closure_declaration_ref_edits(
    expr: &Expr<'_>,
    source: &str,
    edits: &mut Vec<(usize, String)>,
    ref_param_indices_by_fn: &mut HashMap<String, HashSet<usize>>,
) {
    if let ExprKind::Closure { params, body, .. } = &expr.kind {
        inspect_params_and_body(None, params, body, source, edits, ref_param_indices_by_fn);
        collect_declaration_ref_edits(body, source, edits, ref_param_indices_by_fn);
    }
}

/// Inspect parameters of a function or method. If an array parameter is mutated in `body`, mark it for `&`.
fn inspect_params_and_body(
    fn_name_opt: Option<&str>,
    params: &[Param<'_>],
    body: &[Stmt<'_>],
    source: &str,
    edits: &mut Vec<(usize, String)>,
    ref_param_indices_by_fn: &mut HashMap<String, HashSet<usize>>,
) {
    for (idx, param) in params.iter().enumerate() {
        if param.is_reference {
            // Already a reference: register in index map
            if let Some(fn_name) = fn_name_opt {
                ref_param_indices_by_fn
                    .entry(fn_name.to_string())
                    .or_default()
                    .insert(idx);
            }
            continue;
        }

        if param.is_variadic {
            continue;
        }

        let is_array_like = is_array_like_param(param, body);
        if !is_array_like {
            continue;
        }

        let is_mutated = is_param_mutated(param.name.name, body);
        if is_mutated {
            let offset = param.name.span.start;
            // Verify that '&' is not already right before the variable name
            let prefix = &source[..offset.min(source.len())];
            if !prefix.ends_with('&') {
                edits.push((offset, "&".to_string()));
            }

            if let Some(fn_name) = fn_name_opt {
                ref_param_indices_by_fn
                    .entry(fn_name.to_string())
                    .or_default()
                    .insert(idx);
            }
        }
    }
}

/// Checks if a parameter is an array, list, dict, hashMap, set, map, or untyped array.
pub fn is_array_like_param(param: &Param<'_>, body: &[Stmt<'_>]) -> bool {
    if is_array_type_hint(&param.type_hint) {
        return true;
    }
    if param.type_hint.is_none() {
        return is_used_as_array_in_body(param.name.name, body);
    }
    false
}

/// Checks if a type hint corresponds to an array or collection type.
pub fn is_array_type_hint(hint_opt: &Option<TypeHint<'_>>) -> bool {
    let hint = match hint_opt {
        Some(h) => h,
        None => return false,
    };
    check_hint_kind(&hint.kind)
}

fn check_hint_kind(kind: &TypeHintKind<'_>) -> bool {
    match kind {
        TypeHintKind::Array => true,
        TypeHintKind::Named(ident) => {
            let name = ident.name.to_ascii_lowercase();
            matches!(
                name.as_str(),
                "array"
                    | "list"
                    | "dict"
                    | "hashmap"
                    | "hash_map"
                    | "map"
                    | "set"
                    | "iterable"
                    | "collection"
            )
        }
        TypeHintKind::Nullable(inner) => check_hint_kind(&inner.kind),
        TypeHintKind::Union(types) => types.iter().any(|t| check_hint_kind(&t.kind)),
        _ => false,
    }
}

/// Check if an untyped parameter is operated on as an array (e.g. index write, unset index, array functions).
fn is_used_as_array_in_body(var_name: &str, body: &[Stmt<'_>]) -> bool {
    for stmt in body {
        if check_stmt_for_array_usage(stmt, var_name) {
            return true;
        }
    }
    false
}

fn check_stmt_for_array_usage(stmt: &Stmt<'_>, var_name: &str) -> bool {
    match &stmt.kind {
        StmtKind::Expr(expr) => check_expr_for_array_usage(expr, var_name),
        StmtKind::Block(stmts) => stmts.iter().any(|s| check_stmt_for_array_usage(s, var_name)),
        StmtKind::If {
            then_branch,
            elseif_branches,
            else_branch,
            ..
        } => {
            check_stmt_for_array_usage(then_branch, var_name)
                || elseif_branches.iter().any(|b| check_stmt_for_array_usage(&b.body, var_name))
                || else_branch.as_ref().is_some_and(|eb| check_stmt_for_array_usage(eb, var_name))
        }
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::Foreach { body, .. }
        | StmtKind::ForIn { body, .. }
        | StmtKind::Loop { body } => check_stmt_for_array_usage(body, var_name),
        StmtKind::Switch { cases, .. } => cases
            .iter()
            .any(|c| c.body.iter().any(|s| check_stmt_for_array_usage(s, var_name))),
        StmtKind::TryCatch {
            try_block,
            catches,
            finally_block,
        } => {
            try_block.iter().any(|s| check_stmt_for_array_usage(s, var_name))
                || catches
                    .iter()
                    .any(|c| c.body.iter().any(|s| check_stmt_for_array_usage(s, var_name)))
                || finally_block
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| check_stmt_for_array_usage(s, var_name)))
        }
        StmtKind::Unset(exprs) => exprs.iter().any(|e| {
            if let ExprKind::ArrayIndex { array, .. } = &e.kind {
                get_root_variable_name(array) == Some(var_name)
            } else {
                false
            }
        }),
        _ => false,
    }
}

fn check_expr_for_array_usage(expr: &Expr<'_>, var_name: &str) -> bool {
    match &expr.kind {
        ExprKind::Assign { target, .. } => {
            if let ExprKind::ArrayIndex { array, .. } = &target.kind {
                get_root_variable_name(array) == Some(var_name)
            } else {
                false
            }
        }
        ExprKind::Call { callee, args } => {
            if let ExprKind::Ident(id) = &callee.kind {
                if MUTATING_ARRAY_FUNCTIONS.contains(&id.name.to_ascii_lowercase().as_str()) {
                    if let Some(first_arg) = args.first() {
                        return get_root_variable_name(&first_arg.value) == Some(var_name);
                    }
                }
            }
            false
        }
        _ => false,
    }
}

/// Recursively checks if `var_name` is mutated inside `body`.
pub fn is_param_mutated(var_name: &str, body: &[Stmt<'_>]) -> bool {
    for stmt in body {
        if check_stmt_for_mutation(stmt, var_name) {
            return true;
        }
    }
    false
}

fn check_stmt_for_mutation(stmt: &Stmt<'_>, var_name: &str) -> bool {
    match &stmt.kind {
        StmtKind::Expr(expr) => check_expr_for_mutation(expr, var_name),
        StmtKind::Block(stmts) => stmts.iter().any(|s| check_stmt_for_mutation(s, var_name)),
        StmtKind::If {
            then_branch,
            elseif_branches,
            else_branch,
            ..
        } => {
            check_stmt_for_mutation(then_branch, var_name)
                || elseif_branches.iter().any(|b| check_stmt_for_mutation(&b.body, var_name))
                || else_branch.as_ref().is_some_and(|eb| check_stmt_for_mutation(eb, var_name))
        }
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::Foreach { body, .. }
        | StmtKind::ForIn { body, .. }
        | StmtKind::Loop { body } => check_stmt_for_mutation(body, var_name),
        StmtKind::Switch { cases, .. } => cases
            .iter()
            .any(|c| c.body.iter().any(|s| check_stmt_for_mutation(s, var_name))),
        StmtKind::TryCatch {
            try_block,
            catches,
            finally_block,
        } => {
            try_block.iter().any(|s| check_stmt_for_mutation(s, var_name))
                || catches
                    .iter()
                    .any(|c| c.body.iter().any(|s| check_stmt_for_mutation(s, var_name)))
                || finally_block
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| check_stmt_for_mutation(s, var_name)))
        }
        StmtKind::Unset(exprs) => exprs.iter().any(|e| get_root_variable_name(e) == Some(var_name)),
        _ => false,
    }
}

fn check_expr_for_mutation(expr: &Expr<'_>, var_name: &str) -> bool {
    match &expr.kind {
        // Direct assignment or index assignment: $x = ..., $x[...] = ...
        ExprKind::Assign { target, .. } => {
            if get_root_variable_name(target) == Some(var_name) {
                return true;
            }
            if let ExprKind::List(elements) = &target.kind {
                for el in elements.iter().flatten() {
                    if get_root_variable_name(&el.value) == Some(var_name) {
                        return true;
                    }
                }
            }
            false
        }
        // Increment/decrement: $x++, ++$x[0]
        ExprKind::Unary { op, expr: inner, .. } => {
            if matches!(
                op,
                UnaryOp::PreInc | UnaryOp::PostInc | UnaryOp::PreDec | UnaryOp::PostDec
            ) {
                get_root_variable_name(inner) == Some(var_name)
            } else {
                false
            }
        }
        // Function call: mutating PHP functions or pass-by-reference
        ExprKind::Call { callee, args } => {
            if let ExprKind::Ident(id) = &callee.kind {
                if MUTATING_ARRAY_FUNCTIONS.contains(&id.name.to_ascii_lowercase().as_str()) {
                    if let Some(first_arg) = args.first() {
                        if get_root_variable_name(&first_arg.value) == Some(var_name) {
                            return true;
                        }
                    }
                }
            }
            // Check if passed as reference argument: foo(&$x)
            for arg in args {
                if arg.is_reference && get_root_variable_name(&arg.value) == Some(var_name) {
                    return true;
                }
            }
            false
        }
        // Method call: mutating method or pass-by-reference
        ExprKind::MethodCall {
            object,
            method,
            args,
            ..
        } => {
            if get_root_variable_name(object) == Some(var_name) {
                if MUTATING_METHOD_NAMES.contains(&method.name.to_ascii_lowercase().as_str()) {
                    return true;
                }
            }
            for arg in args {
                if arg.is_reference && get_root_variable_name(&arg.value) == Some(var_name) {
                    return true;
                }
            }
            false
        }
        // Static call: pass-by-reference
        ExprKind::StaticCall { args, .. } => {
            for arg in args {
                if arg.is_reference && get_root_variable_name(&arg.value) == Some(var_name) {
                    return true;
                }
            }
            false
        }
        // Binary or other nested expressions (e.g. inside sub-expressions)
        ExprKind::Binary { left, right, .. } => {
            check_expr_for_mutation(left, var_name) || check_expr_for_mutation(right, var_name)
        }
        ExprKind::Ternary {
            condition,
            if_true,
            if_false,
        } => {
            check_expr_for_mutation(condition, var_name)
                || if_true.as_ref().is_some_and(|e| check_expr_for_mutation(e, var_name))
                || check_expr_for_mutation(if_false, var_name)
        }
        _ => false,
    }
}

/// Recursively extracts the root variable name from expressions like `$x`, `$x[0]`, `$x['key'][1]`, `$x->prop`.
fn get_root_variable_name<'a>(expr: &'a Expr<'_>) -> Option<&'a str> {
    match &expr.kind {
        ExprKind::Variable(v) => Some(v.name),
        ExprKind::ArrayIndex { array, .. } => get_root_variable_name(array),
        ExprKind::PropertyFetch { object, .. } => get_root_variable_name(object),
        _ => None,
    }
}

/// Collect edits for call sites that invoke functions with reference parameters.
fn collect_call_site_ref_edits(
    stmts: &[Stmt<'_>],
    source: &str,
    ref_param_indices_by_fn: &HashMap<String, HashSet<usize>>,
    edits: &mut Vec<(usize, String)>,
) {
    for stmt in stmts {
        collect_call_site_in_stmt(stmt, source, ref_param_indices_by_fn, edits);
    }
}

fn collect_call_site_in_stmt(
    stmt: &Stmt<'_>,
    source: &str,
    ref_param_indices_by_fn: &HashMap<String, HashSet<usize>>,
    edits: &mut Vec<(usize, String)>,
) {
    match &stmt.kind {
        StmtKind::Expr(expr) => {
            collect_call_site_in_expr(expr, source, ref_param_indices_by_fn, edits);
        }
        StmtKind::Block(stmts) => {
            for s in stmts {
                collect_call_site_in_stmt(s, source, ref_param_indices_by_fn, edits);
            }
        }
        StmtKind::If {
            then_branch,
            elseif_branches,
            else_branch,
            ..
        } => {
            collect_call_site_in_stmt(then_branch, source, ref_param_indices_by_fn, edits);
            for branch in elseif_branches {
                collect_call_site_in_stmt(&branch.body, source, ref_param_indices_by_fn, edits);
            }
            if let Some(eb) = else_branch {
                collect_call_site_in_stmt(eb, source, ref_param_indices_by_fn, edits);
            }
        }
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::Foreach { body, .. }
        | StmtKind::ForIn { body, .. }
        | StmtKind::Loop { body } => {
            collect_call_site_in_stmt(body, source, ref_param_indices_by_fn, edits);
        }
        StmtKind::FunctionDecl(f) => {
            if let Some(body) = &f.body {
                for s in body {
                    collect_call_site_in_stmt(s, source, ref_param_indices_by_fn, edits);
                }
            }
        }
        StmtKind::ClassDecl(c) => {
            for member in &c.members {
                if let ClassMemberKind::Method(m) = &member.kind {
                    if let Some(body) = &m.function.body {
                        for s in body {
                            collect_call_site_in_stmt(s, source, ref_param_indices_by_fn, edits);
                        }
                    }
                }
            }
        }
        StmtKind::Return(Some(expr)) => {
            collect_call_site_in_expr(expr, source, ref_param_indices_by_fn, edits);
        }
        StmtKind::Echo(exprs) => {
            for e in exprs {
                collect_call_site_in_expr(e, source, ref_param_indices_by_fn, edits);
            }
        }
        _ => {}
    }
}

fn collect_call_site_in_expr(
    expr: &Expr<'_>,
    source: &str,
    ref_param_indices_by_fn: &HashMap<String, HashSet<usize>>,
    edits: &mut Vec<(usize, String)>,
) {
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            if let ExprKind::Ident(id) = &callee.kind {
                if let Some(ref_indices) = ref_param_indices_by_fn.get(id.name) {
                    for (idx, arg) in args.iter().enumerate() {
                        if ref_indices.contains(&idx) && !arg.is_reference {
                            if matches!(arg.value.kind, ExprKind::Variable(_)) {
                                let offset = arg.span.start;
                                let prefix = &source[..offset.min(source.len())];
                                if !prefix.ends_with('&') {
                                    edits.push((offset, "&".to_string()));
                                }
                            }
                        }
                    }
                }
            }
            collect_call_site_in_expr(callee, source, ref_param_indices_by_fn, edits);
            for arg in args {
                collect_call_site_in_expr(&arg.value, source, ref_param_indices_by_fn, edits);
            }
        }
        ExprKind::Assign { target, value, .. } => {
            collect_call_site_in_expr(target, source, ref_param_indices_by_fn, edits);
            collect_call_site_in_expr(value, source, ref_param_indices_by_fn, edits);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_call_site_in_expr(left, source, ref_param_indices_by_fn, edits);
            collect_call_site_in_expr(right, source, ref_param_indices_by_fn, edits);
        }
        ExprKind::Unary { expr: inner, .. } => {
            collect_call_site_in_expr(inner, source, ref_param_indices_by_fn, edits);
        }
        ExprKind::MethodCall { object, args, .. } => {
            collect_call_site_in_expr(object, source, ref_param_indices_by_fn, edits);
            for arg in args {
                collect_call_site_in_expr(&arg.value, source, ref_param_indices_by_fn, edits);
            }
        }
        _ => {}
    }
}
