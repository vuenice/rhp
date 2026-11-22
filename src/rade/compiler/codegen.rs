use crate::rade::compiler::directives::{
    compile_else, compile_elseif, compile_endfor, compile_endif, compile_for, compile_if,
    compile_directive_with_registry, DirectiveStack,
};
use crate::rade::compiler::registry::DirectiveRegistry;
use crate::rade::compiler::CompileError;
use crate::rade::lexer::{Scanner, SpannedToken, Token};

/// Target output format for compiled RHP code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Standalone script format:
    /// ```rhp
    /// $__out = "";
    /// ...
    /// return $__out;
    /// ```
    #[default]
    Standalone,

    /// Callable closure format:
    /// ```rhp
    /// return function($__data: dict): string {
    ///     $__out = "";
    ///     ...
    ///     return $__out;
    /// };
    /// ```
    Closure,

    /// Directive-only transpilation pass without string accumulation (Milestone 2/3 compatibility)
    DirectivesOnly,
}

/// Compiler and code generator options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompilerOptions {
    /// Target output format.
    pub format: OutputFormat,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Standalone,
        }
    }
}

impl CompilerOptions {
    /// Creates a new `CompilerOptions` with default settings (Standalone).
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates options configured for standalone script format.
    pub fn standalone() -> Self {
        Self {
            format: OutputFormat::Standalone,
        }
    }

    /// Creates options configured for callable closure format.
    pub fn closure() -> Self {
        Self {
            format: OutputFormat::Closure,
        }
    }

    /// Creates options configured for directive-only expansion.
    pub fn directives_only() -> Self {
        Self {
            format: OutputFormat::DirectivesOnly,
        }
    }
}

/// Alias for `CompilerOptions` specifically for codegen.
pub type CodegenOptions = CompilerOptions;

/// Escapes quotes and special characters in string literals for RHP double-quoted strings.
pub fn escape_rhp_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// Pure string accumulator code generator for RHP.
#[derive(Debug, Clone)]
pub struct CodeGenerator {
    options: CompilerOptions,
}

impl Default for CodeGenerator {
    fn default() -> Self {
        Self::new(CompilerOptions::default())
    }
}

impl CodeGenerator {
    /// Creates a new `CodeGenerator` with the specified options.
    pub fn new(options: CompilerOptions) -> Self {
        Self { options }
    }

    /// Returns the compiler options used by this code generator.
    pub fn options(&self) -> &CompilerOptions {
        &self.options
    }

    /// Returns a mutable reference to the options used by this code generator.
    pub fn options_mut(&mut self) -> &mut CompilerOptions {
        &mut self.options
    }

    /// Compiles a template string into target RHP code using the provided directive registry.
    pub fn compile(
        &self,
        input: &str,
        registry: &DirectiveRegistry,
    ) -> Result<String, CompileError> {
        let spanned_tokens = Scanner::scan_spanned(input)?;
        self.generate(&spanned_tokens, registry)
    }

    /// Compiles a slice of `Token`s into target RHP code using default positions.
    pub fn generate_tokens(
        &self,
        tokens: &[Token],
        registry: &DirectiveRegistry,
    ) -> Result<String, CompileError> {
        let spanned: Vec<SpannedToken> = tokens
            .iter()
            .map(|t| SpannedToken::new(t.clone(), 1, 1))
            .collect();
        self.generate(&spanned, registry)
    }

