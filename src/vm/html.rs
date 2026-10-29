//! HTML encoding, decoding, entity transformation, and tag stripping (`Html::`).

use std::collections::HashMap;
use std::sync::OnceLock;

/// PHP standard htmlspecialchars flag constants.
pub const ENT_NOQUOTES: i64 = 0;
pub const ENT_COMPAT: i64 = 2;
pub const ENT_QUOTES: i64 = 3;
pub const ENT_SUBSTITUTE: i64 = 8;
pub const ENT_HTML401: i64 = 0;
pub const ENT_XML1: i64 = 16;
pub const ENT_XHTML: i64 = 32;
pub const ENT_HTML5: i64 = 48;

/// Default flags in modern PHP: ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401 = 11.
pub const DEFAULT_FLAGS: i64 = ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401;

/// Converts special characters to HTML entities (`htmlspecialchars`).
pub fn special_chars(input: &str, flags: i64, double_encode: bool) -> String {
    let encode_double_quotes = (flags & ENT_COMPAT) != 0 || (flags & ENT_QUOTES) == ENT_QUOTES;
    let encode_single_quotes = (flags & ENT_QUOTES) == ENT_QUOTES;

    let mut out = String::with_capacity(input.len() + 16);
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'&' {
            if !double_encode && is_existing_entity(&bytes[i..]) {
                out.push('&');
            } else {
                out.push_str("&amp;");
            }
        } else if b == b'<' {
            out.push_str("&lt;");
        } else if b == b'>' {
            out.push_str("&gt;");
        } else if b == b'"' {
            if encode_double_quotes {
                out.push_str("&quot;");
            } else {
                out.push('"');
            }
        } else if b == b'\'' {
            if encode_single_quotes {
                out.push_str("&#039;");
            } else {
                out.push('\'');
            }
        } else {
            out.push(b as char);
        }
        i += 1;
    }

    out
}

/// Checks if bytes starting at `&` form an existing HTML entity (e.g. `&amp;`, `&#123;`, `&#x1f;`).
fn is_existing_entity(slice: &[u8]) -> bool {
    if slice.is_empty() || slice[0] != b'&' {
        return false;
    }
    let mut end = 1;
    while end < slice.len() && end < 32 {
        if slice[end] == b';' {
            let entity_body = &slice[1..end];
            if entity_body.is_empty() {
                return false;
            }
            if entity_body[0] == b'#' {
                if entity_body.len() > 1 && (entity_body[1] == b'x' || entity_body[1] == b'X') {
                    // Hex entity: &#x[0-9a-fA-F]+;
                    return entity_body[2..].iter().all(|c| c.is_ascii_hexdigit());
                } else {
                    // Decimal entity: #[0-9]+;
                    return entity_body[1..].iter().all(|c| c.is_ascii_digit());
                }
            } else {
                // Named entity: [a-zA-Z0-9]+;
                return entity_body.iter().all(|c| c.is_ascii_alphanumeric());
            }
        }
        if slice[end] == b' ' || slice[end] == b'&' || slice[end] == b'<' {
            break;
        }
        end += 1;
    }
    false
}

/// Decodes special HTML entities back to characters (`htmlspecialchars_decode`).
pub fn special_chars_decode(input: &str, flags: i64) -> String {
    let decode_double_quotes = (flags & ENT_COMPAT) != 0 || (flags & ENT_QUOTES) == ENT_QUOTES;
    let decode_single_quotes = (flags & ENT_QUOTES) == ENT_QUOTES;

    let mut out = input.to_string();
    if decode_double_quotes {
        out = out.replace("&quot;", "\"");
    }
    if decode_single_quotes {
        out = out.replace("&#039;", "'").replace("&#39;", "'").replace("&apos;", "'");
    }
    out = out.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
    out
}

