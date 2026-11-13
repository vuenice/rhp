use crate::ast::{
    expr::{
        Argument, ArrayElement, AssignOp, BinaryOp, ClosureUse, Expr, ExprKind, Literal, MatchArm,
        StringPart, UnaryOp,
    },
    stmt::{ClassDecl, Param, Stmt},
    types::{CastType, ClassModifiers, Ident, Variable},
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
    pub const RANGE: u8 = 22;      // left-associative range operator: `..`
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

            // Explicitly reject removed '.' and '.=' concatenation operators
            if matches!(next_tok.token, Token::Dot) {
                return Err(ParseError::new(
                    ParseErrorKind::Custom {
                        message: "The '.' concatenation operator has been removed. Use '+' for string concatenation.".to_string(),
                    },
                    next_tok.span,
                ));
            }
            if matches!(next_tok.token, Token::DotAssign) {
                return Err(ParseError::new(
                    ParseErrorKind::Custom {
                        message: "The '.=' operator has been removed. Use '+=' for string concatenation assignment.".to_string(),
                    },
                    next_tok.span,
                ));
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

    fn unroll_array_indices(&self, expr: &Expr<'s>) -> (Expr<'s>, Vec<Expr<'s>>) {
        let mut curr = expr;
        let mut indices = Vec::new();
        while let ExprKind::ArrayIndex { array, index } = &curr.kind {
            if let Some(idx) = index {
                indices.push((**idx).clone());
            }
            curr = array;
        }
        indices.reverse();
        (curr.clone(), indices)
    }

    fn lower_isset_operand(&self, operand: &Expr<'s>) -> Result<Expr<'s>, ParseError> {
        match &operand.kind {
            ExprKind::Variable(var) => Err(ParseError::new(
                ParseErrorKind::BareVariableIssetForbidden {
                    var_name: var.name.to_string(),
                },
                operand.span,
            )),
            ExprKind::ArrayIndex { .. } => {
                let (root, indices) = self.unroll_array_indices(operand);
                let mut curr = root;
                for (i, idx) in indices.into_iter().enumerate() {
                    let is_nullsafe = i > 0;
                    let span = curr.span.merge(idx.span);
                    curr = Expr::new(
                        ExprKind::MethodCall {
                            object: Box::new(curr),
                            method: Ident::new("get", span),
                            args: vec![Argument::new(idx, span)],
                            is_nullsafe,
                        },
                        span,
                    );
                }
                let null_expr = Expr::new(ExprKind::Literal(Literal::Null), operand.span);
                Ok(Expr::new(
                    ExprKind::Binary {
                        left: Box::new(curr),
                        op: BinaryOp::NotEq,
                        right: Box::new(null_expr),
                    },
                    operand.span,
                ))
            }
            ExprKind::PropertyFetch {
                object,
                property,
                ..
            } => {
                let nullsafe_prop = Expr::new(
                    ExprKind::PropertyFetch {
                        object: object.clone(),
                        property: property.clone(),
                        is_nullsafe: true,
                    },
                    operand.span,
                );
                let null_expr = Expr::new(ExprKind::Literal(Literal::Null), operand.span);
                Ok(Expr::new(
                    ExprKind::Binary {
                        left: Box::new(nullsafe_prop),
                        op: BinaryOp::NotEq,
                        right: Box::new(null_expr),
                    },
                    operand.span,
                ))
            }
            ExprKind::DynamicPropertyFetch {
                object,
                property,
                ..
            } => {
                let nullsafe_prop = Expr::new(
                    ExprKind::DynamicPropertyFetch {
                        object: object.clone(),
                        property: property.clone(),
                        is_nullsafe: true,
                    },
                    operand.span,
                );
                let null_expr = Expr::new(ExprKind::Literal(Literal::Null), operand.span);
                Ok(Expr::new(
                    ExprKind::Binary {
                        left: Box::new(nullsafe_prop),
                        op: BinaryOp::NotEq,
                        right: Box::new(null_expr),
                    },
                    operand.span,
                ))
            }
            _ => {
                let null_expr = Expr::new(ExprKind::Literal(Literal::Null), operand.span);
                Ok(Expr::new(
                    ExprKind::Binary {
                        left: Box::new(operand.clone()),
                        op: BinaryOp::NotEq,
                        right: Box::new(null_expr),
                    },
                    operand.span,
                ))
            }
        }
    }

    fn lower_empty_operand(&self, operand: &Expr<'s>) -> Expr<'s> {
        match &operand.kind {
            ExprKind::ArrayIndex { .. } => {
                let (root, indices) = self.unroll_array_indices(operand);
                let mut curr = root;
                for (i, idx) in indices.into_iter().enumerate() {
                    let is_nullsafe = i > 0;
                    let span = curr.span.merge(idx.span);
                    curr = Expr::new(
                        ExprKind::MethodCall {
                            object: Box::new(curr),
                            method: Ident::new("get", span),
                            args: vec![Argument::new(idx, span)],
                            is_nullsafe,
                        },
                        span,
                    );
                }
                curr
            }
            ExprKind::PropertyFetch {
                object,
                property,
                ..
            } => Expr::new(
                ExprKind::PropertyFetch {
                    object: object.clone(),
                    property: property.clone(),
                    is_nullsafe: true,
                },
                operand.span,
            ),
            ExprKind::DynamicPropertyFetch {
                object,
                property,
                ..
            } => Expr::new(
                ExprKind::DynamicPropertyFetch {
                    object: object.clone(),
                    property: property.clone(),
                    is_nullsafe: true,
                },
                operand.span,
            ),
            _ => operand.clone(),
        }
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
                    .unescape_utf8_string()
                    .map_err(|e| ParseError::new(ParseErrorKind::Custom { message: e }, spanned.span))?;
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
                        .unescape_utf8_string()
                        .map_err(|e| ParseError::new(ParseErrorKind::Custom { message: e }, spanned.span))?;
                    Ok(Expr::new(
                        ExprKind::Literal(Literal::String { raw, value }),
                        spanned.span,
                    ))
                }
            }
            Token::SingleQuotedBytes(_) | Token::DoubleQuotedBytes(_) => {
                let value = spanned
                    .token
                    .unescape_bytes()
                    .map_err(|e| ParseError::new(ParseErrorKind::Custom { message: e }, spanned.span))?;
                Ok(Expr::new(
                    ExprKind::Literal(Literal::Bytes {
                        raw: spanned.token.as_str(),
                        value,
                    }),
                    spanned.span,
                ))
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

            // Magic Constants
            Token::MagicLine => {
                let line = spanned.span.line as i64;
                Ok(Expr::new(
                    ExprKind::Literal(Literal::Int {
                        raw: spanned.token.as_str(),
                        value: line,
                    }),
                    spanned.span,
                ))
            }
            Token::MagicFile => {
                let file_path = self.current_file_path();
                Ok(Expr::new(
                    ExprKind::Literal(Literal::String {
                        raw: spanned.token.as_str(),
                        value: file_path,
                    }),
                    spanned.span,
                ))
            }
            Token::MagicDir => {
                let dir_path = self.current_dir_path();
                Ok(Expr::new(
                    ExprKind::Literal(Literal::String {
                        raw: spanned.token.as_str(),
                        value: dir_path,
                    }),
                    spanned.span,
                ))
            }
            Token::MagicClass => {
                let class_name = self.scope.current_class().unwrap_or("").to_string();
                Ok(Expr::new(
                    ExprKind::Literal(Literal::String {
                        raw: spanned.token.as_str(),
                        value: class_name,
                    }),
                    spanned.span,
                ))
            }
            Token::MagicMethod => {
                let method_name = self.scope.current_method_fqn().unwrap_or_default();
                Ok(Expr::new(
                    ExprKind::Literal(Literal::String {
                        raw: spanned.token.as_str(),
                        value: method_name,
                    }),
                    spanned.span,
                ))
            }
            Token::MagicFunction => {
                let func_name = self.scope.current_function().unwrap_or("").to_string();
                Ok(Expr::new(
                    ExprKind::Literal(Literal::String {
                        raw: spanned.token.as_str(),
                        value: func_name,
                    }),
                    spanned.span,
                ))
            }
            Token::MagicTrait => {
                let trait_name = self.scope.current_trait().unwrap_or("").to_string();
                Ok(Expr::new(
                    ExprKind::Literal(Literal::String {
                        raw: spanned.token.as_str(),
                        value: trait_name,
                    }),
                    spanned.span,
                ))
            }
            Token::MagicNamespace => {
                let ns_name = self.scope.current_namespace().unwrap_or("").to_string();
                Ok(Expr::new(
                    ExprKind::Literal(Literal::String {
                        raw: spanned.token.as_str(),
                        value: ns_name,
                    }),
                    spanned.span,
                ))
            }

            // Variables
            Token::Variable(name) => Ok(Expr::new(
                ExprKind::Variable(Variable::new(name, spanned.span)),
                spanned.span,
            )),

            // Fully qualified identifier with leading backslash (e.g. `\Exception`, `\App\Models\User`, `\strlen`)
            Token::Backslash => {
                let leading_span = spanned.span;
                let first = self.expect_ident()?;
                let mut end = first.span.end;
                let mut span = leading_span.merge(first.span);
                let mut segments = vec![first.name];

                while self.matches(&Token::Backslash) {
                    let next = self.expect_ident()?;
                    end = next.span.end;
                    span = span.merge(next.span);
                    segments.push(next.name);
                }

                let qualified_name = if let Some(src) = self.source {
                    let raw = &src[leading_span.start..end];
                    if raw.contains(char::is_whitespace) {
                        let s = format!("\\{}", segments.join("\\"));
                        Box::leak(s.into_boxed_str())
                    } else {
                        raw
                    }
                } else {
                    let s = format!("\\{}", segments.join("\\"));
                    Box::leak(s.into_boxed_str())
                };

                Ok(Expr::new(
                    ExprKind::Ident(Ident::new(qualified_name, span)),
                    span,
                ))
            }

            Token::Namespace if self.check(&Token::Backslash) => {
                let start = spanned.span.start;
                let mut end = spanned.span.end;
                let mut span = spanned.span;
                let mut segments = vec!["namespace"];

                while self.matches(&Token::Backslash) {
                    let next = self.expect_ident()?;
                    end = next.span.end;
                    span = span.merge(next.span);
                    segments.push(next.name);
                }

                let qualified_name = if let Some(src) = self.source {
                    let raw = &src[start..end];
                    if raw.contains(char::is_whitespace) {
                        let s = segments.join("\\");
                        Box::leak(s.into_boxed_str())
                    } else {
                        raw
                    }
                } else {
                    let s = segments.join("\\");
                    Box::leak(s.into_boxed_str())
                };

                Ok(Expr::new(
                    ExprKind::Ident(Ident::new(qualified_name, span)),
                    span,
                ))
            }

            // Identifiers & Contextual Type Identifiers
            Token::Ident(name) => {
                // If followed by backslash, this is a qualified identifier (e.g. `App\Models\User`)
                if self.check(&Token::Backslash) {
                    let start = spanned.span.start;
                    let mut end = spanned.span.end;
                    let mut span = spanned.span;
                    let mut segments = vec![name];

                    while self.matches(&Token::Backslash) {
                        let next = self.expect_ident()?;
                        end = next.span.end;
                        span = span.merge(next.span);
                        segments.push(next.name);
                    }

                    let qualified_name = if let Some(src) = self.source {
                        let raw = &src[start..end];
                        if raw.contains(char::is_whitespace) {
                            let s = segments.join("\\");
                            Box::leak(s.into_boxed_str())
                        } else {
                            raw
                        }
                    } else {
                        let s = segments.join("\\");
                        Box::leak(s.into_boxed_str())
                    };

                    return Ok(Expr::new(
                        ExprKind::Ident(Ident::new(qualified_name, span)),
                        span,
                    ));
                }

                if name.eq_ignore_ascii_case("__LINE__") {
                    let line = spanned.span.line as i64;
                    return Ok(Expr::new(
                        ExprKind::Literal(Literal::Int {
                            raw: spanned.token.as_str(),
                            value: line,
                        }),
                        spanned.span,
                    ));
                }
                if name.eq_ignore_ascii_case("__FILE__") {
                    let file_path = self.current_file_path();
                    return Ok(Expr::new(
                        ExprKind::Literal(Literal::String {
                            raw: spanned.token.as_str(),
                            value: file_path,
                        }),
                        spanned.span,
                    ));
                }
                if name.eq_ignore_ascii_case("__DIR__") {
                    let dir_path = self.current_dir_path();
                    return Ok(Expr::new(
                        ExprKind::Literal(Literal::String {
                            raw: spanned.token.as_str(),
                            value: dir_path,
                        }),
                        spanned.span,
                    ));
                }
                if name.eq_ignore_ascii_case("__CLASS__") {
                    let class_name = self.scope.current_class().unwrap_or("").to_string();
                    return Ok(Expr::new(
                        ExprKind::Literal(Literal::String {
                            raw: spanned.token.as_str(),
                            value: class_name,
                        }),
                        spanned.span,
                    ));
                }
                if name.eq_ignore_ascii_case("__METHOD__") {
                    let method_name = self.scope.current_method_fqn().unwrap_or_default();
                    return Ok(Expr::new(
                        ExprKind::Literal(Literal::String {
                            raw: spanned.token.as_str(),
                            value: method_name,
                        }),
                        spanned.span,
                    ));
                }
                if name.eq_ignore_ascii_case("__FUNCTION__") {
                    let func_name = self.scope.current_function().unwrap_or("").to_string();
                    return Ok(Expr::new(
                        ExprKind::Literal(Literal::String {
                            raw: spanned.token.as_str(),
                            value: func_name,
                        }),
                        spanned.span,
                    ));
                }
                if name.eq_ignore_ascii_case("__TRAIT__") {
                    let trait_name = self.scope.current_trait().unwrap_or("").to_string();
                    return Ok(Expr::new(
                        ExprKind::Literal(Literal::String {
                            raw: spanned.token.as_str(),
                            value: trait_name,
                        }),
                        spanned.span,
                    ));
                }
                if name.eq_ignore_ascii_case("__NAMESPACE__") {
                    let ns_name = self.scope.current_namespace().unwrap_or("").to_string();
                    return Ok(Expr::new(
                        ExprKind::Literal(Literal::String {
                            raw: spanned.token.as_str(),
                            value: ns_name,
                        }),
                        spanned.span,
                    ));
                }

                Ok(Expr::new(
                    ExprKind::Ident(Ident::new(name, spanned.span)),
                    spanned.span,
                ))
            }
            Token::TypeList | Token::TypeDict | Token::TypeSet | Token::TypeHashMap | Token::Echo => Ok(Expr::new(
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
                return Err(ParseError::new(
                    ParseErrorKind::Custom {
                        message: "The 'await' keyword is not supported in RHP. Use synchronous facades (File, DB, HTTP) to get results synchronously, or call '->wait()' on Future objects.".to_string(),
                    },
                    spanned.span,
                ));
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

            // Parentheses: grouping or reject PHP-style type casting
            Token::OpenParen => {
                // Reject PHP-style type cast `(int) $x`, `(string) $x`, etc.
                if let Some(next) = self.peek() {
                    let is_cast_type = match next.token {
                        Token::TypeInt
                        | Token::TypeFloat
                        | Token::TypeString
                        | Token::TypeBytes
                        | Token::TypeBool
                        | Token::TypeArray
                        | Token::TypeObject
                        | Token::Ident("integer" | "double" | "real" | "boolean" | "binary") => true,
                        _ => false,
                    };

                    if is_cast_type && self.peek_nth(1).map(|t| &t.token) == Some(&Token::CloseParen) {
                        let type_name = next.token.as_str();
                        let target_type = match type_name {
                            "integer" => "int",
                            "boolean" => "bool",
                            "double" | "real" => "float",
                            "binary" => "string",
                            other => other,
                        };
                        return Err(ParseError::new(
                            ParseErrorKind::Custom {
                                message: format!(
                                    "Type cast syntax '({})' is not supported in RHP. Use functional conversion '{}(...)' instead (e.g. '{}($x)').",
                                    type_name, target_type, target_type
                                ),
                            },
                            spanned.span.merge(self.peek_nth(1).unwrap().span),
                        ));
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

                if elements.len() == 1 && elements[0].key.is_none() && !elements[0].is_unpack {
                    let elem_val = &elements[0].value;
                    if let ExprKind::Binary { op: BinaryOp::Range, left, right } = &elem_val.kind {
                        let (start_expr, stop_expr, step_expr) = if let ExprKind::Binary {
                            op: BinaryOp::Range,
                            left: inner_left,
                            right: inner_right,
                        } = &left.kind {
                            (inner_left.as_ref().clone(), inner_right.as_ref().clone(), Some(right.as_ref().clone()))
                        } else {
                            (left.as_ref().clone(), right.as_ref().clone(), None)
                        };

                        let arr_target = Expr::new(
                            ExprKind::Ident(Ident::new("Arr", span)),
                            span,
                        );
                        let mut args = vec![
                            Argument::new(start_expr.clone(), start_expr.span),
                            Argument::new(stop_expr.clone(), stop_expr.span),
                        ];
                        if let Some(step) = step_expr {
                            args.push(Argument::new(step.clone(), step.span));
                        }

                        return Ok(Expr::new(
                            ExprKind::StaticCall {
                                target: Box::new(arr_target),
                                method: Ident::new("range", span),
                                args,
                            },
                            span,
                        ));
                    }
                }

                Ok(Expr::new(ExprKind::Array(elements), span))
            }

            // Static closure or arrow function `static fn(...) => ...` / `static function(...) { ... }`
            Token::Static => {
                if self.matches(&Token::Fn) {
                    let (params, return_type, body) = self.with_function_scope("{closure}", |stream| {
                        let params = stream.parse_formal_parameters()?;
                        let return_type = if stream.matches(&Token::Colon) {
                            Some(stream.parse_type_hint()?)
                        } else {
                            None
                        };
                        stream.expect(Token::DoubleArrow)?;
                        let body = stream.parse_expr(binding_power::LOWEST)?;
                        Ok((params, return_type, body))
                    })?;
                    let span = spanned.span.merge(body.span);
                    Ok(Expr::new(
                        ExprKind::ArrowFunction {
                            params,
                            return_type,
                            body: Box::new(body),
                        },
                        span,
                    ))
                } else if self.matches(&Token::Function) {
                    let (params, uses, return_type, body, end_span) = self.with_function_scope("{closure}", |stream| {
                        let params = stream.parse_formal_parameters()?;
                        let mut uses = Vec::new();
                        if stream.matches(&Token::Use) {
                            stream.expect(Token::OpenParen)?;
                            uses = stream.separated_by(
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
                            stream.expect(Token::CloseParen)?;
                        }

                        let return_type = if stream.matches(&Token::Colon) {
                            Some(stream.parse_type_hint()?)
                        } else {
                            None
                        };

                        let body = stream.parse_block_statements()?;
                        let end_span = stream.current_span();
                        Ok((params, uses, return_type, body, end_span))
                    })?;
                    let span = spanned.span.merge(end_span);

                    Ok(Expr::new(
                        ExprKind::Closure {
                            is_static: true,
                            params,
                            uses,
                            return_type,
                            body,
                        },
                        span,
                    ))
                } else {
                    let ident = Ident::new("static", spanned.span);
                    Ok(Expr::new(ExprKind::Ident(ident), spanned.span))
                }
            }

            // Arrow function `fn($n) => $n * 2`
            Token::Fn => {
                let (params, return_type, body) = self.with_function_scope("{closure}", |stream| {
                    let params = stream.parse_formal_parameters()?;
                    let return_type = if stream.matches(&Token::Colon) {
                        Some(stream.parse_type_hint()?)
                    } else {
                        None
                    };
                    stream.expect(Token::DoubleArrow)?;
                    let body = stream.parse_expr(binding_power::LOWEST)?;
                    Ok((params, return_type, body))
                })?;
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
                let (params, uses, return_type, body, end_span) = self.with_function_scope("{closure}", |stream| {
                    let params = stream.parse_formal_parameters()?;
                    let mut uses = Vec::new();
                    if stream.matches(&Token::Use) {
                        stream.expect(Token::OpenParen)?;
                        uses = stream.separated_by(
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
                        stream.expect(Token::CloseParen)?;
                    }

                    let return_type = if stream.matches(&Token::Colon) {
                        Some(stream.parse_type_hint()?)
                    } else {
                        None
                    };

                    let body = stream.parse_block_statements()?;
                    let end_span = stream.current_span();
                    Ok((params, uses, return_type, body, end_span))
                })?;
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

            // New expression `new User($name)` or `new $className($name)` or `new class ...`
            Token::New => {
                let start_span = spanned.span;

                let is_anon_class = match self.peek().map(|t| &t.token) {
                    Some(Token::Class) => true,
                    Some(Token::Readonly) => {
                        matches!(self.peek_nth(1).map(|t| &t.token), Some(Token::Class))
                    }
                    _ => false,
                };

                if is_anon_class {
                    let mut is_readonly = false;
                    if self.matches(&Token::Readonly) {
                        is_readonly = true;
                    }
                    let class_tok = self.expect(Token::Class)?;

                    let mut args = Vec::new();
                    if self.matches(&Token::OpenParen) {
                        args = self.parse_call_arguments()?;
                        self.expect(Token::CloseParen)?;
                    }

                    let extends = if self.matches(&Token::Extends) {
                        let parent = self.expect_qualified_ident()?;
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

                    let anon_name = format!("class@anonymous${}_{}", start_span.file_id, start_span.start);
                    let anon_str: &'s str = Box::leak(anon_name.into_boxed_str());
                    let class_ident = Ident::new(anon_str, class_tok.span);

                    self.expect(Token::OpenBrace)?;
                    let members = self.with_class_scope(anon_str.to_string(), |stream| {
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

                    let class_decl = ClassDecl {
                        span,
                        attributes: Vec::new(),
                        modifiers: ClassModifiers {
                            is_readonly,
                            ..Default::default()
                        },
                        name: class_ident,
                        extends,
                        implements,
                        members,
                    };

                    return Ok(Expr::new(
                        ExprKind::AnonymousClass {
                            class_decl: Box::new(class_decl),
                            args,
                        },
                        span,
                    ));
                }

                let (class_expr, end_target_span) = if matches!(
                    self.peek().map(|t| &t.token),
                    Some(Token::Variable(_))
                ) {
                    let var = self.expect_variable()?;
                    let v_span = var.span;
                    (Expr::new(ExprKind::Variable(var), v_span), v_span)
                } else {
                    let class_ident = self.expect_qualified_ident()?;

                    // Static facade instantiation is strictly forbidden by specification
                    let check_name = class_ident.name.strip_prefix('\\').unwrap_or(class_ident.name);
                    if is_static_facade(check_name) || is_static_only(check_name) {
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
                let kind = match spanned.token {
                    Token::TypeInt => CastType::Int,
                    Token::TypeString => CastType::String,
                    Token::TypeFloat => CastType::Float,
                    Token::TypeBool => CastType::Bool,
                    _ => unreachable!(),
                };
                self.expect(Token::OpenParen)?;
                let expr = self.parse_expr_lowest()?;
                let close_tok = self.expect(Token::CloseParen)?;
                let span = spanned.span.merge(close_tok.span);
                Ok(Expr::new(
                    ExprKind::Cast {
                        kind,
                        expr: Box::new(expr),
                    },
                    span,
                ))
            }

            Token::Isset => {
                let _open_paren = self.expect(Token::OpenParen)?;
                let mut isset_exprs = Vec::new();
                while !self.check(&Token::CloseParen) && !self.is_eof() {
                    let arg = self.parse_expr_lowest()?;
                    let lowered = self.lower_isset_operand(&arg)?;
                    let span = arg.span;
                    let isset_single = Expr::new(
                        ExprKind::Isset {
                            operand: Box::new(arg),
                            lowered: Box::new(lowered),
                        },
                        span,
                    );
                    isset_exprs.push(isset_single);
                    if !self.matches(&Token::Comma) {
                        break;
                    }
                }
                let close_paren = self.expect(Token::CloseParen)?;

                if isset_exprs.is_empty() {
                    return Err(ParseError::new(
                        ParseErrorKind::UnexpectedToken {
                            expected: "expression".to_string(),
                            found: ")".to_string(),
                        },
                        close_paren.span,
                    ));
                }

                let mut result = isset_exprs.remove(0);
                for next_isset in isset_exprs {
                    let span = result.span.merge(next_isset.span);
                    result = Expr::new(
                        ExprKind::Binary {
                            left: Box::new(result),
                            op: BinaryOp::LogicalAnd,
                            right: Box::new(next_isset),
                        },
                        span,
                    );
                }
                Ok(result)
            }

            Token::Empty => {
                let open_paren = self.expect(Token::OpenParen)?;
                let arg = self.parse_expr_lowest()?;
                let close_paren = self.expect(Token::CloseParen)?;
                let full_span = spanned.span.merge(close_paren.span);
                let lowered = self.lower_empty_operand(&arg);
                let lowered_span = lowered.span;
                Ok(Expr::static_call(
                    "Sys",
                    "empty",
                    vec![Argument::new(lowered, lowered_span)],
                    open_paren.span,
                    full_span,
                ))
            }

            Token::Exit => {
                let mut args = Vec::new();
                let mut end_span = spanned.span;
                if self.matches(&Token::OpenParen) {
                    if !self.check(&Token::CloseParen) {
                        let arg_expr = self.parse_expr_lowest()?;
                        let span = arg_expr.span;
                        args.push(Argument::new(arg_expr, span));
                    }
                    let close_paren = self.expect(Token::CloseParen)?;
                    end_span = close_paren.span;
                }
                let full_span = spanned.span.merge(end_span);
                Ok(Expr::static_call(
                    "Sys",
                    "exit",
                    args,
                    spanned.span,
                    full_span,
                ))
            }

            Token::Die => {
                let mut args = Vec::new();
                let mut end_span = spanned.span;
                if self.matches(&Token::OpenParen) {
                    if !self.check(&Token::CloseParen) {
                        let arg_expr = self.parse_expr_lowest()?;
                        let span = arg_expr.span;
                        args.push(Argument::new(arg_expr, span));
                    }
                    let close_paren = self.expect(Token::CloseParen)?;
                    end_span = close_paren.span;
                }
                let full_span = spanned.span.merge(end_span);
                Ok(Expr::static_call(
                    "Sys",
                    "die",
                    args,
                    spanned.span,
                    full_span,
                ))
            }

            token if token.is_type() => {
                let type_name = token.as_str();
                Ok(Expr::new(
                    ExprKind::Literal(Literal::String {
                        raw: type_name,
                        value: type_name.to_string(),
                    }),
                    spanned.span,
                ))
            }

            Token::Instanceof => {
                self.expect(Token::OpenParen)?;
                let obj = self.parse_expr_lowest()?;
                self.expect(Token::Comma)?;
                let class_expr = self.parse_expr_lowest()?;
                let close_tok = self.expect(Token::CloseParen)?;
                let span = spanned.span.merge(close_tok.span);
                Ok(Expr::new(
                    ExprKind::Instanceof {
                        expr: Box::new(obj),
                        class: Box::new(class_expr),
                    },
                    span,
                ))
            }

            _ => {
                let mut err = ParseError::new(
                    ParseErrorKind::UnexpectedToken {
                        expected: "expression".to_string(),
                        found: spanned.token.as_str().to_string(),
                    },
                    spanned.span,
                );
                err.found_token = Some(spanned.token.as_str().to_string());
                err.expected_tokens = Some(vec![
                    "identifier".to_string(),
                    "number".to_string(),
                    "string".to_string(),
                    "`(`".to_string(),
                ]);
                if let Some((delim, delim_span)) = self.innermost_open_delimiter() {
                    let delim_char = match delim {
                        Token::OpenParen => "(",
                        Token::OpenBrace => "{",
                        Token::OpenBracket => "[",
                        _ => "(",
                    };
                    err = err.with_secondary_label(*delim_span, format!("this `{}` is still open", delim_char));
                }
                Err(err)
            }
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
                    let raw_name = callee_ident.name;
                    let callee_name = raw_name.strip_prefix('\\').unwrap_or(raw_name);
                    let callee_span = callee_ident.span;

                    if matches!(
                        callee_name,
                        "require"
                            | "require_once"
                            | "include"
                            | "include_once"
                            | "spl_autoload_register"
                            | "spl_autoload_unregister"
                    ) {
                        return Err(ParseError::new(
                            ParseErrorKind::DynamicIncludeForbidden {
                                construct: callee_name.to_string(),
                            },
                            callee_span,
                        ));
                    }

                    if callee_name.eq_ignore_ascii_case("gettype") || callee_name == "get_debug_type" {
                        return Ok(Expr::static_call("Debug", "type", args, callee_span, span));
                    }

                    if (callee_name == "is_a" || callee_name == "instanceof") && args.len() >= 2 {
                        let obj = args[0].value.clone();
                        let class_expr = args[1].value.clone();
                        return Ok(Expr::new(
                            ExprKind::Instanceof {
                                expr: Box::new(obj),
                                class: Box::new(class_expr),
                            },
                            span,
                        ));
                    }

                    if callee_name.eq_ignore_ascii_case("now") && !self.is_user_defined_function(callee_name) {
                        return Ok(Expr::static_call("Time", "now", args, callee_span, span));
                    }

                    if callee_name.eq_ignore_ascii_case("date") && !self.is_user_defined_function(callee_name) {
                        return Ok(Expr::static_call("Time", "date", args, callee_span, span));
                    }

                    let call_node = if is_reserved_intrinsic(callee_name) {
                        // Rewrites to Sys::* (e.g. Sys::env(), Sys::print(), Sys::echo(), Sys::dd())
                        let method = match callee_name {
                            "println" => "print",
                            "os_env" => "osEnv",
                            "register_shutdown_function" => "registerShutdownFunction",
                            "set_error_handler" => "setErrorHandler",
                            "restore_error_handler" => "restoreErrorHandler",
                            "set_exception_handler" => "setExceptionHandler",
                            "restore_exception_handler" => "restoreExceptionHandler",
                            "trigger_error" => "triggerError",
                            other => other,
                        };
                        Expr::static_call("Sys", method, args, callee_span, span)
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

            // Member access `left->member`, dynamic `left->{expr}`, or nullsafe `left?->...`
            Token::Arrow | Token::NullsafeArrow => {
                let is_nullsafe = matches!(op_tok.token, Token::NullsafeArrow);

                if self.check(&Token::OpenBrace) {
                    self.advance(); // consume '{'
                    let dyn_expr = self.parse_expr_lowest()?;
                    let close_brace = self.expect(Token::CloseBrace)?;

                    // If followed by `(`, this is a dynamic method call `$obj->{$method}(...)`
                    if self.matches(&Token::OpenParen) {
                        let args = self.parse_call_arguments()?;
                        let close_paren = self.expect(Token::CloseParen)?;
                        let span = left.span.merge(close_paren.span);
                        Ok(Expr::new(
                            ExprKind::DynamicMethodCall {
                                object: Box::new(left),
                                method: Box::new(dyn_expr),
                                args,
                                is_nullsafe,
                            },
                            span,
                        ))
                    } else {
                        let span = left.span.merge(close_brace.span);
                        Ok(Expr::new(
                            ExprKind::DynamicPropertyFetch {
                                object: Box::new(left),
                                property: Box::new(dyn_expr),
                                is_nullsafe,
                            },
                            span,
                        ))
                    }
                } else {
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

            // Macro invocation `ident!(...)` (e.g. `println!(...)`, `print!(...)`, `echo!(...)`)
            Token::Bang => {
                if let ExprKind::Ident(ref callee_ident) = left.kind {
                    let callee_name = callee_ident.name;
                    let callee_span = callee_ident.span;
                    self.expect(Token::OpenParen)?;
                    let args = self.parse_call_arguments()?;
                    let close_tok = self.expect(Token::CloseParen)?;
                    let span = left.span.merge(close_tok.span);

                    let method_name = match callee_name {
                        "println" => "print",
                        "print" => "echo",
                        "echo" => "echo",
                        other => other,
                    };
                    Ok(Expr::static_call("Sys", method_name, args, callee_span, span))
                } else {
                    Err(ParseError::new(
                        ParseErrorKind::UnexpectedToken {
                            expected: "identifier preceding '!' macro invocation".to_string(),
                            found: "!".to_string(),
                        },
                        op_tok.span,
                    ))
                }
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
                } else if op_tok.token == Token::Identical {
                    return Err(ParseError::new(
                        ParseErrorKind::UnsupportedOperator {
                            op: "===".to_string(),
                            suggested: "==".to_string(),
                        },
                        op_tok.span,
                    ));
                } else if op_tok.token == Token::NotIdentical {
                    return Err(ParseError::new(
                        ParseErrorKind::UnsupportedOperator {
                            op: "!==".to_string(),
                            suggested: "!=".to_string(),
                        },
                        op_tok.span,
                    ));
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
        let mut args = Vec::new();
        if self.check(&Token::CloseParen) {
            return Ok(args);
        }

        loop {
            let arg_start = self.current_span();

            // Check if this argument is skipped (empty argument indicated by comma)
            if self.check(&Token::Comma) {
                args.push(Argument::skipped(arg_start));
                self.advance(); // consume the comma
                if self.check(&Token::CloseParen) {
                    break;
                }
                continue;
            }

            let is_unpack = self.matches(&Token::Ellipsis);
            let is_reference = self.matches(&Token::Ampersand);

            // Named argument check `name: $val` or `name: &$val` - reject named parameters
            if let Some(tok) = self.peek() {
                if (tok.token.is_keyword() || matches!(tok.token, Token::Ident(_)))
                    && self.peek_nth(1).map(|t| &t.token) == Some(&Token::Colon)
                {
                    let name = self.expect_ident()?;
                    let colon = self.expect(Token::Colon)?;
                    return Err(ParseError::new(
                        ParseErrorKind::Custom {
                            message: format!(
                                "Named parameters are not supported in RHP (found '{}:'). To skip optional parameters, use extra commas (e.g. `fn($req, , $opt)`) at the call site.",
                                name.name
                            ),
                        },
                        arg_start.merge(colon.span),
                    ));
                }
            }

            let value = self.parse_expr_lowest()?;
            let span = arg_start.merge(value.span);
            args.push(Argument {
                span,
                name: None,
                value,
                is_unpack,
                is_reference,
                is_skipped: false,
            });

            if self.matches(&Token::Comma) {
                if self.check(&Token::CloseParen) {
                    let close_tok = self.peek().unwrap().clone();
                    let mut err = ParseError::new(
                        ParseErrorKind::UnexpectedToken {
                            expected: "expression".to_string(),
                            found: close_tok.token.as_str().to_string(),
                        },
                        close_tok.span,
                    );
                    err.found_token = Some(close_tok.token.as_str().to_string());
                    err.expected_tokens = Some(vec![
                        "identifier".to_string(),
                        "number".to_string(),
                        "string".to_string(),
                        "`(`".to_string(),
                    ]);
                    if let Some((Token::OpenParen, open_span)) = self.innermost_open_delimiter() {
                        err = err.with_secondary_label(*open_span, "this `(` is still open");
                    }
                    return Err(err);
                }
            } else {
                break;
            }
        }

        Ok(args)
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
                if s.check(&Token::Colon) {
                    let colon = s.expect(Token::Colon)?;
                    return Err(ParseError::new(
                        ParseErrorKind::Custom {
                            message: format!(
                                "Colon type syntax '{}: Type' is not supported. Use prefix type syntax 'Type {}' instead (e.g., 'int {}').",
                                name.name, name.name, name.name
                            ),
                        },
                        name.span.merge(colon.span),
                    ));
                }

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
        Token::OpenParen | Token::OpenBracket | Token::Arrow | Token::NullsafeArrow | Token::DoubleColon | Token::Bang => {
            Some((binding_power::CALL_MEMBER_INDEX, binding_power::CALL_MEMBER_INDEX + 1))
        }

        // Power operator is right-associative (higher left bp)
        Token::StarStar => Some((binding_power::POWER + 1, binding_power::POWER)),

        // Multiplicative
        Token::Star | Token::Slash | Token::Percent => {
            Some((binding_power::MULTIPLICATIVE, binding_power::MULTIPLICATIVE + 1))
        }

        // Additive
        Token::Plus | Token::Minus => {
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

        // Range operator `..`
        Token::DotDot => Some((binding_power::RANGE, binding_power::RANGE + 1)),

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
        Token::Ampersand => Some(BinaryOp::BitAnd),
        Token::Pipe => Some(BinaryOp::BitOr),
        Token::Caret => Some(BinaryOp::BitXor),
        Token::ShiftLeft => Some(BinaryOp::Shl),
        Token::ShiftRight => Some(BinaryOp::Shr),
        Token::Equal => Some(BinaryOp::Eq),
        Token::NotEqual | Token::NotEqualAlt => Some(BinaryOp::NotEq),
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
        Token::DotDot => Some(BinaryOp::Range),
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
                b'n' => { current_lit.push('\n'); i += 2; }
                b'r' => { current_lit.push('\r'); i += 2; }
                b't' => { current_lit.push('\t'); i += 2; }
                b'\\' => { current_lit.push('\\'); i += 2; }
                b'"' => { current_lit.push('"'); i += 2; }
                b'$' => { current_lit.push('$'); i += 2; }
                b'0' => { current_lit.push('\0'); i += 2; }
                b'u' => {
                    if i + 2 < bytes.len() && bytes[i + 2] == b'{' {
                        let mut k = i + 3;
                        let mut hex = String::new();
                        while k < bytes.len() && bytes[k] != b'}' && (bytes[k] as char).is_ascii_hexdigit() {
                            hex.push(bytes[k] as char);
                            k += 1;
                        }
                        if k < bytes.len() && bytes[k] == b'}' && !hex.is_empty() {
                            if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                                if let Some(ch) = char::from_u32(cp) {
                                    current_lit.push(ch);
                                    i = k + 1;
                                    continue;
                                }
                            }
                        }
                        return Err(ParseError::new(
                            ParseErrorKind::Custom {
                                message: "Invalid Unicode escape sequence in string literal".to_string(),
                            },
                            span,
                        ));
                    } else {
                        current_lit.push('\\');
                        current_lit.push('u');
                        i += 2;
                    }
                }
                b'x' | b'X' => {
                    let mut hex = String::new();
                    let mut k = i + 2;
                    while k < bytes.len() && k < i + 4 && (bytes[k] as char).is_ascii_hexdigit() {
                        hex.push(bytes[k] as char);
                        k += 1;
                    }
                    if let Ok(b) = u8::from_str_radix(&hex, 16) {
                        if b > 0x7F {
                            return Err(ParseError::new(
                                ParseErrorKind::Custom {
                                    message: format!(
                                        "Invalid UTF-8 escape '\\x{}' in string literal. Use a bytes literal b\"...\" instead.",
                                        hex
                                    ),
                                },
                                span,
                            ));
                        }
                        current_lit.push(b as char);
                        i = k;
                        continue;
                    } else {
                        current_lit.push('\\');
                        current_lit.push('x');
                        i += 2;
                    }
                }
                other => {
                    current_lit.push('\\');
                    current_lit.push(other as char);
                    i += 2;
                }
            }
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