    /// Generates pure, valid RHP code from spanned tokens adhering to RHP language rules:
    /// - Initializes `$__out = "";`
    /// - Accumulates with `+=`
    /// - Merges consecutive text nodes and escapes special characters
    /// - Handles `EchoEscaped`, `EchoRaw`, `EscapedVerbatim`
    /// - Validates and emits control flow directives (`if`, `for`, etc.)
    /// - Ends with `return $__out;`
    /// - Wraps in callable closure or standalone script format based on `options.format`
    pub fn generate(
        &self,
        tokens: &[SpannedToken],
        registry: &DirectiveRegistry,
    ) -> Result<String, CompileError> {
        let mut stack = DirectiveStack::new();
        let mut lines: Vec<String> = Vec::new();
        let is_closure = self.options.format == OutputFormat::Closure;
        let base_indent: usize = if is_closure { 1 } else { 0 };
        let mut current_block_indent: usize = 0;

        let indent_str = |level: usize| -> String {
            "    ".repeat(level)
        };

        if is_closure {
            lines.push("return function($__data: dict): string {".to_string());
        }

        // Initialize accumulator: $__out = "";
        lines.push(format!("{}$__out = \"\";", indent_str(base_indent)));

        let mut pending_text = String::new();

        let flush_text = |lines: &mut Vec<String>,
                          pending_text: &mut String,
                          block_indent: usize| {
            if !pending_text.is_empty() {
                let escaped = escape_rhp_string(pending_text);
                let level = base_indent + block_indent;
                lines.push(format!("{}$__out += \"{}\";", indent_str(level), escaped));
                pending_text.clear();
            }
        };

        for spanned in tokens {
            match &spanned.token {
                Token::Text(text) => {
                    // Accumulate consecutive text nodes for performance
                    pending_text.push_str(text);
                }
                Token::Comment(_) => {
                    // Blade comments are stripped during compilation.
                    // Consecutive text across comments is merged.
                }
                Token::Directive { name, args } => {
                    // Check if this directive is unregistered and parameterless:
                    // in Blade, unregistered directives without parentheses are treated as literal text
                    let is_custom = registry.contains(name) || DirectiveRegistry::global().contains(name);
                    let is_builtin = matches!(name.as_str(), "if" | "elseif" | "else" | "endif" | "for" | "endfor");

                    if !is_builtin && !is_custom && args.is_none() {
                        // Unregistered without parentheses -> treat as literal text
                        pending_text.push_str(&format!("@{name}"));
                        continue;
                    }

                    // Flush any pending text before directive control flow or custom helper
                    flush_text(&mut lines, &mut pending_text, current_block_indent);

                    match name.as_str() {
                        "if" => {
                            let cond = args
                                .as_deref()
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .ok_or_else(|| CompileError::MissingDirectiveArgs {
                                    directive: "if".to_string(),
                                    line: spanned.line,
                                    column: spanned.column,
                                })?;
                            stack.handle_if(spanned.line, spanned.column);
                            let code = compile_if(cond);
                            let level = base_indent + current_block_indent;
                            lines.push(format!("{}{}", indent_str(level), code));
                            current_block_indent += 1;
                        }
                        "elseif" => {
                            let cond = args
                                .as_deref()
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .ok_or_else(|| CompileError::MissingDirectiveArgs {
                                    directive: "elseif".to_string(),
                                    line: spanned.line,
                                    column: spanned.column,
                                })?;
                            stack.handle_elseif(spanned.line, spanned.column)?;
                            let code = compile_elseif(cond);
                            if current_block_indent > 0 {
                                current_block_indent -= 1;
                            }
                            let level = base_indent + current_block_indent;
                            lines.push(format!("{}{}", indent_str(level), code));
                            current_block_indent += 1;
                        }
                        "else" => {
                            if let Some(arg) = args {
                                if !arg.trim().is_empty() {
                                    return Err(CompileError::UnexpectedDirectiveArgs {
                                        directive: "else".to_string(),
                                        line: spanned.line,
                                        column: spanned.column,
                                    });
                                }
                            }
                            stack.handle_else(spanned.line, spanned.column)?;
                            let code = compile_else();
                            if current_block_indent > 0 {
                                current_block_indent -= 1;
                            }
                            let level = base_indent + current_block_indent;
                            lines.push(format!("{}{}", indent_str(level), code));
                            current_block_indent += 1;
                        }
                        "endif" => {
                            if let Some(arg) = args {
                                if !arg.trim().is_empty() {
                                    return Err(CompileError::UnexpectedDirectiveArgs {
                                        directive: "endif".to_string(),
                                        line: spanned.line,
                                        column: spanned.column,
                                    });
                                }
                            }
                            stack.handle_endif(spanned.line, spanned.column)?;
                            let code = compile_endif();
                            if current_block_indent > 0 {
                                current_block_indent -= 1;
                            }
                            let level = base_indent + current_block_indent;
                            lines.push(format!("{}{}", indent_str(level), code));
                        }
                        "for" => {
                            let for_args = args
                                .as_deref()
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .ok_or_else(|| CompileError::MissingDirectiveArgs {
                                    directive: "for".to_string(),
                                    line: spanned.line,
                                    column: spanned.column,
                                })?;
                            stack.handle_for(spanned.line, spanned.column);
                            let code = compile_for(for_args);
                            let level = base_indent + current_block_indent;
                            lines.push(format!("{}{}", indent_str(level), code));
                            current_block_indent += 1;
                        }
                        "endfor" => {
                            if let Some(arg) = args {
                                if !arg.trim().is_empty() {
                                    return Err(CompileError::UnexpectedDirectiveArgs {
                                        directive: "endfor".to_string(),
                                        line: spanned.line,
                                        column: spanned.column,
                                    });
                                }
                            }
                            stack.handle_endfor(spanned.line, spanned.column)?;
                            let code = compile_endfor();
                            if current_block_indent > 0 {
                                current_block_indent -= 1;
                            }
                            let level = base_indent + current_block_indent;
                            lines.push(format!("{}{}", indent_str(level), code));
                        }
                        _ => {
                            // Custom directive or error
                            let compiled = compile_directive_with_registry(
                                name,
                                args.as_deref(),
                                spanned.line,
                                spanned.column,
                                &mut stack,
                                registry,
                            )?;
                            for line in compiled.lines() {
                                let trimmed = line.trim();
                                if !trimmed.is_empty() {
                                    let level = base_indent + current_block_indent;
                                    lines.push(format!("{}{}", indent_str(level), trimmed));
                                }
                            }
                        }
                    }
                }
                Token::EchoEscaped(expr) => {
                    flush_text(&mut lines, &mut pending_text, current_block_indent);
                    let level = base_indent + current_block_indent;
                    lines.push(format!(
                        "{}$__out += Html::escape({});",
                        indent_str(level),
                        expr.trim()
                    ));
                }
                Token::EchoRaw(expr) => {
                    flush_text(&mut lines, &mut pending_text, current_block_indent);
                    let level = base_indent + current_block_indent;
                    lines.push(format!(
                        "{}$__out += string({});",
                        indent_str(level),
                        expr.trim()
                    ));
                }
                Token::EscapedVerbatim(verbatim) => {
                    flush_text(&mut lines, &mut pending_text, current_block_indent);
                    let formatted = if verbatim.starts_with("{{") && verbatim.ends_with("}}") {
                        escape_rhp_string(verbatim)
                    } else {
                        format!("{{{{ {} }}}}", escape_rhp_string(verbatim.trim()))
                    };
                    let level = base_indent + current_block_indent;
                    lines.push(format!("{}$__out += \"{}\";", indent_str(level), formatted));
                }
                Token::RawRhp(code) => {
                    flush_text(&mut lines, &mut pending_text, current_block_indent);
                    for line in code.lines() {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            let level = base_indent + current_block_indent;
                            lines.push(format!("{}{}", indent_str(level), trimmed));
                        }
                    }
                }
            }
        }

