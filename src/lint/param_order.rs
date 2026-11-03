use crate::ast::{
    expr::{Expr, ExprKind},
    stmt::{ClassMemberKind, Param, Stmt, StmtKind},
    visitor::{walk_expr, walk_stmt, AstVisitor},
};
use crate::lexer::span::Span;
use crate::lint::reporter::{Issue, IssueLevel};
use crate::parser::parse;

/// Automatically fixes parameter orders in function, method, closure, and arrow function definitions
/// so that all required parameters precede optional parameters (parameters with default values).
///
/// Preserves relative order of required parameters, relative order of optional parameters,
/// and trailing variadic parameter if present.
pub fn fix_parameter_order(source: &str) -> String {
    let program = match parse(source) {
        Ok(p) => p,
        Err(_) => return source.to_string(),
    };

    let mut collector = ParamOrderFixCollector {
        source,
        edits: Vec::new(),
    };

    for stmt in &program.statements {
        collector.visit_stmt(stmt);
    }

    if collector.edits.is_empty() {
        return source.to_string();
    }

    // Sort edits by start span descending to replace from back to front
    collector.edits.sort_by(|a, b| b.0.start.cmp(&a.0.start));
    collector.edits.dedup_by(|a, b| a.0.start == b.0.start && a.0.end == b.0.end);

    let mut result = source.to_string();
    for (span, replacement) in collector.edits {
        if span.end <= result.len() && span.start <= span.end {
            result.replace_range(span.start..span.end, &replacement);
        }
    }

    result
}

/// Collects issues where a required parameter follows an optional parameter without modifying source.
pub fn check_parameter_order(source: &str, file_name: &str) -> Vec<Issue> {
    let program = match parse(source) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let mut collector = ParamOrderIssueCollector {
        file_name,
        issues: Vec::new(),
    };

    for stmt in &program.statements {
        collector.visit_stmt(stmt);
    }

    collector.issues
}

