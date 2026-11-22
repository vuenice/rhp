use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use regex::Regex;
use serde::Serialize;

use crate::lexer::facade::LEGACY_PHP_FUNCTIONS;
use crate::lexer::{Lexer, Token};
use crate::lint::param_order::{
    fix_parameter_order_detailed, ReorderedFunctionInfo,
};

/// Options for the migrate command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrateOptions {
    /// Whether to write modifications to disk (true) or check only / dry run (false).
    pub fix: bool,
    /// Target PHP version migrating from (e.g. 7 or 8). Default is 7.
    pub php_version: u32,
}

impl Default for MigrateOptions {
    fn default() -> Self {
        Self {
            fix: true,
            php_version: 7,
        }
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
    pub heredoc_count: usize,
    pub declare_count: usize,
    pub error_reporting_count: usize,
    pub ini_set_count: usize,
    pub cast_count: usize,
    pub list_destructure_count: usize,
    pub foreach_count: usize,
    pub get_replace_count: usize,
    pub post_replace_count: usize,
    pub error_suppress_count: usize,
    pub var_dump_count: usize,
    pub strict_eq_count: usize,
    pub byte_param_count: usize,
    pub param_type_count: usize,
    pub named_arg_count: usize,
    pub param_order_count: usize,
    pub void_return_count: usize,
    pub legacy_include_count: usize,
    pub ob_rewritten_count: usize,
    pub echo_response_count: usize,
    pub rade_directive_count: usize,
    pub warnings: Vec<String>,
    #[serde(skip)]
    pub original_content: String,
    #[serde(skip)]
    pub migrated_content: String,
    #[serde(skip)]
    pub upgraded_content: String,
}

/// Aggregated report of migrating multiple files.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MigrateReport {
    pub files_scanned: usize,
    pub files_modified: usize,
    pub results: Vec<MigrateFileResult>,
    pub warnings: Vec<String>,
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
                    if res.declare_count > 0 {
                        out.push_str(&format!("      ~ {} declare() directive(s) removed\n", res.declare_count));
                    }
                    if res.error_reporting_count > 0 {
                        out.push_str(&format!("      ~ {} error_reporting(...) call(s) removed\n", res.error_reporting_count));
                    }
                    if res.ini_set_count > 0 {
                        out.push_str(&format!("      ~ {} ini_set(...) call(s) commented out\n", res.ini_set_count));
                    }
                    if res.cast_count > 0 {
                        out.push_str(&format!("      ~ {} type cast(s) converted to functional conversion(s)\n", res.cast_count));
                    }
                    if res.list_destructure_count > 0 {
                        out.push_str(&format!("      ~ {} list(...) destructuring construct(s) converted to [...]\n", res.list_destructure_count));
                    }
                    if res.foreach_count > 0 {
                        out.push_str(&format!("      ~ {} foreach loop(s) rewritten to for-in\n", res.foreach_count));
                    }
                    if res.get_replace_count > 0 {
                        out.push_str(&format!("      ~ {} $_GET[...] access(es) replaced with Request::query(...)\n", res.get_replace_count));
                    }
                    if res.post_replace_count > 0 {
                        out.push_str(&format!("      ~ {} $_POST[...] access(es) replaced with Request::body(...)\n", res.post_replace_count));
                    }
                    if res.error_suppress_count > 0 {
                        out.push_str(&format!("      ~ {} error suppression '@' operator(s) removed\n", res.error_suppress_count));
                    }
                    if res.var_dump_count > 0 {
                        out.push_str(&format!("      ~ {} var_dump / var_export call(s) replaced with dd()\n", res.var_dump_count));
                    }
                    if res.strict_eq_count > 0 {
                        out.push_str(&format!("      ~ {} strict comparison operator(s) replaced ('===' -> '==', '!==' -> '!=')\n", res.strict_eq_count));
                    }
                    if res.byte_param_count > 0 {
                        out.push_str(&format!("      ~ {} function parameter(s) receiving binary strings updated to 'byte'\n", res.byte_param_count));
                    }
                    if res.param_type_count > 0 {
                        out.push_str(&format!("      ~ {} parameter & property type(s) migrated to colon syntax ($x: Type)\n", res.param_type_count));
                    }
                    if res.named_arg_count > 0 {
                        out.push_str(&format!("      ~ {} named argument call(s) converted to positional in proper order\n", res.named_arg_count));
                    }
                    if res.param_order_count > 0 {
                        out.push_str(&format!("      ~ {} function parameter order(s) fixed (required before optional with //before: comment)\n", res.param_order_count));
                    }
                    if res.void_return_count > 0 {
                        out.push_str(&format!("      ~ {} void return type(s) removed\n", res.void_return_count));
                    }
                    if res.legacy_include_count > 0 {
                        out.push_str(&format!("      ~ {} legacy require/include statement(s) removed\n", res.legacy_include_count));
                    }
                    if res.ob_rewritten_count > 0 {
                        out.push_str(&format!("      ~ {} output buffering (ob_*) construct(s) rewritten\n", res.ob_rewritten_count));
                    }
                    if res.echo_response_count > 0 {
                        out.push_str(&format!("      ~ {} echo statement(s) converted to Response returns\n", res.echo_response_count));
                    }
                    if res.rade_directive_count > 0 {
                        out.push_str(&format!("      ~ {} PHP template tag(s) transpiled to Rade directives\n", res.rade_directive_count));
                    }
                    for w in &res.warnings {
                        out.push_str(&format!("      ⚠ {}\n", w));
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
                    if res.heredoc_count > 0 {
                        out.push_str(&format!("      ~ {} Heredoc / Nowdoc block(s) upgraded to ``` markdown syntax\n", res.heredoc_count));
                    }
                    if res.php_tag_removed {
                        out.push_str("      ~ removed opening '<?php' and/or closing '?>' tag(s)\n");
                    }
                }
            }
        } else {
            out.push_str("\nNo PHP legacy superglobals, globals, or tags required migrating.\n");
        }

        if !self.warnings.is_empty() {
            out.push_str("\nWarnings & Review Items:\n");
            for w in &self.warnings {
                out.push_str(&format!("  ⚠ {}\n", w));
            }
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

/// Migrate a source string in memory with default options.
pub fn migrate_source(source: &str, file_name: &str) -> MigrateFileResult {
    migrate_source_with_options_and_context(source, file_name, MigrateOptions::default(), &HashMap::new())
}

/// Migrate a source string in memory with custom options.
pub fn migrate_source_with_options(
    source: &str,
    file_name: &str,
    options: MigrateOptions,
) -> MigrateFileResult {
    migrate_source_with_options_and_context(source, file_name, options, &HashMap::new())
}

/// Migrate a source string in memory with additional project-wide function signatures.
pub fn migrate_source_with_context(
    source: &str,
    file_name: &str,
    extra_signatures: &HashMap<String, Vec<String>>,
) -> MigrateFileResult {
    migrate_source_with_options_and_context(source, file_name, MigrateOptions::default(), extra_signatures)
}

/// Migrate a source string in memory with options and additional project-wide function signatures.
pub fn migrate_source_with_options_and_context(
    source: &str,
    file_name: &str,
    options: MigrateOptions,
    extra_signatures: &HashMap<String, Vec<String>>,
) -> MigrateFileResult {
    let mut current = source.to_string();
    let mut cookie_set_count = 0;
    let mut global_replace_count = 0;
    let mut dot_concat_count = 0;
    let mut curl_migrated_count = 0;
    let mut curl_multi_count = 0;
    let mut heredoc_count = 0;
    let mut declare_count = 0;
    let mut error_reporting_count = 0;
    let mut ini_set_count = 0;
    let mut cast_count = 0;
    let mut list_destructure_count = 0;
    let mut foreach_count = 0;
    let mut get_replace_count = 0;
    let mut post_replace_count = 0;
    let mut error_suppress_count = 0;
    let mut var_dump_count = 0;
    let mut strict_eq_count = 0;
    let mut param_type_count = 0;
    let mut named_arg_count = 0;
    let mut param_order_count = 0;
    let mut void_return_count = 0;
    let mut legacy_include_count = 0;
    let mut ob_rewritten_count = 0;
    let mut echo_response_count = 0;
    let mut rade_directive_count = 0;
    let mut warnings = Vec::new();

    // Step -0.1: Transpile PHP template tags (<?=, <?php if (...): ?>, <?php foreach (...): ?>) to Rade directives
    let (after_rade, r_count) = transpile_php_template_to_rade(&current);
    rade_directive_count += r_count;
    current = after_rade;

    // Step 0: Remove <?php from start and ?> from end
    let (after_tags, tags_removed) = remove_php_tags(&current);
    current = after_tags;

    // Step 0.0003: Migrate simple output buffering (ob_start + echo / include -> concat / View::render)
    let (after_ob, ob_count, ob_warnings) = migrate_simple_output_buffering(&current);
    ob_rewritten_count += ob_count;
    warnings.extend(ob_warnings);
    current = after_ob;

    // Step 0.0006: Strip legacy include/require statements (autoload, class files)
    let (after_includes, inc_count) = strip_legacy_includes(&current);
    legacy_include_count += inc_count;
    current = after_includes;

    // Step 0.0009: Migrate controller / action echo to Response::html / Response::json
    let (after_resp, echo_resp_count, echo_warnings) = migrate_controller_echo(&current);
    echo_response_count += echo_resp_count;
    warnings.extend(echo_warnings);
    current = after_resp;

    // Step 0.00095: Rewrite json_encode / json_decode / json_validate to Json::
    let (after_json, json_warnings) = migrate_json_calls(&current);
    warnings.extend(json_warnings);
    current = after_json;

    // Step 0.001: `===` / `!==` are parse errors in RHP. Rewrite them before any
    // later step parses the file. Strings and comments are left unchanged.
    let after_eq = super::replace_strict_equality_operators(&current);
    strict_eq_count += current.len() - after_eq.len();
    current = after_eq;

    // Step 0.01: Strip redundant declare(...) directive statements and blocks
    let (after_declares, d_count) = strip_declare_statements(&current);
    declare_count += d_count;
    current = after_declares;

    // Step 0.02: Strip legacy error_reporting(...) calls
    let (after_reporting, er_count) = strip_error_reporting(&current);
    error_reporting_count += er_count;
    current = after_reporting;

    // Step 0.03: Comment legacy ini_set(...) configuration calls (// ini_set(...);)
    let (after_ini, ini_c) = comment_ini_set_calls(&current);
    ini_set_count += ini_c;
    current = after_ini;

    // Step 0.04: Replace parenthesized type casts ((int)$x, (string)$x, (float)$x, (bool)$x)
    // with functional conversions (int($x), string($x), float($x), bool($x))
    let (after_casts, c_cast_count) = replace_type_casts(&current);
    cast_count += c_cast_count;
    current = after_casts;

    // Step 0.05: Replace legacy list(...) destructuring with [...] array destructuring
    let (after_lists, l_count) = replace_list_destructuring(&current);
    list_destructure_count += l_count;
    current = after_lists;

    // Step 0.055: Rewrite foreach ($items as $x) to for ($x in $items)
    let (after_foreach, fe_count) = replace_foreach_with_for_in(&current);
    foreach_count += fe_count;
    current = after_foreach;

    // Step 0.06: Remove error suppression operator '@' (@unlink() -> unlink())
    let (after_suppress, s_count) = remove_error_suppression(&current);
    error_suppress_count += s_count;
    current = after_suppress;

    // Step 0.07: Replace $_GET[...] query array access with Request::query(...)
    let (after_get, g_count) = replace_get_array_access(&current);
    get_replace_count += g_count;
    current = after_get;

    // Step 0.08: Replace $_POST[...] body array access with Request::body(...)
    let (after_post, p_count) = replace_post_array_access(&current);
    post_replace_count += p_count;
    current = after_post;

    // Step 0.09: Replace var_dump(...) and var_export(...) with dd(...)
    let (after_dump, vd_count) = replace_var_dump_and_export(&current);
    var_dump_count += vd_count;
    current = after_dump;

    // Step 0.1: Upgrade Heredoc / Nowdoc blocks to ``` markdown syntax
    let (after_heredocs, h_count) = upgrade_heredoc_blocks(&current);
    heredoc_count += h_count;
    current = after_heredocs;

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

    // Step 4.5: Migrate PHP parameter and property types from prefix syntax (Type $x) to colon syntax ($x: Type)
    let (after_params, p_type_count) = migrate_php_parameter_and_property_types(&current);
    param_type_count += p_type_count;
    current = after_params;

    // Step 4.6: Rewrite prefix nullable types (?int -> int?, ?User -> User?)
    current = rewrite_postfix_nullable(&current);

    // Step 5: Detect non-UTF-8 strings and update function parameters receiving them to 'byte'
    let (after_bytes, byte_param_count) = upgrade_binary_string_arguments(&current);
    current = after_bytes;

    if options.php_version >= 8 {
        // Step 6: When PHP version is 8, only check for named arguments, reorder them, and remove argument names.
        // Parameter declarations are NOT reordered.
        let (after_named, n_count) = migrate_named_arguments_with_context(&current, extra_signatures);
        named_arg_count += n_count;
        current = after_named;
    } else {
        // Step 6 & 7: When PHP version is < 8:
        // 1. Automatically fix function parameter orders in declarations (required before optional)
        //    and add `//before: <old_sig>` comment above each reordered declaration.
        let (after_params, p_count, reordered_fns) = fix_parameter_order_detailed(&current, true);
        param_order_count += p_count;
        current = after_params;

        // 2. Swap arguments at normal positional call sites and warn on tricky call sites
        let (after_pos, p_warnings, _swapped_c) =
            swap_positional_calls_and_warn_tricky(&current, &reordered_fns);
        current = after_pos;
        warnings.extend(p_warnings);

        // 3. Convert any named arguments to positional arguments in proper parameter order and strip argument names
        let (after_named, n_count) = migrate_named_arguments_with_context(&current, extra_signatures);
        named_arg_count += n_count;
        current = after_named;
    }

    let (after_void, v_count) = remove_explicit_void_returns(&current);
    void_return_count += v_count;
    current = after_void;

    warnings.extend(check_rune_and_ctype_usage(&current));

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
        heredoc_count,
        declare_count,
        error_reporting_count,
        ini_set_count,
        cast_count,
        list_destructure_count,
        foreach_count,
        get_replace_count,
        post_replace_count,
        error_suppress_count,
        var_dump_count,
        strict_eq_count,
        byte_param_count,
        param_type_count,
        named_arg_count,
        param_order_count,
        void_return_count,
        legacy_include_count,
        ob_rewritten_count,
        echo_response_count,
        rade_directive_count,
        warnings,
        original_content: source.to_string(),
        migrated_content: current.clone(),
        upgraded_content: current,
    }
}

/// Migrate a single file on disk with additional known function signatures.
pub fn migrate_file_with_signatures(
    path: &Path,
    options: MigrateOptions,
    signatures: &HashMap<String, Vec<String>>,
) -> io::Result<MigrateFileResult> {
    let source = fs::read_to_string(path)?;
    let file_path = path.display().to_string();
    let result = migrate_source_with_options_and_context(&source, &file_path, options, signatures);

    if result.modified && options.fix {
        fs::write(path, &result.migrated_content)?;
    }

    Ok(result)
}

/// Migrate a single file on disk.
pub fn migrate_file(path: &Path, options: MigrateOptions) -> io::Result<MigrateFileResult> {
    migrate_file_with_signatures(path, options, &HashMap::new())
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
        for w in &res.warnings {
            report.warnings.push(w.clone());
        }
        report.results.push(res);
        return Ok(report);
    }

    if target.is_dir() {
        let mut signatures = HashMap::new();
        let _ = collect_signatures_recursive(target, &mut signatures);
        migrate_directory_recursive(target, options, &signatures, &mut report)?;
    }

    Ok(report)
}

