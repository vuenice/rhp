use crate::ast::{
    expr::Expr,
    stmt::{
        Attribute, CatchClause, ClassConstDecl, ClassDecl, ClassMember, ClassMemberKind,
        ElseIfBranch, EnumCase, EnumDecl, FunctionDecl, InterfaceDecl, MethodDecl, PropertyDecl,
        Stmt, StmtKind, SwitchCase, TraitAdaptation, TraitAlias, TraitDecl, TraitInsteadOf,
        TraitUseDecl, UseImport,
    },
    types::{ClassModifiers, Visibility},
};
use crate::lexer::{
    facade::{is_static_facade, is_static_only, register_tier3_facade},
    span::Span,
    token::Token,
    SpannedToken,
};
use crate::parser::{
    combinator::CombinatorStream,
    error::{ParseError, ParseErrorKind},
};

impl<'s> CombinatorStream<'s> {
    /// Parse a single top-level or block statement.
    pub fn parse_stmt(&mut self) -> Result<Stmt<'s>, ParseError> {
        self.skip_php_tags();
        self.skip_doc_comments();

        let start_span = self.current_span();

        // 1. Empty statement (stray semicolon)
        if self.matches(&Token::SemiColon) {
            return Ok(Stmt::new(StmtKind::Empty, start_span));
        }

        // 2. Block statement `{ ... }`
        if self.check(&Token::OpenBrace) {
            let stmts = self.parse_block_statements()?;
            let end_span = self.current_span();
            return Ok(Stmt::new(StmtKind::Block(stmts), start_span.merge(end_span)));
        }

        // 3. Attributes preceding declarations (`#[Route(...)]`)
        if self.check(&Token::AttributeStart) {
            let attributes = self.parse_attributes()?;
            return self.parse_declaration_with_attributes(attributes, start_span);
        }

        // 3b. Disallow template directives in RHP source code
        if self.check(&Token::At) {
            let at_span = self.current_span();
            if let Some(tok) = self.peek_nth(1) {
                if matches!(tok.token, Token::Ident(_) | Token::If | Token::For | Token::Foreach) {
                    let dname = match tok.token {
                        Token::Ident(s) => s,
                        Token::If => "if",
                        Token::For => "for",
                        Token::Foreach => "foreach",
                        _ => "directive",
                    };
                    return Err(ParseError::new(
                        ParseErrorKind::Custom {
                            message: format!(
                                "Template directives (`@{}`) are only valid inside template files (.rhp.html, .blade.rhp). In RHP source code, use `#[{}]` for attributes or standard PHP control flow instead.",
                                dname, dname
                            ),
                        },
                        at_span.merge(tok.span),
                    ));
                }
            }
        }

        // 4. Strongly-typed variable declaration: only prefix type syntax `int $x = 10;` is supported.
        // Colon syntax `$x: int = 10;` is explicitly rejected.
        if let Some(tok) = self.peek() {
            if matches!(tok.token, Token::Variable(_))
                && self.peek_nth(1).map(|t| &t.token) == Some(&Token::Colon)
            {
                let var = self.expect_variable()?;
                let colon = self.expect(Token::Colon)?;
                return Err(ParseError::new(
                    ParseErrorKind::Custom {
                        message: format!(
                            "Colon type syntax '{}: Type' is not supported. Use prefix type syntax 'Type {}' instead (e.g., 'int {} = 10;').",
                            var.name, var.name, var.name
                        ),
                    },
                    var.span.merge(colon.span),
                ));
            }
        }

        if self.is_prefix_typed_var_decl() {
            let type_hint = self.parse_type_hint()?;
            let var = self.expect_variable()?;
            let init = if self.matches(&Token::Assign) {
                Some(self.parse_expr_lowest()?)
            } else {
                None
            };
            self.expect_statement_terminator()?;
            let end_span = init
                .as_ref()
                .map(|e| e.span)
                .unwrap_or(var.span);
            return Ok(Stmt::new(
                StmtKind::VarDecl {
                    var,
                    type_hint: Some(type_hint),
                    init,
                },
                start_span.merge(end_span),
            ));
        }

        // 5. Control Flow: If statement
        if self.matches(&Token::If) {
            return self.parse_if_stmt(start_span);
        }

        // 6. Control Flow: While loop
        if self.matches(&Token::While) {
            self.expect(Token::OpenParen)?;
            let condition = self.parse_expr_lowest()?;
            self.expect(Token::CloseParen)?;
            let body = self.parse_stmt()?;
            let span = start_span.merge(body.span);
            return Ok(Stmt::new(
                StmtKind::While {
                    condition,
                    body: Box::new(body),
                },
                span,
            ));
        }

