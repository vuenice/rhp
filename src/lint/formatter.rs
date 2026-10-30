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
    let with_conditions = wrap_control_conditions(source);
    let with_ops_moved = move_trailing_operators(&with_conditions);
    let with_semicolons = insert_missing_semicolons(&with_ops_moved);
    let preprocessed = split_function_braces_to_next_line(&with_semicolons);
    let lines = split_lines(&preprocessed);
    let mut formatted_lines = Vec::with_capacity(lines.len());
    let mut indent_level: usize = 0;
    let mut in_multiline_comment = false;
    let mut in_multiline_string = false;
    let mut string_delim: char = '"';
    let mut previous_was_empty = false;
    let mut is_in_continuation = false;

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

        // Adjust indent level for lines starting with closing braces/brackets/parentheses
        let leading_closes = count_leading_closes(&spaced);
        let is_case_or_default = spaced.starts_with("case ") || spaced.starts_with("default:") || spaced == "default";

        let current_line_indent = if leading_closes > 0 && indent_level > 0 {
            indent_level.saturating_sub(leading_closes)
        } else if is_case_or_default && indent_level > 0 {
            indent_level.saturating_sub(1)
        } else {
            indent_level
        };

        let is_continuation_line = starts_with_continuation_op(&spaced) || is_in_continuation;
        let effective_indent = if is_continuation_line && leading_closes == 0 && !is_case_or_default {
            current_line_indent + 1
        } else {
            current_line_indent
        };

        let indent = " ".repeat(effective_indent * 4);
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

        // Update continuation state for next line:
        let (code_part, _) = split_line_comment(&spaced);
        let trimmed_code = code_part.trim_end();
        if trimmed_code.ends_with(';') || trimmed_code.ends_with('{') || trimmed_code.ends_with('}') {
            is_in_continuation = false;
        } else if ends_with_continuation_op(&spaced) || starts_with_continuation_op(&spaced) {
            is_in_continuation = true;
        } else {
            is_in_continuation = false;
        }
    }

    // Join with newlines and ensure exactly one trailing newline
    let mut result = formatted_lines.join("\n");
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Count closing tokens `}`, `]`, `)` at the beginning of a line.
fn count_leading_closes(line: &str) -> usize {
    let mut count = 0;
    for c in line.chars() {
        if c == '}' || c == ']' || c == ')' {
            count += 1;
        } else if (c == ';' || c == ',') && count > 0 {
            continue;
        } else if c == ' ' || c == '\t' {
            continue;
        } else {
            break;
        }
    }
    count
}

/// Separates opening curly braces `{` of function and method declarations onto the next line
/// following standard PER-CS / PSR-12 and Carthage Mago formatting rules.
fn split_function_braces_to_next_line(source: &str) -> String {
    let mut result_lines = Vec::new();
    let mut in_function_signature = false;
    let mut paren_balance: usize = 0;
    let mut in_multiline_comment = false;

    for line in source.lines() {
        let trimmed = line.trim();

        // If inside a multiline comment, check for end
        if in_multiline_comment {
            if trimmed.contains("*/") {
                in_multiline_comment = false;
            }
            result_lines.push(line.to_string());
            continue;
        }

        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_multiline_comment = true;
            result_lines.push(line.to_string());
            continue;
        }

        // Check if a function/method declaration begins on this line
        if !in_function_signature {
            if is_function_declaration_start(trimmed) {
                let (opens, closes) = count_parens(trimmed);
                if opens > closes {
                    in_function_signature = true;
                    paren_balance = opens - closes;
                    result_lines.push(line.to_string());
                    continue;
                } else if let Some(without) = ends_with_open_brace(line) {
                    result_lines.push(without);
                    result_lines.push("{".to_string());
                    continue;
                }
            }
        } else {
            // Inside multiline function signature
            let (opens, closes) = count_parens(trimmed);
            paren_balance += opens;
            if closes >= paren_balance {
                // Function parameter list finished
                in_function_signature = false;
                paren_balance = 0;
                if let Some(without) = ends_with_open_brace(line) {
                    result_lines.push(without);
                    result_lines.push("{".to_string());
                    continue;
                }
            } else {
                paren_balance -= closes;
            }
        }

        result_lines.push(line.to_string());
    }

    result_lines.join("\n")
}

