use std::fmt;

/// Exact source span tracking attached to tokens and AST nodes.
///
/// Complies with the specification requirement:
/// `Span { file_id, start, end, line, col }`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    /// Identifier of the source file.
    pub file_id: usize,
    /// Byte offset where the span starts (inclusive).
    pub start: usize,
    /// Byte offset where the span ends (exclusive).
    pub end: usize,
    /// 1-based line number where the span starts.
    pub line: usize,
    /// 1-based column number where the span starts (in UTF-8 characters).
    pub col: usize,
}

impl Span {
    /// Create a new Span with file_id, byte range, line, and column.
    #[inline]
    pub fn new(file_id: usize, start: usize, end: usize, line: usize, col: usize) -> Self {
        Self {
            file_id,
            start,
            end,
            line,
            col,
        }
    }

    /// Return the length in bytes of the span.
    #[inline]
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Return true if the span is zero-width.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Merge this span with another span to form an enclosing span.
    pub fn merge(self, other: Span) -> Span {
        if self.file_id != other.file_id {
            return self;
        }
        let start = self.start.min(other.start);
        let end = self.end.max(other.end);
        let (line, col) = if self.start <= other.start {
            (self.line, self.col)
        } else {
            (other.line, other.col)
        };
        Span {
            file_id: self.file_id,
            start,
            end,
            line,
            col,
        }
    }

    /// Checks if a byte offset is contained within this span.
    #[inline]
    pub fn contains_offset(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}-{}:{}",
            self.file_id, self.line, self.col, self.start, self.end
        )
    }
}

/// Represents a source file in the compilation engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub id: usize,
    pub name: String,
    pub source: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    pub fn new(id: usize, name: impl Into<String>, source: impl Into<String>) -> Self {
        let name = name.into();
        let source = source.into();
        let mut line_starts = vec![0];
        let bytes = source.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\r' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    line_starts.push(i + 2);
                    i += 2;
                    continue;
                } else {
                    line_starts.push(i + 1);
                }
            } else if bytes[i] == b'\n' {
                line_starts.push(i + 1);
            }
            i += 1;
        }
        Self {
            id,
            name,
            source,
            line_starts,
        }
    }

    /// Resolve a byte offset to a 1-based (line, column).
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.source.len());
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        let line = line_idx + 1;
        // Count characters from line_start to offset for column
        let col = self.source[line_start..offset].chars().count() + 1;
        (line, col)
    }

    /// Resolve a byte offset to a 1-based (line, column in UTF-16 code units).
    /// Aligns with the Language Server Protocol (LSP) character position specification.
    pub fn line_col_utf16(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.source.len());
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        let line = line_idx + 1;
        let col = self.source[line_start..offset].encode_utf16().count() + 1;
        (line, col)
    }

    /// Total number of lines in this source file.
    #[inline]
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Return the starting byte offset of line `line` (1-based).
    pub fn line_start_offset(&self, line: usize) -> Option<usize> {
        if line == 0 || line > self.line_starts.len() {
            None
        } else {
            Some(self.line_starts[line - 1])
        }
    }

    /// Construct a Span for a byte range.
    pub fn span_for_range(&self, start: usize, end: usize) -> Span {
        let (line, col) = self.line_col(start);
        Span::new(self.id, start, end, line, col)
    }

    /// Get the text of a specific 1-based line (without trailing newline).
    pub fn get_line(&self, line: usize) -> Option<&str> {
        if line == 0 || line > self.line_starts.len() {
            return None;
        }
        let start = self.line_starts[line - 1];
        let end = if line < self.line_starts.len() {
            let next_start = self.line_starts[line];
            if next_start > 0 && self.source.as_bytes().get(next_start - 1) == Some(&b'\n') {
                if next_start > 1 && self.source.as_bytes().get(next_start - 2) == Some(&b'\r') {
                    next_start - 2
                } else {
                    next_start - 1
                }
            } else {
                next_start
            }
        } else {
            self.source.len()
        };
        Some(&self.source[start..end])
    }
}

/// Global or engine-level source map storing all compiled source files.
#[derive(Debug, Default, Clone)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    /// Add a new source file to the source map, returning its unique file_id.
    pub fn add_file(&mut self, name: impl Into<String>, source: impl Into<String>) -> usize {
        let id = self.files.len();
        self.files.push(SourceFile::new(id, name, source));
        id
    }

    /// Retrieve a reference to a source file by its id.
    pub fn get_file(&self, file_id: usize) -> Option<&SourceFile> {
        self.files.get(file_id)
    }

    /// Resolve a span for a file and byte range.
    pub fn lookup_span(&self, file_id: usize, start: usize, end: usize) -> Option<Span> {
        self.get_file(file_id).map(|f| f.span_for_range(start, end))
    }

    /// Format a diagnostic message with context lines and visual span underline.
    pub fn format_diagnostic(&self, span: &Span, message: &str) -> String {
        let file = match self.get_file(span.file_id) {
            Some(f) => f,
            None => return format!("error: {} at {}", message, span),
        };

        let line_text = file.get_line(span.line).unwrap_or("");
        let underline_len = span.len().max(1);
        let padding = " ".repeat(span.col.saturating_sub(1));
        let carets = "^".repeat(underline_len);

        format!(
            "--> {}:{}:{}\n  |\n{:>3} | {}\n  | {}{}\n  = {}",
            file.name, span.line, span.col, span.line, line_text, padding, carets, message
        )
    }
}