        // 7. Control Flow: Do-While loop
        if self.matches(&Token::Do) {
            let body = self.parse_stmt()?;
            self.expect(Token::While)?;
            self.expect(Token::OpenParen)?;
            let condition = self.parse_expr_lowest()?;
            self.expect(Token::CloseParen)?;
            self.expect_statement_terminator()?;
            let span = start_span.merge(self.current_span());
            return Ok(Stmt::new(
                StmtKind::DoWhile {
                    body: Box::new(body),
                    condition,
                },
                span,
            ));
        }

        // 8. Control Flow: For loop / For-in loop
        if self.matches(&Token::For) {
            self.expect(Token::OpenParen)?;

            let is_for_in = if self.check(&Token::SemiColon) {
                false
            } else {
                let mut is_in = false;
                let mut i = 0;
                while let Some(tok) = self.peek_nth(i) {
                    if tok.token == Token::SemiColon || tok.token == Token::OpenBrace {
                        break;
                    }
                    if tok.token == Token::In {
                        is_in = true;
                        break;
                    }
                    i += 1;
                }
                is_in
            };

            if is_for_in {
                let first_expr = self.parse_expr_lowest()?;
                let (key_var, value_var) = if self.matches(&Token::DoubleArrow) {
                    let second_expr = self.parse_expr_lowest()?;
                    let k_var = match &first_expr.kind {
                        crate::ast::expr::ExprKind::Variable(v) => Some(v.clone()),
                        _ => None,
                    };
                    (k_var, Box::new(second_expr))
                } else {
                    (None, Box::new(first_expr))
                };

                self.expect(Token::In)?;
                let expr = self.parse_expr_lowest()?;
                self.expect(Token::CloseParen)?;
                let body = self.parse_stmt()?;
                let span = start_span.merge(body.span);
                return Ok(Stmt::new(
                    StmtKind::ForIn {
                        expr,
                        key_var,
                        value_var,
                        body: Box::new(body),
                    },
                    span,
                ));
            } else {
                let init = self.separated_by(|s| s.parse_expr_lowest(), Token::Comma, &Token::SemiColon)?;
                self.expect(Token::SemiColon)?;
                let condition = self.separated_by(|s| s.parse_expr_lowest(), Token::Comma, &Token::SemiColon)?;
                self.expect(Token::SemiColon)?;
                let update = self.separated_by(|s| s.parse_expr_lowest(), Token::Comma, &Token::CloseParen)?;
                self.expect(Token::CloseParen)?;
                let body = self.parse_stmt()?;
                let span = start_span.merge(body.span);
                return Ok(Stmt::new(
                    StmtKind::For {
                        init,
                        condition,
                        update,
                        body: Box::new(body),
                    },
                    span,
                ));
            }
        }

        // 9. Control Flow: Foreach loop
        if self.matches(&Token::Foreach) {
            self.expect(Token::OpenParen)?;
            let expr = self.parse_expr_lowest()?;
            self.expect(Token::As)?;

            let first_var = self.parse_expr_lowest()?;
            let (key_var, value_var) = if self.matches(&Token::DoubleArrow) {
                let second_var = self.parse_expr_lowest()?;
                let k_var = match &first_var.kind {
                    crate::ast::expr::ExprKind::Variable(v) => Some(v.clone()),
                    _ => None,
                };
                (k_var, Box::new(second_var))
            } else {
                (None, Box::new(first_var))
            };

            self.expect(Token::CloseParen)?;
            let body = self.parse_stmt()?;
            let span = start_span.merge(body.span);
            return Ok(Stmt::new(
                StmtKind::Foreach {
                    expr,
                    key_var,
                    value_var,
                    body: Box::new(body),
                },
                span,
            ));
        }

        // 9b. Control Flow: Loop statement (`loop { ... }`)
        if self.matches(&Token::Loop) {
            let body = self.parse_stmt()?;
            let span = start_span.merge(body.span);
            return Ok(Stmt::new(
                StmtKind::Loop {
                    body: Box::new(body),
                },
                span,
            ));
        }

        // 10. Control Flow: Switch statement
        if self.matches(&Token::Switch) {
            self.expect(Token::OpenParen)?;
            let condition = self.parse_expr_lowest()?;
            self.expect(Token::CloseParen)?;
            self.expect(Token::OpenBrace)?;

            let mut cases = Vec::new();
            while !self.check(&Token::CloseBrace) && !self.is_eof() {
                let case_start = self.current_span();
                let is_default = self.matches(&Token::Default);
                let value = if is_default {
                    None
                } else {
                    self.expect(Token::Case)?;
                    Some(self.parse_expr_lowest()?)
                };
                // Colon or semicolon can follow case
                if !self.matches(&Token::Colon) {
                    self.expect(Token::SemiColon)?;
                }

                let mut body = Vec::new();
                while !self.check(&Token::Case)
                    && !self.check(&Token::Default)
                    && !self.check(&Token::CloseBrace)
                    && !self.is_eof()
                {
                    body.push(self.parse_stmt()?);
                }
                let case_end = self.current_span();
                cases.push(SwitchCase {
                    span: case_start.merge(case_end),
                    value,
                    body,
                });
            }
            let close_brace = self.expect(Token::CloseBrace)?;
            let span = start_span.merge(close_brace.span);
            return Ok(Stmt::new(StmtKind::Switch { condition, cases }, span));
        }

