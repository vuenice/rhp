use crate::ast::{
    expr::{
        Argument, ArrayElement, AssignOp, BinaryOp, ClosureUse, Expr, ExprKind, Literal, MatchArm,
        StringPart, UnaryOp,
    },
    stmt::{Param, Stmt},
    types::{CastType, Ident, Variable},
};
use crate::lexer::{
    facade::{
        is_legacy_php_function, is_reserved_intrinsic, is_static_facade, is_static_only,
        register_macro,
    },
    span::Span,
    token::Token,
    Lexer, SpannedToken,
};
use crate::parser::{
    combinator::CombinatorStream,
    error::{ParseError, ParseErrorKind},
};

/// Numeric binding powers for the Pratt expression parser.
pub mod binding_power {
    pub const LOWEST: u8 = 0;
    pub const YIELD: u8 = 1;
    pub const WORD_OR: u8 = 5;
    pub const WORD_XOR: u8 = 7;
    pub const WORD_AND: u8 = 9;
    pub const ASSIGNMENT: u8 = 20; // right-associative: (20, 19)
    pub const TERNARY: u8 = 25;    // right-associative: (25, 24)
    pub const NULL_COALESCE: u8 = 30; // right-associative: (30, 29)
    pub const LOGICAL_OR: u8 = 35;
    pub const LOGICAL_AND: u8 = 40;
    pub const BITWISE_OR: u8 = 45;
    pub const BITWISE_XOR: u8 = 50;
    pub const BITWISE_AND: u8 = 55;
    pub const EQUALITY: u8 = 60;
    pub const RELATIONAL: u8 = 65;
    pub const SHIFT: u8 = 70;
    pub const ADDITIVE: u8 = 75;
    pub const MULTIPLICATIVE: u8 = 80;
    pub const INSTANCEOF: u8 = 85;
    pub const POWER: u8 = 90; // right-associative: (91, 90)
    pub const PREFIX: u8 = 93;
    pub const POSTFIX: u8 = 95;
    pub const CALL_MEMBER_INDEX: u8 = 100;
}

