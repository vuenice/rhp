pub mod codegen;
pub mod directives;
pub mod registry;

pub use codegen::{
    CodeGenerator, CodegenOptions, CompilerOptions, OutputFormat,
};
pub use directives::{
    compile_directive, compile_directive_with_registry, compile_else, compile_elseif,
    compile_endfor, compile_endif, compile_for, compile_if, is_builtin_directive,
    DirectiveEntry, DirectiveKind, DirectiveStack,
};
pub use registry::{register_directive, DirectiveHandler, DirectiveRegistry};

use std::fmt;

use crate::rade::lexer::{LexerError, Scanner, SpannedToken, Token};

/// Errors that can occur during template compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// Lexer error occurred during scanning
    Lexer(LexerError),

    /// A closing directive (@endif, @endfor) was encountered without a matching opener
    UnexpectedClosingDirective {
        directive: String,
        line: usize,
        column: usize,
    },

    /// A closing directive does not match the currently open block
    MismatchedClosingDirective {
        expected: String,
        found: String,
        line: usize,
        column: usize,
    },

    /// The template ended with an unclosed block (@if or @for)
    UnclosedDirective {
        directive: String,
        line: usize,
        column: usize,
    },

    /// A directive appears in an invalid context (e.g. @else or @elseif outside of an @if block)
    UnexpectedDirective {
        directive: String,
        line: usize,
        column: usize,
        message: String,
    },

    /// A directive requires arguments that were not provided or were empty
    MissingDirectiveArgs {
        directive: String,
        line: usize,
        column: usize,
    },

    /// A directive received unexpected arguments
    UnexpectedDirectiveArgs {
        directive: String,
        line: usize,
        column: usize,
    },

    /// A directive is not recognized as a built-in or registered custom directive
    UnknownDirective {
        directive: String,
        line: usize,
        column: usize,
    },
}

impl CompileError {
    /// Returns the 1-indexed line number where the error occurred or started.
    pub fn line(&self) -> usize {
        match self {
            CompileError::Lexer(err) => match err {
                LexerError::UnclosedComment { line, .. }
                | LexerError::UnclosedEcho { line, .. }
                | LexerError::UnclosedRawEcho { line, .. }
                | LexerError::UnclosedRawRhp { line, .. }
                | LexerError::UnclosedDirectiveArgs { line, .. } => *line,
            },
            CompileError::UnexpectedClosingDirective { line, .. }
            | CompileError::MismatchedClosingDirective { line, .. }
            | CompileError::UnclosedDirective { line, .. }
            | CompileError::UnexpectedDirective { line, .. }
            | CompileError::MissingDirectiveArgs { line, .. }
            | CompileError::UnexpectedDirectiveArgs { line, .. }
            | CompileError::UnknownDirective { line, .. } => *line,
        }
    }

    /// Returns the 1-indexed column number where the error occurred or started.
    pub fn column(&self) -> usize {
        match self {
            CompileError::Lexer(err) => match err {
                LexerError::UnclosedComment { column, .. }
                | LexerError::UnclosedEcho { column, .. }
                | LexerError::UnclosedRawEcho { column, .. }
                | LexerError::UnclosedRawRhp { column, .. }
                | LexerError::UnclosedDirectiveArgs { column, .. } => *column,
            },
            CompileError::UnexpectedClosingDirective { column, .. }
            | CompileError::MismatchedClosingDirective { column, .. }
            | CompileError::UnclosedDirective { column, .. }
            | CompileError::UnexpectedDirective { column, .. }
            | CompileError::MissingDirectiveArgs { column, .. }
            | CompileError::UnexpectedDirectiveArgs { column, .. }
            | CompileError::UnknownDirective { column, .. } => *column,
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Lexer(err) => write!(f, "Lexer error: {err}"),
            CompileError::UnexpectedClosingDirective {
                directive,
                line,
                column,
            } => {
                write!(
                    f,
                    "Unexpected closing directive '@{directive}' without matching opener at line {line}, column {column}"
                )
            }
            CompileError::MismatchedClosingDirective {
                expected,
                found,
                line,
                column,
            } => {
                write!(
                    f,
                    "Mismatched closing directive: expected '@{expected}', found '@{found}' at line {line}, column {column}"
                )
            }
            CompileError::UnclosedDirective {
                directive,
                line,
                column,
            } => {
                write!(
                    f,
                    "Unclosed directive '@{directive}' starting at line {line}, column {column}"
                )
            }
            CompileError::UnexpectedDirective {
                directive,
                line,
                column,
                message,
            } => {
                write!(
                    f,
                    "Unexpected directive '@{directive}' at line {line}, column {column}: {message}"
                )
            }
            CompileError::MissingDirectiveArgs {
                directive,
                line,
                column,
            } => {
                write!(
                    f,
                    "Directive '@{directive}' requires arguments at line {line}, column {column}"
                )
            }
            CompileError::UnexpectedDirectiveArgs {
                directive,
                line,
                column,
            } => {
                write!(
                    f,
                    "Directive '@{directive}' does not accept arguments at line {line}, column {column}"
                )
            }
            CompileError::UnknownDirective {
                directive,
                line,
                column,
            } => {
                write!(
                    f,
                    "Unknown directive '@{directive}' at line {line}, column {column}"
                )
            }
        }
    }
}

