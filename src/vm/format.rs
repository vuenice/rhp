//! High-performance string formatting engine supporting Rust-style template formatting
//! (`{:.2}`, `{:05}`, `{:x}`, `{:b}`, etc.) directly mapping to Rust print/println semantics.

use crate::vm::value::PhpValue;

/// Format arguments for `Sys::echo`, `Sys::print`, `Sys::println`, and `Str::format`.
pub fn format_sys_args(args: &[PhpValue]) -> String {
    if args.is_empty() {
        return String::new();
    }

    if args.len() == 1 {
        if let PhpValue::String(s) = &args[0] {
            let str_val = s.to_string();
            if str_val.contains("{{") || str_val.contains("}}") {
                return str_val.replace("{{", "{").replace("}}", "}");
            }
            return str_val;
        }
        return args[0].to_php_string();
    }

    let template = args[0].to_php_string();
    let format_args = &args[1..];

    if template.contains('{') {
        format_rust_template(&template, format_args)
    } else {
        // No placeholders: concatenate all arguments
        args.iter().map(|a| a.to_php_string()).collect::<String>()
    }
}

/// Format a Rust-style format template (`"Age: {}, Name: {}"`, `"{:.2}"`, `"{:05}"`, `"{:x}"`, `"{:b}"`).
pub fn format_rust_template(template: &str, args: &[PhpValue]) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    let mut auto_idx = 0;

    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') {
                chars.next();
                result.push('{');
                continue;
            }

            let mut inside = String::new();
            let mut closed = false;
            while let Some(&inner_c) = chars.peek() {
                chars.next();
                if inner_c == '}' {
                    closed = true;
                    break;
                }
                inside.push(inner_c);
            }

            if !closed {
                result.push('{');
                result.push_str(&inside);
                continue;
            }

            let (arg_selector, spec_str) = if let Some(colon_pos) = inside.find(':') {
                (&inside[..colon_pos], &inside[colon_pos + 1..])
            } else {
                (inside.as_str(), "")
            };

            let target_idx = if arg_selector.is_empty() {
                let idx = auto_idx;
                auto_idx += 1;
                Some(idx)
            } else if let Ok(parsed) = arg_selector.parse::<usize>() {
                Some(parsed)
            } else {
                None
            };

            if let Some(idx) = target_idx {
                if let Some(val) = args.get(idx) {
                    result.push_str(&format_spec_value(val, spec_str));
                }
            }
        } else if c == '}' {
            if chars.peek() == Some(&'}') {
                chars.next();
                result.push('}');
            } else {
                result.push('}');
            }
        } else {
            result.push(c);
        }
    }

    result
}

#[derive(Debug, Default)]
struct FormatSpec {
    fill: Option<char>,
    align: Option<char>,
    sign: Option<char>,
    alternate: bool,
    zero_pad: bool,
    width: Option<usize>,
    precision: Option<usize>,
    format_type: Option<char>,
}

