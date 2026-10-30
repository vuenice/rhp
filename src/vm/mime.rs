//! # MIME Detection and Extension Guessing Module
//!
//! Provides content sniffing (magic bytes inspection) and file extension mapping
//! for the `File` and `AFile` facades, matching Laravel's `File::mimeType()` and
//! `File::guessExtension()` APIs.

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Maximum number of bytes to read for magic byte inspection.
const SNIFF_BUFFER_SIZE: usize = 8192;

/// Inspect a file on disk and detect its MIME type.
/// Returns `None` if the file cannot be opened or read (e.g. does not exist).
pub fn detect_mime_type_for_path(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut buffer = Vec::with_capacity(SNIFF_BUFFER_SIZE);
    let mut take = (&mut file).take(SNIFF_BUFFER_SIZE as u64);
    take.read_to_end(&mut buffer).ok()?;

    let filename = path.file_name().and_then(|f| f.to_str());
    Some(detect_mime_type_from_bytes(&buffer, filename))
}

/// Detect MIME type from an in-memory byte buffer with optional filename/path hint.
pub fn detect_mime_type_from_bytes(bytes: &[u8], filename: Option<&str>) -> String {
    // 1. Sniff magic numbers / binary signatures
    if let Some(mime) = sniff_magic_bytes(bytes, filename) {
        return mime.to_string();
    }

    // 2. Check filename extension if provided
    if let Some(name) = filename {
        if let Some(ext) = Path::new(name).extension().and_then(|e| e.to_str()) {
            if let Some(mime) = extension_to_mime(ext) {
                return mime.to_string();
            }
        }
    }

    // 3. Inspect if the content looks like valid UTF-8 text
    if !bytes.is_empty() && is_plain_text(bytes) {
        return "text/plain".to_string();
    }

    // 4. Default fallback
    "application/octet-stream".to_string()
}

