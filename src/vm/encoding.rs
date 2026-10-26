//! Multi-byte character encoding and binary conversion module for RHP.
//!
//! Provides binary-safe decoding and encoding for legacy character sets
//! including Shift-JIS (SJIS / CP932), EUC-JP, ISO-2022-JP (JIS), UTF-8,
//! ASCII, ISO-8859-1 (Latin-1), Hex, and Base64.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Recognized character encoding identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    ShiftJis,
    EucJp,
    Iso2022Jp,
    Ascii,
    Iso8859_1,
    Binary,
}

impl Encoding {
    pub fn parse(name: &str) -> Option<Self> {
        let clean = name.trim().to_uppercase().replace(['-', '_', ' '], "");
        match clean.as_str() {
            "UTF8" => Some(Self::Utf8),
            "SJIS" | "SHIFTJIS" | "JSHIFT" | "CP932" | "WINDOWS31J" | "MS932" => Some(Self::ShiftJis),
            "EUCJP" | "EUC" => Some(Self::EucJp),
            "ISO2022JP" | "JIS" | "2022JP" => Some(Self::Iso2022Jp),
            "ASCII" | "USASCII" => Some(Self::Ascii),
            "ISO88591" | "LATIN1" => Some(Self::Iso8859_1),
            "BINARY" | "RAW" | "BYTES" => Some(Self::Binary),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::ShiftJis => "Shift-JIS",
            Self::EucJp => "EUC-JP",
            Self::Iso2022Jp => "ISO-2022-JP",
            Self::Ascii => "ASCII",
            Self::Iso8859_1 => "ISO-8859-1",
            Self::Binary => "Binary",
        }
    }
}

// =========================================================================
// JIS X 0208 and Shift-JIS Lookup Tables
// =========================================================================

struct JisTables {
    jis_to_unicode: HashMap<u16, char>,
    unicode_to_jis: HashMap<char, u16>,
}

static JIS_TABLES: OnceLock<JisTables> = OnceLock::new();

