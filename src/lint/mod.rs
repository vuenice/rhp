pub mod auto_assign;
pub mod autotype;
pub mod casing;
pub mod diagnostics;
pub mod formatter;
pub mod migrate;
pub use migrate as upgrade_php;
pub mod param_order;
pub mod ref_params;
pub mod reporter;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::diagnostic::{check_source, DEFAULT_MAX_ERRORS_PER_FILE};
use crate::lint::migrate::{
    compute_dependency_order_from_paths_and_content, emit_stream_event, format_relative_file_path,
};

pub use auto_assign::auto_assign_collection_calls;
pub use autotype::add_variable_types;
pub use casing::{
    apply_casing, check_casing, is_camel_case, is_pascal_case, is_snake_case, is_special_var,
    to_camel_case, to_pascal_case, to_snake_case,
};
pub use diagnostics::{run_typecheck, type_error_to_code};
pub use formatter::{format_source, split_statements_on_semicolons};
pub use migrate::{
    comment_ini_set_calls, migrate_file, migrate_named_arguments, migrate_path, migrate_source,
    migrate_source_with_options, migrate_source_with_options_and_context,
    remove_error_suppression, remove_php_tags, replace_get_array_access, replace_list_destructuring,
    replace_post_array_access, replace_type_casts, replace_var_dump_and_export,
    strip_declare_statements, strip_error_reporting, strip_legacy_includes,
    replace_legacy_includes_with_use_and_namespace, resolve_file_psr4_namespace, find_project_root_and_psr4,
    migrate_simple_output_buffering, migrate_controller_echo, transpile_php_template_to_rade,
    swap_positional_calls_and_warn_tricky, migrate_pdo_queries,
    upgrade_php_file, upgrade_php_path, upgrade_php_source, MigrateFileResult, MigrateOptions,
    MigrateReport, UpgradeFileResult, UpgradeOptions, UpgradeReport,
};
pub use param_order::{
    check_parameter_order, fix_parameter_order, fix_parameter_order_detailed,
    fix_parameter_order_with_count, has_required_after_optional, ReorderedFunctionInfo,
};
pub use ref_params::convert_mutated_array_params_to_ref;
pub use reporter::{Issue, IssueLevel, IssueSpan, LintReport};

/// Lint configuration options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LintOptions {
    /// Whether to write fixes back to disk. When false (--no-fix or --check), acts as dry-run.
    pub fix: bool,
    /// Whether to perform casing conversions (snake_case variables, PascalCase classes, camelCase functions).
    /// When false (--no-case-change), skips identifier renaming and casing diagnostics.
    pub case_change: bool,
    /// Whether to emit real-time NDJSON stream events to stdout. Default is false.
    pub stream: bool,
}

impl Default for LintOptions {
    fn default() -> Self {
        Self {
            fix: true,
            case_change: true,
            stream: false,
        }
    }
}

/// Individual file linting result.
#[derive(Debug, Clone)]
pub struct LintResult {
    pub file_name: String,
    pub original_content: String,
    pub formatted_content: String,
    pub modified: bool,
    pub issues: Vec<Issue>,
}

/// Lint in-memory source code with default options (fix enabled, case change enabled):
/// 1. Auto-apply casing fixes (snake_case variables, PascalCase classes, camelCase functions/test_ in snake_case).
/// 2. Auto-apply types before untyped variables (`$x = '';` -> `string $x = '';`, `$y = 1;` -> `int $y = 1;`).
/// 3. Auto-apply indentation and formatting (4-space indent, operator spacing, braces).
/// 4. Run typecheck on the resulting code and collect issues to be resolved.
pub fn lint_source(source: &str, file_name: &str) -> LintResult {
    lint_source_with_options(source, file_name, LintOptions::default())
}

