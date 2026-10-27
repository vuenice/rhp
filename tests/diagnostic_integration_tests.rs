use std::fs;
use std::process::Command;

use rhp::diagnostic::{
    check_source, Applicability, DiagnosticLevel,
};

#[test]
fn test_diagnostic_source_of_truth_ndjson_and_schema() {
    let bad_code = r#"fn total($a, $b) {
    return add($a, );
}
"#;

    let diags = check_source(bad_code, "src/user.rhp", 5);
    assert_eq!(diags.len(), 1);

    let d = &diags[0];
    assert_eq!(d.level, DiagnosticLevel::Error);
    assert_eq!(d.code, "RHP0102");
    assert_eq!(d.message, "unexpected token `)`, expected expression");
    assert_eq!(d.span.file, "src/user.rhp");
    assert_eq!(d.span.start.line, 2);
    assert_eq!(d.found.as_deref(), Some(")"));

    // Expected tokens check
    let expected = d.expected.as_ref().expect("expected tokens present");
    assert!(expected.contains(&"`(`".to_string()) || expected.contains(&"identifier".to_string()));

    // Labels check: primary and secondary open delimiter
    assert_eq!(d.labels.len(), 2);
    assert!(d.labels[0].primary);
    assert_eq!(d.labels[0].text, "expected expression here");
    assert!(!d.labels[1].primary);
    assert!(d.labels[1].text.contains("this `(` is still open"));

    // Snippet check: ~2 lines of context with marker
    assert!(!d.snippet.is_empty());
    let err_line = d.snippet.iter().find(|s| s.line == 2).expect("error line present");
    assert!(err_line.text.contains("return add($a, );"));
    let marker = err_line.marker.as_ref().expect("marker present");
    assert_eq!(marker.len, 1);

    // Suggestions check: machine_applicable trailing comma removal
    assert_eq!(d.suggestions.len(), 1);
    let sug = &d.suggestions[0];
    assert_eq!(sug.message, "remove the trailing comma");
    assert_eq!(sug.applicability, Applicability::MachineApplicable);
    assert_eq!(sug.edits.len(), 1);
    assert_eq!(sug.edits[0].replacement, "");

    // Explain check
    assert_eq!(d.explain, "rhp explain RHP0102");

    // Rendered check
    let rendered = d.rendered.as_ref().expect("rendered string present");
    assert!(rendered.contains("error[RHP0102]: unexpected token `)`, expected expression"));
    assert!(rendered.contains("--> src/user.rhp:2:"));
    assert!(rendered.contains("help: remove the trailing comma"));

    // Verify NDJSON serialization
    let json_str = d.to_ndjson();
    assert!(!json_str.contains('\n'), "NDJSON must be a single line");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("Valid JSON");
    assert_eq!(parsed["code"], "RHP0102");
    assert_eq!(parsed["level"], "error");
    assert!(parsed.get("rendered").is_some());
    assert!(parsed.get("labels").is_some());
}

#[test]
fn test_diagnostic_json_short_format_drops_rendered_and_labels() {
    let bad_code = r#"fn total($a, $b) {
    return add($a, );
}
"#;

    let diags = check_source(bad_code, "src/user.rhp", 5);
    let short_d = diags[0].to_short();
    let json_str = short_d.to_ndjson();

    assert!(!json_str.contains('\n'), "NDJSON must be a single line");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("Valid JSON");

    assert_eq!(parsed["code"], "RHP0102");
    assert!(parsed.get("rendered").is_none(), "json-short must drop 'rendered'");
    assert!(parsed.get("labels").is_none(), "json-short must drop 'labels'");
    assert!(parsed.get("snippet").is_some(), "snippet remains for agent context");
    assert!(parsed.get("suggestions").is_some(), "suggestions remain for fixes");
}

#[test]
fn test_diagnostic_human_format_rust_elm_style() {
    let bad_code = r#"fn total($a, $b) {
    return add($a, );
}
"#;

    let diags = check_source(bad_code, "src/user.rhp", 5);
    let rendered = diags[0].render_human();

    assert!(rendered.contains("error[RHP0102]: unexpected token `)`, expected expression"));
    assert!(rendered.contains("--> src/user.rhp:2:"));
    assert!(rendered.contains("return add($a, );"));
    assert!(rendered.contains("expected expression here"));
    assert!(rendered.contains("this `(` is still open"));
    assert!(rendered.contains("help: remove the trailing comma"));
}