fn get_jis_tables() -> &'static JisTables {
    JIS_TABLES.get_or_init(|| {
        let mut jis_to_unicode = HashMap::new();
        let mut unicode_to_jis = HashMap::new();

        let mut insert_pair = |jis: u16, ch: char| {
            jis_to_unicode.insert(jis, ch);
            unicode_to_jis.insert(ch, jis);
        };

        // 1. JIS Row 1: Special Characters and Punctuation (0x2121..=0x217E)
        let row1_chars = [
            (0x2121, '　'), (0x2122, '、'), (0x2123, '。'), (0x2124, '，'),
            (0x2125, '．'), (0x2126, '・'), (0x2127, '：'), (0x2128, '；'),
            (0x2129, '？'), (0x212A, '！'), (0x212B, '゛'), (0x212C, '゜'),
            (0x212D, '´'), (0x212E, '｀'), (0x212F, '¨'), (0x2130, '＾'),
            (0x2131, '￣'), (0x2132, '＿'), (0x2133, 'ヽ'), (0x2134, 'ヾ'),
            (0x2135, 'ゝ'), (0x2136, 'ゞ'), (0x2137, '〃'), (0x2138, '仝'),
            (0x2139, '々'), (0x213A, '〆'), (0x213B, '〇'), (0x213C, 'ー'),
            (0x213D, '—'), (0x213E, '‐'), (0x213F, '／'), (0x2140, '＼'),
            (0x2141, '〜'), (0x2142, '‖'), (0x2143, '｜'), (0x2144, '…'),
            (0x2145, '‥'), (0x2146, '「'), (0x2147, '」'), (0x2148, '『'),
            (0x2149, '』'), (0x214A, '【'), (0x214B, '】'), (0x214C, '（'),
            (0x214D, '）'), (0x214E, '〔'), (0x214F, '〕'), (0x2150, '［'),
            (0x2151, '］'), (0x2152, '｛'), (0x2153, '｝'), (0x2154, '〈'),
            (0x2155, '〉'), (0x2156, '《'), (0x2157, '》'), (0x2158, '＋'),
            (0x2159, '−'), (0x215A, '±'), (0x215B, '×'), (0x215C, '÷'),
            (0x215D, '＝'), (0x215E, '≠'), (0x215F, '＜'), (0x2160, '＞'),
            (0x2161, '≦'), (0x2162, '≧'), (0x2163, '∞'), (0x2164, '∴'),
            (0x2165, '♂'), (0x2166, '♀'), (0x2167, '°'), (0x2168, '′'),
            (0x2169, '″'), (0x216A, '℃'), (0x216B, '¥'), (0x216C, '＄'),
            (0x216D, '¢'), (0x216E, '£'), (0x216F, '％'), (0x2170, '＃'),
            (0x2171, '＆'), (0x2172, '＊'), (0x2173, '＠'), (0x2174, '§'),
            (0x2175, '☆'), (0x2176, '★'), (0x2177, '○'), (0x2178, '●'),
            (0x2179, '◎'), (0x217A, '◇'), (0x217B, '◆'), (0x217C, '□'),
            (0x217D, '■'), (0x217E, '△'),
        ];
        for (jis, ch) in row1_chars {
            insert_pair(jis, ch);
        }

        // 2. JIS Row 3: Full-width ASCII (0x2321..=0x237E)
        // 0x2330..=0x2339 is '０'..'９'
        for i in 0..10 {
            if let Some(ch) = char::from_u32(0xFF10 + i) {
                insert_pair(0x2330 + i as u16, ch);
            }
        }
        // 0x2341..=0x235A is 'Ａ'..'Ｚ'
        for i in 0..26 {
            if let Some(ch) = char::from_u32(0xFF21 + i) {
                insert_pair(0x2341 + i as u16, ch);
            }
        }
        // 0x2361..=0x237A is 'ａ'..'ｚ'
        for i in 0..26 {
            if let Some(ch) = char::from_u32(0xFF41 + i) {
                insert_pair(0x2361 + i as u16, ch);
            }
        }

        // 3. JIS Row 4: Hiragana (0x2421..=0x2473 -> U+3041..=U+3093)
        for i in 0..=(0x73 - 0x21) {
            if let Some(ch) = char::from_u32(0x3041 + i) {
                insert_pair(0x2421 + i as u16, ch);
            }
        }

        // 4. JIS Row 5: Katakana (0x2521..=0x2576 -> U+30A1..=U+30F6)
        for i in 0..=(0x76 - 0x21) {
            if let Some(ch) = char::from_u32(0x30A1 + i) {
                insert_pair(0x2521 + i as u16, ch);
            }
        }

        // 5. Common and Essential Kanji
        let kanji_pairs = [
            (0x4024, '世'), (0x3326, '界'), (0x467C, '日'), (0x435C, '本'),
            (0x386C, '語'), (0x3B64, '私'), (0x452B, '東'), (0x357E, '京'),
            (0x3F4C, '生'), (0x4740, '年'), (0x376E, '月'), (0x3251, '火'),
            (0x3F65, '水'), (0x4C58, '木'), (0x3662, '金'), (0x4559, '土'),
            (0x393E, '山'), (0x4062, '川'), (0x3938, '三'), (0x306C, '一'),
            (0x4673, '二'), (0x3B4D, '四'), (0x385E, '五'), (0x4F3B, '六'),
            (0x3C37, '七'), (0x482C, '八'), (0x3663, '九'), (0x3D3E, '十'),
            (0x4934, '百'), (0x4067, '千'), (0x4C7C, '万'), (0x315F, '円'),
            (0x3B7A, '時'), (0x4A2C, '分'), (0x4A42, '文'), (0x3B7A, '字'),
            (0x323E, '化'), (0x3954, '国'), (0x3F6C, '人'), (0x4267, '大'),
            (0x3E2C, '小'), (0x4366, '中'), (0x392D, '高'), (0x3042, '安'),
            (0x3F56, '新'), (0x3845, '古'), (0x446B, '長'), (0x432F, '短'),
            (0x4A6A, '法'), (0x4E7C, '律'), (0x3F3F, '政'), (0x3C23, '治'),
            (0x3750, '経'), (0x3A51, '済'), (0x3358, '学'), (0x3921, '校'),
            (0x452A, '同'), (0x4D3F, '名'), (0x4134, '前'), (0x3861, '後'),
            (0x4B3B, '北'), (0x466E, '南'), (0x456C, '東'),
            (0x3B3E, '西'), (0x3E2F, '上'), (0x3238, '下'), (0x3A38, '左'),
            (0x3126, '右'), (0x3F4E, '性'), (0x4C35, '男'), (0x3D77, '女'),
            (0x3B52, '子'), (0x3648, '気'), (0x4524, '天'), (0x312B, '雨'),
            (0x4069, '電'), (0x3C56, '車'), (0x3258, '会'), (0x3C52, '社'),
            (0x4528, '店'), (0x317F, '駅'), (0x3F6B, '道'), (0x382E, '家'),
            (0x3840, '戸'), (0x4C67, '門'), (0x3862, '光'), (0x3877, '音'),
            (0x4C4C, '目'), (0x3C28, '耳'), (0x387D, '口'), (0x4E6A, '手'),
            (0x3B2D, '足'), (0x3F55, '心'), (0x4E74, '力'), (0x4C31, '白'),
            (0x3975, '黒'), (0x4056, '赤'), (0x4042, '青'), (0x322B, '黄'),
        ];
        for (jis, ch) in kanji_pairs {
            insert_pair(jis, ch);
        }

        JisTables {
            jis_to_unicode,
            unicode_to_jis,
        }
    })
}

