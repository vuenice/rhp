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
    pub php_tag_removed: bool,
    pub curl_migrated_count: usize,
    pub curl_multi_count: usize,
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
                    if res.curl_migrated_count > 0 {
                        out.push_str(&format!("      ~ {} cURL call(s) upgraded to Http facade\n", res.curl_migrated_count));
                    }
                    if res.curl_multi_count > 0 {
                        out.push_str(&format!("      ! {} curl_multi_* loop(s) flagged for manual review (see https://rhp.dev/docs/reference/unsupported-php#curl-extension)\n", res.curl_multi_count));
                    }
                    if res.php_tag_removed {
                        out.push_str("      ~ removed opening '<?php' and/or closing '?>' tag(s)\n");
                    }
                }
            }
        } else {
            out.push_str("\nNo PHP legacy superglobals, globals, or tags required migrating.\n");
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
    let mut curl_migrated_count = 0;
    let mut curl_multi_count = 0;

    // Step 0: Remove <?php from start and ?> from end
    let (after_tags, tags_removed) = remove_php_tags(&current);
    current = after_tags;

    // Step 0.5: Upgrade cURL functions to Http facade and flag curl_multi_*
    let (after_curl, c_migrated, c_multi) = upgrade_curl_calls(&current);
    curl_migrated_count += c_migrated;
    curl_multi_count += c_multi;
    current = after_curl;

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
        php_tag_removed: tags_removed,
        curl_migrated_count,
        curl_multi_count,
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
// Step 0: Remove <?php from start and ?> from end of PHP files
// =========================================================================

/// Remove `<?php` from the start and `?>` from the end of PHP files.
/// Returns the updated string and whether any tags were removed.
pub fn remove_php_tags(source: &str) -> (String, bool) {
    let mut modified = false;
    let mut s = source.to_string();

    // 1. Remove <?php from start
    let bom_offset = if s.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };

    let without_bom = &s[bom_offset..];
    let trimmed_start = without_bom.trim_start();
    let leading_ws_len = without_bom.len() - trimmed_start.len();

    let is_php_open = if trimmed_start.len() >= 5 {
        trimmed_start[..5].eq_ignore_ascii_case("<?php")
    } else {
        false
    };

    if is_php_open {
        let after_tag = &trimmed_start[5..];
        let valid_tag = after_tag.is_empty()
            || after_tag.starts_with(|c: char| c.is_ascii_whitespace() || c == '?');

        if valid_tag {
            modified = true;
            let mut cut_idx = leading_ws_len + 5;
            let bytes = without_bom.as_bytes();
            let n = bytes.len();

            // Skip spaces/tabs on <?php line
            while cut_idx < n && (bytes[cut_idx] == b' ' || bytes[cut_idx] == b'\t') {
                cut_idx += 1;
            }

            // Skip newline on <?php line if present
            if cut_idx < n && bytes[cut_idx] == b'\r' && cut_idx + 1 < n && bytes[cut_idx + 1] == b'\n' {
                cut_idx += 2;
            } else if cut_idx < n && bytes[cut_idx] == b'\n' {
                cut_idx += 1;
            }

            // Skip any blank lines immediately following <?php line
            loop {
                let mut line_end = cut_idx;
                let mut only_ws = true;
                while line_end < n && bytes[line_end] != b'\n' {
                    if bytes[line_end] != b' ' && bytes[line_end] != b'\t' && bytes[line_end] != b'\r' {
                        only_ws = false;
                        break;
                    }
                    line_end += 1;
                }
                if only_ws && line_end < n && bytes[line_end] == b'\n' {
                    cut_idx = line_end + 1;
                } else {
                    break;
                }
            }

            s = without_bom[cut_idx..].to_string();
        }
    }

    // 2. Remove ?> from end
    if !s.is_empty() {
        let trimmed_end = s.trim_end();
        if trimmed_end.ends_with("?>") {
            let tag_end = trimmed_end.len();
            let tag_start = tag_end - 2;

            let mask = build_code_mask(&s);
            if tag_start < mask.len() && mask[tag_start] {
                modified = true;
                let bytes = s.as_bytes();
                let mut line_start = tag_start;
                while line_start > 0 && (bytes[line_start - 1] == b' ' || bytes[line_start - 1] == b'\t') {
                    line_start -= 1;
                }

                let cut_start = line_start;
                let remaining = s[..cut_start].to_string();

                if !remaining.is_empty() {
                    let had_crlf = s.ends_with("\r\n");
                    let had_trailing_newline = s.ends_with('\n');
                    let trimmed_rem = remaining.trim_end();
                    let mut cleaned = trimmed_rem.to_string();
                    if had_crlf {
                        cleaned.push_str("\r\n");
                    } else if had_trailing_newline || (line_start > 0 && bytes[line_start - 1] == b'\n') {
                        if line_start > 1 && bytes[line_start - 2] == b'\r' {
                            cleaned.push_str("\r\n");
                        } else {
                            cleaned.push('\n');
                        }
                    }
                    s = cleaned;
                } else {
                    s = String::new();
                }
            }
        }
    }

    (s, modified)
}