fn migrate_directory_recursive(
    dir: &Path,
    options: MigrateOptions,
    signatures: &HashMap<String, Vec<String>>,
    report: &mut MigrateReport,
) -> io::Result<()> {
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
            migrate_directory_recursive(&path, options, signatures, report)?;
        } else if is_php_or_rhp_file(&path) {
            let res = migrate_file_with_signatures(&path, options, signatures)?;
            report.files_scanned += 1;
            if res.modified {
                report.files_modified += 1;
            }
            for w in &res.warnings {
                report.warnings.push(w.clone());
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

        // Triple-backtick markdown string: ```...```
        if b == b'`' && i + 2 < n && bytes[i + 1] == b'`' && bytes[i + 2] == b'`' {
            is_code[i] = false;
            is_code[i + 1] = false;
            is_code[i + 2] = false;
            i += 3;
            while i < n {
                is_code[i] = false;
                if bytes[i] == b'\\' && i + 1 < n {
                    is_code[i + 1] = false;
                    i += 2;
                    continue;
                }
                if bytes[i] == b'`' && i + 2 < n && bytes[i + 1] == b'`' && bytes[i + 2] == b'`' {
                    is_code[i + 1] = false;
                    is_code[i + 2] = false;
                    i += 3;
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
// Step 0.1: Upgrade Heredoc / Nowdoc blocks to ``` markdown syntax
// =========================================================================

/// Upgrades PHP Heredoc and Nowdoc syntax (`<<<HTML ... HTML;`, `<<<SQL ... SQL;`, `<<<EOF ... EOF;`)
/// to Markdown triple-backtick strings (```` ```html ... ```; ````, ````` ```sql ... ```; ````, ```` ``` ... ```; ````).
///
/// Delimiters like `HTML` and `SQL` are converted to lowercase markdown code block language tags (` ```html `, ` ```sql `).
/// Generic/sentinel delimiters (`EOF`, `EOD`, `EOT`, `TEXT`, etc.) are converted to untagged markdown blocks (` ``` `).
/// Any closing delimiter `TAG` on its own line (preserving any leading indentation) is replaced with ```` ``` ````,
/// while preserving any trailing punctuation (`;`, `,`, `)`, `]`, etc.).
pub fn upgrade_heredoc_blocks(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(n);
    let mut i = 0;
    let mut count = 0;

    while i < n {
        // Look for heredoc opening `<<<` in code context
        if mask[i] && bytes[i..].starts_with(b"<<<") {
            let mut p = i + 3;
            // Skip optional horizontal whitespace
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t') {
                p += 1;
            }

            // Extract tag (unquoted, single-quoted nowdoc, or double-quoted heredoc)
            let mut tag_str = "";

            if p < n && (bytes[p] == b'\'' || bytes[p] == b'"') {
                let quote = bytes[p];
                p += 1;
                let start = p;
                while p < n && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_' || bytes[p] >= 0x80) {
                    p += 1;
                }
                let t = &source[start..p];
                if p < n && bytes[p] == quote && !t.is_empty() {
                    p += 1; // consume closing quote
                    tag_str = t;
                }
            } else if p < n && (bytes[p].is_ascii_alphabetic() || bytes[p] == b'_' || bytes[p] >= 0x80) {
                let start = p;
                p += 1;
                while p < n && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_' || bytes[p] >= 0x80) {
                    p += 1;
                }
                tag_str = &source[start..p];
            }

            // Check if followed by spaces and newline
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t') {
                p += 1;
            }

            let is_valid_opening = !tag_str.is_empty() && (
                (p < n && (bytes[p] == b'\n' || bytes[p] == b'\r')) || p == n
            );

            if is_valid_opening {
                // Determine markdown language tag
                let tag_lower = tag_str.to_ascii_lowercase();
                let lang = match tag_lower.as_str() {
                    "eof" | "eod" | "eot" | "end" | "str" | "text" | "string" | "txt" | "data" | "doc" | "heredoc" | "nowdoc" => "",
                    _ => &tag_lower,
                };

                // Advance p to start of body (skipping \r?\n)
                let mut body_p = p;
                if body_p < n && bytes[body_p] == b'\r' && body_p + 1 < n && bytes[body_p + 1] == b'\n' {
                    body_p += 2;
                } else if body_p < n && bytes[body_p] == b'\n' {
                    body_p += 1;
                }

                let mut cur = body_p;
                let mut found_closing = None;

                while cur <= n {
                    let line_start = cur;
                    let mut s = line_start;
                    while s < n && (bytes[s] == b' ' || bytes[s] == b'\t') {
                        s += 1;
                    }

                    if cur < n && source[s..].starts_with(tag_str) {
                        let after_tag = s + tag_str.len();
                        let mut rest = after_tag;
                        while rest < n && (bytes[rest] == b' ' || bytes[rest] == b'\t') {
                            rest += 1;
                        }

                        let is_valid_closing = if rest >= n {
                            true
                        } else {
                            let b = bytes[rest];
                            match b {
                                b'\r' | b'\n' | b';' | b',' | b')' | b']' | b'}' | b'.' | b'+' | b'-' | b'*' | b'/' | b'?' | b':' => true,
                                _ => false,
                            }
                        };

                        if is_valid_closing {
                            found_closing = Some((line_start, s, after_tag));
                            break;
                        }
                    }

                    // Advance to next line
                    while cur < n && bytes[cur] != b'\n' {
                        cur += 1;
                    }
                    if cur < n && bytes[cur] == b'\n' {
                        cur += 1;
                    } else {
                        break;
                    }
                }

                if let Some((_line_start, s, after_tag)) = found_closing {
                    // Valid heredoc found!
                    out.push_str("```");
                    out.push_str(lang);
                    // Copy newline after opening delimiter
                    out.push_str(&source[p..body_p]);
                    // Copy body content up to indentation before closing tag
                    out.push_str(&source[body_p..s]);
                    // Closing tag replaced with ```
                    out.push_str("```");
                    // Advance outer loop index to after_tag
                    i = after_tag;
                    count += 1;
                    continue;
                }
            }
        }

        let ch = source[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    (out, count)
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
                let encoded_inner = if trimmed_val.starts_with("json_encode(") && trimmed_val.ends_with(')') {
                    Some(&trimmed_val["json_encode(".len()..trimmed_val.len() - 1])
                } else if trimmed_val.starts_with("Json::encode(") && trimmed_val.ends_with(')') {
                    Some(&trimmed_val["Json::encode(".len()..trimmed_val.len() - 1])
                } else {
                    None
                };
                if let Some(inner) = encoded_inner {
                    let data_arg = extract_first_call_arg(inner.trim());
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

// =========================================================================

/// Strips redundant `declare(...)` directive statements and blocks from PHP source code.
///
/// Automatically removes:
/// - `declare(strict_types=1);`
/// - `declare(ticks=1);`
/// - `declare(encoding=...);`
/// - Combined directives: `declare(strict_types=1, ticks=1);`
/// - Block forms: `declare(...) { ... }` or `declare(...): ... enddeclare;`
///
/// Directives inside comments or strings are preserved intact.
pub fn strip_declare_statements(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut i = 0;

    while i < n {
        if !mask[i] {
            i += 1;
            continue;
        }

        // Must be at a word boundary
        if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
            i += 1;
            continue;
        }

        // Match "declare" case-insensitively
        if i + 7 <= n && source[i..i + 7].eq_ignore_ascii_case("declare") {
            let after_word = i + 7;
            if after_word < n && (bytes[after_word].is_ascii_alphanumeric() || bytes[after_word] == b'_') {
                i = after_word;
                continue;
            }

            // Skip whitespace up to '('
            let mut p = after_word;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }

            if p < n && bytes[p] == b'(' {
                // Find matching ')'
                let mut depth = 0;
                let mut close_paren = None;
                let mut cur = p;
                while cur < n {
                    if mask[cur] {
                        if bytes[cur] == b'(' {
                            depth += 1;
                        } else if bytes[cur] == b')' {
                            depth -= 1;
                            if depth == 0 {
                                close_paren = Some(cur);
                                break;
                            }
                        }
                    }
                    cur += 1;
                }

                if let Some(cp) = close_paren {
                    let mut after_p = cp + 1;
                    while after_p < n && (bytes[after_p] == b' ' || bytes[after_p] == b'\t' || bytes[after_p] == b'\r' || bytes[after_p] == b'\n') {
                        after_p += 1;
                    }

                    if after_p < n && bytes[after_p] == b';' {
                        // Statement form: declare(...);
                        let after_semi = after_p + 1;

                        // Check line before 'declare'
                        let mut line_start = i;
                        while line_start > 0 && bytes[line_start - 1] != b'\n' {
                            line_start -= 1;
                        }
                        let only_ws_before = bytes[line_start..i].iter().all(|&b| b == b' ' || b == b'\t');

                        // Check line after ';'
                        let mut line_end = after_semi;
                        while line_end < n && (bytes[line_end] == b' ' || bytes[line_end] == b'\t') {
                            line_end += 1;
                        }
                        // If followed by single-line comment on the same line, consume it as well
                        if (line_end + 1 < n && bytes[line_end] == b'/' && bytes[line_end + 1] == b'/')
                            || (line_end < n && bytes[line_end] == b'#')
                        {
                            while line_end < n && bytes[line_end] != b'\n' {
                                line_end += 1;
                            }
                        }

                        let only_ws_after = line_end == n || bytes[line_end] == b'\r' || bytes[line_end] == b'\n';

                        if only_ws_before && only_ws_after {
                            // Entire line is just the declare statement
                            let mut full_end = line_end;
                            if full_end < n && bytes[full_end] == b'\r' {
                                full_end += 1;
                            }
                            if full_end < n && bytes[full_end] == b'\n' {
                                full_end += 1;
                            }
                            edits.push(TextEdit {
                                start: line_start,
                                end: full_end,
                                replacement: String::new(),
                            });
                        } else if only_ws_before && !only_ws_after {
                            // Code follows on the same line: declare(...); $x = 1;
                            edits.push(TextEdit {
                                start: line_start,
                                end: line_end,
                                replacement: String::new(),
                            });
                        } else if !only_ws_before && only_ws_after {
                            // Something precedes on the line, e.g. <?php declare(...);
                            let mut start = i;
                            while start > line_start && (bytes[start - 1] == b' ' || bytes[start - 1] == b'\t') {
                                start -= 1;
                            }
                            let mut full_end = line_end;
                            if full_end < n && bytes[full_end] == b'\r' {
                                full_end += 1;
                            }
                            if full_end < n && bytes[full_end] == b'\n' {
                                full_end += 1;
                            }
                            let rep = if bytes[line_start..start].ends_with(b"<?php") || bytes[line_start..start].ends_with(b"<?") {
                                "\n".to_string()
                            } else {
                                String::new()
                            };
                            edits.push(TextEdit {
                                start,
                                end: full_end,
                                replacement: rep,
                            });
                        } else {
                            // Code before and after on the same line: foo(); declare(...); bar();
                            let mut start = i;
                            while start > line_start && (bytes[start - 1] == b' ' || bytes[start - 1] == b'\t') {
                                start -= 1;
                            }
                            let rep = if bytes[line_start..start].ends_with(b"<?php") || bytes[line_start..start].ends_with(b"<?") {
                                "\n".to_string()
                            } else {
                                " ".to_string()
                            };
                            edits.push(TextEdit {
                                start,
                                end: line_end,
                                replacement: rep,
                            });
                        }

                        count += 1;
                        i = after_semi;
                        continue;
                    } else if after_p < n && bytes[after_p] == b'{' {
                        // Block form: declare(...) { ... }
                        let brace_start = after_p;
                        let mut b_depth = 0;
                        let mut close_brace = None;
                        let mut b_cur = brace_start;
                        while b_cur < n {
                            if mask[b_cur] {
                                if bytes[b_cur] == b'{' {
                                    b_depth += 1;
                                } else if bytes[b_cur] == b'}' {
                                    b_depth -= 1;
                                    if b_depth == 0 {
                                        close_brace = Some(b_cur);
                                        break;
                                    }
                                }
                            }
                            b_cur += 1;
                        }

                        if let Some(cb) = close_brace {
                            let mut line_start = i;
                            while line_start > 0 && bytes[line_start - 1] != b'\n' {
                                line_start -= 1;
                            }
                            let only_ws_before = bytes[line_start..i].iter().all(|&b| b == b' ' || b == b'\t');
                            let mut after_open_brace = brace_start + 1;
                            while after_open_brace < n && (bytes[after_open_brace] == b' ' || bytes[after_open_brace] == b'\t') {
                                after_open_brace += 1;
                            }
                            let only_ws_after = after_open_brace == n || bytes[after_open_brace] == b'\r' || bytes[after_open_brace] == b'\n';

                            if only_ws_before && only_ws_after {
                                let mut full_end = after_open_brace;
                                if full_end < n && bytes[full_end] == b'\r' { full_end += 1; }
                                if full_end < n && bytes[full_end] == b'\n' { full_end += 1; }
                                edits.push(TextEdit {
                                    start: line_start,
                                    end: full_end,
                                    replacement: String::new(),
                                });
                            } else {
                                edits.push(TextEdit {
                                    start: i,
                                    end: after_open_brace,
                                    replacement: String::new(),
                                });
                            }

                            // Remove closing '}'
                            let mut cb_line_start = cb;
                            while cb_line_start > 0 && bytes[cb_line_start - 1] != b'\n' {
                                cb_line_start -= 1;
                            }
                            let cb_ws_before = bytes[cb_line_start..cb].iter().all(|&b| b == b' ' || b == b'\t');
                            let mut cb_line_end = cb + 1;
                            while cb_line_end < n && (bytes[cb_line_end] == b' ' || bytes[cb_line_end] == b'\t') {
                                cb_line_end += 1;
                            }
                            let cb_ws_after = cb_line_end == n || bytes[cb_line_end] == b'\r' || bytes[cb_line_end] == b'\n';

                            if cb_ws_before && cb_ws_after {
                                let mut full_end = cb_line_end;
                                if full_end < n && bytes[full_end] == b'\r' { full_end += 1; }
                                if full_end < n && bytes[full_end] == b'\n' { full_end += 1; }
                                edits.push(TextEdit {
                                    start: cb_line_start,
                                    end: full_end,
                                    replacement: String::new(),
                                });
                            } else {
                                edits.push(TextEdit {
                                    start: cb,
                                    end: cb_line_end,
                                    replacement: String::new(),
                                });
                            }

                            count += 1;
                            i = brace_start + 1;
                            continue;
                        }
                    } else if after_p < n && bytes[after_p] == b':' {
                        // Alternative block form: declare(...): ... enddeclare;
                        let colon_pos = after_p;
                        let mut ed_pos = None;
                        let mut ed_end = None;
                        let mut c_search = colon_pos + 1;
                        while c_search + 10 <= n {
                            if mask[c_search] && source[c_search..c_search + 10].eq_ignore_ascii_case("enddeclare") {
                                let mut after_ed = c_search + 10;
                                while after_ed < n && (bytes[after_ed] == b' ' || bytes[after_ed] == b'\t') {
                                    after_ed += 1;
                                }
                                if after_ed < n && bytes[after_ed] == b';' {
                                    ed_pos = Some(c_search);
                                    ed_end = Some(after_ed + 1);
                                    break;
                                }
                            }
                            c_search += 1;
                        }

                        if let (Some(ed_start), Some(ed_finish)) = (ed_pos, ed_end) {
                            let mut line_start = i;
                            while line_start > 0 && bytes[line_start - 1] != b'\n' {
                                line_start -= 1;
                            }
                            let only_ws_before = bytes[line_start..i].iter().all(|&b| b == b' ' || b == b'\t');
                            let mut after_colon = colon_pos + 1;
                            while after_colon < n && (bytes[after_colon] == b' ' || bytes[after_colon] == b'\t') {
                                after_colon += 1;
                            }
                            let only_ws_after = after_colon == n || bytes[after_colon] == b'\r' || bytes[after_colon] == b'\n';

                            if only_ws_before && only_ws_after {
                                let mut full_end = after_colon;
                                if full_end < n && bytes[full_end] == b'\r' { full_end += 1; }
                                if full_end < n && bytes[full_end] == b'\n' { full_end += 1; }
                                edits.push(TextEdit {
                                    start: line_start,
                                    end: full_end,
                                    replacement: String::new(),
                                });
                            } else {
                                edits.push(TextEdit {
                                    start: i,
                                    end: after_colon,
                                    replacement: String::new(),
                                });
                            }

                            // Remove enddeclare;
                            let mut ed_line_start = ed_start;
                            while ed_line_start > 0 && bytes[ed_line_start - 1] != b'\n' {
                                ed_line_start -= 1;
                            }
                            let ed_ws_before = bytes[ed_line_start..ed_start].iter().all(|&b| b == b' ' || b == b'\t');
                            let mut ed_line_end = ed_finish;
                            while ed_line_end < n && (bytes[ed_line_end] == b' ' || bytes[ed_line_end] == b'\t') {
                                ed_line_end += 1;
                            }
                            let ed_ws_after = ed_line_end == n || bytes[ed_line_end] == b'\r' || bytes[ed_line_end] == b'\n';

                            if ed_ws_before && ed_ws_after {
                                let mut full_end = ed_line_end;
                                if full_end < n && bytes[full_end] == b'\r' { full_end += 1; }
                                if full_end < n && bytes[full_end] == b'\n' { full_end += 1; }
                                edits.push(TextEdit {
                                    start: ed_line_start,
                                    end: full_end,
                                    replacement: String::new(),
                                });
                            } else {
                                edits.push(TextEdit {
                                    start: ed_start,
                                    end: ed_line_end,
                                    replacement: String::new(),
                                });
                            }

                            count += 1;
                            i = colon_pos + 1;
                            continue;
                        }
                    }
                }
            }
        }

        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    // Sort edits descending by start offset
    edits.sort_by(|a, b| b.start.cmp(&a.start));

    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    // Collapse multiple consecutive blank lines leaving at most one blank line
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }
    while result.contains("\r\n\r\n\r\n") {
        result = result.replace("\r\n\r\n\r\n", "\r\n\r\n");
    }

    (result, count)
}

// =========================================================================
// Step 1: Strip error_reporting(...) Calls
// =========================================================================

/// Strips legacy PHP `error_reporting(...)` calls:
/// - `error_reporting(E_ALL);`
/// - `error_reporting(0);`
/// - `error_reporting(E_ALL & ~E_NOTICE);`
/// - `error_reporting();`
/// - `\error_reporting(E_ALL);`
/// - `@error_reporting(0);`
///
/// Preserves:
/// - Method calls: `$obj->error_reporting(...)`, `$obj?->error_reporting(...)`
/// - Static calls: `Config::error_reporting(...)`
/// - Function declarations: `function error_reporting(...)`
/// - Variable functions: `$error_reporting(...)`
/// - Occurrences inside strings and comments
pub fn strip_error_reporting(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut i = 0;

    let has_php_tags = source.contains("<?php")
        || source.contains("<?=")
        || source.contains("<? ")
        || source.contains("<?\n")
        || source.contains("<?\r");

    let mut in_php = !has_php_tags;

    while i < n {
        if has_php_tags {
            if !in_php {
                if bytes[i..].starts_with(b"<?php") {
                    in_php = true;
                    i += 5;
                    continue;
                } else if bytes[i..].starts_with(b"<?=") {
                    in_php = true;
                    i += 3;
                    continue;
                } else if bytes[i..].starts_with(b"<?") && (i + 2 == n || bytes[i + 2] == b' ' || bytes[i + 2] == b'\t' || bytes[i + 2] == b'\r' || bytes[i + 2] == b'\n') {
                    in_php = true;
                    i += 2;
                    continue;
                }
            } else if mask[i] && bytes[i..].starts_with(b"?>") {
                in_php = false;
                i += 2;
                continue;
            }
        }

        if !in_php || !mask[i] {
            i += 1;
            continue;
        }

        // Optional leading prefix: '@', '\', or '@\'
        let prefix_start = i;
        let mut p_start = i;
        if bytes[p_start] == b'@' {
            p_start += 1;
            while p_start < n && (bytes[p_start] == b' ' || bytes[p_start] == b'\t') {
                p_start += 1;
            }
        }
        if p_start < n && bytes[p_start] == b'\\' {
            p_start += 1;
        }

        // Word boundary check before start
        if prefix_start > 0 {
            let prev = bytes[prefix_start - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' || prev >= 0x80 {
                i += 1;
                continue;
            }
        }

        // Match "error_reporting" case-insensitively (15 bytes)
        let word = "error_reporting";
        if p_start + word.len() <= n && source[p_start..p_start + word.len()].eq_ignore_ascii_case(word) {
            let after_word = p_start + word.len();

            // Word boundary check after
            if after_word < n && (bytes[after_word].is_ascii_alphanumeric() || bytes[after_word] == b'_' || bytes[after_word] >= 0x80) {
                i = after_word;
                continue;
            }

            // Check what precedes (skipping whitespace backwards before prefix_start)
            let mut b_idx = prefix_start;
            while b_idx > 0 && (bytes[b_idx - 1] == b' ' || bytes[b_idx - 1] == b'\t' || bytes[b_idx - 1] == b'\r' || bytes[b_idx - 1] == b'\n') {
                b_idx -= 1;
            }

            // Reject method calls, static calls, nullsafe calls, variable calls
            if b_idx >= 2 && &bytes[b_idx - 2..b_idx] == b"::" {
                i = after_word;
                continue;
            }
            if b_idx >= 2 && &bytes[b_idx - 2..b_idx] == b"->" {
                i = after_word;
                continue;
            }
            if b_idx >= 3 && &bytes[b_idx - 3..b_idx] == b"?->" {
                i = after_word;
                continue;
            }
            if b_idx >= 1 && bytes[b_idx - 1] == b'$' {
                i = after_word;
                continue;
            }

            // Reject function definition: function error_reporting(...)
            let before_trimmed = source[..b_idx].trim_end();
            if before_trimmed.ends_with("function") {
                let prefix_len = before_trimmed.len() - "function".len();
                if prefix_len == 0 || (!source.as_bytes()[prefix_len - 1].is_ascii_alphanumeric() && source.as_bytes()[prefix_len - 1] != b'_') {
                    i = after_word;
                    continue;
                }
            }
            if before_trimmed.ends_with('&') {
                let before_amp = before_trimmed[..before_trimmed.len() - 1].trim_end();
                if before_amp.ends_with("function") {
                    let prefix_len = before_amp.len() - "function".len();
                    if prefix_len == 0 || (!source.as_bytes()[prefix_len - 1].is_ascii_alphanumeric() && source.as_bytes()[prefix_len - 1] != b'_') {
                        i = after_word;
                        continue;
                    }
                }
            }

            // Skip whitespace up to '('
            let mut p = after_word;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }

            if p < n && bytes[p] == b'(' && mask[p] {
                // Find matching ')'
                let mut depth = 0;
                let mut close_paren = None;
                let mut cur = p;
                while cur < n {
                    if mask[cur] {
                        if bytes[cur] == b'(' {
                            depth += 1;
                        } else if bytes[cur] == b')' {
                            depth -= 1;
                            if depth == 0 {
                                close_paren = Some(cur);
                                break;
                            }
                        }
                    }
                    cur += 1;
                }

                if let Some(cp) = close_paren {
                    let mut after_cp = cp + 1;
                    while after_cp < n && (bytes[after_cp] == b' ' || bytes[after_cp] == b'\t' || bytes[after_cp] == b'\r' || bytes[after_cp] == b'\n') {
                        after_cp += 1;
                    }

                    // Check if followed by ';' or '?>'
                    let has_semi = after_cp < n && bytes[after_cp] == b';';
                    let after_stmt = if has_semi {
                        after_cp + 1
                    } else if after_cp + 1 < n && bytes[after_cp..].starts_with(b"?>") {
                        after_cp
                    } else {
                        // Expression context: remove error_reporting(...) call
                        cp + 1
                    };

                    if has_semi {
                        // Standalone statement: check whitespace before and after to cleanly remove line
                        let mut line_start = prefix_start;
                        while line_start > 0 && bytes[line_start - 1] != b'\n' {
                            line_start -= 1;
                        }
                        let only_ws_before = bytes[line_start..prefix_start].iter().all(|&b| b == b' ' || b == b'\t');

                        let mut line_end = after_stmt;
                        while line_end < n && (bytes[line_end] == b' ' || bytes[line_end] == b'\t') {
                            line_end += 1;
                        }
                        // If followed by single-line comment on the same line, consume it as well
                        if (line_end + 1 < n && bytes[line_end] == b'/' && bytes[line_end + 1] == b'/')
                            || (line_end < n && bytes[line_end] == b'#')
                        {
                            while line_end < n && bytes[line_end] != b'\n' {
                                line_end += 1;
                            }
                        }

                        let only_ws_after = line_end == n || bytes[line_end] == b'\r' || bytes[line_end] == b'\n';

                        if only_ws_before && only_ws_after {
                            // Entire line is just the error_reporting statement
                            let mut full_end = line_end;
                            if full_end < n && bytes[full_end] == b'\r' {
                                full_end += 1;
                            }
                            if full_end < n && bytes[full_end] == b'\n' {
                                full_end += 1;
                            }
                            edits.push(TextEdit {
                                start: line_start,
                                end: full_end,
                                replacement: String::new(),
                            });
                        } else if only_ws_before && !only_ws_after {
                            // Code follows on the same line: error_reporting(0); $x = 1;
                            edits.push(TextEdit {
                                start: line_start,
                                end: line_end,
                                replacement: String::new(),
                            });
                        } else if !only_ws_before && only_ws_after {
                            // Something precedes on line: <?php error_reporting(0);
                            let mut start_pos = prefix_start;
                            while start_pos > line_start && (bytes[start_pos - 1] == b' ' || bytes[start_pos - 1] == b'\t') {
                                start_pos -= 1;
                            }
                            let mut full_end = line_end;
                            if full_end < n && bytes[full_end] == b'\r' {
                                full_end += 1;
                            }
                            if full_end < n && bytes[full_end] == b'\n' {
                                full_end += 1;
                            }
                            let rep = if bytes[line_start..start_pos].ends_with(b"<?php")
                                || bytes[line_start..start_pos].ends_with(b"<?=")
                                || bytes[line_start..start_pos].ends_with(b"<?")
                            {
                                "\n".to_string()
                            } else {
                                String::new()
                            };
                            edits.push(TextEdit {
                                start: start_pos,
                                end: full_end,
                                replacement: rep,
                            });
                        } else {
                            // Both preceded and followed by code on the same line
                            let mut start_pos = prefix_start;
                            while start_pos > line_start && (bytes[start_pos - 1] == b' ' || bytes[start_pos - 1] == b'\t') {
                                start_pos -= 1;
                            }
                            edits.push(TextEdit {
                                start: start_pos,
                                end: line_end,
                                replacement: " ".to_string(),
                            });
                        }
                    } else {
                        // Without semicolon (e.g. before closing tag ?>)
                        edits.push(TextEdit {
                            start: prefix_start,
                            end: after_stmt,
                            replacement: String::new(),
                        });
                    }

                    count += 1;
                    i = after_stmt;
                    continue;
                }
            }
        }

        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));

    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    // Collapse multiple consecutive blank lines leaving at most one blank line
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }
    while result.contains("\r\n\r\n\r\n") {
        result = result.replace("\r\n\r\n\r\n", "\r\n\r\n");
    }

    (result, count)
}

// =========================================================================
// Step 2: Comment Legacy ini_set(...) Calls
// =========================================================================

