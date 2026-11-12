use std::fs;
use std::io;
use std::path::Path;
use regex::Regex;
use serde::Serialize;

/// Options for the migrate command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrateOptions {
    /// Whether to write modifications to disk (true) or check only / dry run (false).
    pub fix: bool,
}

impl Default for MigrateOptions {
    fn default() -> Self {
        Self { fix: true }
    }
}

/// Result of migrating a single file.
#[derive(Debug, Clone, Serialize)]
pub struct MigrateFileResult {
    pub file_path: String,
    pub modified: bool,
    pub added_lines: Vec<String>,
    pub cookie_set_count: usize,
    pub global_replace_count: usize,
    pub dot_concat_count: usize,
    #[serde(skip)]
    pub original_content: String,
    #[serde(skip)]
    pub migrated_content: String,
}

impl MigrateFileResult {
    #[inline]
    pub fn upgraded_content(&self) -> &str {
        &self.migrated_content
    }
}

/// Aggregated report of migrating multiple files.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MigrateReport {
    pub files_scanned: usize,
    pub files_modified: usize,
    pub results: Vec<MigrateFileResult>,
}

impl MigrateReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn to_human(&self, dry_run: bool) -> String {
        let mut out = String::new();
        out.push_str("=======================================================================\n");
        if dry_run {
            out.push_str("🔍 RHP PHP Migration Check (Dry Run - No files modified)\n");
        } else {
            out.push_str("🚀 RHP PHP Migration Tool\n");
        }
        out.push_str("=======================================================================\n");
        out.push_str(&format!("  Files Scanned:  {}\n", self.files_scanned));
        out.push_str(&format!("  Files Modified: {}\n", self.files_modified));

        if self.files_modified > 0 {
            out.push_str("\nMigrated Files:\n");
            for res in &self.results {
                if res.modified {
                    out.push_str(&format!("  • {}\n", res.file_path));
                    for line in &res.added_lines {
                        out.push_str(&format!("      + {}\n", line));
                    }
                    if res.dot_concat_count > 0 {
                        out.push_str(&format!("      ~ {} dot concatenation operator(s) replaced with '+'\n", res.dot_concat_count));
                    }
                    if res.cookie_set_count > 0 {
                        out.push_str(&format!("      ~ {} cookie set call(s) upgraded to Cookie::set()\n", res.cookie_set_count));
                    }
                    if res.global_replace_count > 0 {
                        out.push_str(&format!("      ~ {} $_GLOBAL / $GLOBALS replaced with original global variable(s)\n", res.global_replace_count));
                    }
                }
            }
        } else {
            out.push_str("\nNo PHP legacy superglobals or globals required migrating.\n");
        }
        out
    }
}

// Backwards-compatible aliases
pub type UpgradeOptions = MigrateOptions;
pub type UpgradeFileResult = MigrateFileResult;
pub type UpgradeReport = MigrateReport;
pub use migrate_source as upgrade_php_source;
pub use migrate_file as upgrade_php_file;
pub use migrate_path as upgrade_php_path;

/// Migrate a source string in memory.
pub fn migrate_source(source: &str, file_name: &str) -> MigrateFileResult {
    let mut current = source.to_string();
    let mut cookie_set_count = 0;
    let mut global_replace_count = 0;
    let mut dot_concat_count = 0;

    // Step 1: Replace $_GLOBAL and $GLOBALS with original global values
    let (after_globals, g_count) = replace_globals(&current);
    global_replace_count += g_count;
    current = after_globals;

    // Step 2: Upgrade cookie sets to Cookie::set(...)
    let (after_cookies, c_count) = upgrade_cookie_sets(&current);
    cookie_set_count += c_count;
    current = after_cookies;

    // Step 3: Replace dot concatenation (. and .=) with + and +=
    let (after_dots, d_count) = replace_dot_operators(&current);
    dot_concat_count += d_count;
    current = after_dots;

    // Step 4: Detect used superglobals outside comments/strings and add extra lines
    let (after_lines, added_lines) = add_superglobal_declarations(&current);
    current = after_lines;

    let modified = current != source;

    MigrateFileResult {
        file_path: file_name.to_string(),
        modified,
        added_lines,
        cookie_set_count,
        global_replace_count,
        dot_concat_count,
        original_content: source.to_string(),
        migrated_content: current,
    }
}

