use std::collections::HashSet;

use crate::ast::{
    expr::Argument,
    stmt::Attribute,
    types::{Ident, TypeHint, TypeHintKind, Variable},
};
use crate::lexer::{span::Span, token::Token, SpannedToken};
use crate::parser::error::{ParseError, ParseErrorKind};

/// Lookahead and combinatorial parser helpers operating over a stream of spanned tokens.
pub struct CombinatorStream<'s> {
    pub(crate) tokens: Vec<SpannedToken<'s>>,
    pub(crate) cursor: usize,
    pub(crate) file_id: usize,
    pub(crate) user_functions: HashSet<String>,
}

impl<'s> CombinatorStream<'s> {
    pub fn new(tokens: Vec<SpannedToken<'s>>, file_id: usize) -> Self {
        let mut user_functions = HashSet::new();
        // Pre-scan token stream to collect declared user function names (`function name(`)
        // so that calls before or after declaration are recognized as user-defined functions.
        let mut i = 0;
        while i + 2 < tokens.len() {
            if tokens[i].token == Token::Function {
                // Only standalone function declarations (not method declarations inside a class or anonymous closures)
                if let Token::Ident(name) = tokens[i + 1].token {
                    if tokens[i + 2].token == Token::OpenParen {
                        // Ensure it's not preceded by visibility/static/abstract/final modifiers of a class method
                        let is_method = if i > 0 {
                            matches!(
                                tokens[i - 1].token,
                                Token::Public
                                    | Token::Protected
                                    | Token::Private
                                    | Token::Static
                                    | Token::Abstract
                                    | Token::Final
                            ) || (i > 1
                                && tokens[i - 1].token == Token::Async
                                && matches!(
                                    tokens[i - 2].token,
                                    Token::Public
                                        | Token::Protected
                                        | Token::Private
                                        | Token::Static
                                        | Token::Abstract
                                        | Token::Final
                                ))
                        } else {
                            false
                        };
                        if !is_method {
                            user_functions.insert(name.to_string());
                        }
                    }
                }
            }
            i += 1;
        }

        Self {
            tokens,
            cursor: 0,
            file_id,
            user_functions,
        }
    }

    /// Register a declared user-defined function name in the parser symbol table.
    #[inline]
    pub fn register_user_function(&mut self, name: impl Into<String>) {
        self.user_functions.insert(name.into());
    }

    /// Check if the given identifier is a declared user-defined function.
    #[inline]
    pub fn is_user_defined_function(&self, name: &str) -> bool {
        self.user_functions.contains(name)
    }