/// Strips HTML, XML, and PHP tags from a string (`strip_tags`).
///
/// If `allowed_tags` is specified, tags matching allowed tag names are retained.
pub fn strip_tags(input: &str, allowed_tags: Option<&[String]>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    let mut in_quote: Option<char> = None;
    let mut current_tag = String::new();

    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if !in_tag {
            if c == '<' {
                // Check if this is a comment `<!--` or PHP tag `<?`
                if i + 3 < chars.len() && chars[i + 1] == '!' && chars[i + 2] == '-' && chars[i + 3] == '-' {
                    // Skip HTML comment until `-->`
                    i += 4;
                    while i + 2 < chars.len() && !(chars[i] == '-' && chars[i + 1] == '-' && chars[i + 2] == '>') {
                        i += 1;
                    }
                    i += 3;
                    continue;
                } else if i + 1 < chars.len() && chars[i + 1] == '?' {
                    // Skip PHP tag `<?...?>`
                    i += 2;
                    while i + 1 < chars.len() && !(chars[i] == '?' && chars[i + 1] == '>') {
                        i += 1;
                    }
                    i += 2;
                    continue;
                } else {
                    in_tag = true;
                    current_tag.clear();
                    current_tag.push(c);
                    i += 1;
                    continue;
                }
            } else {
                out.push(c);
            }
        } else {
            // Inside a tag
            current_tag.push(c);

            if let Some(q) = in_quote {
                if c == q {
                    in_quote = None;
                }
            } else if c == '"' || c == '\'' {
                in_quote = Some(c);
            } else if c == '>' {
                in_tag = false;
                if let Some(allowed) = allowed_tags {
                    if is_tag_allowed(&current_tag, allowed) {
                        out.push_str(&current_tag);
                    }
                }
                current_tag.clear();
            }
        }

        i += 1;
    }

    out
}

/// Checks if tag name in `<name attr="val">` or `</name>` is in the allowed list.
fn is_tag_allowed(tag: &str, allowed: &[String]) -> bool {
    let trimmed = tag.trim_start_matches('<').trim_end_matches('>').trim();
    let tag_name = trimmed
        .trim_start_matches('/')
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    if tag_name.is_empty() {
        return false;
    }

    allowed.iter().any(|a| {
        let clean = a.trim_matches(['<', '>', '/', ' ']).to_ascii_lowercase();
        clean == tag_name
    })
}

/// Parse allowed tags from a string format like `"<p><a><b>"` or `"p,a,b"`.
pub fn parse_allowed_tags_string(s: &str) -> Vec<String> {
    if s.contains('<') {
        let mut tags = Vec::new();
        let mut cur = String::new();
        let mut inside = false;
        for c in s.chars() {
            if c == '<' {
                inside = true;
                cur.clear();
            } else if c == '>' {
                inside = false;
                if !cur.is_empty() {
                    tags.push(cur.clone());
                }
            } else if inside {
                cur.push(c);
            }
        }
        tags
    } else {
        s.split([',', ' ', '|'])
            .filter(|p| !p.is_empty())
            .map(|p| p.trim().to_string())
            .collect()
    }
}