impl<'s> CombinatorStream<'s> {
    /// Convenience helper to parse an expression at the lowest precedence.
    pub fn parse_expr_lowest(&mut self) -> Result<Expr<'s>, ParseError> {
        self.parse_expr(binding_power::LOWEST)
    }

    /// Pratt parser driver: parses expressions using top-down operator precedence.
    pub fn parse_expr(&mut self, min_bp: u8) -> Result<Expr<'s>, ParseError> {
        self.skip_doc_comments();
        let mut left = self.parse_prefix()?;

        loop {
            self.skip_doc_comments();
            let next_tok = match self.peek() {
                Some(t) => t.clone(),
                None => break,
            };

            // 1. Postfix operators (`++`, `--`)
            if let Some(postfix_bp) = postfix_binding_power(&next_tok.token) {
                if postfix_bp < min_bp {
                    break;
                }
                self.advance();
                let op = match next_tok.token {
                    Token::PlusPlus => UnaryOp::PostInc,
                    Token::MinusMinus => UnaryOp::PostDec,
                    _ => unreachable!(),
                };
                let span = left.span.merge(next_tok.span);
                left = Expr::new(
                    ExprKind::Unary {
                        op,
                        expr: Box::new(left),
                        is_prefix: false,
                    },
                    span,
                );
                continue;
            }

            // 2. Infix call, member, index, or binary operators
            if let Some((left_bp, right_bp)) = infix_binding_power(&next_tok.token) {
                if left_bp < min_bp {
                    break;
                }
                left = self.parse_infix(left, next_tok, right_bp)?;
                continue;
            }

            break;
        }

        Ok(left)
    }

    /// Parse prefix expressions (null denotation / nud).
    fn parse_prefix(&mut self) -> Result<Expr<'s>, ParseError> {
        let spanned = match self.advance() {
            Some(t) => t,
            None => {
                return Err(ParseError::new(
                    ParseErrorKind::UnexpectedEof {
                        expected: "expression".to_string(),
                    },
                    self.current_span(),
                ))
            }
        };

        match spanned.token {
            // Literals
            Token::IntLiteral(raw) => {
                let value = spanned
                    .token
                    .parse_int()
                    .unwrap_or(0);
                Ok(Expr::new(
                    ExprKind::Literal(Literal::Int { raw, value }),
                    spanned.span,
                ))
            }
            Token::FloatLiteral(raw) => {
                let value = spanned
                    .token
                    .parse_float()
                    .unwrap_or(0.0);
                Ok(Expr::new(
                    ExprKind::Literal(Literal::Float { raw, value }),
                    spanned.span,
                ))
            }
            Token::SingleQuotedString(_) | Token::BacktickString(_) => {
                let value = spanned
                    .token
                    .unescape_string()
                    .unwrap_or_default();
                Ok(Expr::new(
                    ExprKind::Literal(Literal::String {
                        raw: spanned.token.as_str(),
                        value,
                    }),
                    spanned.span,
                ))
            }
            Token::DoubleQuotedString(raw) => {
                if spanned.token.has_interpolation() {
                    let parts = parse_string_interpolation(raw, spanned.span, self.file_id)?;
                    Ok(Expr::new(
                        ExprKind::Literal(Literal::InterpolatedString { raw, parts }),
                        spanned.span,
                    ))
                } else {
                    let value = spanned
                        .token
                        .unescape_string()
                        .unwrap_or_default();
                    Ok(Expr::new(
                        ExprKind::Literal(Literal::String { raw, value }),
                        spanned.span,
                    ))
                }
            }
            Token::True => Ok(Expr::new(
                ExprKind::Literal(Literal::Bool(true)),
                spanned.span,
            )),
            Token::False => Ok(Expr::new(
                ExprKind::Literal(Literal::Bool(false)),
                spanned.span,
            )),
            Token::Null => Ok(Expr::new(ExprKind::Literal(Literal::Null), spanned.span)),

            // Variables
            Token::Variable(name) => Ok(Expr::new(
                ExprKind::Variable(Variable::new(name, spanned.span)),
                spanned.span,
            )),

            // Identifiers & Contextual Type Identifiers
            Token::Ident(name) => Ok(Expr::new(
                ExprKind::Ident(Ident::new(name, spanned.span)),
                spanned.span,
            )),
            Token::TypeList | Token::TypeDict | Token::TypeSet | Token::TypeHashMap => Ok(Expr::new(
                ExprKind::Ident(Ident::new(spanned.token.as_str(), spanned.span)),
                spanned.span,
            )),

            // Unary Prefix Operators
            Token::Plus => {
                let inner = self.parse_expr(binding_power::PREFIX)?;
                let span = spanned.span.merge(inner.span);
                Ok(Expr::new(
                    ExprKind::Unary {
                        op: UnaryOp::Plus,
                        expr: Box::new(inner),
                        is_prefix: true,
                    },
                    span,
                ))
            }
            Token::Minus => {
                let inner = self.parse_expr(binding_power::PREFIX)?;
                let span = spanned.span.merge(inner.span);
                Ok(Expr::new(
                    ExprKind::Unary {
                        op: UnaryOp::Neg,
                        expr: Box::new(inner),
                        is_prefix: true,
                    },
                    span,
                ))
            }
            Token::Bang => {
                let inner = self.parse_expr(binding_power::PREFIX)?;
                let span = spanned.span.merge(inner.span);
                Ok(Expr::new(
                    ExprKind::Unary {
                        op: UnaryOp::Not,
                        expr: Box::new(inner),
                        is_prefix: true,
                    },
                    span,
                ))
            }
            Token::Tilde => {
                let inner = self.parse_expr(binding_power::PREFIX)?;
                let span = spanned.span.merge(inner.span);
                Ok(Expr::new(
                    ExprKind::Unary {
                        op: UnaryOp::BitNot,
                        expr: Box::new(inner),
                        is_prefix: true,
                    },
                    span,
                ))
            }
            Token::PlusPlus => {
                let inner = self.parse_expr(binding_power::PREFIX)?;
                let span = spanned.span.merge(inner.span);
                Ok(Expr::new(
                    ExprKind::Unary {
                        op: UnaryOp::PreInc,
                        expr: Box::new(inner),
                        is_prefix: true,
                    },
                    span,
                ))
            }
            Token::MinusMinus => {
                let inner = self.parse_expr(binding_power::PREFIX)?;
                let span = spanned.span.merge(inner.span);
                Ok(Expr::new(
                    ExprKind::Unary {
                        op: UnaryOp::PreDec,
                        expr: Box::new(inner),
                        is_prefix: true,
                    },
                    span,
                ))
            }
            Token::At => {
                let inner = self.parse_expr(binding_power::PREFIX)?;
                let span = spanned.span.merge(inner.span);
                Ok(Expr::new(
                    ExprKind::Unary {
                        op: UnaryOp::Silence,
                        expr: Box::new(inner),
                        is_prefix: true,
                    },
                    span,
                ))
            }
            Token::Clone => {
                let inner = self.parse_expr(binding_power::PREFIX)?;
                let span = spanned.span.merge(inner.span);
                Ok(Expr::new(ExprKind::Clone(Box::new(inner)), span))
            }
            Token::Await => {
                let inner = self.parse_expr(binding_power::PREFIX)?;
                let span = spanned.span.merge(inner.span);
                Ok(Expr::new(ExprKind::Await(Box::new(inner)), span))
            }

            // Yield expression (`yield`, `yield from`, `yield $k => $v`)
            Token::Yield => {
                let start_span = spanned.span;
                let is_from = self.matches(&Token::From);
                if self.check(&Token::SemiColon) || self.check(&Token::CloseParen) || self.is_eof() {
                    return Ok(Expr::new(
                        ExprKind::Yield {
                            key: None,
                            value: None,
                            is_from,
                        },
                        start_span,
                    ));
                }

                let first = self.parse_expr(binding_power::YIELD)?;
                if self.matches(&Token::DoubleArrow) {
                    let second = self.parse_expr(binding_power::YIELD)?;
                    let span = start_span.merge(second.span);
                    Ok(Expr::new(
                        ExprKind::Yield {
                            key: Some(Box::new(first)),
                            value: Some(Box::new(second)),
                            is_from,
                        },
                        span,
                    ))
                } else {
                    let span = start_span.merge(first.span);
                    Ok(Expr::new(
                        ExprKind::Yield {
                            key: None,
                            value: Some(Box::new(first)),
                            is_from,
                        },
                        span,
                    ))
                }
            }

            // Parentheses: type casting or grouping
            Token::OpenParen => {
                // Check if this is a type cast `(int) $x`, `(string) $x`, etc.
                if let Some(next) = self.peek() {
                    let cast_type = match next.token {
                        Token::TypeInt => Some(CastType::Int),
                        Token::TypeFloat => Some(CastType::Float),
                        Token::TypeString => Some(CastType::String),
                        Token::TypeBool => Some(CastType::Bool),
                        Token::TypeArray => Some(CastType::Array),
                        Token::TypeObject => Some(CastType::Object),
                        Token::Ident("integer") => Some(CastType::Int),
                        Token::Ident("double") | Token::Ident("real") => Some(CastType::Float),
                        Token::Ident("boolean") => Some(CastType::Bool),
                        Token::Ident("binary") => Some(CastType::String),
                        _ => None,
                    };

                    if let Some(kind) = cast_type {
                        if self.peek_nth(1).map(|t| &t.token) == Some(&Token::CloseParen) {
                            self.advance(); // consume type
                            self.advance(); // consume CloseParen
                            let expr = self.parse_expr(binding_power::PREFIX)?;
                            let span = spanned.span.merge(expr.span);
                            return Ok(Expr::new(ExprKind::Cast { kind, expr: Box::new(expr) }, span));
                        }
                    }
                }

                // Otherwise standard grouping `(expr)`
                let inner = self.parse_expr_lowest()?;
                let close_tok = self.expect(Token::CloseParen)?;
                let span = spanned.span.merge(close_tok.span);
                Ok(Expr::new(ExprKind::Grouping(Box::new(inner)), span))
            }

            // Array literal `[...]` or array destructuring
            Token::OpenBracket => {
                let elements = self.separated_by(
                    |s| {
                        let el_start = s.current_span();
                        let is_unpack = s.matches(&Token::Ellipsis);
                        let first = s.parse_expr_lowest()?;

                        if s.matches(&Token::DoubleArrow) {
                            let value = s.parse_expr_lowest()?;
                            let span = el_start.merge(value.span);
                            Ok(ArrayElement {
                                span,
                                key: Some(first),
                                value,
                                is_unpack,
                            })
                        } else {
                            let span = el_start.merge(first.span);
                            Ok(ArrayElement {
                                span,
                                key: None,
                                value: first,
                                is_unpack,
                            })
                        }
                    },
                    Token::Comma,
                    &Token::CloseBracket,
                )?;
                let close_tok = self.expect(Token::CloseBracket)?;
                let span = spanned.span.merge(close_tok.span);
                Ok(Expr::new(ExprKind::Array(elements), span))
            }

            // Arrow function `fn($n) => $n * 2`
            Token::Fn => {
                let params = self.parse_formal_parameters()?;
                let return_type = if self.matches(&Token::Colon) {
                    Some(self.parse_type_hint()?)
                } else {
                    None
                };
                self.expect(Token::DoubleArrow)?;
                let body = self.parse_expr(binding_power::LOWEST)?;
                let span = spanned.span.merge(body.span);
                Ok(Expr::new(
                    ExprKind::ArrowFunction {
                        params,
                        return_type,
                        body: Box::new(body),
                    },
                    span,
                ))
            }

            // Closure expression `function($x) use ($y) { ... }`
            Token::Function => {
                let params = self.parse_formal_parameters()?;
                let mut uses = Vec::new();
                if self.matches(&Token::Use) {
                    self.expect(Token::OpenParen)?;
                    uses = self.separated_by(
                        |s| {
                            let u_start = s.current_span();
                            let by_ref = s.matches(&Token::Ampersand);
                            let var = s.expect_variable()?;
                            let span = u_start.merge(var.span);
                            Ok(ClosureUse { span, var, by_ref })
                        },
                        Token::Comma,
                        &Token::CloseParen,
                    )?;
                    self.expect(Token::CloseParen)?;
                }

                let return_type = if self.matches(&Token::Colon) {
                    Some(self.parse_type_hint()?)
                } else {
                    None
                };

                let body = self.parse_block_statements()?;
                let end_span = self.current_span();
                let span = spanned.span.merge(end_span);
                Ok(Expr::new(
                    ExprKind::Closure {
                        is_static: false,
                        params,
                        uses,
                        return_type,
                        body,
                    },
                    span,
                ))
            }

            // Match expression `match ($val) { 1 => 'one', default => 'other' }`
            Token::Match => {
                self.expect(Token::OpenParen)?;
                let condition = self.parse_expr_lowest()?;
                self.expect(Token::CloseParen)?;
                self.expect(Token::OpenBrace)?;

                let arms = self.separated_by(
                    |s| {
                        let arm_start = s.current_span();
                        let (patterns, is_default) = if s.matches(&Token::Default) {
                            (Vec::new(), true)
                        } else {
                            let pats = s.separated_by(
                                |p| p.parse_expr_lowest(),
                                Token::Comma,
                                &Token::DoubleArrow,
                            )?;
                            (pats, false)
                        };

                        s.expect(Token::DoubleArrow)?;
                        let body = s.parse_expr_lowest()?;
                        let span = arm_start.merge(body.span);
                        Ok(MatchArm {
                            span,
                            patterns,
                            is_default,
                            body,
                        })
                    },
                    Token::Comma,
                    &Token::CloseBrace,
                )?;

                let close_brace = self.expect(Token::CloseBrace)?;
                let span = spanned.span.merge(close_brace.span);
                Ok(Expr::new(
                    ExprKind::Match {
                        condition: Box::new(condition),
                        arms,
                    },
                    span,
                ))
            }

            // New expression `new User($name)` or `new $className($name)`
            Token::New => {
                let start_span = spanned.span;
                let (class_expr, end_target_span) = if matches!(
                    self.peek().map(|t| &t.token),
                    Some(Token::Variable(_))
                ) {
                    let var = self.expect_variable()?;
                    let v_span = var.span;
                    (Expr::new(ExprKind::Variable(var), v_span), v_span)
                } else {
                    let class_ident = self.expect_ident()?;

                    // Static facade instantiation is strictly forbidden by specification
                    if is_static_facade(class_ident.name) || is_static_only(class_ident.name) {
                        return Err(ParseError::new(
                            ParseErrorKind::StaticFacadeInstantiationForbidden {
                                class_name: class_ident.name.to_string(),
                            },
                            class_ident.span,
                        ));
                    }

                    let id_span = class_ident.span;
                    (
                        Expr::new(ExprKind::Ident(class_ident), id_span),
                        id_span,
                    )
                };

                let mut args = Vec::new();
                let mut end_span = end_target_span;
                if self.matches(&Token::OpenParen) {
                    args = self.parse_call_arguments()?;
                    let close_paren = self.expect(Token::CloseParen)?;
                    end_span = close_paren.span;
                }

                let span = start_span.merge(end_span);
                Ok(Expr::new(
                    ExprKind::New {
                        class: Box::new(class_expr),
                        args,
                    },
                    span,
                ))
            }

            Token::TypeInt | Token::TypeString | Token::TypeFloat | Token::TypeBool
                if self.check(&Token::OpenParen) =>
            {
                let type_name = spanned.token.as_str();
                Err(ParseError::new(
                    ParseErrorKind::Custom {
                        message: format!(
                            "Invalid type cast syntax '{}(...)': use native PHP casting '({}) $x' instead to avoid confusion with object instantiation.",
                            type_name, type_name
                        ),
                    },
                    spanned.span,
                ))
            }

            _ => Err(ParseError::new(
                ParseErrorKind::UnexpectedToken {
                    expected: "expression".to_string(),
                    found: spanned.token.as_str().to_string(),
                },
                spanned.span,
            )),
        }
    }

    /// Parse infix expressions (left denotation / led).
    fn parse_infix(
        &mut self,
        left: Expr<'s>,
        op_tok: SpannedToken<'s>,
        right_bp: u8,
    ) -> Result<Expr<'s>, ParseError> {
        self.advance(); // consume the operator token

        match op_tok.token {
            // Function or Method invocation call `left(...)`
            Token::OpenParen => {
                let args = self.parse_call_arguments()?;
                let close_tok = self.expect(Token::CloseParen)?;
                let span = left.span.merge(close_tok.span);

                // Symbol Resolution Check & AST Rewriting (Lowering) for global function calls:
                // 1. Reserved keyword intrinsic (`env`, `print`, `dd`) -> rewrites to `Sys::*`
                // 2. Declared user function -> calls standard user function `ExprKind::Call`
                // 3. Legacy PHP built-in function (`array_merge`, `implode`, etc.) -> rewrites to `phpFun::<callee_name>(args)`
                // 4. Unrecognized global function -> rewrites to `Glob::customHelper(Literal(callee_name), ...args)`
                if let ExprKind::Ident(ref callee_ident) = left.kind {
                    let callee_name = callee_ident.name;
                    let callee_span = callee_ident.span;

                    let call_node = if is_reserved_intrinsic(callee_name) {
                        // Rewrites to Sys::* (e.g. Sys::env(), Sys::print(), Sys::dd())
                        Expr::static_call("Sys", callee_name, args, callee_span, span)
                    } else if self.is_user_defined_function(callee_name) {
                        // Calls standard user function
                        Expr::user_function_call(callee_name, args, callee_span, span)
                    } else if is_legacy_php_function(callee_name) {
                        // Backward support for standard PHP functions: rewrites to phpFun::<callee_name>(args)
                        // which dispatches to the mapped RHP facade signature (e.g. array_merge -> phpFun::array_merge() -> Arr::merge())
                        Expr::static_call("phpFun", callee_name, args, callee_span, span)
                    } else {
                        // Fallback: Transforms legacy custom helper into Glob::customHelper('name', ...args)
                        let mut custom_args = Vec::with_capacity(1 + args.len());
                        custom_args.push(Argument::new(
                            Expr::new(
                                ExprKind::Literal(Literal::String {
                                    raw: callee_name,
                                    value: callee_name.to_string(),
                                }),
                                callee_span,
                            ),
                            callee_span,
                        ));
                        custom_args.extend(args);
                        Expr::static_call("Glob", "customHelper", custom_args, callee_span, span)
                    };

                    return Ok(call_node);
                }

                Ok(Expr::new(
                    ExprKind::Call {
                        callee: Box::new(left),
                        args,
                    },
                    span,
                ))
            }

            // Array index `left[...]`
            Token::OpenBracket => {
                if self.matches(&Token::CloseBracket) {
                    let span = left.span.merge(op_tok.span);
                    Ok(Expr::new(
                        ExprKind::ArrayIndex {
                            array: Box::new(left),
                            index: None,
                        },
                        span,
                    ))
                } else {
                    let index = self.parse_expr_lowest()?;
                    let close_tok = self.expect(Token::CloseBracket)?;
                    let span = left.span.merge(close_tok.span);
                    Ok(Expr::new(
                        ExprKind::ArrayIndex {
                            array: Box::new(left),
                            index: Some(Box::new(index)),
                        },
                        span,
                    ))
                }
            }

            // Member access `left->member` or nullsafe `left?->member`
            Token::Arrow | Token::NullsafeArrow => {
                let is_nullsafe = matches!(op_tok.token, Token::NullsafeArrow);
                let member_name = self.expect_ident()?;

                // If followed by `(`, this is a method call `$obj->method(...)`
                if self.matches(&Token::OpenParen) {
                    let args = self.parse_call_arguments()?;
                    let close_paren = self.expect(Token::CloseParen)?;
                    let span = left.span.merge(close_paren.span);
                    Ok(Expr::new(
                        ExprKind::MethodCall {
                            object: Box::new(left),
                            method: member_name,
                            args,
                            is_nullsafe,
                        },
                        span,
                    ))
                } else {
                    let span = left.span.merge(member_name.span);
                    Ok(Expr::new(
                        ExprKind::PropertyFetch {
                            object: Box::new(left),
                            property: member_name,
                            is_nullsafe,
                        },
                        span,
                    ))
                }
            }

            // Scope resolution `Target::member`
            Token::DoubleColon => {
                if let Some(tok) = self.peek() {
                    if let Token::Variable(var_name) = tok.token {
                        let var_tok = self.advance().unwrap();
                        let span = left.span.merge(var_tok.span);
                        let class_ident = match &left.kind {
                            ExprKind::Ident(id) => id.clone(),
                            _ => Ident::new("", left.span),
                        };
                        return Ok(Expr::new(
                            ExprKind::StaticPropertyFetch {
                                class: class_ident,
                                property: Variable::new(var_name, var_tok.span),
                            },
                            span,
                        ));
                    }
                }

                let member_ident = self.expect_ident()?;

                // If followed by `(`, this is a static method invocation `Facade::method(...)`
                if self.matches(&Token::OpenParen) {
                    let args = self.parse_call_arguments()?;
                    let close_paren = self.expect(Token::CloseParen)?;
                    let span = left.span.merge(close_paren.span);

                    // Validate and register runtime macro definitions (`Arr::macro('toCsv', function ($delimiter = ',') { ... })`)
                    if let ExprKind::Ident(ref class_id) = left.kind {
                        if is_static_facade(class_id.name) && member_ident.name == "macro" {
                            if args.len() != 2 {
                                return Err(ParseError::new(
                                    ParseErrorKind::InvalidFacadeMacroRegistration {
                                        class_name: class_id.name.to_string(),
                                        reason: format!(
                                            "expected exactly 2 arguments (macro name and callable handler), found {}",
                                            args.len()
                                        ),
                                    },
                                    span,
                                ));
                            }

                            if let ExprKind::Literal(Literal::String {
                                value: ref macro_name,
                                ..
                            }) = args[0].value.kind
                            {
                                if macro_name.trim().is_empty() {
                                    return Err(ParseError::new(
                                        ParseErrorKind::InvalidFacadeMacroRegistration {
                                            class_name: class_id.name.to_string(),
                                            reason: "macro name cannot be empty".to_string(),
                                        },
                                        args[0].span,
                                    ));
                                }

                                let params = match &args[1].value.kind {
                                    ExprKind::Closure { params, .. }
                                    | ExprKind::ArrowFunction { params, .. } => params
                                        .iter()
                                        .map(|p| {
                                            let def = p.default_value.as_ref().map(|d| format!("{:?}", d.kind));
                                            (p.name.name.to_string(), def)
                                        })
                                        .collect(),
                                    _ => Vec::new(),
                                };

                                let _ = register_macro(
                                    class_id.name,
                                    macro_name,
                                    params,
                                    &format!("{:?}", args[1].value.kind),
                                );
                            }
                        }
                    }

                    Ok(Expr::new(
                        ExprKind::StaticCall {
                            target: Box::new(left),
                            method: member_ident,
                            args,
                        },
                        span,
                    ))
                } else {
                    let span = left.span.merge(member_ident.span);
                    let class_ident = match &left.kind {
                        ExprKind::Ident(id) => id.clone(),
                        _ => Ident::new("", left.span),
                    };
                    Ok(Expr::new(
                        ExprKind::ClassConstFetch {
                            class: class_ident,
                            const_name: member_ident,
                        },
                        span,
                    ))
                }
            }

            // Ternary `$cond ? $then : $else` or Elvis `$cond ?: $else`
            Token::Question => {
                let if_true = self.parse_expr_lowest()?;
                self.expect(Token::Colon)?;
                let if_false = self.parse_expr(right_bp)?;
                let span = left.span.merge(if_false.span);
                Ok(Expr::new(
                    ExprKind::Ternary {
                        condition: Box::new(left),
                        if_true: Some(Box::new(if_true)),
                        if_false: Box::new(if_false),
                    },
                    span,
                ))
            }
            Token::Elvis => {
                let if_false = self.parse_expr(right_bp)?;
                let span = left.span.merge(if_false.span);
                Ok(Expr::new(
                    ExprKind::Ternary {
                        condition: Box::new(left),
                        if_true: None,
                        if_false: Box::new(if_false),
                    },
                    span,
                ))
            }

            // Instanceof
            Token::Instanceof => {
                let right = self.parse_expr(right_bp)?;
                let span = left.span.merge(right.span);
                Ok(Expr::new(
                    ExprKind::Instanceof {
                        expr: Box::new(left),
                        class: Box::new(right),
                    },
                    span,
                ))
            }

            // Binary & Assignment Operators
            _ => {
                if let Some(assign_op) = to_assign_op(&op_tok.token) {
                    let right = self.parse_expr(right_bp)?;
                    let span = left.span.merge(right.span);
                    Ok(Expr::new(
                        ExprKind::Assign {
                            op: assign_op,
                            target: Box::new(left),
                            value: Box::new(right),
                        },
                        span,
                    ))
                } else if let Some(bin_op) = to_binary_op(&op_tok.token) {
                    let right = self.parse_expr(right_bp)?;
                    let span = left.span.merge(right.span);
                    Ok(Expr::new(
                        ExprKind::Binary {
                            op: bin_op,
                            left: Box::new(left),
                            right: Box::new(right),
                        },
                        span,
                    ))
                } else {
                    Err(ParseError::new(
                        ParseErrorKind::UnexpectedToken {
                            expected: "infix operator".to_string(),
                            found: op_tok.token.as_str().to_string(),
                        },
                        op_tok.span,
                    ))
                }
            }
        }
    }

    /// Parse call argument list inside parentheses `(...)`.
    pub fn parse_call_arguments(&mut self) -> Result<Vec<Argument<'s>>, ParseError> {
        self.separated_by(
            |s| {
                let arg_start = s.current_span();
                let is_unpack = s.matches(&Token::Ellipsis);

                // Named argument check `name: $val`
                if let Some(tok) = s.peek() {
                    if (tok.token.is_keyword() || matches!(tok.token, Token::Ident(_)))
                        && s.peek_nth(1).map(|t| &t.token) == Some(&Token::Colon)
                    {
                        let name = s.expect_ident()?;
                        s.expect(Token::Colon)?;
                        let value = s.parse_expr_lowest()?;
                        let span = arg_start.merge(value.span);
                        return Ok(Argument {
                            span,
                            name: Some(name),
                            value,
                            is_unpack,
                        });
                    }
                }

                let value = s.parse_expr_lowest()?;
                let span = arg_start.merge(value.span);
                Ok(Argument {
                    span,
                    name: None,
                    value,
                    is_unpack,
                })
            },
            Token::Comma,
            &Token::CloseParen,
        )
    }

    /// Parse formal function or method parameter list `(...)`.
    pub fn parse_formal_parameters(&mut self) -> Result<Vec<Param<'s>>, ParseError> {
        self.expect(Token::OpenParen)?;
        let params = self.separated_by(
            |s| {
                let start_span = s.current_span();
                let attributes = s.parse_attributes()?;

                // Visibility / constructor property promotion
                let visibility = if s.matches(&Token::Public) {
                    Some(crate::ast::types::Visibility::Public)
                } else if s.matches(&Token::Protected) {
                    Some(crate::ast::types::Visibility::Protected)
                } else if s.matches(&Token::Private) {
                    Some(crate::ast::types::Visibility::Private)
                } else {
                    None
                };

                let is_readonly = s.matches(&Token::Readonly);

                // Type hint (optional if gradual typing in local scope)
                let type_hint = if !s.check_fn(|t| matches!(t, Token::Variable(_) | Token::Ampersand | Token::Ellipsis)) {
                    Some(s.parse_type_hint()?)
                } else {
                    None
                };

                let is_reference = s.matches(&Token::Ampersand);
                let is_variadic = s.matches(&Token::Ellipsis);
                let name = s.expect_variable()?;

                // Default parameter value
                let default_value = if s.matches(&Token::Assign) {
                    Some(s.parse_expr_lowest()?)
                } else {
                    None
                };

                let end_span = default_value
                    .as_ref()
                    .map(|e| e.span)
                    .unwrap_or(name.span);
                let span = start_span.merge(end_span);

                Ok(Param {
                    span,
                    attributes,
                    visibility,
                    is_readonly,
                    type_hint,
                    is_variadic,
                    is_reference,
                    name,
                    default_value,
                })
            },
            Token::Comma,
            &Token::CloseParen,
        )?;
        self.expect(Token::CloseParen)?;
        Ok(params)
    }

    /// Parse a block of statements enclosed by braces `{ ... }`.
    pub fn parse_block_statements(&mut self) -> Result<Vec<Stmt<'s>>, ParseError> {
        self.expect(Token::OpenBrace)?;
        let mut stmts = Vec::new();
        while !self.check(&Token::CloseBrace) && !self.is_eof() {
            self.skip_php_tags();
            self.skip_doc_comments();
            if self.check(&Token::CloseBrace) {
                break;
            }
            stmts.push(self.parse_stmt()?);
        }
        self.expect(Token::CloseBrace)?;
        Ok(stmts)
    }
}

