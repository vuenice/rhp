pub mod autotype;
pub mod casing;
pub mod diagnostics;
pub mod formatter;
pub mod reporter;

use std::fs;
use std::io;
use std::path::Path;

pub use autotype::add_variable_types;
pub use casing::{
    apply_casing, check_casing, is_camel_case, is_pascal_case, is_snake_case, is_special_var,
    to_camel_case, to_pascal_case, to_snake_case,
};
pub use diagnostics::{run_typecheck, type_error_to_code};
pub use formatter::format_source;
pub use reporter::{Issue, IssueLevel, IssueSpan, LintReport};

/// Lint configuration options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LintOptions {
    /// Whether to write fixes back to disk. When false (--no-fix or --check), acts as dry-run.
    pub fix: bool,
    /// Whether to perform casing conversions (snake_case variables, PascalCase classes, camelCase functions).
    /// When false (--no-case-change), skips identifier renaming and casing diagnostics.
    pub case_change: bool,
}

impl Default for LintOptions {
    fn default() -> Self {
        Self {
            fix: true,
            case_change: true,
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
        // 1. Casing transformations (if enabled)
        let (cased_code, _) = if options.case_change {
            apply_casing(source, file_name)
        } else {
            (source.to_string(), Vec::new())
        };

        // 2. Infer and add variable types
        let typed_code = add_variable_types(&cased_code);

        // 3. Format indentation, operators, and braces
        let formatted_code = format_source(&typed_code);
        let modified = formatted_code != source;

        // 4. Run typecheck on resulting code and collect issues
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

        // 1. Check casing issues (if enabled)
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
        let res = lint_file(target, options)?;
        report.files_scanned += 1;
        if res.modified {
            report.files_modified += 1;
        }
        for issue in res.issues {
            report.add_issue(issue);
        }
        return Ok(report);
    }

    if target.is_dir() {
        lint_directory_recursive(target, options, &mut report)?;
    }

    Ok(report)
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
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        ext == "rhp" || ext == "php"
    } else {
        false
    }
}
