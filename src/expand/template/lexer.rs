use crate::lexer::span::Span;

/// Token emitted by the template scanner.
#[derive(Debug, Clone, PartialEq)]
pub enum TemplateToken {
    /// Plain literal HTML or text.
    Text {
        content: String,
        span: Span,
    },
    /// Directive tag `@name` or `@name(...)`.
    Directive {
        name: String,
        raw_args: Option<String>,
        span: Span,
    },
    /// Echo expression tag: `{{ expr }}` (escaped) or `{!! expr !!}` (raw).
    Echo {
        raw_expr: String,
        is_raw: bool,
        span: Span,
    },
}

impl TemplateToken {
    pub fn span(&self) -> Span {
        match self {
            Self::Text { span, .. } => *span,
            Self::Directive { span, .. } => *span,
            Self::Echo { span, .. } => *span,
        }
    }
}

/// Tokenizer for template source buffers (.rhp.html, .blade.rhp).
pub struct TemplateLexer<'a> {
    source: &'a str,
    file_id: usize,
    chars: Vec<(usize, char)>,
    cursor: usize,
    line: usize,
    col: usize,
}

impl<'a> TemplateLexer<'a> {
    pub fn new(source: &'a str, file_id: usize) -> Self {
        let chars: Vec<(usize, char)> = source.char_indices().collect();
        Self {
            source,
            file_id,
            chars,
            cursor: 0,
            line: 1,
            col: 1,
        }
    }

