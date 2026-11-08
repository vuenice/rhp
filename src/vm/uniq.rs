//! Uniq Facade - High-Performance Unique Identifier Generator
//!
//! Provides static utility helpers for generating unique identifiers:
//! - `Uniq::uniqid(?string $prefix = "", bool $more_entropy = false): string`
//!   PHP-compatible microsecond timestamp generator with optional LCG entropy.
//!   Default mode returns 13-character hex string (8 chars seconds + 5 chars microseconds).
//!   With entropy returns 23-character string (13 chars time + dot + 9 LCG pseudo-random digits).
//! - `Uniq::uuid4(): string` (alias `v4()`, `uuid()`)
//!   RFC 4122 / RFC 9562 Version 4 random UUID.
//! - `Uniq::uuid5(string $namespace, string $name): string` (alias `v5()`)
//!   RFC 4122 / RFC 9562 Version 5 name-based UUID using SHA-1.
//!   Supports standard namespaces: `"dns"`, `"url"`, `"oid"`, `"x500"`, `"nil"` or custom UUIDs.
//! - `Uniq::uuid7(?int $timestamp_ms = null): string` (alias `v7()`)
//!   RFC 9562 Version 7 time-ordered UUID with millisecond precision and monotonic ordering.
//! - `Uniq::uuid3(string $namespace, string $name): string` (alias `v3()`)
//!   RFC 4122 Version 3 name-based UUID using MD5.
//! - Helper functions:
//!   - `Uniq::isValid(string $uuid): bool`
//!   - `Uniq::version(string $uuid): ?int`
//!   - `Uniq::parse(string $uuid): array`
//!   - `Uniq::nil(): string`
//!   - `Uniq::max(): string`

use std::cell::RefCell;
use indexmap::IndexMap;
use sha1::{Digest, Sha1};

use crate::vm::error::RuntimeErrorKind;
use crate::vm::facade::{get_arg_str, FacadeResult};
use crate::vm::value::PhpValue;

thread_local! {
    /// Tracks previous (sec, usec) to ensure uniqueness across sequential calls
    /// matching PHP uniqid.c gettimeofday spin loop behavior.
    static PREV_TIME: RefCell<(u32, u32)> = const { RefCell::new((0, 0)) };

    /// Combined Linear Congruential Generator (LCG) state for uniqid entropy,
    /// matching PHP ext/standard/lcg.c php_combined_lcg.
    static LCG_STATE: RefCell<Option<(i32, i32)>> = const { RefCell::new(None) };

    /// State for UUIDv7 generation (last millisecond, 12-bit monotonic sequence counter).
    static V7_STATE: RefCell<(u64, u16)> = const { RefCell::new((0, 0)) };
}

/// Helper macro for Schrage's method modular multiplication avoiding 32-bit signed overflow.
#[inline]
fn modmult(a: i32, b: i32, c: i32, m: i32, s: &mut i32) {
    let q = *s / a;
    let mut next = b * (*s - a * q) - c * q;
    if next < 0 {
        next += m;
    }
    *s = next;
}

/// PHP-compatible Combined Linear Congruential Generator (`php_combined_lcg`).
///
/// Implements the exact algorithm from PHP's `ext/standard/lcg.c`:
/// - Generator 1: Modulus 2147483563, multipliers (53668, 40014, 12211)
/// - Generator 2: Modulus 2147483399, multipliers (52774, 40692, 3791)
/// Returns a double-precision float strictly in the range `(0, 1)`.
pub fn php_combined_lcg() -> f64 {
    LCG_STATE.with(|cell| {
        let mut state_opt = cell.borrow_mut();
        let (s1, s2) = state_opt.get_or_insert_with(|| {
            let now = std::time::SystemTime::now();
            let duration = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            let sec = duration.as_secs() as i32;
            let usec = duration.subsec_micros() as i32;
            let mut s1 = sec ^ (usec << 11);
            let pid = std::process::id() as i32;
            let mut s2 = pid ^ (usec << 11);

            // Clamp seed states to strictly positive non-zero within respective moduli
            s1 = (s1.abs() % 2147483562) + 1;
            s2 = (s2.abs() % 2147483398) + 1;
            (s1, s2)
        });

        modmult(53668, 40014, 12211, 2147483563, s1);
        modmult(52774, 40692, 3791, 2147483399, s2);

        let mut z = *s1 - *s2;
        if z < 1 {
            z += 2147483562;
        }

        (z as f64) * 4.656613e-10
    })
}

