use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::diagnostic::{
    Diagnostic, DiagnosticLabel, DiagnosticLevel, DiagnosticSpan, LabelSpan, Position,
    SnippetLine, SnippetMarker,
};
use crate::lexer::span::{SourceFile, Span};

static GLOBAL_GENSYM_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Provenance link connecting generated AST nodes back to the originating expansion site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanChain {
    /// The span of the attribute `#[...]` or directive `@...` that generated this code.
    pub origin_span: Span,
    /// The registered name of the expander (e.g., `fillable`, `if`, `datetime`).
    pub expander_name: String,
}

/// Context provided to expanders during AST transformation.
///
/// Provides hygiene guarantees (unique non-capturing identifiers via `gensym`),
/// string interning for AST lifetime satisfaction, span chaining, and diagnostic emission.
pub struct ExpandCtx {
    source_file: Option<SourceFile>,
    /// Table mapping generated node spans to their originating expansion sites.
    pub span_chains: BTreeMap<Span, SpanChain>,
    /// Accumulated diagnostics emitted during expansion.
    pub diagnostics: Vec<Diagnostic>,
}

impl Default for ExpandCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpandCtx {
    /// Create a new expansion context.
    pub fn new() -> Self {
        Self {
            source_file: None,
            span_chains: BTreeMap::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Create an expansion context seeded with a source file for snippet generation.
    pub fn with_source_file(source_file: SourceFile) -> Self {
        Self {
            source_file: Some(source_file),
            span_chains: BTreeMap::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Set or update the active source file.
    pub fn set_source_file(&mut self, source_file: SourceFile) {
        self.source_file = Some(source_file);
    }

    /// Access the active source file, if available.
    pub fn source_file(&self) -> Option<&SourceFile> {
        self.source_file.as_ref()
    }

    /// Generate a fresh, unique identifier guaranteed not to clash with user variables.
    ///
    /// Prefixed with `__rhp_gen_` and a global atomic sequence.
    pub fn gensym(&mut self, prefix: &str) -> &'static str {
        let id = GLOBAL_GENSYM_COUNTER.fetch_add(1, Ordering::SeqCst);
        let name = format!("__rhp_gen_{}_{}", prefix, id);
        Box::leak(name.into_boxed_str())
    }

    /// Intern a string slice into a `'static` reference for AST node placement.
    pub fn intern(&mut self, s: &str) -> &'static str {
        Box::leak(s.to_string().into_boxed_str())
    }

    /// Register a span chain linking a generated span to its originating expansion site.
    pub fn record_span_chain(&mut self, generated_span: Span, origin_span: Span, expander_name: &str) {
        self.span_chains.insert(
            generated_span,
            SpanChain {
                origin_span,
                expander_name: expander_name.to_string(),
            },
        );
    }

    /// Look up the span chain for a given span.
    pub fn lookup_span_chain(&self, span: &Span) -> Option<&SpanChain> {
        self.span_chains.get(span)
    }

    /// Construct a structured compiler diagnostic with accurate source positions and snippets.
    pub fn build_diagnostic(
        &self,
        level: DiagnosticLevel,
        code: &str,
        message: impl Into<String>,
        span: Span,
        primary_label: Option<String>,
        secondary_labels: Vec<(Span, String)>,
    ) -> Diagnostic {
        let message = message.into();
        let file_name = self
            .source_file
            .as_ref()
            .map(|f| f.name.clone())
            .unwrap_or_else(|| format!("file_{}", span.file_id));

        let start_pos = if let Some(ref file) = self.source_file {
            let (l, c) = file.line_col_utf16(span.start);
            Position::new(l, c, span.start)
        } else {
            Position::new(span.line, span.col, span.start)
        };

        let end_pos = if let Some(ref file) = self.source_file {
            let (l, c) = file.line_col_utf16(span.end.max(span.start + 1));
            Position::new(l, c, span.end.max(span.start + 1))
        } else {
            Position::new(span.line, span.col + span.len().max(1), span.end.max(span.start + 1))
        };

        let mut labels = Vec::new();
        labels.push(DiagnosticLabel {
            span: LabelSpan::with_byte(start_pos.line, start_pos.col, start_pos.byte),
            text: primary_label.unwrap_or_else(|| message.clone()),
            primary: true,
        });

        for (sec_span, sec_text) in secondary_labels {
            let (sec_line, sec_col) = if let Some(ref file) = self.source_file {
                file.line_col_utf16(sec_span.start)
            } else {
                (sec_span.line, sec_span.col)
            };
            labels.push(DiagnosticLabel {
                span: LabelSpan::with_byte(sec_line, sec_col, sec_span.start),
                text: sec_text,
                primary: false,
            });
        }

        let snippet = if let Some(ref file) = self.source_file {
            let (err_line, _) = file.line_col(span.start);
            let total_lines = file.line_count();
            let context_start = err_line.saturating_sub(1).max(1);
            let context_end = (err_line + 1).min(total_lines);
            let mut lines = Vec::new();
            for line_num in context_start..=context_end {
                if let Some(text) = file.get_line(line_num) {
                    let marker = if line_num == err_line {
                        Some(SnippetMarker {
                            col: start_pos.col,
                            len: span.len().max(1),
                        })
                    } else {
                        None
                    };
                    lines.push(SnippetLine {
                        line: line_num,
                        text: text.to_string(),
                        marker,
                    });
                }
            }
            lines
        } else {
            Vec::new()
        };

        let explain = format!("rhp explain {}", code);

        Diagnostic {
            level,
            code: code.to_string(),
            message,
            span: DiagnosticSpan {
                file: file_name,
                start: start_pos,
                end: end_pos,
            },
            found: None,
            expected: None,
            labels,
            snippet,
            suggestions: Vec::new(),
            explain,
            rendered: None,
            caused_by: None,
        }
    }

    /// Emit an error diagnostic and return it.
    pub fn error(&mut self, code: &str, message: impl Into<String>, span: Span) -> Diagnostic {
        let diag = self.build_diagnostic(
            DiagnosticLevel::Error,
            code,
            message,
            span,
            None,
            Vec::new(),
        );
        self.diagnostics.push(diag.clone());
        diag
    }

    /// Emit a diagnostic for an error inside generated code, linking to the originating expander site.
    pub fn error_in_generated_code(
        &mut self,
        code: &str,
        message: impl Into<String>,
        origin_span: Span,
        expander_name: &str,
    ) -> Diagnostic {
        let msg = message.into();
        let diag = self.build_diagnostic(
            DiagnosticLevel::Error,
            code,
            format!("in expansion of `{}`: {}", expander_name, msg),
            origin_span,
            Some(format!("originating `{}` expansion here", expander_name)),
            vec![(
                origin_span,
                format!("derived during expansion of `{}`", expander_name),
            )],
        );
        self.diagnostics.push(diag.clone());
        diag
    }
}