/// Lint in-memory source code with custom options.
pub fn lint_source_with_options(source: &str, file_name: &str, options: LintOptions) -> LintResult {
    if options.fix {
        // Fix mode:
        // 0. Auto-convert strict comparison operators `===` and `!==` to `==` and `!=`
        let eq_fixed_code = replace_strict_equality_operators(source);

        // 1. Separate statements with semicolon onto different lines
        let split_stmts_code = split_statements_on_semicolons(&eq_fixed_code);

        // 2. Auto-assign collection transformations ($x->map(); -> $x = $x->map();)
        let auto_assigned_code = auto_assign_collection_calls(&split_stmts_code);

        // 3. Convert mutated array parameters to pass-by-reference (&$param)
        let ref_param_code = convert_mutated_array_params_to_ref(&auto_assigned_code);

        // 4. Reorder function parameters so required arguments always come before optional
        let param_ordered_code = fix_parameter_order(&ref_param_code);

        // 5. Casing transformations (if enabled)
        let (cased_code, _) = if options.case_change {
            apply_casing(&param_ordered_code, file_name)
        } else {
            (param_ordered_code, Vec::new())
        };

        // 6. Infer and add variable types
        let typed_code = add_variable_types(&cased_code);

        // 7. Format indentation, operators, and braces
        let formatted_code = format_source(&typed_code);
        let modified = formatted_code != source;

        // 8. Run typecheck on resulting code and collect issues
        let issues = run_typecheck(&formatted_code, file_name);

        LintResult {
            file_name: file_name.to_string(),
            original_content: source.to_string(),
            formatted_content: formatted_code,
            modified,
            issues,
        }
    } else {
        // Check only (--no-fix or --check):
        let mut issues = Vec::new();

        // 1. Check parameter ordering (required arguments must precede optional arguments)
        issues.extend(check_parameter_order(source, file_name));

        // 2. Check casing issues (if enabled)
        if options.case_change {
            issues.extend(check_casing(source, file_name));
        }

        // 2. Check formatting issues
        let formatted_code = format_source(source);
        if formatted_code != source {
            issues.push(Issue::new(
                IssueLevel::Warning,
                "unformatted-code",
                "File is not formatted according to standard PER-CS style (indentation/spacing).",
                file_name,
                1,
                1,
                0,
                0,
            ));
        }

        // 3. Check typecheck issues
        issues.extend(run_typecheck(source, file_name));

        LintResult {
            file_name: file_name.to_string(),
            original_content: source.to_string(),
            formatted_content: formatted_code,
            modified: false,
            issues,
        }
    }
}

/// Lint a single file on disk. If `options.fix` is true and changes were made, updates the file.
pub fn lint_file(path: &Path, options: LintOptions) -> io::Result<LintResult> {
    let source = fs::read_to_string(path)?;
    let file_name = path.display().to_string();
    let result = lint_source_with_options(&source, &file_name, options);

    if result.modified && options.fix {
        fs::write(path, &result.formatted_content)?;
    }

    Ok(result)
}