impl std::error::Error for CompileError {}

impl From<LexerError> for CompileError {
    fn from(err: LexerError) -> Self {
        CompileError::Lexer(err)
    }
}

/// Compiler for Rade templates that compiles built-in directives, raw RHP blocks,
/// custom registered directives, and template tokens into target code.
#[derive(Debug, Clone)]
pub struct Compiler {
    stack: DirectiveStack,
    registry: DirectiveRegistry,
    options: CompilerOptions,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    /// Creates a new `Compiler` instance with an empty directive stack, a local directive registry,
    /// and default configuration (directives-only expansion for backwards compatibility).
    pub fn new() -> Self {
        Self {
            stack: DirectiveStack::new(),
            registry: DirectiveRegistry::new(),
            options: CompilerOptions::directives_only(),
        }
    }

    /// Creates a new `Compiler` instance configured for standalone script RHP codegen.
    pub fn standalone() -> Self {
        Self {
            stack: DirectiveStack::new(),
            registry: DirectiveRegistry::new(),
            options: CompilerOptions::standalone(),
        }
    }

    /// Creates a new `Compiler` instance configured for callable closure RHP codegen.
    pub fn closure() -> Self {
        Self {
            stack: DirectiveStack::new(),
            registry: DirectiveRegistry::new(),
            options: CompilerOptions::closure(),
        }
    }

    /// Creates a new `Compiler` instance with specific compiler options.
    pub fn with_options(options: CompilerOptions) -> Self {
        Self {
            stack: DirectiveStack::new(),
            registry: DirectiveRegistry::new(),
            options,
        }
    }

    /// Creates a new `Compiler` instance with a specific directive registry.
    pub fn with_registry(registry: DirectiveRegistry) -> Self {
        Self {
            stack: DirectiveStack::new(),
            registry,
            options: CompilerOptions::directives_only(),
        }
    }

    /// Fluent builder method to set the target output format.
    pub fn with_format(mut self, format: OutputFormat) -> Self {
        self.options.format = format;
        self
    }

    /// Returns a reference to the compiler options.
    pub fn options(&self) -> &CompilerOptions {
        &self.options
    }

    /// Returns a mutable reference to the compiler options.
    pub fn options_mut(&mut self) -> &mut CompilerOptions {
        &mut self.options
    }

    /// Sets the compiler options.
    pub fn set_options(&mut self, options: CompilerOptions) {
        self.options = options;
    }

    /// Sets the target output format.
    pub fn set_format(&mut self, format: OutputFormat) {
        self.options.format = format;
    }

    /// Registers a custom directive on this compiler instance.
    pub fn register_directive<F>(&mut self, name: impl Into<String>, handler: F) -> &mut Self
    where
        F: Fn(Option<&str>) -> String + Send + Sync + 'static,
    {
        self.registry.register(name, handler);
        self
    }

    /// Returns a reference to the compiler's directive registry.
    pub fn registry(&self) -> &DirectiveRegistry {
        &self.registry
    }