/// Convert Shift-JIS 2-byte sequence `(s1, s2)` into JIS X 0208 code `(j1, j2)`.
#[inline]
pub fn sjis_to_jis(s1: u8, s2: u8) -> Option<(u8, u8)> {
    let j1_base = if s1 <= 0x9F {
        (s1.checked_sub(0x81)? as u16) * 2 + 0x21
    } else {
        (s1.checked_sub(0xE0)? as u16) * 2 + 0x5F
    };

    if s2 < 0x9F {
        let j2 = if s2 > 0x7F {
            s2.checked_sub(0x20)?
        } else {
            s2.checked_sub(0x1F)?
        };
        Some((j1_base as u8, j2))
    } else {
        let j2 = s2.checked_sub(0x7E)?;
        Some(((j1_base + 1) as u8, j2))
    }
}

/// Convert JIS X 0208 code `(j1, j2)` into Shift-JIS 2-byte sequence `(s1, s2)`.
#[inline]
pub fn jis_to_sjis(j1: u8, j2: u8) -> (u8, u8) {
    let s1 = if j1 % 2 == 1 {
        (j1 + 1) / 2 + (if j1 < 0x5F { 0x70 } else { 0xB0 })
    } else {
        j1 / 2 + (if j1 < 0x5F { 0x70 } else { 0xB0 })
    };

    let s2 = if j1 % 2 == 1 {
        if j2 < 0x60 {
            j2 + 0x1F
        } else {
            j2 + 0x20
        }
    } else {
        j2 + 0x7E
    };

    (s1, s2)
}

/// Map a JIS X 0208 code `(j1, j2)` to a Unicode `char`.
pub fn jis_code_to_char(j1: u8, j2: u8) -> Option<char> {
    let jis = ((j1 as u16) << 8) | (j2 as u16);
    let tables = get_jis_tables();
    if let Some(&ch) = tables.jis_to_unicode.get(&jis) {
        return Some(ch);
    }

    // Algorithmic fallbacks for contiguous JIS blocks:
    if j1 == 0x24 && (0x21..=0x73).contains(&j2) {
        // Hiragana
        return char::from_u32(0x3041 + (j2 - 0x21) as u32);
    }
    if j1 == 0x25 && (0x21..=0x76).contains(&j2) {
        // Katakana
        return char::from_u32(0x30A1 + (j2 - 0x21) as u32);
    }
    if j1 == 0x23 && (0x30..=0x39).contains(&j2) {
        // Fullwidth digits
        return char::from_u32(0xFF10 + (j2 - 0x30) as u32);
    }
    if j1 == 0x23 && (0x41..=0x5A).contains(&j2) {
        // Fullwidth Latin Upper
        return char::from_u32(0xFF21 + (j2 - 0x41) as u32);
    }
    if j1 == 0x23 && (0x61..=0x7A).contains(&j2) {
        // Fullwidth Latin Lower
        return char::from_u32(0xFF41 + (j2 - 0x61) as u32);
    }

    None
}