/// Comments out legacy PHP `ini_set(...)` configuration calls everywhere, preserving the line:
/// - `ini_set('display_errors', '0');` -> `// ini_set('display_errors', '0');`
/// - `@ini_set('display_errors', '0');` -> `// ini_set('display_errors', '0');`
/// - `\ini_set('display_errors', '0');` -> `// ini_set('display_errors', '0');`
/// - `@\ini_set('display_errors', '0');` -> `// ini_set('display_errors', '0');`
/// - `    ini_set('memory_limit', '512M');` -> `    // ini_set('memory_limit', '512M');`
///
/// Preserves:
/// - Method calls: `$obj->ini_set(...)`, `$obj?->ini_set(...)`
/// - Static calls: `Config::ini_set(...)`
/// - Function definitions: `function ini_set(...)`
/// - Variable functions: `$ini_set(...)`
/// - Calls already inside comments or strings
pub fn comment_ini_set_calls(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut i = 0;

    let has_php_tags = source.contains("<?php")
        || source.contains("<?=")
        || source.contains("<? ")
        || source.contains("<?\n")
        || source.contains("<?\r");

    let mut in_php = !has_php_tags;

    while i < n {
        if has_php_tags {
            if !in_php {
                if bytes[i..].starts_with(b"<?php") {
                    in_php = true;
                    i += 5;
                    continue;
                } else if bytes[i..].starts_with(b"<?=") {
                    in_php = true;
                    i += 3;
                    continue;
                } else if bytes[i..].starts_with(b"<?") && (i + 2 == n || bytes[i + 2] == b' ' || bytes[i + 2] == b'\t' || bytes[i + 2] == b'\r' || bytes[i + 2] == b'\n') {
                    in_php = true;
                    i += 2;
                    continue;
                }
            } else if mask[i] && bytes[i..].starts_with(b"?>") {
                in_php = false;
                i += 2;
                continue;
            }
        }

        if !in_php || !mask[i] {
            i += 1;
            continue;
        }

        // Optional leading prefix: '@', '\', or '@\'
        let prefix_start = i;
        let mut p_start = i;
        if bytes[p_start] == b'@' {
            p_start += 1;
            while p_start < n && (bytes[p_start] == b' ' || bytes[p_start] == b'\t') {
                p_start += 1;
            }
        }
        if p_start < n && bytes[p_start] == b'\\' {
            p_start += 1;
        }

        // Word boundary check before start
        if prefix_start > 0 {
            let prev = bytes[prefix_start - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' || prev >= 0x80 {
                i += 1;
                continue;
            }
        }

        // Match "ini_set" case-insensitively (7 bytes)
        let word = "ini_set";
        if p_start + word.len() <= n && source[p_start..p_start + word.len()].eq_ignore_ascii_case(word) {
            let after_word = p_start + word.len();

            // Word boundary check after
            if after_word < n && (bytes[after_word].is_ascii_alphanumeric() || bytes[after_word] == b'_' || bytes[after_word] >= 0x80) {
                i = after_word;
                continue;
            }

            // Check what precedes (skipping whitespace backwards before prefix_start)
            let mut b_idx = prefix_start;
            while b_idx > 0 && (bytes[b_idx - 1] == b' ' || bytes[b_idx - 1] == b'\t' || bytes[b_idx - 1] == b'\r' || bytes[b_idx - 1] == b'\n') {
                b_idx -= 1;
            }

            // Reject method calls, static calls, nullsafe calls, variable calls
            if b_idx >= 2 && &bytes[b_idx - 2..b_idx] == b"::" {
                i = after_word;
                continue;
            }
            if b_idx >= 2 && &bytes[b_idx - 2..b_idx] == b"->" {
                i = after_word;
                continue;
            }
            if b_idx >= 3 && &bytes[b_idx - 3..b_idx] == b"?->" {
                i = after_word;
                continue;
            }
            if b_idx >= 1 && bytes[b_idx - 1] == b'$' {
                i = after_word;
                continue;
            }

            // Reject function definition: function ini_set(...)
            let before_trimmed = source[..b_idx].trim_end();
            if before_trimmed.ends_with("function") {
                let prefix_len = before_trimmed.len() - "function".len();
                if prefix_len == 0 || (!source.as_bytes()[prefix_len - 1].is_ascii_alphanumeric() && source.as_bytes()[prefix_len - 1] != b'_') {
                    i = after_word;
                    continue;
                }
            }

            // Skip whitespace up to '('
            let mut p = after_word;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }

            if p < n && bytes[p] == b'(' && mask[p] {
                // Find matching ')'
                let mut depth = 0;
                let mut close_paren = None;
                let mut cur = p;
                while cur < n {
                    if mask[cur] {
                        if bytes[cur] == b'(' {
                            depth += 1;
                        } else if bytes[cur] == b')' {
                            depth -= 1;
                            if depth == 0 {
                                close_paren = Some(cur);
                                break;
                            }
                        }
                    }
                    cur += 1;
                }

                if let Some(cp) = close_paren {
                    let mut after_cp = cp + 1;
                    while after_cp < n && (bytes[after_cp] == b' ' || bytes[after_cp] == b'\t' || bytes[after_cp] == b'\r' || bytes[after_cp] == b'\n') {
                        after_cp += 1;
                    }

                    // Check if followed by ';'
                    let has_semi = after_cp < n && bytes[after_cp] == b';';
                    let after_stmt = if has_semi {
                        after_cp + 1
                    } else {
                        cp + 1
                    };

                    // Check whitespace before prefix_start on the current line
                    let mut line_start = prefix_start;
                    while line_start > 0 && bytes[line_start - 1] != b'\n' {
                        line_start -= 1;
                    }
                    let only_ws_before = bytes[line_start..prefix_start].iter().all(|&b| b == b' ' || b == b'\t');

                    // Check trailing content on the same line after after_stmt
                    let mut line_end = after_stmt;
                    while line_end < n && (bytes[line_end] == b' ' || bytes[line_end] == b'\t') {
                        line_end += 1;
                    }
                    if (line_end + 1 < n && bytes[line_end] == b'/' && bytes[line_end + 1] == b'/')
                        || (line_end < n && bytes[line_end] == b'#')
                    {
                        while line_end < n && bytes[line_end] != b'\n' {
                            line_end += 1;
                        }
                    }
                    let only_ws_after = line_end == n || bytes[line_end] == b'\r' || bytes[line_end] == b'\n';

                    let stmt_code = &source[p_start..after_stmt];
                    let commented_lines: Vec<String> = stmt_code
                        .lines()
                        .enumerate()
                        .map(|(_, l)| {
                            format!("// {}", l)
                        })
                        .collect();
                    let replacement_text = commented_lines.join("\n");

                    if only_ws_before {
                        if only_ws_after {
                            // Standalone line: replace from prefix_start to after_stmt
                            edits.push(TextEdit {
                                start: prefix_start,
                                end: after_stmt,
                                replacement: replacement_text,
                            });
                        } else {
                            // Code follows on the same line: ini_set(...); $x = 1;
                            edits.push(TextEdit {
                                start: prefix_start,
                                end: line_end,
                                replacement: format!("{}\n", replacement_text),
                            });
                        }
                    } else {
                        // Preceded by something on the line: e.g. <?php ini_set(...) or foo(); ini_set(...)
                        let before_text = &source[line_start..prefix_start];
                        if before_text.ends_with("<?php ") || before_text.ends_with("<?php\t") {
                            let start_edit = line_start + 5;
                            edits.push(TextEdit {
                                start: start_edit,
                                end: after_stmt,
                                replacement: format!("\n// {}", stmt_code),
                            });
                        } else if before_text.ends_with("<?php") || before_text.ends_with("<?=") || before_text.ends_with("<?") {
                            edits.push(TextEdit {
                                start: prefix_start,
                                end: after_stmt,
                                replacement: format!("\n// {}", stmt_code),
                            });
                        } else {
                            edits.push(TextEdit {
                                start: prefix_start,
                                end: after_stmt,
                                replacement: format!("\n// {}", stmt_code),
                            });
                        }
                    }

                    count += 1;
                    i = after_stmt;
                    continue;
                }
            }
        }

        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));

    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

// =========================================================================
// Step 3: Replace Parenthesized Type Casts ((int)$x -> int($x))
// =========================================================================

/// Replaces legacy PHP-style parenthesized casts with functional conversions:
/// - `(int)$x` / `(integer)$x` -> `int($x)`
/// - `(string)$x` / `(binary)$x` -> `string($x)`
/// - `(float)$x` / `(double)$x` / `(real)$x` -> `float($x)`
/// - `(bool)$x` / `(boolean)$x` -> `bool($x)`
///
/// Preserves occurrences inside string literals or comments.
pub fn replace_type_casts(source: &str) -> (String, usize) {
    let mut current = source.to_string();
    let mut total_count = 0;

    // Loop until no more casts are found (handles nested/chained casts like `(int)(string)$x`)
    loop {
        let mask = build_code_mask(&current);
        let bytes = current.as_bytes();
        let n = bytes.len();
        let mut edits: Vec<TextEdit> = Vec::new();
        let mut i = 0;

        while i < n {
            if !mask[i] {
                i += 1;
                continue;
            }

            if bytes[i] == b'(' {
                let mut p = i + 1;
                while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                    p += 1;
                }

                let word_start = p;
                while p < n && bytes[p].is_ascii_alphabetic() {
                    p += 1;
                }
                let word = &current[word_start..p];

                while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                    p += 1;
                }

                if p < n && bytes[p] == b')' {
                    let target_type = match word.to_ascii_lowercase().as_str() {
                        "int" | "integer" => "int",
                        "string" | "binary" => "string",
                        "float" | "double" | "real" => "float",
                        "bool" | "boolean" => "bool",
                        "rune" => "rune",
                        _ => {
                            i += 1;
                            continue;
                        }
                    };

                    let close_paren = p;
                    let mut op_start = close_paren + 1;
                    while op_start < n && (bytes[op_start] == b' ' || bytes[op_start] == b'\t' || bytes[op_start] == b'\r' || bytes[op_start] == b'\n') {
                        op_start += 1;
                    }

                    if op_start < n {
                        // Check if immediately followed by another cast (e.g. `(int)(string)$x`).
                        // If so, let the inner cast convert first.
                        let is_another_cast = if bytes[op_start] == b'(' {
                            let mut cp = op_start + 1;
                            while cp < n && (bytes[cp] == b' ' || bytes[cp] == b'\t') {
                                cp += 1;
                            }
                            let ws = cp;
                            while cp < n && bytes[cp].is_ascii_alphabetic() {
                                cp += 1;
                            }
                            let w = current[ws..cp].to_ascii_lowercase();
                            while cp < n && (bytes[cp] == b' ' || bytes[cp] == b'\t') {
                                cp += 1;
                            }
                            cp < n && bytes[cp] == b')' && matches!(w.as_str(), "int" | "integer" | "string" | "binary" | "float" | "double" | "real" | "bool" | "boolean")
                        } else {
                            false
                        };

                        if !is_another_cast {
                            if let Some(op_end) = find_cast_operand_end(&current, &mask, op_start) {
                                if op_end > op_start {
                                    let operand_str = &current[op_start..op_end];
                                    let trimmed = operand_str.trim();

                                    // If operand is already enclosed in balanced parentheses, e.g. `($a + $b)`
                                    let inner = if trimmed.starts_with('(') && trimmed.ends_with(')') && is_balanced_parens(trimmed) {
                                        &trimmed[1..trimmed.len() - 1]
                                    } else {
                                        trimmed
                                    };

                                    let replacement = format!("{}({})", target_type, inner.trim());
                                    edits.push(TextEdit {
                                        start: i,
                                        end: op_end,
                                        replacement,
                                    });

                                    i = op_end;
                                    continue;
                                }
                            }
                        }
                    }
                }
            }

            i += 1;
        }

        if edits.is_empty() {
            break;
        }

        total_count += edits.len();
        edits.sort_by(|a, b| b.start.cmp(&a.start));

        for edit in edits {
            current.replace_range(edit.start..edit.end, &edit.replacement);
        }
    }

    (current, total_count)
}

fn is_balanced_parens(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes[0] != b'(' || bytes[bytes.len() - 1] != b')' {
        return false;
    }
    let mut depth = 0;
    for (idx, &b) in bytes.iter().enumerate() {
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
            if depth == 0 && idx < bytes.len() - 1 {
                return false;
            }
        }
    }
    depth == 0
}

fn find_cast_operand_end(source: &str, mask: &[bool], start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let n = bytes.len();
    if start >= n || !mask[start] {
        return None;
    }

    let mut cur = start;

    // 1. Optional unary prefix operators: +, -, !, @
    while cur < n && mask[cur] && (bytes[cur] == b'+' || bytes[cur] == b'-' || bytes[cur] == b'!' || bytes[cur] == b'@') {
        cur += 1;
        while cur < n && (bytes[cur] == b' ' || bytes[cur] == b'\t' || bytes[cur] == b'\r' || bytes[cur] == b'\n') {
            cur += 1;
        }
    }
    if cur >= n || !mask[cur] {
        return None;
    }

    // 2. Primary term
    if bytes[cur] == b'(' {
        // Parenthesized expression (...)
        let mut depth = 0;
        let mut close_idx = None;
        let mut p = cur;
        while p < n {
            if mask[p] {
                if bytes[p] == b'(' {
                    depth += 1;
                } else if bytes[p] == b')' {
                    depth -= 1;
                    if depth == 0 {
                        close_idx = Some(p + 1);
                        break;
                    }
                }
            }
            p += 1;
        }
        cur = close_idx?;
    } else if bytes[cur] == b'[' {
        // Array literal [...]
        let mut depth = 0;
        let mut close_idx = None;
        let mut p = cur;
        while p < n {
            if mask[p] {
                if bytes[p] == b'[' {
                    depth += 1;
                } else if bytes[p] == b']' {
                    depth -= 1;
                    if depth == 0 {
                        close_idx = Some(p + 1);
                        break;
                    }
                }
            }
            p += 1;
        }
        cur = close_idx?;
    } else if bytes[cur] == b'\'' || bytes[cur] == b'"' || bytes[cur] == b'`' {
        // String literal
        let quote = bytes[cur];
        cur += 1;
        while cur < n {
            if bytes[cur] == b'\\' && cur + 1 < n {
                cur += 2;
                continue;
            }
            if bytes[cur] == quote {
                cur += 1;
                break;
            }
            cur += 1;
        }
    } else if bytes[cur] == b'$' {
        // Variable: $foo, $$foo
        cur += 1;
        while cur < n && bytes[cur] == b'$' {
            cur += 1;
        }
        if cur < n && (bytes[cur].is_ascii_alphabetic() || bytes[cur] == b'_' || bytes[cur] >= 0x80) {
            cur += 1;
            while cur < n && (bytes[cur].is_ascii_alphanumeric() || bytes[cur] == b'_' || bytes[cur] >= 0x80) {
                cur += 1;
            }
        } else {
            return None;
        }
    } else if bytes[cur].is_ascii_digit() {
        // Numeric literal: 123, 123.45, 0x1a
        while cur < n && (bytes[cur].is_ascii_alphanumeric() || bytes[cur] == b'.' || bytes[cur] == b'_') {
            cur += 1;
        }
    } else if bytes[cur].is_ascii_alphabetic() || bytes[cur] == b'_' || bytes[cur] == b'\\' || bytes[cur] >= 0x80 {
        // Identifier / function name / class name / true / false / null
        while cur < n && (bytes[cur].is_ascii_alphanumeric() || bytes[cur] == b'_' || bytes[cur] == b'\\' || bytes[cur] >= 0x80) {
            cur += 1;
        }
    } else {
        return None;
    }

    // 3. Postfix chain loop: ->prop, ?->prop, ::prop/method, [idx], (args)
    loop {
        let mut p = cur;
        while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
            p += 1;
        }
        if p >= n || !mask[p] {
            break;
        }

        if bytes[p..].starts_with(b"?->") {
            p += 3;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }
            if p < n && (bytes[p].is_ascii_alphabetic() || bytes[p] == b'_' || bytes[p] >= 0x80) {
                p += 1;
                while p < n && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_' || bytes[p] >= 0x80) {
                    p += 1;
                }
                cur = p;
                continue;
            } else {
                break;
            }
        } else if bytes[p..].starts_with(b"->") {
            p += 2;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }
            if p < n && (bytes[p].is_ascii_alphabetic() || bytes[p] == b'_' || bytes[p] >= 0x80) {
                p += 1;
                while p < n && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_' || bytes[p] >= 0x80) {
                    p += 1;
                }
                cur = p;
                continue;
            } else {
                break;
            }
        } else if bytes[p..].starts_with(b"::") {
            p += 2;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }
            if p < n && bytes[p] == b'$' {
                p += 1;
            }
            if p < n && (bytes[p].is_ascii_alphabetic() || bytes[p] == b'_' || bytes[p] >= 0x80) {
                p += 1;
                while p < n && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_' || bytes[p] >= 0x80) {
                    p += 1;
                }
                cur = p;
                continue;
            } else {
                break;
            }
        } else if bytes[p] == b'[' {
            // Array subscript [ ... ]
            let mut depth = 0;
            let mut close_idx = None;
            let mut s = p;
            while s < n {
                if mask[s] {
                    if bytes[s] == b'[' {
                        depth += 1;
                    } else if bytes[s] == b']' {
                        depth -= 1;
                        if depth == 0 {
                            close_idx = Some(s + 1);
                            break;
                        }
                    }
                }
                s += 1;
            }
            if let Some(ci) = close_idx {
                cur = ci;
                continue;
            } else {
                break;
            }
        } else if bytes[p] == b'(' {
            // Function or method call args ( ... )
            let mut depth = 0;
            let mut close_idx = None;
            let mut s = p;
            while s < n {
                if mask[s] {
                    if bytes[s] == b'(' {
                        depth += 1;
                    } else if bytes[s] == b')' {
                        depth -= 1;
                        if depth == 0 {
                            close_idx = Some(s + 1);
                            break;
                        }
                    }
                }
                s += 1;
            }
            if let Some(ci) = close_idx {
                cur = ci;
                continue;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Some(cur)
}

// =========================================================================
// Step 2: Replace Legacy list(...) Destructuring with [...] Array Destructuring
// =========================================================================

/// Replaces legacy PHP `list(...)` destructuring syntax with modern bracket `[...]` destructuring.
///
/// Handles:
/// - Basic destructuring: `list($a, $b) = $coords;` -> `[$a, $b] = $coords;`
/// - Element skipping: `list($r, , $b) = getColors();` -> `[$r, , $b] = getColors();`
/// - Keyed destructuring: `list('id' => $id, 'role' => $role) = $user;` -> `['id' => $id, 'role' => $role] = $user;`
/// - Nested destructuring: `list($a, list($b, $c)) = $nested;` -> `[$a, [$b, $c]] = $nested;`
/// - Inside foreach: `foreach ($rows as list($a, $b))` -> `foreach ($rows as [$a, $b])`
///
/// Preserves:
/// - Method calls: `$obj->list(...)`, `$obj?->list(...)`
/// - Static calls: `Arr::list(...)`
/// - Variable functions: `$list(...)`
/// - Function declarations: `function list(...)`
/// - Occurrences inside strings and comments
pub fn replace_list_destructuring(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut i = 0;

    while i < n {
        if !mask[i] {
            i += 1;
            continue;
        }

        // Must be at a word boundary
        if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_' || bytes[i - 1] == b'$' || bytes[i - 1] >= 0x80) {
            i += 1;
            continue;
        }

        // Match "list" case-insensitively
        if i + 4 <= n && source[i..i + 4].eq_ignore_ascii_case("list") {
            let after_word = i + 4;
            if after_word < n && (bytes[after_word].is_ascii_alphanumeric() || bytes[after_word] == b'_' || bytes[after_word] >= 0x80) {
                i = after_word;
                continue;
            }

            // Check what precedes 'list' (skipping whitespace)
            let mut b_idx = i;
            while b_idx > 0 && (bytes[b_idx - 1] == b' ' || bytes[b_idx - 1] == b'\t' || bytes[b_idx - 1] == b'\r' || bytes[b_idx - 1] == b'\n') {
                b_idx -= 1;
            }

            // Check method calls, static calls, nullsafe calls, variable calls, namespace references
            if b_idx >= 2 && &bytes[b_idx - 2..b_idx] == b"::" {
                i = after_word;
                continue;
            }
            if b_idx >= 2 && &bytes[b_idx - 2..b_idx] == b"->" {
                i = after_word;
                continue;
            }
            if b_idx >= 3 && &bytes[b_idx - 3..b_idx] == b"?->" {
                i = after_word;
                continue;
            }
            if b_idx >= 1 && (bytes[b_idx - 1] == b'$' || bytes[b_idx - 1] == b'\\') {
                i = after_word;
                continue;
            }

            // Check if preceded by 'function' (e.g. function list() or function &list())
            let before_trimmed = source[..b_idx].trim_end();
            if before_trimmed.ends_with("function") {
                let prefix_len = before_trimmed.len() - "function".len();
                if prefix_len == 0 || (!source.as_bytes()[prefix_len - 1].is_ascii_alphanumeric() && source.as_bytes()[prefix_len - 1] != b'_') {
                    i = after_word;
                    continue;
                }
            }
            if before_trimmed.ends_with('&') {
                let before_amp = before_trimmed[..before_trimmed.len() - 1].trim_end();
                if before_amp.ends_with("function") {
                    let prefix_len = before_amp.len() - "function".len();
                    if prefix_len == 0 || (!source.as_bytes()[prefix_len - 1].is_ascii_alphanumeric() && source.as_bytes()[prefix_len - 1] != b'_') {
                        i = after_word;
                        continue;
                    }
                }
            }

            // Skip whitespace up to '('
            let mut p = after_word;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }

            if p < n && bytes[p] == b'(' && mask[p] {
                let open_paren = p;

                // Find matching ')'
                let mut depth = 0;
                let mut close_paren = None;
                let mut cur = open_paren;
                while cur < n {
                    if mask[cur] {
                        if bytes[cur] == b'(' {
                            depth += 1;
                        } else if bytes[cur] == b')' {
                            depth -= 1;
                            if depth == 0 {
                                close_paren = Some(cur);
                                break;
                            }
                        }
                    }
                    cur += 1;
                }

                if let Some(cp) = close_paren {
                    edits.push(TextEdit {
                        start: i,
                        end: open_paren + 1,
                        replacement: "[".to_string(),
                    });
                    edits.push(TextEdit {
                        start: cp,
                        end: cp + 1,
                        replacement: "]".to_string(),
                    });
                    count += 1;
                    i = after_word;
                    continue;
                }
            }
        }

        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));

    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

/// Rewrites `foreach ($items as $value)` and `foreach ($items as $key => $value)`
/// to `for ($value in $items)` and `for ($key => $value in $items)`.
pub fn replace_foreach_with_for_in(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut i = 0;

    while i < n {
        if !mask[i] {
            i += 1;
            continue;
        }
        if i > 0 && (is_migrate_word_byte(bytes[i - 1]) || bytes[i - 1] == b'@') {
            i += 1;
            continue;
        }
        if i + 7 <= n && &source[i..i + 7] == "foreach" {
            let after_word = i + 7;
            if after_word < n && is_migrate_word_byte(bytes[after_word]) {
                i = after_word;
                continue;
            }
            let mut p = after_word;
            while p < n && matches!(bytes[p], b' ' | b'\t' | b'\r' | b'\n') {
                p += 1;
            }
            if p >= n || bytes[p] != b'(' || !mask[p] {
                i = after_word;
                continue;
            }
            let open = p;
            let mut depth = 0i32;
            let mut as_at = None;
            let mut close = None;
            let mut cur = open;
            while cur < n {
                if mask[cur] {
                    if bytes[cur] == b'(' {
                        depth += 1;
                    } else if bytes[cur] == b')' {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(cur);
                            break;
                        }
                    } else if depth == 1 && as_at.is_none() && cur + 2 <= n && &bytes[cur..cur + 2] == b"as" {
                        let before_ok = cur == 0 || !is_migrate_word_byte(bytes[cur - 1]);
                        let after_as = cur + 2;
                        let after_ok = after_as >= n || !is_migrate_word_byte(bytes[after_as]);
                        if before_ok && after_ok {
                            as_at = Some(cur);
                        }
                    }
                }
                cur += 1;
            }
            if let (Some(as_pos), Some(cp)) = (as_at, close) {
                let iterable = source[open + 1..as_pos].trim();
                let binding = source[as_pos + 2..cp].trim();
                if !iterable.is_empty() && !binding.is_empty() {
                    edits.push(TextEdit {
                        start: i,
                        end: cp + 1,
                        replacement: format!("for ({} in {})", binding, iterable),
                    });
                    count += 1;
                    i = cp + 1;
                    continue;
                }
            }
            i = after_word;
            continue;
        }
        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }
    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));
    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }
    (result, count)
}

fn is_migrate_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

// =========================================================================
// Step 3: Remove Error Suppression '@' Operator (@unlink(...) -> unlink(...))
// =========================================================================

