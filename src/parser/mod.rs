pub mod combinator;
pub mod error;
pub mod pratt;
pub mod stmt;

pub use combinator::CombinatorStream;
pub use error::{ParseError, ParseErrorKind};

use crate::ast::{expr::Expr, stmt::Stmt, Program};
use crate::lexer::{Lexer, SpannedToken};

/// High-performance custom Pratt and Combinator parser for the RHP language.
pub struct Parser<'s> {
    stream: CombinatorStream<'s>,
}

impl<'s> Parser<'s> {
    /// Create a new Parser wrapping pre-tokenized spanned tokens.
    pub fn new(tokens: Vec<SpannedToken<'s>>, file_id: usize) -> Self {
        Self {
            stream: CombinatorStream::new(tokens, file_id),
        }
    }

    /// Construct a parser directly from raw source code with default file_id 0.
    pub fn from_source(source: &'s str) -> Result<Self, ParseError> {
        Self::from_source_with_file_id(source, 0)
    }

    /// Construct a parser directly from raw source code with an explicit file_id.
    pub fn from_source_with_file_id(source: &'s str, file_id: usize) -> Result<Self, ParseError> {
        let tokens = Lexer::tokenize_with_file_id(source, file_id)
            .map_err(|e| ParseError::new(ParseErrorKind::Lex(e.error), e.span))?;
        Ok(Self::new(tokens, file_id))
    }

    /// Parse the entire token stream into a strongly-typed Abstract Syntax Tree `Program`.
    pub fn parse_program(&mut self) -> Result<Program<'s>, ParseError> {
        self.stream.skip_php_tags();
        let start_span = self.stream.current_span();
        let mut statements = Vec::new();

        while !self.stream.is_eof() {
            self.stream.skip_php_tags();
            self.stream.skip_doc_comments();
            if self.stream.is_eof() {
                break;
            }
            let stmt = self.stream.parse_stmt()?;
            statements.push(stmt);
        }

        let end_span = statements
            .last()
            .map(|s| s.span)
            .unwrap_or(start_span);
        let span = start_span.merge(end_span);

        Ok(Program::new(statements, span))
    }

    /// Parse a single expression from the token stream.
    pub fn parse_expr(&mut self) -> Result<Expr<'s>, ParseError> {
        self.stream.parse_expr_lowest()
    }

    /// Parse a single statement from the token stream.
    pub fn parse_stmt(&mut self) -> Result<Stmt<'s>, ParseError> {
        self.stream.parse_stmt()
    }
}

/// Convenience function: Parse source code string directly into a typed `Program` AST.
pub fn parse<'s>(source: &'s str) -> Result<Program<'s>, ParseError> {
    let mut parser = Parser::from_source(source)?;
    parser.parse_program()
}

/// Convenience function: Parse source code string directly into a typed `Program` AST with file_id.
pub fn parse_with_file_id<'s>(source: &'s str, file_id: usize) -> Result<Program<'s>, ParseError> {
    let mut parser = Parser::from_source_with_file_id(source, file_id)?;
    parser.parse_program()
}

/// Convenience function: Parse source string directly as a single `Expr` AST node.
pub fn parse_expr<'s>(source: &'s str) -> Result<Expr<'s>, ParseError> {
    let mut parser = Parser::from_source(source)?;
    parser.parse_expr()
}

/// Convenience function: Parse source string directly as a single `Stmt` AST node.
pub fn parse_stmt<'s>(source: &'s str) -> Result<Stmt<'s>, ParseError> {
    let mut parser = Parser::from_source(source)?;
    parser.parse_stmt()
}