/// Sniff magic bytes from content buffer.
fn sniff_magic_bytes(buf: &[u8], filename: Option<&str>) -> Option<&'static str> {
    if buf.is_empty() {
        return None;
    }

    // PNG: \x89PNG\r\n\x1a\n
    if buf.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }

    // JPEG: \xFF\xD8\xFF
    if buf.len() >= 3 && buf[0] == 0xFF && buf[1] == 0xD8 && buf[2] == 0xFF {
        return Some("image/jpeg");
    }

    // GIF: GIF87a or GIF89a
    if buf.starts_with(b"GIF87a") || buf.starts_with(b"GIF89a") {
        return Some("image/gif");
    }

    // WebP: RIFF....WEBP
    if buf.len() >= 12 && &buf[0..4] == b"RIFF" && &buf[8..12] == b"WEBP" {
        return Some("image/webp");
    }

    // AVIF: ....ftypavif or ....ftypavis
    if buf.len() >= 12 && &buf[4..8] == b"ftyp" && (&buf[8..12] == b"avif" || &buf[8..12] == b"avis") {
        return Some("image/avif");
    }

    // BMP: BM
    if buf.starts_with(b"BM") {
        return Some("image/bmp");
    }

    // TIFF: II*\0 or MM\0*
    if buf.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || buf.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        return Some("image/tiff");
    }

    // ICO: \x00\x00\x01\x00
    if buf.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Some("image/x-icon");
    }

    // PDF: %PDF-
    if buf.starts_with(b"%PDF-") {
        return Some("application/pdf");
    }

    // PostScript: %!PS
    if buf.starts_with(b"%!PS") {
        return Some("application/postscript");
    }

    // RTF: {\rtf
    if buf.starts_with(b"{\\rtf") {
        return Some("application/rtf");
    }

    // ZIP: PK\x03\x04
    if buf.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        if let Some(name) = filename {
            let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("");
            let ext_lower = ext.to_ascii_lowercase();
            match ext_lower.as_str() {
                "docx" => return Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
                "xlsx" => return Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
                "pptx" => return Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
                "jar" => return Some("application/java-archive"),
                "apk" => return Some("application/vnd.android.package-archive"),
                "epub" => return Some("application/epub+zip"),
                _ => {}
            }
        }
        return Some("application/zip");
    }

    // Legacy OLE2 (Compound File Binary Format): doc, xls, ppt
    if buf.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        if let Some(name) = filename {
            let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("");
            let ext_lower = ext.to_ascii_lowercase();
            match ext_lower.as_str() {
                "doc" => return Some("application/msword"),
                "xls" => return Some("application/vnd.ms-excel"),
                "ppt" => return Some("application/vnd.ms-powerpoint"),
                _ => {}
            }
        }
        return Some("application/x-ole-storage");
    }

    // GZIP: \x1F\x8B
    if buf.starts_with(&[0x1F, 0x8B]) {
        return Some("application/gzip");
    }

    // 7-Zip: 7z\xBC\xAF\x27\x1C
    if buf.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return Some("application/x-7z-compressed");
    }

    // RAR: Rar!\x1A\x07
    if buf.starts_with(b"Rar!\x1A\x07") {
        return Some("application/x-rar-compressed");
    }

    // Bzip2: BZh
    if buf.starts_with(b"BZh") {
        return Some("application/x-bzip2");
    }

    // XZ: \xFD7zXZ\x00
    if buf.starts_with(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]) {
        return Some("application/x-xz");
    }

    // Zstandard: \x28\xB5\x2F\xFD
    if buf.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        return Some("application/zstd");
    }

    // TAR: ustar at offset 257
    if buf.len() >= 262 && &buf[257..262] == b"ustar" {
        return Some("application/x-tar");
    }

    // WebAssembly: \0asm
    if buf.starts_with(&[0x00, 0x61, 0x73, 0x6D]) {
        return Some("application/wasm");
    }

    // MP3: ID3 or frame sync
    if buf.starts_with(b"ID3")
        || (buf.len() >= 2 && buf[0] == 0xFF && (buf[1] & 0xFE == 0xFA || buf[1] & 0xFE == 0xF2))
    {
        return Some("audio/mpeg");
    }

    // WAV: RIFF....WAVE
    if buf.len() >= 12 && &buf[0..4] == b"RIFF" && &buf[8..12] == b"WAVE" {
        return Some("audio/wav");
    }

    // Ogg: OggS
    if buf.starts_with(b"OggS") {
        return Some("audio/ogg");
    }

    // FLAC: fLaC
    if buf.starts_with(b"fLaC") {
        return Some("audio/flac");
    }

    // MIDI: MThd
    if buf.starts_with(b"MThd") {
        return Some("audio/midi");
    }

    // MP4 / QuickTime / M4A: ....ftyp
    if buf.len() >= 12 && &buf[4..8] == b"ftyp" {
        let brand = &buf[8..12];
        if brand == b"M4A " || brand == b"M4B " {
            return Some("audio/mp4");
        } else if brand == b"qt  " {
            return Some("video/quicktime");
        } else {
            return Some("video/mp4");
        }
    }

    // WebM / Matroska: \x1A\x45\xDF\xA3
    if buf.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        if buf.windows(4).any(|w| w == b"webm") {
            return Some("video/webm");
        }
        return Some("video/x-matroska");
    }

    // AVI: RIFF....AVI
    if buf.len() >= 12 && &buf[0..4] == b"RIFF" && &buf[8..12] == b"AVI " {
        return Some("video/x-msvideo");
    }

    // WOFF: wOFF
    if buf.starts_with(b"wOFF") {
        return Some("font/woff");
    }

    // WOFF2: wOF2
    if buf.starts_with(b"wOF2") {
        return Some("font/woff2");
    }

    // TrueType / OpenType font
    if buf.starts_with(&[0x00, 0x01, 0x00, 0x00]) || buf.starts_with(b"true") {
        return Some("font/ttf");
    }
    if buf.starts_with(b"OTTO") {
        return Some("font/otf");
    }

    // Windows PE Executable: MZ
    if buf.starts_with(b"MZ") {
        return Some("application/x-dosexec");
    }

    // ELF Executable: \x7FELF
    if buf.starts_with(&[0x7F, b'E', b'L', b'F']) {
        return Some("application/x-executable");
    }

    // Mach-O binary
    if buf.starts_with(&[0xFE, 0xED, 0xFA, 0xCE])
        || buf.starts_with(&[0xFE, 0xED, 0xFA, 0xCF])
        || buf.starts_with(&[0xCF, 0xFA, 0xED, 0xFE])
        || buf.starts_with(&[0xCE, 0xFA, 0xED, 0xFE])
    {
        return Some("application/x-mach-binary");
    }

    // SQLite 3
    if buf.starts_with(b"SQLite format 3\0") {
        return Some("application/x-sqlite3");
    }

    // Text-based heuristics (XML, HTML, SVG, JSON)
    let sample_len = buf.len().min(1024);
    if let Ok(text) = std::str::from_utf8(&buf[..sample_len]) {
        let trimmed = text.trim_start_matches('\u{feff}').trim();
        let lower = trimmed.to_ascii_lowercase();

        if lower.starts_with("<?xml") {
            if lower.contains("<svg") {
                return Some("image/svg+xml");
            }
            return Some("application/xml");
        }

        if lower.starts_with("<!doctype html") || lower.starts_with("<html") || lower.starts_with("<head") || lower.starts_with("<body") {
            return Some("text/html");
        }

        if lower.starts_with("<svg") {
            return Some("image/svg+xml");
        }

        if (trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']'))
        {
            if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
                return Some("application/json");
            }
        }
    }

    None
}