// =========================================================================
// Step 1: Replace $_GLOBAL and $GLOBALS with original global values
// =========================================================================

struct TextEdit {
    start: usize,
    end: usize,
    replacement: String,
}

// =========================================================================
// Step 0.5: Replace cURL functions with Http facade and flag curl_multi_*
// =========================================================================

fn get_line_start(source: &str, pos: usize) -> usize {
    let bytes = source.as_bytes();
    let mut p = pos.min(bytes.len());
    while p > 0 && bytes[p - 1] != b'\n' {
        p -= 1;
    }
    p
}

fn get_line_end(source: &str, pos: usize) -> usize {
    let bytes = source.as_bytes();
    let mut p = pos.min(bytes.len());
    while p < bytes.len() && bytes[p] != b'\n' {
        p += 1;
    }
    if p < bytes.len() && bytes[p] == b'\n' {
        p += 1;
    }
    p
}

fn split_array_items(s: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in s.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            current.push(ch);
            escaped = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            current.push(ch);
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            current.push(ch);
            continue;
        }
        if ch == ',' && !in_single && !in_double {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                items.push(trimmed);
            }
            current.clear();
            continue;
        }
        current.push(ch);
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        items.push(trimmed);
    }
    items
}

fn extract_bearer_token(item: &str) -> Option<String> {
    let trimmed = item.trim();
    // Pattern 1: "Authorization: Bearer " . $token or "Authorization: Bearer " + $token
    let re_concat = Regex::new(
        r#"^['"]Authorization:\s*Bearer\s*['"]\s*(?:\.|\+)\s*(.+)$"#,
    ).ok()?;
    if let Some(caps) = re_concat.captures(trimmed) {
        return Some(caps.get(1).unwrap().as_str().trim().to_string());
    }
    // Pattern 2: "Authorization: Bearer xyz"
    let re_literal = Regex::new(
        r#"^['"]Authorization:\s*Bearer\s+([^'"]+)['"]$"#,
    ).ok()?;
    if let Some(caps) = re_literal.captures(trimmed) {
        return Some(format!("\"{}\"", caps.get(1).unwrap().as_str().trim()));
    }
    None
}

#[derive(Default, Debug)]
struct ParsedCurlSession {
    url: Option<String>,
    method: String,
    body: Option<String>,
    headers: Vec<String>,
    bearer_token: Option<String>,
    basic_auth: Option<String>,
    timeout: Option<String>,
    connect_timeout: Option<String>,
    without_verifying: bool,
    res_var: Option<String>,
    status_var: Option<String>,
}