    /// Peek at the current token without consuming it.
    #[inline]
    pub fn peek(&self) -> Option<&SpannedToken<'s>> {
        self.tokens.get(self.cursor)
    }

    /// Peek at the nth token ahead without consuming it (0-indexed).
    #[inline]
    pub fn peek_nth(&self, n: usize) -> Option<&SpannedToken<'s>> {
        self.tokens.get(self.cursor + n)
    }

    /// Current source span (or the end of previous token / default span if at EOF).
    pub fn current_span(&self) -> Span {
        if let Some(t) = self.peek() {
            t.span
        } else if let Some(last) = self.tokens.last() {
            Span::new(self.file_id, last.span.end, last.span.end, last.span.line, last.span.col)
        } else {
            Span::new(self.file_id, 0, 0, 1, 1)
        }
    }

    /// Check if the stream has reached the end of tokens.
    #[inline]
    pub fn is_eof(&self) -> bool {
        self.cursor >= self.tokens.len()
    }

    /// Consume and return the current token.
    #[inline]
    pub fn advance(&mut self) -> Option<SpannedToken<'s>> {
        if self.cursor < self.tokens.len() {
            let tok = self.tokens[self.cursor].clone();
            self.cursor += 1;
            Some(tok)
        } else {
            None
        }
    }

    /// Check if the current token matches a discriminant or equality.
    #[inline]
    pub fn check(&self, expected: &Token<'s>) -> bool {
        match self.peek() {
            Some(t) => &t.token == expected,
            None => false,
        }
    }

    /// Check if the current token satisfies a predicate.
    #[inline]
    pub fn check_fn<F: Fn(&Token<'s>) -> bool>(&self, f: F) -> bool {
        match self.peek() {
            Some(t) => f(&t.token),
            None => false,
        }
    }

    /// Advance if the current token matches `expected`, returning `true`.
    pub fn matches(&mut self, expected: &Token<'s>) -> bool {
        if self.check(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Expect a specific token or return an `UnexpectedToken` / `UnexpectedEof` error.
    pub fn expect(&mut self, expected: Token<'s>) -> Result<SpannedToken<'s>, ParseError> {
        let span = self.current_span();
        match self.advance() {
            Some(spanned) => {
                if spanned.token == expected {
                    Ok(spanned)
                } else {
                    Err(ParseError::new(
                        ParseErrorKind::UnexpectedToken {
                            expected: expected.as_str().to_string(),
                            found: spanned.token.as_str().to_string(),
                        },
                        spanned.span,
                    ))
                }
            }
            None => Err(ParseError::new(
                ParseErrorKind::UnexpectedEof {
                    expected: expected.as_str().to_string(),
                },
                span,
            )),
        }
    }

    /// Expect an identifier token, allowing contextual keywords as identifiers where appropriate.
    pub fn expect_ident(&mut self) -> Result<Ident<'s>, ParseError> {
        let span = self.current_span();
        match self.advance() {
            Some(spanned) => {
                if let Token::Ident(name) = spanned.token {
                    Ok(Ident::new(name, spanned.span))
                } else if spanned.token.is_keyword() || spanned.token.is_type() {
                    // Allow keywords in identifier positions (e.g., method names like ->match(), ->class)
                    Ok(Ident::new(spanned.token.as_str(), spanned.span))
                } else {
                    Err(ParseError::new(
                        ParseErrorKind::UnexpectedToken {
                            expected: "identifier".to_string(),
                            found: spanned.token.as_str().to_string(),
                        },
                        spanned.span,
                    ))
                }
            }
            None => Err(ParseError::new(
                ParseErrorKind::UnexpectedEof {
                    expected: "identifier".to_string(),
                },
                span,
            )),
        }
    }

    /// Expect a variable token (`$var`).
    pub fn expect_variable(&mut self) -> Result<Variable<'s>, ParseError> {
        let span = self.current_span();
        match self.advance() {
            Some(spanned) => {
                if let Token::Variable(name) = spanned.token {
                    Ok(Variable::new(name, spanned.span))
                } else {
                    Err(ParseError::new(
                        ParseErrorKind::UnexpectedToken {
                            expected: "variable ($name)".to_string(),
                            found: spanned.token.as_str().to_string(),
                        },
                        spanned.span,
                    ))
                }
            }
            None => Err(ParseError::new(
                ParseErrorKind::UnexpectedEof {
                    expected: "variable ($name)".to_string(),
                },
                span,
            )),
        }
    }

    /// Combinator: Parse an item enclosed by `open` and `close` delimiter tokens.
    pub fn delimited<T, F>(&mut self, open: Token<'s>, mut inner: F, close: Token<'s>) -> Result<T, ParseError>
    where
        F: FnMut(&mut Self) -> Result<T, ParseError>,
    {
        self.expect(open)?;
        let result = inner(self)?;
        self.expect(close)?;
        Ok(result)
    }

    /// Combinator: Parse 0 or more items separated by `separator`, with optional trailing separator.
    pub fn separated_by<T, F>(
        &mut self,
        mut item_parser: F,
        separator: Token<'s>,
        terminator: &Token<'s>,
    ) -> Result<Vec<T>, ParseError>
    where
        F: FnMut(&mut Self) -> Result<T, ParseError>,
    {
        let mut items = Vec::new();
        if self.check(terminator) {
            return Ok(items);
        }

        loop {
            if self.check(terminator) {
                break;
            }
            let item = item_parser(self)?;
            items.push(item);

            if self.matches(&separator) {
                if self.check(terminator) {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(items)
    }

    /// Skip opening and closing PHP tags (`<?php`, `?>`, etc.) if present.
    pub fn skip_php_tags(&mut self) {
        while let Some(tok) = self.peek() {
            match tok.token {
                Token::PhpOpen | Token::PhpShortOpen | Token::PhpEchoOpen | Token::PhpClose => {
                    self.advance();
                }
                _ => break,
            }
        }
    }

    /// Skip standalone doc comments between statements or expressions.
    pub fn skip_doc_comments(&mut self) {
        while let Some(tok) = self.peek() {
            if matches!(tok.token, Token::DocComment(_)) {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Combinator: Parse language attributes (`#[Route('/api')]`).
    pub fn parse_attributes(&mut self) -> Result<Vec<Attribute<'s>>, ParseError> {
        let mut attributes = Vec::new();

        while self.matches(&Token::AttributeStart) {
            let start_span = self.current_span();
            let name = self.expect_ident()?;
            let mut args = Vec::new();

            if self.matches(&Token::OpenParen) {
                args = self.separated_by(
                    |s| {
                        // Argument can be named or positional
                        let arg_start = s.current_span();
                        if let Some(tok) = s.peek() {
                            if (tok.token.is_keyword() || matches!(tok.token, Token::Ident(_)))
                                && s.peek_nth(1).map(|t| &t.token) == Some(&Token::Colon)
                            {
                                let arg_name = s.expect_ident()?;
                                s.expect(Token::Colon)?;
                                let val = s.parse_expr_lowest()?;
                                let span = arg_start.merge(val.span);
                                return Ok(Argument::named(arg_name, val, span));
                            }
                        }
                        let val = s.parse_expr_lowest()?;
                        let span = arg_start.merge(val.span);
                        Ok(Argument::new(val, span))
                    },
                    Token::Comma,
                    &Token::CloseParen,
                )?;
                self.expect(Token::CloseParen)?;
            }

            self.expect(Token::CloseBracket)?;
            let end_span = self.current_span();
            attributes.push(Attribute {
                span: start_span.merge(end_span),
                name,
                args,
            });
        }

        Ok(attributes)
    }

    /// Combinator: Parse a strongly-typed type hint (including nullable `?`, union `|`, intersection `&`).
    pub fn parse_type_hint(&mut self) -> Result<TypeHint<'s>, ParseError> {
        let start_span = self.current_span();

        // Nullable prefix `?Type`
        if self.matches(&Token::Question) {
            let inner = self.parse_single_type_hint()?;
            let span = start_span.merge(inner.span);
            return Ok(TypeHint::new(TypeHintKind::Nullable(Box::new(inner)), span));
        }

        let first = self.parse_single_type_hint()?;

        // Union types `Type1 | Type2`
        if self.check(&Token::Pipe) {
            let mut types = vec![first];
            while self.matches(&Token::Pipe) {
                types.push(self.parse_single_type_hint()?);
            }
            let end_span = types.last().unwrap().span;
            let span = start_span.merge(end_span);
            return Ok(TypeHint::new(TypeHintKind::Union(types), span));
        }

        // Intersection types `Type1 & Type2`
        if self.check(&Token::Ampersand) {
            let mut types = vec![first];
            while self.matches(&Token::Ampersand) {
                types.push(self.parse_single_type_hint()?);
            }
            let end_span = types.last().unwrap().span;
            let span = start_span.merge(end_span);
            return Ok(TypeHint::new(TypeHintKind::Intersection(types), span));
        }

        Ok(first)
    }

    /// Parse a single base type hint token.
    fn parse_single_type_hint(&mut self) -> Result<TypeHint<'s>, ParseError> {
        let span = self.current_span();
        match self.advance() {
            Some(spanned) => match spanned.token {
                Token::TypeInt
                | Token::TypeFloat
                | Token::TypeString
                | Token::TypeBool
                | Token::TypeVoid
                | Token::TypeMixed
                | Token::TypeNever => {
                    let ident = Ident::new(spanned.token.as_str(), spanned.span);
                    Ok(TypeHint::new(TypeHintKind::Named(ident), spanned.span))
                }
                Token::Ident(name) => {
                    if matches!(name, "String" | "Boolean" | "Integer" | "boolean" | "integer") {
                        return Err(ParseError::new(
                            ParseErrorKind::Custom {
                                message: format!(
                                    "Invalid primitive type '{}': primitive types must be strictly lowercase (string, bool, int, float).",
                                    name
                                ),
                            },
                            spanned.span,
                        ));
                    }
                    let ident = Ident::new(name, spanned.span);
                    Ok(TypeHint::new(TypeHintKind::Named(ident), spanned.span))
                }
                Token::TypeCallable => Ok(TypeHint::new(TypeHintKind::Callable, spanned.span)),
                Token::TypeArray => Ok(TypeHint::new(TypeHintKind::Array, spanned.span)),
                Token::TypeIterable => Ok(TypeHint::new(TypeHintKind::Iterable, spanned.span)),
                _ => Err(ParseError::new(
                    ParseErrorKind::UnexpectedToken {
                        expected: "type hint".to_string(),
                        found: spanned.token.as_str().to_string(),
                    },
                    spanned.span,
                )),
            },
            None => Err(ParseError::new(
                ParseErrorKind::UnexpectedEof {
                    expected: "type hint".to_string(),
                },
                span,
            )),
        }
    }
}