/// Retrieve unique time components `(sec, usec)` guaranteeing microsecond monotonicity.
fn get_unique_time() -> (u32, u32) {
    PREV_TIME.with(|cell| {
        let mut prev = cell.borrow_mut();
        let mut attempts = 0;
        loop {
            let now = std::time::SystemTime::now();
            let duration = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            let sec = duration.as_secs() as u32;
            let usec = (duration.subsec_micros() % 0x100000) as u32;

            if sec != prev.0 || usec != prev.1 {
                *prev = (sec, usec);
                return (sec, usec);
            }

            attempts += 1;
            if attempts > 100 {
                // In tight loops or frozen clock scenarios, artificially advance microsecond
                let new_usec = (prev.1 + 1) % 0x100000;
                let new_sec = if new_usec == 0 { prev.0 + 1 } else { prev.0 };
                *prev = (new_sec, new_usec);
                return (new_sec, new_usec);
            }
            std::hint::spin_loop();
        }
    })
}

/// Generate a unique identifier based on server time down to microsecond precision.
///
/// Default mode: 13-character hex string (8 hex digits seconds + 5 hex digits microseconds).
/// With entropy: 23-character string (13-character time string + dot + 9 pseudo-random digits).
pub fn generate_uniqid(prefix: &str, more_entropy: bool) -> String {
    let (sec, usec) = get_unique_time();

    if more_entropy {
        let mut lcg = php_combined_lcg() * 10.0;
        if lcg >= 10.0 {
            lcg = 9.99999999;
        }
        format!("{prefix}{sec:08x}{usec:05x}{lcg:.8}")
    } else {
        format!("{prefix}{sec:08x}{usec:05x}")
    }
}

/// Format 16 raw bytes into canonical 36-character hyphenated UUID string.
#[inline]
pub fn format_uuid(b: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3],
        b[4], b[5],
        b[6], b[7],
        b[8], b[9],
        b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Parse a UUID string (hyphenated 36-char, unhyphenated 32-char, or standard alias) into 16 raw bytes.
pub fn parse_uuid_bytes(s: &str) -> Option<[u8; 16]> {
    let clean = s.trim();
    let lower = clean.to_ascii_lowercase();
    match lower.as_str() {
        "dns" => return parse_uuid_hex("6ba7b8109dad11d180b400c04fd430c8"),
        "url" => return parse_uuid_hex("6ba7b8119dad11d180b400c04fd430c8"),
        "oid" => return parse_uuid_hex("6ba7b8129dad11d180b400c04fd430c8"),
        "x500" => return parse_uuid_hex("6ba7b8149dad11d180b400c04fd430c8"),
        "nil" => return Some([0u8; 16]),
        "max" => return Some([0xFF; 16]),
        _ => {}
    }

    if clean.len() == 36 {
        let b = clean.as_bytes();
        if b[8] != b'-' || b[13] != b'-' || b[18] != b'-' || b[23] != b'-' {
            return None;
        }
        let hex_only: String = clean.chars().filter(|c| *c != '-').collect();
        parse_uuid_hex(&hex_only)
    } else if clean.len() == 32 {
        parse_uuid_hex(clean)
    } else {
        None
    }
}

