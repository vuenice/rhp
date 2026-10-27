//! Single structured diagnostic as the single source of truth for all RHP compiler errors,
//! warnings, and internal compiler failures.
//!
//! Diagnostics can be rendered two ways:
//! 1. Human-readable text by default (Rust / Elm style).
//! 2. NDJSON stream (one JSON object per line) behind `--format=json`, terminating with a summary object.
//! 3. Short NDJSON behind `--format=json-short` (omits `rendered` and `labels` to minimize agent context tokens).

pub mod builder;
pub mod check;
pub mod codes;
pub mod render;

pub use builder::*;
pub use check::*;
pub use codes::*;
pub use render::render_human;

use serde::{Deserialize, Serialize};

/// Severity tier of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    /// User code is syntactically or semantically invalid.
    Error,
    /// Deprecated syntax or best-practice convention warning.
    Warning,
    /// Compiler bug or internal panic (distinct from user syntax errors).
    InternalError,
    /// Informational note.
    Note,
    /// Actionable suggestion or guidance.
    Help,
}

impl DiagnosticLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::InternalError => "internal_error",
            Self::Note => "note",
            Self::Help => "help",
        }
    }
}

/// Precise position in a source file.
/// Column numbers are 1-based UTF-16 code units, aligning directly with Language Server Protocol (LSP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number in UTF-16 code units.
    pub col: usize,
    /// 0-based byte offset in the source file.
    pub byte: usize,
}

impl Position {
    #[inline]
    pub fn new(line: usize, col: usize, byte: usize) -> Self {
        Self { line, col, byte }
    }
}

/// Source span encompassing a start and end position within a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSpan {
    pub file: String,
    pub start: Position,
    pub end: Position,
}

/// Span location for an inline label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelSpan {
    pub line: usize,
    pub col: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte: Option<usize>,
}

impl LabelSpan {
    #[inline]
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col, byte: None }
    }

    #[inline]
    pub fn with_byte(line: usize, col: usize, byte: usize) -> Self {
        Self { line, col, byte: Some(byte) }
    }
}

/// Visual label attached to a primary or secondary span location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticLabel {
    pub span: LabelSpan,
    pub text: String,
    pub primary: bool,
}

/// Column marker indicating the exact span of the error in snippet lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetMarker {
    pub col: usize,
    pub len: usize,
}

/// A line of source code context within a snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetLine {
    pub line: usize,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<SnippetMarker>,
}

/// Applicability tier for automatic fixes (matching rustc's convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    /// Can be applied automatically by tools and agents without developer review.
    MachineApplicable,
    /// May change semantics or require developer review.
    MaybeIncorrect,
    /// Contains placeholder tokens that must be filled in by developer.
    HasPlaceholders,
}

/// Edit position for code suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditPosition {
    pub line: usize,
    pub col: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte: Option<usize>,
}

impl EditPosition {
    #[inline]
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col, byte: None }
    }

    #[inline]
    pub fn with_byte(line: usize, col: usize, byte: usize) -> Self {
        Self { line, col, byte: Some(byte) }
    }
}

/// Concrete text replacement within a source span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticEdit {
    pub start: EditPosition,
    pub end: EditPosition,
    pub replacement: String,
}

/// Machine-applicable or suggested fix for the diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSuggestion {
    pub message: String,
    pub applicability: Applicability,
    pub edits: Vec<DiagnosticEdit>,
}

/// Single structured diagnostic — the source of truth for all compiler errors and warnings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Severity level ('error', 'warning', 'internal_error').
    pub level: DiagnosticLevel,
    /// Stable diagnostic code (e.g. 'RHP0102') backed by 'rhp explain <code>'.
    pub code: String,
    /// Primary error message.
    pub message: String,
    /// Primary source span.
    pub span: DiagnosticSpan,
    /// Encountered token on parse error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found: Option<String>,
    /// Expected tokens on parse error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<Vec<String>>,
    /// Primary and secondary annotated labels.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<DiagnosticLabel>,
    /// Contextual lines of source code surrounding the error (~2 lines).
    pub snippet: Vec<SnippetLine>,
    /// Machine-applicable or guidance suggestions.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<DiagnosticSuggestion>,
    /// Explain command to fetch detailed documentation.
    pub explain: String,
    /// Human-rendered Elm/Rust-style text representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered: Option<String>,
    /// Optional parent diagnostic for cascading errors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caused_by: Option<Box<Diagnostic>>,
}