#[test]
fn test_cli_rhp_check_ndjson_stream_with_summary() {
    let temp_dir = std::env::temp_dir().join("rhp_diag_test_check");
    let _ = fs::create_dir_all(&temp_dir);
    let temp_file = temp_dir.join("calc.rhp");

    let bad_code = "fn total($a, $b) {\n    return add($a, );\n}\n";
    fs::write(&temp_file, bad_code).expect("write temp file");

    let exe_path = env!("CARGO_BIN_EXE_rhp");
    let output = Command::new(exe_path)
        .arg("check")
        .arg(temp_file.to_str().unwrap())
        .arg("--format=json")
        .output()
        .expect("run rhp check");

    assert_eq!(output.status.code(), Some(1), "Code errors must exit with code 1");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();

    // Must be exactly 2 NDJSON lines: 1 diagnostic line + 1 summary line
    assert_eq!(lines.len(), 2, "Expected 1 diagnostic + 1 summary line, got: {}", stdout);

    let diag_json: serde_json::Value = serde_json::from_str(lines[0]).expect("first line valid JSON");
    assert_eq!(diag_json["code"], "RHP0102");
    assert_eq!(diag_json["level"], "error");

    let summary_json: serde_json::Value = serde_json::from_str(lines[1]).expect("second line valid JSON");
    assert_eq!(summary_json["level"], "summary");
    assert_eq!(summary_json["errors"], 1);
    assert_eq!(summary_json["warnings"], 0);
    assert_eq!(summary_json["status"], "error");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_rhp_check_json_short_format() {
    let temp_dir = std::env::temp_dir().join("rhp_diag_test_short");
    let _ = fs::create_dir_all(&temp_dir);
    let temp_file = temp_dir.join("calc.rhp");

    let bad_code = "fn total($a, $b) {\n    return add($a, );\n}\n";
    fs::write(&temp_file, bad_code).expect("write temp file");

    let exe_path = env!("CARGO_BIN_EXE_rhp");
    let output = Command::new(exe_path)
        .arg("check")
        .arg(temp_file.to_str().unwrap())
        .arg("--format=json-short")
        .output()
        .expect("run rhp check --format=json-short");

    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2);

    let diag_json: serde_json::Value = serde_json::from_str(lines[0]).expect("valid JSON");
    assert_eq!(diag_json["code"], "RHP0102");
    assert!(diag_json.get("rendered").is_none());
    assert!(diag_json.get("labels").is_none());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_rhp_check_clean_file_exits_zero() {
    let temp_dir = std::env::temp_dir().join("rhp_diag_test_clean");
    let _ = fs::create_dir_all(&temp_dir);
    let temp_file = temp_dir.join("clean.rhp");

    let valid_code = "function add(int $a, int $b): int {\n    return $a + $b;\n}\n";
    fs::write(&temp_file, valid_code).expect("write temp file");

    let exe_path = env!("CARGO_BIN_EXE_rhp");
    let output = Command::new(exe_path)
        .arg("check")
        .arg(temp_file.to_str().unwrap())
        .arg("--format=json")
        .output()
        .expect("run rhp check");

    assert_eq!(output.status.code(), Some(0), "Clean file must exit with code 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 1);

    let summary_json: serde_json::Value = serde_json::from_str(lines[0]).expect("summary line valid JSON");
    assert_eq!(summary_json["level"], "summary");
    assert_eq!(summary_json["errors"], 0);
    assert_eq!(summary_json["status"], "ok");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_rhp_explain_command() {
    let exe_path = env!("CARGO_BIN_EXE_rhp");

    // Human format explain
    let output = Command::new(exe_path)
        .arg("explain")
        .arg("RHP0102")
        .output()
        .expect("run rhp explain");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("RHP0102: Unexpected token"));
    assert!(stdout.contains("Common Causes:"));
    assert!(stdout.contains("How to fix:"));

    // JSON format explain
    let output_json = Command::new(exe_path)
        .arg("explain")
        .arg("RHP0102")
        .arg("--format=json")
        .output()
        .expect("run rhp explain json");

    assert!(output_json.status.success());
    let stdout_json = String::from_utf8_lossy(&output_json.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout_json).expect("valid JSON");
    assert_eq!(parsed["code"], "RHP0102");
    assert_eq!(parsed["title"], "Unexpected token");
    assert!(parsed.get("explanation").is_some());
}

#[test]
fn test_internal_error_severity_tier_and_message() {
    let d = rhp::diagnostic::DiagnosticBuilder::internal_error("src/jit.rs", "cranelift compilation aborted");
    assert_eq!(d.level, DiagnosticLevel::InternalError);
    assert_eq!(d.code, "RHP9999");

    let rendered = d.render_human();
    assert!(rendered.contains("internal_error[RHP9999]:"));
    assert!(rendered.contains("this is a bug in rhp"));
    assert!(rendered.contains("https://github.com/vuenice/rhp/issues"));
}

#[test]
fn test_cascading_error_suppression_and_cap() {
    // A missing closing brace causing multiple downstream syntax anomalies
    let cascading_code = r#"
class Broken {
    public function foo() {
        $x = 10;
        // missing closing brace

    public function bar() {
        return 20;
    }

    public function baz() {
        return 30;
    }
}
"#;

    let diags = check_source(cascading_code, "test_cascade.rhp", 3);
    assert!(!diags.is_empty());
    assert!(diags.len() <= 3, "Errors must be capped to max_errors (3)");
}

#[test]
fn test_utf16_column_units_and_byte_offsets() {
    // UTF-8 multi-byte characters (emoji 😀 is 4 UTF-8 bytes, 2 UTF-16 code units)
    let source = "/* 😀 */ fn total($a, ) {}";
    let file = rhp::lexer::SourceFile::new(0, "test.rhp", source);

    let (line, col_char) = file.line_col(9);
    let (_, col_utf16) = file.line_col_utf16(9);
    let start_offset = file.line_start_offset(1);

    assert_eq!(line, 1);
    assert_eq!(start_offset, Some(0));
    assert!(col_utf16 >= col_char);
}