    /// Returns a mutable reference to the compiler's directive registry.
    pub fn registry_mut(&mut self) -> &mut DirectiveRegistry {
        &mut self.registry
    }

    /// Replaces the compiler's directive registry.
    pub fn set_registry(&mut self, registry: DirectiveRegistry) {
        self.registry = registry;
    }

    /// Compiles a template string into target RHP/template code according to the configured options.
    pub fn compile(&mut self, input: &str) -> Result<String, CompileError> {
        let spanned_tokens = Scanner::scan_spanned(input)?;
        self.compile_spanned_tokens(&spanned_tokens)
    }

    /// Compiles a template string into standalone RHP code using pure string accumulation.
    pub fn compile_to_rhp(&mut self, input: &str) -> Result<String, CompileError> {
        let spanned_tokens = Scanner::scan_spanned(input)?;
        CodeGenerator::new(CompilerOptions::standalone()).generate(&spanned_tokens, &self.registry)
    }

    /// Compiles a template string into callable closure RHP code using pure string accumulation.
    pub fn compile_to_closure(&mut self, input: &str) -> Result<String, CompileError> {
        let spanned_tokens = Scanner::scan_spanned(input)?;
        CodeGenerator::new(CompilerOptions::closure()).generate(&spanned_tokens, &self.registry)
    }

    /// Compiles a slice of `SpannedToken`s according to the configured options.
    pub fn compile_spanned_tokens(
        &mut self,
        tokens: &[SpannedToken],
    ) -> Result<String, CompileError> {
        match self.options.format {
            OutputFormat::DirectivesOnly => {
                let mut output = String::new();

                for spanned in tokens {
                    match &spanned.token {
                        Token::Text(text) => {
                            output.push_str(text);
                        }
                        Token::Comment(_) => {
                            // Blade comments are stripped during compilation
                        }
                        Token::RawRhp(code) => {
                            // Directly emits the inner RHP code as-is without any wrapping or alteration
                            output.push_str(code);
                        }
                        Token::Directive { name, args } => {
                            let compiled = compile_directive_with_registry(
                                name,
                                args.as_deref(),
                                spanned.line,
                                spanned.column,
                                &mut self.stack,
                                &self.registry,
                            )?;
                            output.push_str(&compiled);
                        }
                        Token::EchoEscaped(expr) => {
                            output.push_str("{{ ");
                            output.push_str(expr);
                            output.push_str(" }}");
                        }
                        Token::EchoRaw(expr) => {
                            output.push_str("{!! ");
                            output.push_str(expr);
                            output.push_str(" !!}");
                        }
                        Token::EscapedVerbatim(verbatim) => {
                            output.push_str(verbatim);
                        }
                    }
                }

                self.stack.finish()?;
                Ok(output)
            }
            OutputFormat::Standalone | OutputFormat::Closure => {
                CodeGenerator::new(self.options).generate(tokens, &self.registry)
            }
        }
    }

    /// Compiles a slice of `Token`s, using line 1, column 1 as fallback positions.
    pub fn compile_tokens(&mut self, tokens: &[Token]) -> Result<String, CompileError> {
        let spanned: Vec<SpannedToken> = tokens
            .iter()
            .map(|t| SpannedToken::new(t.clone(), 1, 1))
            .collect();
        self.compile_spanned_tokens(&spanned)
    }
}

/// Convenience helper function to compile a template string in one call (legacy directive-expansion pass).
pub fn compile(input: &str) -> Result<String, CompileError> {
    Compiler::new().compile(input)
}

/// Convenience helper function to compile a template string into standalone RHP code.
pub fn compile_to_rhp(input: &str) -> Result<String, CompileError> {
    CodeGenerator::new(CompilerOptions::standalone()).compile(input, DirectiveRegistry::global())
}

/// Convenience helper function to compile a template string into callable closure format.
pub fn compile_to_closure(input: &str) -> Result<String, CompileError> {
    CodeGenerator::new(CompilerOptions::closure()).compile(input, DirectiveRegistry::global())
}

/// Convenience helper function to compile a template string with specific options.
pub fn compile_with_options(
    input: &str,
    options: CompilerOptions,
) -> Result<String, CompileError> {
    CodeGenerator::new(options).compile(input, DirectiveRegistry::global())
}