fn parse_uuid_hex(hex_str: &str) -> Option<[u8; 16]> {
    if hex_str.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for i in 0..16 {
        bytes[i] = u8::from_str_radix(&hex_str[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}

/// Generate RFC 4122 / RFC 9562 Version 4 (random) UUID.
pub fn generate_uuid4() -> Result<String, RuntimeErrorKind> {
    let rand_bytes = crate::vm::crypto::random_bytes(16).map_err(|e| RuntimeErrorKind::Custom {
        message: format!("Uniq::uuid4 failed: {}", e),
    })?;
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&rand_bytes);

    // Set version to 4 (0100b)
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    // Set variant to RFC 4122 (10xxb)
    bytes[8] = (bytes[8] & 0x3F) | 0x80;

    Ok(format_uuid(&bytes))
}

/// Generate RFC 4122 / RFC 9562 Version 5 (SHA-1 name-based) UUID.
pub fn generate_uuid5(namespace: &str, name: &str) -> Result<String, RuntimeErrorKind> {
    let ns_bytes = parse_uuid_bytes(namespace).ok_or_else(|| RuntimeErrorKind::Custom {
        message: format!("Uniq::uuid5(): Invalid namespace UUID '{}'", namespace),
    })?;

    let mut hasher = Sha1::new();
    hasher.update(&ns_bytes);
    hasher.update(name.as_bytes());
    let hash = hasher.finalize();

    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[0..16]);

    // Set version to 5 (0101b)
    bytes[6] = (bytes[6] & 0x0F) | 0x50;
    // Set variant to RFC 4122 (10xxb)
    bytes[8] = (bytes[8] & 0x3F) | 0x80;

    Ok(format_uuid(&bytes))
}

/// Generate RFC 4122 Version 3 (MD5 name-based) UUID.
pub fn generate_uuid3(namespace: &str, name: &str) -> Result<String, RuntimeErrorKind> {
    let ns_bytes = parse_uuid_bytes(namespace).ok_or_else(|| RuntimeErrorKind::Custom {
        message: format!("Uniq::uuid3(): Invalid namespace UUID '{}'", namespace),
    })?;

    let mut input = Vec::with_capacity(16 + name.len());
    input.extend_from_slice(&ns_bytes);
    input.extend_from_slice(name.as_bytes());
    let hash = crate::vm::crypto::md5::md5_digest(&input);

    let mut bytes = hash;
    // Set version to 3 (0011b)
    bytes[6] = (bytes[6] & 0x0F) | 0x30;
    // Set variant to RFC 4122 (10xxb)
    bytes[8] = (bytes[8] & 0x3F) | 0x80;

    Ok(format_uuid(&bytes))
}

/// Generate RFC 9562 Version 7 (Unix Epoch time-ordered) UUID.
///
/// Features:
/// - 48-bit big-endian millisecond timestamp for chronological ordering.
/// - 12-bit monotonic sequence counter / random data in `rand_a`.
/// - 62 bits of cryptographic pseudo-randomness in `rand_b`.
pub fn generate_uuid7(custom_ts: Option<u64>) -> Result<String, RuntimeErrorKind> {
    let (ts_ms, counter) = match custom_ts {
        Some(ts) => {
            let rnd_bytes = crate::vm::crypto::random_bytes(2).map_err(|e| RuntimeErrorKind::Custom {
                message: format!("Uniq::uuid7 failed to obtain random bytes: {}", e),
            })?;
            let rnd = u16::from_be_bytes([rnd_bytes[0], rnd_bytes[1]]) & 0x0FFF;
            (ts, rnd)
        }
        None => {
            V7_STATE.with(|cell| {
                let mut state = cell.borrow_mut();
                let now = std::time::SystemTime::now();
                let now_ms = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;

                if now_ms > state.0 {
                    state.0 = now_ms;
                    let rnd = if let Ok(rb) = crate::vm::crypto::random_bytes(2) {
                        u16::from_be_bytes([rb[0], rb[1]]) & 0x03FF
                    } else {
                        0
                    };
                    state.1 = rnd;
                } else {
                    // Same millisecond: increment sequence counter for monotonic ordering
                    state.1 = (state.1 + 1) & 0x0FFF;
                }
                (state.0, state.1)
            })
        }
    };

    let mut bytes = [0u8; 16];
    // 48-bit timestamp
    bytes[0] = ((ts_ms >> 40) & 0xFF) as u8;
    bytes[1] = ((ts_ms >> 32) & 0xFF) as u8;
    bytes[2] = ((ts_ms >> 24) & 0xFF) as u8;
    bytes[3] = ((ts_ms >> 16) & 0xFF) as u8;
    bytes[4] = ((ts_ms >> 8) & 0xFF) as u8;
    bytes[5] = (ts_ms & 0xFF) as u8;

    // Version 7 (0111b) + rand_a (12 bits)
    bytes[6] = 0x70 | (((counter >> 8) & 0x0F) as u8);
    bytes[7] = (counter & 0xFF) as u8;

    // rand_b (64 bits total, variant takes upper 2 bits)
    let rand_b = crate::vm::crypto::random_bytes(8).map_err(|e| RuntimeErrorKind::Custom {
        message: format!("Uniq::uuid7 failed to obtain random bytes: {}", e),
    })?;
    bytes[8] = (rand_b[0] & 0x3F) | 0x80; // Variant RFC 4122 (10xxb)
    bytes[9..16].copy_from_slice(&rand_b[1..8]);

    Ok(format_uuid(&bytes))
}

/// Check whether a string is a valid UUID format (36-char hyphenated or 32-char hex).
pub fn is_valid_uuid(s: &str) -> bool {
    parse_uuid_bytes(s).is_some()
}

/// Parse UUID into structured components dictionary.
pub fn parse_uuid_info(s: &str) -> Result<PhpValue, RuntimeErrorKind> {
    let bytes = parse_uuid_bytes(s).ok_or_else(|| RuntimeErrorKind::Custom {
        message: format!("Uniq::parse(): Invalid UUID string '{}'", s),
    })?;

    let mut map = IndexMap::new();
    let hex_str = hex::encode(bytes);
    map.insert("hex".to_string(), PhpValue::string(hex_str));

    let ver = (bytes[6] >> 4) as i32;
    map.insert("version".to_string(), PhpValue::Int((ver) as i64));

    let var_byte = bytes[8];
    let variant = if (var_byte & 0x80) == 0 {
        "reserved_ncs"
    } else if (var_byte & 0xC0) == 0x80 {
        "rfc4122"
    } else if (var_byte & 0xE0) == 0xC0 {
        "reserved_microsoft"
    } else {
        "reserved_future"
    };
    map.insert("variant".to_string(), PhpValue::string(variant));

    if ver == 7 {
        let ts_ms = ((bytes[0] as u64) << 40)
            | ((bytes[1] as u64) << 32)
            | ((bytes[2] as u64) << 24)
            | ((bytes[3] as u64) << 16)
            | ((bytes[4] as u64) << 8)
            | (bytes[5] as u64);
        let val = ts_ms as i64;
        let ts_val = if let Ok(narrow) = i32::try_from(val) {
            PhpValue::Int((narrow) as i64)
        } else {
            PhpValue::Int((val as i128) as i64)
        };
        map.insert("timestamp_ms".to_string(), ts_val);
    }

    Ok(PhpValue::dict_from_map(map))
}

/// Central dispatch routine for static calls on `Uniq::*`.
pub fn dispatch_uniq(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        // PHP-compatible uniqid
        // Example: `Uniq::uniqid()`, `Uniq::uniqid('prefix_')`, `Uniq::uniqid('', true)`
        "uniqid" | "id" => {
            let prefix = match args.first() {
                Some(PhpValue::Null) | None => String::new(),
                Some(v) => v.to_php_string(),
            };
            let more_entropy = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
            let id = generate_uniqid(&prefix, more_entropy);
            Ok(PhpValue::string(id))
        }

        // UUIDv4 (Random)
        // Example: `Uniq::uuid4()`, `Uniq::v4()`, `Uniq::uuid()`
        "uuid4" | "v4" | "uuid" => {
            let uuid = generate_uuid4()?;
            Ok(PhpValue::string(uuid))
        }

        // UUIDv5 (SHA-1 Name-based)
        // Example: `Uniq::uuid5('dns', 'php.net')`, `Uniq::uuid5('6ba7b810-9dad-11d1-80b4-00c04fd430c8', 'widget')`
        "uuid5" | "v5" => {
            let ns = get_arg_str(args, 0, "Uniq::uuid5")?;
            let name = get_arg_str(args, 1, "Uniq::uuid5")?;
            let uuid = generate_uuid5(&ns, &name)?;
            Ok(PhpValue::string(uuid))
        }

        // UUIDv7 (Time-ordered)
        // Example: `Uniq::uuid7()`, `Uniq::v7()`, `Uniq::uuid7(1730706000000)`
        "uuid7" | "v7" => {
            let custom_ts = if let Some(arg) = args.first() {
                if matches!(arg, PhpValue::Null) {
                    None
                } else if let Some(i) = arg.to_i64() {
                    Some(i as u64)
                } else {
                    return Err(RuntimeErrorKind::TypeError {
                        message: format!("Uniq::uuid7() expects integer timestamp, got {}", arg.type_name()),
                    });
                }
            } else {
                None
            };
            let uuid = generate_uuid7(custom_ts)?;
            Ok(PhpValue::string(uuid))
        }

        // UUIDv3 (MD5 Name-based)
        // Example: `Uniq::uuid3('dns', 'php.net')`
        "uuid3" | "v3" => {
            let ns = get_arg_str(args, 0, "Uniq::uuid3")?;
            let name = get_arg_str(args, 1, "Uniq::uuid3")?;
            let uuid = generate_uuid3(&ns, &name)?;
            Ok(PhpValue::string(uuid))
        }

        // UUID Validation
        // Example: `Uniq::isValid($uuid)`
        "isValid" | "is_valid" => {
            let s = get_arg_str(args, 0, "Uniq::isValid")?;
            Ok(PhpValue::Bool(is_valid_uuid(&s)))
        }

        // UUID Version inspection
        // Example: `Uniq::version($uuid)` -> 4, 7, 5, etc. or null
        "version" => {
            let s = get_arg_str(args, 0, "Uniq::version")?;
            if let Some(bytes) = parse_uuid_bytes(&s) {
                let ver = (bytes[6] >> 4) as i32;
                Ok(PhpValue::Int((ver) as i64))
            } else {
                Ok(PhpValue::Null)
            }
        }

        // UUID Parsing
        // Example: `Uniq::parse($uuid)`
        "parse" => {
            let s = get_arg_str(args, 0, "Uniq::parse")?;
            let dict = parse_uuid_info(&s)?;
            Ok(dict)
        }

        // Special UUIDs
        "nil" => Ok(PhpValue::string("00000000-0000-0000-0000-000000000000")),
        "max" => Ok(PhpValue::string("ffffffff-ffff-ffff-ffff-ffffffffffff")),

        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Uniq".to_string(),
            method: method.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uniqid_default_length_and_format() {
        let id1 = generate_uniqid("", false);
        let id2 = generate_uniqid("", false);

        assert_eq!(id1.len(), 13, "Default uniqid must be 13 characters");
        assert_eq!(id2.len(), 13, "Default uniqid must be 13 characters");
        assert_ne!(id1, id2, "Sequential uniqid calls must produce distinct IDs");

        // Verify hex characters
        assert!(id1.chars().all(|c| c.is_ascii_hexdigit()), "uniqid must be hexadecimal");
    }

    #[test]
    fn test_uniqid_with_prefix() {
        let pfx = "test_";
        let id = generate_uniqid(pfx, false);
        assert_eq!(id.len(), pfx.len() + 13);
        assert!(id.starts_with(pfx));
    }

    #[test]
    fn test_uniqid_with_entropy() {
        let id = generate_uniqid("", true);
        assert_eq!(id.len(), 23, "With entropy, uniqid must be 23 characters");
        assert!(id.contains('.'), "With entropy, uniqid must contain a dot separator");

        let pfx = "usr_";
        let id_pfx = generate_uniqid(pfx, true);
        assert_eq!(id_pfx.len(), pfx.len() + 23);
        assert!(id_pfx.starts_with(pfx));
    }

    #[test]
    fn test_uuid4_rfc4122_compliance() {
        let u = generate_uuid4().expect("uuid4 generation should succeed");
        assert_eq!(u.len(), 36);
        assert!(is_valid_uuid(&u));

        let bytes = parse_uuid_bytes(&u).expect("must parse uuid bytes");
        // Version 4
        assert_eq!(bytes[6] >> 4, 4);
        // Variant 10xx
        assert_eq!(bytes[8] >> 6, 2);
    }

    #[test]
    fn test_uuid5_rfc4122_vector() {
        // RFC 4122 Appendix B test vector:
        // Namespace DNS: 6ba7b810-9dad-11d1-80b4-00c04fd430c8
        // Name: "python.org"
        // Expected UUID: 886313e1-3b8a-5372-9b90-0c9aee199e5d
        let u = generate_uuid5("dns", "python.org").expect("uuid5 should succeed");
        assert_eq!(u, "886313e1-3b8a-5372-9b90-0c9aee199e5d");
        assert_eq!(parse_uuid_bytes(&u).unwrap()[6] >> 4, 5);
    }

    #[test]
    fn test_uuid7_time_ordered() {
        let u1 = generate_uuid7(Some(1730706000000)).expect("uuid7 should succeed");
        let u2 = generate_uuid7(Some(1730706001000)).expect("uuid7 should succeed");

        assert_eq!(u1.len(), 36);
        assert_eq!(u2.len(), 36);
        assert!(is_valid_uuid(&u1));
        assert!(is_valid_uuid(&u2));

        let b1 = parse_uuid_bytes(&u1).unwrap();
        let b2 = parse_uuid_bytes(&u2).unwrap();
        assert_eq!(b1[6] >> 4, 7);
        assert_eq!(b2[6] >> 4, 7);

        // Lexicographically sortable by creation timestamp
        assert!(u1 < u2, "Earlier timestamp UUIDv7 must be lexicographically smaller");
    }

    #[test]
    fn test_uuid3_md5_vector() {
        // Namespace DNS: 6ba7b810-9dad-11d1-80b4-00c04fd430c8, Name: "python.org"
        // Expected UUIDv3: 6fa459ea-ee8a-3ca4-894e-db77e160355e
        let u = generate_uuid3("dns", "python.org").expect("uuid3 should succeed");
        assert_eq!(u, "6fa459ea-ee8a-3ca4-894e-db77e160355e");
        assert_eq!(parse_uuid_bytes(&u).unwrap()[6] >> 4, 3);
    }

    #[test]
    fn test_is_valid_and_parse() {
        assert!(is_valid_uuid("00000000-0000-0000-0000-000000000000"));
        assert!(!is_valid_uuid("invalid-uuid-string"));
        assert!(!is_valid_uuid("12345678-1234-1234-1234-1234567890zz"));

        let info = parse_uuid_info("886313e1-3b8a-5372-9b90-0c9aee199e5d").unwrap();
        match info {
            PhpValue::Dict(d) => {
                let map = d.borrow();
                assert_eq!(map.get("version"), Some(&PhpValue::Int(5)));
                assert_eq!(map.get("variant"), Some(&PhpValue::string("rfc4122")));
            }
            _ => panic!("Expected dict"),
        }
    }
}