impl FormatSpec {
    fn parse(spec: &str) -> Self {
        let mut parsed = Self::default();
        if spec.is_empty() {
            return parsed;
        }

        let chars: Vec<char> = spec.chars().collect();
        let mut i = 0;
        let n = chars.len();

        // 1. Fill and align
        if i + 1 < n && (chars[i + 1] == '<' || chars[i + 1] == '^' || chars[i + 1] == '>') {
            parsed.fill = Some(chars[i]);
            parsed.align = Some(chars[i + 1]);
            i += 2;
        } else if i < n && (chars[i] == '<' || chars[i] == '^' || chars[i] == '>') {
            parsed.align = Some(chars[i]);
            i += 1;
        }

        // 2. Sign
        if i < n && (chars[i] == '+' || chars[i] == '-') {
            if chars[i] == '+' {
                parsed.sign = Some('+');
            }
            i += 1;
        }

        // 3. Alternate '#'
        if i < n && chars[i] == '#' {
            parsed.alternate = true;
            i += 1;
        }

        // 4. Zero-pad '0'
        if i < n && chars[i] == '0' {
            parsed.zero_pad = true;
            i += 1;
        }

        // 5. Width
        if i < n && chars[i].is_ascii_digit() {
            let start = i;
            while i < n && chars[i].is_ascii_digit() {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            parsed.width = s.parse::<usize>().ok();
        }

        // 6. Precision
        if i < n && chars[i] == '.' {
            i += 1;
            let start = i;
            while i < n && chars[i].is_ascii_digit() {
                i += 1;
            }
            if start < i {
                let s: String = chars[start..i].iter().collect();
                parsed.precision = s.parse::<usize>().ok();
            } else {
                parsed.precision = Some(0);
            }
        }

        // 7. Type
        if i < n {
            parsed.format_type = Some(chars[i]);
        }

        parsed
    }
}

fn format_spec_value(val: &PhpValue, spec_str: &str) -> String {
    if spec_str.is_empty() {
        return php_value_to_format_string(val);
    }

    let spec = FormatSpec::parse(spec_str);

    match spec.format_type {
        Some('x') | Some('X') => {
            let is_upper = spec.format_type == Some('X');
            let num = val.to_i64().unwrap_or(0);
            let raw_hex = if is_upper {
                format!("{:X}", num as u64)
            } else {
                format!("{:x}", num as u64)
            };
            let prefix = if spec.alternate {
                if is_upper { "0X" } else { "0x" }
            } else {
                ""
            };
            if spec.zero_pad {
                let width = spec.width.unwrap_or(0);
                let current_len = prefix.len() + raw_hex.len();
                let zeros = if width > current_len { width - current_len } else { 0 };
                format!("{}{}{}", prefix, "0".repeat(zeros), raw_hex)
            } else {
                let full = format!("{}{}", prefix, raw_hex);
                apply_alignment(&full, &spec, false)
            }
        }
        Some('b') | Some('B') => {
            let num = val.to_i64().unwrap_or(0);
            let raw_bin = format!("{:b}", num as u64);
            let prefix = if spec.alternate { "0b" } else { "" };
            if spec.zero_pad {
                let width = spec.width.unwrap_or(0);
                let current_len = prefix.len() + raw_bin.len();
                let zeros = if width > current_len { width - current_len } else { 0 };
                format!("{}{}{}", prefix, "0".repeat(zeros), raw_bin)
            } else {
                let full = format!("{}{}", prefix, raw_bin);
                apply_alignment(&full, &spec, false)
            }
        }
        Some('o') => {
            let num = val.to_i64().unwrap_or(0);
            let raw_oct = format!("{:o}", num as u64);
            let prefix = if spec.alternate { "0o" } else { "" };
            if spec.zero_pad {
                let width = spec.width.unwrap_or(0);
                let current_len = prefix.len() + raw_oct.len();
                let zeros = if width > current_len { width - current_len } else { 0 };
                format!("{}{}{}", prefix, "0".repeat(zeros), raw_oct)
            } else {
                let full = format!("{}{}", prefix, raw_oct);
                apply_alignment(&full, &spec, false)
            }
        }
        Some('e') | Some('E') => {
            let num = val.to_f64().unwrap_or(0.0);
            let is_upper = spec.format_type == Some('E');
            let formatted = if let Some(prec) = spec.precision {
                if is_upper {
                    format!("{num:.prec$E}")
                } else {
                    format!("{num:.prec$e}")
                }
            } else if is_upper {
                format!("{num:E}")
            } else {
                format!("{num:e}")
            };
            apply_alignment(&formatted, &spec, true)
        }
        Some('?') => {
            format!("{:?}", val)
        }
        _ => {
            // Check if precision is specified for float or string
            if let Some(prec) = spec.precision {
                if spec.format_type == Some('s') {
                    let s = php_value_to_format_string(val);
                    let truncated: String = s.chars().take(prec).collect();
                    return apply_alignment(&truncated, &spec, false);
                }

                if let Some(num) = val.to_f64() {
                    let is_neg = num < 0.0;
                    let abs_num = num.abs();
                    let abs_str = format!("{abs_num:.prec$}");
                    let sign_str = if is_neg {
                        "-"
                    } else if spec.sign == Some('+') {
                        "+"
                    } else {
                        ""
                    };

                    if spec.zero_pad {
                        let width = spec.width.unwrap_or(0);
                        let current_len = sign_str.len() + abs_str.len();
                        let zeros = if width > current_len { width - current_len } else { 0 };
                        return format!("{}{}{}", sign_str, "0".repeat(zeros), abs_str);
                    } else {
                        let full = format!("{}{}", sign_str, abs_str);
                        return apply_alignment(&full, &spec, true);
                    }
                } else {
                    let s = php_value_to_format_string(val);
                    let truncated: String = s.chars().take(prec).collect();
                    return apply_alignment(&truncated, &spec, false);
                }
            }

            // Zero-padding for numbers (e.g. {:05} with 42 -> 00042)
            if spec.zero_pad {
                if let Some(num) = val.to_i64() {
                    let is_neg = num < 0;
                    let abs_str = (num.abs() as u64).to_string();
                    let sign_str = if is_neg {
                        "-"
                    } else if spec.sign == Some('+') {
                        "+"
                    } else {
                        ""
                    };
                    let width = spec.width.unwrap_or(0);
                    let current_len = sign_str.len() + abs_str.len();
                    let zeros = if width > current_len { width - current_len } else { 0 };
                    return format!("{}{}{}", sign_str, "0".repeat(zeros), abs_str);
                } else if let Some(num) = val.to_f64() {
                    let is_neg = num < 0.0;
                    let abs_num = num.abs();
                    let abs_str = abs_num.to_string();
                    let sign_str = if is_neg {
                        "-"
                    } else if spec.sign == Some('+') {
                        "+"
                    } else {
                        ""
                    };
                    let width = spec.width.unwrap_or(0);
                    let current_len = sign_str.len() + abs_str.len();
                    let zeros = if width > current_len { width - current_len } else { 0 };
                    return format!("{}{}{}", sign_str, "0".repeat(zeros), abs_str);
                }
            }

            // Standard string/number formatting with width / align / sign
            let mut base = php_value_to_format_string(val);
            let is_number = val.to_f64().is_some();
            if spec.sign == Some('+') {
                if let Some(n) = val.to_f64() {
                    if n >= 0.0 {
                        base = format!("+{base}");
                    }
                }
            }
            apply_alignment(&base, &spec, is_number)
        }
    }
}

fn apply_alignment(s: &str, spec: &FormatSpec, is_numeric: bool) -> String {
    let width = match spec.width {
        Some(w) if w > s.len() => w,
        _ => return s.to_string(),
    };
    let fill = spec.fill.unwrap_or(' ');
    let needed = width - s.len();
    match spec.align {
        Some('<') => {
            let mut out = s.to_string();
            for _ in 0..needed {
                out.push(fill);
            }
            out
        }
        Some('^') => {
            let left_pad = needed / 2;
            let right_pad = needed - left_pad;
            let mut out = String::with_capacity(width);
            for _ in 0..left_pad {
                out.push(fill);
            }
            out.push_str(s);
            for _ in 0..right_pad {
                out.push(fill);
            }
            out
        }
        Some('>') => {
            let mut out = String::with_capacity(width);
            for _ in 0..needed {
                out.push(fill);
            }
            out.push_str(s);
            out
        }
        None => {
            if is_numeric {
                let mut out = String::with_capacity(width);
                for _ in 0..needed {
                    out.push(fill);
                }
                out.push_str(s);
                out
            } else {
                let mut out = s.to_string();
                for _ in 0..needed {
                    out.push(fill);
                }
                out
            }
        }
        _ => s.to_string(),
    }
}

pub fn php_value_to_format_string(val: &PhpValue) -> String {
    match val {
        PhpValue::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
        PhpValue::Null => "null".to_string(),
        PhpValue::Int(n) => n.to_string(),
        PhpValue::Float(f) => {
            if f.fract() == 0.0 && !f.is_infinite() && !f.is_nan() {
                f.to_string()
            } else {
                f.to_string()
            }
        }
        PhpValue::RustInt { value, .. } => value.to_string(),
        PhpValue::RustFloat { value, .. } => value.to_string(),
        PhpValue::String(s) => s.to_string(),
        _ => val.to_php_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_rust_templates() {
        assert_eq!(
            format_rust_template("Age: {}, Name: {}", &[PhpValue::Int(30), PhpValue::string("Bob")]),
            "Age: 30, Name: Bob"
        );
        assert_eq!(
            format_rust_template("{:.2}", &[PhpValue::Float(3.141592)]),
            "3.14"
        );
        assert_eq!(
            format_rust_template("{:05}", &[PhpValue::Int(42)]),
            "00042"
        );
        assert_eq!(
            format_rust_template("{:05}", &[PhpValue::Int(-42)]),
            "-0042"
        );
        assert_eq!(
            format_rust_template("{:x}", &[PhpValue::Int(255)]),
            "ff"
        );
        assert_eq!(
            format_rust_template("{:X}", &[PhpValue::Int(255)]),
            "FF"
        );
        assert_eq!(
            format_rust_template("{:b}", &[PhpValue::Int(5)]),
            "101"
        );
        assert_eq!(
            format_rust_template("{:#x}", &[PhpValue::Int(255)]),
            "0xff"
        );
        assert_eq!(
            format_rust_template("{:#b}", &[PhpValue::Int(5)]),
            "0b101"
        );
        assert_eq!(
            format_rust_template("{:08b}", &[PhpValue::Int(5)]),
            "00000101"
        );
        assert_eq!(
            format_rust_template("{{escaped}}", &[]),
            "{escaped}"
        );
        assert_eq!(
            format_rust_template("{1} {0}", &[PhpValue::string("world"), PhpValue::string("hello")]),
            "hello world"
        );
    }
}