/// Check if byte slice contains valid UTF-8 text with no control characters (except common whitespace).
fn is_plain_text(bytes: &[u8]) -> bool {
    let check_len = bytes.len().min(4096);
    let slice = &bytes[..check_len];
    if let Ok(s) = std::str::from_utf8(slice) {
        // Reject if contains null byte
        !s.chars().any(|c| c == '\0')
    } else {
        false
    }
}

/// Guess the canonical file extension from a file path or MIME type string.
/// Matches Laravel's `File::guessExtension()`.
pub fn guess_extension(mime_or_path: &str) -> Option<&'static str> {
    let trimmed = mime_or_path.trim();

    // 1. If the path exists on disk, detect its MIME type from content and map to extension
    let p = Path::new(trimmed);
    if p.exists() {
        if let Some(detected) = detect_mime_type_for_path(p) {
            if let Some(ext) = mime_to_extension(&detected) {
                return Some(ext);
            }
        }
    }

    // 2. Try looking up as a direct MIME type (e.g. "image/png" -> "png")
    if let Some(ext) = mime_to_extension(trimmed) {
        return Some(ext);
    }

    // 3. Fallback: Check if the path string has an extension with a known MIME mapping
    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
        let lower = ext.to_ascii_lowercase();
        if let Some(mime) = extension_to_mime(&lower) {
            if let Some(canonical) = mime_to_extension(mime) {
                return Some(canonical);
            }
        }
    }

    None
}

/// Map file extension to standard MIME type.
pub fn extension_to_mime(ext: &str) -> Option<&'static str> {
    let lower = ext.trim_start_matches('.').to_ascii_lowercase();
    match lower.as_str() {
        // Images
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" | "svgz" => Some("image/svg+xml"),
        "ico" => Some("image/x-icon"),
        "bmp" => Some("image/bmp"),
        "tiff" | "tif" => Some("image/tiff"),
        "avif" => Some("image/avif"),
        "heic" => Some("image/heic"),
        "heif" => Some("image/heif"),

        // Documents
        "pdf" => Some("application/pdf"),
        "doc" => Some("application/msword"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "xls" => Some("application/vnd.ms-excel"),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "ppt" => Some("application/vnd.ms-powerpoint"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "odt" => Some("application/vnd.oasis.opendocument.text"),
        "ods" => Some("application/vnd.oasis.opendocument.spreadsheet"),
        "odp" => Some("application/vnd.oasis.opendocument.presentation"),
        "rtf" => Some("application/rtf"),

        // Web & Text
        "html" | "htm" => Some("text/html"),
        "css" => Some("text/css"),
        "js" | "mjs" | "cjs" => Some("application/javascript"),
        "ts" | "tsx" => Some("application/typescript"),
        "json" => Some("application/json"),
        "jsonld" => Some("application/ld+json"),
        "xml" => Some("application/xml"),
        "txt" => Some("text/plain"),
        "csv" => Some("text/csv"),
        "tsv" => Some("text/tab-separated-values"),
        "yaml" | "yml" => Some("text/yaml"),
        "md" | "markdown" => Some("text/markdown"),

        // Archives
        "zip" => Some("application/zip"),
        "tar" => Some("application/x-tar"),
        "gz" | "gzip" | "tgz" => Some("application/gzip"),
        "bz2" => Some("application/x-bzip2"),
        "7z" => Some("application/x-7z-compressed"),
        "rar" => Some("application/x-rar-compressed"),
        "xz" => Some("application/x-xz"),
        "zst" => Some("application/zstd"),
        "jar" => Some("application/java-archive"),
        "apk" => Some("application/vnd.android.package-archive"),
        "epub" => Some("application/epub+zip"),

        // Audio
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "ogg" | "oga" => Some("audio/ogg"),
        "flac" => Some("audio/flac"),
        "aac" => Some("audio/aac"),
        "m4a" => Some("audio/mp4"),
        "mid" | "midi" => Some("audio/midi"),
        "wma" => Some("audio/x-ms-wma"),

        // Video
        "mp4" | "m4v" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mkv" => Some("video/x-matroska"),
        "avi" => Some("video/x-msvideo"),
        "mov" => Some("video/quicktime"),
        "wmv" => Some("video/x-ms-wmv"),
        "flv" => Some("video/x-flv"),

        // Fonts
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),
        "ttf" => Some("font/ttf"),
        "otf" => Some("font/otf"),
        "eot" => Some("application/vnd.ms-fontobject"),

        // Code & Scripts
        "php" | "rhp" => Some("text/x-php"),
        "rs" => Some("text/x-rust"),
        "py" => Some("text/x-python"),
        "sh" | "bash" => Some("application/x-sh"),
        "sql" => Some("application/sql"),
        "wasm" => Some("application/wasm"),

        _ => None,
    }
}

