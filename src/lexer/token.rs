use logos::{Lexer, Logos, Skip};
use std::fmt;

use super::error::LexError;

fn skip_block_comment<'s>(lex: &mut Lexer<'s, Token<'s>>) -> Skip {
    let remainder = lex.remainder();
    if let Some(end) = remainder.find("*/") {
        lex.bump(end + 2);
    } else {
        lex.bump(remainder.len());
    }
    Skip
}

fn lex_doc_comment<'s>(lex: &mut Lexer<'s, Token<'s>>) -> Result<&'s str, LexError> {
    let remainder = lex.remainder();
    if let Some(end) = remainder.find("*/") {
        let total_len = end + 2;
        let start = lex.span().start;
        let full_len = lex.span().len() + total_len;
        lex.bump(total_len);
        Ok(&lex.source()[start..start + full_len])
    } else {
        lex.bump(remainder.len());
        Err(LexError::UnterminatedComment)
    }
}

fn err_unterminated_single_quote<'s>(_lex: &mut Lexer<'s, Token<'s>>) -> Result<(), LexError> {
    Err(LexError::UnterminatedString)
}

fn err_unterminated_double_quote<'s>(_lex: &mut Lexer<'s, Token<'s>>) -> Result<(), LexError> {
    Err(LexError::UnterminatedString)
}

fn err_unterminated_backtick<'s>(_lex: &mut Lexer<'s, Token<'s>>) -> Result<(), LexError> {
    Err(LexError::UnterminatedString)
}

fn err_invalid_numeric<'s>(lex: &mut Lexer<'s, Token<'s>>) -> Result<(), LexError> {
    let slice = lex.slice();
    if slice.ends_with('_') || slice.ends_with('.') {
        Err(LexError::InvalidNumericLiteral(
            "Numeric literal cannot end with a separator or decimal point.".to_string(),
        ))
    } else {
        Err(LexError::InvalidNumericLiteral(
            "SyntaxError: invalid syntax.".to_string(),
        ))
    }
}

fn err_invalid_variable<'s>(_lex: &mut Lexer<'s, Token<'s>>) -> Result<(), LexError> {
    Err(LexError::InvalidVariableIdentifier(
        "Variable names must start with a letter or underscore, followed by alphanumeric characters (e.g., $varName)."
            .to_string(),
    ))
}