/// Migrate a single file on disk.
pub fn migrate_file(path: &Path, options: MigrateOptions) -> io::Result<MigrateFileResult> {
    let source = fs::read_to_string(path)?;
    let file_path = path.display().to_string();
    let result = migrate_source(&source, &file_path);

    if result.modified && options.fix {
        fs::write(path, &result.migrated_content)?;
    }

    Ok(result)
}

/// Migrate a path (either a single file or a directory recursively).
pub fn migrate_path(target: &Path, options: MigrateOptions) -> io::Result<MigrateReport> {
    let mut report = MigrateReport::new();

    if target.is_file() {
        let res = migrate_file(target, options)?;
        report.files_scanned += 1;
        if res.modified {
            report.files_modified += 1;
        }
        report.results.push(res);
        return Ok(report);
    }

    if target.is_dir() {
        migrate_directory_recursive(target, options, &mut report)?;
    }

    Ok(report)
}

fn migrate_directory_recursive(dir: &Path, options: MigrateOptions, report: &mut MigrateReport) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            if name == "target" || name == ".git" || name == "vendor" || name == "node_modules" {
                continue;
            }
            migrate_directory_recursive(&path, options, report)?;
        } else if is_php_or_rhp_file(&path) {
            let res = migrate_file(&path, options)?;
            report.files_scanned += 1;
            if res.modified {
                report.files_modified += 1;
            }
            report.results.push(res);
        }
    }

    Ok(())
}

fn is_php_or_rhp_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        ext == "php" || ext == "rhp"
    } else {
        false
    }
}

// =========================================================================
// Code Masking: Computes boolean vector indicating if byte is in code
// (outside comments and string literals)
// =========================================================================

