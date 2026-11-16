use crate::ast::{
    expr::{Expr, ExprKind},
    stmt::{ClassMemberKind, Param, Stmt, StmtKind},
    visitor::{walk_expr, walk_stmt, AstVisitor},
};
use crate::lexer::span::Span;
use crate::lint::reporter::{Issue, IssueLevel};
use crate::parser::parse_allowing_named_args;

/// Information about a function whose parameter declaration order was fixed.
#[derive(Debug, Clone)]
pub struct ReorderedFunctionInfo {
    pub name: String,
    pub old_signature: String,
    pub old_params: Vec<String>,
    pub new_params: Vec<String>,
    pub line: usize,
}

/// Automatically fixes parameter orders in function, method, closure, and arrow function definitions
/// so that all required parameters precede optional parameters (parameters with default values),
/// optionally inserting `//before: <old_sig>` above declarations,
/// returning the transformed code, modified count, and detailed reordered function metadata.
pub fn fix_parameter_order_detailed(
    source: &str,
    add_before_comment: bool,
) -> (String, usize, Vec<ReorderedFunctionInfo>) {
    let program = match parse_allowing_named_args(source) {
        Ok(p) => p,
        Err(_) => return (source.to_string(), 0, Vec::new()),
    };

    let mut collector = ParamOrderFixCollector {
        source,
        add_before_comment,
        edits: Vec::new(),
        reordered_fns: Vec::new(),
    };

    for stmt in &program.statements {
        collector.visit_stmt(stmt);
    }

    if collector.edits.is_empty() {
        return (source.to_string(), 0, Vec::new());
    }

    // Sort edits by start span descending to replace from back to front
    collector
        .edits
        .sort_by(|a, b| b.0.start.cmp(&a.0.start).then_with(|| b.0.end.cmp(&a.0.end)));
    collector
        .edits
        .dedup_by(|a, b| a.0.start == b.0.start && a.0.end == b.0.end && a.1 == b.1);

    let count = if !collector.reordered_fns.is_empty() {
        collector.reordered_fns.len()
    } else {
        collector.edits.len()
    };

    let mut result = source.to_string();
    for (span, replacement) in collector.edits {
        if span.end <= result.len() && span.start <= span.end {
            result.replace_range(span.start..span.end, &replacement);
        }
    }

    (result, count, collector.reordered_fns)
}

/// Automatically fixes parameter orders in function, method, closure, and arrow function definitions
/// so that all required parameters precede optional parameters (parameters with default values),
/// returning the transformed code along with the count of parameter lists modified.
pub fn fix_parameter_order_with_count(source: &str) -> (String, usize) {
    let (res, count, _) = fix_parameter_order_detailed(source, true);
    (res, count)
}

/// Automatically fixes parameter orders in function, method, closure, and arrow function definitions
/// so that all required parameters precede optional parameters (parameters with default values).
///
/// Preserves relative order of required parameters, relative order of optional parameters,
/// and trailing variadic parameter if present.
pub fn fix_parameter_order(source: &str) -> String {
    fix_parameter_order_with_count(source).0
}

