use std::fmt;
use crate::rade::compiler::registry::DirectiveRegistry;
use crate::rade::compiler::CompileError;

/// The kind of block-level directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveKind {
    If,
    For,
}

impl fmt::Display for DirectiveKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DirectiveKind::If => write!(f, "if"),
            DirectiveKind::For => write!(f, "for"),
        }
    }
}

/// A block directive entry stored on the directive stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveEntry {
    pub kind: DirectiveKind,
    pub line: usize,
    pub column: usize,
    pub has_else: bool,
}

/// Maintains an open/close directive stack to validate matching open/close tags
/// and proper control flow nesting.
#[derive(Debug, Clone, Default)]
pub struct DirectiveStack {
    stack: Vec<DirectiveEntry>,
}

impl DirectiveStack {
    /// Creates a new empty `DirectiveStack`.
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Returns true if the stack is currently empty.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Returns the current nesting depth of open block directives.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Pushes a new block opener onto the stack.
    pub fn push(&mut self, kind: DirectiveKind, line: usize, column: usize) {
        self.stack.push(DirectiveEntry {
            kind,
            line,
            column,
            has_else: false,
        });
    }

    /// Returns a reference to the innermost currently open block entry, if any.
    pub fn peek(&self) -> Option<&DirectiveEntry> {
        self.stack.last()
    }

    /// Handles an `@if` directive by pushing an `If` block onto the stack.
    pub fn handle_if(&mut self, line: usize, column: usize) {
        self.push(DirectiveKind::If, line, column);
    }

    /// Validates an `@elseif` directive against the current stack.
    pub fn handle_elseif(&mut self, line: usize, column: usize) -> Result<(), CompileError> {
        match self.stack.last() {
            None => Err(CompileError::UnexpectedDirective {
                directive: "elseif".to_string(),
                line,
                column,
                message: "@elseif cannot appear outside of an @if block".to_string(),
            }),
            Some(entry) if entry.kind != DirectiveKind::If => {
                Err(CompileError::UnexpectedDirective {
                    directive: "elseif".to_string(),
                    line,
                    column,
                    message: format!("@elseif cannot appear inside a @{} block", entry.kind),
                })
            }
            Some(entry) if entry.has_else => Err(CompileError::UnexpectedDirective {
                directive: "elseif".to_string(),
                line,
                column,
                message: "@elseif cannot appear after @else in the same @if block".to_string(),
            }),
            Some(_) => Ok(()),
        }
    }

    /// Validates an `@else` directive against the current stack.
    pub fn handle_else(&mut self, line: usize, column: usize) -> Result<(), CompileError> {
        match self.stack.last_mut() {
            None => Err(CompileError::UnexpectedDirective {
                directive: "else".to_string(),
                line,
                column,
                message: "@else cannot appear outside of an @if block".to_string(),
            }),
            Some(entry) if entry.kind != DirectiveKind::If => {
                Err(CompileError::UnexpectedDirective {
                    directive: "else".to_string(),
                    line,
                    column,
                    message: format!("@else cannot appear inside a @{} block", entry.kind),
                })
            }
            Some(entry) if entry.has_else => Err(CompileError::UnexpectedDirective {
                directive: "else".to_string(),
                line,
                column,
                message: "Multiple @else directives in the same @if block".to_string(),
            }),
            Some(entry) => {
                entry.has_else = true;
                Ok(())
            }
        }
    }

    /// Validates and closes an `@if` block upon encountering `@endif`.
    pub fn handle_endif(&mut self, line: usize, column: usize) -> Result<(), CompileError> {
        match self.stack.last() {
            None => Err(CompileError::UnexpectedClosingDirective {
                directive: "endif".to_string(),
                line,
                column,
            }),
            Some(entry) if entry.kind != DirectiveKind::If => {
                Err(CompileError::MismatchedClosingDirective {
                    expected: format!("end{}", entry.kind),
                    found: "endif".to_string(),
                    line,
                    column,
                })
            }
            Some(_) => {
                self.stack.pop();
                Ok(())
            }
        }
    }

    /// Handles a `@for` directive by pushing a `For` block onto the stack.
    pub fn handle_for(&mut self, line: usize, column: usize) {
        self.push(DirectiveKind::For, line, column);
    }

    /// Validates and closes a `@for` block upon encountering `@endfor`.
    pub fn handle_endfor(&mut self, line: usize, column: usize) -> Result<(), CompileError> {
        match self.stack.last() {
            None => Err(CompileError::UnexpectedClosingDirective {
                directive: "endfor".to_string(),
                line,
                column,
            }),
            Some(entry) if entry.kind != DirectiveKind::For => {
                Err(CompileError::MismatchedClosingDirective {
                    expected: format!("end{}", entry.kind),
                    found: "endfor".to_string(),
                    line,
                    column,
                })
            }
            Some(_) => {
                self.stack.pop();
                Ok(())
            }
        }
    }