/// Helper mapping token to postfix binding power.
fn postfix_binding_power(token: &Token<'_>) -> Option<u8> {
    match token {
        Token::PlusPlus | Token::MinusMinus => Some(binding_power::POSTFIX),
        _ => None,
    }
}

/// Helper mapping token to (left_bp, right_bp) for infix Pratt parsing.
fn infix_binding_power(token: &Token<'_>) -> Option<(u8, u8)> {
    match token {
        // High precedence accessors
        Token::OpenParen | Token::OpenBracket | Token::Arrow | Token::NullsafeArrow | Token::DoubleColon => {
            Some((binding_power::CALL_MEMBER_INDEX, binding_power::CALL_MEMBER_INDEX + 1))
        }

        // Power operator is right-associative (higher left bp)
        Token::StarStar => Some((binding_power::POWER + 1, binding_power::POWER)),

        // Multiplicative
        Token::Star | Token::Slash | Token::Percent => {
            Some((binding_power::MULTIPLICATIVE, binding_power::MULTIPLICATIVE + 1))
        }

        // Additive & string concatenation
        Token::Plus | Token::Minus | Token::Dot => {
            Some((binding_power::ADDITIVE, binding_power::ADDITIVE + 1))
        }

        // Bitwise shift
        Token::ShiftLeft | Token::ShiftRight => {
            Some((binding_power::SHIFT, binding_power::SHIFT + 1))
        }

        // Relational
        Token::LessThan | Token::GreaterThan | Token::LessThanOrEqual | Token::GreaterThanOrEqual | Token::Spaceship => {
            Some((binding_power::RELATIONAL, binding_power::RELATIONAL + 1))
        }

        // Instanceof
        Token::Instanceof => Some((binding_power::INSTANCEOF, binding_power::INSTANCEOF + 1)),

        // Equality
        Token::Equal | Token::NotEqual | Token::NotEqualAlt | Token::Identical | Token::NotIdentical => {
            Some((binding_power::EQUALITY, binding_power::EQUALITY + 1))
        }

        // Bitwise logic
        Token::Ampersand => Some((binding_power::BITWISE_AND, binding_power::BITWISE_AND + 1)),
        Token::Caret => Some((binding_power::BITWISE_XOR, binding_power::BITWISE_XOR + 1)),
        Token::Pipe => Some((binding_power::BITWISE_OR, binding_power::BITWISE_OR + 1)),

        // Logical
        Token::AmpersandAmpersand => Some((binding_power::LOGICAL_AND, binding_power::LOGICAL_AND + 1)),
        Token::PipePipe => Some((binding_power::LOGICAL_OR, binding_power::LOGICAL_OR + 1)),

        // Null coalescing is right-associative
        Token::NullCoalesce => Some((binding_power::NULL_COALESCE + 1, binding_power::NULL_COALESCE)),

        // Ternary & Elvis
        Token::Question | Token::Elvis => Some((binding_power::TERNARY + 1, binding_power::TERNARY)),

        // Assignment operators are right-associative
        Token::Assign
        | Token::PlusAssign
        | Token::MinusAssign
        | Token::StarAssign
        | Token::SlashAssign
        | Token::PercentAssign
        | Token::StarStarAssign
        | Token::DotAssign
        | Token::NullCoalesceAssign
        | Token::AmpersandAssign
        | Token::PipeAssign
        | Token::CaretAssign
        | Token::ShiftLeftAssign
        | Token::ShiftRightAssign => Some((binding_power::ASSIGNMENT + 1, binding_power::ASSIGNMENT)),

        // Logical word operators
        Token::And => Some((binding_power::WORD_AND, binding_power::WORD_AND + 1)),
        Token::Xor => Some((binding_power::WORD_XOR, binding_power::WORD_XOR + 1)),
        Token::Or => Some((binding_power::WORD_OR, binding_power::WORD_OR + 1)),

        _ => None,
    }
}

