use std::fmt;

/// Severity level of a diagnostic issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueLevel {
    Error,
    Warning,
    Note,
    Help,
}

impl IssueLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
            Self::Help => "help",
        }
    }
}

impl fmt::Display for IssueLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Source byte range of a diagnostic issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IssueSpan {
    pub start: usize,
    pub end: usize,
}

/// A diagnostic issue discovered during linting or type checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub level: IssueLevel,
    pub code: String,
    pub message: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub span: IssueSpan,
}

impl Issue {
    pub fn new(
        level: IssueLevel,
        code: impl Into<String>,
        message: impl Into<String>,
        file: impl Into<String>,
        line: usize,
        column: usize,
        start: usize,
        end: usize,
    ) -> Self {
        Self {
            level,
            code: code.into(),
            message: message.into(),
            file: file.into(),
            line,
            column,
            span: IssueSpan { start, end },
        }
    }

    /// Escape string for JSON serialization.
    fn escape_json(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 8);
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{08}' => out.push_str("\\b"),
                '\u{0C}' => out.push_str("\\f"),
                c if (c as u32) < 0x20 => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => out.push(c),
            }
        }
        out
    }

    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "    {{\n",
                "      \"level\": \"{}\",\n",
                "      \"code\": \"{}\",\n",
                "      \"message\": \"{}\",\n",
                "      \"file\": \"{}\",\n",
                "      \"line\": {},\n",
                "      \"column\": {},\n",
                "      \"span\": {{\n",
                "        \"start\": {},\n",
                "        \"end\": {}\n",
                "      }}\n",
                "    }}"
            ),
            self.level.as_str(),
            Self::escape_json(&self.code),
            Self::escape_json(&self.message),
            Self::escape_json(&self.file),
            self.line,
            self.column,
            self.span.start,
            self.span.end
        )
    }
}

/// Aggregated report of a lint run across one or more files.
#[derive(Debug, Clone, Default)]
pub struct LintReport {
    pub files_scanned: usize,
    pub files_modified: usize,
    pub issues: Vec<Issue>,
}

impl LintReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_issue(&mut self, issue: Issue) {
        self.issues.push(issue);
    }

    pub fn total_issues(&self) -> usize {
        self.issues.len()
    }

    pub fn errors_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.level == IssueLevel::Error)
            .count()
    }

    pub fn warnings_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.level == IssueLevel::Warning)
            .count()
    }

    pub fn status(&self) -> &'static str {
        if self.errors_count() > 0 {
            "error"
        } else {
            "ok"
        }
    }

    /// Convert report to formatted JSON output.
    pub fn to_json(&self) -> String {
        let issues_json = if self.issues.is_empty() {
            "[]".to_string()
        } else {
            let items: Vec<String> = self.issues.iter().map(|i| i.to_json()).collect();
            format!("[\n{}\n  ]", items.join(",\n"))
        };

        format!(
            concat!(
                "{{\n",
                "  \"status\": \"{}\",\n",
                "  \"files_scanned\": {},\n",
                "  \"files_modified\": {},\n",
                "  \"total_issues\": {},\n",
                "  \"errors\": {},\n",
                "  \"warnings\": {},\n",
                "  \"issues\": {}\n",
                "}}"
            ),
            self.status(),
            self.files_scanned,
            self.files_modified,
            self.total_issues(),
            self.errors_count(),
            self.warnings_count(),
            issues_json
        )
    }
}