fn ends_with_open_brace(line: &str) -> Option<String> {
    let trimmed = line.trim_end();
    if trimmed.ends_with('{') {
        Some(trimmed[..trimmed.len() - 1].trim_end().to_string())
    } else {
        None
    }
}

fn is_function_declaration_start(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut in_str = false;
    let mut str_delim = '"';

    while i < n {
        let c = chars[i];
        if in_str {
            if c == '\\' && i + 1 < n {
                i += 2;
                continue;
            }
            if c == str_delim {
                in_str = false;
            }
            i += 1;
            continue;
        }

        if c == '"' || c == '\'' || c == '`' {
            in_str = true;
            str_delim = c;
            i += 1;
            continue;
        }

        if (c == '/' && i + 1 < n && (chars[i + 1] == '/' || chars[i + 1] == '*')) || c == '#' {
            break;
        }

        // Check for whole keyword "function"
        if line[i..].starts_with("function") {
            let before_ok = i == 0 || (!chars[i - 1].is_alphanumeric() && chars[i - 1] != '_' && chars[i - 1] != '$');
            let after_idx = i + 8;
            let after_ok = after_idx < n && (chars[after_idx].is_whitespace() || chars[after_idx] == '&');

            if before_ok && after_ok {
                let mut j = after_idx;
                while j < n && (chars[j].is_whitespace() || chars[j] == '&') {
                    j += 1;
                }
                // Named function/method has an identifier name, not '('
                if j < n && (chars[j].is_alphabetic() || chars[j] == '_') {
                    return true;
                }
            }
        }

        i += 1;
    }

    false
}

fn count_parens(line: &str) -> (usize, usize) {
    let mut opens = 0;
    let mut closes = 0;
    let mut in_str = false;
    let mut str_delim = '"';
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        let c = chars[i];
        if in_str {
            if c == '\\' && i + 1 < n {
                i += 2;
                continue;
            }
            if c == str_delim {
                in_str = false;
            }
            i += 1;
            continue;
        }

        if c == '"' || c == '\'' || c == '`' {
            in_str = true;
            str_delim = c;
            i += 1;
            continue;
        }

        if (c == '/' && i + 1 < n && (chars[i + 1] == '/' || chars[i + 1] == '*')) || c == '#' {
            break;
        }

        if c == '(' {
            opens += 1;
        } else if c == ')' {
            closes += 1;
        }

        i += 1;
    }

    (opens, closes)
}

/// Split input into lines while handling both LF and CRLF.
fn split_lines(source: &str) -> Vec<&str> {
    source.lines().collect()
}