        // 11. Break & Continue
        if self.matches(&Token::Break) {
            let level = if !self.check(&Token::SemiColon)
                && !self.check(&Token::CloseBrace)
                && self.peek().map(|t| t.span.line == start_span.line).unwrap_or(false)
            {
                Some(self.parse_expr_lowest()?)
            } else {
                None
            };
            self.expect_statement_terminator()?;
            let span = start_span.merge(self.current_span());
            return Ok(Stmt::new(StmtKind::Break(level), span));
        }
        if self.matches(&Token::Continue) {
            let level = if !self.check(&Token::SemiColon)
                && !self.check(&Token::CloseBrace)
                && self.peek().map(|t| t.span.line == start_span.line).unwrap_or(false)
            {
                Some(self.parse_expr_lowest()?)
            } else {
                None
            };
            self.expect_statement_terminator()?;
            let span = start_span.merge(self.current_span());
            return Ok(Stmt::new(StmtKind::Continue(level), span));
        }

        // 12. Return & Throw
        if self.matches(&Token::Return) {
            let value = if !self.check(&Token::SemiColon)
                && !self.check(&Token::CloseBrace)
                && self.peek().map(|t| t.span.line == start_span.line).unwrap_or(false)
            {
                Some(self.parse_expr_lowest()?)
            } else {
                None
            };
            self.expect_statement_terminator()?;
            let span = start_span.merge(self.current_span());
            return Ok(Stmt::new(StmtKind::Return(value), span));
        }
        if self.matches(&Token::Throw) {
            let expr = self.parse_expr_lowest()?;
            self.expect_statement_terminator()?;
            let span = start_span.merge(expr.span);
            return Ok(Stmt::new(StmtKind::Throw(expr), span));
        }

        // 13. Try-Catch-Finally
        if self.matches(&Token::Try) {
            let try_block = self.parse_block_statements()?;
            let mut catches = Vec::new();

            while self.matches(&Token::Catch) {
                let catch_start = self.current_span();
                self.expect(Token::OpenParen)?;
                let mut types = vec![self.expect_qualified_ident()?];
                while self.matches(&Token::Pipe) {
                    types.push(self.expect_qualified_ident()?);
                }
                let var = self.expect_variable()?;
                self.expect(Token::CloseParen)?;
                let body = self.parse_block_statements()?;
                let catch_end = self.current_span();
                catches.push(CatchClause {
                    span: catch_start.merge(catch_end),
                    types,
                    var,
                    body,
                });
            }

            let finally_block = if self.matches(&Token::Finally) {
                Some(self.parse_block_statements()?)
            } else {
                None
            };

            let span = start_span.merge(self.current_span());
            return Ok(Stmt::new(
                StmtKind::TryCatch {
                    try_block,
                    catches,
                    finally_block,
                },
                span,
            ));
        }

        // 14. Namespace statement
        if self.matches(&Token::Namespace) {
            let name = if !self.check(&Token::SemiColon) && !self.check(&Token::OpenBrace) {
                Some(self.expect_qualified_ident()?)
            } else {
                None
            };

            if self.check(&Token::OpenBrace) {
                let prev_ns = self.scope.namespace.clone();
                self.scope.namespace = name.as_ref().map(|id| id.name.to_string());
                let body = self.parse_block_statements()?;
                self.scope.namespace = prev_ns;
                let span = start_span.merge(self.current_span());
                return Ok(Stmt::new(
                    StmtKind::Namespace {
                        name,
                        body: Some(body),
                    },
                    span,
                ));
            } else {
                self.scope.namespace = name.as_ref().map(|id| id.name.to_string());
                self.expect_statement_terminator()?;
                let span = start_span.merge(self.current_span());
                return Ok(Stmt::new(
                    StmtKind::Namespace { name, body: None },
                    span,
                ));
            }
        }

