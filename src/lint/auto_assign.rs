use crate::ast::{
    expr::{Expr, ExprKind},
    stmt::{ClassMemberKind, Stmt, StmtKind},
};
use crate::parser::parse;

/// Known array and collection transformation methods that return modified collections.
const COLLECTION_TRANSFORM_METHODS: &[&str] = &[
    "map",
    "filter",
    "reduce",
    "sort",
    "rsort",
    "asort",
    "arsort",
    "ksort",
    "krsort",
    "usort",
    "uasort",
    "uksort",
    "reverse",
    "unique",
    "chunk",
    "flatten",
    "concat",
    "slice",
    "splice",
    "take",
    "skip",
    "merge",
    "diff",
    "intersect",
    "keys",
    "values",
    "flip",
    "pad",
    "fill",
    "shuffle",
    "combine",
    "transform",
    "pipe",
    "flatmap",
    "flat_map",
    "collapse",
    "groupby",
    "group_by",
    "keyby",
    "key_by",
    "pluck",
    "reject",
    "prepend",
];

/// Automatically assigns unassigned array/collection method calls back to the target variable.
///
/// For example:
/// - `$x->map();` becomes `$x = $x->map();`
/// - `Arr::list($z)->map();` becomes `$z = Arr::list($z)->map();`
pub fn auto_assign_collection_calls(source: &str) -> String {
    let program = match parse(source) {
        Ok(p) => p,
        Err(_) => return source.to_string(),
    };

    let mut edits: Vec<(usize, String)> = Vec::new();

    collect_auto_assign_edits(&program.statements, source, &mut edits);

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

/// Recursively collect statements that qualify for auto-assignment.
fn collect_auto_assign_edits(stmts: &[Stmt<'_>], source: &str, edits: &mut Vec<(usize, String)>) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Expr(expr) => {
                if !matches!(expr.kind, ExprKind::Assign { .. }) {
                    if let Some(target_var) = get_auto_assign_target(expr) {
                        let offset = expr.span.start;
                        let prefix = &source[..offset.min(source.len())];
                        let trimmed_prefix = prefix.trim_end();
                        if !trimmed_prefix.ends_with('=') {
                            edits.push((offset, format!("{} = ", target_var)));
                        }
                    }
                }
                // Check if the expression contains closures
                collect_auto_assign_in_expr(expr, source, edits);
            }
            StmtKind::Block(inner_stmts) => collect_auto_assign_edits(inner_stmts, source, edits),
            StmtKind::If {
                then_branch,
                elseif_branches,
                else_branch,
                ..
            } => {
                collect_auto_assign_in_stmt(then_branch, source, edits);
                for branch in elseif_branches {
                    collect_auto_assign_in_stmt(&branch.body, source, edits);
                }
                if let Some(eb) = else_branch {
                    collect_auto_assign_in_stmt(eb, source, edits);
                }
            }
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::Foreach { body, .. }
            | StmtKind::ForIn { body, .. }
            | StmtKind::Loop { body } => {
                collect_auto_assign_in_stmt(body, source, edits);
            }
            StmtKind::Switch { cases, .. } => {
                for case in cases {
                    collect_auto_assign_edits(&case.body, source, edits);
                }
            }
            StmtKind::TryCatch {
                try_block,
                catches,
                finally_block,
            } => {
                collect_auto_assign_edits(try_block, source, edits);
                for c in catches {
                    collect_auto_assign_edits(&c.body, source, edits);
                }
                if let Some(fb) = finally_block {
                    collect_auto_assign_edits(fb, source, edits);
                }
            }
            StmtKind::FunctionDecl(f) => {
                if let Some(body) = &f.body {
                    collect_auto_assign_edits(body, source, edits);
                }
            }
            StmtKind::ClassDecl(c) => {
                for member in &c.members {
                    if let ClassMemberKind::Method(m) = &member.kind {
                        if let Some(body) = &m.function.body {
                            collect_auto_assign_edits(body, source, edits);
                        }
                    }
                }
            }
            StmtKind::TraitDecl(t) => {
                for member in &t.members {
                    if let ClassMemberKind::Method(m) = &member.kind {
                        if let Some(body) = &m.function.body {
                            collect_auto_assign_edits(body, source, edits);
                        }
                    }
                }
            }
            StmtKind::Namespace {
                body: Some(body), ..
            } => {
                collect_auto_assign_edits(body, source, edits);
            }
            _ => {}
        }
    }
}

fn collect_auto_assign_in_stmt(stmt: &Stmt<'_>, source: &str, edits: &mut Vec<(usize, String)>) {
    if let StmtKind::Block(stmts) = &stmt.kind {
        collect_auto_assign_edits(stmts, source, edits);
    } else {
        collect_auto_assign_edits(std::slice::from_ref(stmt), source, edits);
    }
}

fn collect_auto_assign_in_expr(expr: &Expr<'_>, source: &str, edits: &mut Vec<(usize, String)>) {
    if let ExprKind::Closure { body, .. } = &expr.kind {
        collect_auto_assign_edits(body, source, edits);
    }
}

/// Identifies if an expression is an unassigned collection method call and returns the target variable name.
pub fn get_auto_assign_target(expr: &Expr<'_>) -> Option<String> {
    if let ExprKind::MethodCall { object, method, .. } = &expr.kind {
        let mut methods = vec![method.name];
        let mut curr = object.as_ref();
        while let ExprKind::MethodCall {
            object: inner,
            method: m,
            ..
        } = &curr.kind
        {
            methods.push(m.name);
            curr = inner.as_ref();
        }

        // Case 1: Arr::list($z)->map() or \Arr::list($z)->map()
        if let ExprKind::StaticCall {
            target,
            method: static_method,
            args,
        } = &curr.kind
        {
            let is_arr_facade = match &target.kind {
                ExprKind::Ident(id) => {
                    id.name.eq_ignore_ascii_case("Arr")
                        || id.name.eq_ignore_ascii_case("\\Arr")
                        || id.name.ends_with("\\Arr")
                }
                _ => false,
            };
            if is_arr_facade {
                let m_name = static_method.name.to_ascii_lowercase();
                if matches!(
                    m_name.as_str(),
                    "list" | "dict" | "set" | "hashmap" | "map" | "from" | "wrap"
                ) {
                    if let Some(first_arg) = args.first() {
                        if let ExprKind::Variable(var) = &first_arg.value.kind {
                            return Some(var.name.to_string());
                        }
                    }
                }
            }
        }

        // Case 2: $x->map()
        if let ExprKind::Variable(var) = &curr.kind {
            let has_collection_method = methods.iter().any(|m| {
                let lower = m.to_ascii_lowercase();
                COLLECTION_TRANSFORM_METHODS.contains(&lower.as_str())
            });
            if has_collection_method {
                return Some(var.name.to_string());
            }
        }
    }
    None
}