/// Scan line for structural braces, brackets, parentheses outside strings and comments.
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

        if c == '{' || c == '[' || c == '(' {
            opens += 1;
        } else if c == '}' || c == ']' || c == ')' {
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
            i += 1;
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            if i < n && chars[i] != ')' && chars[i] != ']' && chars[i] != '}' {
                output.push(' ');
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

        // Ternary `?` at the beginning of a continuation line
        if c == '?' && output.trim().is_empty() && i + 1 < n && chars[i + 1] != '?' && chars[i + 1] != '-' {
            output.push('?');
            output.push(' ');
            i += 1;
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            continue;
        }

        // Ternary `:` at the beginning of a continuation line
        if c == ':' && output.trim().is_empty() && i + 1 < n && chars[i + 1] != ':' {
            output.push(':');
            output.push(' ');
            i += 1;
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
            } else if !last_word.is_empty() && last_word != "function" {
                while output.ends_with(' ') {
                    output.pop();
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
            let is_binary = is_binary_op_context(&output)
                || (output.trim().is_empty() && i + 1 < n && (chars[i + 1] == ' ' || chars[i + 1] == '\t'));
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
            if c == '*' && output.trim().is_empty() && (i + 1 >= n || chars[i + 1] != '$') {
                output.push('*');
                i += 1;
                continue;
            }
            let is_binary = is_binary_op_context(&output)
                || (output.trim().is_empty() && i + 1 < n && (chars[i + 1] == ' ' || chars[i + 1] == '\t'));
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

/// Split a line into code and comment portions outside of string literals.
fn split_line_comment(line: &str) -> (&str, &str) {
    let mut in_str = false;
    let mut str_delim = '"';
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        let c = chars[i];
        if in_str {
            if c == '\\' && i + 1 < n {
                i += 2;
                continue;
            }
            if c == str_delim {
                in_str = false;
            }
            i += 1;
            continue;
        }

        if c == '"' || c == '\'' || c == '`' {
            in_str = true;
            str_delim = c;
            i += 1;
            continue;
        }

        // Check for attribute #[
        if c == '#' && i + 1 < n && chars[i + 1] == '[' {
            i += 2;
            continue;
        }

        // Single line comments // or #
        if (c == '/' && i + 1 < n && chars[i + 1] == '/') || c == '#' {
            let byte_idx = line.char_indices().nth(i).map(|(idx, _)| idx).unwrap_or(line.len());
            return (&line[..byte_idx], &line[byte_idx..]);
        }

        i += 1;
    }

    (line, "")
}

/// Converts `if $i <10` to `if ($i < 10)` and `for $item in $items` to `for ($item in $items)`,
/// adding round brackets to `if`, `for`, `elseif`, and `while` statements when absent.
fn wrap_control_conditions(source: &str) -> String {
    let mut output_lines = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut in_multiline_comment = false;

    for line in lines {
        let trimmed = line.trim();
        if in_multiline_comment {
            if trimmed.contains("*/") {
                in_multiline_comment = false;
            }
            output_lines.push(line.to_string());
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_multiline_comment = true;
            output_lines.push(line.to_string());
            continue;
        }

        let (code, comment) = split_line_comment(line);
        let trimmed_code = code.trim();
        if trimmed_code.is_empty() {
            output_lines.push(line.to_string());
            continue;
        }

        if let Some(new_code) = transform_control_line(code) {
            output_lines.push(format!("{}{}", new_code, comment));
        } else {
            output_lines.push(line.to_string());
        }
    }

    output_lines.join("\n")
}

fn transform_control_line(code: &str) -> Option<String> {
    let leading_len = code.len() - code.trim_start().len();
    let leading_ws = &code[..leading_len];
    let content = code.trim_start();

    // Check optional leading closing brace e.g. `} elseif ` or `} else if `
    let (prefix, after_prefix) = if content.starts_with("} ") {
        ("} ", &content[2..])
    } else if content.starts_with('}') {
        ("} ", content[1..].trim_start())
    } else {
        ("", content)
    };

    let keywords = ["elseif", "if", "for", "while"];
    for &kw in &keywords {
        if after_prefix.starts_with(kw) {
            let remainder = &after_prefix[kw.len()..];
            // Must be followed by whitespace or '('
            if remainder.starts_with(' ') || remainder.starts_with('\t') || remainder.starts_with('(') {
                let trimmed_rem = remainder.trim_start();
                let has_trailing_brace = trimmed_rem.ends_with('{');
                let condition_part = if has_trailing_brace {
                    trimmed_rem[..trimmed_rem.len() - 1].trim()
                } else {
                    trimmed_rem.trim()
                };

                if condition_part.is_empty() {
                    return None;
                }

                // Check if already fully enclosed in parentheses
                if is_fully_enclosed_in_parens(condition_part) {
                    return None;
                }

                let wrapped = format!("({})", condition_part);
                return if has_trailing_brace {
                    Some(format!("{}{}{} {} {{", leading_ws, prefix, kw, wrapped))
                } else {
                    Some(format!("{}{}{} {}", leading_ws, prefix, kw, wrapped))
                };
            }
        }
    }

    None
}

fn is_fully_enclosed_in_parens(s: &str) -> bool {
    let trimmed = s.trim();
    if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
        return false;
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let n = chars.len();
    let mut depth = 0;
    let mut in_str = false;
    let mut str_delim = '"';

    for i in 0..n {
        let c = chars[i];
        if in_str {
            if c == '\\' && i + 1 < n {
                continue;
            }
            if c == str_delim {
                in_str = false;
            }
            continue;
        }
        if c == '"' || c == '\'' || c == '`' {
            in_str = true;
            str_delim = c;
            continue;
        }
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 && i < n - 1 {
                return false;
            }
        }
    }
    depth == 0
}

/// Moves trailing operators at the end of a line (e.g. `->`, `&&`, `||`, `+`, `.`, `?`, `:`)
/// to the beginning of the next line, preserving indentation and comments.
fn move_trailing_operators(source: &str) -> String {
    let mut lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();
    let n = lines.len();
    let mut in_multiline_comment = false;

    let mut i = 0;
    while i < n {
        let line = lines[i].clone();
        let trimmed = line.trim();

        if in_multiline_comment {
            if trimmed.contains("*/") {
                in_multiline_comment = false;
            }
            i += 1;
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_multiline_comment = true;
            i += 1;
            continue;
        }

        let (code, comment) = split_line_comment(&line);
        if let Some((code_without_op, op)) = extract_trailing_operator(code) {
            // Find next non-empty, non-comment line j > i
            let mut j = i + 1;
            let mut found_target = None;
            while j < n {
                let next_trimmed = lines[j].trim();
                if next_trimmed.is_empty() {
                    j += 1;
                    continue;
                }
                if next_trimmed.starts_with("//") || next_trimmed.starts_with('#') || next_trimmed.starts_with("/*") {
                    j += 1;
                    continue;
                }
                found_target = Some(j);
                break;
            }

            if let Some(target_idx) = found_target {
                let leading_len = line.len() - line.trim_start().len();
                let leading_ws = &line[..leading_len];
                lines[i] = if comment.is_empty() {
                    format!("{}{}", leading_ws, code_without_op.trim_start())
                } else {
                    format!("{}{}{}", leading_ws, code_without_op.trim_start(), comment)
                };

                let target_line = &lines[target_idx];
                let target_leading_len = target_line.len() - target_line.trim_start().len();
                let target_leading = &target_line[..target_leading_len];
                let target_content = target_line.trim_start();

                let new_target = if op == "->" || op == "?->" {
                    format!("{}{}{}", target_leading, op, target_content)
                } else {
                    format!("{}{} {}", target_leading, op, target_content)
                };
                lines[target_idx] = new_target;
            }
        }

        i += 1;
    }

    lines.join("\n")
}

fn extract_trailing_operator(code: &str) -> Option<(&str, &str)> {
    let trimmed = code.trim_end();
    if trimmed.is_empty() {
        return None;
    }

    let candidate_ops = [
        "?->", "===", "!==", "<=", ">=", "->", "&&", "||", "??", "==", "!=",
        "+", "-", "*", "/", "%", ".", "?", ":"
    ];

    for &op in &candidate_ops {
        if trimmed.ends_with(op) {
            let before_op = &trimmed[..trimmed.len() - op.len()];
            if is_valid_trailing_op(trimmed, before_op, op) {
                return Some((before_op.trim_end(), op));
            }
        }
    }

    None
}

fn is_valid_trailing_op(trimmed: &str, before_op: &str, op: &str) -> bool {
    if is_pos_inside_string(trimmed, before_op.len()) {
        return false;
    }

    match op {
        "." => {
            if trimmed.ends_with("...") {
                return false;
            }
            if before_op.trim_end().chars().last().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                return false;
            }
        }
        "+" => {
            if trimmed.ends_with("++") {
                return false;
            }
        }
        "-" => {
            if trimmed.ends_with("--") || trimmed.ends_with("->") {
                return false;
            }
        }
        "*" => {
            if trimmed.ends_with("/*") || trimmed.ends_with("*/") || before_op.trim().is_empty() {
                return false;
            }
        }
        "/" => {
            if trimmed.ends_with("//") || trimmed.ends_with("/*") {
                return false;
            }
        }
        "?" => {
            if trimmed.ends_with("??") || trimmed.ends_with("?->") {
                return false;
            }
            let last_char = before_op.trim_end().chars().last();
            let is_operand = matches!(last_char, Some(')') | Some(']') | Some('\'') | Some('"') | Some('`') | Some('$'))
                || last_char.map(|c| c.is_alphanumeric()).unwrap_or(false);
            if !is_operand {
                return false;
            }
        }
        ":" => {
            if trimmed.ends_with("::") {
                return false;
            }
            let before_trim = before_op.trim();
            if before_trim.starts_with("case ") || before_trim == "default" || before_trim.ends_with(')') {
                return false;
            }
            if !before_trim.is_empty() && before_trim.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return false;
            }
        }
        _ => {}
    }

    true
}