        // 14b. Import statement (`import "./Cache.rs";`, `import "./Cache.rs" as Cache;`, `import Cache from "./Cache.rs";`)
        if self.matches(&Token::Import) {
            let (path, alias) = if let Some(spanned) = self.peek() {
                if let Some(path_str) = spanned
                    .token
                    .unescape_string()
                    .or_else(|| spanned.token.raw_string_content().map(|s| s.to_string()))
                {
                    self.advance();
                    let alias = if self.matches(&Token::As) {
                        Some(self.expect_ident()?)
                    } else {
                        None
                    };
                    (path_str, alias)
                } else {
                    let alias_ident = self.expect_ident()?;
                    let from_ident = self.expect_ident()?;
                    if from_ident.name != "from" {
                        return Err(ParseError::new(
                            ParseErrorKind::UnexpectedToken {
                                expected: "'from'".to_string(),
                                found: from_ident.name.to_string(),
                            },
                            from_ident.span,
                        ));
                    }
                    let path_spanned = self.advance().ok_or_else(|| {
                        ParseError::new(
                            ParseErrorKind::UnexpectedEof {
                                expected: "string literal path".to_string(),
                            },
                            self.current_span(),
                        )
                    })?;
                    let path_str = path_spanned
                        .token
                        .unescape_string()
                        .or_else(|| path_spanned.token.raw_string_content().map(|s| s.to_string()))
                        .ok_or_else(|| {
                            ParseError::new(
                                ParseErrorKind::UnexpectedToken {
                                    expected: "string literal path".to_string(),
                                    found: path_spanned.token.as_str().to_string(),
                                },
                                path_spanned.span,
                            )
                        })?;
                    (path_str, Some(alias_ident))
                }
            } else {
                return Err(ParseError::new(
                    ParseErrorKind::UnexpectedEof {
                        expected: "import path or identifier".to_string(),
                    },
                    self.current_span(),
                ));
            };

            self.expect_statement_terminator()?;
            let span = start_span.merge(self.current_span());
            return Ok(Stmt::new(StmtKind::RustImport { path, alias }, span));
        }

        // 15. Use statement
        if self.matches(&Token::Use) {
            // Check if this is a file import (`use "./Cache.rs";` or `use "./Cache.rs" as Cache;`)
            if let Some(spanned) = self.peek() {
                if let Some(path_str) = spanned
                    .token
                    .unescape_string()
                    .or_else(|| spanned.token.raw_string_content().map(|s| s.to_string()))
                {
                    self.advance();
                    let alias = if self.matches(&Token::As) {
                        Some(self.expect_ident()?)
                    } else {
                        None
                    };
                    self.expect_statement_terminator()?;
                    let span = start_span.merge(self.current_span());
                    return Ok(Stmt::new(StmtKind::RustImport { path: path_str, alias }, span));
                }
            }

            let imports = self.separated_by(
                |s| {
                    let imp_start = s.current_span();
                    let name = s.expect_qualified_ident()?;
                    let alias = if s.matches(&Token::As) {
                        Some(s.expect_ident()?)
                    } else {
                        None
                    };
                    let span = imp_start.merge(alias.as_ref().map(|a| a.span).unwrap_or(name.span));
                    Ok(UseImport { span, name, alias })
                },
                Token::Comma,
                &Token::SemiColon,
            )?;
            self.expect_statement_terminator()?;
            let span = start_span.merge(self.current_span());
            return Ok(Stmt::new(StmtKind::Use(imports), span));
        }

        // 16. Global variables
        if self.matches(&Token::Global) {
            let vars = self.separated_by(
                |s| s.expect_variable(),
                Token::Comma,
                &Token::SemiColon,
            )?;
            self.expect_statement_terminator()?;
            let span = start_span.merge(self.current_span());
            return Ok(Stmt::new(StmtKind::Global(vars), span));
        }

        // 17. Echo statement or echo(...) / echo!(...) invocation
        if self.matches(&Token::Echo) {
            if self.check(&Token::Bang) {
                self.advance(); // consume '!'
                let open_paren = self.expect(Token::OpenParen)?;
                let args = self.parse_call_arguments()?;
                let close_paren = self.expect(Token::CloseParen)?;
                self.expect_statement_terminator()?;
                let span = start_span.merge(close_paren.span);
                let call_expr = Expr::static_call("Sys", "echo", args, open_paren.span, span);
                return Ok(Stmt::new(StmtKind::Expr(call_expr), span));
            }
            if self.check(&Token::OpenParen) {
                let open_paren = self.advance().unwrap();
                let args = self.parse_call_arguments()?;
                let close_paren = self.expect(Token::CloseParen)?;
                self.expect_statement_terminator()?;
                let span = start_span.merge(close_paren.span);
                let call_expr = Expr::static_call("Sys", "echo", args, open_paren.span, span);
                return Ok(Stmt::new(StmtKind::Expr(call_expr), span));
            }
            let exprs = self.separated_by(
                |s| s.parse_expr_lowest(),
                Token::Comma,
                &Token::SemiColon,
            )?;
            self.expect_statement_terminator()?;
            let span = start_span.merge(self.current_span());
            return Ok(Stmt::new(StmtKind::Echo(exprs), span));
        }

        // 18. Defer statement
        if self.matches(&Token::Defer) {
            let body = self.parse_stmt()?;
            let span = start_span.merge(body.span);
            return Ok(Stmt::new(StmtKind::Defer(Box::new(body)), span));
        }

        // 19. Declarations: Function, Class, Interface, Trait, Enum
        if self.check_declaration_start() {
            return self.parse_declaration_with_attributes(Vec::new(), start_span);
        }