/// Lint a path (either a single file or a directory recursively).
pub fn lint_path(target: &Path, options: LintOptions) -> io::Result<LintReport> {
    let mut report = LintReport::new();

    if target.is_file() {
        if options.stream {
            emit_stream_event(&serde_json::json!({
                "phase": "project_regex",
                "event": "start",
                "total_files": 1
            }));
        }

        let res = lint_file(target, options)?;
        report.files_scanned += 1;
        if res.modified {
            report.files_modified += 1;
        }
        for issue in &res.issues {
            report.add_issue(issue.clone());
        }

        if options.stream {
            let rel_path = format_relative_file_path(target, target.parent().unwrap_or(Path::new(".")));
            if res.modified {
                emit_stream_event(&serde_json::json!({
                    "phase": "project_regex",
                    "event": "file_modified",
                    "file": rel_path,
                    "changes_count": 1
                }));
            }
            let modified_count = if res.modified { 1 } else { 0 };
            emit_stream_event(&serde_json::json!({
                "phase": "project_regex",
                "event": "complete",
                "files_modified": modified_count,
                "modified_count": modified_count
            }));

            // Phase 2
            emit_stream_event(&serde_json::json!({
                "phase": "file_verify",
                "event": "file_start",
                "file": rel_path,
                "index": 1,
                "total": 1
            }));

            let diags = check_source(&res.formatted_content, &rel_path, DEFAULT_MAX_ERRORS_PER_FILE);

            for diag in &diags {
                emit_stream_event(&serde_json::json!({
                    "phase": "file_verify",
                    "event": "diagnostic",
                    "file": rel_path,
                    "level": diag.level.as_str(),
                    "code": diag.code,
                    "message": diag.message,
                    "line": diag.span.start.line,
                    "col": diag.span.start.col,
                    "diagnostic": diag
                }));
            }

            if diags.is_empty() {
                for issue in &res.issues {
                    emit_stream_event(&serde_json::json!({
                        "phase": "file_verify",
                        "event": "diagnostic",
                        "file": rel_path,
                        "level": issue.level.as_str(),
                        "code": issue.code,
                        "message": issue.message,
                        "line": issue.line,
                        "col": issue.column
                    }));
                }
            }

            let has_warnings = !diags.is_empty()
                || (!options.fix && !res.issues.is_empty())
                || res.issues.iter().any(|i| i.level == IssueLevel::Error);
            let status = if has_warnings { "warning" } else { "migrated" };

            emit_stream_event(&serde_json::json!({
                "phase": "file_verify",
                "event": "file_complete",
                "file": rel_path,
                "index": 1,
                "total": 1,
                "status": status,
                "diagnostics": diags
            }));

            emit_stream_event(&serde_json::json!({
                "phase": "all_done",
                "event": "complete",
                "summary": {
                    "migrated": if status == "migrated" { 1 } else { 0 },
                    "warnings": if status == "warning" { 1 } else { 0 }
                }
            }));
        }

        return Ok(report);
    }

    if target.is_dir() {
        if options.stream {
            lint_project_stream(target, options, &mut report)?;
        } else {
            lint_directory_recursive(target, options, &mut report)?;
        }
    }

    Ok(report)
}

struct LintItem {
    path: PathBuf,
    rel_path: String,
    formatted_content: String,
    issues: Vec<Issue>,
}

fn collect_lint_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
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
            collect_lint_files_recursive(&path, files)?;
        } else if is_rhp_or_php_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn lint_project_stream(dir: &Path, options: LintOptions, report: &mut LintReport) -> io::Result<()> {
    let mut file_paths = Vec::new();
    collect_lint_files_recursive(dir, &mut file_paths)?;
    file_paths.sort();

    let total_files = file_paths.len();
    emit_stream_event(&serde_json::json!({
        "phase": "project_regex",
        "event": "start",
        "total_files": total_files
    }));

    let mut items: Vec<LintItem> = Vec::with_capacity(total_files);

    for path in file_paths {
        let rel_path = format_relative_file_path(&path, dir);
        let original_content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("[I/O Error] Could not read '{}': {}. Skipping.", path.display(), e);
                continue;
            }
        };

        let file_name = path.display().to_string();
        let res = lint_source_with_options(&original_content, &file_name, options);

        report.files_scanned += 1;
        if res.modified {
            report.files_modified += 1;
            if options.fix {
                if let Err(e) = fs::write(&path, &res.formatted_content) {
                    eprintln!("[I/O Error] Could not write '{}': {}. Skipping save.", path.display(), e);
                }
            }
            emit_stream_event(&serde_json::json!({
                "phase": "project_regex",
                "event": "file_modified",
                "file": rel_path,
                "changes_count": 1
            }));
        }

        for issue in &res.issues {
            report.add_issue(issue.clone());
        }

        items.push(LintItem {
            path,
            rel_path,
            formatted_content: res.formatted_content,
            issues: res.issues,
        });
    }

    emit_stream_event(&serde_json::json!({
        "phase": "project_regex",
        "event": "complete",
        "files_modified": report.files_modified,
        "modified_count": report.files_modified
    }));

    // Phase 2: File-by-File Type Resolution & Compiler Verification in Dependency Order
    let file_refs: Vec<(&Path, &str)> = items
        .iter()
        .map(|it| (it.path.as_path(), it.formatted_content.as_str()))
        .collect();
    let order = compute_dependency_order_from_paths_and_content(&file_refs, dir);

    let total = items.len();
    let mut migrated_count = 0;
    let mut warning_count = 0;

    for (idx, &item_idx) in order.iter().enumerate() {
        let item = &items[item_idx];
        let index = idx + 1;

        emit_stream_event(&serde_json::json!({
            "phase": "file_verify",
            "event": "file_start",
            "file": item.rel_path,
            "index": index,
            "total": total
        }));

        let diags = check_source(&item.formatted_content, &item.rel_path, DEFAULT_MAX_ERRORS_PER_FILE);

        for diag in &diags {
            emit_stream_event(&serde_json::json!({
                "phase": "file_verify",
                "event": "diagnostic",
                "file": item.rel_path,
                "level": diag.level.as_str(),
                "code": diag.code,
                "message": diag.message,
                "line": diag.span.start.line,
                "col": diag.span.start.col,
                "diagnostic": diag
            }));
        }

        if diags.is_empty() {
            for issue in &item.issues {
                emit_stream_event(&serde_json::json!({
                    "phase": "file_verify",
                    "event": "diagnostic",
                    "file": item.rel_path,
                    "level": issue.level.as_str(),
                    "code": issue.code,
                    "message": issue.message,
                    "line": issue.line,
                    "col": issue.column
                }));
            }
        }

        let has_issues = !diags.is_empty()
            || (!options.fix && !item.issues.is_empty())
            || item.issues.iter().any(|i| i.level == IssueLevel::Error);

        let status = if has_issues {
            warning_count += 1;
            "warning"
        } else {
            migrated_count += 1;
            "migrated"
        };

        emit_stream_event(&serde_json::json!({
            "phase": "file_verify",
            "event": "file_complete",
            "file": item.rel_path,
            "index": index,
            "total": total,
            "status": status,
            "diagnostics": diags
        }));
    }

    emit_stream_event(&serde_json::json!({
        "phase": "all_done",
        "event": "complete",
        "summary": {
            "migrated": migrated_count,
            "warnings": warning_count
        }
    }));

    Ok(())
}