impl Diagnostic {
    /// Return a token-budget-optimized copy with `rendered` and `labels` omitted.
    pub fn to_short(&self) -> Self {
        let mut short = self.clone();
        short.rendered = None;
        short.labels.clear();
        short
    }

    /// Serialize to single-line NDJSON format.
    pub fn to_ndjson(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Render human-readable Rust/Elm-style terminal output.
    pub fn render_human(&self) -> String {
        if let Some(ref r) = self.rendered {
            r.clone()
        } else {
            render_human(self)
        }
    }
}

/// Summary object emitted at the end of NDJSON diagnostic streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSummary {
    pub level: String,
    pub errors: usize,
    pub warnings: usize,
    pub status: String,
}

impl DiagnosticSummary {
    pub fn new(errors: usize, warnings: usize) -> Self {
        Self {
            level: "summary".to_string(),
            errors,
            warnings,
            status: if errors > 0 { "error".to_string() } else { "ok".to_string() },
        }
    }

    pub fn to_ndjson(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Output format for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiagnosticFormat {
    /// Rust/Elm-style human-readable terminal output.
    #[default]
    Human,
    /// Full NDJSON stream with rendered snippets and labels.
    Json,
    /// Minimized NDJSON stream omitting rendered strings and labels for agent token budget.
    JsonShort,
}

impl DiagnosticFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "human" | "text" | "plain" => Some(Self::Human),
            "json" => Some(Self::Json),
            "json-short" | "json_short" | "short" => Some(Self::JsonShort),
            _ => None,
        }
    }
}

/// Diagnostic stream emitter that handles rendering and final summary emission.
pub struct DiagnosticEmitter {
    pub format: DiagnosticFormat,
    pub diagnostics: Vec<Diagnostic>,
    pub max_errors_per_file: usize,
}

impl DiagnosticEmitter {
    pub fn new(format: DiagnosticFormat) -> Self {
        Self {
            format,
            diagnostics: Vec::new(),
            max_errors_per_file: 5,
        }
    }

    /// Emit a single diagnostic to stdout / stderr based on the format.
    pub fn emit(&mut self, d: Diagnostic) {
        match self.format {
            DiagnosticFormat::Human => {
                eprintln!("{}", d.render_human());
            }
            DiagnosticFormat::Json => {
                println!("{}", d.to_ndjson());
            }
            DiagnosticFormat::JsonShort => {
                println!("{}", d.to_short().to_ndjson());
            }
        }
        self.diagnostics.push(d);
    }

    /// Total errors recorded so far.
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| matches!(d.level, DiagnosticLevel::Error | DiagnosticLevel::InternalError))
            .count()
    }

    /// Total warnings recorded so far.
    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.level == DiagnosticLevel::Warning)
            .count()
    }

    /// Total internal compiler errors recorded so far.
    pub fn internal_error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.level == DiagnosticLevel::InternalError)
            .count()
    }

    /// Emit the final summary object and return process exit code:
    /// - 0: Success (no errors)
    /// - 1: Code errors (syntax or type errors detected in user code)
    /// - 2: Compiler crashed / internal compiler error
    pub fn finish(&self) -> i32 {
        let errors = self.error_count();
        let warnings = self.warning_count();
        let internal_errors = self.internal_error_count();

        match self.format {
            DiagnosticFormat::Json | DiagnosticFormat::JsonShort => {
                let summary = DiagnosticSummary::new(errors, warnings);
                println!("{}", summary.to_ndjson());
            }
            DiagnosticFormat::Human => {
                if errors > 0 {
                    if errors == 1 {
                        eprintln!("error: could not compile due to 1 previous error");
                    } else {
                        eprintln!("error: could not compile due to {} previous errors", errors);
                    }
                } else if warnings > 0 {
                    if warnings == 1 {
                        eprintln!("warning: 1 warning emitted");
                    } else {
                        eprintln!("warning: {} warnings emitted", warnings);
                    }
                }
            }
        }

        if internal_errors > 0 {
            2
        } else if errors > 0 {
            1
        } else {
            0
        }
    }
}