        // 20. Default Fallback: Expression statement
        let expr = self.parse_expr_lowest()?;
        // Optional statement terminator (semicolon is required except before closing brace or EOF)
        if !self.check(&Token::CloseBrace) && !self.is_eof() {
            self.expect_statement_terminator()?;
        }
        let span = start_span.merge(expr.span);
        Ok(Stmt::new(StmtKind::Expr(expr), span))
    }

    /// Check if current tokens signal the beginning of a top-level declaration.
    fn check_declaration_start(&self) -> bool {
        match self.peek().map(|t| &t.token) {
            Some(
                Token::Function
                | Token::Fn
                | Token::Async
                | Token::Class
                | Token::Abstract
                | Token::Final
                | Token::Readonly
                | Token::Singleton
                | Token::Interface
                | Token::Trait
                | Token::Enum,
            ) => true,
            Some(Token::Static) => {
                matches!(
                    self.peek_nth(1).map(|t| &t.token),
                    Some(Token::Class | Token::Function | Token::Fn)
                )
            }
            _ => false,
        }
    }

    /// Parse declarations that may carry attribute annotations.
    fn parse_declaration_with_attributes(
        &mut self,
        attributes: Vec<Attribute<'s>>,
        start_span: Span,
    ) -> Result<Stmt<'s>, ParseError> {
        let is_async = self.matches(&Token::Async);

        // Standalone Function
        if self.matches(&Token::Function) || self.matches(&Token::Fn) {
            let name = self.expect_ident()?;
            self.register_user_function(name.name);
            let (params, return_type, body) = self.with_function_scope(name.name, |stream| {
                let params = stream.parse_formal_parameters()?;
                let return_type = if stream.matches(&Token::Colon) {
                    Some(stream.parse_type_hint()?)
                } else {
                    None
                };
                let body = if stream.matches(&Token::SemiColon) {
                    None
                } else {
                    Some(stream.parse_block_statements()?)
                };
                Ok((params, return_type, body))
            })?;
            let span = start_span.merge(self.current_span());
            return Ok(Stmt::new(
                StmtKind::FunctionDecl(FunctionDecl {
                    span,
                    attributes,
                    is_async,
                    name,
                    params,
                    return_type,
                    body,
                }),
                span,
            ));
        }

        // Class Modifiers (can appear in any combination: `static class`, `final readonly class`, `singleton class`, etc.)
        let mut is_abstract = false;
        let mut is_final = false;
        let mut is_readonly = false;
        let mut is_static_mod = false;
        let mut is_singleton = false;

        loop {
            if self.matches(&Token::Static) {
                is_static_mod = true;
            } else if self.matches(&Token::Abstract) {
                is_abstract = true;
            } else if self.matches(&Token::Final) {
                is_final = true;
            } else if self.matches(&Token::Readonly) {
                is_readonly = true;
            } else if self.matches(&Token::Singleton) {
                is_singleton = true;
            } else {
                break;
            }
        }

        // Script-land static facades can also be declared via attributes (#[StaticOnly], #[Facade], #[StaticFacade])
        // and singletons via #[Singleton]
        for attr in &attributes {
            let attr_name = attr.name.name;
            if attr_name == "StaticOnly" || attr_name == "Facade" || attr_name == "StaticFacade" {
                is_static_mod = true;
            } else if attr_name == "Singleton" {
                is_singleton = true;
            }
        }

        let modifiers = ClassModifiers {
            is_abstract,
            is_final,
            is_readonly,
            is_static_only: is_static_mod,
            is_singleton,
        };

        // Class declaration
        if self.matches(&Token::Class) {
            let name = self.expect_ident()?;
            if is_static_mod {
                register_tier3_facade(name.name, "User-defined Tier 3 script static facade", true);
            }
            let extends = if self.matches(&Token::Extends) {
                let parent = self.expect_qualified_ident()?;
                // Extending static facades (e.g., `extends Arr`, `extends Str`, or Tier 2/3 static facades) is strictly forbidden
                let check_name = parent.name.strip_prefix('\\').unwrap_or(parent.name);
                if is_static_facade(check_name) || is_static_only(check_name) {
                    return Err(ParseError::new(
                        ParseErrorKind::StaticFacadeInheritanceForbidden {
                            class_name: parent.name.to_string(),
                        },
                        parent.span,
                    ));
                }
                Some(parent)
            } else {
                None
            };
            let implements = if self.matches(&Token::Implements) {
                self.separated_by(|s| s.expect_qualified_ident(), Token::Comma, &Token::OpenBrace)?
            } else {
                Vec::new()
            };

            let class_fqn = match &self.scope.namespace {
                Some(ns) => format!("{}\\{}", ns, name.name),
                None => name.name.to_string(),
            };
            self.expect(Token::OpenBrace)?;
            let members = self.with_class_scope(class_fqn, |stream| {
                let mut members = Vec::new();
                while !stream.check(&Token::CloseBrace) && !stream.is_eof() {
                    stream.skip_doc_comments();
                    if stream.check(&Token::CloseBrace) {
                        break;
                    }
                    members.push(stream.parse_class_member()?);
                }
                Ok(members)
            })?;
            let close_brace = self.expect(Token::CloseBrace)?;
            let span = start_span.merge(close_brace.span);
            return Ok(Stmt::new(
                StmtKind::ClassDecl(ClassDecl {
                    span,
                    attributes,
                    modifiers,
                    name,
                    extends,
                    implements,
                    members,
                }),
                span,
            ));
        }

        // Interface declaration
        if self.matches(&Token::Interface) {
            let name = self.expect_ident()?;
            let extends = if self.matches(&Token::Extends) {
                self.separated_by(|s| s.expect_qualified_ident(), Token::Comma, &Token::OpenBrace)?
            } else {
                Vec::new()
            };
            let iface_fqn = match &self.scope.namespace {
                Some(ns) => format!("{}\\{}", ns, name.name),
                None => name.name.to_string(),
            };
            self.expect(Token::OpenBrace)?;
            let methods = self.with_class_scope(iface_fqn, |stream| {
                let mut methods = Vec::new();
                while !stream.check(&Token::CloseBrace) && !stream.is_eof() {
                    stream.skip_doc_comments();
                    if stream.check(&Token::CloseBrace) {
                        break;
                    }
                    let member = stream.parse_class_member()?;
                    if let ClassMemberKind::Method(m) = member.kind {
                        methods.push(m);
                    }
                }
                Ok(methods)
            })?;
            let close_brace = self.expect(Token::CloseBrace)?;
            let span = start_span.merge(close_brace.span);
            return Ok(Stmt::new(
                StmtKind::InterfaceDecl(InterfaceDecl {
                    span,
                    attributes,
                    name,
                    extends,
                    methods,
                }),
                span,
            ));
        }

        // Trait declaration
        if self.matches(&Token::Trait) {
            let name = self.expect_ident()?;
            let trait_fqn = match &self.scope.namespace {
                Some(ns) => format!("{}\\{}", ns, name.name),
                None => name.name.to_string(),
            };
            self.expect(Token::OpenBrace)?;
            let members = self.with_trait_scope(trait_fqn, |stream| {
                let mut members = Vec::new();
                while !stream.check(&Token::CloseBrace) && !stream.is_eof() {
                    stream.skip_doc_comments();
                    if stream.check(&Token::CloseBrace) {
                        break;
                    }
                    members.push(stream.parse_class_member()?);
                }
                Ok(members)
            })?;
            let close_brace = self.expect(Token::CloseBrace)?;
            let span = start_span.merge(close_brace.span);
            return Ok(Stmt::new(
                StmtKind::TraitDecl(TraitDecl {
                    span,
                    attributes,
                    name,
                    members,
                }),
                span,
            ));
        }

        // Enum declaration
        if self.matches(&Token::Enum) {
            let name = self.expect_ident()?;
            let backing_type = if self.matches(&Token::Colon) {
                Some(self.parse_type_hint()?)
            } else {
                None
            };
            let implements = if self.matches(&Token::Implements) {
                self.separated_by(|s| s.expect_qualified_ident(), Token::Comma, &Token::OpenBrace)?
            } else {
                Vec::new()
            };
            let enum_fqn = match &self.scope.namespace {
                Some(ns) => format!("{}\\{}", ns, name.name),
                None => name.name.to_string(),
            };
            self.expect(Token::OpenBrace)?;
            let (cases, methods) = self.with_class_scope(enum_fqn, |stream| {
                let mut cases = Vec::new();
                let mut methods = Vec::new();

                while !stream.check(&Token::CloseBrace) && !stream.is_eof() {
                    stream.skip_doc_comments();
                    if stream.check(&Token::CloseBrace) {
                        break;
                    }
                    let member_attrs = stream.parse_attributes()?;
                    if stream.matches(&Token::Case) {
                        let case_start = stream.current_span();
                        let case_name = stream.expect_ident()?;
                        let case_val = if stream.matches(&Token::Assign) {
                            Some(stream.parse_expr_lowest()?)
                        } else {
                            None
                        };
                        stream.expect_statement_terminator()?;
                        let case_span = case_start.merge(stream.current_span());
                        cases.push(EnumCase {
                            span: case_span,
                            attributes: member_attrs,
                            name: case_name,
                            value: case_val,
                        });
                    } else {
                        let member = stream.parse_class_member()?;
                        if let ClassMemberKind::Method(m) = member.kind {
                            methods.push(m);
                        }
                    }
                }
                Ok((cases, methods))
            })?;
            let close_brace = self.expect(Token::CloseBrace)?;
            let span = start_span.merge(close_brace.span);
            return Ok(Stmt::new(
                StmtKind::EnumDecl(EnumDecl {
                    span,
                    attributes,
                    name,
                    backing_type,
                    implements,
                    cases,
                    methods,
                }),
                span,
            ));
        }

        Err(ParseError::new(
            ParseErrorKind::UnexpectedToken {
                expected: "declaration (function, class, interface, trait, enum)".to_string(),
                found: self.peek().map(|t| t.token.as_str()).unwrap_or("EOF").to_string(),
            },
            start_span,
        ))
    }

    /// Parse a single member inside a class, trait, or interface body.
    pub fn parse_class_member(&mut self) -> Result<ClassMember<'s>, ParseError> {
        let start_span = self.current_span();
        let attributes = self.parse_attributes()?;

        // Trait use statement inside class or trait
        if self.matches(&Token::Use) {
            let mut traits = Vec::new();
            traits.push(self.expect_qualified_ident()?);
            while self.matches(&Token::Comma) {
                traits.push(self.expect_qualified_ident()?);
            }

            let mut adaptations = Vec::new();
            if self.matches(&Token::OpenBrace) {
                while !self.check(&Token::CloseBrace) && !self.is_eof() {
                    if self.matches(&Token::SemiColon) {
                        continue;
                    }

                    let item_start = self.current_span();
                    let first_ident = self.expect_qualified_ident()?;
                    let (trait_name, method) = if self.matches(&Token::DoubleColon) {
                        let method_ident = self.expect_ident()?;
                        (Some(first_ident), method_ident)
                    } else {
                        (None, first_ident)
                    };

                    if self.matches(&Token::Insteadof) {
                        let mut instead_of = Vec::new();
                        instead_of.push(self.expect_qualified_ident()?);
                        while self.matches(&Token::Comma) {
                            instead_of.push(self.expect_qualified_ident()?);
                        }
                        if self.check(&Token::SemiColon) {
                            self.advance();
                        } else if !self.check(&Token::CloseBrace) {
                            self.expect_statement_terminator()?;
                        }
                        let span = item_start.merge(self.current_span());
                        adaptations.push(TraitAdaptation::InsteadOf(TraitInsteadOf {
                            span,
                            trait_name,
                            method,
                            instead_of,
                        }));
                    } else if self.matches(&Token::As) {
                        let visibility = if self.matches(&Token::Public) {
                            Some(Visibility::Public)
                        } else if self.matches(&Token::Protected) {
                            Some(Visibility::Protected)
                        } else if self.matches(&Token::Private) {
                            Some(Visibility::Private)
                        } else {
                            None
                        };

                        let alias = if !self.check(&Token::SemiColon) && !self.check(&Token::CloseBrace) {
                            Some(self.expect_ident()?)
                        } else {
                            None
                        };

                        if visibility.is_none() && alias.is_none() {
                            return Err(ParseError::new(
                                ParseErrorKind::UnexpectedToken {
                                    expected: "method alias name or visibility modifier (public, protected, private)".to_string(),
                                    found: self.peek().map(|t| t.token.as_str()).unwrap_or("EOF").to_string(),
                                },
                                self.current_span(),
                            ));
                        }

                        if self.check(&Token::SemiColon) {
                            self.advance();
                        } else if !self.check(&Token::CloseBrace) {
                            self.expect_statement_terminator()?;
                        }
                        let span = item_start.merge(self.current_span());
                        adaptations.push(TraitAdaptation::Alias(TraitAlias {
                            span,
                            trait_name,
                            method,
                            visibility,
                            alias,
                        }));
                    } else {
                        return Err(ParseError::new(
                            ParseErrorKind::UnexpectedToken {
                                expected: "'insteadof' or 'as'".to_string(),
                                found: self.peek().map(|t| t.token.as_str()).unwrap_or("EOF").to_string(),
                            },
                            self.current_span(),
                        ));
                    }
                }
                self.expect(Token::CloseBrace)?;
                if self.check(&Token::SemiColon) {
                    self.advance();
                }
            } else {
                self.expect_statement_terminator()?;
            }

            let span = start_span.merge(self.current_span());
            return Ok(ClassMember {
                span,
                kind: ClassMemberKind::TraitUse(TraitUseDecl { traits, adaptations }),
            });
        }

        // Modifiers: visibility, static, abstract, final, readonly
        let mut visibility = Visibility::Public;
        if self.matches(&Token::Public) {
            visibility = Visibility::Public;
        } else if self.matches(&Token::Protected) {
            visibility = Visibility::Protected;
        } else if self.matches(&Token::Private) {
            visibility = Visibility::Private;
        }

        let is_static = self.matches(&Token::Static);
        let is_abstract = self.matches(&Token::Abstract);
        let is_final = self.matches(&Token::Final);
        let is_readonly = self.matches(&Token::Readonly);

        // Constant member `const NAME = value;`
        if self.matches(&Token::Const) {
            let name = self.expect_ident()?;
            self.expect(Token::Assign)?;
            let value = self.parse_expr_lowest()?;
            self.expect_statement_terminator()?;
            let span = start_span.merge(value.span);
            return Ok(ClassMember {
                span,
                kind: ClassMemberKind::Constant(ClassConstDecl {
                    attributes,
                    visibility,
                    is_final,
                    name,
                    value,
                }),
            });
        }

        // Method member `function name(params): return_type { ... }`
        let is_async = self.matches(&Token::Async);
        if self.matches(&Token::Function) {
            let name = self.expect_ident()?;
            let (params, return_type, body) = self.with_function_scope(name.name, |stream| {
                let params = stream.parse_formal_parameters()?;
                let return_type = if stream.matches(&Token::Colon) {
                    Some(stream.parse_type_hint()?)
                } else {
                    None
                };
                let body = if stream.matches(&Token::SemiColon) {
                    None
                } else {
                    Some(stream.parse_block_statements()?)
                };
                Ok((params, return_type, body))
            })?;
            let span = start_span.merge(self.current_span());
            return Ok(ClassMember {
                span,
                kind: ClassMemberKind::Method(MethodDecl {
                    attributes: attributes.clone(),
                    visibility,
                    is_static,
                    is_abstract,
                    is_final,
                    function: FunctionDecl {
                        span,
                        attributes,
                        is_async,
                        name,
                        params,
                        return_type,
                        body,
                    },
                }),
            });
        }

        // Property member `[type] $name [= default];`
        let type_hint = if !self.check_fn(|t| matches!(t, Token::Variable(_))) {
            Some(self.parse_type_hint()?)
        } else {
            None
        };

        let name = self.expect_variable()?;
        if self.check(&Token::Colon) {
            let colon = self.expect(Token::Colon)?;
            return Err(ParseError::new(
                ParseErrorKind::Custom {
                    message: format!(
                        "Colon type syntax '{}: Type' is not supported. Use prefix type syntax 'Type {}' instead (e.g., 'int {} = 10;').",
                        name.name, name.name, name.name
                    ),
                },
                name.span.merge(colon.span),
            ));
        }
        let default = if self.matches(&Token::Assign) {
            Some(self.parse_expr_lowest()?)
        } else {
            None
        };
        self.expect_statement_terminator()?;
        let span = start_span.merge(self.current_span());

        Ok(ClassMember {
            span,
            kind: ClassMemberKind::Property(PropertyDecl {
                attributes,
                visibility,
                is_static,
                is_readonly,
                type_hint,
                name,
                default,
            }),
        })
    }

    /// Parse an if statement with optional elseif and else branches.
    fn parse_if_stmt(&mut self, start_span: Span) -> Result<Stmt<'s>, ParseError> {
        self.expect(Token::OpenParen)?;
        let condition = self.parse_expr_lowest()?;
        self.expect(Token::CloseParen)?;
        let then_branch = self.parse_stmt()?;

        let mut elseif_branches = Vec::new();
        while self.matches(&Token::Elseif) {
            let branch_start = self.current_span();
            self.expect(Token::OpenParen)?;
            let elif_cond = self.parse_expr_lowest()?;
            self.expect(Token::CloseParen)?;
            let elif_body = self.parse_stmt()?;
            let branch_span = branch_start.merge(elif_body.span);
            elseif_branches.push(ElseIfBranch {
                span: branch_span,
                condition: elif_cond,
                body: elif_body,
            });
        }

        let else_branch = if self.matches(&Token::Else) {
            if self.check(&Token::If) {
                return Err(ParseError::new(
                    ParseErrorKind::Custom {
                        message: "Use the single keyword 'elseif' instead of 'else if' for conditional chaining.".to_string(),
                    },
                    self.current_span(),
                ));
            }
            Some(Box::new(self.parse_stmt()?))
        } else {
            None
        };

        let end_span = else_branch
            .as_ref()
            .map(|e| e.span)
            .or_else(|| elseif_branches.last().map(|b| b.span))
            .unwrap_or(then_branch.span);
        let span = start_span.merge(end_span);

        Ok(Stmt::new(
            StmtKind::If {
                condition,
                then_branch: Box::new(then_branch),
                elseif_branches,
                else_branch,
            },
            span,
        ))
    }

    /// Expect statement terminator (semicolon `;` or newline/closing brace/EOF).
    fn expect_statement_terminator(&mut self) -> Result<SpannedToken<'s>, ParseError> {
        if self.matches(&Token::SemiColon) {
            let span = self.previous_token().map(|t| t.span).unwrap_or_else(|| self.current_span());
            return Ok(SpannedToken::new(Token::SemiColon, span));
        }
        let prev_span = self.previous_token().map(|t| t.span).unwrap_or_else(|| self.current_span());
        // Allow automatic semicolon termination across newlines, before closing brace, or at EOF
        if let Some(next) = self.peek() {
            if next.token == Token::CloseBrace || next.span.line > prev_span.line {
                return Ok(SpannedToken::new(Token::SemiColon, prev_span));
            }
        } else if self.is_eof() {
            return Ok(SpannedToken::new(Token::SemiColon, prev_span));
        }
        self.expect(Token::SemiColon)
    }
}