/// Removes legacy PHP error suppression operator `@`:
/// - `@unlink($file);` -> `unlink($file);`
/// - `@file_get_contents($url)` -> `file_get_contents($url)`
/// - `@$arr['key']` -> `$arr['key']`
/// - `@mkdir($dir, 0777, true)` -> `mkdir($dir, 0777, true)`
/// - `@@foo()` -> `foo()`
/// - `return @json_decode($str);` -> `return json_decode($str);`
/// - `if (@($a / $b))` -> `if (($a / $b))`
///
/// Preserves:
/// - Email addresses inside strings and HTML (`admin@example.com`)
/// - PHPDoc / annotations in comments (`@param`, `@return`, `@var`, `@deprecated`)
/// - Template directives (`@if`, `@foreach`, `@extends`, `@section`, etc.)
pub fn remove_error_suppression(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut i = 0;

    let has_php_tags = source.contains("<?php")
        || source.contains("<?=")
        || source.contains("<? ")
        || source.contains("<?\n")
        || source.contains("<?\r");

    // Track whether we are inside <?php ... ?> tags
    let mut in_php = !has_php_tags;

    while i < n {
        // Track PHP tag boundaries if file uses <?php tags
        if has_php_tags {
            if !in_php {
                if bytes[i..].starts_with(b"<?php") {
                    in_php = true;
                    i += 5;
                    continue;
                } else if bytes[i..].starts_with(b"<?=") {
                    in_php = true;
                    i += 3;
                    continue;
                } else if bytes[i..].starts_with(b"<?") && (i + 2 == n || bytes[i + 2] == b' ' || bytes[i + 2] == b'\t' || bytes[i + 2] == b'\r' || bytes[i + 2] == b'\n') {
                    in_php = true;
                    i += 2;
                    continue;
                }
            } else if mask[i] && bytes[i..].starts_with(b"?>") {
                in_php = false;
                i += 2;
                continue;
            }
        }

        if !in_php || !mask[i] || bytes[i] != b'@' {
            i += 1;
            continue;
        }

        // Check preceding character (skipping whitespace backwards)
        let mut b_idx = i;
        while b_idx > 0 && (bytes[b_idx - 1] == b' ' || bytes[b_idx - 1] == b'\t' || bytes[b_idx - 1] == b'\r' || bytes[b_idx - 1] == b'\n') {
            b_idx -= 1;
        }

        let before_ws = &source[..b_idx];
        if before_ws.ends_with("<?php") || before_ws.ends_with("<?=") || before_ws.ends_with("<?") {
            // Preceded by PHP opening tag -> valid position for expression / statement
        } else if b_idx > 0 {
            let prev_byte = bytes[b_idx - 1];

            // If preceded by a closing delimiter, literal quote, or backslash, it's NOT error suppression
            if prev_byte == b')' || prev_byte == b']' || prev_byte == b'}' || prev_byte == b'\'' || prev_byte == b'"' || prev_byte == b'`' || prev_byte == b'\\' {
                i += 1;
                continue;
            }

            // If preceded by an alphanumeric char or '_' or '$' or unicode:
            if prev_byte.is_ascii_alphanumeric() || prev_byte == b'_' || prev_byte == b'$' || prev_byte >= 0x80 {
                // Check if the preceding word is a PHP keyword that can precede an expression
                let mut word_start = b_idx - 1;
                while word_start > 0 && (bytes[word_start - 1].is_ascii_alphanumeric() || bytes[word_start - 1] == b'_') {
                    word_start -= 1;
                }
                let prev_word = &source[word_start..b_idx];

                let is_preceding_keyword = matches!(
                    prev_word.to_ascii_lowercase().as_str(),
                    "return" | "yield" | "echo" | "print" | "case" | "throw" | "and" | "or" | "xor" | "clone" | "new" | "instanceof" | "match"
                );

                if !is_preceding_keyword {
                    // Preceded by an identifier / variable / number like `admin@` or `$x@` -> NOT error suppression
                    i += 1;
                    continue;
                }
            }
        }

        // Check following character (skipping whitespace forwards)
        let mut p = i + 1;
        while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
            p += 1;
        }

        if p >= n {
            i += 1;
            continue;
        }

        let next_byte = bytes[p];

        // In PHP, the silenced operand must be a valid expression:
        // - variable: $foo
        // - parenthesized expression: (...)
        // - array: [...]
        // - namespace separator: \foo()
        // - unary operator: !, ~, +, -, @
        // - identifier / function call: unlink(...)
        let is_valid_operand = match next_byte {
            b'$' | b'(' | b'[' | b'\\' | b'!' | b'~' | b'+' | b'-' | b'@' => true,
            b if b.is_ascii_alphabetic() || b == b'_' || b >= 0x80 => {
                // Read identifier
                let id_start = p;
                let mut id_end = p;
                while id_end < n && (bytes[id_end].is_ascii_alphanumeric() || bytes[id_end] == b'_' || bytes[id_end] >= 0x80) {
                    id_end += 1;
                }
                let id = &source[id_start..id_end];

                // Ensure it's not a template/Blade directive
                !is_blade_directive(id)
            }
            _ => false,
        };

        if !is_valid_operand {
            i += 1;
            continue;
        }

        // Found error suppression operator '@'.
        // Consume '@' and any following horizontal spaces/tabs between '@' and the operand.
        let mut edit_end = i + 1;
        while edit_end < n && (bytes[edit_end] == b' ' || bytes[edit_end] == b'\t') {
            edit_end += 1;
        }

        edits.push(TextEdit {
            start: i,
            end: edit_end,
            replacement: String::new(),
        });
        count += 1;
        i = edit_end;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));

    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

fn is_blade_directive(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "if" | "elseif" | "else" | "endif"
            | "unless" | "endunless"
            | "isset" | "endisset"
            | "empty" | "endempty"
            | "auth" | "endauth"
            | "guest" | "endguest"
            | "production" | "endproduction"
            | "env" | "endenv"
            | "hassection" | "sectionmissing"
            | "switch" | "case" | "default" | "endswitch" | "break" | "continue"
            | "for" | "endfor"
            | "foreach" | "endforeach"
            | "forelse" | "endforelse"
            | "while" | "endwhile"
            | "extends" | "section" | "endsection" | "yield" | "show"
            | "include" | "includeif" | "includewhen" | "includeunless" | "includefirst"
            | "each" | "once" | "endonce"
            | "push" | "endpush" | "stack" | "prepend" | "endprepend"
            | "inject" | "component" | "endcomponent" | "slot" | "endslot"
            | "props" | "aware"
            | "vite" | "livewire" | "csrf" | "method" | "dd" | "dump"
    )
}

// =========================================================================
// Step 6: Replace $_GET[...] Array Access with Request::query(...)
// =========================================================================

/// Replaces legacy PHP `$_GET[...]` query parameter accesses with modern `Request::query(...)`:
/// - `$_GET['id']` -> `Request::query('id')`
/// - `$_GET["id"]` -> `Request::query("id")`
/// - `$_GET[$key]` -> `Request::query($key)`
/// - `$_GET[0]` -> `Request::query(0)`
/// - `$_GET['filter']['sub']` -> `Request::query('filter')['sub']`
///
/// Preserves:
/// - Plain `$_GET` variable references (e.g. `$params = $_GET;`, `foreach ($_GET as $k => $v)`)
/// - Occurrences inside string literals or comments
pub fn replace_get_array_access(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut i = 0;

    let has_php_tags = source.contains("<?php")
        || source.contains("<?=")
        || source.contains("<? ")
        || source.contains("<?\n")
        || source.contains("<?\r");

    let mut in_php = !has_php_tags;

    while i < n {
        if has_php_tags {
            if !in_php {
                if bytes[i..].starts_with(b"<?php") {
                    in_php = true;
                    i += 5;
                    continue;
                } else if bytes[i..].starts_with(b"<?=") {
                    in_php = true;
                    i += 3;
                    continue;
                } else if bytes[i..].starts_with(b"<?") && (i + 2 == n || bytes[i + 2] == b' ' || bytes[i + 2] == b'\t' || bytes[i + 2] == b'\r' || bytes[i + 2] == b'\n') {
                    in_php = true;
                    i += 2;
                    continue;
                }
            } else if mask[i] && bytes[i..].starts_with(b"?>") {
                in_php = false;
                i += 2;
                continue;
            }
        }

        if !in_php || !mask[i] {
            i += 1;
            continue;
        }

        // Word boundary check before start
        if i > 0 {
            let prev = bytes[i - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' || prev >= 0x80 {
                i += 1;
                continue;
            }
        }

        // Check for "$_GET" (5 bytes)
        let word = "$_GET";
        if i + word.len() <= n && source[i..i + word.len()].eq_ignore_ascii_case(word) {
            let after_word = i + word.len();

            // Word boundary check after $_GET
            if after_word < n && (bytes[after_word].is_ascii_alphanumeric() || bytes[after_word] == b'_' || bytes[after_word] >= 0x80) {
                i = after_word;
                continue;
            }

            // Skip whitespace up to '['
            let mut p = after_word;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }

            if p < n && bytes[p] == b'[' && mask[p] {
                // Find matching ']'
                let mut depth = 0;
                let mut close_bracket = None;
                let mut cur = p;
                while cur < n {
                    if mask[cur] {
                        if bytes[cur] == b'[' {
                            depth += 1;
                        } else if bytes[cur] == b']' {
                            depth -= 1;
                            if depth == 0 {
                                close_bracket = Some(cur);
                                break;
                            }
                        }
                    }
                    cur += 1;
                }

                if let Some(cb) = close_bracket {
                    let inner = &source[p + 1..cb];
                    let trimmed = inner.trim();
                    if !trimmed.is_empty() {
                        edits.push(TextEdit {
                            start: i,
                            end: cb + 1,
                            replacement: format!("Request::query({})", trimmed),
                        });
                        count += 1;
                        i = cb + 1;
                        continue;
                    }
                }
            }
        }

        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));

    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

// =========================================================================
// Step 7: Replace $_POST[...] array access with Request::body(...)
// =========================================================================

/// Replaces legacy `$_POST[...]` body array accesses with `Request::body(...)`.
///
/// Converts:
/// - `$_POST['id']` -> `Request::body('id')`
/// - `$_POST["id"]` -> `Request::body("id")`
/// - `$_POST[$key]` -> `Request::body($key)`
/// - `$_POST[0]` -> `Request::body(0)`
/// - `$_POST['filter']['sub']` -> `Request::body('filter')['sub']`
///
/// Preserves:
/// - Plain `$_POST` variable references (e.g. `$params = $_POST;`, `foreach ($_POST as $k => $v)`)
/// - Occurrences inside string literals or comments
pub fn replace_post_array_access(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut i = 0;

    let has_php_tags = source.contains("<?php")
        || source.contains("<?=")
        || source.contains("<? ")
        || source.contains("<?\n")
        || source.contains("<?\r");

    let mut in_php = !has_php_tags;

    while i < n {
        if has_php_tags {
            if !in_php {
                if bytes[i..].starts_with(b"<?php") {
                    in_php = true;
                    i += 5;
                    continue;
                } else if bytes[i..].starts_with(b"<?=") {
                    in_php = true;
                    i += 3;
                    continue;
                } else if bytes[i..].starts_with(b"<?") && (i + 2 == n || bytes[i + 2] == b' ' || bytes[i + 2] == b'\t' || bytes[i + 2] == b'\r' || bytes[i + 2] == b'\n') {
                    in_php = true;
                    i += 2;
                    continue;
                }
            } else if mask[i] && bytes[i..].starts_with(b"?>") {
                in_php = false;
                i += 2;
                continue;
            }
        }

        if !in_php || !mask[i] {
            i += 1;
            continue;
        }

        // Word boundary check before start
        if i > 0 {
            let prev = bytes[i - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' || prev >= 0x80 {
                i += 1;
                continue;
            }
        }

        // Check for "$_POST" (6 bytes)
        let word = "$_POST";
        if i + word.len() <= n && source[i..i + word.len()].eq_ignore_ascii_case(word) {
            let after_word = i + word.len();

            // Word boundary check after $_POST
            if after_word < n && (bytes[after_word].is_ascii_alphanumeric() || bytes[after_word] == b'_' || bytes[after_word] >= 0x80) {
                i = after_word;
                continue;
            }

            // Skip whitespace up to '['
            let mut p = after_word;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }

            if p < n && bytes[p] == b'[' && mask[p] {
                // Find matching ']'
                let mut depth = 0;
                let mut close_bracket = None;
                let mut cur = p;
                while cur < n {
                    if mask[cur] {
                        if bytes[cur] == b'[' {
                            depth += 1;
                        } else if bytes[cur] == b']' {
                            depth -= 1;
                            if depth == 0 {
                                close_bracket = Some(cur);
                                break;
                            }
                        }
                    }
                    cur += 1;
                }

                if let Some(cb) = close_bracket {
                    let inner = &source[p + 1..cb];
                    let trimmed = inner.trim();
                    if !trimmed.is_empty() {
                        edits.push(TextEdit {
                            start: i,
                            end: cb + 1,
                            replacement: format!("Request::body({})", trimmed),
                        });
                        count += 1;
                        i = cb + 1;
                        continue;
                    }
                }
            }
        }

        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));

    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

// =========================================================================
// Step 8: Replace var_dump(...) and var_export(...) with dd(...)
// =========================================================================

/// Replaces legacy PHP debugging calls `var_dump(...)` and `var_export(...)` with `dd(...)`.
///
/// Converts:
/// - `var_dump($a);` -> `dd($a);`
/// - `var_dump($a, $b);` -> `dd($a, $b);`
/// - `var_export($x);` -> `dd($x);`
/// - `\var_dump($x);` -> `dd($x);`
/// - `\var_export($x);` -> `dd($x);`
///
/// Preserves:
/// - Method calls: `$obj->var_dump(...)`, `$obj?->var_export(...)`
/// - Static calls: `Dumper::var_dump(...)`, `Exporter::var_export(...)`
/// - Function declarations: `function var_dump(...)`
/// - Variable functions: `$var_dump(...)`
/// - Occurrences inside strings and comments
pub fn replace_var_dump_and_export(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut i = 0;

    let has_php_tags = source.contains("<?php")
        || source.contains("<?=")
        || source.contains("<? ")
        || source.contains("<?\n")
        || source.contains("<?\r");

    let mut in_php = !has_php_tags;

    while i < n {
        if has_php_tags {
            if !in_php {
                if bytes[i..].starts_with(b"<?php") {
                    in_php = true;
                    i += 5;
                    continue;
                } else if bytes[i..].starts_with(b"<?=") {
                    in_php = true;
                    i += 3;
                    continue;
                } else if bytes[i..].starts_with(b"<?") && (i + 2 == n || bytes[i + 2] == b' ' || bytes[i + 2] == b'\t' || bytes[i + 2] == b'\r' || bytes[i + 2] == b'\n') {
                    in_php = true;
                    i += 2;
                    continue;
                }
            } else if mask[i] && bytes[i..].starts_with(b"?>") {
                in_php = false;
                i += 2;
                continue;
            }
        }

        if !in_php || !mask[i] {
            i += 1;
            continue;
        }

        // Check for optional leading backslash: \var_dump or \var_export
        let has_leading_backslash = bytes[i] == b'\\';
        let word_start = if has_leading_backslash { i + 1 } else { i };

        // Must be at a word boundary before
        if i > 0 {
            let prev = bytes[i - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' || prev >= 0x80 {
                i += 1;
                continue;
            }
        }

        // Check if word is "var_dump" (8 bytes) or "var_export" (10 bytes)
        let matched_len = if word_start + 8 <= n && source[word_start..word_start + 8].eq_ignore_ascii_case("var_dump") {
            Some(8)
        } else if word_start + 10 <= n && source[word_start..word_start + 10].eq_ignore_ascii_case("var_export") {
            Some(10)
        } else {
            None
        };

        if let Some(wlen) = matched_len {
            let after_word = word_start + wlen;

            // Must be at a word boundary after
            if after_word < n && (bytes[after_word].is_ascii_alphanumeric() || bytes[after_word] == b'_' || bytes[after_word] >= 0x80) {
                i = after_word;
                continue;
            }

            // Check what precedes (skipping whitespace backwards before i)
            let mut b_idx = i;
            while b_idx > 0 && (bytes[b_idx - 1] == b' ' || bytes[b_idx - 1] == b'\t' || bytes[b_idx - 1] == b'\r' || bytes[b_idx - 1] == b'\n') {
                b_idx -= 1;
            }

            // Reject method calls, static calls, nullsafe calls, variable calls
            if b_idx >= 2 && &bytes[b_idx - 2..b_idx] == b"::" {
                i = after_word;
                continue;
            }
            if b_idx >= 2 && &bytes[b_idx - 2..b_idx] == b"->" {
                i = after_word;
                continue;
            }
            if b_idx >= 3 && &bytes[b_idx - 3..b_idx] == b"?->" {
                i = after_word;
                continue;
            }
            if b_idx >= 1 && bytes[b_idx - 1] == b'$' {
                i = after_word;
                continue;
            }

            // Reject function definition: function var_dump(...) or function &var_dump(...)
            let before_trimmed = source[..b_idx].trim_end();
            if before_trimmed.ends_with("function") {
                let prefix_len = before_trimmed.len() - "function".len();
                if prefix_len == 0 || (!source.as_bytes()[prefix_len - 1].is_ascii_alphanumeric() && source.as_bytes()[prefix_len - 1] != b'_') {
                    i = after_word;
                    continue;
                }
            }
            if before_trimmed.ends_with('&') {
                let before_amp = before_trimmed[..before_trimmed.len() - 1].trim_end();
                if before_amp.ends_with("function") {
                    let prefix_len = before_amp.len() - "function".len();
                    if prefix_len == 0 || (!source.as_bytes()[prefix_len - 1].is_ascii_alphanumeric() && source.as_bytes()[prefix_len - 1] != b'_') {
                        i = after_word;
                        continue;
                    }
                }
            }

            // Must be followed by '(' (skipping whitespace forwards)
            let mut p = after_word;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }

            if p < n && bytes[p] == b'(' && mask[p] {
                edits.push(TextEdit {
                    start: i,
                    end: after_word,
                    replacement: "dd".to_string(),
                });
                count += 1;
                i = after_word;
                continue;
            }
        }

        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));

    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

// =========================================================================
// Step 5: Detect non-UTF-8 strings and update function parameters to 'byte'
// =========================================================================

/// Detects non-UTF-8 strings in PHP source code (literals with \x80..\xFF or raw bytes),
/// determines if they or variables holding them are passed as arguments to functions or methods,
/// and updates those parameter type hints to `byte`.
pub fn upgrade_binary_string_arguments(source: &str) -> (String, usize) {
    let program = match crate::parser::parse(source) {
        Ok(p) => p,
        Err(_) => return (source.to_string(), 0),
    };

    // 1. Identify which variables hold non-UTF-8 strings
    let mut binary_vars: std::collections::HashSet<String> = std::collections::HashSet::new();
    find_binary_assignments(&program.statements, &mut binary_vars);

    // 2. Map (fn_name, arg_index) -> bool (is_binary)
    let mut binary_args: std::collections::HashSet<(String, usize)> = std::collections::HashSet::new();
    find_binary_calls(&program.statements, &binary_vars, &mut binary_args);

    // 3. Locate matching function and method definitions and collect edits
    let mut edits: Vec<TextEdit> = Vec::new();
    collect_param_type_edits(&program.statements, &binary_args, &binary_vars, &mut edits, source);

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    let count = edits.len();
    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));
    edits.dedup_by(|a, b| a.start == b.start && a.end == b.end);

    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

fn is_expr_binary(expr: &crate::ast::expr::Expr<'_>, binary_vars: &std::collections::HashSet<String>) -> bool {
    use crate::ast::expr::{ExprKind, Literal};
    match &expr.kind {
        ExprKind::Literal(Literal::String { raw, .. }) => {
            crate::lexer::token::find_invalid_utf8_escape(raw).is_some()
        }
        ExprKind::Literal(Literal::Bytes { .. }) => true,
        ExprKind::Cast { kind: crate::ast::types::CastType::Bytes, .. } => true,
        ExprKind::Variable(var) => binary_vars.contains(var.name),
        ExprKind::Call { callee, .. } => {
            if let ExprKind::Ident(id) = &callee.kind {
                matches!(id.name, "byte" | "Byte" | "bytes" | "Bytes")
            } else {
                false
            }
        }
        ExprKind::MethodCall { method, .. } => {
            matches!(method.name, "toEncoding" | "slice" | "convert")
        }
        ExprKind::StaticCall { target, method, .. } => {
            if let ExprKind::Ident(target_id) = &target.kind {
                if target_id.name == "Str" {
                    matches!(method.name, "bytes" | "toEncoding" | "convertEncoding" | "fromBytes")
                } else {
                    false
                }
            } else {
                false
            }
        }
        ExprKind::Grouping(inner) => is_expr_binary(inner, binary_vars),
        _ => false,
    }
}