/// Convert token to BinaryOp if applicable.
fn to_binary_op(token: &Token<'_>) -> Option<BinaryOp> {
    match token {
        Token::Plus => Some(BinaryOp::Add),
        Token::Minus => Some(BinaryOp::Sub),
        Token::Star => Some(BinaryOp::Mul),
        Token::Slash => Some(BinaryOp::Div),
        Token::Percent => Some(BinaryOp::Mod),
        Token::StarStar => Some(BinaryOp::Pow),
        Token::Dot => Some(BinaryOp::Concat),
        Token::Ampersand => Some(BinaryOp::BitAnd),
        Token::Pipe => Some(BinaryOp::BitOr),
        Token::Caret => Some(BinaryOp::BitXor),
        Token::ShiftLeft => Some(BinaryOp::Shl),
        Token::ShiftRight => Some(BinaryOp::Shr),
        Token::Equal => Some(BinaryOp::Eq),
        Token::Identical => Some(BinaryOp::Identical),
        Token::NotEqual | Token::NotEqualAlt => Some(BinaryOp::NotEq),
        Token::NotIdentical => Some(BinaryOp::NotIdentical),
        Token::LessThan => Some(BinaryOp::Lt),
        Token::GreaterThan => Some(BinaryOp::Gt),
        Token::LessThanOrEqual => Some(BinaryOp::LtEq),
        Token::GreaterThanOrEqual => Some(BinaryOp::GtEq),
        Token::Spaceship => Some(BinaryOp::Spaceship),
        Token::AmpersandAmpersand => Some(BinaryOp::LogicalAnd),
        Token::PipePipe => Some(BinaryOp::LogicalOr),
        Token::And => Some(BinaryOp::WordAnd),
        Token::Or => Some(BinaryOp::WordOr),
        Token::Xor => Some(BinaryOp::WordXor),
        Token::NullCoalesce => Some(BinaryOp::Coalesce),
        _ => None,
    }
}