/// Map MIME type to standard canonical file extension.
pub fn mime_to_extension(mime: &str) -> Option<&'static str> {
    let clean = mime.split(';').next().unwrap_or(mime).trim().to_ascii_lowercase();
    match clean.as_str() {
        // Images
        "image/jpeg" => Some("jpeg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/svg+xml" => Some("svg"),
        "image/x-icon" | "image/vnd.microsoft.icon" => Some("ico"),
        "image/bmp" => Some("bmp"),
        "image/tiff" => Some("tiff"),
        "image/avif" => Some("avif"),
        "image/heic" => Some("heic"),
        "image/heif" => Some("heif"),

        // Documents
        "application/pdf" => Some("pdf"),
        "application/msword" => Some("doc"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some("docx"),
        "application/vnd.ms-excel" => Some("xls"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some("xlsx"),
        "application/vnd.ms-powerpoint" => Some("ppt"),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => Some("pptx"),
        "application/vnd.oasis.opendocument.text" => Some("odt"),
        "application/vnd.oasis.opendocument.spreadsheet" => Some("ods"),
        "application/vnd.oasis.opendocument.presentation" => Some("odp"),
        "application/rtf" => Some("rtf"),
        "application/postscript" => Some("ps"),

        // Web & Text
        "text/html" => Some("html"),
        "text/css" => Some("css"),
        "application/javascript" | "text/javascript" => Some("js"),
        "application/typescript" => Some("ts"),
        "application/json" => Some("json"),
        "application/ld+json" => Some("jsonld"),
        "application/xml" | "text/xml" => Some("xml"),
        "text/plain" => Some("txt"),
        "text/csv" => Some("csv"),
        "text/tab-separated-values" => Some("tsv"),
        "text/yaml" | "application/x-yaml" => Some("yaml"),
        "text/markdown" | "text/x-markdown" => Some("md"),

        // Archives
        "application/zip" => Some("zip"),
        "application/x-tar" => Some("tar"),
        "application/gzip" => Some("gz"),
        "application/x-bzip2" => Some("bz2"),
        "application/x-7z-compressed" => Some("7z"),
        "application/x-rar-compressed" => Some("rar"),
        "application/x-xz" => Some("xz"),
        "application/zstd" => Some("zst"),
        "application/java-archive" => Some("jar"),
        "application/vnd.android.package-archive" => Some("apk"),
        "application/epub+zip" => Some("epub"),

        // Audio
        "audio/mpeg" => Some("mp3"),
        "audio/wav" | "audio/x-wav" => Some("wav"),
        "audio/ogg" => Some("ogg"),
        "audio/flac" => Some("flac"),
        "audio/aac" => Some("aac"),
        "audio/midi" => Some("mid"),
        "audio/x-ms-wma" => Some("wma"),

        // Video
        "video/mp4" => Some("mp4"),
        "video/webm" => Some("webm"),
        "video/x-matroska" => Some("mkv"),
        "video/x-msvideo" => Some("avi"),
        "video/quicktime" => Some("mov"),
        "video/x-ms-wmv" => Some("wmv"),
        "video/x-flv" => Some("flv"),

        // Fonts
        "font/woff" => Some("woff"),
        "font/woff2" => Some("woff2"),
        "font/ttf" => Some("ttf"),
        "font/otf" => Some("otf"),
        "application/vnd.ms-fontobject" => Some("eot"),

        // Code & Binary
        "text/x-php" => Some("php"),
        "text/x-rust" => Some("rs"),
        "text/x-python" => Some("py"),
        "application/x-sh" => Some("sh"),
        "application/sql" => Some("sql"),
        "application/wasm" => Some("wasm"),
        "application/x-sqlite3" => Some("sqlite"),
        "application/octet-stream" => Some("bin"),

        _ => None,
    }
}
