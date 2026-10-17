pub mod error;
pub mod facade;
pub mod span;
pub mod token;

use std::fmt;
use logos::Logos;

pub use error::{LexError, SpannedLexError};
pub use facade::{
    get_custom_helper, get_facades_by_tier, get_legacy_php_function, get_macro, get_static_facade,
    has_custom_helper, has_macro, is_legacy_php_function, is_native_or_plugin_facade,
    is_reserved_intrinsic, is_static_facade, is_static_only, php_function_signature_map,
    register_custom_helper, register_macro, register_tier2_facade, register_tier3_facade,
    validate_inheritance, validate_instantiation, FacadeInfo, FacadeRegistry, FacadeTier,
    LegacyPhpFunctionMapping, MacroEntry, MacroRegistry, LEGACY_PHP_FUNCTIONS, NATIVE_FACADES,
    RESERVED_INTRINSICS,
};
pub use span::{SourceFile, SourceMap, Span};
pub use token::Token;

/// A token bundled with its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken<'s> {
    pub token: Token<'s>,
    pub span: Span,
}

impl<'s> SpannedToken<'s> {
    #[inline]
    pub fn new(token: Token<'s>, span: Span) -> Self {
        Self { token, span }
    }

    /// Access the raw slice of this token in the source code.
    #[inline]
    pub fn slice<'a>(&self, source: &'a str) -> &'a str {
        &source[self.span.start..self.span.end]
    }
}

impl<'s> fmt::Display for SpannedToken<'s> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} at {}", self.token, self.span)
    }
}

/// High-performance, zero-copy DFA lexical analyzer engine.
///
/// Wraps `logos::Lexer` and provides streaming tokenization with exact source span tracking.
pub struct Lexer<'s> {
    source: &'s str,
    inner: logos::Lexer<'s, Token<'s>>,
    file_id: usize,
    curr_offset: usize,
    curr_line: usize,
    curr_col: usize,
}

impl<'s> Lexer<'s> {
    /// Create a new Lexer for the given source string with default file_id 0.
    pub fn new(source: &'s str) -> Self {
        Self::with_file_id(source, 0)
    }

    /// Create a new Lexer with a specific file_id.
    pub fn with_file_id(source: &'s str, file_id: usize) -> Self {
        Self {
            source,
            inner: Token::lexer(source),
            file_id,
            curr_offset: 0,
            curr_line: 1,
            curr_col: 1,
        }
    }

    /// Advance the line and column counters from `curr_offset` up to `target_offset`.
    fn advance_position_to(&mut self, target_offset: usize) {
        if target_offset <= self.curr_offset {
            return;
        }

        let bytes = self.source.as_bytes();
        let mut i = self.curr_offset;
        while i < target_offset {
            let b = bytes[i];
            if b == b'\r' {
                self.curr_line += 1;
                self.curr_col = 1;
                if i + 1 < target_offset && bytes[i + 1] == b'\n' {
                    i += 2;
                    continue;
                }
            } else if b == b'\n' {
                self.curr_line += 1;
                self.curr_col = 1;
            } else if (b & 0xC0) != 0x80 {
                // Advance by 1 for UTF-8 start bytes and ASCII
                self.curr_col += 1;
            }
            i += 1;
        }
        self.curr_offset = target_offset;
    }

    /// Convenience method to tokenize the entire source string into a vector of spanned tokens.
    pub fn tokenize(source: &'s str) -> Result<Vec<SpannedToken<'s>>, SpannedLexError> {
        Self::new(source).collect()
    }

    /// Convenience method to tokenize with an explicit file_id.
    pub fn tokenize_with_file_id(
        source: &'s str,
        file_id: usize,
    ) -> Result<Vec<SpannedToken<'s>>, SpannedLexError> {
        Self::with_file_id(source, file_id).collect()
    }
}

impl<'s> Iterator for Lexer<'s> {
    type Item = Result<SpannedToken<'s>, SpannedLexError>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.next()?;
        let range = self.inner.span();

        // Advance line/col through skipped whitespace/comments up to the token start
        self.advance_position_to(range.start);
        let token_line = self.curr_line;
        let token_col = self.curr_col;

        // Advance line/col through the token content itself
        self.advance_position_to(range.end);

        let span = Span::new(self.file_id, range.start, range.end, token_line, token_col);