/// Convert token to AssignOp if applicable.
fn to_assign_op(token: &Token<'_>) -> Option<AssignOp> {
    match token {
        Token::Assign => Some(AssignOp::Assign),
        Token::PlusAssign => Some(AssignOp::AddAssign),
        Token::MinusAssign => Some(AssignOp::SubAssign),
        Token::StarAssign => Some(AssignOp::MulAssign),
        Token::SlashAssign => Some(AssignOp::DivAssign),
        Token::PercentAssign => Some(AssignOp::ModAssign),
        Token::StarStarAssign => Some(AssignOp::PowAssign),
        Token::DotAssign => Some(AssignOp::ConcatAssign),
        Token::NullCoalesceAssign => Some(AssignOp::CoalesceAssign),
        Token::AmpersandAssign => Some(AssignOp::BitAndAssign),
        Token::PipeAssign => Some(AssignOp::BitOrAssign),
        Token::CaretAssign => Some(AssignOp::BitXorAssign),
        Token::ShiftLeftAssign => Some(AssignOp::ShlAssign),
        Token::ShiftRightAssign => Some(AssignOp::ShrAssign),
        _ => None,
    }
}

/// Mini-parser for double-quoted string interpolation (`"user={$id}"` or `"hello $name"`).
fn parse_string_interpolation<'s>(
    raw: &'s str,
    span: Span,
    file_id: usize,
) -> Result<Vec<StringPart<'s>>, ParseError> {
    let mut parts = Vec::new();
    // Strip surrounding quotes
    let inner = if raw.len() >= 2 {
        &raw[1..raw.len() - 1]
    } else {
        raw
    };

    let mut current_lit = String::new();
    let bytes = inner.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'$' {
            if !current_lit.is_empty() {
                parts.push(StringPart::Literal(std::mem::take(&mut current_lit)));
            }
            // Find closing '}'
            let start_expr = i + 1;
            let mut j = start_expr;
            while j < bytes.len() && bytes[j] != b'}' {
                j += 1;
            }
            if j < bytes.len() {
                let expr_str = &inner[start_expr..j];
                let tokens = Lexer::tokenize_with_file_id(expr_str, file_id)
                    .map_err(|e| ParseError::new(ParseErrorKind::Lex(e.error), span))?;
                let mut stream = CombinatorStream::new(tokens, file_id);
                let expr = stream.parse_expr_lowest()?;
                parts.push(StringPart::Expr(expr));
                i = j + 1;
                continue;
            }
        } else if bytes[i] == b'$' {
            if !current_lit.is_empty() {
                parts.push(StringPart::Literal(std::mem::take(&mut current_lit)));
            }
            let start_var = i;
            if i + 1 >= bytes.len()
                || !(bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_' || bytes[i + 1] >= 0x7f)
            {
                let var_span = Span::new(
                    file_id,
                    span.start + 1 + start_var,
                    span.start + 1 + (i + 2).min(bytes.len()),
                    span.line,
                    span.col,
                );
                return Err(ParseError::new(
                    ParseErrorKind::Lex(crate::lexer::error::LexError::InvalidVariableIdentifier(
                        "Variable names must start with a letter or underscore, followed by alphanumeric characters (e.g., $varName).".to_string(),
                    )),
                    var_span,
                ));
            }
            let mut j = i + 1;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] >= 0x7f)
            {
                j += 1;
            }
            let var_str = &inner[start_var..j];
            let var_span = Span::new(file_id, span.start + 1 + start_var, span.start + 1 + j, span.line, span.col);
            let var_expr = Expr::new(
                ExprKind::Variable(Variable::new(var_str, var_span)),
                var_span,
            );
            parts.push(StringPart::Expr(var_expr));
            i = j;
            continue;
        } else if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'n' => current_lit.push('\n'),
                b'r' => current_lit.push('\r'),
                b't' => current_lit.push('\t'),
                b'\\' => current_lit.push('\\'),
                b'"' => current_lit.push('"'),
                b'$' => current_lit.push('$'),
                other => {
                    current_lit.push('\\');
                    current_lit.push(other as char);
                }
            }
            i += 2;
            continue;
        } else {
            current_lit.push(bytes[i] as char);
            i += 1;
        }
    }

    if !current_lit.is_empty() {
        parts.push(StringPart::Literal(current_lit));
    }

    Ok(parts)
}
