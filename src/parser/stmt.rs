use crate::ast::{
    stmt::{
        Attribute, CatchClause, ClassConstDecl, ClassDecl, ClassMember, ClassMemberKind,
        ElseIfBranch, EnumCase, EnumDecl, FunctionDecl, InterfaceDecl, MethodDecl, PropertyDecl,
        Stmt, StmtKind, SwitchCase, TraitDecl, TraitUseDecl, UseImport,
    },
    types::{ClassModifiers, Visibility},
};
use crate::lexer::{facade::is_static_facade, span::Span, token::Token, SpannedToken};
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

        // 4. Strongly-typed variable declaration (`$x: int = 10;` or `$x: int;`)
        if let Some(tok) = self.peek() {
            if matches!(tok.token, Token::Variable(_))
                && self.peek_nth(1).map(|t| &t.token) == Some(&Token::Colon)
            {
                let var = self.expect_variable()?;
                self.expect(Token::Colon)?;
                let type_hint = self.parse_type_hint()?;
                let init = if self.matches(&Token::Assign) {
                    Some(self.parse_expr_lowest()?)
                } else {
                    None
                };
                self.expect_statement_terminator()?;
                let end_span = init
                    .as_ref()
                    .map(|e| e.span)
                    .unwrap_or(type_hint.span);
                return Ok(Stmt::new(
                    StmtKind::VarDecl {
                        var,
                        type_hint: Some(type_hint),
                        init,
                    },
                    start_span.merge(end_span),
                ));
            }
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

        // 8. Control Flow: For loop
        if self.matches(&Token::For) {
            self.expect(Token::OpenParen)?;
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
            let level = if !self.check(&Token::SemiColon) {
                Some(self.parse_expr_lowest()?)
            } else {
                None
            };
            self.expect_statement_terminator()?;
            let span = start_span.merge(self.current_span());
            return Ok(Stmt::new(StmtKind::Break(level), span));
        }
        if self.matches(&Token::Continue) {
            let level = if !self.check(&Token::SemiColon) {
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
            let value = if !self.check(&Token::SemiColon) {
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
                let mut types = vec![self.expect_ident()?];
                while self.matches(&Token::Pipe) {
                    types.push(self.expect_ident()?);
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
                Some(self.expect_ident()?)
            } else {
                None
            };

            if self.check(&Token::OpenBrace) {
                let body = self.parse_block_statements()?;
                let span = start_span.merge(self.current_span());
                return Ok(Stmt::new(
                    StmtKind::Namespace {
                        name,
                        body: Some(body),
                    },
                    span,
                ));
            } else {
                self.expect_statement_terminator()?;
                let span = start_span.merge(self.current_span());
                return Ok(Stmt::new(
                    StmtKind::Namespace { name, body: None },
                    span,
                ));
            }
        }

        // 15. Use statement
        if self.matches(&Token::Use) {
            let imports = self.separated_by(
                |s| {
                    let imp_start = s.current_span();
                    let name = s.expect_ident()?;
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

        // 17. Echo statement
        if self.matches(&Token::Echo) {
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
                | Token::Async
                | Token::Class
                | Token::Abstract
                | Token::Final
                | Token::Readonly
                | Token::Interface
                | Token::Trait
                | Token::Enum,
            ) => true,
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
        if self.matches(&Token::Function) {
            let name = self.expect_ident()?;
            self.register_user_function(name.name);
            let params = self.parse_formal_parameters()?;
            let return_type = if self.matches(&Token::Colon) {
                Some(self.parse_type_hint()?)
            } else {
                None
            };
            let body = if self.matches(&Token::SemiColon) {
                None
            } else {
                Some(self.parse_block_statements()?)
            };
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

        // Class Modifiers
        let is_abstract = self.matches(&Token::Abstract);
        let is_final = self.matches(&Token::Final);
        let is_readonly = self.matches(&Token::Readonly);
        let modifiers = ClassModifiers {
            is_abstract,
            is_final,
            is_readonly,
        };

        // Class declaration
        if self.matches(&Token::Class) {
            let name = self.expect_ident()?;
            let extends = if self.matches(&Token::Extends) {
                let parent = self.expect_ident()?;
                // Extending static facades (e.g., `extends Arr`, `extends Str`) is strictly forbidden
                if is_static_facade(parent.name) {
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
                self.separated_by(|s| s.expect_ident(), Token::Comma, &Token::OpenBrace)?
            } else {
                Vec::new()
            };

            self.expect(Token::OpenBrace)?;
            let mut members = Vec::new();
            while !self.check(&Token::CloseBrace) && !self.is_eof() {
                self.skip_doc_comments();
                if self.check(&Token::CloseBrace) {
                    break;
                }
                members.push(self.parse_class_member()?);
            }
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
                self.separated_by(|s| s.expect_ident(), Token::Comma, &Token::OpenBrace)?
            } else {
                Vec::new()
            };
            self.expect(Token::OpenBrace)?;
            let mut methods = Vec::new();
            while !self.check(&Token::CloseBrace) && !self.is_eof() {
                self.skip_doc_comments();
                if self.check(&Token::CloseBrace) {
                    break;
                }
                let member = self.parse_class_member()?;
                if let ClassMemberKind::Method(m) = member.kind {
                    methods.push(m);
                }
            }
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
            self.expect(Token::OpenBrace)?;
            let mut members = Vec::new();
            while !self.check(&Token::CloseBrace) && !self.is_eof() {
                self.skip_doc_comments();
                if self.check(&Token::CloseBrace) {
                    break;
                }
                members.push(self.parse_class_member()?);
            }
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
                self.separated_by(|s| s.expect_ident(), Token::Comma, &Token::OpenBrace)?
            } else {
                Vec::new()
            };

            self.expect(Token::OpenBrace)?;
            let mut cases = Vec::new();
            let mut methods = Vec::new();

            while !self.check(&Token::CloseBrace) && !self.is_eof() {
                self.skip_doc_comments();
                if self.check(&Token::CloseBrace) {
                    break;
                }
                let member_attrs = self.parse_attributes()?;
                if self.matches(&Token::Case) {
                    let case_start = self.current_span();
                    let case_name = self.expect_ident()?;
                    let case_val = if self.matches(&Token::Assign) {
                        Some(self.parse_expr_lowest()?)
                    } else {
                        None
                    };
                    self.expect_statement_terminator()?;
                    let case_span = case_start.merge(self.current_span());
                    cases.push(EnumCase {
                        span: case_span,
                        attributes: member_attrs,
                        name: case_name,
                        value: case_val,
                    });
                } else {
                    let member = self.parse_class_member()?;
                    if let ClassMemberKind::Method(m) = member.kind {
                        methods.push(m);
                    }
                }
            }
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

        // Trait use statement inside class
        if self.matches(&Token::Use) {
            let traits = self.separated_by(|s| s.expect_ident(), Token::Comma, &Token::SemiColon)?;
            self.expect_statement_terminator()?;
            let span = start_span.merge(self.current_span());
            return Ok(ClassMember {
                span,
                kind: ClassMemberKind::TraitUse(TraitUseDecl { traits }),
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
            let params = self.parse_formal_parameters()?;
            let return_type = if self.matches(&Token::Colon) {
                Some(self.parse_type_hint()?)
            } else {
                None
            };
            let body = if self.matches(&Token::SemiColon) {
                None
            } else {
                Some(self.parse_block_statements()?)
            };
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

    /// Expect statement terminator (semicolon `;`).
    fn expect_statement_terminator(&mut self) -> Result<SpannedToken<'s>, ParseError> {
        self.expect(Token::SemiColon)
    }
}