fn extract_first_call_arg(s: &str) -> &str {
    let mut depth_paren = 0;
    let mut depth_bracket = 0;
    let mut depth_brace = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for (idx, ch) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if in_single || in_double {
            continue;
        }
        match ch {
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            ',' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                return s[..idx].trim();
            }
            _ => {}
        }
    }
    s.trim()
}

fn parse_curl_block_options(block_text: &str, init_url: &str) -> ParsedCurlSession {
    let mut opts = ParsedCurlSession::default();
    opts.method = "get".to_string();

    if !init_url.trim().is_empty() {
        opts.url = Some(init_url.trim().to_string());
    }

    let mut apply_opt = |opt_name: &str, opt_val: &str| {
        let trimmed_val = opt_val.trim();
        match opt_name {
            "CURLOPT_URL" => {
                opts.url = Some(trimmed_val.to_string());
            }
            "CURLOPT_POST" => {
                if trimmed_val == "1" || trimmed_val.eq_ignore_ascii_case("true") {
                    opts.method = "post".to_string();
                }
            }
            "CURLOPT_CUSTOMREQUEST" => {
                let clean_m = trimmed_val.trim_matches('"').trim_matches('\'').to_lowercase();
                if !clean_m.is_empty() {
                    opts.method = clean_m;
                }
            }
            "CURLOPT_HTTPGET" => {
                if trimmed_val == "1" || trimmed_val.eq_ignore_ascii_case("true") {
                    opts.method = "get".to_string();
                }
            }
            "CURLOPT_POSTFIELDS" => {
                if trimmed_val.starts_with("json_encode(") && trimmed_val.ends_with(')') {
                    let inner = trimmed_val["json_encode(".len()..trimmed_val.len() - 1].trim();
                    let data_arg = extract_first_call_arg(inner);
                    opts.body = Some(data_arg.to_string());
                } else {
                    opts.body = Some(trimmed_val.to_string());
                }
            }
            "CURLOPT_HTTPHEADER" => {
                if let Some(open_b) = trimmed_val.find('[') {
                    if let Some(close_b) = trimmed_val.rfind(']') {
                        let inner = &trimmed_val[open_b + 1..close_b];
                        for item in split_array_items(inner) {
                            if let Some(tok) = extract_bearer_token(&item) {
                                opts.bearer_token = Some(tok);
                            } else if item.to_lowercase().contains("content-type: application/json") {
                                // Content-type json handled natively
                            } else if !item.is_empty() {
                                opts.headers.push(item);
                            }
                        }
                    }
                }
            }
            "CURLOPT_USERPWD" => {
                opts.basic_auth = Some(trimmed_val.to_string());
            }
            "CURLOPT_TIMEOUT" => {
                opts.timeout = Some(trimmed_val.to_string());
            }
            "CURLOPT_CONNECTTIMEOUT" => {
                opts.connect_timeout = Some(trimmed_val.to_string());
            }
            "CURLOPT_SSL_VERIFYPEER" => {
                if trimmed_val == "0" || trimmed_val.eq_ignore_ascii_case("false") {
                    opts.without_verifying = true;
                }
            }
            _ => {}
        }
    };

    // 1. Parse individual curl_setopt($ch, CONST, VALUE)
    let re_setopt = Regex::new(
        r#"(?:\\)?curl_setopt\s*\(\s*\$[a-zA-Z_][a-zA-Z0-9_]*\s*,\s*([A-Za-z0-9_]+)\s*,\s*([^;]+)\)\s*;"#,
    ).unwrap();
    for caps in re_setopt.captures_iter(block_text) {
        apply_opt(&caps[1], &caps[2]);
    }

    // 2. Parse batch curl_setopt_array($ch, [...])
    let re_setopt_array = Regex::new(
        r#"(?:\\)?curl_setopt_array\s*\(\s*\$[a-zA-Z_][a-zA-Z0-9_]*\s*,\s*\[([^;]*)\]\s*\)\s*;"#,
    ).unwrap();
    let re_array_entry = Regex::new(
        r#"(CURLOPT_[A-Z0-9_]+)\s*=>\s*([^,\r\n]+)"#,
    ).unwrap();

    for caps in re_setopt_array.captures_iter(block_text) {
        let inner = &caps[1];
        for entry in re_array_entry.captures_iter(inner) {
            apply_opt(&entry[1], &entry[2]);
        }
    }

    // 3. Execution variable
    let re_exec_assign = Regex::new(
        r#"\$([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*(?:\\)?curl_exec\s*\("#,
    ).unwrap();
    if let Some(caps) = re_exec_assign.captures(block_text) {
        opts.res_var = Some(caps[1].to_string());
    } else if block_text.contains("curl_exec(") {
        opts.res_var = None;
    }

    // 4. Status code variable
    let re_getinfo = Regex::new(
        r#"\$([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*(?:\\)?curl_getinfo\s*\([^,;)]*,\s*CURLINFO_(?:HTTP|RESPONSE)_CODE\s*\)\s*;"#,
    ).unwrap();
    if let Some(caps) = re_getinfo.captures(block_text) {
        opts.status_var = Some(caps[1].to_string());
    }

    opts
}

