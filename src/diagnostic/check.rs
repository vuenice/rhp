//! Compiler verification pass (`rhp check`) with cascading error recovery and suppression.

use std::fs;
use std::path::Path;

use crate::ast::Program;
use crate::lexer::{span::SourceFile, Lexer};
use crate::lint::run_typecheck;
use crate::parser::{CombinatorStream, Parser};

use super::{builder::DiagnosticBuilder, Diagnostic};

/// Default maximum errors per file to report before suppressing cascading noise.
pub const DEFAULT_MAX_ERRORS_PER_FILE: usize = 5;

/// Check an in-memory RHP source string and return all discovered diagnostics.
///
/// Implements parser recovery and cascading error suppression up to `max_errors`.
pub fn check_source(source: &str, file_name: &str, max_errors: usize) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let source_file = SourceFile::new(0, file_name, source);
    let builder = DiagnosticBuilder::new(&source_file);

    // 1. Lexing phase: token check
    let tokens = match Lexer::tokenize_with_file_id(source, 0) {
        Ok(toks) => toks,
        Err(lex_err) => {
            diagnostics.push(builder.from_lex_error(&lex_err));
            return diagnostics;
        }
    };

    // 2. Parsing phase with recovery and cascading suppression
    let mut stream = CombinatorStream::new(tokens, 0);
    stream.source = Some(source);
    let mut parser = Parser::new(stream.tokens.clone(), 0);
    parser = parser.with_file_path(file_name);

    let parsed_program: Option<Program<'_>> = match parser.parse_program() {
        Ok(prog) => Some(prog),
        Err(parse_err) => {
            diagnostics.push(builder.from_parse_error(&parse_err));

            // If an unclosed delimiter or fatal error occurred, attempt statement-level recovery
            let mut recovered_stream = CombinatorStream::new(stream.tokens, 0);
            recovered_stream.source = Some(source);
            recovered_stream.cursor = parse_err.span.end.min(recovered_stream.tokens.len());

            while !recovered_stream.is_eof() && diagnostics.len() < max_errors {
                recovered_stream.skip_php_tags();
                recovered_stream.skip_doc_comments();
                if recovered_stream.is_eof() {
                    break;
                }

                // Advance to next statement boundary (semicolon or closing brace)
                while !recovered_stream.is_eof() {
                    if let Some(tok) = recovered_stream.advance() {
                        if matches!(
                            tok.token,
                            crate::lexer::Token::SemiColon
                                | crate::lexer::Token::CloseBrace
                                | crate::lexer::Token::PhpClose
                        ) {
                            break;
                        }
                    }
                }

                if recovered_stream.is_eof() {
                    break;
                }

                // Attempt to parse next statement
                if let Err(followup_err) = recovered_stream.parse_stmt() {
                    let mut followup_diag = builder.from_parse_error(&followup_err);
                    // Mark as potentially caused by initial syntax error
                    if let Some(first_diag) = diagnostics.first().cloned() {
                        followup_diag.caused_by = Some(Box::new(first_diag));
                    }
                    diagnostics.push(followup_diag);
                }
            }

            None
        }
    };

    // 3. Typecheck & boundary verification phase (if parsing succeeded)
    if parsed_program.is_some() {
        let issues = run_typecheck(source, file_name);
        for issue in issues {
            if diagnostics.len() >= max_errors {
                break;
            }
            diagnostics.push(builder.from_issue(&issue));
        }
    }

    // 4. Sort diagnostics in source order
    diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.col, d.span.start.byte));

    // Cap diagnostics to max_errors
    if diagnostics.len() > max_errors {
        diagnostics.truncate(max_errors);
    }

    diagnostics
}

/// Check a single source file on disk.
pub fn check_file(path: &Path, max_errors: usize) -> Result<Vec<Diagnostic>, std::io::Error> {
    let source = fs::read_to_string(path)?;
    let file_name = path.to_string_lossy().to_string();
    Ok(check_source(&source, &file_name, max_errors))
}

/// Check a target path (file or directory recursively).
pub fn check_path(target: &Path, max_errors: usize) -> Result<Vec<Diagnostic>, std::io::Error> {
    let mut all_diagnostics = Vec::new();

    if target.is_file() {
        let mut diags = check_file(target, max_errors)?;
        all_diagnostics.append(&mut diags);
    } else if target.is_dir() {
        let mut rhp_files = Vec::new();
        collect_rhp_files(target, &mut rhp_files);

        for file in rhp_files {
            if let Ok(mut diags) = check_file(&file, max_errors) {
                all_diagnostics.append(&mut diags);
            }
        }
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Path '{}' does not exist", target.display()),
        ));
    }

    all_diagnostics.sort_by(|a, b| {
        a.span
            .file
            .cmp(&b.span.file)
            .then_with(|| a.span.start.line.cmp(&b.span.start.line))
            .then_with(|| a.span.start.col.cmp(&b.span.start.col))
    });

    Ok(all_diagnostics)
}

/// Recursively collect all `.rhp` files in a directory, ignoring `vendor/`, `target/`, and `.git/`.
fn collect_rhp_files(dir: &Path, collected: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "vendor" || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                collect_rhp_files(&path, collected);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rhp") {
                collected.push(path);
            }
        }
    }
}