fn is_pos_inside_string(s: &str, target_byte_pos: usize) -> bool {
    let mut in_str = false;
    let mut str_delim = '"';
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        let (byte_pos, c) = chars[i];
        if byte_pos >= target_byte_pos {
            return in_str;
        }
        if in_str {
            if c == '\\' && i + 1 < n {
                i += 2;
                continue;
            }
            if c == str_delim {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' || c == '`' {
            in_str = true;
            str_delim = c;
        }
        i += 1;
    }
    in_str
}

fn starts_with_continuation_op(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("->") || trimmed.starts_with("?->") {
        return true;
    }
    if trimmed.starts_with("&&") || trimmed.starts_with("||") || trimmed.starts_with("??") {
        return true;
    }
    if trimmed.starts_with("===") || trimmed.starts_with("!==") || trimmed.starts_with("<=")
        || trimmed.starts_with(">=") || trimmed.starts_with("==") || trimmed.starts_with("!=")
    {
        return true;
    }
    if trimmed.starts_with("=>") {
        return true;
    }
    if trimmed.starts_with('.') && !trimmed.starts_with("...") {
        let chars: Vec<char> = trimmed.chars().collect();
        if chars.len() > 1 && !chars[1].is_ascii_digit() {
            return true;
        }
    }
    if trimmed.starts_with('?') && !trimmed.starts_with("??") && !trimmed.starts_with("?->") {
        return true;
    }
    if trimmed.starts_with(':') && !trimmed.starts_with("::") {
        return true;
    }
    if (trimmed.starts_with('+') && !trimmed.starts_with("++"))
        || (trimmed.starts_with('-') && !trimmed.starts_with("--") && !trimmed.starts_with("->"))
        || (trimmed.starts_with('*') && !trimmed.starts_with("*/"))
        || (trimmed.starts_with('/') && !trimmed.starts_with("//") && !trimmed.starts_with("/*"))
        || trimmed.starts_with('%')
    {
        let chars: Vec<char> = trimmed.chars().collect();
        if chars.len() > 1 && (chars[1].is_whitespace() || chars[1] == '$' || chars[1].is_alphanumeric()) {
            return true;
        }
    }
    false
}

fn ends_with_continuation_op(line: &str) -> bool {
    let (code, _) = split_line_comment(line);
    let trimmed = code.trim_end();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.ends_with('=')
        || trimmed.ends_with("=>")
        || trimmed.ends_with("->")
        || trimmed.ends_with("?->")
        || trimmed.ends_with("&&")
        || trimmed.ends_with("||")
        || trimmed.ends_with("??")
        || trimmed.ends_with('?')
        || trimmed.ends_with(':')
        || trimmed.ends_with('+')
        || trimmed.ends_with('-')
        || trimmed.ends_with('*')
        || trimmed.ends_with('/')
        || trimmed.ends_with('%')
        || trimmed.ends_with('.')
}

/// Automatically appends missing semicolons to complete statements.
fn insert_missing_semicolons(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let n = lines.len();
    let mut result_lines = Vec::with_capacity(n);

    let mut paren_depth: isize = 0;
    let mut bracket_depth: isize = 0;
    let mut in_multiline_comment = false;
    let mut in_multiline_string = false;
    let mut str_delim = '"';

    for i in 0..n {
        let raw_line = lines[i];
        let trimmed = raw_line.trim();

        if trimmed.is_empty() {
            result_lines.push(raw_line.to_string());
            continue;
        }

        if in_multiline_comment {
            if trimmed.contains("*/") {
                in_multiline_comment = false;
            }
            result_lines.push(raw_line.to_string());
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_multiline_comment = true;
            result_lines.push(raw_line.to_string());
            continue;
        }

        if in_multiline_string {
            if trimmed.ends_with(str_delim) && !trimmed.ends_with(&format!("\\{}", str_delim)) {
                in_multiline_string = false;
            }
            result_lines.push(raw_line.to_string());
            continue;
        }

        let (code, comment) = split_line_comment(raw_line);
        let trimmed_code = code.trim();

        if trimmed_code.is_empty() {
            result_lines.push(raw_line.to_string());
            continue;
        }

        let (opens_p, closes_p, opens_b, closes_b, enters_comment, enters_string, delim) =
            scan_line_delims(trimmed_code);
        if enters_comment {
            in_multiline_comment = true;
        }
        if enters_string {
            in_multiline_string = true;
            str_delim = delim;
        }

        let start_paren = paren_depth;
        let start_bracket = bracket_depth;
        let end_paren = paren_depth + (opens_p as isize) - (closes_p as isize);
        let end_bracket = bracket_depth + (opens_b as isize) - (closes_b as isize);

        let needs_semi = should_add_semicolon(
            trimmed_code,
            raw_line,
            i,
            &lines,
            start_paren,
            start_bracket,
            end_paren,
            end_bracket,
        );

        if needs_semi {
            let leading_len = raw_line.len() - raw_line.trim_start().len();
            let leading_ws = &raw_line[..leading_len];
            let new_line = if comment.is_empty() {
                format!("{}{};", leading_ws, trimmed_code)
            } else {
                format!("{}{};{}", leading_ws, trimmed_code, comment)
            };
            result_lines.push(new_line);
        } else {
            result_lines.push(raw_line.to_string());
        }

        paren_depth = end_paren.max(0);
        bracket_depth = end_bracket.max(0);
    }

    result_lines.join("\n")
}

fn scan_line_delims(line: &str) -> (usize, usize, usize, usize, bool, bool, char) {
    let mut opens_p = 0;
    let mut closes_p = 0;
    let mut opens_b = 0;
    let mut closes_b = 0;
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
                chars.next();
            } else if c == str_char {
                in_str = false;
            }
            continue;
        }

        if c == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if c == '#' {
            break;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            in_comment = true;
            continue;
        }

        if c == '"' || c == '\'' || c == '`' {
            in_str = true;
            str_char = c;
            continue;
        }

        if c == '(' {
            opens_p += 1;
        } else if c == ')' {
            closes_p += 1;
        } else if c == '[' {
            opens_b += 1;
        } else if c == ']' {
            closes_b += 1;
        }
    }

    (opens_p, closes_p, opens_b, closes_b, in_comment, in_str, str_char)
}

