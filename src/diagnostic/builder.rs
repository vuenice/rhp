//! Diagnostic builder converting lexer, parser, type system, and lint errors into structured diagnostics.

use crate::lexer::{error::SpannedLexError, span::SourceFile, Span};
use crate::lint::reporter::{Issue, IssueLevel};
use crate::parser::error::{ParseError, ParseErrorKind};
use crate::type_system::error::{TypeError, TypeErrorKind};

use super::{
    render::render_human, Applicability, Diagnostic, DiagnosticEdit, DiagnosticLabel,
    DiagnosticLevel, DiagnosticSpan, DiagnosticSuggestion, EditPosition, LabelSpan, Position,
    SnippetLine, SnippetMarker,
};

/// Builder for constructing structured diagnostics with source map context.
pub struct DiagnosticBuilder<'a> {
    file: &'a SourceFile,
}

impl<'a> DiagnosticBuilder<'a> {
    pub fn new(file: &'a SourceFile) -> Self {
        Self { file }
    }

    /// Convert a 0-based byte offset into a 1-based `Position` with UTF-16 column.
    pub fn position_at(&self, offset: usize) -> Position {
        let (line, col) = self.file.line_col_utf16(offset);
        Position::new(line, col, offset.min(self.file.source.len()))
    }

    /// Construct a `DiagnosticSpan` from a raw `Span`.
    pub fn diagnostic_span(&self, span: &Span) -> DiagnosticSpan {
        DiagnosticSpan {
            file: self.file.name.clone(),
            start: self.position_at(span.start),
            end: self.position_at(span.end.max(span.start + 1)),
        }
    }

