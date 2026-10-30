use crate::ast::expr::{Expr, ExprKind};
use crate::ast::stmt::Stmt;
use crate::ast::types::Variable;
use crate::diagnostic::Diagnostic;
use crate::expand::builder::AstBuilder;
use crate::expand::context::ExpandCtx;
use crate::expand::expansion::Expansion;
use crate::expand::site::ExpansionSite;
use crate::expand::template::TemplateNode;
use crate::expand::traits::Expander;

/// Helper to lower template nodes recursively into RHP statements.
pub fn lower_template_nodes(
    nodes: &[TemplateNode<'static>],
    ctx: &mut ExpandCtx,
    registry: &crate::expand::registry::ExpanderRegistry,
) -> Result<Vec<Stmt<'static>>, Diagnostic> {
    let mut stmts = Vec::new();
    for node in nodes {
        match node {
            TemplateNode::Text { content, span } => {
                let mut builder = AstBuilder::new(ctx, *span, "text");
                let s_expr = builder.string_expr(content);
                stmts.push(builder.echo_stmt(vec![s_expr]));
            }
            TemplateNode::Echo { expr, raw, span } => {
                let mut builder = AstBuilder::new(ctx, *span, "echo");
                if *raw {
                    stmts.push(builder.echo_stmt(vec![expr.clone()]));
                } else {
                    // HTML escaping: Html::escape($expr)
                    let escaped = builder.static_call_expr("Html", "escape", vec![expr.clone()]);
                    stmts.push(builder.echo_stmt(vec![escaped]));
                }
            }
            TemplateNode::Directive(dir) => {
                let dname = dir.name.as_str();
                let handler = registry.get_directive(dname).ok_or_else(|| {
                    ctx.error(
                        "RHP0501",
                        format!("unknown template directive `@{}`", dname),
                        dir.span,
                    )
                })?;

                let site = ExpansionSite::Directive {
                    name: dname,
                    args: &dir.args,
                    raw_args: dir.raw_args,
                    span: dir.span,
                    body: &dir.body,
                };

                let expansion = handler.expand(ctx, site)?;
                match expansion {
                    Expansion::Statements(s) => stmts.extend(s),
                    Expansion::Empty => {}
                    _ => {
                        return Err(ctx.error(
                            "RHP0505",
                            format!("directive `@{}` returned unexpected non-statement expansion", dname),
                            dir.span,
                        ));
                    }
                }
            }
        }
    }
    Ok(stmts)
}

/// Built-in `@if($cond) ... @elseif($cond) ... @else ... @endif` directive expander.
pub struct IfExpander;

impl Expander for IfExpander {
    fn name(&self) -> &str {
        "if"
    }

    fn expand(&self, ctx: &mut ExpandCtx, site: ExpansionSite) -> Result<Expansion, Diagnostic> {
        let (raw_args, span, body) = match site {
            ExpansionSite::Directive {
                raw_args,
                span,
                body,
                ..
            } => (raw_args, span, body),
            _ => {
                return Err(ctx.error("RHP0506", "`@if` can only be used as a template directive", site.span()));
            }
        };

        if raw_args.trim().is_empty() {
            return Err(ctx.error("RHP0502", "directive `@if` requires a condition expression in parentheses", span));
        }

        let interned_cond = ctx.intern(raw_args);
        let cond = crate::parser::parse_expr(interned_cond).map_err(|e| {
            ctx.error("RHP0102", format!("failed to parse `@if` condition: {}", e), span)
        })?;

        let reg = crate::expand::registry::ExpanderRegistry::default_with_builtins();
        let static_body: &[TemplateNode<'static>] = unsafe { std::mem::transmute(body) };
        let then_stmts = lower_template_nodes(static_body, ctx, &reg)?;

        let mut builder = AstBuilder::new(ctx, span, "if");
        let stmt = builder.if_stmt(cond, then_stmts, Vec::new(), None);
        Ok(Expansion::Statements(vec![stmt]))
    }
}

/// Built-in `@for($init; $cond; $update) ... @endfor` directive expander.
pub struct ForExpander;

impl Expander for ForExpander {
    fn name(&self) -> &str {
        "for"
    }