fn should_add_semicolon(
    trimmed_code: &str,
    raw_line: &str,
    current_idx: usize,
    all_lines: &[&str],
    _start_paren: isize,
    _start_bracket: isize,
    end_paren: isize,
    end_bracket: isize,
) -> bool {
    if trimmed_code.ends_with(';') {
        return false;
    }
    if trimmed_code.ends_with('{') || trimmed_code.ends_with('}') {
        return false;
    }
    if trimmed_code.ends_with(',') {
        return false;
    }
    if end_paren > 0 || end_bracket > 0 {
        return false;
    }
    if trimmed_code.starts_with("<?") || trimmed_code == "?>" || (trimmed_code.starts_with("#[") && trimmed_code.ends_with(']')) {
        return false;
    }

    let is_do_while_end = trimmed_code.starts_with("} while") || (trimmed_code.starts_with("while") && trimmed_code.ends_with(')'));
    if !is_do_while_end {
        if trimmed_code.starts_with("if ") || trimmed_code.starts_with("if(")
            || trimmed_code.starts_with("for ") || trimmed_code.starts_with("for(")
            || trimmed_code.starts_with("foreach ") || trimmed_code.starts_with("foreach(")
            || trimmed_code.starts_with("while ") || trimmed_code.starts_with("while(")
            || trimmed_code.starts_with("switch ") || trimmed_code.starts_with("switch(")
            || trimmed_code.starts_with("match ") || trimmed_code.starts_with("match(")
            || trimmed_code.starts_with("class ")
            || trimmed_code.starts_with("interface ")
            || trimmed_code.starts_with("trait ")
            || trimmed_code.starts_with("enum ")
            || trimmed_code == "else" || trimmed_code.starts_with("else ")
            || trimmed_code.starts_with("elseif ") || trimmed_code.starts_with("elseif(")
            || trimmed_code == "try"
            || trimmed_code.starts_with("catch ") || trimmed_code.starts_with("catch(")
            || trimmed_code == "finally"
            || trimmed_code == "do"
            || trimmed_code.starts_with("case ") || trimmed_code == "default" || trimmed_code.starts_with("default:")
        {
            return false;
        }
    }

    let op_ends = [
        "=", "=>", "+=", "-=", "*=", "/=", "%=", ".=", "??=",
        "+", "-", "*", "/", "%", ".", "->", "?->", "::",
        "&&", "||", "??", "?", ":", "|", "&", "^",
        "===", "!==", "==", "!=", "<=", ">="
    ];
    for op in &op_ends {
        if trimmed_code.ends_with(op) {
            return false;
        }
    }

    // Lookahead to next non-empty, non-comment line
    let n = all_lines.len();
    let mut next_idx = current_idx + 1;
    let mut found_next = None;
    while next_idx < n {
        let next_raw = all_lines[next_idx];
        let next_trim = next_raw.trim();
        if next_trim.is_empty() {
            next_idx += 1;
            continue;
        }
        if next_trim.starts_with("//") || next_trim.starts_with('#') || next_trim.starts_with("/*") {
            next_idx += 1;
            continue;
        }
        found_next = Some((next_raw, next_trim));
        break;
    }

    if let Some((next_raw, next_trim)) = found_next {
        if next_trim.starts_with('{') {
            return false;
        }
        if starts_with_continuation_op(next_trim) {
            return false;
        }

        let cur_indent = raw_line.len() - raw_line.trim_start().len();
        let next_indent = next_raw.len() - next_raw.trim_start().len();
        if next_indent > cur_indent {
            if !trimmed_code.starts_with("use ") && !trimmed_code.starts_with("namespace ") {
                let next_starts_statement = next_trim.starts_with("return ")
                    || next_trim.starts_with("let ")
                    || next_trim.starts_with("var ")
                    || next_trim.starts_with("if ")
                    || next_trim.starts_with("for ")
                    || next_trim.starts_with("while ")
                    || next_trim.starts_with("echo ");
                if !next_starts_statement {
                    return false;
                }
            }
        }
    }

    true
}