/// Check if a parameter list has any required parameter after an optional parameter.
pub fn has_required_after_optional(params: &[Param<'_>]) -> bool {
    let mut seen_optional = false;
    for p in params {
        if p.default_value.is_some() {
            seen_optional = true;
        } else if !p.is_variadic && seen_optional {
            return true;
        }
    }
    false
}

fn reorder_params(params: &[Param<'_>], source: &str) -> Option<(Span, String)> {
    if !has_required_after_optional(params) || params.is_empty() {
        return None;
    }

    let first = params.first()?;
    let last = params.last()?;
    let overall_span = first.span.merge(last.span);

    let mut required = Vec::new();
    let mut optional = Vec::new();
    let mut variadic = Vec::new();

    for p in params {
        if p.span.end <= source.len() && p.span.start <= p.span.end {
            let text = &source[p.span.start..p.span.end];
            if p.is_variadic {
                variadic.push(text);
            } else if p.default_value.is_some() {
                optional.push(text);
            } else {
                required.push(text);
            }
        }
    }

    let mut all = Vec::with_capacity(params.len());
    all.extend(required);
    all.extend(optional);
    all.extend(variadic);

    let replacement = all.join(", ");
    Some((overall_span, replacement))
}

struct ParamOrderFixCollector<'a> {
    source: &'a str,
    edits: Vec<(Span, String)>,
}

impl<'s, 'a> AstVisitor<'s> for ParamOrderFixCollector<'a> {
    fn visit_stmt(&mut self, stmt: &Stmt<'s>) {
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => {
                if let Some(edit) = reorder_params(&f.params, self.source) {
                    self.edits.push(edit);
                }
            }
            StmtKind::ClassDecl(c) => {
                for member in &c.members {
                    if let ClassMemberKind::Method(m) = &member.kind {
                        if let Some(edit) = reorder_params(&m.function.params, self.source) {
                            self.edits.push(edit);
                        }
                    }
                }
            }
            StmtKind::InterfaceDecl(i) => {
                for m in &i.methods {
                    if let Some(edit) = reorder_params(&m.function.params, self.source) {
                        self.edits.push(edit);
                    }
                }
            }
            StmtKind::TraitDecl(t) => {
                for member in &t.members {
                    if let ClassMemberKind::Method(m) = &member.kind {
                        if let Some(edit) = reorder_params(&m.function.params, self.source) {
                            self.edits.push(edit);
                        }
                    }
                }
            }
            StmtKind::EnumDecl(e) => {
                for m in &e.methods {
                    if let Some(edit) = reorder_params(&m.function.params, self.source) {
                        self.edits.push(edit);
                    }
                }
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &Expr<'s>) {
        match &expr.kind {
            ExprKind::Closure { params, .. } | ExprKind::ArrowFunction { params, .. } => {
                if let Some(edit) = reorder_params(params, self.source) {
                    self.edits.push(edit);
                }
            }
            ExprKind::AnonymousClass { class_decl, .. } => {
                for member in &class_decl.members {
                    if let ClassMemberKind::Method(m) = &member.kind {
                        if let Some(edit) = reorder_params(&m.function.params, self.source) {
                            self.edits.push(edit);
                        }
                    }
                }
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

struct ParamOrderIssueCollector<'a> {
    file_name: &'a str,
    issues: Vec<Issue>,
}

impl<'s, 'a> AstVisitor<'s> for ParamOrderIssueCollector<'a> {
    fn visit_stmt(&mut self, stmt: &Stmt<'s>) {
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => {
                self.check_params(&f.params, &format!("function '{}'", f.name.name));
            }
            StmtKind::ClassDecl(c) => {
                for member in &c.members {
                    if let ClassMemberKind::Method(m) = &member.kind {
                        self.check_params(
                            &m.function.params,
                            &format!("method '{}::{}'", c.name.name, m.function.name.name),
                        );
                    }
                }
            }
            StmtKind::InterfaceDecl(i) => {
                for m in &i.methods {
                    self.check_params(
                        &m.function.params,
                        &format!("method '{}::{}'", i.name.name, m.function.name.name),
                    );
                }
            }
            StmtKind::TraitDecl(t) => {
                for member in &t.members {
                    if let ClassMemberKind::Method(m) = &member.kind {
                        self.check_params(
                            &m.function.params,
                            &format!("method '{}::{}'", t.name.name, m.function.name.name),
                        );
                    }
                }
            }
            StmtKind::EnumDecl(e) => {
                for m in &e.methods {
                    self.check_params(
                        &m.function.params,
                        &format!("method '{}::{}'", e.name.name, m.function.name.name),
                    );
                }
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &Expr<'s>) {
        match &expr.kind {
            ExprKind::Closure { params, .. } => {
                self.check_params(params, "closure");
            }
            ExprKind::ArrowFunction { params, .. } => {
                self.check_params(params, "arrow function");
            }
            ExprKind::AnonymousClass { class_decl, .. } => {
                for member in &class_decl.members {
                    if let ClassMemberKind::Method(m) = &member.kind {
                        self.check_params(
                            &m.function.params,
                            &format!("method '{}'", m.function.name.name),
                        );
                    }
                }
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

impl<'a> ParamOrderIssueCollector<'a> {
    fn check_params(&mut self, params: &[Param<'_>], callable_name: &str) {
        let mut seen_optional: Option<&Param<'_>> = None;
        for p in params {
            if p.default_value.is_some() {
                if seen_optional.is_none() {
                    seen_optional = Some(p);
                }
            } else if !p.is_variadic {
                if let Some(opt) = seen_optional {
                    self.issues.push(Issue::new(
                        IssueLevel::Error,
                        "required-param-after-optional",
                        format!(
                            "Required parameter '{}' cannot follow optional parameter '{}' in {}.",
                            p.name.name, opt.name.name, callable_name
                        ),
                        self.file_name,
                        p.span.line,
                        p.span.col,
                        p.span.start,
                        p.span.end,
                    ));
                }
            }
        }
    }
}
