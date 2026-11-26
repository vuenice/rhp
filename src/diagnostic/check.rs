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

/// Determines whether a file path or source content represents a Rade/HTML template.
pub fn is_template_file(file_name: &str, source: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".rade.rhp")
        || lower.ends_with(".rade")
        || lower.ends_with(".blade.php")
        || lower.ends_with(".blade.rhp")
        || lower.ends_with(".phtml")
    {
        return true;
    }
    crate::lint::is_php_view_template(source, file_name)
}

/// Check an in-memory RHP source string and return all discovered diagnostics.
///
/// Implements parser recovery and cascading error suppression up to `max_errors`.
pub fn check_source(source: &str, file_name: &str, max_errors: usize) -> Vec<Diagnostic> {
    if is_template_file(file_name, source) {
        return check_template_source(source, file_name, max_errors);
    }

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

/// Check an in-memory Rade/HTML template source string and return all discovered diagnostics.
///
/// Implements template structure validation and native RHP block (@rhp ... @endrhp) linting.
pub fn check_template_source(source: &str, file_name: &str, max_errors: usize) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let source_file = SourceFile::new(0, file_name, source);
    let builder = DiagnosticBuilder::new(&source_file);

    // 1. Scan template with Rade scanner to validate syntax and tokenize directives
    let spanned_tokens = match crate::rade::lexer::Scanner::scan_spanned(source) {
        Ok(toks) => toks,
        Err(lex_err) => {
            diagnostics.push(builder.from_rade_lexer_error(&lex_err));
            return diagnostics;
        }
    };

    // 2. Validate template control flow / directive structure with Rade compiler
    if let Err(compile_err) = crate::rade::compile(source) {
        diagnostics.push(builder.from_rade_compile_error(&compile_err));
        if diagnostics.len() >= max_errors {
            return diagnostics;
        }
    }

    // 3. Lint native RHP blocks (@rhp ... @endrhp) and echo expressions inside the template
    for spanned in &spanned_tokens {
        if diagnostics.len() >= max_errors {
            break;
        }

        match &spanned.token {
            crate::rade::lexer::Token::RawRhp(inner_code) => {
                check_inner_rhp_block(
                    inner_code,
                    &source_file,
                    spanned.line,
                    spanned.column,
                    file_name,
                    max_errors,
                    &builder,
                    &mut diagnostics,
                );
            }
            crate::rade::lexer::Token::EchoEscaped(expr) | crate::rade::lexer::Token::EchoRaw(expr) => {
                check_inner_rhp_expr(
                    expr,
                    &source_file,
                    spanned.line,
                    spanned.column,
                    file_name,
                    &builder,
                    &mut diagnostics,
                );
            }
            _ => {}
        }
    }

    diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.col, d.span.start.byte));
    if diagnostics.len() > max_errors {
        diagnostics.truncate(max_errors);
    }
    diagnostics
}

fn check_inner_rhp_block(
    inner_code: &str,
    source_file: &SourceFile,
    block_line: usize,
    block_col: usize,
    file_name: &str,
    max_errors: usize,
    builder: &DiagnosticBuilder,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let trimmed = inner_code.trim();
    if trimmed.is_empty() {
        return;
    }

    let leading_newlines = inner_code
        .chars()
        .take_while(|c| c.is_whitespace())
        .filter(|&c| c == '\n')
        .count();
    let code_line = block_line + leading_newlines;
    let code_col = if leading_newlines == 0 {
        block_col + 4
    } else {
        if let Some(first_non_ws) = inner_code.lines().nth(leading_newlines) {
            first_non_ws.chars().take_while(|c| c.is_whitespace()).count() + 1
        } else {
            1
        }
    };

    // 1. Lexing phase
    let tokens = match Lexer::tokenize_with_file_id(trimmed, 0) {
        Ok(toks) => toks,
        Err(lex_err) => {
            let mut diag = builder.from_lex_error(&lex_err);
            adjust_diagnostic_span(&mut diag, code_line, code_col, source_file);
            diagnostics.push(diag);
            return;
        }
    };

    // 2. Parsing phase
    let mut parser = Parser::new(tokens, 0).with_file_path(file_name);
    let parsed_program = match parser.parse_program() {
        Ok(prog) => Some(prog),
        Err(parse_err) => {
            let mut diag = builder.from_parse_error(&parse_err);
            adjust_diagnostic_span(&mut diag, code_line, code_col, source_file);
            diagnostics.push(diag);
            None
        }
    };

    // 3. Typecheck & boundary verification phase
    if parsed_program.is_some() {
        let issues = run_typecheck(trimmed, file_name);
        for issue in issues {
            if diagnostics.len() >= max_errors {
                break;
            }
            let mut diag = builder.from_issue(&issue);
            adjust_diagnostic_span(&mut diag, code_line, code_col, source_file);
            diagnostics.push(diag);
        }
    }
}