/// Collects issues where a required parameter follows an optional parameter without modifying source.
pub fn check_parameter_order(source: &str, file_name: &str) -> Vec<Issue> {
    let program = match parse_allowing_named_args(source) {
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

fn reorder_params(
    params: &[Param<'_>],
    source: &str,
) -> Option<(Span, String, Vec<String>, Vec<String>)> {
    if !has_required_after_optional(params) || params.is_empty() {
        return None;
    }

    let first = params.first()?;
    let last = params.last()?;
    let overall_span = first.span.merge(last.span);

    let mut old_param_names = Vec::new();
    let mut required_before = Vec::new();
    let mut optional = Vec::new();
    let mut required_after = Vec::new();
    let mut variadic = Vec::new();

    let mut seen_optional = false;
    for p in params {
        let p_name = p.name.name.to_string();
        old_param_names.push(p_name.clone());

        if p.span.end <= source.len() && p.span.start <= p.span.end {
            let text = &source[p.span.start..p.span.end];
            if p.is_variadic {
                variadic.push((p_name, text));
            } else if p.default_value.is_some() {
                seen_optional = true;
                optional.push((p_name, text));
            } else if seen_optional {
                required_after.push((p_name, text));
            } else {
                required_before.push((p_name, text));
            }
        }
    }

    let mut all_texts = Vec::with_capacity(params.len());
    let mut new_param_names = Vec::with_capacity(params.len());

    // Shift required_after parameters first, then required_before, optional, variadic
    for (name, text) in required_after {
        new_param_names.push(name);
        all_texts.push(text);
    }
    for (name, text) in required_before {
        new_param_names.push(name);
        all_texts.push(text);
    }
    for (name, text) in optional {
        new_param_names.push(name);
        all_texts.push(text);
    }
    for (name, text) in variadic {
        new_param_names.push(name);
        all_texts.push(text);
    }

    let replacement = all_texts.join(", ");
    Some((overall_span, replacement, old_param_names, new_param_names))
}

struct ParamOrderFixCollector<'a> {
    source: &'a str,
    add_before_comment: bool,
    edits: Vec<(Span, String)>,
    reordered_fns: Vec<ReorderedFunctionInfo>,
}

impl<'a> ParamOrderFixCollector<'a> {
    fn record_declaration_fix(
        &mut self,
        decl_span: Span,
        name: &str,
        params: &[Param<'_>],
    ) {
        if let Some((overall_span, replacement, old_params, new_params)) =
            reorder_params(params, self.source)
        {
            self.edits.push((overall_span, replacement));

            let decl_span_start = decl_span.start;
            let line_start = self.source[..decl_span_start]
                .rfind('\n')
                .map(|i| i + 1)
                .unwrap_or(0);
            let indent_ws = self.source[line_start..decl_span_start]
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect::<String>();

            let body_pos = self.source[decl_span_start..]
                .find('{')
                .or_else(|| self.source[decl_span_start..].find(';'))
                .map(|p| decl_span_start + p)
                .unwrap_or(decl_span_start);
            let old_sig = self.source[decl_span_start..body_pos].trim();

            let prev_has_before = if line_start > 0 {
                let prev_slice = &self.source[..line_start - 1];
                let prev_start = prev_slice.rfind('\n').map(|i| i + 1).unwrap_or(0);
                self.source[prev_start..line_start - 1]
                    .trim()
                    .starts_with("//before:")
            } else {
                false
            };

            if self.add_before_comment && !prev_has_before && !old_sig.is_empty() {
                let comment_text = format!("{}//before: {}\n", indent_ws, old_sig);
                self.edits.push((
                    Span::new(decl_span.file_id, line_start, line_start, decl_span.line, 1),
                    comment_text,
                ));
            }

            self.reordered_fns.push(ReorderedFunctionInfo {
                name: name.to_string(),
                old_signature: old_sig.to_string(),
                old_params,
                new_params,
                line: decl_span.line,
            });
        }
    }
}

impl<'s, 'a> AstVisitor<'s> for ParamOrderFixCollector<'a> {
    fn visit_stmt(&mut self, stmt: &Stmt<'s>) {
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => {
                self.record_declaration_fix(stmt.span, f.name.name, &f.params);
            }
            StmtKind::ClassDecl(c) => {
                for member in &c.members {
                    if let ClassMemberKind::Method(m) = &member.kind {
                        self.record_declaration_fix(
                            member.span,
                            m.function.name.name,
                            &m.function.params,
                        );
                    }
                }
            }
            StmtKind::InterfaceDecl(i) => {
                for m in &i.methods {
                    self.record_declaration_fix(
                        m.function.span,
                        m.function.name.name,
                        &m.function.params,
                    );
                }
            }
            StmtKind::TraitDecl(t) => {
                for member in &t.members {
                    if let ClassMemberKind::Method(m) = &member.kind {
                        self.record_declaration_fix(
                            member.span,
                            m.function.name.name,
                            &m.function.params,
                        );
                    }
                }
            }
            StmtKind::EnumDecl(e) => {
                for m in &e.methods {
                    self.record_declaration_fix(
                        m.function.span,
                        m.function.name.name,
                        &m.function.params,
                    );
                }
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &Expr<'s>) {
        match &expr.kind {
            ExprKind::Closure { params, .. } | ExprKind::ArrowFunction { params, .. } => {
                if let Some((overall_span, replacement, _, _)) = reorder_params(params, self.source) {
                    self.edits.push((overall_span, replacement));
                }
            }
            ExprKind::AnonymousClass { class_decl, .. } => {
                for member in &class_decl.members {
                    if let ClassMemberKind::Method(m) = &member.kind {
                        self.record_declaration_fix(
                            member.span,
                            m.function.name.name,
                            &m.function.params,
                        );
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