fn build_code_mask(source: &str) -> Vec<bool> {
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut is_code = vec![true; n];
    let mut i = 0;

    while i < n {
        let b = bytes[i];

        // Single-line comment: // or #
        if (b == b'/' && i + 1 < n && bytes[i + 1] == b'/') || b == b'#' {
            while i < n && bytes[i] != b'\n' {
                is_code[i] = false;
                i += 1;
            }
            continue;
        }

        // Multi-line comment: /* ... */
        if b == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            is_code[i] = false;
            is_code[i + 1] = false;
            i += 2;
            while i < n {
                is_code[i] = false;
                if bytes[i] == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                    is_code[i + 1] = false;
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Single-quoted string: '...'
        if b == b'\'' {
            is_code[i] = false;
            i += 1;
            while i < n {
                is_code[i] = false;
                if bytes[i] == b'\\' && i + 1 < n {
                    is_code[i + 1] = false;
                    i += 2;
                    continue;
                }
                if bytes[i] == b'\'' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Double-quoted string: "..."
        if b == b'"' {
            is_code[i] = false;
            i += 1;
            while i < n {
                is_code[i] = false;
                if bytes[i] == b'\\' && i + 1 < n {
                    is_code[i + 1] = false;
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Backtick string: `...`
        if b == b'`' {
            is_code[i] = false;
            i += 1;
            while i < n {
                is_code[i] = false;
                if bytes[i] == b'\\' && i + 1 < n {
                    is_code[i + 1] = false;
                    i += 2;
                    continue;
                }
                if bytes[i] == b'`' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        i += 1;
    }

    is_code
}

// =========================================================================
// Step 1: Replace $_GLOBAL and $GLOBALS with original global values
// =========================================================================

struct TextEdit {
    start: usize,
    end: usize,
    replacement: String,
}

/// Replace `$_GLOBAL['var']`, `$_GLOBALS['var']`, and `$GLOBALS['var']` with `$var`.
/// Dynamic variable access `$GLOBALS[$k]` becomes `$$k`.
/// Standalone `$_GLOBAL` becomes `$GLOBALS`.
fn replace_globals(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let mut edits: Vec<TextEdit> = Vec::new();

    let re_literal = Regex::new(
        r#"(?:\$_GLOBAL|\$_GLOBALS|\$GLOBALS)\s*\[\s*['"]([a-zA-Z_][a-zA-Z0-9_]*)['"]\s*\]"#,
    ).unwrap();

    let re_dynamic = Regex::new(
        r#"(?:\$_GLOBAL|\$_GLOBALS|\$GLOBALS)\s*\[\s*\$([a-zA-Z_][a-zA-Z0-9_]*)\s*\]"#,
    ).unwrap();

    let re_standalone = Regex::new(r"\$_GLOBAL\b").unwrap();

    // 1. Literal subscript replacements: $_GLOBAL['foo'] -> $foo
    for caps in re_literal.captures_iter(source) {
        let mat = caps.get(0).unwrap();
        if mask[mat.start()] {
            let var_name = caps.get(1).unwrap().as_str();
            edits.push(TextEdit {
                start: mat.start(),
                end: mat.end(),
                replacement: format!("${}", var_name),
            });
        }
    }

    // 2. Dynamic subscript replacements: $_GLOBAL[$foo] -> $$foo
    for caps in re_dynamic.captures_iter(source) {
        let mat = caps.get(0).unwrap();
        if mask[mat.start()] {
            let var_name = caps.get(1).unwrap().as_str();
            edits.push(TextEdit {
                start: mat.start(),
                end: mat.end(),
                replacement: format!("$${}", var_name),
            });
        }
    }

    // Sort edits descending by start offset
    edits.sort_by(|a, b| b.start.cmp(&a.start));

    let mut result = source.to_string();
    let count = edits.len();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    // 3. Standalone $_GLOBAL -> $GLOBALS on any remaining instances
    let mask_after = build_code_mask(&result);
    let mut standalone_edits: Vec<TextEdit> = Vec::new();
    for mat in re_standalone.find_iter(&result) {
        if mask_after[mat.start()] {
            standalone_edits.push(TextEdit {
                start: mat.start(),
                end: mat.end(),
                replacement: "$GLOBALS".to_string(),
            });
        }
    }
    standalone_edits.sort_by(|a, b| b.start.cmp(&a.start));
    let standalone_count = standalone_edits.len();
    for edit in standalone_edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count + standalone_count)
}

// =========================================================================
// Step 2: Upgrade cookie sets to Cookie::set(...)
// =========================================================================

/// Upgrades:
/// - `$_COOKIE['key'] = $val;` -> `Cookie::set('key', $val);`
/// - `setcookie(...)` -> `Cookie::set(...)`
/// - `setrawcookie(...)` -> `Cookie::set(...)`
fn upgrade_cookie_sets(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let mut edits: Vec<TextEdit> = Vec::new();

    let re_cookie_assign = Regex::new(
        r#"\$_COOKIE\s*\[\s*(['"][^'"]+['"]|\$[a-zA-Z_][a-zA-Z0-9_]*)\s*\]\s*=\s*([^;]+);"#
    ).unwrap();

    let re_setcookie = Regex::new(r"(?:\b|\\)(setcookie|setrawcookie)\s*\(").unwrap();

    // 1. $_COOKIE[...] = ...; -> Cookie::set(...);
    for caps in re_cookie_assign.captures_iter(source) {
        let mat = caps.get(0).unwrap();
        if mask[mat.start()] {
            let key = caps.get(1).unwrap().as_str();
            let val = caps.get(2).unwrap().as_str().trim();
            edits.push(TextEdit {
                start: mat.start(),
                end: mat.end(),
                replacement: format!("Cookie::set({}, {});", key, val),
            });
        }
    }

    // 2. setcookie(...) / setrawcookie(...) -> Cookie::set(...)
    for caps in re_setcookie.captures_iter(source) {
        let mat = caps.get(0).unwrap();
        if mask[mat.start()] {
            edits.push(TextEdit {
                start: mat.start(),
                end: mat.end(),
                replacement: "Cookie::set(".to_string(),
            });
        }
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start));
    let count = edits.len();

    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

// =========================================================================
// Step 3: Replace dot concatenation (. and .=) with + and +=
// =========================================================================

/// Replace PHP dot concatenation (`.` and `.=`) with `+` and `+=`.
/// Ignores occurrences in comments and string literals (via mask).
/// Preserves `...` (ellipsis), `..` (range), and floating point numbers (e.g. `3.14`, `0.5`).
fn replace_dot_operators(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();

    let mut i = 0;
    while i < n {
        if !mask[i] {
            i += 1;
            continue;
        }

        // 1. Check for `...` (ellipsis / splat)
        if bytes[i] == b'.' && i + 2 < n && bytes[i + 1] == b'.' && bytes[i + 2] == b'.' {
            i += 3;
            continue;
        }

        // 2. Check for `..` (range)
        if bytes[i] == b'.' && i + 1 < n && bytes[i + 1] == b'.' {
            i += 2;
            continue;
        }

        // 3. Check for `.=` (dot-assign)
        if bytes[i] == b'.' && i + 1 < n && bytes[i + 1] == b'=' {
            // Ensure not preceded by '.'
            if i > 0 && bytes[i - 1] == b'.' {
                i += 2;
                continue;
            }

            let has_ws_before = i > 0 && bytes[i - 1].is_ascii_whitespace();
            let has_ws_after = i + 2 < n && bytes[i + 2].is_ascii_whitespace();

            let replacement = match (has_ws_before, has_ws_after) {
                (true, true) => "+=".to_string(),
                (false, false) => " += ".to_string(),
                (false, true) => " +=".to_string(),
                (true, false) => "+= ".to_string(),
            };

            edits.push(TextEdit {
                start: i,
                end: i + 2,
                replacement,
            });
            i += 2;
            continue;
        }

        // 4. Check for standalone `.`
        if bytes[i] == b'.' {
            let prev_is_dot = i > 0 && bytes[i - 1] == b'.';
            let next_is_dot = i + 1 < n && bytes[i + 1] == b'.';
            if prev_is_dot || next_is_dot {
                i += 1;
                continue;
            }

            // Check if this dot is part of a floating-point number:
            // a) Digit immediately before AND digit immediately after: e.g. 3.14
            let prev_is_digit = i > 0 && bytes[i - 1].is_ascii_digit();
            let next_is_digit = i + 1 < n && bytes[i + 1].is_ascii_digit();
            if prev_is_digit && next_is_digit {
                i += 1;
                continue;
            }

            // b) Dot immediately followed by a digit (e.g. `.5`):
            if next_is_digit {
                let mut p = i;
                let mut prev_non_ws = None;
                while p > 0 {
                    p -= 1;
                    if !bytes[p].is_ascii_whitespace() {
                        prev_non_ws = Some(bytes[p]);
                        break;
                    }
                }

                let is_float = match prev_non_ws {
                    None => true,
                    Some(b'=' | b'(' | b'[' | b'{' | b',' | b';' | b':' | b'?' | b'!' | b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>' | b'&' | b'|' | b'^' | b'~') => true,
                    _ => false,
                };

                if is_float {
                    i += 1;
                    continue;
                }
            }

            // This is a string concatenation operator!
            let has_ws_before = i > 0 && bytes[i - 1].is_ascii_whitespace();
            let has_ws_after = i + 1 < n && bytes[i + 1].is_ascii_whitespace();

            let replacement = match (has_ws_before, has_ws_after) {
                (true, true) => "+".to_string(),
                (false, false) => " + ".to_string(),
                (false, true) => " +".to_string(),
                (true, false) => "+ ".to_string(),
            };

            edits.push(TextEdit {
                start: i,
                end: i + 1,
                replacement,
            });

            i += 1;
            continue;
        }

        i += 1;
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start));
    let count = edits.len();

    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

// =========================================================================
// Step 4: Add extra lines if variables are used
// =========================================================================

/// Scan code outside comments/strings and add extra lines for used superglobals.
fn add_superglobal_declarations(source: &str) -> (String, Vec<String>) {
    let mask = build_code_mask(source);

    // Helpers to check if a pattern matches outside comments and strings
    let has_code_match = |re: &Regex| -> bool {
        for mat in re.find_iter(source) {
            if mask[mat.start()] {
                return true;
            }
        }
        false
    };

    // Check if an existing initialization line is present in code
    let re_has_get_init = Regex::new(r"\$_GET\s*=\s*Request::query\s*\(\s*\)\s*;").unwrap();
    let re_has_post_init = Regex::new(r"\$_POST\s*=\s*Request::body\s*\(\s*\)\s*;").unwrap();
    let re_has_server_method_init = Regex::new(r#"\$_SERVER\s*\[\s*['"]REQUEST_METHOD['"]\s*\]\s*=\s*Request::method\s*\(\s*\)\s*;"#).unwrap();
    let re_has_server_ip_init = Regex::new(r#"\$_SERVER\s*\[\s*['"]REMOTE_ADDR['"]\s*\]\s*=\s*Request::ip\s*\(\s*\)\s*;"#).unwrap();
    let re_has_server_host_init = Regex::new(r#"\$_SERVER\s*\[\s*['"]HTTP_HOST['"]\s*\]\s*=\s*Request::host\s*\(\s*\)\s*;"#).unwrap();
    let re_has_files_init = Regex::new(r"\$_FILES\s*=\s*Request::files\s*\(\s*\)\s*;").unwrap();
    let re_has_cookie_init = Regex::new(r"\$_COOKIE\s*=\s*Cookie::get\s*\(\s*\)\s*;").unwrap();

    let has_get_init = has_code_match(&re_has_get_init);
    let has_post_init = has_code_match(&re_has_post_init);
    let has_server_method_init = has_code_match(&re_has_server_method_init);
    let has_server_ip_init = has_code_match(&re_has_server_ip_init);
    let has_server_host_init = has_code_match(&re_has_server_host_init);
    let has_files_init = has_code_match(&re_has_files_init);
    let has_cookie_init = has_code_match(&re_has_cookie_init);

    // Check if variables are used outside of the initialization statements themselves
    let is_var_used = |re_var: &Regex, re_init: &Regex| -> bool {
        for mat in re_var.find_iter(source) {
            if mask[mat.start()] {
                // Check if this occurrence is part of an initialization statement
                let is_init_part = re_init.find_iter(source).any(|init_mat| {
                    mat.start() >= init_mat.start() && mat.end() <= init_mat.end()
                });
                if !is_init_part {
                    return true;
                }
            }
        }
        false
    };

    let re_get = Regex::new(r"\$_GET\b").unwrap();
    let re_post = Regex::new(r"\$_POST\b").unwrap();
    let re_server_method = Regex::new(r#"\$_SERVER\s*\[\s*['"]REQUEST_METHOD['"]\s*\]"#).unwrap();
    let re_server_ip = Regex::new(r#"\$_SERVER\s*\[\s*['"]REMOTE_ADDR['"]\s*\]"#).unwrap();
    let re_server_host = Regex::new(r#"\$_SERVER\s*\[\s*['"]HTTP_HOST['"]\s*\]"#).unwrap();
    let re_files = Regex::new(r"\$_FILES\b").unwrap();
    let re_cookie = Regex::new(r"\$_COOKIE\b").unwrap();

    let used_get = is_var_used(&re_get, &re_has_get_init);
    let used_post = is_var_used(&re_post, &re_has_post_init);
    let used_server_method = is_var_used(&re_server_method, &re_has_server_method_init);
    let used_server_ip = is_var_used(&re_server_ip, &re_has_server_ip_init);
    let used_server_host = is_var_used(&re_server_host, &re_has_server_host_init);
    let used_files = is_var_used(&re_files, &re_has_files_init);
    let used_cookie = is_var_used(&re_cookie, &re_has_cookie_init);

    let mut extra_lines = Vec::new();

    if used_get && !has_get_init {
        extra_lines.push("$_GET = Request::query();".to_string());
    }
    if used_post && !has_post_init {
        extra_lines.push("$_POST = Request::body();".to_string());
    }
    if used_server_method && !has_server_method_init {
        extra_lines.push("$_SERVER['REQUEST_METHOD'] = Request::method();".to_string());
    }
    if used_server_ip && !has_server_ip_init {
        extra_lines.push("$_SERVER['REMOTE_ADDR'] = Request::ip();".to_string());
    }
    if used_server_host && !has_server_host_init {
        extra_lines.push("$_SERVER['HTTP_HOST'] = Request::host();".to_string());
    }
    if used_files && !has_files_init {
        extra_lines.push("$_FILES = Request::files();".to_string());
    }
    if used_cookie && !has_cookie_init {
        extra_lines.push("$_COOKIE = Cookie::get();".to_string());
    }

    if extra_lines.is_empty() {
        return (source.to_string(), Vec::new());
    }

    let insert_pos = find_insertion_point(source);
    let lines_block = extra_lines.join("\n");

    let mut result = String::with_capacity(source.len() + lines_block.len() + 4);
    result.push_str(&source[..insert_pos]);

    let rest = &source[insert_pos..];
    if rest.starts_with("\r\n") || rest.starts_with('\n') {
        result.push_str(&lines_block);
        result.push('\n');
    } else if insert_pos == source.len() {
        result.push_str(&lines_block);
        result.push('\n');
    } else {
        result.push_str(&lines_block);
        result.push_str("\n\n");
    }
    result.push_str(rest);

    (result, extra_lines)
}

/// Determines the best byte position to insert superglobal initialization lines.
fn find_insertion_point(source: &str) -> usize {
    let trimmed_start = source.trim_start();
    let leading_ws = source.len() - trimmed_start.len();

    if trimmed_start.starts_with("<?php") || trimmed_start.starts_with("<?=") || trimmed_start.starts_with("<?") {
        let tag_len = if trimmed_start.starts_with("<?php") {
            5
        } else if trimmed_start.starts_with("<?=") {
            3
        } else {
            2
        };
        let mut idx = leading_ws + tag_len;

        // Skip remainder of the <?php line
        while idx < source.len() && source.as_bytes()[idx] != b'\n' {
            idx += 1;
        }
        if idx < source.len() && source.as_bytes()[idx] == b'\n' {
            idx += 1;
        }

        // Check if declare(...) follows
        let rem = &source[idx..];
        let trimmed_rem = rem.trim_start();
        if trimmed_rem.starts_with("declare") {
            let offset_to_declare = rem.len() - trimmed_rem.len();
            if let Some(semi_pos) = trimmed_rem.find(';') {
                let mut after_declare = idx + offset_to_declare + semi_pos + 1;
                while after_declare < source.len() && source.as_bytes()[after_declare] != b'\n' {
                    after_declare += 1;
                }
                if after_declare < source.len() && source.as_bytes()[after_declare] == b'\n' {
                    after_declare += 1;
                }
                idx = after_declare;
            }
        }

        // Check if namespace ...; follows
        let rem = &source[idx..];
        let trimmed_rem = rem.trim_start();
        if trimmed_rem.starts_with("namespace ") {
            let offset_to_ns = rem.len() - trimmed_rem.len();
            if let Some(semi_pos) = trimmed_rem.find(';') {
                let mut after_ns = idx + offset_to_ns + semi_pos + 1;
                while after_ns < source.len() && source.as_bytes()[after_ns] != b'\n' {
                    after_ns += 1;
                }
                if after_ns < source.len() && source.as_bytes()[after_ns] == b'\n' {
                    after_ns += 1;
                }
                idx = after_ns;
            }
        }

        idx
    } else {
        // No <?php tag
        let trimmed = source.trim_start();
        if trimmed.starts_with("namespace ") {
            let offset_to_ns = source.len() - trimmed.len();
            if let Some(semi_pos) = trimmed.find(';') {
                let mut after_ns = offset_to_ns + semi_pos + 1;
                while after_ns < source.len() && source.as_bytes()[after_ns] != b'\n' {
                    after_ns += 1;
                }
                if after_ns < source.len() && source.as_bytes()[after_ns] == b'\n' {
                    after_ns += 1;
                }
                return after_ns;
            }
        }
        0
    }
}