fn lint_directory_recursive(dir: &Path, options: LintOptions, report: &mut LintReport) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip ignored directories
        if path.is_dir() {
            if name == "target" || name == ".git" || name == "vendor" || name == "node_modules" {
                continue;
            }
            lint_directory_recursive(&path, options, report)?;
        } else if is_rhp_or_php_file(&path) {
            let res = lint_file(&path, options)?;
            report.files_scanned += 1;
            if res.modified {
                report.files_modified += 1;
            }
            for issue in res.issues {
                report.add_issue(issue);
            }
        }
    }

    Ok(())
}

fn is_rhp_or_php_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    if path_str.ends_with(".blade.php") || path_str.ends_with(".rade.rhp") {
        return true;
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(ext, "rhp" | "php" | "phtml" | "blade" | "rade")
    } else {
        false
    }
}

/// Replaces strict comparison operators `===` and `!==` with `==` and `!=`
/// outside string literals and comments.
pub fn replace_strict_equality_operators(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let chars: Vec<char> = source.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut in_str = false;
    let mut str_delim = '"';

    while i < n {
        let c = chars[i];

        if in_str {
            output.push(c);
            if c == '\\' && i + 1 < n {
                output.push(chars[i + 1]);
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
            output.push(c);
            i += 1;
            continue;
        }

        // Single-line comments: preserve until newline
        if (c == '/' && i + 1 < n && chars[i + 1] == '/') || c == '#' {
            while i < n && chars[i] != '\n' {
                output.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // Multi-line comments: preserve until */
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

        // Check for ===
        if c == '=' && i + 2 < n && chars[i + 1] == '=' && chars[i + 2] == '=' {
            output.push_str("==");
            i += 3;
            continue;
        }

        // Check for !==
        if c == '!' && i + 2 < n && chars[i + 1] == '=' && chars[i + 2] == '=' {
            output.push_str("!=");
            i += 3;
            continue;
        }

        output.push(c);
        i += 1;
    }

    output
}