    /// Validates that all opened blocks were closed when EOF is reached.
    pub fn finish(&self) -> Result<(), CompileError> {
        if let Some(entry) = self.stack.last() {
            Err(CompileError::UnclosedDirective {
                directive: entry.kind.to_string(),
                line: entry.line,
                column: entry.column,
            })
        } else {
            Ok(())
        }
    }
}

/// Returns true if the directive name is a known built-in control flow directive.
pub fn is_builtin_directive(name: &str) -> bool {
    matches!(name, "if" | "elseif" | "else" | "endif" | "for" | "endfor")
}

/// Compiles `@if($condition)` to `if ($condition) {`.
pub fn compile_if(condition: &str) -> String {
    format!("if ({}) {{", condition.trim())
}

/// Compiles `@elseif($condition)` to `} else if ($condition) {`.
pub fn compile_elseif(condition: &str) -> String {
    format!("}} else if ({}) {{", condition.trim())
}

/// Compiles `@else` to `} else {`.
pub fn compile_else() -> &'static str {
    "} else {"
}

/// Compiles `@endif` to `}`.
pub fn compile_endif() -> &'static str {
    "}"
}

/// Compiles `@for($init; $condition; $step)` to `for ($init; $condition; $step) {`.
pub fn compile_for(args: &str) -> String {
    format!("for ({}) {{", args.trim())
}

/// Compiles `@endfor` to `}`.
pub fn compile_endfor() -> &'static str {
    "}"
}

/// Processes and compiles a directive token using the provided registry (and falling back to the global registry),
/// updating and validating against the directive stack.
pub fn compile_directive_with_registry(
    name: &str,
    args: Option<&str>,
    line: usize,
    column: usize,
    stack: &mut DirectiveStack,
    registry: &DirectiveRegistry,
) -> Result<String, CompileError> {
    match name {
        "if" => {
            let cond = args
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CompileError::MissingDirectiveArgs {
                    directive: "if".to_string(),
                    line,
                    column,
                })?;
            stack.handle_if(line, column);
            Ok(compile_if(cond))
        }
        "elseif" => {
            let cond = args
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CompileError::MissingDirectiveArgs {
                    directive: "elseif".to_string(),
                    line,
                    column,
                })?;
            stack.handle_elseif(line, column)?;
            Ok(compile_elseif(cond))
        }
        "else" => {
            if let Some(arg) = args {
                if !arg.trim().is_empty() {
                    return Err(CompileError::UnexpectedDirectiveArgs {
                        directive: "else".to_string(),
                        line,
                        column,
                    });
                }
            }
            stack.handle_else(line, column)?;
            Ok(compile_else().to_string())
        }
        "endif" => {
            if let Some(arg) = args {
                if !arg.trim().is_empty() {
                    return Err(CompileError::UnexpectedDirectiveArgs {
                        directive: "endif".to_string(),
                        line,
                        column,
                    });
                }
            }
            stack.handle_endif(line, column)?;
            Ok(compile_endif().to_string())
        }
        "for" => {
            let for_args = args
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CompileError::MissingDirectiveArgs {
                    directive: "for".to_string(),
                    line,
                    column,
                })?;
            stack.handle_for(line, column);
            Ok(compile_for(for_args))
        }
        "endfor" => {
            if let Some(arg) = args {
                if !arg.trim().is_empty() {
                    return Err(CompileError::UnexpectedDirectiveArgs {
                        directive: "endfor".to_string(),
                        line,
                        column,
                    });
                }
            }
            stack.handle_endfor(line, column)?;
            Ok(compile_endfor().to_string())
        }
        _ => {
            // Check custom registry (local first, then global)
            let handler = registry.get(name).or_else(|| DirectiveRegistry::global().get(name));
            if let Some(handler) = handler {
                Ok(handler(args))
            } else if args.is_some() {
                Err(CompileError::UnknownDirective {
                    directive: name.to_string(),
                    line,
                    column,
                })
            } else {
                // If unregistered and lacks parenthesis, treat as plain text
                Ok(format!("@{name}"))
            }
        }
    }
}

/// Processes and compiles a directive token using the global directive registry,
/// updating and validating against the directive stack.
pub fn compile_directive(
    name: &str,
    args: Option<&str>,
    line: usize,
    column: usize,
    stack: &mut DirectiveStack,
) -> Result<String, CompileError> {
    compile_directive_with_registry(name, args, line, column, stack, DirectiveRegistry::global())
}

