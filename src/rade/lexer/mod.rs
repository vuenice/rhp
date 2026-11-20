pub mod scanner;
pub mod token;

pub use scanner::{LexerError, Scanner};
pub use token::{SpannedToken, Token};

/// Tokenizes a template string into a vector of tokens.
pub fn tokenize(input: &str) -> Result<Vec<Token>, LexerError> {
    Scanner::scan(input)
}

/// Tokenizes a template string into a vector of spanned tokens.
pub fn tokenize_spanned(input: &str) -> Result<Vec<SpannedToken>, LexerError> {
    Scanner::scan_spanned(input)
}