fn build_http_chain(opts: &ParsedCurlSession) -> String {
    let mut chain = String::from("Http::");

    if let Some(tok) = &opts.bearer_token {
        chain.push_str(&format!("withToken({})->", tok));
    }

    if let Some(auth) = &opts.basic_auth {
        if auth.starts_with('"') && auth.ends_with('"') && auth.contains(':') {
            let inner = &auth[1..auth.len() - 1];
            if let Some((u, p)) = inner.split_once(':') {
                chain.push_str(&format!("withBasicAuth(\"{}\", \"{}\")->", u, p));
            } else {
                chain.push_str(&format!("withBasicAuth({})->", auth));
            }
        } else {
            chain.push_str(&format!("withBasicAuth({})->", auth));
        }
    }

    if !opts.headers.is_empty() {
        chain.push_str(&format!("withHeaders([{}])->", opts.headers.join(", ")));
    }

    if let Some(to) = &opts.timeout {
        chain.push_str(&format!("timeout({})->", to));
    }

    if let Some(cto) = &opts.connect_timeout {
        chain.push_str(&format!("connectTimeout({})->", cto));
    }

    if opts.without_verifying {
        chain.push_str("withoutVerifying()->");
    }

    let url = opts.url.as_deref().unwrap_or("\"\"");
    let method = if opts.method.is_empty() { "get" } else { &opts.method };

    match method {
        "post" => {
            if let Some(b) = &opts.body {
                chain.push_str(&format!("post({}, {})", url, b));
            } else {
                chain.push_str(&format!("post({})", url));
            }
        }
        "put" => {
            if let Some(b) = &opts.body {
                chain.push_str(&format!("put({}, {})", url, b));
            } else {
                chain.push_str(&format!("put({})", url));
            }
        }
        "patch" => {
            if let Some(b) = &opts.body {
                chain.push_str(&format!("patch({}, {})", url, b));
            } else {
                chain.push_str(&format!("patch({})", url));
            }
        }
        "delete" => {
            if let Some(b) = &opts.body {
                chain.push_str(&format!("delete({}, {})", url, b));
            } else {
                chain.push_str(&format!("delete({})", url));
            }
        }
        _ => {
            chain.push_str(&format!("get({})", url));
        }
    }

    chain
}

