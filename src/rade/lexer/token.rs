/// Represents tokens produced by the Rade lexer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Plain HTML or text chunks.
    Text(String),

    /// Escaped echo content inside `{{ ... }}`.
    EchoEscaped(String),

    /// Raw unescaped echo content inside `{!! ... !!}`.
    EchoRaw(String),

    /// Literal Blade echo `@{{ ... }}` which should output `{{ ... }}` verbatim.
    EscapedVerbatim(String),

    /// Comment content inside `{{-- ... --}}` (to be ignored/stripped during compilation).
    Comment(String),

    /// Template directive such as `@if(...)`, `@for(...)`, `@else`, etc.
    Directive {
        name: String,
        args: Option<String>,
    },

    /// Raw RHP code block inside `@rhp ... @endrhp`.
    RawRhp(String),
}

impl Token {
    /// Returns true if this token is plain text.
    pub fn is_text(&self) -> bool {
        matches!(self, Token::Text(_))
    }

    /// Returns true if this token is an escaped echo.
    pub fn is_echo_escaped(&self) -> bool {
        matches!(self, Token::EchoEscaped(_))
    }

    /// Returns true if this token is a raw echo.
    pub fn is_echo_raw(&self) -> bool {
        matches!(self, Token::EchoRaw(_))
    }

    /// Returns true if this token is an escaped verbatim echo.
    pub fn is_escaped_verbatim(&self) -> bool {
        matches!(self, Token::EscapedVerbatim(_))
    }

    /// Returns true if this token is a comment.
    pub fn is_comment(&self) -> bool {
        matches!(self, Token::Comment(_))
    }

    /// Returns true if this token is a directive.
    pub fn is_directive(&self) -> bool {
        matches!(self, Token::Directive { .. })
    }

    /// Returns true if this token is a raw RHP block.
    pub fn is_raw_rhp(&self) -> bool {
        matches!(self, Token::RawRhp(_))
    }

    /// If this token is a Directive, returns its name and arguments.
    pub fn as_directive(&self) -> Option<(&str, Option<&str>)> {
        match self {
            Token::Directive { name, args } => Some((name.as_str(), args.as_deref())),
            _ => None,
        }
    }

    /// Returns the inner content of the token, if applicable.
    pub fn inner_content(&self) -> Option<&str> {
        match self {
            Token::Text(s) => Some(s.as_str()),
            Token::EchoEscaped(s) => Some(s.as_str()),
            Token::EchoRaw(s) => Some(s.as_str()),
            Token::Comment(s) => Some(s.as_str()),
            Token::RawRhp(s) => Some(s.as_str()),
            Token::EscapedVerbatim(s) => {
                let stripped = s.strip_prefix("{{").and_then(|rest| rest.strip_suffix("}}"));
                stripped.map(|inner| inner.trim())
            }
            Token::Directive { args, .. } => args.as_deref(),
        }
    }

    /// Wraps this token with 1-indexed line and column positions.
    pub fn spanned(self, line: usize, column: usize) -> SpannedToken {
        SpannedToken::new(self, line, column)
    }
}

/// A token with its starting 1-indexed line and column positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpannedToken {
    pub token: Token,
    pub line: usize,
    pub column: usize,
}

impl SpannedToken {
    /// Creates a new `SpannedToken`.
    pub fn new(token: Token, line: usize, column: usize) -> Self {
        Self { token, line, column }
    }
}

impl std::ops::Deref for SpannedToken {
    type Target = Token;

    fn deref(&self) -> &Self::Target {
        &self.token
    }
}