/// Map a Unicode `char` to a JIS X 0208 code `(j1, j2)`.
pub fn char_to_jis_code(ch: char) -> Option<(u8, u8)> {
    let tables = get_jis_tables();
    if let Some(&jis) = tables.unicode_to_jis.get(&ch) {
        return Some(((jis >> 8) as u8, (jis & 0xFF) as u8));
    }

    let cp = ch as u32;
    // Hiragana
    if (0x3041..=0x3093).contains(&cp) {
        let j2 = (cp - 0x3041 + 0x21) as u8;
        return Some((0x24, j2));
    }
    // Katakana
    if (0x30A1..=0x30F6).contains(&cp) {
        let j2 = (cp - 0x30A1 + 0x21) as u8;
        return Some((0x25, j2));
    }
    // Fullwidth digits
    if (0xFF10..=0xFF19).contains(&cp) {
        let j2 = (cp - 0xFF10 + 0x30) as u8;
        return Some((0x23, j2));
    }
    // Fullwidth Upper
    if (0xFF21..=0xFF3A).contains(&cp) {
        let j2 = (cp - 0xFF21 + 0x41) as u8;
        return Some((0x23, j2));
    }
    // Fullwidth Lower
    if (0xFF41..=0xFF5A).contains(&cp) {
        let j2 = (cp - 0xFF41 + 0x61) as u8;
        return Some((0x23, j2));
    }

    None
}

// =========================================================================
// Shift-JIS Decoding & Encoding
// =========================================================================

/// Decodes raw Shift-JIS bytes to a valid UTF-8 Rust `String`.
pub fn decode_shift_jis(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b <= 0x7F {
            // ASCII
            out.push(b as char);
            i += 1;
        } else if (0xA1..=0xDF).contains(&b) {
            // Half-width Katakana (U+FF61..=U+FF9F)
            if let Some(ch) = char::from_u32(0xFF61 + (b - 0xA1) as u32) {
                out.push(ch);
            } else {
                out.push('\u{FFFD}');
            }
            i += 1;
        } else if ((0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b)) && i + 1 < bytes.len() {
            // 2-byte Shift-JIS lead
            let trail = bytes[i + 1];
            if let Some((j1, j2)) = sjis_to_jis(b, trail) {
                if let Some(ch) = jis_code_to_char(j1, j2) {
                    out.push(ch);
                } else {
                    out.push('\u{FFFD}');
                }
            } else {
                out.push('\u{FFFD}');
            }
            i += 2;
        } else {
            // Invalid byte
            out.push('\u{FFFD}');
            i += 1;
        }
    }
    out
}

/// Encodes a UTF-8 string to Shift-JIS raw bytes.
pub fn encode_shift_jis(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for ch in s.chars() {
        let cp = ch as u32;
        if cp <= 0x7F {
            out.push(cp as u8);
        } else if (0xFF61..=0xFF9F).contains(&cp) {
            // Half-width Katakana
            out.push((cp - 0xFF61 + 0xA1) as u8);
        } else if let Some((j1, j2)) = char_to_jis_code(ch) {
            let (s1, s2) = jis_to_sjis(j1, j2);
            out.push(s1);
            out.push(s2);
        } else {
            // Fallback: ASCII question mark for unrepresentable chars
            out.push(b'?');
        }
    }
    out
}

// =========================================================================
// EUC-JP Decoding & Encoding
// =========================================================================

/// Decodes raw EUC-JP bytes to a valid UTF-8 Rust `String`.
pub fn decode_euc_jp(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b <= 0x7F {
            out.push(b as char);
            i += 1;
        } else if b == 0x8E && i + 1 < bytes.len() {
            // Single Shift 2: Half-width Katakana
            let trail = bytes[i + 1];
            if (0xA1..=0xDF).contains(&trail) {
                if let Some(ch) = char::from_u32(0xFF61 + (trail - 0xA1) as u32) {
                    out.push(ch);
                } else {
                    out.push('\u{FFFD}');
                }
            } else {
                out.push('\u{FFFD}');
            }
            i += 2;
        } else if (0xA1..=0xFE).contains(&b) && i + 1 < bytes.len() {
            // 2-byte JIS X 0208
            let trail = bytes[i + 1];
            if (0xA1..=0xFE).contains(&trail) {
                let j1 = b - 0x80;
                let j2 = trail - 0x80;
                if let Some(ch) = jis_code_to_char(j1, j2) {
                    out.push(ch);
                } else {
                    out.push('\u{FFFD}');
                }
            } else {
                out.push('\u{FFFD}');
            }
            i += 2;
        } else {
            out.push('\u{FFFD}');
            i += 1;
        }
    }
    out
}

