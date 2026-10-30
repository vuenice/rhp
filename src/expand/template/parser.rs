use crate::ast::expr::{Argument, Expr};
use crate::ast::types::Ident;
use crate::diagnostic::Diagnostic;
use crate::expand::context::ExpandCtx;
use crate::expand::template::lexer::TemplateToken;
use crate::lexer::span::Span;

/// A node in the parsed template syntax tree.
#[derive(Debug, Clone, PartialEq)]
pub enum TemplateNode<'s> {
    /// Literal HTML or text chunk.
    Text {
        content: String,
        span: Span,
    },
    /// Echo tag: `{{ expr }}` or `{!! expr !!}`.
    Echo {
        expr: Expr<'s>,
        raw: bool,
        span: Span,
    },
    /// Directive block or statement: `@if`, `@for`, `@foreach`, `@datetime(...)`, etc.
    Directive(TemplateDirective<'s>),
}

/// A parsed directive invocation with optional child body and else/elseif branches.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateDirective<'s> {
    pub name: Ident<'s>,
    pub args: Vec<Argument<'s>>,
    pub raw_args: &'s str,
    pub body: Vec<TemplateNode<'s>>,
    pub else_branches: Vec<TemplateElseBranch<'s>>,
    pub span: Span,
}

/// An `elseif` or `else` branch within an `@if` directive block.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateElseBranch<'s> {
    pub name: Ident<'s>,
    pub condition: Option<Expr<'s>>,
    pub body: Vec<TemplateNode<'s>>,
    pub span: Span,
}

/// Parser for converting tokenized template streams into structured template AST trees.
pub struct TemplateParser<'a> {
    tokens: Vec<TemplateToken>,
    cursor: usize,
    ctx: &'a mut ExpandCtx,
}

impl<'a> TemplateParser<'a> {
    pub fn new(tokens: Vec<TemplateToken>, ctx: &'a mut ExpandCtx) -> Self {
        Self {
            tokens,
            cursor: 0,
            ctx,
        }
    }

    fn peek(&self) -> Option<&TemplateToken> {
        self.tokens.get(self.cursor)
    }

    fn advance(&mut self) -> Option<TemplateToken> {
        if self.cursor < self.tokens.len() {
            let tok = self.tokens[self.cursor].clone();
            self.cursor += 1;
            Some(tok)
        } else {
            None
        }
    }

    /// Parse the token stream into a sequence of [`TemplateNode`] items.
    pub fn parse(&mut self) -> Result<Vec<TemplateNode<'static>>, Diagnostic> {
        let mut nodes = Vec::new();

        while let Some(tok) = self.peek() {
            match tok {
                TemplateToken::Text { content, span } => {
                    let content = content.clone();
                    let span = *span;
                    self.advance();
                    nodes.push(TemplateNode::Text { content, span });
                }
                TemplateToken::Echo {
                    raw_expr,
                    is_raw,
                    span,
                } => {
                    let raw_expr = raw_expr.clone();
                    let is_raw = *is_raw;
                    let span = *span;
                    self.advance();

                    let expr = self.parse_expression_str(&raw_expr, span)?;
                    nodes.push(TemplateNode::Echo {
                        expr,
                        raw: is_raw,
                        span,
                    });
                }
                TemplateToken::Directive { name, .. } => {
                    let name_str = name.clone();
                    if is_closing_directive(&name_str) || is_branch_directive(&name_str) {
                        let span = tok.span();
                        return Err(self.ctx.error(
                            "RHP0503",
                            format!("unexpected closing or branch directive `@{}` without matching opening directive", name_str),
                            span,
                        ));
                    }

                    let directive = self.parse_directive_block()?;
                    nodes.push(TemplateNode::Directive(directive));
                }
            }
        }