/// Upgrades PHP cURL functions to modern RHP `Http` facade.
///
/// Features:
/// - Comments out original cURL code blocks preserving indentation.
/// - Option A: `$response = Http::...; $res = $response->body();`
/// - Option B: If followed by `json_decode`, converts directly to `$data = Http::...->json();`
/// - Unassigned `curl_exec`: automatically wraps in `Future::async(fn() => Http::...);`
/// - Flags `curl_multi_*` loops for manual review with guide link:
///   `https://rhp.dev/docs/reference/unsupported-php#curl-extension`
///   and comments: "You can use Http::pool([...]) / Future::wait([...])"
pub fn upgrade_curl_calls(source: &str) -> (String, usize, usize) {
    let mask = build_code_mask(source);
    let mut curl_migrated_count = 0;
    let mut curl_multi_count = 0;

    // Check if any curl_ appears in unmasked code
    let has_curl = {
        let bytes = source.as_bytes();
        let mut found = false;
        let mut idx = 0;
        while idx + 5 <= bytes.len() {
            if mask[idx] && &bytes[idx..idx + 5] == b"curl_" {
                found = true;
                break;
            }
            idx += 1;
        }
        found
    };

    if !has_curl {
        return (source.to_string(), 0, 0);
    }

    let mut current = source.to_string();

    // ---------------------------------------------------------------------
    // Pass 1: Flag and comment out curl_multi_* loops/calls
    // ---------------------------------------------------------------------
    let re_multi_line = Regex::new(r#"(?m)^([ \t]*).*?\bcurl_multi_[a-zA-Z0-9_]+\b.*$"#).unwrap();
    let re_multi_do_loop = Regex::new(
        r#"(?s)do\s*\{[^{}]*curl_multi_[^{}]*\}\s*while\s*\([^;]*\);"#,
    ).unwrap();

    let mut multi_spans: Vec<(usize, usize)> = Vec::new();

    for m in re_multi_do_loop.find_iter(&current) {
        let mask_cur = build_code_mask(&current);
        if mask_cur[m.start()] {
            let s = get_line_start(&current, m.start());
            let e = get_line_end(&current, m.end());
            multi_spans.push((s, e));
        }
    }

    for m in re_multi_line.find_iter(&current) {
        let mask_cur = build_code_mask(&current);
        if mask_cur[m.start()] {
            let s = get_line_start(&current, m.start());
            let e = get_line_end(&current, m.end());
            let already_covered = multi_spans.iter().any(|(ms, me)| s >= *ms && e <= *me);
            if !already_covered {
                multi_spans.push((s, e));
            }
        }
    }

    multi_spans.sort_by_key(|s| s.0);
    let mut merged_multi_spans: Vec<(usize, usize)> = Vec::new();
    for span in multi_spans {
        if let Some(last) = merged_multi_spans.last_mut() {
            if span.0 <= last.1 + 100 && current[last.1..span.0].trim().is_empty() {
                last.1 = last.1.max(span.1);
                continue;
            }
        }
        merged_multi_spans.push(span);
    }

    if !merged_multi_spans.is_empty() {
        let mut edits: Vec<TextEdit> = Vec::new();
        for (start, end) in merged_multi_spans {
            let chunk = &current[start..end];
            let first_line = chunk.lines().next().unwrap_or("");
            let indent_len = first_line.len() - first_line.trim_start().len();
            let indent = &first_line[..indent_len];

            let mut replacement = String::new();
            replacement.push_str(&format!(
                "{}// NOTICE: curl_multi_* requires manual review. See https://rhp.dev/docs/reference/unsupported-php#curl-extension\n",
                indent
            ));
            replacement.push_str(&format!(
                "{}// You can use Http::pool([...]) / Future::wait([...])\n",
                indent
            ));

            for line in chunk.lines() {
                if line.trim().is_empty() {
                    replacement.push('\n');
                } else {
                    let l_ind_len = line.len() - line.trim_start().len();
                    let l_ind = &line[..l_ind_len];
                    let l_content = &line[l_ind_len..];
                    replacement.push_str(l_ind);
                    replacement.push_str("// ");
                    replacement.push_str(l_content);
                    replacement.push('\n');
                }
            }

            edits.push(TextEdit {
                start,
                end,
                replacement,
            });
            curl_multi_count += 1;
        }

        edits.sort_by(|a, b| b.start.cmp(&a.start));
        for edit in edits {
            current.replace_range(edit.start..edit.end, &edit.replacement);
        }
    }

    // ---------------------------------------------------------------------
    // Pass 2: Upgrade standard curl_* handle blocks to Http facade
    // ---------------------------------------------------------------------
    let re_init = Regex::new(
        r#"(?m)^([ \t]*)(?:\$([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*)?(?:\\)?curl_init\s*\(([^)]*)\)\s*;"#,
    ).unwrap();

    let mut session_edits: Vec<TextEdit> = Vec::new();
    let mask_pass2 = build_code_mask(&current);

    for caps in re_init.captures_iter(&current) {
        let mat = caps.get(0).unwrap();
        if !mask_pass2[mat.start()] {
            continue;
        }

        let base_indent = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let handle_var = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let init_url = caps.get(3).map(|m| m.as_str()).unwrap_or("");

        let block_start = get_line_start(&current, mat.start());

        let search_start = mat.end();
        let rem = &current[search_start..];

        let mut session_end_offset = None;

        if !handle_var.is_empty() {
            let re_close = Regex::new(&format!(
                r#"(?:\\)?curl_close\s*\(\s*\${}\s*\)\s*;"#,
                regex::escape(handle_var)
            )).unwrap();
            if let Some(m_close) = re_close.find(rem) {
                session_end_offset = Some(search_start + m_close.end());
            }
        }

        if session_end_offset.is_none() {
            let pattern_exec = if !handle_var.is_empty() {
                format!(r#"(?:\\)?curl_exec\s*\(\s*\${}\s*\)\s*;"#, regex::escape(handle_var))
            } else {
                r#"(?:\\)?curl_exec\s*\([^)]*\)\s*;"#.to_string()
            };
            if let Ok(re_exec) = Regex::new(&pattern_exec) {
                if let Some(m_exec) = re_exec.find(rem) {
                    session_end_offset = Some(search_start + m_exec.end());
                }
            }
        }

        let session_end = match session_end_offset {
            Some(end) => end,
            None => continue,
        };

        let intermediate_text = &current[block_start..session_end];
        let opts = parse_curl_block_options(intermediate_text, init_url);

        let mut json_decode_var: Option<String> = None;
        let mut final_block_end = session_end;

        if let Some(ref rv) = opts.res_var {
            let after_session = &current[session_end..];
            let trimmed_after = after_session.trim_start();
            let whitespace_len = after_session.len() - trimmed_after.len();

            let re_json_decode = Regex::new(&format!(
                r#"^\$([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*(?:\\)?json_decode\s*\(\s*\${}\s*(?:,\s*(?:true|false|1|0))?\s*\)\s*;"#,
                regex::escape(rv)
            )).unwrap();

            if let Some(dec_caps) = re_json_decode.captures(trimmed_after) {
                let dec_match = dec_caps.get(0).unwrap();
                json_decode_var = Some(dec_caps[1].to_string());
                final_block_end = session_end + whitespace_len + dec_match.end();
            }
        }

        let block_end = get_line_end(&current, final_block_end);
        let block_text = &current[block_start..block_end];

        let chain = build_http_chain(&opts);

        let mut synthesized = String::new();
        if opts.res_var.is_none() {
            synthesized.push_str(&format!("{}Future::async(fn() => {});\n", base_indent, chain));
        } else if let Some(ref data_var) = json_decode_var {
            if let Some(ref sv) = opts.status_var {
                synthesized.push_str(&format!("{}$response = {};\n", base_indent, chain));
                synthesized.push_str(&format!("{}${} = $response->status();\n", base_indent, sv));
                synthesized.push_str(&format!("{}${} = $response->json();\n", base_indent, data_var));
            } else {
                synthesized.push_str(&format!("{}${} = {}->json();\n", base_indent, data_var, chain));
            }
        } else {
            let rv = opts.res_var.as_ref().unwrap();
            if let Some(ref sv) = opts.status_var {
                synthesized.push_str(&format!("{}$response = {};\n", base_indent, chain));
                synthesized.push_str(&format!("{}${} = $response->body();\n", base_indent, rv));
                synthesized.push_str(&format!("{}${} = $response->status();\n", base_indent, sv));
            } else {
                synthesized.push_str(&format!("{}$response = {};\n", base_indent, chain));
                synthesized.push_str(&format!("{}${} = $response->body();\n", base_indent, rv));
            }
        }

        let mut commented = String::new();
        for line in block_text.lines() {
            if line.trim().is_empty() {
                commented.push('\n');
            } else {
                let l_ind_len = line.len() - line.trim_start().len();
                let l_ind = &line[..l_ind_len];
                let l_content = &line[l_ind_len..];
                commented.push_str(l_ind);
                commented.push_str("// ");
                commented.push_str(l_content);
                commented.push('\n');
            }
        }

        let replacement = format!("{}{}", commented, synthesized);

        session_edits.push(TextEdit {
            start: block_start,
            end: block_end,
            replacement,
        });
        curl_migrated_count += 1;
    }

    session_edits.sort_by(|a, b| b.start.cmp(&a.start));
    for edit in session_edits {
        current.replace_range(edit.start..edit.end, &edit.replacement);
    }

    // ---------------------------------------------------------------------
    // Pass 3: Clean up remaining standalone curl_errno / curl_error
    // ---------------------------------------------------------------------
    let mask_pass3 = build_code_mask(&current);
    let mut cleanup_edits: Vec<TextEdit> = Vec::new();

    let re_errno = Regex::new(r#"(?:\\)?curl_errno\s*\(\s*\$[a-zA-Z_][a-zA-Z0-9_]*\s*\)"#).unwrap();
    for mat in re_errno.find_iter(&current) {
        if mask_pass3[mat.start()] {
            cleanup_edits.push(TextEdit {
                start: mat.start(),
                end: mat.end(),
                replacement: "$response->failed()".to_string(),
            });
        }
    }

    let re_error = Regex::new(r#"(?:\\)?curl_error\s*\(\s*\$[a-zA-Z_][a-zA-Z0-9_]*\s*\)"#).unwrap();
    for mat in re_error.find_iter(&current) {
        if mask_pass3[mat.start()] {
            cleanup_edits.push(TextEdit {
                start: mat.start(),
                end: mat.end(),
                replacement: "\"HTTP error: \" + string($response->status())".to_string(),
            });
        }
    }

    let re_standalone_close = Regex::new(r#"(?m)^([ \t]*)(?:\\)?curl_close\s*\(\s*\$[a-zA-Z_][a-zA-Z0-9_]*\s*\)\s*;"#).unwrap();
    for caps in re_standalone_close.captures_iter(&current) {
        let mat = caps.get(0).unwrap();
        if mask_pass3[mat.start()] {
            let ind = &caps[1];
            cleanup_edits.push(TextEdit {
                start: mat.start(),
                end: mat.end(),
                replacement: format!("{}// curl_close(...);", ind),
            });
        }
    }

    cleanup_edits.sort_by(|a, b| b.start.cmp(&a.start));
    for edit in cleanup_edits {
        current.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (current, curl_migrated_count, curl_multi_count)
}

// =========================================================================
// Step 1: Replace $_GLOBAL and $GLOBALS with original global values
// =========================================================================

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
        let leading_offset = source.len() - trimmed.len();
        let mut idx = leading_offset;

        if trimmed.starts_with("declare") {
            let offset_to_declare = source.len() - trimmed.len();
            if let Some(semi_pos) = trimmed.find(';') {
                let mut after_declare = offset_to_declare + semi_pos + 1;
                while after_declare < source.len() && source.as_bytes()[after_declare] != b'\n' {
                    after_declare += 1;
                }
                if after_declare < source.len() && source.as_bytes()[after_declare] == b'\n' {
                    after_declare += 1;
                }
                idx = after_declare;
            }
        }

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
                return after_ns;
            }
        }

        idx
    }
}