/// Encodes a UTF-8 string to EUC-JP raw bytes.
pub fn encode_euc_jp(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for ch in s.chars() {
        let cp = ch as u32;
        if cp <= 0x7F {
            out.push(cp as u8);
        } else if (0xFF61..=0xFF9F).contains(&cp) {
            out.push(0x8E);
            out.push((cp - 0xFF61 + 0xA1) as u8);
        } else if let Some((j1, j2)) = char_to_jis_code(ch) {
            out.push(j1 + 0x80);
            out.push(j2 + 0x80);
        } else {
            out.push(b'?');
        }
    }
    out
}

// =========================================================================
// ISO-2022-JP (JIS) Decoding & Encoding
// =========================================================================

/// Decodes raw ISO-2022-JP escape-sequenced bytes to a UTF-8 `String`.
pub fn decode_iso_2022_jp(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    let mut in_jis = false;

    while i < bytes.len() {
        // Escape sequence check:
        // ESC $ B (1B 24 42) or ESC $ @ (1B 24 40) => JIS mode
        // ESC ( B (1B 28 42) or ESC ( J (1B 28 4A) => ASCII mode
        if bytes[i] == 0x1B && i + 2 < bytes.len() {
            if bytes[i + 1] == 0x24 && (bytes[i + 2] == 0x42 || bytes[i + 2] == 0x40) {
                in_jis = true;
                i += 3;
                continue;
            } else if bytes[i + 1] == 0x28 && (bytes[i + 2] == 0x42 || bytes[i + 2] == 0x4A) {
                in_jis = false;
                i += 3;
                continue;
            }
        }

        if in_jis {
            if i + 1 < bytes.len() {
                let j1 = bytes[i];
                let j2 = bytes[i + 1];
                if let Some(ch) = jis_code_to_char(j1, j2) {
                    out.push(ch);
                } else {
                    out.push('\u{FFFD}');
                }
                i += 2;
            } else {
                out.push('\u{FFFD}');
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Encodes a UTF-8 string to ISO-2022-JP bytes with escape sequences.
pub fn encode_iso_2022_jp(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 16);
    let mut in_jis = false;

    for ch in s.chars() {
        let cp = ch as u32;
        if cp <= 0x7F {
            if in_jis {
                // Return to ASCII: ESC ( B
                out.extend_from_slice(b"\x1B(B");
                in_jis = false;
            }
            out.push(cp as u8);
        } else if let Some((j1, j2)) = char_to_jis_code(ch) {
            if !in_jis {
                // Enter JIS X 0208: ESC $ B
                out.extend_from_slice(b"\x1B$B");
                in_jis = true;
            }
            out.push(j1);
            out.push(j2);
        } else {
            if in_jis {
                out.extend_from_slice(b"\x1B(B");
                in_jis = false;
            }
            out.push(b'?');
        }
    }

    if in_jis {
        out.extend_from_slice(b"\x1B(B");
    }

    out
}

// =========================================================================
// General Conversion & Detection Facades
// =========================================================================

/// Convert bytes from a named source encoding into a valid UTF-8 `String`.
pub fn decode_to_utf8(bytes: &[u8], encoding: &str) -> Result<String, String> {
    let enc = Encoding::parse(encoding).ok_or_else(|| format!("Unsupported encoding: '{}'", encoding))?;
    match enc {
        Encoding::Utf8 => {
            if let Ok(s) = std::str::from_utf8(bytes) {
                Ok(s.to_string())
            } else {
                Ok(String::from_utf8_lossy(bytes).into_owned())
            }
        }
        Encoding::ShiftJis => Ok(decode_shift_jis(bytes)),
        Encoding::EucJp => Ok(decode_euc_jp(bytes)),
        Encoding::Iso2022Jp => Ok(decode_iso_2022_jp(bytes)),
        Encoding::Ascii => Ok(bytes.iter().map(|&b| (b & 0x7F) as char).collect()),
        Encoding::Iso8859_1 | Encoding::Binary => Ok(bytes.iter().map(|&b| b as char).collect()),
    }
}

/// Convert a UTF-8 string into raw bytes of the target encoding.
pub fn encode_from_utf8(s: &str, encoding: &str) -> Result<Vec<u8>, String> {
    let enc = Encoding::parse(encoding).ok_or_else(|| format!("Unsupported encoding: '{}'", encoding))?;
    match enc {
        Encoding::Utf8 => Ok(s.as_bytes().to_vec()),
        Encoding::ShiftJis => Ok(encode_shift_jis(s)),
        Encoding::EucJp => Ok(encode_euc_jp(s)),
        Encoding::Iso2022Jp => Ok(encode_iso_2022_jp(s)),
        Encoding::Ascii => Ok(s.chars().map(|c| (c as u32 & 0x7F) as u8).collect()),
        Encoding::Iso8859_1 | Encoding::Binary => {
            Ok(s.chars().map(|c| (c as u32 & 0xFF) as u8).collect())
        }
    }
}

/// Detects the probable encoding of a raw byte buffer.
pub fn detect_encoding(bytes: &[u8]) -> &'static str {
    // 1. Check if entirely ASCII
    if bytes.iter().all(|&b| b < 0x80) {
        if bytes.windows(3).any(|w| w == b"\x1B$B" || w == b"\x1B$@") {
            return "ISO-2022-JP";
        }
        return "ASCII";
    }

    // 2. Check if valid UTF-8
    if std::str::from_utf8(bytes).is_ok() {
        return "UTF-8";
    }

    // 3. Check for ISO-2022-JP escape sequences
    if bytes.windows(3).any(|w| w == b"\x1B$B" || w == b"\x1B$@") {
        return "ISO-2022-JP";
    }

    // 4. Shift-JIS vs EUC-JP heuristic
    let mut sjis_score = 0;
    let mut euc_score = 0;
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b) {
            if i + 1 < bytes.len() {
                let trail = bytes[i + 1];
                if (0x40..=0x7E).contains(&trail) || (0x80..=0xFC).contains(&trail) {
                    sjis_score += 2;
                }
            }
        }
        if (0xA1..=0xFE).contains(&b) && i + 1 < bytes.len() {
            let trail = bytes[i + 1];
            if (0xA1..=0xFE).contains(&trail) {
                euc_score += 2;
            }
        }
        i += 1;
    }

    if sjis_score >= euc_score && sjis_score > 0 {
        "Shift-JIS"
    } else if euc_score > 0 {
        "EUC-JP"
    } else {
        "Binary"
    }
}