        Ok(nodes)
    }

    fn parse_directive_block(&mut self) -> Result<TemplateDirective<'static>, Diagnostic> {
        let open_tok = self.advance().expect("Directive token expected");
        let (name_str, raw_args_opt, open_span) = match open_tok {
            TemplateToken::Directive {
                name,
                raw_args,
                span,
            } => (name, raw_args, span),
            _ => unreachable!(),
        };

        let raw_args = raw_args_opt.unwrap_or_default();
        let interned_raw_args = self.ctx.intern(&raw_args);
        let interned_name = self.ctx.intern(&name_str);
        let ident = Ident::new(interned_name, open_span);

        let parsed_args = if !raw_args.trim().is_empty() {
            self.parse_arguments_str(&raw_args, open_span).unwrap_or_default()
        } else {
            Vec::new()
        };

        // Determine if this is a block directive
        if is_block_directive(&name_str) {
            let closing_name = expected_closing_directive(&name_str);
            let mut body = Vec::new();
            let mut else_branches = Vec::new();
            let mut closed = false;

            while let Some(tok) = self.peek() {
                match tok {
                    TemplateToken::Directive { name, raw_args, span } => {
                        let dname = name.clone();
                        let dspan = *span;
                        let draw_args = raw_args.clone();

                        if dname == closing_name {
                            self.advance(); // consume closing directive
                            closed = true;
                            break;
                        } else if name_str == "if" && (dname == "elseif" || dname == "else") {
                            self.advance(); // consume branch directive
                            let branch_ident = Ident::new(self.ctx.intern(&dname), dspan);
                            let cond = if dname == "elseif" {
                                let arg_str = draw_args.unwrap_or_default();
                                Some(self.parse_expression_str(&arg_str, dspan)?)
                            } else {
                                None
                            };

                            let mut branch_body = Vec::new();
                            while let Some(btok) = self.peek() {
                                if let TemplateToken::Directive { name: bname, .. } = btok {
                                    if bname == "endif" || bname == "elseif" || bname == "else" {
                                        break;
                                    }
                                }
                                branch_body.push(self.parse_single_node()?);
                            }

                            else_branches.push(TemplateElseBranch {
                                name: branch_ident,
                                condition: cond,
                                body: branch_body,
                                span: dspan,
                            });
                        } else if is_closing_directive(&dname) {
                            return Err(self.ctx.error(
                                "RHP0503",
                                format!(
                                    "mismatched closing directive: expected `@{}` to close `@{}` at line {}, found `@{}`",
                                    closing_name, name_str, open_span.line, dname
                                ),
                                dspan,
                            ));
                        } else {
                            body.push(self.parse_single_node()?);
                        }
                    }
                    _ => {
                        body.push(self.parse_single_node()?);
                    }
                }
            }

            if !closed {
                return Err(self.ctx.error(
                    "RHP0503",
                    format!(
                        "unclosed directive `@{}` opened at line {}: missing closing `@{}`",
                        name_str, open_span.line, closing_name
                    ),
                    open_span,
                ));
            }

            Ok(TemplateDirective {
                name: ident,
                args: parsed_args,
                raw_args: interned_raw_args,
                body,
                else_branches,
                span: open_span,
            })
        } else {
            // Standalone directive (e.g. `@datetime($x)`)
            Ok(TemplateDirective {
                name: ident,
                args: parsed_args,
                raw_args: interned_raw_args,
                body: Vec::new(),
                else_branches: Vec::new(),
                span: open_span,
            })
        }
    }

    fn parse_single_node(&mut self) -> Result<TemplateNode<'static>, Diagnostic> {
        let tok = self.advance().expect("Expected template token");
        match tok {
            TemplateToken::Text { content, span } => Ok(TemplateNode::Text { content, span }),
            TemplateToken::Echo {
                raw_expr,
                is_raw,
                span,
            } => {
                let expr = self.parse_expression_str(&raw_expr, span)?;
                Ok(TemplateNode::Echo {
                    expr,
                    raw: is_raw,
                    span,
                })
            }
            TemplateToken::Directive { .. } => {
                // Re-wind 1 token and parse as directive block or statement
                self.cursor -= 1;
                let dir = self.parse_directive_block()?;
                Ok(TemplateNode::Directive(dir))
            }
        }
    }

    fn parse_expression_str(&mut self, expr_str: &str, span: Span) -> Result<Expr<'static>, Diagnostic> {
        let interned = self.ctx.intern(expr_str);
        crate::parser::parse_expr(interned).map_err(|e| {
            self.ctx.error(
                "RHP0102",
                format!("failed to parse template expression: {}", e),
                span,
            )
        })
    }

    fn parse_arguments_str(&mut self, args_str: &str, span: Span) -> Result<Vec<Argument<'static>>, Diagnostic> {
        // Wrap in dummy function call `__dummy(args)` to reuse expression parser argument handling
        let wrapper = format!("__dummy({})", args_str);
        let interned = self.ctx.intern(&wrapper);
        let expr = crate::parser::parse_expr(interned).map_err(|e| {
            self.ctx.error(
                "RHP0102",
                format!("failed to parse directive arguments `({})`: {}", args_str, e),
                span,
            )
        })?;

        match expr.kind {
            crate::ast::expr::ExprKind::Call { args, .. } => Ok(args),
            _ => {
                let single_expr = self.parse_expression_str(args_str, span)?;
                Ok(vec![Argument::new(single_expr, span)])
            }
        }
    }
}

fn is_block_directive(name: &str) -> bool {
    matches!(name, "if" | "for" | "foreach" | "while")
}

fn is_closing_directive(name: &str) -> bool {
    matches!(name, "endif" | "endfor" | "endforeach" | "endwhile")
}

fn is_branch_directive(name: &str) -> bool {
    matches!(name, "elseif" | "else")
}

fn expected_closing_directive(open_name: &str) -> &'static str {
    match open_name {
        "if" => "endif",
        "for" => "endfor",
        "foreach" => "endforeach",
        "while" => "endwhile",
        _ => "endif",
    }
}
