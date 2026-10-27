//! Human-readable diagnostic renderer (Rust / Elm style).

use std::fmt::Write;

use super::{Diagnostic, DiagnosticLevel};

/// Render a structured diagnostic into rustc/Elm style human-readable terminal output.
pub fn render_human(d: &Diagnostic) -> String {
    let mut out = String::new();

    let level_str = match d.level {
        DiagnosticLevel::Error => "error",
        DiagnosticLevel::Warning => "warning",
        DiagnosticLevel::InternalError => "internal_error",
        DiagnosticLevel::Note => "note",
        DiagnosticLevel::Help => "help",
    };

    // Header line: error[RHP0102]: message
    let _ = writeln!(out, "{}[{}]: {}", level_str, d.code, d.message);

    // Location line:   --> file:line:col
    let _ = writeln!(
        out,
        "  --> {}:{}:{}",
        d.span.file, d.span.start.line, d.span.start.col
    );

    // Internal error notice if applicable
    if d.level == DiagnosticLevel::InternalError {
        let _ = writeln!(out, "   |");
        let _ = writeln!(
            out,
            "  = note: this is a bug in rhp {}",
            env!("CARGO_PKG_VERSION")
        );
        let _ = writeln!(
            out,
            "  = note: please report this issue at https://github.com/vuenice/rhp/issues"
        );
        return out;
    }

    if d.snippet.is_empty() {
        return out;
    }

    // Determine max line number width for gutter alignment
    let max_line = d
        .snippet
        .iter()
        .map(|s| s.line)
        .max()
        .unwrap_or(d.span.start.line);
    let gutter_width = max_line.to_string().len().max(2);

    let _ = writeln!(out, "{:>width$} |", "", width = gutter_width);

    for s in &d.snippet {
        let _ = writeln!(
            out,
            "{:>width$} | {}",
            s.line,
            s.text,
            width = gutter_width
        );

        // Check if there are labels on this line or a snippet marker
        let line_labels: Vec<_> = d.labels.iter().filter(|l| l.span.line == s.line).collect();
        let primary_label = line_labels.iter().find(|l| l.primary).copied();

        if let Some(ref marker) = s.marker {
            let padding = " ".repeat(marker.col.saturating_sub(1));
            let carets = "^".repeat(marker.len.max(1));

            if let Some(lbl) = primary_label {
                let _ = writeln!(
                    out,
                    "{:>width$} | {}{} {}",
                    "",
                    padding,
                    carets,
                    lbl.text,
                    width = gutter_width
                );
            } else {
                let _ = writeln!(
                    out,
                    "{:>width$} | {}{}",
                    "",
                    padding,
                    carets,
                    width = gutter_width
                );
            }

            // Secondary labels on the same line if any
            for sec in line_labels.iter().filter(|l| !l.primary) {
                let sec_pad = " ".repeat(sec.span.col.saturating_sub(1));
                let _ = writeln!(
                    out,
                    "{:>width$} | {}- this `{}` is still open",
                    "",
                    sec_pad,
                    if sec.text.contains('(') { "(" } else if sec.text.contains('{') { "{" } else { "[" },
                    width = gutter_width
                );
            }
        } else if !line_labels.is_empty() {
            // Secondary labels on lines without primary marker
            for lbl in line_labels {
                let padding = " ".repeat(lbl.span.col.saturating_sub(1));
                let _ = writeln!(
                    out,
                    "{:>width$} | {}- {}",
                    "",
                    padding,
                    lbl.text,
                    width = gutter_width
                );
            }
        }
    }

    let _ = writeln!(out, "{:>width$} |", "", width = gutter_width);

    // Help & Suggestions
    for sug in &d.suggestions {
        let _ = writeln!(out, "help: {}", sug.message);
    }

    out
}