    fn current_byte(&self) -> usize {
        if self.cursor < self.chars.len() {
            self.chars[self.cursor].0
        } else {
            self.source.len()
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.chars.get(self.cursor).map(|(_, c)| *c)
    }

    fn peek_char_nth(&self, n: usize) -> Option<char> {
        self.chars.get(self.cursor + n).map(|(_, c)| *c)
    }

    fn advance(&mut self) -> Option<char> {
        if self.cursor >= self.chars.len() {
            return None;
        }
        let (_, c) = self.chars[self.cursor];
        self.cursor += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    /// Tokenize the entire template into a stream of tokens.
    pub fn tokenize(&mut self) -> Result<Vec<TemplateToken>, String> {
        let mut tokens = Vec::new();
        let mut text_buf = String::new();
        let mut text_start_byte = self.current_byte();
        let mut text_start_line = self.line;
        let mut text_start_col = self.col;

        while let Some(c) = self.peek_char() {
            // Check for escaped '@': '@@' emits literal '@'
            if c == '@' && self.peek_char_nth(1) == Some('@') {
                self.advance(); // consume first '@'
                self.advance(); // consume second '@'
                text_buf.push('@');
                continue;
            }

            // Check for Raw Echo: `{!! ... !!}`
            if c == '{' && self.peek_char_nth(1) == Some('!') && self.peek_char_nth(2) == Some('!') {
                if !text_buf.is_empty() {
                    let end_byte = self.current_byte();
                    tokens.push(TemplateToken::Text {
                        content: std::mem::take(&mut text_buf),
                        span: Span::new(
                            self.file_id,
                            text_start_byte,
                            end_byte,
                            text_start_line,
                            text_start_col,
                        ),
                    });
                }
                tokens.push(self.lex_raw_echo()?);
                text_start_byte = self.current_byte();
                text_start_line = self.line;
                text_start_col = self.col;
                continue;
            }

            // Check for Escaped Echo: `{{ ... }}`
            if c == '{' && self.peek_char_nth(1) == Some('{') {
                if !text_buf.is_empty() {
                    let end_byte = self.current_byte();
                    tokens.push(TemplateToken::Text {
                        content: std::mem::take(&mut text_buf),
                        span: Span::new(
                            self.file_id,
                            text_start_byte,
                            end_byte,
                            text_start_line,
                            text_start_col,
                        ),
                    });
                }
                tokens.push(self.lex_escaped_echo()?);
                text_start_byte = self.current_byte();
                text_start_line = self.line;
                text_start_col = self.col;
                continue;
            }

            // Check for Directive: `@directive`
            if c == '@' && self.is_directive_start() {
                if !text_buf.is_empty() {
                    let end_byte = self.current_byte();
                    tokens.push(TemplateToken::Text {
                        content: std::mem::take(&mut text_buf),
                        span: Span::new(
                            self.file_id,
                            text_start_byte,
                            end_byte,
                            text_start_line,
                            text_start_col,
                        ),
                    });
                }
                tokens.push(self.lex_directive()?);
                text_start_byte = self.current_byte();
                text_start_line = self.line;
                text_start_col = self.col;
                continue;
            }

            // Ordinary character (HTML, text, or '#[' which is literal in templates)
            text_buf.push(c);
            self.advance();
        }

        if !text_buf.is_empty() {
            let end_byte = self.current_byte();
            tokens.push(TemplateToken::Text {
                content: text_buf,
                span: Span::new(
                    self.file_id,
                    text_start_byte,
                    end_byte,
                    text_start_line,
                    text_start_col,
                ),
            });
        }

        Ok(tokens)
    }

    fn is_directive_start(&self) -> bool {
        if let Some(next) = self.peek_char_nth(1) {
            next.is_ascii_alphabetic() || next == '_'
        } else {
            false
        }
    }

    fn lex_raw_echo(&mut self) -> Result<TemplateToken, String> {
        let start_byte = self.current_byte();
        let start_line = self.line;
        let start_col = self.col;

        self.advance(); // '{'
        self.advance(); // '!'
        self.advance(); // '!'

        let mut raw = String::new();
        let mut closed = false;

        while let Some(c) = self.peek_char() {
            if c == '!' && self.peek_char_nth(1) == Some('!') && self.peek_char_nth(2) == Some('}') {
                self.advance();
                self.advance();
                self.advance();
                closed = true;
                break;
            }
            raw.push(c);
            self.advance();
        }

        if !closed {
            return Err("Unclosed raw echo tag `{!!`".to_string());
        }

        let end_byte = self.current_byte();
        Ok(TemplateToken::Echo {
            raw_expr: raw.trim().to_string(),
            is_raw: true,
            span: Span::new(self.file_id, start_byte, end_byte, start_line, start_col),
        })
    }

    fn lex_escaped_echo(&mut self) -> Result<TemplateToken, String> {
        let start_byte = self.current_byte();
        let start_line = self.line;
        let start_col = self.col;

        self.advance(); // '{'
        self.advance(); // '{'

        let mut raw = String::new();
        let mut closed = false;

        while let Some(c) = self.peek_char() {
            if c == '}' && self.peek_char_nth(1) == Some('}') {
                self.advance();
                self.advance();
                closed = true;
                break;
            }
            raw.push(c);
            self.advance();
        }

        if !closed {
            return Err("Unclosed echo tag `{{`".to_string());
        }

        let end_byte = self.current_byte();
        Ok(TemplateToken::Echo {
            raw_expr: raw.trim().to_string(),
            is_raw: false,
            span: Span::new(self.file_id, start_byte, end_byte, start_line, start_col),
        })
    }

    fn lex_directive(&mut self) -> Result<TemplateToken, String> {
        let start_byte = self.current_byte();
        let start_line = self.line;
        let start_col = self.col;

        self.advance(); // consume '@'

        let mut name = String::new();
        while let Some(c) = self.peek_char() {
            if c.is_ascii_alphanumeric() || c == '_' {
                name.push(c);
                self.advance();
            } else {
                break;
            }
        }

        // Check if directive has arguments in parentheses: `@directive(...)`
        // Note: do not skip arbitrary whitespace if newline intervenes
        let raw_args = if self.peek_char() == Some('(') {
            self.advance(); // '('
            let mut paren_depth = 1;
            let mut arg_buf = String::new();
            let mut in_single_quote = false;
            let mut in_double_quote = false;
            let mut escaped = false;

            while let Some(c) = self.peek_char() {
                if escaped {
                    arg_buf.push(c);
                    self.advance();
                    escaped = false;
                    continue;
                }
                if c == '\\' {
                    escaped = true;
                    arg_buf.push(c);
                    self.advance();
                    continue;
                }
                if c == '\'' && !in_double_quote {
                    in_single_quote = !in_single_quote;
                } else if c == '"' && !in_single_quote {
                    in_double_quote = !in_double_quote;
                } else if !in_single_quote && !in_double_quote {
                    if c == '(' {
                        paren_depth += 1;
                    } else if c == ')' {
                        paren_depth -= 1;
                        if paren_depth == 0 {
                            self.advance(); // consume final ')'
                            break;
                        }
                    }
                }
                arg_buf.push(c);
                self.advance();
            }

            if paren_depth != 0 {
                return Err(format!("Unterminated argument list for directive `@{}`", name));
            }

            Some(arg_buf)
        } else {
            None
        };

        let end_byte = self.current_byte();
        Ok(TemplateToken::Directive {
            name,
            raw_args,
            span: Span::new(self.file_id, start_byte, end_byte, start_line, start_col),
        })
    }
}