/// Strongly-typed deterministic finite automaton (DFA) token enumeration.
///
/// Designed with zero-copy string slicing referencing the source buffer lifetime `'s`.
#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(error = LexError)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip(r"//[^\r\n]*", allow_greedy = true))]
pub enum Token<'s> {
    // -------------------------------------------------------------------------
    // Comments
    // -------------------------------------------------------------------------
    #[token("/**", lex_doc_comment)]
    DocComment(&'s str),

    #[token("/*", skip_block_comment)]
    BlockComment,

    // -------------------------------------------------------------------------
    // PHP Tags
    // -------------------------------------------------------------------------
    #[token("<?php")]
    PhpOpen,

    #[token("<?=")]
    PhpEchoOpen,

    #[token("<?")]
    PhpShortOpen,

    #[token("?>")]
    PhpClose,

    // -------------------------------------------------------------------------
    // Variables & Identifiers
    // -------------------------------------------------------------------------
    #[regex(r"\$[a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*", |lex| lex.slice())]
    Variable(&'s str),

    #[regex(r"\$\$[a-zA-Z0-9_\x7f-\xff\$]*|\$[0-9][a-zA-Z0-9_\x7f-\xff]*|\$", err_invalid_variable)]
    InvalidVariable,

    // -------------------------------------------------------------------------
    // Specific Language Keywords
    // -------------------------------------------------------------------------
    // Functions & Closures
    #[token("fn")]
    Fn,

    #[token("function")]
    Function,

    // Control Flow
    #[token("if")]
    If,

    #[token("else")]
    Else,

    #[token("elseif")]
    Elseif,

    #[token("while")]
    While,

    #[token("do")]
    Do,

    #[token("for")]
    For,

    #[token("foreach")]
    Foreach,

    #[token("as")]
    As,

    #[token("break")]
    Break,

    #[token("continue")]
    Continue,

    #[token("switch")]
    Switch,

    #[token("case")]
    Case,

    #[token("default")]
    Default,

    #[token("match")]
    Match,

    #[token("return")]
    Return,

    #[token("echo")]
    Echo,

    #[token("yield")]
    Yield,

    #[token("from")]
    From,

    #[token("goto")]
    Goto,

    // OOP, Classes & Visibility
    #[token("class")]
    Class,

    #[token("interface")]
    Interface,

    #[token("trait")]
    Trait,

    #[token("enum")]
    Enum,

    #[token("extends")]
    Extends,

    #[token("implements")]
    Implements,

    #[token("public")]
    Public,

    #[token("protected")]
    Protected,

    #[token("private")]
    Private,

    #[token("static")]
    Static,

    #[token("abstract")]
    Abstract,

    #[token("final")]
    Final,

    #[token("readonly")]
    Readonly,

    #[token("singleton")]
    Singleton,

    #[token("const")]
    Const,

    #[token("new")]
    New,

    #[token("clone")]
    Clone,

    #[token("instanceof")]
    Instanceof,

    // Namespaces & Imports
    #[token("namespace")]
    Namespace,

    #[token("use")]
    Use,

    #[token("declare")]
    Declare,

    #[token("global")]
    Global,

    // Exception Handling
    #[token("try")]
    Try,

    #[token("catch")]
    Catch,

    #[token("finally")]
    Finally,

    #[token("throw")]
    Throw,

    // Asynchronous & Architecture Specific Keywords
    #[token("async")]
    Async,

    #[token("await")]
    Await,

    #[token("defer")]
    Defer,

    // Logical Word Operators
    #[token("and")]
    And,

    #[token("or")]
    Or,

    #[token("xor")]
    Xor,

    // Literals (Keywords - Strictly Lowercase)
    #[token("true")]
    True,

    #[token("false")]
    False,

    #[token("null")]
    Null,

    // -------------------------------------------------------------------------
    // Type Keywords (Strictly Lowercase Primitives: string, bool, int, float)
    // Note: 'Int::' and 'Float::' are supporting classes, not base datatypes;
    // base types 'int', 'float', 'string', 'bool' are primitives.
    // -------------------------------------------------------------------------
    #[token("string")]
    TypeString,

    #[token("int")]
    TypeInt,

    #[token("float")]
    TypeFloat,

    #[token("bool")]
    TypeBool,

    #[token("void")]
    TypeVoid,

    #[token("mixed")]
    TypeMixed,

    #[token("callable")]
    TypeCallable,

    #[token("array")]
    TypeArray,

    #[token("object")]
    TypeObject,

    #[token("iterable")]
    TypeIterable,

    #[token("never")]
    TypeNever,

    #[token("list")]
    TypeList,

    #[token("dict")]
    TypeDict,

    #[token("set")]
    TypeSet,

    #[token("hashmap")]
    TypeHashMap,

    // -------------------------------------------------------------------------
    // Identifiers (Fallback after keywords)
    // -------------------------------------------------------------------------
    #[regex(r"[a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*", |lex| lex.slice())]
    Ident(&'s str),

    // -------------------------------------------------------------------------
    // Numeric Literals (Zero-Copy Slicing & Strict Lexical Validation)
    // -------------------------------------------------------------------------
    // Valid Floats (e.g., 3.14, 1.0e10, 1.5e-3, 0.5)
    #[regex(r"[0-9]+(_[0-9]+)*\.[0-9]+(_[0-9]+)*([eE][+-]?[0-9]+(_[0-9]+)*)?|[0-9]+(_[0-9]+)*[eE][+-]?[0-9]+(_[0-9]+)*", |lex| lex.slice())]
    FloatLiteral(&'s str),

    // Hexadecimal integers (e.g. 0x1A_3F)
    #[regex(r"0[xX][0-9a-fA-F]+(_[0-9a-fA-F]+)*", |lex| lex.slice())]
    // Binary integers (e.g. 0b1010_0101)
    #[regex(r"0[bB][01]+(_[01]+)*", |lex| lex.slice())]
    // Octal integers (e.g. 0o755)
    #[regex(r"0[oO][0-7]+(_[0-7]+)*", |lex| lex.slice())]
    // Standard decimal integers (e.g. 100, 1_000_000)
    #[regex(r"[0-9]+(_[0-9]+)*", |lex| lex.slice())]
    IntLiteral(&'s str),

    // Malformed numeric literals: trailing underscore/dot, leading dot (.5), incomplete prefix (0x, 0b, 0o), incomplete exponent (1e), or consecutive underscores
    #[regex(r"[0-9][0-9_]*_|\.[0-9][0-9_]*([eE][+-]?[0-9_]*)?|[0-9][0-9_]*\.[0-9_]*_|[0-9][0-9_]*(\.[0-9][0-9_]*)?[eE][+-]?[0-9_]*_|0[xX][0-9a-fA-F_]*_|0[bB][01_]*_|0[oO][0-7_]*_|[0-9][0-9_]*\.|0[xXbBoO]|[0-9][0-9_]*(\.[0-9][0-9_]*)?[eE][+-]?|[0-9][0-9_]*__[0-9_]*", err_invalid_numeric)]
    InvalidNumeric,

    // -------------------------------------------------------------------------
    // String Literals (Zero-Copy Slicing)
    // -------------------------------------------------------------------------
    #[regex(r#"(?s:'([^'\\]|\\.)*')"#, |lex| lex.slice())]
    SingleQuotedString(&'s str),

    #[token("'", err_unterminated_single_quote)]
    UnterminatedSingleQuote,

    #[regex(r#"(?s:"([^"\\]|\\.)*")"#, |lex| lex.slice())]
    DoubleQuotedString(&'s str),

    #[token("\"", err_unterminated_double_quote)]
    UnterminatedDoubleQuote,

    #[regex(r#"(?s:`([^`\\]|\\.)*`)"#, |lex| lex.slice())]
    BacktickString(&'s str),

    #[token("`", err_unterminated_backtick)]
    UnterminatedBacktick,

    // -------------------------------------------------------------------------
    // Multi-Character Operators & Punctuation
    // -------------------------------------------------------------------------
    #[token("::")]
    DoubleColon,

    #[token("?->")]
    NullsafeArrow,

    #[token("->")]
    Arrow,

    #[token("=>")]
    DoubleArrow,

    #[token("...")]
    Ellipsis,

    #[token("..")]
    DotDot,

    #[token("===")]
    Identical,

    #[token("!==")]
    NotIdentical,

    #[token("==")]
    Equal,

    #[token("!=")]
    NotEqual,

    #[token("<>")]
    NotEqualAlt,

    #[token("<=>")]
    Spaceship,

    #[token("<=")]
    LessThanOrEqual,

    #[token(">=")]
    GreaterThanOrEqual,

    #[token("<<=")]
    ShiftLeftAssign,

    #[token(">>=")]
    ShiftRightAssign,

    #[token("<<")]
    ShiftLeft,

    #[token(">>")]
    ShiftRight,

    #[token("&&")]
    AmpersandAmpersand,

    #[token("||")]
    PipePipe,

    #[token("??=")]
    NullCoalesceAssign,

    #[token("??")]
    NullCoalesce,

    #[token("?:")]
    Elvis,

    #[token("**=")]
    StarStarAssign,

    #[token("**")]
    StarStar,

    #[token("+=")]
    PlusAssign,

    #[token("-=")]
    MinusAssign,

    #[token("*=")]
    StarAssign,

    #[token("/=")]
    SlashAssign,

    #[token("%=")]
    PercentAssign,

    #[token(".=")]
    DotAssign,

    #[token("&=")]
    AmpersandAssign,

    #[token("|=")]
    PipeAssign,

    #[token("^=")]
    CaretAssign,

    #[token("++")]
    PlusPlus,

    #[token("--")]
    MinusMinus,

    #[token("#[")]
    AttributeStart,

    // -------------------------------------------------------------------------
    // Single-Character Operators & Delimiters
    // -------------------------------------------------------------------------
    #[token("+")]
    Plus,

    #[token("-")]
    Minus,

    #[token("*")]
    Star,

    #[token("/")]
    Slash,

    #[token("%")]
    Percent,

    #[token("=")]
    Assign,

    #[token("<")]
    LessThan,

    #[token(">")]
    GreaterThan,

    #[token("&")]
    Ampersand,

    #[token("|")]
    Pipe,

    #[token("^")]
    Caret,

    #[token("~")]
    Tilde,

    #[token("!")]
    Bang,

    #[token("?")]
    Question,

    #[token(":")]
    Colon,

    #[token(";")]
    SemiColon,

    #[token(",")]
    Comma,

    #[token(".")]
    Dot,

    #[token("\\")]
    Backslash,

    #[token("@")]
    At,

    #[token("(")]
    OpenParen,

    #[token(")")]
    CloseParen,

    #[token("[")]
    OpenBracket,

    #[token("]")]
    CloseBracket,

    #[token("{")]
    OpenBrace,

    #[token("}")]
    CloseBrace,
}

impl<'s> Token<'s> {
    /// Returns the variable name without the leading `$` symbol.
    #[inline]
    pub fn variable_name(&self) -> Option<&'s str> {
        match self {
            Token::Variable(name) => Some(&name[1..]),
            _ => None,
        }
    }

    /// Returns the literal slice or a static string representation of this token.
    pub fn as_str(&self) -> &'s str {
        match self {
            Token::DocComment(s) => s,
            Token::Variable(s) => s,
            Token::Ident(s) => s,
            Token::IntLiteral(s) => s,
            Token::FloatLiteral(s) => s,
            Token::SingleQuotedString(s) => s,
            Token::DoubleQuotedString(s) => s,
            Token::BacktickString(s) => s,
            Token::Fn => "fn",
            Token::Function => "function",
            Token::If => "if",
            Token::Else => "else",
            Token::Elseif => "elseif",
            Token::While => "while",
            Token::Do => "do",
            Token::For => "for",
            Token::Foreach => "foreach",
            Token::As => "as",
            Token::Break => "break",
            Token::Continue => "continue",
            Token::Switch => "switch",
            Token::Case => "case",
            Token::Default => "default",
            Token::Match => "match",
            Token::Return => "return",
            Token::Echo => "echo",
            Token::Yield => "yield",
            Token::From => "from",
            Token::Goto => "goto",
            Token::Class => "class",
            Token::Interface => "interface",
            Token::Trait => "trait",
            Token::Enum => "enum",
            Token::Extends => "extends",
            Token::Implements => "implements",
            Token::Public => "public",
            Token::Protected => "protected",
            Token::Private => "private",
            Token::Static => "static",
            Token::Abstract => "abstract",
            Token::Final => "final",
            Token::Readonly => "readonly",
            Token::Singleton => "singleton",
            Token::Const => "const",
            Token::New => "new",
            Token::Clone => "clone",
            Token::Instanceof => "instanceof",
            Token::Namespace => "namespace",
            Token::Use => "use",
            Token::Declare => "declare",
            Token::Global => "global",
            Token::Try => "try",
            Token::Catch => "catch",
            Token::Finally => "finally",
            Token::Throw => "throw",
            Token::Async => "async",
            Token::Await => "await",
            Token::Defer => "defer",
            Token::And => "and",
            Token::Or => "or",
            Token::Xor => "xor",
            Token::True => "true",
            Token::False => "false",
            Token::Null => "null",
            Token::TypeString => "string",
            Token::TypeInt => "int",
            Token::TypeFloat => "float",
            Token::TypeBool => "bool",
            Token::TypeVoid => "void",
            Token::TypeMixed => "mixed",
            Token::TypeCallable => "callable",
            Token::TypeArray => "array",
            Token::TypeObject => "object",
            Token::TypeIterable => "iterable",
            Token::TypeNever => "never",
            Token::TypeList => "list",
            Token::TypeDict => "dict",
            Token::TypeSet => "set",
            Token::TypeHashMap => "hashmap",
            Token::DoubleColon => "::",
            Token::NullsafeArrow => "?->",
            Token::Arrow => "->",
            Token::DoubleArrow => "=>",
            Token::Ellipsis => "...",
            Token::DotDot => "..",
            Token::Identical => "===",
            Token::NotIdentical => "!==",
            Token::Equal => "==",
            Token::NotEqual => "!=",
            Token::NotEqualAlt => "<>",
            Token::Spaceship => "<=>",
            Token::LessThanOrEqual => "<=",
            Token::GreaterThanOrEqual => ">=",
            Token::ShiftLeftAssign => "<<=",
            Token::ShiftRightAssign => ">>=",
            Token::ShiftLeft => "<<",
            Token::ShiftRight => ">>",
            Token::AmpersandAmpersand => "&&",
            Token::PipePipe => "||",
            Token::NullCoalesceAssign => "??=",
            Token::NullCoalesce => "??",
            Token::Elvis => "?:",
            Token::StarStarAssign => "**=",
            Token::StarStar => "**",
            Token::PlusAssign => "+=",
            Token::MinusAssign => "-=",
            Token::StarAssign => "*=",
            Token::SlashAssign => "/=",
            Token::PercentAssign => "%=",
            Token::DotAssign => ".=",
            Token::AmpersandAssign => "&=",
            Token::PipeAssign => "|=",
            Token::CaretAssign => "^=",
            Token::PlusPlus => "++",
            Token::MinusMinus => "--",
            Token::AttributeStart => "#[",
            Token::Plus => "+",
            Token::Minus => "-",
            Token::Star => "*",
            Token::Slash => "/",
            Token::Percent => "%",
            Token::Assign => "=",
            Token::LessThan => "<",
            Token::GreaterThan => ">",
            Token::Ampersand => "&",
            Token::Pipe => "|",
            Token::Caret => "^",
            Token::Tilde => "~",
            Token::Bang => "!",
            Token::Question => "?",
            Token::Colon => ":",
            Token::SemiColon => ";",
            Token::Comma => ",",
            Token::Dot => ".",
            Token::Backslash => "\\",
            Token::At => "@",
            Token::OpenParen => "(",
            Token::CloseParen => ")",
            Token::OpenBracket => "[",
            Token::CloseBracket => "]",
            Token::OpenBrace => "{",
            Token::CloseBrace => "}",
            Token::PhpOpen => "<?php",
            Token::PhpEchoOpen => "<?=",
            Token::PhpShortOpen => "<?",
            Token::PhpClose => "?>",
            Token::BlockComment => "/* ... */",
            Token::UnterminatedSingleQuote => "'",
            Token::UnterminatedDoubleQuote => "\"",
            Token::UnterminatedBacktick => "`",
            Token::InvalidVariable => "$",
            Token::InvalidNumeric => "",
        }
    }

    /// Check if this token represents a keyword.
    pub fn is_keyword(&self) -> bool {
        matches!(
            self,
            Token::Fn
                | Token::Function
                | Token::If
                | Token::Else
                | Token::Elseif
                | Token::While
                | Token::Do
                | Token::For
                | Token::Foreach
                | Token::As
                | Token::Break
                | Token::Continue
                | Token::Switch
                | Token::Case
                | Token::Default
                | Token::Match
                | Token::Return
                | Token::Echo
                | Token::Yield
                | Token::From
                | Token::Goto
                | Token::Class
                | Token::Interface
                | Token::Trait
                | Token::Enum
                | Token::Extends
                | Token::Implements
                | Token::Public
                | Token::Protected
                | Token::Private
                | Token::Static
                | Token::Abstract
                | Token::Final
                | Token::Readonly
                | Token::Singleton
                | Token::Const
                | Token::New
                | Token::Clone
                | Token::Instanceof
                | Token::Namespace
                | Token::Use
                | Token::Declare
                | Token::Global
                | Token::Try
                | Token::Catch
                | Token::Finally
                | Token::Throw
                | Token::Async
                | Token::Await
                | Token::Defer
                | Token::And
                | Token::Or
                | Token::Xor
        )
    }

    /// Check if this token represents a type keyword.
    pub fn is_type(&self) -> bool {
        matches!(
            self,
            Token::TypeString
                | Token::TypeInt
                | Token::TypeFloat
                | Token::TypeBool
                | Token::TypeVoid
                | Token::TypeMixed
                | Token::TypeCallable
                | Token::TypeArray
                | Token::TypeObject
                | Token::TypeIterable
                | Token::TypeNever
                | Token::TypeList
                | Token::TypeDict
                | Token::TypeSet
                | Token::TypeHashMap
        )
    }

    /// Check if this token is a literal.
    pub fn is_literal(&self) -> bool {
        matches!(
            self,
            Token::IntLiteral(_)
                | Token::FloatLiteral(_)
                | Token::SingleQuotedString(_)
                | Token::DoubleQuotedString(_)
                | Token::BacktickString(_)
                | Token::True
                | Token::False
                | Token::Null
        )
    }

    /// Parse an integer literal to i64 (supporting hex, octal, binary, and decimal).
    pub fn parse_int(&self) -> Option<i64> {
        match self {
            Token::IntLiteral(raw) => {
                let cleaned: String = raw.chars().filter(|c| *c != '_').collect();
                if cleaned.starts_with("0x") || cleaned.starts_with("0X") {
                    i64::from_str_radix(&cleaned[2..], 16).ok()
                } else if cleaned.starts_with("0b") || cleaned.starts_with("0B") {
                    i64::from_str_radix(&cleaned[2..], 2).ok()
                } else if cleaned.starts_with("0o") || cleaned.starts_with("0O") {
                    i64::from_str_radix(&cleaned[2..], 8).ok()
                } else {
                    cleaned.parse::<i64>().ok()
                }
            }
            _ => None,
        }
    }

    /// Parse a float literal to f64.
    pub fn parse_float(&self) -> Option<f64> {
        match self {
            Token::FloatLiteral(raw) => {
                let cleaned: String = raw.chars().filter(|c| *c != '_').collect();
                cleaned.parse::<f64>().ok()
            }
            _ => None,
        }
    }

    /// Extracts the raw inner slice of a string literal, removing surrounding quotes.
    pub fn raw_string_content(&self) -> Option<&'s str> {
        match self {
            Token::SingleQuotedString(s) | Token::DoubleQuotedString(s) | Token::BacktickString(s) => {
                if s.len() >= 2 {
                    Some(&s[1..s.len() - 1])
                } else {
                    Some("")
                }
            }
            _ => None,
        }
    }

    /// Checks if a double-quoted string contains potential interpolation (`$` or `{$`).
    pub fn has_interpolation(&self) -> bool {
        match self {
            Token::DoubleQuotedString(s) => s.contains('$'),
            _ => false,
        }
    }

    /// Unescapes common escape characters within a string literal.
    pub fn unescape_string(&self) -> Option<String> {
        let content = self.raw_string_content()?;
        let mut result = String::with_capacity(content.len());
        let mut chars = content.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => result.push('\n'),
                    Some('r') => result.push('\r'),
                    Some('t') => result.push('\t'),
                    Some('\\') => result.push('\\'),
                    Some('\'') => result.push('\''),
                    Some('"') => result.push('"'),
                    Some('`') => result.push('`'),
                    Some('$') => result.push('$'),
                    Some('0') => result.push('\0'),
                    Some(other) => {
                        result.push('\\');
                        result.push(other);
                    }
                    None => result.push('\\'),
                }
            } else {
                result.push(c);
            }
        }

        Some(result)
    }
}

impl<'s> fmt::Display for Token<'s> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