fn find_binary_assignments(stmts: &[crate::ast::stmt::Stmt<'_>], binary_vars: &mut std::collections::HashSet<String>) {
    use crate::ast::expr::ExprKind;
    use crate::ast::stmt::StmtKind;

    for stmt in stmts {
        match &stmt.kind {
            StmtKind::VarDecl { var, init: Some(init), .. } => {
                if is_expr_binary(init, binary_vars) {
                    binary_vars.insert(var.name.to_string());
                }
            }
            StmtKind::Expr(expr) => {
                if let ExprKind::Assign { target, value, .. } = &expr.kind {
                    if let ExprKind::Variable(var) = &target.kind {
                        if is_expr_binary(value, binary_vars) {
                            binary_vars.insert(var.name.to_string());
                        }
                    }
                }
            }
            StmtKind::Block(inner) | StmtKind::Namespace { body: Some(inner), .. } => {
                find_binary_assignments(inner, binary_vars);
            }
            StmtKind::If { then_branch, elseif_branches, else_branch, .. } => {
                find_binary_assignments(std::slice::from_ref(then_branch.as_ref()), binary_vars);
                for elif in elseif_branches {
                    find_binary_assignments(std::slice::from_ref(&elif.body), binary_vars);
                }
                if let Some(eb) = else_branch {
                    find_binary_assignments(std::slice::from_ref(eb.as_ref()), binary_vars);
                }
            }
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::Foreach { body, .. }
            | StmtKind::ForIn { body, .. }
            | StmtKind::Loop { body } => {
                find_binary_assignments(std::slice::from_ref(body.as_ref()), binary_vars);
            }
            StmtKind::FunctionDecl(f) => {
                if let Some(ref body) = f.body {
                    find_binary_assignments(body, binary_vars);
                }
            }
            StmtKind::ClassDecl(c) => {
                for member in &c.members {
                    if let crate::ast::stmt::ClassMemberKind::Method(m) = &member.kind {
                        if let Some(ref body) = m.function.body {
                            find_binary_assignments(body, binary_vars);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn find_binary_calls(
    stmts: &[crate::ast::stmt::Stmt<'_>],
    binary_vars: &std::collections::HashSet<String>,
    binary_args: &mut std::collections::HashSet<(String, usize)>,
) {
    use crate::ast::stmt::StmtKind;
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Expr(expr) => {
                inspect_expr_for_calls(expr, binary_vars, binary_args);
            }
            StmtKind::VarDecl { init: Some(init), .. } => {
                inspect_expr_for_calls(init, binary_vars, binary_args);
            }
            StmtKind::Return(Some(expr)) => {
                inspect_expr_for_calls(expr, binary_vars, binary_args);
            }
            StmtKind::Echo(exprs) => {
                for e in exprs {
                    inspect_expr_for_calls(e, binary_vars, binary_args);
                }
            }
            StmtKind::Block(inner) | StmtKind::Namespace { body: Some(inner), .. } => {
                find_binary_calls(inner, binary_vars, binary_args);
            }
            StmtKind::If { condition, then_branch, elseif_branches, else_branch } => {
                inspect_expr_for_calls(condition, binary_vars, binary_args);
                find_binary_calls(std::slice::from_ref(then_branch.as_ref()), binary_vars, binary_args);
                for elif in elseif_branches {
                    inspect_expr_for_calls(&elif.condition, binary_vars, binary_args);
                    find_binary_calls(std::slice::from_ref(&elif.body), binary_vars, binary_args);
                }
                if let Some(eb) = else_branch {
                    find_binary_calls(std::slice::from_ref(eb.as_ref()), binary_vars, binary_args);
                }
            }
            StmtKind::While { condition, body } | StmtKind::DoWhile { condition, body } => {
                inspect_expr_for_calls(condition, binary_vars, binary_args);
                find_binary_calls(std::slice::from_ref(body.as_ref()), binary_vars, binary_args);
            }
            StmtKind::For { init, condition, update, body } => {
                for e in init {
                    inspect_expr_for_calls(e, binary_vars, binary_args);
                }
                for e in condition {
                    inspect_expr_for_calls(e, binary_vars, binary_args);
                }
                for e in update {
                    inspect_expr_for_calls(e, binary_vars, binary_args);
                }
                find_binary_calls(std::slice::from_ref(body.as_ref()), binary_vars, binary_args);
            }
            StmtKind::Foreach { expr, body, .. } => {
                inspect_expr_for_calls(expr, binary_vars, binary_args);
                find_binary_calls(std::slice::from_ref(body.as_ref()), binary_vars, binary_args);
            }
            StmtKind::FunctionDecl(f) => {
                if let Some(ref body) = f.body {
                    find_binary_calls(body, binary_vars, binary_args);
                }
            }
            StmtKind::ClassDecl(c) => {
                for member in &c.members {
                    if let crate::ast::stmt::ClassMemberKind::Method(m) = &member.kind {
                        if let Some(ref body) = m.function.body {
                            find_binary_calls(body, binary_vars, binary_args);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn inspect_expr_for_calls(
    expr: &crate::ast::expr::Expr<'_>,
    binary_vars: &std::collections::HashSet<String>,
    binary_args: &mut std::collections::HashSet<(String, usize)>,
) {
    use crate::ast::expr::ExprKind;
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            if let ExprKind::Ident(id) = &callee.kind {
                for (idx, arg) in args.iter().enumerate() {
                    if is_expr_binary(&arg.value, binary_vars) {
                        binary_args.insert((id.name.to_string(), idx));
                    }
                }
            }
            inspect_expr_for_calls(callee, binary_vars, binary_args);
            for arg in args {
                inspect_expr_for_calls(&arg.value, binary_vars, binary_args);
            }
        }
        ExprKind::MethodCall { object, method, args, .. } => {
            for (idx, arg) in args.iter().enumerate() {
                if is_expr_binary(&arg.value, binary_vars) {
                    binary_args.insert((method.name.to_string(), idx));
                }
            }
            inspect_expr_for_calls(object, binary_vars, binary_args);
            for arg in args {
                inspect_expr_for_calls(&arg.value, binary_vars, binary_args);
            }
        }
        ExprKind::StaticCall { method, args, .. } => {
            for (idx, arg) in args.iter().enumerate() {
                if is_expr_binary(&arg.value, binary_vars) {
                    binary_args.insert((method.name.to_string(), idx));
                }
            }
            for arg in args {
                inspect_expr_for_calls(&arg.value, binary_vars, binary_args);
            }
        }
        ExprKind::Assign { target, value, .. } => {
            inspect_expr_for_calls(target, binary_vars, binary_args);
            inspect_expr_for_calls(value, binary_vars, binary_args);
        }
        ExprKind::Binary { left, right, .. } => {
            inspect_expr_for_calls(left, binary_vars, binary_args);
            inspect_expr_for_calls(right, binary_vars, binary_args);
        }
        ExprKind::Unary { expr, .. } | ExprKind::Grouping(expr) => {
            inspect_expr_for_calls(expr, binary_vars, binary_args);
        }
        ExprKind::Ternary { condition, if_true, if_false } => {
            inspect_expr_for_calls(condition, binary_vars, binary_args);
            if let Some(t) = if_true {
                inspect_expr_for_calls(t, binary_vars, binary_args);
            }
            inspect_expr_for_calls(if_false, binary_vars, binary_args);
        }
        ExprKind::Array(elements) => {
            for el in elements {
                if let Some(k) = &el.key {
                    inspect_expr_for_calls(k, binary_vars, binary_args);
                }
                inspect_expr_for_calls(&el.value, binary_vars, binary_args);
            }
        }
        _ => {}
    }
}

fn collect_param_type_edits(
    stmts: &[crate::ast::stmt::Stmt<'_>],
    binary_args: &std::collections::HashSet<(String, usize)>,
    binary_vars: &std::collections::HashSet<String>,
    edits: &mut Vec<TextEdit>,
    source: &str,
) {
    use crate::ast::stmt::StmtKind;

    for stmt in stmts {
        match &stmt.kind {
            StmtKind::FunctionDecl(func) => {
                check_fn_params(func.name.name, &func.params, binary_args, binary_vars, edits, source);
                if let Some(ref body) = func.body {
                    collect_param_type_edits(body, binary_args, binary_vars, edits, source);
                }
            }
            StmtKind::ClassDecl(class_decl) => {
                for member in &class_decl.members {
                    if let crate::ast::stmt::ClassMemberKind::Method(method) = &member.kind {
                        check_fn_params(method.function.name.name, &method.function.params, binary_args, binary_vars, edits, source);
                        if let Some(ref body) = method.function.body {
                            collect_param_type_edits(body, binary_args, binary_vars, edits, source);
                        }
                    }
                }
            }
            StmtKind::Block(inner) | StmtKind::Namespace { body: Some(inner), .. } => {
                collect_param_type_edits(inner, binary_args, binary_vars, edits, source);
            }
            StmtKind::If { then_branch, elseif_branches, else_branch, .. } => {
                collect_param_type_edits(std::slice::from_ref(then_branch.as_ref()), binary_args, binary_vars, edits, source);
                for elif in elseif_branches {
                    collect_param_type_edits(std::slice::from_ref(&elif.body), binary_args, binary_vars, edits, source);
                }
                if let Some(eb) = else_branch {
                    collect_param_type_edits(std::slice::from_ref(eb.as_ref()), binary_args, binary_vars, edits, source);
                }
            }
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::Foreach { body, .. }
            | StmtKind::ForIn { body, .. }
            | StmtKind::Loop { body } => {
                collect_param_type_edits(std::slice::from_ref(body.as_ref()), binary_args, binary_vars, edits, source);
            }
            StmtKind::Expr(expr) | StmtKind::VarDecl { init: Some(expr), .. } | StmtKind::Return(Some(expr)) => {
                inspect_expr_for_param_edits(expr, binary_args, binary_vars, edits, source);
            }
            _ => {}
        }
    }
}

fn inspect_expr_for_param_edits(
    expr: &crate::ast::expr::Expr<'_>,
    binary_args: &std::collections::HashSet<(String, usize)>,
    binary_vars: &std::collections::HashSet<String>,
    edits: &mut Vec<TextEdit>,
    source: &str,
) {
    use crate::ast::expr::ExprKind;
    match &expr.kind {
        ExprKind::Closure { params, body, .. } => {
            check_fn_params("", params, binary_args, binary_vars, edits, source);
            collect_param_type_edits(body, binary_args, binary_vars, edits, source);
        }
        ExprKind::ArrowFunction { params, body, .. } => {
            check_fn_params("", params, binary_args, binary_vars, edits, source);
            inspect_expr_for_param_edits(body, binary_args, binary_vars, edits, source);
        }
        ExprKind::Call { callee, args } => {
            inspect_expr_for_param_edits(callee, binary_args, binary_vars, edits, source);
            for arg in args {
                inspect_expr_for_param_edits(&arg.value, binary_args, binary_vars, edits, source);
            }
        }
        ExprKind::MethodCall { object, args, .. } => {
            inspect_expr_for_param_edits(object, binary_args, binary_vars, edits, source);
            for arg in args {
                inspect_expr_for_param_edits(&arg.value, binary_args, binary_vars, edits, source);
            }
        }
        ExprKind::StaticCall { args, .. } => {
            for arg in args {
                inspect_expr_for_param_edits(&arg.value, binary_args, binary_vars, edits, source);
            }
        }
        ExprKind::Assign { target, value, .. } => {
            inspect_expr_for_param_edits(target, binary_args, binary_vars, edits, source);
            inspect_expr_for_param_edits(value, binary_args, binary_vars, edits, source);
        }
        ExprKind::Array(elems) => {
            for elem in elems {
                if let Some(ref k) = elem.key {
                    inspect_expr_for_param_edits(k, binary_args, binary_vars, edits, source);
                }
                inspect_expr_for_param_edits(&elem.value, binary_args, binary_vars, edits, source);
            }
        }
        ExprKind::Grouping(inner) => {
            inspect_expr_for_param_edits(inner, binary_args, binary_vars, edits, source);
        }
        _ => {}
    }
}

fn check_fn_params(
    fn_name: &str,
    params: &[crate::ast::stmt::Param<'_>],
    binary_args: &std::collections::HashSet<(String, usize)>,
    binary_vars: &std::collections::HashSet<String>,
    edits: &mut Vec<TextEdit>,
    source: &str,
) {
    for (idx, param) in params.iter().enumerate() {
        let is_binary_call_arg = !fn_name.is_empty() && binary_args.contains(&(fn_name.to_string(), idx));
        let is_binary_default = param.default_value.as_ref().map_or(false, |def| is_expr_binary(def, binary_vars));
        let is_binary_type_hint = param.type_hint.as_ref().map_or(false, |hint| {
            let raw = source[hint.span.start..hint.span.end].trim();
            raw == "binary" || raw == "bytes"
        });

        if is_binary_call_arg || is_binary_default || is_binary_type_hint {
            // Check if parameter is already typed or untyped
            if let Some(ref hint) = param.type_hint {
                // If it's already "byte" or "bytes", nothing to change
                let raw_hint = &source[hint.span.start..hint.span.end];
                if raw_hint.trim() != "byte" {
                    edits.push(TextEdit {
                        start: hint.span.start,
                        end: hint.span.end,
                        replacement: "byte".to_string(),
                    });
                }
            } else {
                // Untyped parameter: insert ": byte" after param name
                let insert_pos = param.name.span.end;
                edits.push(TextEdit {
                    start: insert_pos,
                    end: insert_pos,
                    replacement: ": byte".to_string(),
                });
            }
        }
    }
}

// =========================================================================
// Step 6: Convert named arguments to positional arguments in proper order
// =========================================================================

/// Migrate named arguments across a PHP source string.
/// Converts call sites with named arguments (`fn(b: 2, a: 1)`) to positional arguments
/// in the proper parameter declaration order (`fn(1, 2)`), stripping argument names
/// and inserting skipped argument commas (`fn($a, , $c)`) for omitted optional parameters.
/// Rewrite prefix nullable types to postfix form.
///
/// `?int` becomes `int?`, `\App\User` prefixed with `?` becomes `\App\User?`,
/// and untyped `?$var` becomes `mixed? $var`. Strings, comments, `?->`, `??`,
/// `?:`, and ternary branches such as `$flag ? int($x) : 0` are left alone.
/// A `?` beside `|` or `&` is left alone so the result is not `int?|string`.
pub fn rewrite_postfix_nullable(source: &str) -> String {
    let tokens = match Lexer::tokenize(source) {
        Ok(tokens) => tokens,
        Err(_) => return source.to_string(),
    };
    if tokens.is_empty() {
        return source.to_string();
    }

    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;
    let mut i = 0usize;
    while i < tokens.len() {
        if !matches!(tokens[i].token, Token::Question) {
            i += 1;
            continue;
        }
        if let Some((end_idx, replacement)) = match_prefix_nullable(source, &tokens, i) {
            let q_start = tokens[i].span.start;
            let type_end = tokens[end_idx].span.end;
            out.push_str(&source[cursor..q_start]);
            out.push_str(&replacement);
            cursor = type_end;
            i = end_idx + 1;
        } else {
            i += 1;
        }
    }
    out.push_str(&source[cursor..]);
    out
}

fn match_prefix_nullable(
    source: &str,
    tokens: &[crate::lexer::SpannedToken<'_>],
    q_idx: usize,
) -> Option<(usize, String)> {
    let next = q_idx + 1;
    if next >= tokens.len() {
        return None;
    }

    // Untyped `?$var` in a type position becomes `mixed? $var`.
    if matches!(tokens[next].token, Token::Variable(_)) {
        if is_nullable_follower(tokens, next + 1) {
            let var = &source[tokens[next].span.start..tokens[next].span.end];
            return Some((next, format!("mixed? {var}")));
        }
        return None;
    }

    let type_end = type_name_end(tokens, next)?;
    if !is_nullable_follower(tokens, type_end + 1) {
        return None;
    }
    let type_slice = &source[tokens[next].span.start..tokens[type_end].span.end];
    Some((type_end, format!("{type_slice}?")))
}

fn is_type_name_token(token: &Token<'_>) -> bool {
    token.is_type()
        || matches!(
            token,
            Token::Ident(_) | Token::Static | Token::Null | Token::True | Token::False
        )
}

/// Last token index of a base type or qualified name starting at `start`.
fn type_name_end(tokens: &[crate::lexer::SpannedToken<'_>], start: usize) -> Option<usize> {
    if start >= tokens.len() {
        return None;
    }
    let mut i = start;
    if matches!(tokens[i].token, Token::Backslash) {
        i += 1;
        if i >= tokens.len() || !matches!(tokens[i].token, Token::Ident(_)) {
            return None;
        }
    } else if !is_type_name_token(&tokens[i].token) {
        return None;
    }

    if matches!(tokens[i].token, Token::Ident(_)) {
        i += 1;
        while i + 1 < tokens.len()
            && matches!(tokens[i].token, Token::Backslash)
            && matches!(tokens[i + 1].token, Token::Ident(_))
        {
            i += 2;
        }
        return Some(i - 1);
    }
    Some(i)
}

fn is_nullable_follower(tokens: &[crate::lexer::SpannedToken<'_>], idx: usize) -> bool {
    let Some(tok) = tokens.get(idx) else {
        return true;
    };
    match &tok.token {
        Token::Variable(_)
        | Token::Ellipsis
        | Token::Assign
        | Token::Comma
        | Token::CloseParen
        | Token::OpenBrace
        | Token::SemiColon => true,
        Token::Ampersand => matches!(
            tokens.get(idx + 1).map(|t| &t.token),
            Some(Token::Variable(_) | Token::Ellipsis)
        ),
        _ => false,
    }
}

fn is_prop_modifier(tok: &Token<'_>) -> bool {
    matches!(
        tok,
        Token::Public | Token::Protected | Token::Private | Token::Readonly | Token::Static
    ) || matches!(tok, Token::Ident("var"))
}

/// Migrates PHP parameter types (`Type $param`) and class property types (`public Type $prop;`)
/// to RHP colon syntax (`$param: Type`, `public $prop: Type;`).
/// Preserves prefix nullable `?Type` as `$param: ?Type` and `$prop: ?Type`.
pub fn migrate_php_parameter_and_property_types(source: &str) -> (String, usize) {
    let tokens = match Lexer::tokenize(source) {
        Ok(t) => t,
        Err(_) => return (source.to_string(), 0),
    };
    if tokens.is_empty() {
        return (source.to_string(), 0);
    }

    let mut edits: Vec<TextEdit> = Vec::new();
    let n = tokens.len();
    let mut i = 0;
    let mut brace_depth = 0;
    let mut class_depth_stack: Vec<usize> = Vec::new();
    let mut next_open_brace_is_class = false;

    while i < n {
        match &tokens[i].token {
            Token::Class | Token::Trait | Token::Interface | Token::Enum => {
                if !(i > 0 && matches!(tokens[i - 1].token, Token::DoubleColon)) {
                    next_open_brace_is_class = true;
                }
            }
            Token::OpenBrace => {
                brace_depth += 1;
                if next_open_brace_is_class {
                    class_depth_stack.push(brace_depth);
                    next_open_brace_is_class = false;
                }
            }
            Token::CloseBrace => {
                if class_depth_stack.last() == Some(&brace_depth) {
                    class_depth_stack.pop();
                }
                brace_depth = brace_depth.saturating_sub(1);
            }
            Token::Function | Token::Fn => {
                let mut j = i + 1;
                if j < n && matches!(tokens[j].token, Token::Ampersand) {
                    j += 1;
                }
                if j < n && matches!(tokens[j].token, Token::Ident(_)) {
                    j += 1;
                }
                if j < n && matches!(tokens[j].token, Token::OpenParen) {
                    let paren_open = j;
                    let mut paren_depth = 1;
                    let mut k = j + 1;
                    while k < n && paren_depth > 0 {
                        match &tokens[k].token {
                            Token::OpenParen => paren_depth += 1,
                            Token::CloseParen => paren_depth -= 1,
                            _ => {}
                        }
                        if paren_depth > 0 {
                            k += 1;
                        }
                    }
                    let paren_close = k;
                    collect_parameter_list_edits(source, &tokens, paren_open + 1, paren_close, &mut edits);
                    i = paren_close;
                }
            }
            tok if is_prop_modifier(tok) && class_depth_stack.last() == Some(&brace_depth) =>
            {
                let mut m_end = i;
                while m_end < n && is_prop_modifier(&tokens[m_end].token) {
                    m_end += 1;
                }
                if m_end < n && !matches!(tokens[m_end].token, Token::Function | Token::Const | Token::Use) {
                    let mut stmt_end = m_end;
                    let mut p_depth = 0;
                    let mut b_depth = 0;
                    while stmt_end < n {
                        match &tokens[stmt_end].token {
                            Token::OpenParen => p_depth += 1,
                            Token::CloseParen => if p_depth > 0 { p_depth -= 1; },
                            Token::OpenBracket => b_depth += 1,
                            Token::CloseBracket => if b_depth > 0 { b_depth -= 1; },
                            Token::SemiColon if p_depth == 0 && b_depth == 0 => break,
                            Token::OpenBrace if p_depth == 0 && b_depth == 0 => break,
                            _ => {}
                        }
                        stmt_end += 1;
                    }
                    collect_property_edits(source, &tokens, m_end, stmt_end, &mut edits);
                    i = stmt_end;
                }
            }
            _ => {}
        }
        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));
    edits.dedup_by(|a, b| a.start == b.start && a.end == b.end);

    let count = edits.len();
    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

fn collect_parameter_list_edits(
    source: &str,
    tokens: &[crate::lexer::SpannedToken<'_>],
    start: usize,
    end: usize,
    edits: &mut Vec<TextEdit>,
) {
    let mut p_start = start;
    let mut paren_depth = 0;
    let mut bracket_depth = 0;
    let mut brace_depth = 0;

    for idx in start..=end {
        let is_comma_or_end = if idx == end {
            true
        } else {
            match &tokens[idx].token {
                Token::OpenParen => { paren_depth += 1; false }
                Token::CloseParen => { if paren_depth > 0 { paren_depth -= 1; } false }
                Token::OpenBracket => { bracket_depth += 1; false }
                Token::CloseBracket => { if bracket_depth > 0 { bracket_depth -= 1; } false }
                Token::OpenBrace => { brace_depth += 1; false }
                Token::CloseBrace => { if brace_depth > 0 { brace_depth -= 1; } false }
                Token::Comma if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => true,
                _ => false,
            }
        };

        if is_comma_or_end {
            let p_end = idx;
            if p_start < p_end {
                migrate_single_parameter(source, tokens, p_start, p_end, edits);
            }
            p_start = idx + 1;
        }
    }
}

fn migrate_single_parameter(
    source: &str,
    tokens: &[crate::lexer::SpannedToken<'_>],
    start: usize,
    end: usize,
    edits: &mut Vec<TextEdit>,
) {
    let mut k = start;
    // 1. Skip attribute blocks: #[...]
    while k < end && matches!(tokens[k].token, Token::AttributeStart) {
        k += 1;
        let mut b_depth = 1;
        while k < end && b_depth > 0 {
            match &tokens[k].token {
                Token::OpenBracket => b_depth += 1,
                Token::CloseBracket => b_depth -= 1,
                _ => {}
            }
            k += 1;
        }
    }

    // 2. Skip constructor promotion modifiers: public, protected, private, readonly
    while k < end && matches!(
        tokens[k].token,
        Token::Public | Token::Protected | Token::Private | Token::Readonly
    ) {
        k += 1;
    }

    let type_start_idx = k;

    // 3. Find the parameter variable (at depth 0 before any Assign `=`)
    let mut var_idx = None;
    let mut p_depth = 0;
    let mut b_depth = 0;
    let mut curr = k;
    while curr < end {
        match &tokens[curr].token {
            Token::OpenParen => p_depth += 1,
            Token::CloseParen => if p_depth > 0 { p_depth -= 1; },
            Token::OpenBracket => b_depth += 1,
            Token::CloseBracket => if b_depth > 0 { b_depth -= 1; },
            Token::Assign if p_depth == 0 && b_depth == 0 => break,
            Token::Variable(_) if p_depth == 0 && b_depth == 0 => {
                var_idx = Some(curr);
                break;
            }
            _ => {}
        }
        curr += 1;
    }

    let Some(v_idx) = var_idx else { return };

    // Check if '&' or '...' precedes v_idx
    let mut var_start_idx = v_idx;
    if var_start_idx > k && matches!(tokens[var_start_idx - 1].token, Token::Ellipsis) {
        var_start_idx -= 1;
    }
    if var_start_idx > k && matches!(tokens[var_start_idx - 1].token, Token::Ampersand) {
        var_start_idx -= 1;
    }

    // Check if there is already a colon after v_idx: `$x: Type`
    if v_idx + 1 < end && matches!(tokens[v_idx + 1].token, Token::Colon) {
        return;
    }

    if type_start_idx < var_start_idx {
        let type_start = tokens[type_start_idx].span.start;
        let type_end = tokens[var_start_idx - 1].span.end;
        let var_start = tokens[var_start_idx].span.start;
        let var_end = tokens[v_idx].span.end;

        let type_slice = source[type_start..type_end].trim();
        let var_slice = &source[var_start..var_end];

        let replacement = if type_slice == "?" {
            format!("{var_slice}: mixed?")
        } else if let Some(stripped) = type_slice.strip_prefix('?') {
            format!("{var_slice}: {stripped}?")
        } else {
            format!("{var_slice}: {type_slice}")
        };

        edits.push(TextEdit {
            start: type_start,
            end: var_end,
            replacement,
        });
    }
}

fn collect_property_edits(
    source: &str,
    tokens: &[crate::lexer::SpannedToken<'_>],
    start: usize,
    end: usize,
    edits: &mut Vec<TextEdit>,
) {
    if start >= end {
        return;
    }

    // Find the property variable before '=' or ';' at depth 0
    let mut var_idx = None;
    let mut p_depth = 0;
    let mut b_depth = 0;
    let mut curr = start;

    while curr < end {
        match &tokens[curr].token {
            Token::OpenParen => p_depth += 1,
            Token::CloseParen => if p_depth > 0 { p_depth -= 1; },
            Token::OpenBracket => b_depth += 1,
            Token::CloseBracket => if b_depth > 0 { b_depth -= 1; },
            Token::Assign if p_depth == 0 && b_depth == 0 => break,
            Token::SemiColon if p_depth == 0 && b_depth == 0 => break,
            Token::Variable(_) if p_depth == 0 && b_depth == 0 => {
                var_idx = Some(curr);
                break;
            }
            _ => {}
        }
        curr += 1;
    }

    let Some(v_idx) = var_idx else { return };

    // Check if already in colon syntax: `$x: Type`
    if v_idx + 1 < end && matches!(tokens[v_idx + 1].token, Token::Colon) {
        return;
    }

    if v_idx > start {
        let type_start = tokens[start].span.start;
        let type_end = tokens[v_idx - 1].span.end;
        let var_start = tokens[v_idx].span.start;
        let var_end = tokens[v_idx].span.end;

        let type_slice = source[type_start..type_end].trim();
        let var_slice = &source[var_start..var_end];

        let replacement = if type_slice == "?" {
            format!("{var_slice}: mixed?")
        } else if let Some(stripped) = type_slice.strip_prefix('?') {
            format!("{var_slice}: {stripped}?")
        } else {
            format!("{var_slice}: {type_slice}")
        };

        edits.push(TextEdit {
            start: type_start,
            end: var_end,
            replacement,
        });
    }
}

pub fn migrate_named_arguments(source: &str) -> (String, usize) {
    migrate_named_arguments_with_context(source, &HashMap::new())
}

/// Migrate named arguments across a PHP source string with additional context signatures.
pub fn migrate_named_arguments_with_context(
    source: &str,
    extra_signatures: &HashMap<String, Vec<String>>,
) -> (String, usize) {
    let mut current = source.to_string();
    let mut total_count = 0;

    // Build combined parameter signatures: built-ins + local source definitions + extra context
    let mut signatures = build_builtin_parameter_map();
    signatures.extend(extract_function_signatures(source));
    signatures.extend(extra_signatures.clone());

    let mut iteration = 0;
    while iteration < 50 {
        iteration += 1;
        let mask = build_code_mask(&current);
        let bytes = current.as_bytes();
        let n = bytes.len();

        let mut edits: Vec<TextEdit> = Vec::new();
        let mut i = 0;

        while i < n {
            if !mask[i] || bytes[i] != b'(' {
                i += 1;
                continue;
            }

            let open_paren = i;

            // Check if this paren is inside an attribute annotation (e.g. #[Route(path: "/api")])
            if is_inside_attribute_at(&current, &mask, open_paren) {
                i += 1;
                continue;
            }

            // Extract callee name preceding open paren
            let (full_callee, simple_callee) = match extract_callee_preceding_paren(&current, &mask, open_paren) {
                Some(c) => c,
                None => {
                    i += 1;
                    continue;
                }
            };

            // Find matching closing paren
            let close_paren = match find_matching_paren(&current, &mask, open_paren) {
                Some(cp) => cp,
                None => {
                    i += 1;
                    continue;
                }
            };

            let args_str = &current[open_paren + 1..close_paren];
            let raw_args = split_call_args(args_str);

            let mut parsed_args = Vec::new();
            let mut has_named = false;
            for arg in &raw_args {
                let (opt_name, val) = parse_named_arg(arg);
                if opt_name.is_some() {
                    has_named = true;
                }
                parsed_args.push((opt_name, val));
            }

            if !has_named {
                i += 1;
                continue;
            }

            // If this call contains nested calls that still have named arguments, skip outer in this pass
            if has_nested_named_arg_call(args_str) {
                i += 1;
                continue;
            }

            // Find parameter list for this callee
            let params_opt = signatures
                .get(&full_callee)
                .or_else(|| signatures.get(&simple_callee));

            let multiline = args_str.contains('\n');
            let indent = detect_indent(args_str);
            let had_trailing_comma = args_str.trim_end().ends_with(',');

            let formatted = if let Some(params) = params_opt {
                let mut slots: Vec<Option<String>> = vec![None; params.len()];
                let mut extra_args: Vec<String> = Vec::new();

                for (opt_name, val) in parsed_args {
                    match opt_name {
                        Some(name) => {
                            if let Some(idx) = params.iter().position(|p| p == &name) {
                                if idx >= slots.len() {
                                    slots.resize(idx + 1, None);
                                }
                                slots[idx] = Some(val);
                            } else {
                                extra_args.push(val);
                            }
                        }
                        None => {
                            let next_free = slots.iter().position(|s| s.is_none()).unwrap_or(slots.len());
                            if next_free < slots.len() {
                                slots[next_free] = Some(val);
                            } else {
                                slots.push(Some(val));
                            }
                        }
                    }
                }

                let max_idx = slots.iter().rposition(|s| s.is_some());
                let mut final_slots = Vec::new();
                if let Some(max_i) = max_idx {
                    for j in 0..=max_i {
                        final_slots.push(slots[j].clone().unwrap_or_default());
                    }
                }
                final_slots.extend(extra_args);

                format_call_args(&final_slots, multiline, &indent, had_trailing_comma)
            } else {
                // Unknown function: keep arguments in original order, strip names
                let mut final_slots = Vec::new();
                for (_, val) in parsed_args {
                    final_slots.push(val);
                }
                format_call_args(&final_slots, multiline, &indent, had_trailing_comma)
            };

            edits.push(TextEdit {
                start: open_paren + 1,
                end: close_paren,
                replacement: formatted,
            });

            i = close_paren + 1;
        }

        if edits.is_empty() {
            break;
        }

        total_count += edits.len();
        edits.sort_by(|a, b| b.start.cmp(&a.start));
        for edit in edits {
            current.replace_range(edit.start..edit.end, &edit.replacement);
        }
    }

    (current, total_count)
}

fn byte_offset_to_line(source: &str, offset: usize) -> usize {
    let limit = offset.min(source.len());
    source[..limit].chars().filter(|&c| c == '\n').count() + 1
}

/// Swap arguments at normal positional call sites for reordered functions,
/// and emit detailed warnings for tricky call sites (e.g. argument unpacking `...`, callbacks, dynamic calls).
pub fn swap_positional_calls_and_warn_tricky(
    source: &str,
    reordered_fns: &[ReorderedFunctionInfo],
) -> (String, Vec<String>, usize) {
    let mut warnings = Vec::new();
    let mut current = source.to_string();
    let mut swapped_count = 0;

    if reordered_fns.is_empty() {
        return (current, warnings, 0);
    }

    let mut fn_map: HashMap<String, &ReorderedFunctionInfo> = HashMap::new();
    for f in reordered_fns {
        fn_map.insert(f.name.clone(), f);
        warnings.push(format!(
            "WARNING: Parameter order changed for '{}'. Please inspect non-named call sites of '{}()' for argument alignment.",
            f.old_signature, f.name
        ));
    }

    let mask = build_code_mask(&current);
    let bytes = current.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();

    let callback_dispatchers = [
        "call_user_func",
        "call_user_func_array",
        "array_map",
        "array_filter",
        "usort",
        "register_shutdown_function",
    ];

    let mut i = 0;
    while i < n {
        if !mask[i] || bytes[i] != b'(' {
            i += 1;
            continue;
        }

        let open_paren = i;
        if is_inside_attribute_at(&current, &mask, open_paren) {
            i += 1;
            continue;
        }

        let (full_callee, simple_callee) = match extract_callee_preceding_paren(&current, &mask, open_paren) {
            Some(c) => c,
            None => {
                i += 1;
                continue;
            }
        };

        let close_paren = match find_matching_paren(&current, &mask, open_paren) {
            Some(cp) => cp,
            None => {
                i += 1;
                continue;
            }
        };

        let args_str = &current[open_paren + 1..close_paren];
        let call_line = byte_offset_to_line(&current, open_paren);

        // 1. Check callback dispatchers (e.g. call_user_func('myfun', ...))
        if callback_dispatchers.contains(&simple_callee.as_str()) {
            for (fn_name, info) in &fn_map {
                let needle1 = format!("'{}'", fn_name);
                let needle2 = format!("\"{}\"", fn_name);
                if args_str.contains(&needle1) || args_str.contains(&needle2) {
                    warnings.push(format!(
                        "WARNING: Parameter order changed for '{}'. Tricky call site using callback ({}) cannot be automatically reordered at line {}. Please inspect this call site for argument alignment.",
                        info.name, simple_callee, call_line
                    ));
                }
            }
            i = close_paren + 1;
            continue;
        }

        // 2. Check direct calls to reordered functions
        let target_info = fn_map.get(&simple_callee).or_else(|| fn_map.get(&full_callee));
        if let Some(info) = target_info {
            // Check if call has tricky syntax: argument unpacking `...`
            if args_str.contains("...") {
                warnings.push(format!(
                    "WARNING: Parameter order changed for '{}'. Tricky call site using argument unpacking (...) cannot be automatically reordered at line {}. Please inspect this call site for argument alignment.",
                    info.name, call_line
                ));
                i = close_paren + 1;
                continue;
            }

            let raw_args = split_call_args(args_str);
            // Check if call has named arguments (handled by migrate_named_arguments_with_context)
            let has_named = raw_args.iter().any(|arg| parse_named_arg(arg).0.is_some());
            if has_named {
                i = close_paren + 1;
                continue;
            }

            if raw_args.is_empty() {
                i = close_paren + 1;
                continue;
            }

            // Normal positional call site: swap arguments to match info.new_params
            let mut arg_by_old_name: HashMap<String, String> = HashMap::new();
            for (arg_idx, arg_val) in raw_args.iter().enumerate() {
                if arg_idx < info.old_params.len() {
                    let old_param_clean = info.old_params[arg_idx].trim_start_matches('$').to_string();
                    arg_by_old_name.insert(old_param_clean, arg_val.trim().to_string());
                }
            }

            let mut new_slots: Vec<Option<String>> = vec![None; info.new_params.len()];
            for (new_idx, param_name) in info.new_params.iter().enumerate() {
                let param_clean = param_name.trim_start_matches('$');
                if let Some(val) = arg_by_old_name.get(param_clean) {
                    new_slots[new_idx] = Some(val.clone());
                }
            }

            let mut extra_args = Vec::new();
            if raw_args.len() > info.old_params.len() {
                for extra in &raw_args[info.old_params.len()..] {
                    extra_args.push(extra.trim().to_string());
                }
            }

            let max_idx = new_slots.iter().rposition(|s| s.is_some());
            let mut final_args = Vec::new();
            if let Some(max_i) = max_idx {
                for j in 0..=max_i {
                    final_args.push(new_slots[j].clone().unwrap_or_default());
                }
            }
            final_args.extend(extra_args);

            let multiline = args_str.contains('\n');
            let indent = detect_indent(args_str);
            let trailing_comma = args_str.trim_end().ends_with(',');
            let formatted = format_call_args(&final_args, multiline, &indent, trailing_comma);

            if formatted != args_str {
                edits.push(TextEdit {
                    start: open_paren + 1,
                    end: close_paren,
                    replacement: formatted,
                });
                swapped_count += 1;
            }
        }

        i = close_paren + 1;
    }

    if !edits.is_empty() {
        edits.sort_by(|a, b| b.start.cmp(&a.start));
        edits.dedup_by(|a, b| a.start == b.start && a.end == b.end);
        for edit in edits {
            if edit.end <= current.len() && edit.start <= edit.end {
                current.replace_range(edit.start..edit.end, &edit.replacement);
            }
        }
    }

    (current, warnings, swapped_count)
}

/// Recursively collect function and method parameter signatures across all PHP/RHP files in a directory.
pub fn collect_signatures_recursive(
    dir: &Path,
    signatures: &mut HashMap<String, Vec<String>>,
) -> io::Result<()> {
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
            collect_signatures_recursive(&path, signatures)?;
        } else if is_php_or_rhp_file(&path) {
            if let Ok(content) = fs::read_to_string(&path) {
                let file_sigs = extract_function_signatures(&content);
                signatures.extend(file_sigs);
            }
        }
    }

    Ok(())
}

/// Extract function, class method, and constructor parameter signatures from source code.
/// Ensures parameter lists are normalized with required parameters preceding optional ones.
pub fn extract_function_signatures(source: &str) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();

    let mut i = 0;
    let mut current_class: Option<String> = None;
    let mut brace_depth = 0;
    let mut class_brace_depth: Option<usize> = None;

    while i < n {
        if !mask[i] {
            i += 1;
            continue;
        }

        if bytes[i] == b'{' {
            brace_depth += 1;
            i += 1;
            continue;
        } else if bytes[i] == b'}' {
            if brace_depth > 0 {
                brace_depth -= 1;
            }
            if let Some(target) = class_brace_depth {
                if brace_depth < target {
                    current_class = None;
                    class_brace_depth = None;
                }
            }
            i += 1;
            continue;
        }

        // Check for class, interface, trait, or enum declaration
        let is_class_kw = (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_')
            && ((source[i..].starts_with("class") && (i + 5 == n || !bytes[i + 5].is_ascii_alphanumeric() && bytes[i + 5] != b'_'))
                || (source[i..].starts_with("interface") && (i + 9 == n || !bytes[i + 9].is_ascii_alphanumeric() && bytes[i + 9] != b'_'))
                || (source[i..].starts_with("trait") && (i + 5 == n || !bytes[i + 5].is_ascii_alphanumeric() && bytes[i + 5] != b'_'))
                || (source[i..].starts_with("enum") && (i + 4 == n || !bytes[i + 4].is_ascii_alphanumeric() && bytes[i + 4] != b'_')));

        if is_class_kw {
            let kw_len = if source[i..].starts_with("interface") {
                9
            } else if source[i..].starts_with("enum") {
                4
            } else {
                5
            };
            let mut p = i + kw_len;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }
            let name_start = p;
            while p < n && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_') {
                p += 1;
            }
            if p > name_start {
                let name = source[name_start..p].to_string();
                current_class = Some(name);
                class_brace_depth = Some(brace_depth + 1);
            }
            i = p;
            continue;
        }

        // Check for function declaration
        let is_func_kw = (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_')
            && source[i..].starts_with("function")
            && (i + 8 == n || !bytes[i + 8].is_ascii_alphanumeric() && bytes[i + 8] != b'_');

        if is_func_kw {
            let mut p = i + 8;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }
            if p < n && bytes[p] == b'&' {
                p += 1;
                while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                    p += 1;
                }
            }
            let name_start = p;
            while p < n && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_') {
                p += 1;
            }
            let func_name = if p > name_start {
                source[name_start..p].to_string()
            } else {
                String::new()
            };

            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }

            if p < n && bytes[p] == b'(' {
                let open_paren = p;
                if let Some(close_paren) = find_matching_paren(source, &mask, open_paren) {
                    let params_str = &source[open_paren + 1..close_paren];
                    let params = parse_param_definition_names(params_str);
                    if !func_name.is_empty() {
                        if let Some(ref cls) = current_class {
                            if func_name == "__construct" {
                                map.insert(cls.clone(), params.clone());
                                map.insert(format!("{}::__construct", cls), params.clone());
                            }
                            map.insert(format!("{}::{}", cls, func_name), params.clone());
                            map.insert(func_name.clone(), params.clone());
                        } else {
                            map.insert(func_name.clone(), params.clone());
                        }
                    }
                    i = close_paren + 1;
                    continue;
                }
            }
            i = p;
            continue;
        }

        i += 1;
    }

    map
}

/// Build parameter mapping for built-in PHP functions from facade definitions and PHP 8 standard library.
pub fn build_builtin_parameter_map() -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();

    // 1. Populate from LEGACY_PHP_FUNCTIONS
    for mapping in LEGACY_PHP_FUNCTIONS {
        let sig = mapping.rhp_signature;
        if let Some(open) = sig.find('(') {
            if let Some(close) = sig.rfind(')') {
                let params_str = &sig[open + 1..close];
                let params = parse_param_definition_names(params_str);
                if !params.is_empty() {
                    map.insert(mapping.php_name.to_string(), params.clone());
                    map.insert(mapping.rhp_method.to_string(), params.clone());
                    let full_facade_method = format!("{}::{}", mapping.rhp_facade, mapping.rhp_method);
                    map.insert(full_facade_method, params);
                }
            }
        }
    }

    // 2. Comprehensive standard PHP functions with their official PHP 8 parameter names
    let standard_fns: &[(&str, &[&str])] = &[
        ("htmlspecialchars", &["string", "flags", "encoding", "double_encode"]),
        ("setcookie", &["name", "value", "expires_or_options", "path", "domain", "secure", "httponly"]),
        ("setrawcookie", &["name", "value", "expires_or_options", "path", "domain", "secure", "httponly"]),
        ("explode", &["separator", "string", "limit"]),
        ("implode", &["separator", "array"]),
        ("in_array", &["needle", "haystack", "strict"]),
        ("array_search", &["needle", "haystack", "strict"]),
        ("str_contains", &["haystack", "needle"]),
        ("str_starts_with", &["haystack", "needle"]),
        ("str_ends_with", &["haystack", "needle"]),
        ("str_replace", &["search", "replace", "subject", "count"]),
        ("substr", &["string", "offset", "length"]),
        ("strlen", &["string"]),
        ("number_format", &["num", "decimals", "decimal_separator", "thousands_separator"]),
        ("date", &["format", "timestamp"]),
        ("round", &["num", "precision", "mode"]),
        ("preg_match", &["pattern", "subject", "matches", "flags", "offset"]),
        ("preg_replace", &["pattern", "replacement", "subject", "limit", "count"]),
        ("json_encode", &["value", "flags", "depth"]),
        ("json_decode", &["json", "associative", "depth", "flags"]),
        ("password_hash", &["password", "algo", "options"]),
        ("password_verify", &["password", "hash"]),
        ("range", &["start", "end", "step"]),
        ("array_fill", &["start_index", "count", "value"]),
        ("array_pad", &["array", "length", "value"]),
        ("array_chunk", &["array", "length", "preserve_keys"]),
        ("array_column", &["array", "column_key", "index_key"]),
        ("array_key_exists", &["key", "array"]),
        ("array_combine", &["keys", "values"]),
        ("array_slice", &["array", "offset", "length", "preserve_keys"]),
        ("array_splice", &["array", "offset", "length", "replacement"]),
        ("header", &["header", "replace", "response_code"]),
        ("http_response_code", &["response_code"]),
        ("file_get_contents", &["filename", "use_include_path", "context", "offset", "length"]),
        ("file_put_contents", &["filename", "data", "flags", "context"]),
        ("fopen", &["filename", "mode", "use_include_path", "context"]),
        ("fclose", &["stream"]),
        ("fread", &["stream", "length"]),
        ("fwrite", &["stream", "data", "length"]),
        ("trim", &["string", "characters"]),
        ("ltrim", &["string", "characters"]),
        ("rtrim", &["string", "characters"]),
        ("microtime", &["as_float"]),
        ("sleep", &["seconds"]),
        ("usleep", &["microseconds"]),
        ("strpos", &["haystack", "needle", "offset"]),
        ("strrpos", &["haystack", "needle", "offset"]),
        ("stripos", &["haystack", "needle", "offset"]),
        ("strripos", &["haystack", "needle", "offset"]),
        ("str_pad", &["string", "length", "pad_string", "pad_type"]),
        ("str_repeat", &["string", "times"]),
        ("str_split", &["string", "length"]),
        ("count", &["value", "mode"]),
    ];

    for &(fn_name, params) in standard_fns {
        map.insert(fn_name.to_string(), params.iter().map(|s| s.to_string()).collect());
    }

    map
}

fn parse_param_definition_names(params_str: &str) -> Vec<String> {
    let chunks = split_call_args(params_str);
    let mut names = Vec::new();

    for chunk in chunks {
        let trimmed = chunk.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Split on '=' to distinguish parameter declaration from default value
        let decl_part = trimmed.split('=').next().unwrap_or(trimmed);

        // Find '$' followed by identifier
        if let Some(pos) = decl_part.find('$') {
            let after_dollar = &decl_part[pos + 1..];
            let name_end = after_dollar
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(after_dollar.len());
            let name = &after_dollar[..name_end];
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }

    names
}

fn find_matching_paren(source: &str, mask: &[bool], open_paren: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let n = bytes.len();
    if open_paren >= n || bytes[open_paren] != b'(' {
        return None;
    }

    let mut depth = 1;
    let mut i = open_paren + 1;
    while i < n {
        if mask[i] {
            if bytes[i] == b'(' {
                depth += 1;
            } else if bytes[i] == b')' {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        i += 1;
    }

    None
}

fn is_inside_attribute_at(source: &str, mask: &[bool], open_paren: usize) -> bool {
    let bytes = source.as_bytes();
    let mut p = open_paren;
    let mut bracket_depth = 0;

    while p > 0 {
        p -= 1;
        if !mask[p] {
            continue;
        }
        if bytes[p] == b']' {
            bracket_depth += 1;
        } else if bytes[p] == b'[' && p > 0 && bytes[p - 1] == b'#' {
            if bracket_depth == 0 {
                return true;
            }
            bracket_depth -= 1;
            p -= 1;
        } else if bytes[p] == b';' || bytes[p] == b'{' || bytes[p] == b'}' {
            break;
        }
    }

    false
}

fn extract_callee_preceding_paren(source: &str, _mask: &[bool], open_paren: usize) -> Option<(String, String)> {
    let bytes = source.as_bytes();
    if open_paren == 0 {
        return None;
    }

    let mut p = open_paren;
    while p > 0 && (bytes[p - 1] == b' ' || bytes[p - 1] == b'\t' || bytes[p - 1] == b'\r' || bytes[p - 1] == b'\n') {
        p -= 1;
    }
    if p == 0 {
        return None;
    }

    // Must be identifier char
    if !bytes[p - 1].is_ascii_alphanumeric() && bytes[p - 1] != b'_' {
        return None;
    }

    let id_end = p;
    while p > 0 && (bytes[p - 1].is_ascii_alphanumeric() || bytes[p - 1] == b'_' || bytes[p - 1] == b'\\') {
        p -= 1;
    }
    let id_start = p;
    let raw_ident = &source[id_start..id_end];
    let simple_name = raw_ident.trim_start_matches('\\').to_string();

    // Check if simple_name is a non-call keyword
    let non_call_keywords = [
        "if", "elseif", "while", "for", "foreach", "switch", "match", "catch",
        "declare", "function", "fn", "return", "echo", "print", "list", "array",
        "isset", "empty", "unset", "eval", "include", "include_once", "require", "require_once",
    ];
    if non_call_keywords.contains(&simple_name.as_str()) {
        return None;
    }

    // Check what precedes raw_ident
    let mut q = id_start;
    while q > 0 && (bytes[q - 1] == b' ' || bytes[q - 1] == b'\t' || bytes[q - 1] == b'\r' || bytes[q - 1] == b'\n') {
        q -= 1;
    }

    // If preceded by 'function', this is a declaration, not a call
    if q >= 8 && &source[q - 8..q] == "function" && (q == 8 || !bytes[q - 9].is_ascii_alphanumeric() && bytes[q - 9] != b'_') {
        return None;
    }

    if q >= 2 && &source[q - 2..q] == "::" {
        // Class::method
        let mut c = q - 2;
        while c > 0 && (bytes[c - 1] == b' ' || bytes[c - 1] == b'\t' || bytes[c - 1] == b'\r' || bytes[c - 1] == b'\n') {
            c -= 1;
        }
        let class_end = c;
        while c > 0 && (bytes[c - 1].is_ascii_alphanumeric() || bytes[c - 1] == b'_' || bytes[c - 1] == b'\\') {
            c -= 1;
        }
        let class_name = source[c..class_end].trim_start_matches('\\');
        let full = format!("{}::{}", class_name, simple_name);
        return Some((full, simple_name));
    }

    Some((simple_name.clone(), simple_name))
}

fn split_call_args(args_str: &str) -> Vec<String> {
    let mut args = Vec::new();
    let bytes = args_str.as_bytes();
    let n = bytes.len();
    let mask = build_code_mask(args_str);

    let mut start = 0;
    let mut paren_depth = 0;
    let mut bracket_depth = 0;
    let mut brace_depth = 0;

    let mut i = 0;
    while i < n {
        if !mask[i] {
            i += 1;
            continue;
        }

        match bytes[i] {
            b'(' => paren_depth += 1,
            b')' => {
                if paren_depth > 0 {
                    paren_depth -= 1;
                }
            }
            b'[' => bracket_depth += 1,
            b']' => {
                if bracket_depth > 0 {
                    bracket_depth -= 1;
                }
            }
            b'{' => brace_depth += 1,
            b'}' => {
                if brace_depth > 0 {
                    brace_depth -= 1;
                }
            }
            b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                args.push(args_str[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }

    if start < n || (start == n && !args.is_empty()) {
        let remainder = &args_str[start..];
        if !remainder.trim().is_empty() {
            args.push(remainder.to_string());
        }
    }

    args
}

fn parse_named_arg(arg_str: &str) -> (Option<String>, String) {
    let trimmed = arg_str.trim();
    let bytes = trimmed.as_bytes();
    let n = bytes.len();
    if n == 0 {
        return (None, String::new());
    }

    if bytes[0].is_ascii_alphabetic() || bytes[0] == b'_' {
        let mut p = 0;
        while p < n && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_') {
            p += 1;
        }
        let name = &trimmed[0..p];
        let mut after = p;
        while after < n && (bytes[after] == b' ' || bytes[after] == b'\t' || bytes[after] == b'\r' || bytes[after] == b'\n') {
            after += 1;
        }
        if after < n && bytes[after] == b':' && (after + 1 == n || bytes[after + 1] != b':') {
            let val = trimmed[after + 1..].trim();
            return (Some(name.to_string()), val.to_string());
        }
    }

    (None, trimmed.to_string())
}

fn has_nested_named_arg_call(args_str: &str) -> bool {
    let mask = build_code_mask(args_str);
    let bytes = args_str.as_bytes();
    let n = bytes.len();

    let mut i = 0;
    while i < n {
        if mask[i] && bytes[i] == b'(' {
            if let Some(close) = find_matching_paren(args_str, &mask, i) {
                let inner = &args_str[i + 1..close];
                let inner_args = split_call_args(inner);
                for arg in inner_args {
                    let (opt_name, _) = parse_named_arg(&arg);
                    if opt_name.is_some() {
                        return true;
                    }
                }
                i = close + 1;
                continue;
            }
        }
        i += 1;
    }

    false
}

fn detect_indent(args_str: &str) -> String {
    for line in args_str.lines().skip(1) {
        let trimmed = line.trim_start();
        if !trimmed.is_empty() {
            let indent_len = line.len() - trimmed.len();
            return line[..indent_len].to_string();
        }
    }
    "    ".to_string()
}

fn remove_explicit_void_returns(source: &str) -> (String, usize) {
    use crate::ast::expr::ExprKind;
    use crate::ast::stmt::{ClassMemberKind, StmtKind};
    use crate::ast::types::TypeHintKind;
    use crate::ast::visitor::{walk_expr, walk_stmt, AstVisitor};
    use crate::ast::Program;

    let program = match crate::parser::parse(source) {
        Ok(program) => program,
        Err(_) => return (source.to_string(), 0),
    };

    struct VoidReturnSpanVisitor<'a> {
        ranges: &'a mut Vec<(usize, usize)>,
    }

    impl VoidReturnSpanVisitor<'_> {
        fn note_return(&mut self, hint: Option<&crate::ast::types::TypeHint<'_>>) {
            let Some(hint) = hint else {
                return;
            };
            if matches!(&hint.kind, TypeHintKind::Named(ident) if ident.name.eq_ignore_ascii_case("void")) {
                self.ranges.push((hint.span.start, hint.span.end));
            }
        }
    }

    impl<'s> AstVisitor<'s> for VoidReturnSpanVisitor<'_> {
        fn visit_stmt(&mut self, stmt: &crate::ast::stmt::Stmt<'s>) {
            match &stmt.kind {
                StmtKind::FunctionDecl(func) => self.note_return(func.return_type.as_ref()),
                StmtKind::ClassDecl(cls) => {
                    for member in &cls.members {
                        if let ClassMemberKind::Method(method) = &member.kind {
                            self.note_return(method.function.return_type.as_ref());
                        }
                    }
                }
                StmtKind::InterfaceDecl(iface) => {
                    for method in &iface.methods {
                        self.note_return(method.function.return_type.as_ref());
                    }
                }
                StmtKind::TraitDecl(tr) => {
                    for member in &tr.members {
                        if let ClassMemberKind::Method(method) = &member.kind {
                            self.note_return(method.function.return_type.as_ref());
                        }
                    }
                }
                StmtKind::EnumDecl(enm) => {
                    for method in &enm.methods {
                        self.note_return(method.function.return_type.as_ref());
                    }
                }
                _ => {}
            }
            walk_stmt(self, stmt);
            if let StmtKind::InterfaceDecl(iface) = &stmt.kind {
                for method in &iface.methods {
                    if let Some(body) = &method.function.body {
                        for inner in body {
                            self.visit_stmt(inner);
                        }
                    }
                }
            }
        }

        fn visit_expr(&mut self, expr: &crate::ast::expr::Expr<'s>) {
            match &expr.kind {
                ExprKind::ArrowFunction { return_type, .. } | ExprKind::Closure { return_type, .. } => {
                    self.note_return(return_type.as_ref());
                }
                ExprKind::AnonymousClass { class_decl, .. } => {
                    for member in &class_decl.members {
                        if let ClassMemberKind::Method(method) = &member.kind {
                            self.note_return(method.function.return_type.as_ref());
                        }
                    }
                }
                _ => {}
            }
            walk_expr(self, expr);
        }
    }

    let mut ranges = Vec::new();
    {
        let mut visitor = VoidReturnSpanVisitor { ranges: &mut ranges };
        let program_ref: &Program<'_> = &program;
        visitor.visit_program(program_ref);
    }
    if ranges.is_empty() {
        return (source.to_string(), 0);
    }

    let mut edits = Vec::new();
    for (start, end) in ranges {
        if let Some(range) = void_annotation_range(source, start, end) {
            edits.push(range);
        }
    }
    edits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    edits.dedup();
    let count = edits.len();
    let mut result = source.to_string();
    for (start, end) in edits {
        if start < end && end <= result.len() {
            result.replace_range(start..end, "");
        }
    }
    (result, count)
}

fn void_annotation_range(source: &str, hint_start: usize, hint_end: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    if hint_end > bytes.len() || hint_start > hint_end {
        return None;
    }
    let mut colon = hint_start;
    while colon > 0 && bytes[colon - 1].is_ascii_whitespace() {
        colon -= 1;
    }
    if colon == 0 || bytes[colon - 1] != b':' {
        return None;
    }
    Some((colon - 1, hint_end))
}

fn format_call_args(slots: &[String], multiline: bool, indent: &str, had_trailing_comma: bool) -> String {
    if slots.is_empty() {
        return String::new();
    }
    if multiline {
        let mut out = String::new();
        out.push('\n');
        for (i, slot) in slots.iter().enumerate() {
            out.push_str(indent);
            out.push_str(slot);
            if i + 1 < slots.len() || had_trailing_comma {
                out.push(',');
            }
            out.push('\n');
        }
        out
    } else {
        let mut out = String::new();
        for (i, slot) in slots.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(slot);
        }
        out
    }
}

pub fn check_rune_and_ctype_usage(source: &str) -> Vec<String> {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut warnings = Vec::new();
    let ctype_funcs = [
        "ctype_digit",
        "ctype_alpha",
        "ctype_alnum",
        "ctype_space",
        "ctype_upper",
        "ctype_lower",
        "ctype_punct",
        "ctype_cntrl",
    ];

    let mut i = 0;
    while i < n {
        if !mask[i] || !bytes[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }

        let start = i;
        while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let ident = &source[start..i];

        if ctype_funcs.contains(&ident) {
            let line = byte_offset_to_line(source, start);
            warnings.push(format!(
                "NOTICE: '{}()' only checks ASCII characters at line {}. For full Unicode character inspection, consider using 'Rune::is*()' or 'Str::*()'.",
                ident, line
            ));
        }
    }
    warnings
}

// =========================================================================
// Step 0.0001: Transpile PHP Template Tags to Rade Directives
// =========================================================================

fn extract_escaped_expr(inner: &str) -> String {
    let s = inner.trim().trim_end_matches(';').trim();
    let lower = s.to_ascii_lowercase();
    let func = if lower.starts_with("htmlspecialchars") {
        Some("htmlspecialchars")
    } else if lower.starts_with("htmlentities") {
        Some("htmlentities")
    } else {
        None
    };

    if let Some(f) = func {
        let after_f = s[f.len()..].trim_start();
        if after_f.starts_with('(') && after_f.ends_with(')') {
            let inside = after_f[1..after_f.len() - 1].trim();
            let mut depth = 0;
            let mut in_single = false;
            let mut in_double = false;
            let mut first_arg_end = inside.len();

            let bytes = inside.as_bytes();
            for (idx, &b) in bytes.iter().enumerate() {
                if b == b'\'' && !in_double {
                    in_single = !in_single;
                } else if b == b'"' && !in_single {
                    in_double = !in_double;
                } else if !in_single && !in_double {
                    if b == b'(' || b == b'[' {
                        depth += 1;
                    } else if b == b')' || b == b']' {
                        if depth > 0 { depth -= 1; }
                    } else if depth == 0 && b == b',' {
                        first_arg_end = idx;
                        break;
                    }
                }
            }
            return inside[..first_arg_end].trim().to_string();
        }
    }
    s.to_string()
}

fn is_template_source(source: &str) -> bool {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut i = 0;

    while i < n {
        if !mask[i] {
            i += 1;
            continue;
        }

        if bytes[i..].starts_with(b"<?=") {
            return true;
        }

        if bytes[i..].starts_with(b"<?php") {
            let after = i + 5;
            let mut p = after;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }
            if p + 2 <= n && &bytes[p..p + 2] == b"if" {
                if let Some(tag_end) = source[p..].find("?>") {
                    let inside = source[p..p + tag_end].trim();
                    if inside.ends_with(':') {
                        return true;
                    }
                }
            } else if p + 7 <= n && &bytes[p..p + 7] == b"foreach" {
                if let Some(tag_end) = source[p..].find("?>") {
                    let inside = source[p..p + tag_end].trim();
                    if inside.ends_with(':') {
                        return true;
                    }
                }
            } else if p + 3 <= n && &bytes[p..p + 3] == b"for" {
                if let Some(tag_end) = source[p..].find("?>") {
                    let inside = source[p..p + tag_end].trim();
                    if inside.ends_with(':') {
                        return true;
                    }
                }
            } else if p + 5 <= n && (&bytes[p..p + 5] == b"endif" || &bytes[p..p + 5] == b"while") {
                if &bytes[p..p + 5] == b"endif" {
                    return true;
                }
                if let Some(tag_end) = source[p..].find("?>") {
                    let inside = source[p..p + tag_end].trim();
                    if inside.ends_with(':') {
                        return true;
                    }
                }
            } else if p + 10 <= n && &bytes[p..p + 10] == b"endforeach" {
                return true;
            } else if p + 6 <= n && (&bytes[p..p + 6] == b"endfor" || &bytes[p..p + 6] == b"elseif") {
                return true;
            } else if p + 8 <= n && &bytes[p..p + 8] == b"endwhile" {
                return true;
            }
        }

        i += 1;
    }

    false
}

pub fn transpile_php_template_to_rade(source: &str) -> (String, usize) {
    if !is_template_source(source) {
        return (source.to_string(), 0);
    }

    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut i = 0;

    while i < n {
        if !mask[i] {
            i += 1;
            continue;
        }

        // Match <?= ... ?> short echo tag
        if bytes[i..].starts_with(b"<?=") {
            let tag_start = i;
            let after_open = i + 3;
            let mut p = after_open;
            let mut in_single = false;
            let mut in_double = false;
            let mut close_tag = None;

            while p < n {
                let b = bytes[p];
                if b == b'\'' && !in_double {
                    in_single = !in_single;
                } else if b == b'"' && !in_single {
                    in_double = !in_double;
                } else if !in_single && !in_double && bytes[p..].starts_with(b"?>") {
                    close_tag = Some(p);
                    break;
                }
                p += 1;
            }

            if let Some(tag_end) = close_tag {
                let inner = &source[after_open..tag_end];
                let expr = extract_escaped_expr(inner);
                edits.push(TextEdit {
                    start: tag_start,
                    end: tag_end + 2,
                    replacement: format!("{{{{ {} }}}}", expr.trim()),
                });
                count += 1;
                i = tag_end + 2;
                continue;
            }
        }

        // Match <?php ... ?> tags
        if bytes[i..].starts_with(b"<?php") {
            let tag_start = i;
            let after_open = i + 5;
            let mut p = after_open;
            let mut in_single = false;
            let mut in_double = false;
            let mut close_tag = None;

            while p < n {
                let b = bytes[p];
                if b == b'\'' && !in_double {
                    in_single = !in_single;
                } else if b == b'"' && !in_single {
                    in_double = !in_double;
                } else if !in_single && !in_double && bytes[p..].starts_with(b"?>") {
                    close_tag = Some(p);
                    break;
                }
                p += 1;
            }

            if let Some(tag_end) = close_tag {
                let inner = source[after_open..tag_end].trim();

                // 1) echo in template: <?php echo ...; ?>
                if inner.starts_with("echo") && (inner.len() == 4 || inner.as_bytes()[4].is_ascii_whitespace()) {
                    let rest = inner[4..].trim();
                    let expr = extract_escaped_expr(rest);
                    edits.push(TextEdit {
                        start: tag_start,
                        end: tag_end + 2,
                        replacement: format!("{{{{ {} }}}}", expr.trim()),
                    });
                    count += 1;
                    i = tag_end + 2;
                    continue;
                }

                // 2) Control flow directives with ':'
                if inner.starts_with("if") && (inner.len() == 2 || inner.as_bytes()[2] == b' ' || inner.as_bytes()[2] == b'(') {
                    let rest = inner[2..].trim();
                    if rest.starts_with('(') && rest.ends_with(':') {
                        let cond = rest[..rest.len() - 1].trim();
                        edits.push(TextEdit {
                            start: tag_start,
                            end: tag_end + 2,
                            replacement: format!("@if {}", cond),
                        });
                        count += 1;
                        i = tag_end + 2;
                        continue;
                    }
                }

                if inner.starts_with("elseif") && (inner.len() == 6 || inner.as_bytes()[6] == b' ' || inner.as_bytes()[6] == b'(') {
                    let rest = inner[6..].trim();
                    if rest.starts_with('(') && rest.ends_with(':') {
                        let cond = rest[..rest.len() - 1].trim();
                        edits.push(TextEdit {
                            start: tag_start,
                            end: tag_end + 2,
                            replacement: format!("@elseif {}", cond),
                        });
                        count += 1;
                        i = tag_end + 2;
                        continue;
                    }
                }

                if inner == "else:" || inner == "else :" {
                    edits.push(TextEdit {
                        start: tag_start,
                        end: tag_end + 2,
                        replacement: "@else".to_string(),
                    });
                    count += 1;
                    i = tag_end + 2;
                    continue;
                }

                if inner == "endif;" || inner == "endif ;" || inner == "endif:" || inner == "endif" {
                    edits.push(TextEdit {
                        start: tag_start,
                        end: tag_end + 2,
                        replacement: "@endif".to_string(),
                    });
                    count += 1;
                    i = tag_end + 2;
                    continue;
                }

                if inner.starts_with("foreach") && (inner.len() == 7 || inner.as_bytes()[7] == b' ' || inner.as_bytes()[7] == b'(') {
                    let rest = inner[7..].trim();
                    if rest.starts_with('(') && rest.ends_with(':') {
                        let loop_hdr = rest[1..rest.len() - 2].trim();
                        edits.push(TextEdit {
                            start: tag_start,
                            end: tag_end + 2,
                            replacement: format!("@foreach ({})", loop_hdr),
                        });
                        count += 1;
                        i = tag_end + 2;
                        continue;
                    }
                }

                if inner == "endforeach;" || inner == "endforeach ;" || inner == "endforeach:" || inner == "endforeach" {
                    edits.push(TextEdit {
                        start: tag_start,
                        end: tag_end + 2,
                        replacement: "@endforeach".to_string(),
                    });
                    count += 1;
                    i = tag_end + 2;
                    continue;
                }

                if inner.starts_with("for") && (inner.len() == 3 || inner.as_bytes()[3] == b' ' || inner.as_bytes()[3] == b'(') {
                    let rest = inner[3..].trim();
                    if rest.starts_with('(') && rest.ends_with(':') {
                        let loop_hdr = rest[1..rest.len() - 2].trim();
                        edits.push(TextEdit {
                            start: tag_start,
                            end: tag_end + 2,
                            replacement: format!("@for ({})", loop_hdr),
                        });
                        count += 1;
                        i = tag_end + 2;
                        continue;
                    }
                }

                if inner == "endfor;" || inner == "endfor ;" || inner == "endfor:" || inner == "endfor" {
                    edits.push(TextEdit {
                        start: tag_start,
                        end: tag_end + 2,
                        replacement: "@endfor".to_string(),
                    });
                    count += 1;
                    i = tag_end + 2;
                    continue;
                }

                if inner.starts_with("while") && (inner.len() == 5 || inner.as_bytes()[5] == b' ' || inner.as_bytes()[5] == b'(') {
                    let rest = inner[5..].trim();
                    if rest.starts_with('(') && rest.ends_with(':') {
                        let cond = rest[..rest.len() - 1].trim();
                        edits.push(TextEdit {
                            start: tag_start,
                            end: tag_end + 2,
                            replacement: format!("@while {}", cond),
                        });
                        count += 1;
                        i = tag_end + 2;
                        continue;
                    }
                }

                if inner == "endwhile;" || inner == "endwhile ;" || inner == "endwhile:" || inner == "endwhile" {
                    edits.push(TextEdit {
                        start: tag_start,
                        end: tag_end + 2,
                        replacement: "@endwhile".to_string(),
                    });
                    count += 1;
                    i = tag_end + 2;
                    continue;
                }

                // 3) include in template: <?php include 'path/to/view.php'; ?>
                if inner.starts_with("include") && (inner.len() == 7 || inner.as_bytes()[7].is_ascii_whitespace() || inner.as_bytes()[7] == b'(') {
                    if let Some(path) = extract_string_literal(inner) {
                        let view_name = sanitize_view_name(&path);
                        edits.push(TextEdit {
                            start: tag_start,
                            end: tag_end + 2,
                            replacement: format!("@include('{}')", view_name),
                        });
                        count += 1;
                        i = tag_end + 2;
                        continue;
                    }
                }
            }
        }

        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));
    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

fn extract_string_literal(s: &str) -> Option<String> {
    for quote in [b'\'', b'"'] {
        if let Some(start) = s.as_bytes().iter().position(|&b| b == quote) {
            if let Some(end) = s.as_bytes()[start + 1..].iter().position(|&b| b == quote) {
                return Some(s[start + 1..start + 1 + end].to_string());
            }
        }
    }
    None
}

fn sanitize_view_name(path: &str) -> String {
    let mut s = path.trim();
    s = s.trim_start_matches('.').trim_start_matches('/');
    if s.starts_with("resources/views/") {
        s = &s["resources/views/".len()..];
    } else if s.starts_with("views/") {
        s = &s["views/".len()..];
    }
    for ext in [".blade.php", ".rade.rhp", ".blade.rhp", ".php", ".html"] {
        if s.ends_with(ext) {
            s = &s[..s.len() - ext.len()];
            break;
        }
    }
    s.replace('/', ".")
}

fn parse_echo_expressions(body: &str) -> Option<Vec<String>> {
    let mut exprs = Vec::new();
    for stmt in body.split(';') {
        let trimmed = stmt.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("echo") && (trimmed.len() == 4 || trimmed.as_bytes()[4].is_ascii_whitespace() || trimmed.as_bytes()[4] == b'(') {
            let rest = trimmed[4..].trim();
            let expr = if rest.starts_with('(') && rest.ends_with(')') {
                rest[1..rest.len() - 1].trim()
            } else {
                rest
            };
            if !expr.is_empty() {
                exprs.push(expr.to_string());
            }
        } else if trimmed.starts_with("print") && (trimmed.len() == 5 || trimmed.as_bytes()[5].is_ascii_whitespace() || trimmed.as_bytes()[5] == b'(') {
            let rest = trimmed[5..].trim();
            let expr = if rest.starts_with('(') && rest.ends_with(')') {
                rest[1..rest.len() - 1].trim()
            } else {
                rest
            };
            if !expr.is_empty() {
                exprs.push(expr.to_string());
            }
        } else {
            return None; // Contains non-echo statement
        }
    }
    if exprs.is_empty() {
        None
    } else {
        Some(exprs)
    }
}

// =========================================================================
// Step 0.0003: Migrate Simple Output Buffering (ob_*)
// =========================================================================

pub fn migrate_simple_output_buffering(source: &str) -> (String, usize, Vec<String>) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut warnings = Vec::new();
    let mut i = 0;

    while i < n {
        if !mask[i] {
            i += 1;
            continue;
        }

        if i > 0 && is_migrate_word_byte(bytes[i - 1]) {
            i += 1;
            continue;
        }

        // Match ob_start
        if i + 8 <= n && source[i..i + 8].eq_ignore_ascii_case("ob_start") {
            let after_ob = i + 8;
            if after_ob < n && is_migrate_word_byte(bytes[after_ob]) {
                i = after_ob;
                continue;
            }

            // Reject if preceded by ->, ::, $
            let mut b_idx = i;
            while b_idx > 0 && (bytes[b_idx - 1] == b' ' || bytes[b_idx - 1] == b'\t' || bytes[b_idx - 1] == b'\r' || bytes[b_idx - 1] == b'\n') {
                b_idx -= 1;
            }
            if b_idx >= 2 && (&bytes[b_idx - 2..b_idx] == b"::" || &bytes[b_idx - 2..b_idx] == b"->") {
                i = after_ob;
                continue;
            }
            if b_idx >= 1 && bytes[b_idx - 1] == b'$' {
                i = after_ob;
                continue;
            }

            // Find '(' and matching ')'
            let mut p = after_ob;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\r' || bytes[p] == b'\n') {
                p += 1;
            }
            if p >= n || bytes[p] != b'(' {
                i = after_ob;
                continue;
            }
            let mut cp = p + 1;
            while cp < n && (bytes[cp] == b' ' || bytes[cp] == b'\t' || bytes[cp] == b'\r' || bytes[cp] == b'\n') {
                cp += 1;
            }
            if cp >= n || bytes[cp] != b')' {
                let line = byte_offset_to_line(source, i);
                warnings.push(format!("Notice: 'ob_start' with callback at line {} requires manual refactoring or AI migration to middleware.", line));
                i = after_ob;
                continue;
            }

            let mut semi = cp + 1;
            while semi < n && (bytes[semi] == b' ' || bytes[semi] == b'\t' || bytes[semi] == b'\r' || bytes[semi] == b'\n') {
                semi += 1;
            }
            if semi >= n || bytes[semi] != b';' {
                i = after_ob;
                continue;
            }
            let ob_start_stmt_end = semi + 1;

            // Scan forward to find matching ob_get_clean() or ob_end_clean()
            let mut scan = ob_start_stmt_end;
            let mut found_match = None;

            while scan < n {
                if !mask[scan] {
                    scan += 1;
                    continue;
                }

                // If another ob_start occurs, nested buffer -> skip
                if scan > 0 && !is_migrate_word_byte(bytes[scan - 1]) && scan + 8 <= n && source[scan..scan + 8].eq_ignore_ascii_case("ob_start") {
                    let after = scan + 8;
                    if after == n || !is_migrate_word_byte(bytes[after]) {
                        break;
                    }
                }

                // Look for: ob_end_clean()
                if (scan == 0 || !is_migrate_word_byte(bytes[scan - 1])) && scan + 12 <= n && source[scan..scan + 12].eq_ignore_ascii_case("ob_end_clean") {
                    let after = scan + 12;
                    if after == n || !is_migrate_word_byte(bytes[after]) {
                        let mut end_p = after;
                        while end_p < n && bytes[end_p] != b';' && bytes[end_p] != b'\n' {
                            end_p += 1;
                        }
                        if end_p < n && bytes[end_p] == b';' {
                            found_match = Some((None, scan, end_p + 1, false));
                            break;
                        }
                    }
                }

                // Look for: $var = ob_get_clean()
                if bytes[scan] == b'$' {
                    let var_start = scan;
                    let mut var_end = scan + 1;
                    while var_end < n && (bytes[var_end].is_ascii_alphanumeric() || bytes[var_end] == b'_') {
                        var_end += 1;
                    }
                    let var_name = &source[var_start..var_end];

                    let mut eq_p = var_end;
                    while eq_p < n && (bytes[eq_p] == b' ' || bytes[eq_p] == b'\t' || bytes[eq_p] == b'\r' || bytes[eq_p] == b'\n') {
                        eq_p += 1;
                    }
                    if eq_p < n && bytes[eq_p] == b'=' {
                        let mut rhs = eq_p + 1;
                        while rhs < n && (bytes[rhs] == b' ' || bytes[rhs] == b'\t' || bytes[rhs] == b'\r' || bytes[rhs] == b'\n') {
                            rhs += 1;
                        }
                        if rhs < n && bytes[rhs] == b'\\' { rhs += 1; }
                        if rhs + 12 <= n && source[rhs..rhs + 12].eq_ignore_ascii_case("ob_get_clean") {
                            let after = rhs + 12;
                            if after == n || !is_migrate_word_byte(bytes[after]) {
                                let mut semi_p = after;
                                while semi_p < n && bytes[semi_p] != b';' && bytes[semi_p] != b'\n' {
                                    semi_p += 1;
                                }
                                if semi_p < n && bytes[semi_p] == b';' {
                                    found_match = Some((Some(var_name.to_string()), var_start, semi_p + 1, true));
                                    break;
                                }
                            }
                        }
                    }
                }

                scan += 1;
            }

            if let Some((target_var, end_stmt_start, end_stmt_end, is_get_clean)) = found_match {
                let inner_body = &source[ob_start_stmt_end..end_stmt_start];
                let trimmed_body = inner_body.trim();

                let line_start = get_line_start(source, i);
                let indent = &source[line_start..i];

                if !is_get_clean {
                    if trimmed_body.is_empty() {
                        let full_end = get_line_end(source, end_stmt_end);
                        edits.push(TextEdit {
                            start: line_start,
                            end: full_end,
                            replacement: String::new(),
                        });
                        count += 1;
                        i = full_end;
                        continue;
                    } else if !trimmed_body.contains("echo") && !trimmed_body.contains("print") {
                        let mut replacement = String::new();
                        replacement.push_str(indent);
                        replacement.push_str(trimmed_body);
                        replacement.push('\n');

                        let full_end = get_line_end(source, end_stmt_end);
                        edits.push(TextEdit {
                            start: line_start,
                            end: full_end,
                            replacement,
                        });
                        count += 1;
                        i = full_end;
                        continue;
                    }
                } else if let Some(var) = target_var {
                    let is_include = trimmed_body.starts_with("include")
                        || trimmed_body.starts_with("require")
                        || trimmed_body.starts_with("include_once")
                        || trimmed_body.starts_with("require_once");

                    if is_include {
                        if let Some(view_path) = extract_string_literal(trimmed_body) {
                            let view_name = sanitize_view_name(&view_path);
                            let replacement = format!("{}{} = View::render('{}');\n", indent, var, view_name);
                            let full_end = get_line_end(source, end_stmt_end);
                            edits.push(TextEdit {
                                start: line_start,
                                end: full_end,
                                replacement,
                            });
                            count += 1;
                            i = full_end;
                            continue;
                        }
                    }

                    if let Some(echo_exprs) = parse_echo_expressions(trimmed_body) {
                        let expr_str = if echo_exprs.len() == 1 {
                            let e = &echo_exprs[0];
                            if e.starts_with('"') || e.starts_with('\'') || e.contains('+') {
                                e.clone()
                            } else {
                                format!("string({})", e)
                            }
                        } else {
                            echo_exprs.join(" + ")
                        };

                        let replacement = format!("{}{} = {};\n", indent, var, expr_str);
                        let full_end = get_line_end(source, end_stmt_end);
                        edits.push(TextEdit {
                            start: line_start,
                            end: full_end,
                            replacement,
                        });
                        count += 1;
                        i = full_end;
                        continue;
                    }
                }
            }

            i = after_ob;
            continue;
        }

        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0, warnings);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));
    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count, warnings)
}

// =========================================================================
// Step 0.0006: Strip Legacy include / require Statements
// =========================================================================

pub fn strip_legacy_includes(source: &str) -> (String, usize) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut count = 0;
    let mut i = 0;

    while i < n {
        if !mask[i] {
            i += 1;
            continue;
        }

        if i > 0 && (is_migrate_word_byte(bytes[i - 1]) || bytes[i - 1] == b'@') {
            i += 1;
            continue;
        }

        let mut kw_len = 0;
        let mut is_kw = false;
        if i + 12 <= n && (source[i..i + 12].eq_ignore_ascii_case("require_once") || source[i..i + 12].eq_ignore_ascii_case("include_once")) {
            kw_len = 12;
            is_kw = true;
        } else if i + 7 <= n && (source[i..i + 7].eq_ignore_ascii_case("require") || source[i..i + 7].eq_ignore_ascii_case("include")) {
            kw_len = 7;
            is_kw = true;
        }

        if !is_kw {
            i += 1;
            continue;
        }

        let after_kw = i + kw_len;
        if after_kw < n && is_migrate_word_byte(bytes[after_kw]) {
            i = after_kw;
            continue;
        }

        // Reject if preceded by ->, ::, or $
        let mut b_idx = i;
        while b_idx > 0 && (bytes[b_idx - 1] == b' ' || bytes[b_idx - 1] == b'\t' || bytes[b_idx - 1] == b'\r' || bytes[b_idx - 1] == b'\n') {
            b_idx -= 1;
        }
        if b_idx >= 2 && (&bytes[b_idx - 2..b_idx] == b"::" || &bytes[b_idx - 2..b_idx] == b"->") {
            i = after_kw;
            continue;
        }
        if b_idx >= 1 && bytes[b_idx - 1] == b'$' {
            i = after_kw;
            continue;
        }

        // Standalone statement check (preceded by ;, {, }, <?php, <?, or start of file)
        let is_standalone = if b_idx == 0 {
            true
        } else {
            let prev_ch = bytes[b_idx - 1];
            prev_ch == b';' || prev_ch == b'{' || prev_ch == b'}' || bytes[..b_idx].ends_with(b"<?php") || bytes[..b_idx].ends_with(b"<?")
        };

        if !is_standalone {
            i = after_kw;
            continue;
        }

        // Scan forward to statement terminator ';' or '?>'
        let mut cur = after_kw;
        let mut depth = 0;
        let mut found_semi = None;

        while cur < n {
            if mask[cur] {
                if bytes[cur] == b'(' || bytes[cur] == b'[' {
                    depth += 1;
                } else if bytes[cur] == b')' || bytes[cur] == b']' {
                    if depth > 0 {
                        depth -= 1;
                    }
                } else if depth == 0 && bytes[cur] == b';' {
                    found_semi = Some(cur);
                    break;
                } else if depth == 0 && cur + 1 < n && bytes[cur..].starts_with(b"?>") {
                    found_semi = Some(cur);
                    break;
                }
            }
            cur += 1;
        }

        if let Some(semi_pos) = found_semi {
            let after_stmt = if bytes[semi_pos] == b';' { semi_pos + 1 } else { semi_pos };

            let line_start = get_line_start(source, i);
            let only_ws_before = bytes[line_start..i].iter().all(|&b| b == b' ' || b == b'\t');

            let mut line_end = after_stmt;
            while line_end < n && (bytes[line_end] == b' ' || bytes[line_end] == b'\t') {
                line_end += 1;
            }
            if (line_end + 1 < n && bytes[line_end] == b'/' && bytes[line_end + 1] == b'/')
                || (line_end < n && bytes[line_end] == b'#')
            {
                while line_end < n && bytes[line_end] != b'\n' {
                    line_end += 1;
                }
            }

            let only_ws_after = line_end == n || bytes[line_end] == b'\r' || bytes[line_end] == b'\n';

            if only_ws_before && only_ws_after {
                let mut full_end = line_end;
                if full_end < n && bytes[full_end] == b'\r' { full_end += 1; }
                if full_end < n && bytes[full_end] == b'\n' { full_end += 1; }

                edits.push(TextEdit {
                    start: line_start,
                    end: full_end,
                    replacement: String::new(),
                });
            } else {
                edits.push(TextEdit {
                    start: i,
                    end: after_stmt,
                    replacement: String::new(),
                });
            }

            count += 1;
            i = after_stmt;
            continue;
        }

        i = after_kw;
    }

    if edits.is_empty() {
        return (source.to_string(), 0);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));
    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count)
}

// =========================================================================
// Step 0.0009: Migrate Controller / Action Echo to Response Returns
// =========================================================================

pub fn migrate_controller_echo(source: &str) -> (String, usize, Vec<String>) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut warnings = Vec::new();
    let mut count = 0;
    let mut i = 0;

    while i < n {
        if !mask[i] {
            i += 1;
            continue;
        }

        if i > 0 && is_migrate_word_byte(bytes[i - 1]) {
            i += 1;
            continue;
        }

        if i + 4 <= n && source[i..i + 4].eq_ignore_ascii_case("echo") {
            let after_echo = i + 4;
            if after_echo < n && is_migrate_word_byte(bytes[after_echo]) {
                i = after_echo;
                continue;
            }

            let mut b_idx = i;
            while b_idx > 0 && (bytes[b_idx - 1] == b' ' || bytes[b_idx - 1] == b'\t' || bytes[b_idx - 1] == b'\r' || bytes[b_idx - 1] == b'\n') {
                b_idx -= 1;
            }
            if b_idx >= 2 && (&bytes[b_idx - 2..b_idx] == b"::" || &bytes[b_idx - 2..b_idx] == b"->") {
                i = after_echo;
                continue;
            }
            if b_idx >= 1 && bytes[b_idx - 1] == b'$' {
                i = after_echo;
                continue;
            }

            let mut p = after_echo;
            while p < n && (bytes[p] == b' ' || bytes[p] == b'\t') {
                p += 1;
            }

            // 1) View::render(...)
            let view_render = if p + 12 <= n && source[p..p + 12].eq_ignore_ascii_case("View::render") {
                Some(12)
            } else if p + 13 <= n && source[p..p + 13].eq_ignore_ascii_case("\\View::render") {
                Some(13)
            } else {
                None
            };

            if let Some(_) = view_render {
                let mut semi = p;
                while semi < n && bytes[semi] != b';' && bytes[semi] != b'\n' {
                    semi += 1;
                }
                if semi < n && bytes[semi] == b';' {
                    let call_expr = source[p..semi].trim();
                    edits.push(TextEdit {
                        start: i,
                        end: semi + 1,
                        replacement: format!("return Response::html({});", call_expr),
                    });
                    count += 1;
                    i = semi + 1;
                    continue;
                }
            }

            // 2) Response::html(...) / Response::json(...)
            let is_response = (p + 10 <= n && source[p..p + 10].eq_ignore_ascii_case("Response::"))
                || (p + 11 <= n && source[p..p + 11].eq_ignore_ascii_case("\\Response::"));

            if is_response {
                edits.push(TextEdit {
                    start: i,
                    end: p,
                    replacement: "return ".to_string(),
                });
                count += 1;
                i = p;
                continue;
            }

            // 3) json_encode(...)
            let is_json = if p + 11 <= n && source[p..p + 11].eq_ignore_ascii_case("json_encode") {
                Some(11)
            } else if p + 12 <= n && source[p..p + 12].eq_ignore_ascii_case("\\json_encode") {
                Some(12)
            } else {
                None
            };

            if let Some(json_len) = is_json {
                let mut cp = p + json_len;
                while cp < n && bytes[cp] != b'(' {
                    cp += 1;
                }
                if cp < n {
                    let mut depth = 0;
                    let mut close_paren = None;
                    let mut cur = cp;
                    while cur < n {
                        if mask[cur] {
                            if bytes[cur] == b'(' {
                                depth += 1;
                            } else if bytes[cur] == b')' {
                                depth -= 1;
                                if depth == 0 {
                                    close_paren = Some(cur);
                                    break;
                                }
                            }
                        }
                        cur += 1;
                    }
                    if let Some(end_paren) = close_paren {
                        let mut semi = end_paren + 1;
                        while semi < n && (bytes[semi] == b' ' || bytes[semi] == b'\t') {
                            semi += 1;
                        }
                        if semi < n && bytes[semi] == b';' {
                            let args = source[cp + 1..end_paren].trim();
                            let parts = split_top_level_args(args);
                            let value = parts.first().map(|s| s.trim()).unwrap_or("");
                            if parts.len() > 1 {
                                warnings.push(format!(
                                    "echo json_encode flags were dropped; Response::json keeps the value only: {args}"
                                ));
                            }
                            edits.push(TextEdit {
                                start: i,
                                end: semi + 1,
                                replacement: format!("return Response::json({value});"),
                            });
                            count += 1;
                            i = semi + 1;
                            continue;
                        }
                    }
                }
            }

            i = after_echo;
            continue;
        }

        i += 1;
    }

    if edits.is_empty() {
        return (source.to_string(), 0, warnings);
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));
    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }

    (result, count, warnings)
}

fn split_top_level_args(args: &str) -> Vec<&str> {
    if args.trim().is_empty() {
        return Vec::new();
    }
    let mut parts = Vec::new();
    let mut start = 0;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (idx, ch) in args.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' && q == '"' {
                escaped = true;
                continue;
            }
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            '[' => bracket += 1,
            ']' => bracket = bracket.saturating_sub(1),
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            ',' if paren == 0 && bracket == 0 && brace == 0 => {
                parts.push(&args[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(&args[start..]);
    parts
}

fn strip_named_arg(arg: &str) -> &str {
    let trimmed = arg.trim();
    let bytes = trimmed.as_bytes();
    if bytes.is_empty() || !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
        return trimmed;
    }
    let mut i = 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    let mut j = i;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    if j < bytes.len() && bytes[j] == b':' && (j + 1 >= bytes.len() || bytes[j + 1] != b':') {
        trimmed[j + 1..].trim()
    } else {
        trimmed
    }
}

const JSON_HEX_TAG: u64 = 1;
const JSON_HEX_AMP: u64 = 2;
const JSON_HEX_APOS: u64 = 4;
const JSON_HEX_QUOT: u64 = 8;
const JSON_FORCE_OBJECT: u64 = 16;
const JSON_NUMERIC_CHECK: u64 = 32;
const JSON_UNESCAPED_SLASHES: u64 = 64;
const JSON_PRETTY_PRINT: u64 = 128;
const JSON_UNESCAPED_UNICODE: u64 = 256;
const JSON_PARTIAL_OUTPUT_ON_ERROR: u64 = 512;
const JSON_PRESERVE_ZERO_FRACTION: u64 = 1024;
const JSON_UNESCAPED_LINE_TERMINATORS: u64 = 2048;
const JSON_INVALID_UTF8_IGNORE: u64 = 1_048_576;
const JSON_INVALID_UTF8_SUBSTITUTE: u64 = 2_097_152;
const JSON_THROW_ON_ERROR: u64 = 4_194_304;
const JSON_HTML_FLAGS: u64 = JSON_HEX_TAG | JSON_HEX_AMP | JSON_HEX_APOS | JSON_HEX_QUOT;

fn migrate_json_calls(source: &str) -> (String, Vec<String>) {
    let mask = build_code_mask(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let names = [
        "json_last_error_msg",
        "json_last_error",
        "json_validate",
        "json_encode",
        "json_decode",
    ];
    let mut edits: Vec<TextEdit> = Vec::new();
    let mut warnings = Vec::new();
    let mut i = 0;
    while i < n {
        if !mask[i] || (i > 0 && is_migrate_word_byte(bytes[i - 1])) {
            i += 1;
            continue;
        }
        let mut matched = None;
        for name in names {
            let len = name.len();
            if i + len <= n
                && source[i..i + len].eq_ignore_ascii_case(name)
                && (i + len == n || !is_migrate_word_byte(bytes[i + len]))
            {
                matched = Some((name, len));
                break;
            }
        }
        let Some((name, len)) = matched else {
            i += 1;
            continue;
        };
        let mut start = i;
        if start > 0 && bytes[start - 1] == b'\\' {
            start -= 1;
        }
        if name == "json_last_error" || name == "json_last_error_msg" {
            warnings.push(format!("{name}() has no Json facade equivalent"));
            i += len;
            continue;
        }
        let mut p = i + len;
        while p < n && (bytes[p] == b' ' || bytes[p] == b'\t') {
            p += 1;
        }
        if p >= n || bytes[p] != b'(' {
            i += len;
            continue;
        }
        let mut depth = 0i32;
        let mut end = None;
        let mut cur = p;
        while cur < n {
            if mask[cur] {
                if bytes[cur] == b'(' {
                    depth += 1;
                } else if bytes[cur] == b')' {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(cur);
                        break;
                    }
                }
            }
            cur += 1;
        }
        let Some(end_paren) = end else {
            i += len;
            continue;
        };
        let args = split_top_level_args(&source[p + 1..end_paren]);
        let replacement = match name {
            "json_encode" => rewrite_json_encode(&args, &mut warnings),
            "json_decode" => rewrite_json_decode(&args, &mut warnings),
            "json_validate" => rewrite_json_validate(&args, &mut warnings),
            _ => None,
        };
        if let Some(replacement) = replacement {
            edits.push(TextEdit {
                start,
                end: end_paren + 1,
                replacement,
            });
        }
        i = end_paren + 1;
    }

    if edits.is_empty() {
        return (source.to_string(), warnings);
    }
    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));
    let mut result = source.to_string();
    for edit in edits {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }
    (result, warnings)
}

