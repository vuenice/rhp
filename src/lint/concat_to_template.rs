use crate::ast::{
    expr::{BinaryOp, Expr, ExprKind, Literal, StringPart},
    visitor::{walk_expr, AstVisitor},
};
use crate::lint::reporter::{Issue, IssueLevel};
use crate::parser::parse;

#[derive(Debug, Clone)]
struct ConcatEdit {
    start: usize,
    end: usize,
    replacement: String,
    line: usize,
    col: usize,
}

/// Convert string concatenation chains (`.` or `+` involving `'...'`, `"..."`, or `` `...` ``)
/// into backtick template strings (`` `...{$var}...` ``) in RHP source code.
pub fn convert_concat_to_template_strings(source: &str) -> String {
    let edits = collect_concat_edits(source);
    if edits.is_empty() {
        return source.to_string();
    }

    let mut out = source.to_string();
    let mut sorted = edits;
    sorted.sort_by(|a, b| b.start.cmp(&a.start));
    for edit in sorted {
        if edit.start <= edit.end && edit.end <= out.len() {
            out.replace_range(edit.start..edit.end, &edit.replacement);
        }
    }
    out
}

/// Check RHP source code for string concatenation chains that can be written as
/// backtick template strings and return lint issues.
pub fn check_concat_to_template_strings(source: &str, file_name: &str) -> Vec<Issue> {
    collect_concat_edits(source)
        .into_iter()
        .map(|edit| {
            Issue::new(
                IssueLevel::Warning,
                "prefer-template-string",
                format!(
                    "Use template string interpolation `{}` instead of string concatenation.",
                    edit.replacement
                ),
                file_name,
                edit.line,
                edit.col,
                edit.start,
                edit.end,
            )
        })
        .collect()
}

struct ConcatCollector<'a> {
    source: &'a str,
    edits: Vec<ConcatEdit>,
}

impl<'s> AstVisitor<'s> for ConcatCollector<'s> {
    fn visit_expr(&mut self, expr: &Expr<'s>) {
        if let ExprKind::Binary { op, left, right } = &expr.kind {
            if is_concat_op(*op) {
                if let Some(replacement) = try_convert_concat_expr(expr, *op, self.source) {
                    self.edits.push(ConcatEdit {
                        start: expr.span.start,
                        end: expr.span.end,
                        replacement,
                        line: expr.span.line,
                        col: expr.span.col,
                    });
                    return;
                }
                self.visit_expr(left);
                self.visit_expr(right);
                return;
            }
        }
        walk_expr(self, expr);
    }
}

fn collect_concat_edits(source: &str) -> Vec<ConcatEdit> {
    if let Ok(program) = parse(source) {
        let mut collector = ConcatCollector {
            source,
            edits: Vec::new(),
        };
        collector.visit_program(&program);
        return collector.edits;
    }

    // RHP parser rejects legacy `.` concatenation; replace standalone `.` with 1-byte `+`
    // outside strings/comments/numbers so byte spans stay identical to `source`.
    let normalized = normalize_concat_dots_same_len(source);
    let Ok(program) = parse(&normalized) else {
        return Vec::new();
    };
    let mut collector = ConcatCollector {
        source,
        edits: Vec::new(),
    };
    collector.visit_program(&program);
    collector.edits
}