/// Global HTML entity mapping table.
static ENTITY_MAP: OnceLock<(HashMap<&'static str, &'static str>, HashMap<char, &'static str>)> =
    OnceLock::new();

fn get_entity_maps() -> &'static (HashMap<&'static str, &'static str>, HashMap<char, &'static str>) {
    ENTITY_MAP.get_or_init(|| {
        let pairs: &[(&'static str, char)] = &[
            ("quot", '"'),
            ("amp", '&'),
            ("apos", '\''),
            ("lt", '<'),
            ("gt", '>'),
            ("nbsp", '\u{00A0}'),
            ("iexcl", '¡'),
            ("cent", '¢'),
            ("pound", '£'),
            ("curren", '¤'),
            ("yen", '¥'),
            ("brvbar", '¦'),
            ("sect", '§'),
            ("uml", '¨'),
            ("copy", '©'),
            ("ordf", 'ª'),
            ("laquo", '«'),
            ("not", '¬'),
            ("reg", '®'),
            ("macr", '¯'),
            ("deg", '°'),
            ("plusmn", '±'),
            ("sup2", '²'),
            ("sup3", '³'),
            ("acute", '´'),
            ("micro", 'µ'),
            ("para", '¶'),
            ("middot", '·'),
            ("cedil", '¸'),
            ("sup1", '¹'),
            ("ordm", 'º'),
            ("raquo", '»'),
            ("frac14", '¼'),
            ("frac12", '½'),
            ("frac34", '¾'),
            ("iquest", '¿'),
            ("Agrave", 'À'),
            ("Aacute", 'Á'),
            ("Acirc", 'Â'),
            ("Atilde", 'Ã'),
            ("Auml", 'Ä'),
            ("Aring", 'Å'),
            ("AElig", 'Æ'),
            ("Ccedil", 'Ç'),
            ("Egrave", 'È'),
            ("Eacute", 'É'),
            ("Ecirc", 'Ê'),
            ("Euml", 'Ë'),
            ("Igrave", 'Ì'),
            ("Iacute", 'Í'),
            ("Icirc", 'Î'),
            ("Iuml", 'Ï'),
            ("ETH", 'Ð'),
            ("Ntilde", 'Ñ'),
            ("Ograve", 'Ò'),
            ("Oacute", 'Ó'),
            ("Ocirc", 'Ô'),
            ("Otilde", 'Õ'),
            ("Ouml", 'Ö'),
            ("times", '×'),
            ("Oslash", 'Ø'),
            ("Ugrave", 'Ù'),
            ("Uacute", 'Ú'),
            ("Ucirc", 'Û'),
            ("Uuml", 'Ü'),
            ("Yacute", 'Ý'),
            ("THORN", 'Þ'),
            ("szlig", 'ß'),
            ("agrave", 'à'),
            ("aacute", 'á'),
            ("acirc", 'â'),
            ("atilde", 'ã'),
            ("auml", 'ä'),
            ("aring", 'å'),
            ("aelig", 'æ'),
            ("ccedil", 'ç'),
            ("egrave", 'è'),
            ("eacute", 'é'),
            ("ecirc", 'ê'),
            ("euml", 'ë'),
            ("igrave", 'ì'),
            ("iacute", 'í'),
            ("icirc", 'î'),
            ("iuml", 'ï'),
            ("eth", 'ð'),
            ("ntilde", 'ñ'),
            ("ograve", 'ò'),
            ("oacute", 'ó'),
            ("ocirc", 'ô'),
            ("otilde", 'õ'),
            ("ouml", 'ö'),
            ("divide", '÷'),
            ("oslash", 'ø'),
            ("ugrave", 'ù'),
            ("uacute", 'ú'),
            ("ucirc", 'û'),
            ("uuml", 'ü'),
            ("yacute", 'ý'),
            ("thorn", 'þ'),
            ("yuml", 'ÿ'),
            ("euro", '€'),
            ("trade", '™'),
        ];

        let mut name_to_char = HashMap::new();
        let mut char_to_name = HashMap::new();

        for &(name, ch) in pairs {
            name_to_char.insert(name, ch);
            char_to_name.insert(ch, name);
        }

        // Special handling for HTML characters
        let mut name_to_str = HashMap::new();
        for (k, v) in name_to_char {
            let s: &'static str = Box::leak(v.to_string().into_boxed_str());
            name_to_str.insert(k, s);
        }

        (name_to_str, char_to_name)
    })
}

/// Convert all applicable characters to HTML entities (`htmlentities`).
pub fn entities(input: &str, flags: i64, double_encode: bool) -> String {
    let (_, char_to_name) = get_entity_maps();
    let encode_double_quotes = (flags & ENT_COMPAT) != 0 || (flags & ENT_QUOTES) == ENT_QUOTES;
    let encode_single_quotes = (flags & ENT_QUOTES) == ENT_QUOTES;

    let mut out = String::with_capacity(input.len() + 32);
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '&' {
            let rest: String = chars[i..].iter().collect();
            if !double_encode && is_existing_entity(rest.as_bytes()) {
                out.push('&');
            } else {
                out.push_str("&amp;");
            }
        } else if ch == '<' {
            out.push_str("&lt;");
        } else if ch == '>' {
            out.push_str("&gt;");
        } else if ch == '"' {
            if encode_double_quotes {
                out.push_str("&quot;");
            } else {
                out.push('"');
            }
        } else if ch == '\'' {
            if encode_single_quotes {
                out.push_str("&#039;");
            } else {
                out.push('\'');
            }
        } else if let Some(entity_name) = char_to_name.get(&ch) {
            out.push('&');
            out.push_str(entity_name);
            out.push(';');
        } else {
            out.push(ch);
        }
        i += 1;
    }

    out
}