        match item {
            Ok(token) => Some(Ok(SpannedToken::new(token, span))),
            Err(err) => Some(Err(SpannedLexError::new(err, span))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_and_method_chaining() {
        let code = r#"
            $numbers = Arr::list([1, 2, 3, 4, 5])
                ->map(fn($n) => $n * 2)
                ->filter(fn($n) => $n > 4);
        "#;
        let tokens = Lexer::tokenize(code).expect("Tokenization failed");
        let token_types: Vec<_> = tokens.iter().map(|t| &t.token).collect();

        assert_eq!(token_types[0], &Token::Variable("$numbers"));
        assert_eq!(token_types[1], &Token::Assign);
        assert_eq!(token_types[2], &Token::Ident("Arr"));
        assert_eq!(token_types[3], &Token::DoubleColon);
        assert_eq!(token_types[4], &Token::TypeList);
        assert_eq!(token_types[5], &Token::OpenParen);
        assert_eq!(token_types[6], &Token::OpenBracket);
        assert_eq!(token_types[7], &Token::IntLiteral("1"));
        assert_eq!(token_types[8], &Token::Comma);
        assert_eq!(token_types[9], &Token::IntLiteral("2"));
    }

    #[test]
    fn test_str_facade() {
        let code = r#"
            $slug = Str::slug('Laravel Style Facades');
            $token = Str::random(32);
            $hasWord = Str::contains('Runtime engine', 'engine');
        "#;
        let tokens = Lexer::tokenize(code).expect("Tokenization failed");
        assert_eq!(tokens[0].token, Token::Variable("$slug"));
        assert_eq!(tokens[1].token, Token::Assign);
        assert_eq!(tokens[2].token, Token::Ident("Str"));
        assert_eq!(tokens[3].token, Token::DoubleColon);
        assert_eq!(tokens[4].token, Token::Ident("slug"));
        assert_eq!(
            tokens[6].token,
            Token::SingleQuotedString("'Laravel Style Facades'")
        );
        assert_eq!(
            tokens[6].token.raw_string_content(),
            Some("Laravel Style Facades")
        );
    }

    #[test]
    fn test_concurrent_future_and_closure() {
        let code = r#"
            [$user, $posts] = Future::wait([
                fn() => DB::table('users')->find($id),
                fn() => HTTP::get("https://api.example.com/posts?user={$id}")->json()
            ]);
        "#;
        let tokens = Lexer::tokenize(code).expect("Tokenization failed");
        assert_eq!(tokens[0].token, Token::OpenBracket);
        assert_eq!(tokens[1].token, Token::Variable("$user"));
        assert_eq!(tokens[2].token, Token::Comma);
        assert_eq!(tokens[3].token, Token::Variable("$posts"));
        assert_eq!(tokens[4].token, Token::CloseBracket);
        assert_eq!(tokens[5].token, Token::Assign);
        assert_eq!(tokens[6].token, Token::Ident("Future"));
        assert_eq!(tokens[7].token, Token::DoubleColon);
        assert_eq!(tokens[8].token, Token::Ident("wait"));
    }

    #[test]
    fn test_rust_types_and_fluent_api() {
        let code = r#"
            $id = Int::i64(9223372036854775807);
            $price = Float::f64(99.99)->toDecimal(2);
            $rnd = Int::i64()->random()->add()->subtract();
            function sum(int $a, float $b): float { return $a + $b; }
        "#;
        let tokens = Lexer::tokenize(code).expect("Tokenization failed");
        // Supporting classes Int and Float are identifiers
        assert_eq!(tokens[2].token, Token::Ident("Int"));
        assert_eq!(tokens[3].token, Token::DoubleColon);
        assert_eq!(tokens[4].token, Token::Ident("i64"));
        assert_eq!(tokens[5].token, Token::OpenParen);
        assert_eq!(tokens[6].token, Token::IntLiteral("9223372036854775807"));
        assert_eq!(tokens[7].token, Token::CloseParen);

        assert_eq!(tokens[11].token, Token::Ident("Float"));
        assert_eq!(tokens[12].token, Token::DoubleColon);
        assert_eq!(tokens[13].token, Token::Ident("f64"));

        // Base datatypes int and float are primitive TypeInt and TypeFloat
        let func_toks: Vec<_> = tokens
            .iter()
            .filter_map(|t| match t.token {
                Token::TypeInt | Token::TypeFloat => Some(t.token.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            func_toks,
            vec![Token::TypeInt, Token::TypeFloat, Token::TypeFloat]
        );
    }

    #[test]
    fn test_static_facade_instantiation_forbidden() {
        // As defined in PDF Page 2 & 3: new Int(), new Arr(), new Str(), new Float() are forbidden
        assert_eq!(
            validate_instantiation("Int"),
            Err("Fatal Error: Cannot instantiate static facade class 'Int'. Use static methods directly.".to_string())
        );
        assert_eq!(
            validate_instantiation("Float"),
            Err("Fatal Error: Cannot instantiate static facade class 'Float'. Use static methods directly.".to_string())
        );
        assert_eq!(
            validate_instantiation("Arr"),
            Err("Fatal Error: Cannot instantiate static facade class 'Arr'. Use static methods directly.".to_string())
        );
        assert_eq!(
            validate_instantiation("Str"),
            Err("Fatal Error: Cannot instantiate static facade class 'Str'. Use static methods directly.".to_string())
        );
        // Normal user classes are allowed to be instantiated
        assert!(validate_instantiation("User").is_ok());
        assert!(validate_instantiation("Order").is_ok());
    }

    #[test]
    fn test_doc_comment_and_attributes() {
        let code = r#"
            /**
             * Controller description
             */
            #[Route("/test")]
            class MyController {
            }
        "#;
        let tokens = Lexer::tokenize(code).expect("Tokenization failed");
        assert!(matches!(tokens[0].token, Token::DocComment(_)));
        assert_eq!(tokens[1].token, Token::AttributeStart);
        assert_eq!(tokens[2].token, Token::Ident("Route"));
        assert_eq!(tokens[3].token, Token::OpenParen);
        assert_eq!(tokens[4].token, Token::DoubleQuotedString("\"/test\""));
        assert_eq!(tokens[5].token, Token::CloseParen);
        assert_eq!(tokens[6].token, Token::CloseBracket);
        assert_eq!(tokens[7].token, Token::Class);
        assert_eq!(tokens[8].token, Token::Ident("MyController"));
    }

    #[test]
    fn test_span_tracking_accuracy() {
        let code = "line1\n  $var = 42;";
        let tokens = Lexer::tokenize(code).expect("Tokenization failed");

        let var_tok = &tokens[1];
        assert_eq!(var_tok.token, Token::Variable("$var"));
        assert_eq!(var_tok.span.line, 2);
        assert_eq!(var_tok.span.col, 3);
        assert_eq!(var_tok.span.start, 8);
        assert_eq!(var_tok.span.end, 12);
    }
}