fn normalize_concat_dots_same_len(source: &str) -> String {
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut out = bytes.to_vec();
    let mut i = 0;

    while i < n {
        let b = bytes[i];
        if (b == b'/' && i + 1 < n && bytes[i + 1] == b'/') || b == b'#' {
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            i += 2;
            while i < n {
                if bytes[i] == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if b == b'\'' || b == b'"' || b == b'`' {
            let quote = b;
            if quote == b'`' && i + 2 < n && bytes[i + 1] == b'`' && bytes[i + 2] == b'`' {
                i += 3;
                while i < n {
                    if bytes[i] == b'\\' && i + 1 < n {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'`' && i + 2 < n && bytes[i + 1] == b'`' && bytes[i + 2] == b'`' {
                        i += 3;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            i += 1;
            while i < n {
                if bytes[i] == b'\\' && i + 1 < n {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if b == b'.' {
            let prev_dot = i > 0 && bytes[i - 1] == b'.';
            let next_dot = i + 1 < n && bytes[i + 1] == b'.';
            let prev_digit = i > 0 && bytes[i - 1].is_ascii_digit();
            let next_digit = i + 1 < n && bytes[i + 1].is_ascii_digit();
            if !prev_dot && !next_dot && !(prev_digit && next_digit) {
                if next_digit {
                    let mut p = i;
                    let mut prev_non_ws = None;
                    while p > 0 {
                        p -= 1;
                        if !bytes[p].is_ascii_whitespace() {
                            prev_non_ws = Some(bytes[p]);
                            break;
                        }
                    }
                    let is_float = matches!(
                        prev_non_ws,
                        None | Some(
                            b'=' | b'(' | b'[' | b'{' | b',' | b';' | b':' | b'?' | b'!' | b'+'
                                | b'-' | b'*' | b'/' | b'%' | b'<' | b'>' | b'&' | b'|' | b'^'
                                | b'~'
                        )
                    );
                    if !is_float {
                        out[i] = b'+';
                    }
                } else {
                    out[i] = b'+';
                }
            }
        }
        i += 1;
    }

    String::from_utf8(out).unwrap_or_else(|_| source.to_string())
}

fn is_concat_op(op: BinaryOp) -> bool {
    matches!(op, BinaryOp::Concat | BinaryOp::Add)
}

fn flatten_concat_chain<'a>(expr: &'a Expr<'a>, op: BinaryOp, out: &mut Vec<&'a Expr<'a>>) {
    if let ExprKind::Binary {
        op: child_op,
        left,
        right,
    } = &expr.kind
    {
        if *child_op == op {
            flatten_concat_chain(left, op, out);
            flatten_concat_chain(right, op, out);
            return;
        }
    }
    out.push(expr);
}

fn is_quoted_or_backtick_string(expr: &Expr<'_>) -> bool {
    match &expr.kind {
        ExprKind::Literal(Literal::String { raw, .. } | Literal::InterpolatedString { raw, .. }) => {
            (raw.starts_with('\'') && raw.ends_with('\''))
                || (raw.starts_with('"') && raw.ends_with('"'))
                || (raw.starts_with('`') && raw.ends_with('`') && !raw.starts_with("```"))
        }
        _ => false,
    }
}

fn is_interpolatable_var_expr(expr: &Expr<'_>, source: &str) -> bool {
    let slice = source
        .get(expr.span.start..expr.span.end)
        .map(str::trim)
        .unwrap_or("");
    slice.starts_with('$') && !slice.contains('`')
}

fn escape_literal_for_backtick(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '\\' => out.push_str("\\\\"),
            '`' => out.push_str("\\`"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '{' => {
                let mut p = i + 1;
                while p < chars.len() && (chars[p] == ' ' || chars[p] == '\t') {
                    p += 1;
                }
                if p < chars.len() && chars[p] == '$' {
                    out.push_str("\\{");
                } else {
                    out.push('{');
                }
            }
            _ => out.push(ch),
        }
        i += 1;
    }
    out
}

fn append_expr_to_template(expr: &Expr<'_>, source: &str, out: &mut String) -> bool {
    match &expr.kind {
        ExprKind::Literal(Literal::String { raw, value }) => {
            if raw.starts_with('`') && raw.ends_with('`') && !raw.starts_with("```") && raw.len() >= 2 {
                out.push_str(&raw[1..raw.len() - 1]);
                true
            } else if (raw.starts_with('\'') && raw.ends_with('\''))
                || (raw.starts_with('"') && raw.ends_with('"'))
            {
                out.push_str(&escape_literal_for_backtick(value));
                true
            } else {
                false
            }
        }
        ExprKind::Literal(Literal::InterpolatedString { raw, parts }) => {
            if raw.starts_with('`') && raw.ends_with('`') && !raw.starts_with("```") && raw.len() >= 2 {
                out.push_str(&raw[1..raw.len() - 1]);
                return true;
            }
            if raw.starts_with('"') && raw.ends_with('"') {
                for part in parts {
                    match part {
                        StringPart::Literal(s) => out.push_str(&escape_literal_for_backtick(s)),
                        StringPart::Expr(inner) => {
                            let Some(slice) = source.get(inner.span.start..inner.span.end) else {
                                return false;
                            };
                            let trimmed = slice.trim();
                            if !trimmed.starts_with('$') || trimmed.contains('`') {
                                return false;
                            }
                            out.push('{');
                            out.push_str(trimmed);
                            out.push('}');
                        }
                    }
                }
                return true;
            }
            false
        }
        _ if is_interpolatable_var_expr(expr, source) => {
            let trimmed = source[expr.span.start..expr.span.end].trim();
            out.push('{');
            out.push_str(trimmed);
            out.push('}');
            true
        }
        _ => false,
    }
}

fn try_convert_concat_expr(expr: &Expr<'_>, op: BinaryOp, source: &str) -> Option<String> {
    let mut parts = Vec::new();
    flatten_concat_chain(expr, op, &mut parts);
    if parts.len() < 2 {
        return None;
    }

    let has_string_literal = parts.iter().any(|p| is_quoted_or_backtick_string(p));
    if !has_string_literal {
        return None;
    }

    let mut inner = String::new();
    for part in &parts {
        if !append_expr_to_template(part, source, &mut inner) {
            return None;
        }
    }

    Some(format!("`{}`", inner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::{lint_source_with_options, LintOptions};

    #[test]
    fn converts_dot_and_plus_concatenation_to_backtick_template() {
        let src = r#"$info = "Hello my name is " . $name . " and my age is " . $age;"#;
        let out = convert_concat_to_template_strings(src);
        assert_eq!(out, r#"$info = `Hello my name is {$name} and my age is {$age}`;"#);

        let plus_src = r#"$info = "Hello my name is " + $name + " and my age is " + $age;"#;
        let plus_out = convert_concat_to_template_strings(plus_src);
        assert_eq!(plus_out, r#"$info = `Hello my name is {$name} and my age is {$age}`;"#);
    }

    #[test]
    fn handles_single_quotes_backticks_and_escapes_safely() {
        let src = r#"$s = 'Hi `user` {$raw} ' . $name . ` ({$role})`;"#;
        let out = convert_concat_to_template_strings(src);
        assert_eq!(out, r#"$s = `Hi \`user\` \{$raw} {$name} ({$role})`;"#);
    }

    #[test]
    fn leaves_numeric_addition_and_plain_html_untouched() {
        let num_src = "$sum = $a + $b + 10;";
        assert_eq!(convert_concat_to_template_strings(num_src), num_src);

        let rade_src = "<p>Hello world.</p>\n<@ \"Hello \" . $name @>\n<@?\n$info = 'Age: ' . $age;\n@>";
        let res = lint_source_with_options(
            rade_src,
            "views/profile.rade.rhp",
            LintOptions {
                fix: true,
                case_change: false,
                stream: false,
            },
        );
        assert!(res.formatted_content.contains("<p>Hello world.</p>"));
        assert!(res.formatted_content.contains("<@ `Hello {$name}` @>"));
        assert!(res.formatted_content.contains("`Age: {$age}`"));
    }

    #[test]
    fn check_mode_reports_prefer_template_string_issue() {
        let src = r#"$msg = "Hello " . $name;"#;
        let issues = check_concat_to_template_strings(src, "test.rhp");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "prefer-template-string");
    }

    #[test]
    fn converted_backtick_template_parses_as_interpolated_string() {
        let converted = convert_concat_to_template_strings(
            r#"$info = "Hello my name is " . $name . " and my age is " . $age;"#,
        );
        let parsed = crate::parser::parse(&converted);
        assert!(parsed.is_ok(), "Expected converted backtick string to parse, got: {:?}", parsed.err());
    }
}