    fn expand(&self, ctx: &mut ExpandCtx, site: ExpansionSite) -> Result<Expansion, Diagnostic> {
        let (raw_args, span, body) = match site {
            ExpansionSite::Directive {
                raw_args,
                span,
                body,
                ..
            } => (raw_args, span, body),
            _ => {
                return Err(ctx.error("RHP0506", "`@for` can only be used as a template directive", site.span()));
            }
        };

        let parts: Vec<&str> = raw_args.split(';').collect();
        if parts.len() != 3 {
            return Err(ctx.error(
                "RHP0502",
                format!("directive `@for` requires 3 semicolon-separated clauses `(init; cond; update)`, got {}", parts.len()),
                span,
            ));
        }

        let init_str = ctx.intern(parts[0].trim());
        let init = if init_str.is_empty() {
            Vec::new()
        } else {
            vec![crate::parser::parse_expr(init_str).map_err(|e| {
                ctx.error("RHP0102", format!("failed to parse `@for` init clause: {}", e), span)
            })?]
        };

        let cond_str = ctx.intern(parts[1].trim());
        let cond = if cond_str.is_empty() {
            Vec::new()
        } else {
            vec![crate::parser::parse_expr(cond_str).map_err(|e| {
                ctx.error("RHP0102", format!("failed to parse `@for` condition clause: {}", e), span)
            })?]
        };

        let update_str = ctx.intern(parts[2].trim());
        let update = if update_str.is_empty() {
            Vec::new()
        } else {
            vec![crate::parser::parse_expr(update_str).map_err(|e| {
                ctx.error("RHP0102", format!("failed to parse `@for` update clause: {}", e), span)
            })?]
        };

        let reg = crate::expand::registry::ExpanderRegistry::default_with_builtins();
        let static_body: &[TemplateNode<'static>] = unsafe { std::mem::transmute(body) };
        let body_stmts = lower_template_nodes(static_body, ctx, &reg)?;

        let mut builder = AstBuilder::new(ctx, span, "for");
        let stmt = builder.for_stmt(init, cond, update, body_stmts);
        Ok(Expansion::Statements(vec![stmt]))
    }
}

/// Built-in `@foreach($collection as $item)` / `@foreach($collection as $key => $val)` directive expander.
pub struct ForeachExpander;

impl Expander for ForeachExpander {
    fn name(&self) -> &str {
        "foreach"
    }

    fn expand(&self, ctx: &mut ExpandCtx, site: ExpansionSite) -> Result<Expansion, Diagnostic> {
        let (raw_args, span, body) = match site {
            ExpansionSite::Directive {
                raw_args,
                span,
                body,
                ..
            } => (raw_args, span, body),
            _ => {
                return Err(ctx.error("RHP0506", "`@foreach` can only be used as a template directive", site.span()));
            }
        };

        let as_idx = raw_args.find(" as ").ok_or_else(|| {
            ctx.error("RHP0502", "directive `@foreach` requires `as` keyword: `($items as $item)`", span)
        })?;

        let expr_str = ctx.intern(raw_args[..as_idx].trim());
        let target_str = raw_args[as_idx + 4..].trim();

        let collection_expr = crate::parser::parse_expr(expr_str).map_err(|e| {
            ctx.error("RHP0102", format!("failed to parse `@foreach` collection expression: {}", e), span)
        })?;

        let (key_var, val_expr) = if let Some(arrow_idx) = target_str.find("=>") {
            let k_str = target_str[..arrow_idx].trim();
            let v_str = target_str[arrow_idx + 2..].trim();

            let k_var = Variable::new(ctx.intern(k_str), span);
            let v_var = Variable::new(ctx.intern(v_str), span);
            let v_expr = Expr::new(ExprKind::Variable(v_var), span);
            (Some(k_var), v_expr)
        } else {
            let v_var = Variable::new(ctx.intern(target_str), span);
            let v_expr = Expr::new(ExprKind::Variable(v_var), span);
            (None, v_expr)
        };

        let reg = crate::expand::registry::ExpanderRegistry::default_with_builtins();
        let static_body: &[TemplateNode<'static>] = unsafe { std::mem::transmute(body) };
        let body_stmts = lower_template_nodes(static_body, ctx, &reg)?;

        let mut builder = AstBuilder::new(ctx, span, "foreach");
        let stmt = builder.foreach_stmt(collection_expr, key_var, val_expr, body_stmts);
        Ok(Expansion::Statements(vec![stmt]))
    }
}

/// Built-in `@datetime($expr)` custom directive expander.
///
/// Formats and outputs dates: `echo date('Y-m-d H:i:s', $expr);`.
pub struct DatetimeExpander;

impl Expander for DatetimeExpander {
    fn name(&self) -> &str {
        "datetime"
    }

    fn expand(&self, ctx: &mut ExpandCtx, site: ExpansionSite) -> Result<Expansion, Diagnostic> {
        let (raw_args, span) = match site {
            ExpansionSite::Directive { raw_args, span, .. } => (raw_args, span),
            _ => {
                return Err(ctx.error("RHP0506", "`@datetime` can only be used as a template directive", site.span()));
            }
        };

        if raw_args.trim().is_empty() {
            return Err(ctx.error("RHP0502", "directive `@datetime` requires a timestamp or date expression argument", span));
        }

        let arg_s = ctx.intern(raw_args);
        let expr = crate::parser::parse_expr(arg_s).map_err(|e| {
            ctx.error("RHP0102", format!("failed to parse `@datetime` argument: {}", e), span)
        })?;

        let mut builder = AstBuilder::new(ctx, span, "datetime");
        let format_str = builder.string_expr("Y-m-d H:i:s");
        let date_call = builder.call_expr("date", vec![format_str, expr]);
        let echo_stmt = builder.echo_stmt(vec![date_call]);

        Ok(Expansion::Statements(vec![echo_stmt]))
    }
}