/// Decodes all HTML entities to their corresponding characters (`html_entity_decode`).
pub fn entity_decode(input: &str, flags: i64) -> String {
    let (name_to_str, _) = get_entity_maps();
    let decode_double_quotes = (flags & ENT_COMPAT) != 0 || (flags & ENT_QUOTES) == ENT_QUOTES;
    let decode_single_quotes = (flags & ENT_QUOTES) == ENT_QUOTES;

    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '&' {
            if let Some(semi_pos) = chars[i..].iter().position(|&c| c == ';') {
                let entity_raw: String = chars[i + 1..i + semi_pos].iter().collect();
                if let Some(rest) = entity_raw.strip_prefix('#') {
                    // Numeric entity: &#123; or &#x7b;
                    let code_point = if let Some(hex_str) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
                        u32::from_str_radix(hex_str, 16).ok()
                    } else {
                        rest.parse::<u32>().ok()
                    };

                    if let Some(cp) = code_point.and_then(char::from_u32) {
                        if cp == '"' && !decode_double_quotes {
                            out.push_str(&format!("&#{};", rest));
                        } else if cp == '\'' && !decode_single_quotes {
                            out.push_str(&format!("&#{};", rest));
                        } else {
                            out.push(cp);
                        }
                        i += semi_pos + 1;
                        continue;
                    }
                } else if entity_raw == "quot" && !decode_double_quotes {
                    out.push_str("&quot;");
                    i += semi_pos + 1;
                    continue;
                } else if (entity_raw == "apos" || entity_raw == "#039" || entity_raw == "#39") && !decode_single_quotes {
                    out.push_str("&apos;");
                    i += semi_pos + 1;
                    continue;
                } else if let Some(val) = name_to_str.get(entity_raw.as_str()) {
                    out.push_str(val);
                    i += semi_pos + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_special_chars() {
        assert_eq!(
            special_chars("A 'quote' is \"bold\" & <tag>", DEFAULT_FLAGS, true),
            "A &#039;quote&#039; is &quot;bold&quot; &amp; &lt;tag&gt;"
        );
        // double_encode = false
        assert_eq!(
            special_chars("Already &amp; encoded & new", DEFAULT_FLAGS, false),
            "Already &amp; encoded &amp; new"
        );
    }

    #[test]
    fn test_special_chars_decode() {
        let encoded = "A &#039;quote&#039; is &quot;bold&quot; &amp; &lt;tag&gt;";
        assert_eq!(
            special_chars_decode(encoded, DEFAULT_FLAGS),
            "A 'quote' is \"bold\" & <tag>"
        );
    }

    #[test]
    fn test_strip_tags() {
        let html = "<p>Test paragraph.</p><!-- Comment --> <a href='#fragment'>Other text</a>";
        assert_eq!(strip_tags(html, None), "Test paragraph. Other text");

        let allowed = vec!["p".to_string()];
        assert_eq!(
            strip_tags(html, Some(&allowed)),
            "<p>Test paragraph.</p> Other text"
        );
    }

    #[test]
    fn test_entities_and_decode() {
        let text = "Café & Bakery © 2026 <World>";
        let enc = entities(text, DEFAULT_FLAGS, true);
        assert!(enc.contains("&copy;"));
        assert!(enc.contains("&eacute;"));
        assert!(enc.contains("&lt;World&gt;"));

        let dec = entity_decode(&enc, DEFAULT_FLAGS);
        assert_eq!(dec, text);
    }
}