    /// Extract contextual snippet lines (~2 lines) surrounding the error.
    pub fn extract_snippet(&self, span: &Span) -> Vec<SnippetLine> {
        let (err_line, _) = self.file.line_col(span.start);
        let start_pos = self.position_at(span.start);

        let total_lines = self.file.line_count();
        let context_start = err_line.saturating_sub(1).max(1);
        let context_end = (err_line + 1).min(total_lines);

        let mut lines = Vec::new();
        for line_num in context_start..=context_end {
            if let Some(text) = self.file.get_line(line_num) {
                let marker = if line_num == err_line {
                    let len = span.len().max(1);
                    Some(SnippetMarker {
                        col: start_pos.col,
                        len,
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
    }

    /// Convert a `ParseError` into a structured `Diagnostic`.
    pub fn from_parse_error(&self, err: &ParseError) -> Diagnostic {
        let span = self.diagnostic_span(&err.span);
        let snippet = self.extract_snippet(&err.span);

        let (code, message, found, expected) = match &err.kind {
            ParseErrorKind::UnexpectedToken { expected, found } => (
                "RHP0102".to_string(),
                format!("unexpected token `{}`, expected {}", found, expected),
                err.found_token.clone().or_else(|| Some(found.clone())),
                err.expected_tokens.clone().or_else(|| {
                    if expected == "expression" {
                        Some(vec![
                            "identifier".to_string(),
                            "number".to_string(),
                            "string".to_string(),
                            "`(`".to_string(),
                        ])
                    } else {
                        Some(vec![expected.clone()])
                    }
                }),
            ),
            ParseErrorKind::UnexpectedEof { expected } => (
                "RHP0101".to_string(),
                format!("unexpected end of input: expected {}", expected),
                Some("<EOF>".to_string()),
                Some(vec![expected.clone()]),
            ),
            ParseErrorKind::InvalidAssignmentTarget => (
                "RHP0105".to_string(),
                "invalid assignment target: left-hand side is not assignable".to_string(),
                None,
                None,
            ),
            ParseErrorKind::StaticFacadeInstantiationForbidden { class_name } => (
                "RHP0110".to_string(),
                format!(
                    "cannot instantiate static facade class '{}'. Use static methods directly.",
                    class_name
                ),
                None,
                None,
            ),
            ParseErrorKind::StaticFacadeInheritanceForbidden { class_name } => (
                "RHP0111".to_string(),
                format!(
                    "cannot extend static facade class '{}'. Use '{}::macro()' to register custom methods dynamically.",
                    class_name, class_name
                ),
                None,
                None,
            ),
            ParseErrorKind::InvalidFacadeMacroRegistration { class_name, reason } => (
                "RHP0112".to_string(),
                format!("invalid macro registration on facade '{}': {}", class_name, reason),
                None,
                None,
            ),
            ParseErrorKind::Custom { message } => (
                "RHP0106".to_string(),
                format!("syntax error: {}", message),
                None,
                None,
            ),
            ParseErrorKind::Lex(lex_err) => match lex_err {
                crate::lexer::error::LexError::UnterminatedString => (
                    "RHP0002".to_string(),
                    "unclosed string literal".to_string(),
                    None,
                    Some(vec!["'\"'".to_string()]),
                ),
                crate::lexer::error::LexError::InvalidNumericLiteral(msg) => (
                    "RHP0003".to_string(),
                    format!("invalid numeric literal: {}", msg),
                    None,
                    None,
                ),
                _ => (
                    "RHP0001".to_string(),
                    format!("unexpected character or token: {}", lex_err),
                    None,
                    None,
                ),
            },
        };

        // Labels: Primary label at error location, secondary label at open delimiter if any
        let mut labels = Vec::new();
        labels.push(DiagnosticLabel {
            span: LabelSpan::with_byte(span.start.line, span.start.col, span.start.byte),
            text: if let Some(ref f) = found {
                if f == ")" || f == "]" || f == "}" {
                    "expected expression here".to_string()
                } else {
                    format!("unexpected `{}`", f)
                }
            } else {
                "error occurred here".to_string()
            },
            primary: true,
        });

        if let Some((sec_span, ref text)) = err.secondary_label {
            let sec_pos = self.position_at(sec_span.start);
            labels.push(DiagnosticLabel {
                span: LabelSpan::with_byte(sec_pos.line, sec_pos.col, sec_pos.byte),
                text: text.clone(),
                primary: false,
            });
        }

        // Suggestions: Trailing comma removal detection
        let mut suggestions = Vec::new();
        if let Some(ref f) = found {
            if f == ")" || f == "]" {
                // Check if preceded by a comma
                let bytes = self.file.source.as_bytes();
                let mut cursor = err.span.start.saturating_sub(1);
                // Skip backwards over spaces/tabs
                while cursor > 0 && (bytes[cursor] == b' ' || bytes[cursor] == b'\t') {
                    cursor -= 1;
                }
                if bytes.get(cursor) == Some(&b',') {
                    let comma_start = cursor;
                    let comma_pos = self.position_at(comma_start);
                    let replace_end_pos = self.position_at(err.span.start);
                    suggestions.push(DiagnosticSuggestion {
                        message: "remove the trailing comma".to_string(),
                        applicability: Applicability::MachineApplicable,
                        edits: vec![DiagnosticEdit {
                            start: EditPosition::with_byte(
                                comma_pos.line,
                                comma_pos.col,
                                comma_pos.byte,
                            ),
                            end: EditPosition::with_byte(
                                replace_end_pos.line,
                                replace_end_pos.col,
                                replace_end_pos.byte,
                            ),
                            replacement: String::new(),
                        }],
                    });
                }
            }
        }

        let explain = format!("rhp explain {}", code);

        let mut d = Diagnostic {
            level: DiagnosticLevel::Error,
            code,
            message,
            span,
            found,
            expected,
            labels,
            snippet,
            suggestions,
            explain,
            rendered: None,
            caused_by: None,
        };

        d.rendered = Some(render_human(&d));
        d
    }

    /// Convert a `TypeError` into a structured `Diagnostic`.
    pub fn from_type_error(&self, err: &TypeError) -> Diagnostic {
        let span = self.diagnostic_span(&err.span);
        let snippet = self.extract_snippet(&err.span);

        let (code, message) = type_error_to_code_and_message(&err.kind);

        let mut labels = vec![DiagnosticLabel {
            span: LabelSpan::with_byte(span.start.line, span.start.col, span.start.byte),
            text: err.kind.to_string(),
            primary: true,
        }];

        if let Some((sec_span, ref text)) = err.secondary_label {
            let sec_pos = self.position_at(sec_span.start);
            labels.push(DiagnosticLabel {
                span: LabelSpan::with_byte(sec_pos.line, sec_pos.col, sec_pos.byte),
                text: text.clone(),
                primary: false,
            });
        }

        let mut suggestions = Vec::new();
        match &err.kind {
            TypeErrorKind::MissingReturnType { .. } => {
                suggestions.push(DiagnosticSuggestion {
                    message: "add an explicit return type annotation (e.g. ': void' or ': mixed')".to_string(),
                    applicability: Applicability::HasPlaceholders,
                    edits: vec![DiagnosticEdit {
                        start: EditPosition::with_byte(span.end.line, span.end.col, span.end.byte),
                        end: EditPosition::with_byte(span.end.line, span.end.col, span.end.byte),
                        replacement: ": mixed".to_string(),
                    }],
                });
            }
            TypeErrorKind::MissingParameterType { param_name, .. } => {
                suggestions.push(DiagnosticSuggestion {
                    message: format!("add an explicit type hint to parameter '{}'", param_name),
                    applicability: Applicability::HasPlaceholders,
                    edits: vec![],
                });
            }
            TypeErrorKind::PassByReferenceForbidden { .. } => {
                suggestions.push(DiagnosticSuggestion {
                    message: "remove '&' and return the updated value or use a mutable collection wrapper".to_string(),
                    applicability: Applicability::MaybeIncorrect,
                    edits: vec![],
                });
            }
            _ => {}
        }

        let explain = format!("rhp explain {}", code);

        let mut d = Diagnostic {
            level: DiagnosticLevel::Error,
            code,
            message,
            span,
            found: None,
            expected: None,
            labels,
            snippet,
            suggestions,
            explain,
            rendered: None,
            caused_by: None,
        };

        d.rendered = Some(render_human(&d));
        d
    }

    /// Convert a `SpannedLexError` into a structured `Diagnostic`.
    pub fn from_lex_error(&self, err: &SpannedLexError) -> Diagnostic {
        let span = self.diagnostic_span(&err.span);
        let snippet = self.extract_snippet(&err.span);

        let (code, message) = match &err.error {
            crate::lexer::error::LexError::UnterminatedString => (
                "RHP0002".to_string(),
                "unclosed string literal".to_string(),
            ),
            crate::lexer::error::LexError::InvalidNumericLiteral(msg) => (
                "RHP0003".to_string(),
                format!("invalid numeric literal: {}", msg),
            ),
            _ => (
                "RHP0001".to_string(),
                format!("unexpected character: {}", err.error),
            ),
        };

        let labels = vec![DiagnosticLabel {
            span: LabelSpan::with_byte(span.start.line, span.start.col, span.start.byte),
            text: message.clone(),
            primary: true,
        }];

        let explain = format!("rhp explain {}", code);

        let mut d = Diagnostic {
            level: DiagnosticLevel::Error,
            code,
            message,
            span,
            found: None,
            expected: None,
            labels,
            snippet,
            suggestions: Vec::new(),
            explain,
            rendered: None,
            caused_by: None,
        };

        d.rendered = Some(render_human(&d));
        d
    }

    /// Convert a lint `Issue` into a structured `Diagnostic`.
    pub fn from_issue(&self, issue: &Issue) -> Diagnostic {
        let span_obj = Span::new(
            0,
            issue.span.start,
            issue.span.end,
            issue.line,
            issue.column,
        );
        let span = self.diagnostic_span(&span_obj);
        let snippet = self.extract_snippet(&span_obj);

        let level = match issue.level {
            IssueLevel::Error => DiagnosticLevel::Error,
            IssueLevel::Warning => DiagnosticLevel::Warning,
            IssueLevel::Note => DiagnosticLevel::Note,
            IssueLevel::Help => DiagnosticLevel::Help,
        };

        let code = map_lint_code_to_rhp(&issue.code);

        let labels = vec![DiagnosticLabel {
            span: LabelSpan::with_byte(span.start.line, span.start.col, span.start.byte),
            text: issue.message.clone(),
            primary: true,
        }];

        let mut suggestions = Vec::new();
        if issue.code == "deprecated-superglobal" || code == "RHP0401" {
            suggestions.push(DiagnosticSuggestion {
                message: "use 'Request::get()' or 'Session::get()' static facade methods instead".to_string(),
                applicability: Applicability::MaybeIncorrect,
                edits: vec![],
            });
        }

        let explain = format!("rhp explain {}", code);

        let mut d = Diagnostic {
            level,
            code,
            message: issue.message.clone(),
            span,
            found: None,
            expected: None,
            labels,
            snippet,
            suggestions,
            explain,
            rendered: None,
            caused_by: None,
        };

        d.rendered = Some(render_human(&d));
        d
    }

    /// Construct an internal compiler error diagnostic.
    pub fn internal_error(file_name: impl Into<String>, message: impl Into<String>) -> Diagnostic {
        let file = file_name.into();
        let msg = message.into();
        let span = DiagnosticSpan {
            file,
            start: Position::new(1, 1, 0),
            end: Position::new(1, 1, 0),
        };

        let mut d = Diagnostic {
            level: DiagnosticLevel::InternalError,
            code: "RHP9999".to_string(),
            message: format!("internal compiler error: {}", msg),
            span,
            found: None,
            expected: None,
            labels: Vec::new(),
            snippet: Vec::new(),
            suggestions: Vec::new(),
            explain: "rhp explain RHP9999".to_string(),
            rendered: None,
            caused_by: None,
        };

        d.rendered = Some(render_human(&d));
        d
    }
}

/// Map `TypeErrorKind` to stable `RHP02xx` / `RHP03xx` code and formatted message.
pub fn type_error_to_code_and_message(kind: &TypeErrorKind) -> (String, String) {
    let code = match kind {
        TypeErrorKind::MissingParameterType { .. } => "RHP0201",
        TypeErrorKind::MissingReturnType { .. } => "RHP0202",
        TypeErrorKind::ReturnTypeMismatch { .. } => "RHP0203",
        TypeErrorKind::ArgumentTypeMismatch { .. } => "RHP0204",
        TypeErrorKind::ArityMismatch { .. } => "RHP0205",
        TypeErrorKind::UnknownNamedArgument { .. } => "RHP0206",
        TypeErrorKind::VariableTypeMismatch { .. } => "RHP0207",
        TypeErrorKind::ParameterDefaultTypeMismatch { .. } => "RHP0208",
        TypeErrorKind::PropertyDefaultTypeMismatch { .. } => "RHP0209",
        TypeErrorKind::PropertyAssignmentTypeMismatch { .. } => "RHP0210",
        TypeErrorKind::ReadonlyPropertyMutation { .. } => "RHP0211",
        TypeErrorKind::VoidReturnWithValue { .. } => "RHP0212",
        TypeErrorKind::NeverFunctionReturns { .. } => "RHP0213",
        TypeErrorKind::MissingReturnStatement { .. } => "RHP0214",
        TypeErrorKind::InvalidBinaryOperation { .. } => "RHP0215",
        TypeErrorKind::InvalidUnaryOperation { .. } => "RHP0216",
        TypeErrorKind::InvalidTypeAnnotation { .. } => "RHP0217",
        TypeErrorKind::PassByReferenceForbidden { .. } => "RHP0220",
        TypeErrorKind::MissingReferenceArgument { .. } => "RHP0221",
        TypeErrorKind::UnexpectedReferenceArgument { .. } => "RHP0222",
        TypeErrorKind::ReferenceArgumentMustBeVariable { .. } => "RHP0223",
        TypeErrorKind::ClosureReferenceCaptureForbidden { .. } => "RHP0224",
        TypeErrorKind::CyclicClosureCaptureForbidden { .. } => "RHP0225",
        TypeErrorKind::CyclicReferenceRisk { .. } => "RHP0226",
        TypeErrorKind::InterfaceContractViolation { .. } => "RHP0301",
        TypeErrorKind::MethodOverrideContractViolation { .. } => "RHP0302",
        TypeErrorKind::ConstructorReturnTypeForbidden { .. } => "RHP0303",
        TypeErrorKind::TraitMethodCollision { .. } => "RHP0304",
        TypeErrorKind::TraitAdaptationError { .. } => "RHP0305",
        TypeErrorKind::StaticFacadeInstantiationForbidden { .. } => "RHP0110",
        TypeErrorKind::StaticFacadeInheritanceForbidden { .. } => "RHP0111",
        TypeErrorKind::NonStaticSingletonMethodCallForbidden { .. } => "RHP0114",
        TypeErrorKind::SingletonInstantiationForbidden { .. } => "RHP0113",
        TypeErrorKind::NonSingletonExtendsSingletonForbidden { .. } => "RHP0115",
    };

    (code.to_string(), kind.to_string())
}

/// Map string lint issue code to stable `RHPxxxx` code.
pub fn map_lint_code_to_rhp(code: &str) -> String {
    match code {
        "parse-error" => "RHP0102".to_string(),
        "missing-parameter-type" => "RHP0201".to_string(),
        "missing-return-type" => "RHP0202".to_string(),
        "return-type-mismatch" => "RHP0203".to_string(),
        "argument-type-mismatch" => "RHP0204".to_string(),
        "arity-mismatch" => "RHP0205".to_string(),
        "unknown-named-argument" => "RHP0206".to_string(),
        "variable-type-mismatch" => "RHP0207".to_string(),
        "parameter-default-type-mismatch" => "RHP0208".to_string(),
        "property-default-type-mismatch" => "RHP0209".to_string(),
        "property-assignment-type-mismatch" => "RHP0210".to_string(),
        "readonly-property-mutation" => "RHP0211".to_string(),
        "void-return-with-value" => "RHP0212".to_string(),
        "never-function-returns" => "RHP0213".to_string(),
        "missing-return-statement" => "RHP0214".to_string(),
        "invalid-binary-operation" => "RHP0215".to_string(),
        "invalid-unary-operation" => "RHP0216".to_string(),
        "invalid-type-annotation" => "RHP0217".to_string(),
        "pass-by-reference-forbidden" => "RHP0220".to_string(),
        "interface-contract-violation" => "RHP0301".to_string(),
        "method-override-contract-violation" => "RHP0302".to_string(),
        "constructor-return-type-forbidden" => "RHP0303".to_string(),
        "trait-method-collision" => "RHP0304".to_string(),
        "trait-adaptation-error" => "RHP0305".to_string(),
        "static-facade-instantiation-forbidden" => "RHP0110".to_string(),
        "static-facade-inheritance-forbidden" => "RHP0111".to_string(),
        "deprecated-superglobal" => "RHP0401".to_string(),
        "naming-casing" => "RHP0402".to_string(),
        c if c.starts_with("RHP") => c.to_string(),
        _ => "RHP0106".to_string(),
    }
}
