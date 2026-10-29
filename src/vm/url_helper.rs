//! URL parsing, query string building, and base64 helper utilities (`Url::`).

use base64::prelude::*;
use indexmap::IndexMap;
use url::Url;

use crate::vm::value::PhpValue;

/// PHP parse_url component constants.
pub const PHP_URL_SCHEME: i64 = 0;
pub const PHP_URL_HOST: i64 = 1;
pub const PHP_URL_PORT: i64 = 2;
pub const PHP_URL_USER: i64 = 3;
pub const PHP_URL_PASS: i64 = 4;
pub const PHP_URL_PATH: i64 = 5;
pub const PHP_URL_QUERY: i64 = 6;
pub const PHP_URL_FRAGMENT: i64 = 7;

/// PHP query encoding type constants.
pub const PHP_QUERY_RFC1738: i64 = 1;
pub const PHP_QUERY_RFC3986: i64 = 2;

/// Parse a URL into its constituent components (`parse_url`).
pub fn parse_url_components(url_str: &str) -> Option<IndexMap<String, PhpValue>> {
    let mut map = IndexMap::new();

    if let Ok(parsed) = Url::parse(url_str) {
        // Absolute URL with scheme
        map.insert("scheme".to_string(), PhpValue::string(parsed.scheme()));

        if let Some(host) = parsed.host_str() {
            map.insert("host".to_string(), PhpValue::string(host));
        }

        if let Some(port) = parsed.port() {
            map.insert("port".to_string(), PhpValue::Int(port as i32));
        }

        let user = parsed.username();
        if !user.is_empty() {
            map.insert("user".to_string(), PhpValue::string(user));
        }

        if let Some(pass) = parsed.password() {
            map.insert("pass".to_string(), PhpValue::string(pass));
        }

        let path = parsed.path();
        if !path.is_empty() && path != "/" {
            map.insert("path".to_string(), PhpValue::string(path));
        } else if url_str.ends_with('/') || path == "/" {
            map.insert("path".to_string(), PhpValue::string(path));
        }

        if let Some(query) = parsed.query() {
            map.insert("query".to_string(), PhpValue::string(query));
        }

        if let Some(frag) = parsed.fragment() {
            map.insert("fragment".to_string(), PhpValue::string(frag));
        }

        return Some(map);
    }

    // Attempt relative / scheme-less URL parsing
    parse_relative_url(url_str, &mut map);
    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

fn parse_relative_url(url_str: &str, map: &mut IndexMap<String, PhpValue>) {
    let mut rest = url_str;

    // Fragment `#...`
    if let Some(pos) = rest.find('#') {
        map.insert("fragment".to_string(), PhpValue::string(&rest[pos + 1..]));
        rest = &rest[..pos];
    }

    // Query `?...`
    if let Some(pos) = rest.find('?') {
        map.insert("query".to_string(), PhpValue::string(&rest[pos + 1..]));
        rest = &rest[..pos];
    }

    // Check if network-path reference `//host[:port]/path`
    if let Some(stripped) = rest.strip_prefix("//") {
        let (host_part, path_part) = match stripped.find('/') {
            Some(p) => (&stripped[..p], &stripped[p..]),
            None => (stripped, ""),
        };

        let user_host = if let Some(at) = host_part.find('@') {
            let auth = &host_part[..at];
            let host_only = &host_part[at + 1..];
            if let Some(colon) = auth.find(':') {
                map.insert("user".to_string(), PhpValue::string(&auth[..colon]));
                map.insert("pass".to_string(), PhpValue::string(&auth[colon + 1..]));
            } else {
                map.insert("user".to_string(), PhpValue::string(auth));
            }
            host_only
        } else {
            host_part
        };

        if let Some(colon) = user_host.rfind(':') {
            let host_only = &user_host[..colon];
            if let Ok(port_num) = user_host[colon + 1..].parse::<i32>() {
                map.insert("host".to_string(), PhpValue::string(host_only));
                map.insert("port".to_string(), PhpValue::Int(port_num));
            } else {
                map.insert("host".to_string(), PhpValue::string(user_host));
            }
        } else if !user_host.is_empty() {
            map.insert("host".to_string(), PhpValue::string(user_host));
        }

        if !path_part.is_empty() {
            map.insert("path".to_string(), PhpValue::string(path_part));
        }
        return;
    }

    if !rest.is_empty() {
        map.insert("path".to_string(), PhpValue::string(rest));
    }
}

/// Query component lookup helper.
pub fn get_url_component(url_str: &str, component: i64) -> Option<PhpValue> {
    let map = parse_url_components(url_str)?;
    let key = match component {
        PHP_URL_SCHEME => "scheme",
        PHP_URL_HOST => "host",
        PHP_URL_PORT => "port",
        PHP_URL_USER => "user",
        PHP_URL_PASS => "pass",
        PHP_URL_PATH => "path",
        PHP_URL_QUERY => "query",
        PHP_URL_FRAGMENT => "fragment",
        _ => return None,
    };
    map.get(key).cloned()
}

/// Builds URL-encoded query string (`http_build_query`).
pub fn http_build_query(
    data: &PhpValue,
    numeric_prefix: &str,
    arg_separator: &str,
    encoding_type: i64,
) -> String {
    let mut pairs = Vec::new();
    flatten_query_value(data, "", numeric_prefix, encoding_type, &mut pairs);

    let sep = if arg_separator.is_empty() { "&" } else { arg_separator };
    pairs.join(sep)
}

fn flatten_query_value(
    val: &PhpValue,
    prefix: &str,
    numeric_prefix: &str,
    encoding_type: i64,
    out: &mut Vec<String>,
) {
    match val {
        PhpValue::Array(arr) => {
            for (idx, v) in arr.borrow().iter().enumerate() {
                let key_str = idx.to_string();
                let key_name = build_nested_key(prefix, &key_str, numeric_prefix);
                flatten_query_value(v, &key_name, numeric_prefix, encoding_type, out);
            }
        }
        PhpValue::List(items) => {
            for (idx, v) in items.borrow().iter().enumerate() {
                let key_str = idx.to_string();
                let key_name = build_nested_key(prefix, &key_str, numeric_prefix);
                flatten_query_value(v, &key_name, numeric_prefix, encoding_type, out);
            }
        }
        PhpValue::Dict(items) => {
            for (k, v) in items.borrow().iter() {
                let key_name = build_nested_key(prefix, k, numeric_prefix);
                flatten_query_value(v, &key_name, numeric_prefix, encoding_type, out);
            }
        }
        PhpValue::HashMap(map) => {
            for (k, v) in map.borrow().iter() {
                let k_str = k.to_php_string();
                let key_name = build_nested_key(prefix, &k_str, numeric_prefix);
                flatten_query_value(v, &key_name, numeric_prefix, encoding_type, out);
            }
        }
        PhpValue::Set(set) => {
            for (idx, v) in set.borrow().iter().enumerate() {
                let key_str = idx.to_string();
                let key_name = build_nested_key(prefix, &key_str, numeric_prefix);
                flatten_query_value(v, &key_name, numeric_prefix, encoding_type, out);
            }
        }
        scalar => {
            if !prefix.is_empty() {
                let val_str = scalar.to_php_string();
                let encoded_key = encode_query_str(prefix, encoding_type);
                let encoded_val = encode_query_str(&val_str, encoding_type);
                out.push(format!("{}={}", encoded_key, encoded_val));
            }
        }
    }
}

fn build_nested_key(parent: &str, current: &str, numeric_prefix: &str) -> String {
    if parent.is_empty() {
        if current.chars().all(|c| c.is_ascii_digit()) && !numeric_prefix.is_empty() {
            format!("{}{}", numeric_prefix, current)
        } else {
            current.to_string()
        }
    } else {
        format!("{}[{}]", parent, current)
    }
}

fn encode_query_str(s: &str, encoding_type: i64) -> String {
    if encoding_type == PHP_QUERY_RFC3986 {
        // RFC 3986: percent encode spaces as %20
        urlencoding::encode(s).into_owned()
    } else {
        // RFC 1738: spaces as '+'
        let enc = urlencoding::encode(s);
        enc.replace("%20", "+")
    }
}

/// URL decode string (`urldecode` or `rawurldecode`).
pub fn url_decode(s: &str, raw: bool) -> String {
    let input = if raw { s } else { &s.replace('+', " ") };
    urlencoding::decode(input).unwrap_or_else(|_| s.into()).into_owned()
}

/// Base64 encode string bytes.
pub fn base64_encode(data: &[u8]) -> String {
    BASE64_STANDARD.encode(data)
}

/// Base64 decode string.
pub fn base64_decode(data: &str, strict: bool) -> Result<Vec<u8>, String> {
    let cleaned = if strict {
        if data.contains(|c: char| c.is_whitespace()) {
            return Err("Whitespace not allowed in strict base64 decode".to_string());
        }
        data
    } else {
        data.trim()
    };
    BASE64_STANDARD.decode(cleaned).map_err(|e| e.to_string())
}

/// URL-safe base64 encode without padding.
pub fn base64_url_encode(data: &[u8]) -> String {
    BASE64_URL_SAFE_NO_PAD.encode(data)
}

/// URL-safe base64 decode.
pub fn base64_url_decode(data: &str) -> Result<Vec<u8>, String> {
    BASE64_URL_SAFE_NO_PAD.decode(data.trim()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_url() {
        let u = "https://user:secret@example.com:8443/api/v1/users?role=admin&active=1#details";
        let parsed = parse_url_components(u).unwrap();
        assert_eq!(parsed.get("scheme").unwrap().to_php_string(), "https");
        assert_eq!(parsed.get("host").unwrap().to_php_string(), "example.com");
        assert_eq!(parsed.get("port").unwrap().to_i64(), Some(8443));
        assert_eq!(parsed.get("user").unwrap().to_php_string(), "user");
        assert_eq!(parsed.get("pass").unwrap().to_php_string(), "secret");
        assert_eq!(parsed.get("path").unwrap().to_php_string(), "/api/v1/users");
        assert_eq!(parsed.get("query").unwrap().to_php_string(), "role=admin&active=1");
        assert_eq!(parsed.get("fragment").unwrap().to_php_string(), "details");

        // Relative URL
        let rel = "/path/to/resource?sort=desc#top";
        let rel_parsed = parse_url_components(rel).unwrap();
        assert_eq!(rel_parsed.get("path").unwrap().to_php_string(), "/path/to/resource");
        assert_eq!(rel_parsed.get("query").unwrap().to_php_string(), "sort=desc");
        assert_eq!(rel_parsed.get("fragment").unwrap().to_php_string(), "top");
    }

    #[test]
    fn test_http_build_query() {
        let mut dict = IndexMap::new();
        dict.insert("foo".to_string(), PhpValue::string("bar"));
        dict.insert("baz".to_string(), PhpValue::string("hello world"));

        let val = PhpValue::dict_from_map(dict);
        let q = http_build_query(&val, "", "&", PHP_QUERY_RFC1738);
        assert_eq!(q, "foo=bar&baz=hello+world");

        let q_rfc = http_build_query(&val, "", "&", PHP_QUERY_RFC3986);
        assert_eq!(q_rfc, "foo=bar&baz=hello%20world");
    }

    #[test]
    fn test_base64() {
        let orig = b"Hello RHP World!";
        let enc = base64_encode(orig);
        assert_eq!(enc, "SGVsbG8gUkhQIFdvcmxkIQ==");

        let dec = base64_decode(&enc, true).unwrap();
        assert_eq!(dec, orig);
    }
}