fn check_inner_rhp_expr(
    expr: &str,
    source_file: &SourceFile,
    start_line: usize,
    start_col: usize,
    file_name: &str,
    builder: &DiagnosticBuilder,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return;
    }

    let stmt_code = format!("{};", trimmed);
    if let Ok(tokens) = Lexer::tokenize_with_file_id(&stmt_code, 0) {
        let mut parser = Parser::new(tokens, 0).with_file_path(file_name);
        if let Err(parse_err) = parser.parse_stmt() {
            let mut diag = builder.from_parse_error(&parse_err);
            adjust_diagnostic_span(&mut diag, start_line, start_col + 2, source_file);
            diagnostics.push(diag);
        }
    }
}

fn adjust_diagnostic_span(
    diag: &mut Diagnostic,
    start_line: usize,
    start_col: usize,
    source_file: &SourceFile,
) {
    let err_line = if diag.span.start.line == 1 {
        diag.span.start.col = diag.span.start.col.saturating_add(start_col.saturating_sub(1));
        start_line
    } else {
        start_line + diag.span.start.line - 1
    };
    diag.span.start.line = err_line;

    if diag.span.end.line == 1 {
        diag.span.end.col = diag.span.end.col.saturating_add(start_col.saturating_sub(1));
        diag.span.end.line = start_line;
    } else {
        diag.span.end.line = start_line + diag.span.end.line - 1;
    }

    if let Some(line_byte) = source_file.line_start_offset(diag.span.start.line) {
        diag.span.start.byte = line_byte + diag.span.start.col.saturating_sub(1);
    }
    if let Some(line_byte) = source_file.line_start_offset(diag.span.end.line) {
        diag.span.end.byte = line_byte + diag.span.end.col.saturating_sub(1);
    }

    for label in &mut diag.labels {
        if label.span.line == 1 {
            label.span.col = label.span.col.saturating_add(start_col.saturating_sub(1));
            label.span.line = start_line;
        } else {
            label.span.line = start_line + label.span.line - 1;
        }
        if let Some(line_byte) = source_file.line_start_offset(label.span.line) {
            label.span.byte = Some(line_byte + label.span.col.saturating_sub(1));
        }
    }

    let total_lines = source_file.line_count();
    let context_start = err_line.saturating_sub(1).max(1);
    let context_end = (err_line + 1).min(total_lines);

    let mut lines = Vec::new();
    for line_num in context_start..=context_end {
        if let Some(text) = source_file.get_line(line_num) {
            let marker = if line_num == err_line {
                Some(super::SnippetMarker {
                    col: diag.span.start.col,
                    len: diag.span.end.col.saturating_sub(diag.span.start.col).max(1),
                })
            } else {
                None
            };
            lines.push(super::SnippetLine {
                line: line_num,
                text: text.to_string(),
                marker,
            });
        }
    }
    diag.snippet = lines;
    diag.rendered = Some(super::render::render_human(diag));
}