/// Validates whether a byte slice represents valid Shift-JIS.
pub fn is_valid_shift_jis(bytes: &[u8]) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b <= 0x7F {
            i += 1;
        } else if (0xA1..=0xDF).contains(&b) {
            i += 1;
        } else if (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b) {
            if i + 1 >= bytes.len() {
                return false;
            }
            let trail = bytes[i + 1];
            if let Some((j1, j2)) = sjis_to_jis(b, trail) {
                if jis_code_to_char(j1, j2).is_some() {
                    i += 2;
                    continue;
                }
            }
            return false;
        } else {
            return false;
        }
    }
    true
}

/// Validates whether a byte slice represents valid EUC-JP.
pub fn is_valid_euc_jp(bytes: &[u8]) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b <= 0x7F {
            i += 1;
        } else if b == 0x8E && i + 1 < bytes.len() && (0xA1..=0xDF).contains(&bytes[i + 1]) {
            i += 2;
        } else if (0xA1..=0xFE).contains(&b) && i + 1 < bytes.len() && (0xA1..=0xFE).contains(&bytes[i + 1]) {
            let j1 = b - 0x80;
            let j2 = bytes[i + 1] - 0x80;
            if jis_code_to_char(j1, j2).is_some() {
                i += 2;
            } else {
                return false;
            }
        } else {
            return false;
        }
    }
    true
}

/// Validates whether a byte slice is valid for the specified character encoding.
pub fn is_valid_encoding(bytes: &[u8], encoding_name: &str) -> bool {
    match Encoding::parse(encoding_name) {
        Some(Encoding::Utf8) => std::str::from_utf8(bytes).is_ok(),
        Some(Encoding::ShiftJis) => is_valid_shift_jis(bytes),
        Some(Encoding::EucJp) => is_valid_euc_jp(bytes),
        Some(Encoding::Iso2022Jp) => {
            if bytes.windows(3).any(|w| w == b"\x1B$B" || w == b"\x1B$@") {
                true
            } else {
                bytes.iter().all(|&b| b < 0x80)
            }
        }
        Some(Encoding::Ascii) => bytes.iter().all(|&b| b < 0x80),
        Some(Encoding::Iso8859_1 | Encoding::Binary) => true,
        None => false,
    }
}

