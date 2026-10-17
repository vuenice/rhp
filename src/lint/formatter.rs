/// High-performance code formatter for RHP and PHP source code.
///
/// Implements standard formatting rules inspired by Carthage Mago and PER-CS:
/// - 4-space indentation per nesting level
/// - Standardized binary operator spacing (`$a + $b`, `$x = 1`)
/// - Standardized keyword and brace spacing (`if ($cond) {`, `function foo($a, $b) {`)
/// - Normalized commas (single space after, none before)
/// - Semicolon formatting (no space before)
/// - Preserves string literals and comment contents intact
/// - Trims trailing whitespace and collapses redundant blank lines
pub fn format_source(source: &str) -> String {
    let lines = split_lines(source);
    let mut formatted_lines = Vec::with_capacity(lines.len());
    let mut indent_level: usize = 0;
    let mut in_multiline_comment = false;
    let mut in_multiline_string = false;
    let mut string_delim: char = '"';
    let mut previous_was_empty = false;

    for raw_line in lines {
        let trimmed = raw_line.trim();

        // Handle empty lines
        if trimmed.is_empty() {
            if in_multiline_comment || in_multiline_string {
                formatted_lines.push(String::new());
                previous_was_empty = false;
            } else if !previous_was_empty && !formatted_lines.is_empty() {
                formatted_lines.push(String::new());
                previous_was_empty = true;
            }
            continue;
        }

        // Check if inside a multi-line comment
        if in_multiline_comment {
            let indent = " ".repeat(indent_level * 4);
            formatted_lines.push(format!("{}{}", indent, trimmed));
            if trimmed.contains("*/") {
                in_multiline_comment = false;
            }
            previous_was_empty = false;
            continue;
        }

        // Check if inside a multi-line string
        if in_multiline_string {
            formatted_lines.push(raw_line.to_string());
            if trimmed.ends_with(string_delim) && !trimmed.ends_with(&format!("\\{}", string_delim)) {
                in_multiline_string = false;
            }
            previous_was_empty = false;
            continue;
        }

        // Normalize spacing within line code segments
        let spaced = format_line_spacing(trimmed);

        // Adjust indent level for lines starting with closing braces
        let starts_with_close = spaced.starts_with('}') || spaced.starts_with(']');
        let is_case_or_default = spaced.starts_with("case ") || spaced.starts_with("default:") || spaced == "default";

        let current_line_indent = if starts_with_close && indent_level > 0 {
            indent_level - 1
        } else if is_case_or_default && indent_level > 0 {
            indent_level.saturating_sub(1)
        } else {
            indent_level
        };

        let indent = " ".repeat(current_line_indent * 4);
        formatted_lines.push(format!("{}{}", indent, spaced));
        previous_was_empty = false;

        // Calculate net change in indentation level from this line
        let (opens, closes, enters_comment, enters_string, delim) = scan_line_structural_tokens(&spaced);
        if enters_comment {
            in_multiline_comment = true;
        }
        if enters_string {
            in_multiline_string = true;
            string_delim = delim;
        }

        // Apply nesting adjustments
        if opens > closes {
            indent_level += opens - closes;
        } else if closes > opens {
            indent_level = indent_level.saturating_sub(closes - opens);
        }
    }

    // Join with newlines and ensure exactly one trailing newline
    let mut result = formatted_lines.join("\n");
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Split input into lines while handling both LF and CRLF.
fn split_lines(source: &str) -> Vec<&str> {
    source.lines().collect()
}

/// Scan line for structural braces and multi-line openers outside strings and comments.
fn scan_line_structural_tokens(line: &str) -> (usize, usize, bool, bool, char) {
    let mut opens = 0;
    let mut closes = 0;
    let mut in_str = false;
    let mut str_char = '"';
    let mut in_comment = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if in_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_comment = false;
            }
            continue;
        }

        if in_str {
            if c == '\\' {
                chars.next(); // skip escaped char
            } else if c == str_char {
                in_str = false;
            }
            continue;
        }

        // Single line comments
        if c == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if c == '#' {
            break;
        }

        // Multi-line comment open
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            in_comment = true;
            continue;
        }

        // String opens
        if c == '"' || c == '\'' || c == '`' {
            in_str = true;
            str_char = c;
            continue;
        }

        if c == '{' {
            opens += 1;
        } else if c == '}' {
            closes += 1;
        }
    }

    (opens, closes, in_comment, in_str, str_char)
}