fn rewrite_json_encode(args: &[&str], warnings: &mut Vec<String>) -> Option<String> {
    let value = strip_named_arg(args.first().copied().unwrap_or(""));
    if args.len() <= 1 {
        return Some(format!("Json::encode({value})"));
    }
    let flags_expr = strip_named_arg(args[1]);
    let depth_expr = args.get(2).map(|arg| strip_named_arg(arg));
    let Some(bits) = parse_json_flag_expr(flags_expr) else {
        warnings.push(format!(
            "json_encode flags were not a constant expression and were dropped: {flags_expr}"
        ));
        return Some(format!("Json::encode({value})"));
    };
    let mut depth = None;
    if let Some(expr) = depth_expr {
        match expr.parse::<i64>() {
            Ok(n) => depth = Some(n),
            Err(_) => warnings.push(format!(
                "json_encode depth was not an integer literal and was dropped: {expr}"
            )),
        }
    }
    Some(format!(
        "Json::encode({value}{})",
        json_options_dict(bits, depth, flags_expr, warnings)
    ))
}

fn rewrite_json_decode(args: &[&str], warnings: &mut Vec<String>) -> Option<String> {
    let json = strip_named_arg(args.first().copied().unwrap_or(""));
    if let Some(second) = args.get(1) {
        let assoc = strip_named_arg(second);
        let associative = matches!(
            assoc,
            "true" | "TRUE" | "True" | "1" | "JSON_OBJECT_AS_ARRAY" | "\\JSON_OBJECT_AS_ARRAY"
        );
        let explicit_false = matches!(assoc, "false" | "FALSE" | "False" | "0");
        if explicit_false {
            warnings.push(
                "json_decode associative false has no object mode; migrated to Json::decode"
                    .to_string(),
            );
        } else if !associative {
            warnings.push(format!(
                "json_decode associative argument was dropped: {assoc}"
            ));
        }
    }
    if args.len() > 2 {
        warnings.push("json_decode depth and flags were dropped".to_string());
    }
    Some(format!("Json::decode({json})"))
}