        // Flush any trailing plain text
        flush_text(&mut lines, &mut pending_text, current_block_indent);

        // Validate that all control flow blocks were properly closed
        stack.finish()?;

        // End compiled code with return $__out;
        lines.push(format!("{}return $__out;", indent_str(base_indent)));

        if is_closure {
            lines.push("};".to_string());
        }

        Ok(lines.join("\n"))
    }
}

/// Convenience helper function to compile a template string into standalone RHP code.
pub fn compile_to_rhp(input: &str) -> Result<String, CompileError> {
    CodeGenerator::new(CompilerOptions::standalone()).compile(input, DirectiveRegistry::global())
}

/// Convenience helper function to compile a template string into callable closure RHP code.
pub fn compile_to_closure(input: &str) -> Result<String, CompileError> {
    CodeGenerator::new(CompilerOptions::closure()).compile(input, DirectiveRegistry::global())
}

/// Convenience helper function to compile a template string with specific options.
pub fn compile(
    input: &str,
    options: &CompilerOptions,
    registry: &DirectiveRegistry,
) -> Result<String, CompileError> {
    CodeGenerator::new(*options).compile(input, registry)
}

/// Compiles a slice of `Token`s with specific options.
pub fn compile_tokens(
    tokens: &[Token],
    options: &CompilerOptions,
    registry: &DirectiveRegistry,
) -> Result<String, CompileError> {
    CodeGenerator::new(*options).generate_tokens(tokens, registry)
}

/// Compiles a slice of `SpannedToken`s with specific options.
pub fn compile_spanned_tokens(
    tokens: &[SpannedToken],
    options: &CompilerOptions,
    registry: &DirectiveRegistry,
) -> Result<String, CompileError> {
    CodeGenerator::new(*options).generate(tokens, registry)
}