/// Format whitespace and operator spacing within a single line.
fn format_line_spacing(line: &str) -> String {
    let mut output = String::with_capacity(line.len() + 16);
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;

    let mut in_str = false;
    let mut str_delim = '"';

    while i < n {
        let c = chars[i];

        // Handle string literals
        if in_str {
            output.push(c);
            if c == '\\' && i + 1 < n {
                i += 1;
                output.push(chars[i]);
            } else if c == str_delim {
                in_str = false;
            }
            i += 1;
            continue;
        }

        if c == '"' || c == '\'' || c == '`' {
            in_str = true;
            str_delim = c;
            output.push(c);
            i += 1;
            continue;
        }

        // Handle single-line comments: preserve rest of line verbatim
        if (c == '/' && i + 1 < n && chars[i + 1] == '/') || c == '#' {
            output.push_str(&line[i..]);
            break;
        }

        // Handle multi-line comment start: preserve until end
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            output.push_str("/*");
            i += 2;
            while i < n {
                if chars[i] == '*' && i + 1 < n && chars[i + 1] == '/' {
                    output.push_str("*/");
                    i += 2;
                    break;
                }
                output.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // Normalize multiple spaces into single space outside strings
        if c == ' ' || c == '\t' {
            if !output.ends_with(' ') && !output.is_empty() {
                output.push(' ');
            }
            i += 1;
            continue;
        }

        // Normalize commas: no space before, one space after
        if c == ',' {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push(',');
            output.push(' ');
            i += 1;
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            continue;
        }

        // Normalize semicolons: no space before
        if c == ';' {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push(';');
            i += 1;
            continue;
        }

        // Arrow `->` and nullsafe `?->`
        if c == '?' && i + 2 < n && chars[i + 1] == '-' && chars[i + 2] == '>' {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push_str("?->");
            i += 3;
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            continue;
        }
        if c == '-' && i + 1 < n && chars[i + 1] == '>' {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push_str("->");
            i += 2;
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            continue;
        }

        // Double colon `::`
        if c == ':' && i + 1 < n && chars[i + 1] == ':' {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push_str("::");
            i += 2;
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            continue;
        }

        // Return type colon `: `
        if c == ':' && i + 1 < n && chars[i + 1] != ':' {
            // Check if this looks like a return type colon `): type` or case label
            if output.ends_with(')') {
                output.push_str(": ");
                i += 1;
                while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                    i += 1;
                }
                continue;
            }
        }

        // Array double arrow `=>`
        if c == '=' && i + 1 < n && chars[i + 1] == '>' {
            ensure_space_before(&mut output);
            output.push_str("=> ");
            i += 2;
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            continue;
        }

        // Strict comparison `===` and `!==`
        if (c == '=' || c == '!') && i + 2 < n && chars[i + 1] == '=' && chars[i + 2] == '=' {
            ensure_space_before(&mut output);
            output.push(c);
            output.push_str("== ");
            i += 3;
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            continue;
        }

        // Comparison and compound assignments `==`, `!=`, `<=`, `>=`, `+=`, `-=`, `*=`, `/=`, `%=`, `.=`, `??=`, `??`
        if i + 1 < n {
            let next = chars[i + 1];
            let pair = (c, next);
            let is_two_char_op = matches!(
                pair,
                ('=', '=')
                    | ('!', '=')
                    | ('<', '=')
                    | ('>', '=')
                    | ('+', '=')
                    | ('-', '=')
                    | ('*', '=')
                    | ('/', '=')
                    | ('%', '=')
                    | ('.', '=')
                    | ('?', '?')
                    | ('&', '&')
                    | ('|', '|')
            );

            if is_two_char_op {
                // Check if followed by `=` for `??=`
                if c == '?' && next == '?' && i + 2 < n && chars[i + 2] == '=' {
                    ensure_space_before(&mut output);
                    output.push_str("??= ");
                    i += 3;
                    while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                        i += 1;
                    }
                    continue;
                }

                ensure_space_before(&mut output);
                output.push(c);
                output.push(next);
                output.push(' ');
                i += 2;
                while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                    i += 1;
                }
                continue;
            }
        }

        // Single character assignment `=` (not preceded or followed by `=`)
        if c == '=' {
            ensure_space_before(&mut output);
            output.push_str("= ");
            i += 1;
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            continue;
        }

        // Binary comparison `<` and `>` (when not part of tag `<?` or `?>` or `->` or `=>`)
        if (c == '<' || c == '>') && !output.ends_with('?') {
            ensure_space_before(&mut output);
            output.push(c);
            output.push(' ');
            i += 1;
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            continue;
        }

        // Opening brace `{`: space before if preceded by `)`, `else`, `try`, `finally`
        if c == '{' {
            if !output.is_empty() && !output.ends_with(' ') && !output.ends_with('{') {
                output.push(' ');
            }
            output.push('{');
            i += 1;
            continue;
        }

        // Closing brace `}`: space after if followed by keyword like `else`, `elseif`, `catch`, `finally`
        if c == '}' {
            output.push('}');
            i += 1;
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            if i < n && chars[i].is_alphanumeric() {
                output.push(' ');
            }
            continue;
        }

        // Opening parenthesis `(`: keyword spacing for `if (`, `while (`, `for (`, `foreach (`
        if c == '(' {
            let last_word = get_last_word(&output);
            if matches!(last_word.as_str(), "if" | "elseif" | "while" | "for" | "foreach" | "switch" | "catch") {
                if !output.ends_with(' ') {
                    output.push(' ');
                }
            }
            output.push('(');
            i += 1;
            continue;
        }

        // Binary plus and minus (distinguishing from unary `-5` or `$i++`)
        if c == '+' || c == '-' {
            // Check for `++` and `--`
            if i + 1 < n && chars[i + 1] == c {
                output.push(c);
                output.push(c);
                i += 2;
                continue;
            }

            // Distinguish binary vs unary
            let is_binary = is_binary_op_context(&output);
            if is_binary {
                ensure_space_before(&mut output);
                output.push(c);
                output.push(' ');
                i += 1;
                while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                    i += 1;
                }
                continue;
            } else {
                output.push(c);
                i += 1;
                continue;
            }
        }

        // Multiplication `*` and division `/` and modulo `%`
        if c == '*' || c == '%' || (c == '/' && i + 1 < n && chars[i + 1] != '/' && chars[i + 1] != '*') {
            // Check if part of docblock ` * ` or comment
            if c == '*' && output.trim().is_empty() {
                output.push('*');
                i += 1;
                continue;
            }
            let is_binary = is_binary_op_context(&output);
            if is_binary {
                ensure_space_before(&mut output);
                output.push(c);
                output.push(' ');
                i += 1;
                while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                    i += 1;
                }
                continue;
            }
        }

        // String concatenation `.` outside numbers (e.g. `$a . $b`, not `3.14`)
        if c == '.' {
            let prev_digit = output.chars().last().map(|ch| ch.is_ascii_digit()).unwrap_or(false);
            let next_digit = if i + 1 < n { chars[i + 1].is_ascii_digit() } else { false };
            let is_ellipsis = (i + 1 < n && chars[i + 1] == '.') || output.ends_with('.');

            if !prev_digit && !next_digit && !is_ellipsis {
                ensure_space_before(&mut output);
                output.push_str(". ");
                i += 1;
                while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                    i += 1;
                }
                continue;
            }
        }

        output.push(c);
        i += 1;
    }

    // Clean up trailing spaces before semicolon or at end of line
    output.trim_end().to_string()
}

fn ensure_space_before(output: &mut String) {
    if !output.is_empty() && !output.ends_with(' ') {
        output.push(' ');
    }
}

fn get_last_word(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .last()
        .unwrap_or("")
        .to_string()
}

fn is_binary_op_context(output: &str) -> bool {
    let trimmed = output.trim_end();
    if trimmed.is_empty() {
        return false;
    }
    let last = trimmed.chars().last().unwrap();
    // Preceded by identifier, variable, literal, or closing delimiter
    last.is_alphanumeric() || last == '$' || last == ')' || last == ']' || last == '\'' || last == '"' || last == '`'
}