fn rewrite_json_validate(args: &[&str], warnings: &mut Vec<String>) -> Option<String> {
    let json = strip_named_arg(args.first().copied().unwrap_or(""));
    if args.len() > 1 {
        warnings.push("json_validate extra arguments were dropped".to_string());
    }
    Some(format!("Json::validate({json})"))
}

fn parse_json_flag_expr(expr: &str) -> Option<u64> {
    let mut bits = 0u64;
    let mut current = String::new();
    let mut paren = 0i32;
    for ch in expr.chars() {
        match ch {
            '(' => {
                paren += 1;
                current.push(ch);
            }
            ')' => {
                paren = paren.saturating_sub(1);
                current.push(ch);
            }
            '|' if paren == 0 => {
                bits |= json_flag_token(current.trim())?;
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        bits |= json_flag_token(current.trim())?;
    }
    Some(bits)
}

fn json_flag_token(token: &str) -> Option<u64> {
    let name = token.trim().trim_start_matches('\\');
    if let Ok(n) = name.parse::<u64>() {
        return Some(n);
    }
    Some(match name {
        "JSON_HEX_TAG" => JSON_HEX_TAG,
        "JSON_HEX_AMP" => JSON_HEX_AMP,
        "JSON_HEX_APOS" => JSON_HEX_APOS,
        "JSON_HEX_QUOT" => JSON_HEX_QUOT,
        "JSON_FORCE_OBJECT" => JSON_FORCE_OBJECT,
        "JSON_NUMERIC_CHECK" => JSON_NUMERIC_CHECK,
        "JSON_UNESCAPED_SLASHES" => JSON_UNESCAPED_SLASHES,
        "JSON_PRETTY_PRINT" => JSON_PRETTY_PRINT,
        "JSON_UNESCAPED_UNICODE" => JSON_UNESCAPED_UNICODE,
        "JSON_PARTIAL_OUTPUT_ON_ERROR" => JSON_PARTIAL_OUTPUT_ON_ERROR,
        "JSON_PRESERVE_ZERO_FRACTION" => JSON_PRESERVE_ZERO_FRACTION,
        "JSON_UNESCAPED_LINE_TERMINATORS" => JSON_UNESCAPED_LINE_TERMINATORS,
        "JSON_INVALID_UTF8_IGNORE" => JSON_INVALID_UTF8_IGNORE,
        "JSON_INVALID_UTF8_SUBSTITUTE" => JSON_INVALID_UTF8_SUBSTITUTE,
        "JSON_THROW_ON_ERROR" => JSON_THROW_ON_ERROR,
        _ => return None,
    })
}

fn json_options_dict(
    bits: u64,
    depth: Option<i64>,
    original: &str,
    warnings: &mut Vec<String>,
) -> String {
    let silent = JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR;
    let warned = JSON_PARTIAL_OUTPUT_ON_ERROR
        | JSON_INVALID_UTF8_IGNORE
        | JSON_INVALID_UTF8_SUBSTITUTE
        | JSON_UNESCAPED_LINE_TERMINATORS;
    let known = JSON_HTML_FLAGS
        | JSON_FORCE_OBJECT
        | JSON_NUMERIC_CHECK
        | JSON_PRETTY_PRINT
        | JSON_PRESERVE_ZERO_FRACTION
        | silent
        | warned;
    if bits & warned != 0 {
        warnings.push(format!(
            "json_encode flags have no options-dict key and were dropped: {original}"
        ));
    }
    if bits & !known != 0 {
        warnings.push(format!(
            "json_encode flags contained unknown bits and were dropped: {original}"
        ));
    }
    let mut pairs = Vec::new();
    if bits & JSON_PRETTY_PRINT != 0 {
        pairs.push("\"pretty\" => true".to_string());
    }
    if bits & JSON_FORCE_OBJECT != 0 {
        pairs.push("\"forceObject\" => true".to_string());
    }
    if bits & JSON_PRESERVE_ZERO_FRACTION != 0 {
        pairs.push("\"preserveZeroFraction\" => true".to_string());
    }
    if bits & JSON_NUMERIC_CHECK != 0 {
        pairs.push("\"numericCheck\" => true".to_string());
    }
    if bits & JSON_HTML_FLAGS != 0 {
        pairs.push("\"html\" => true".to_string());
    }
    if let Some(depth) = depth {
        if depth != 512 {
            pairs.push(format!("\"depth\" => {depth}"));
        }
    }
    if pairs.is_empty() {
        String::new()
    } else {
        format!(", [{}]", pairs.join(", "))
    }
}



