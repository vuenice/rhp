use std::fs;
use std::process::Command;

use rhp::lint::{add_variable_types, format_source, lint_source};

#[test]
fn test_formatter_indentation_and_operator_spacing() {
    let unformatted = r#"
if($x>0){
$y=10;
$z=$x+$y;
}else{
$z=0;
}
"#;

    let formatted = format_source(unformatted);
    let expected = "\
if ($x > 0) {
    $y = 10;
    $z = $x + $y;
} else {
    $z = 0;
}
";
    assert_eq!(formatted, expected);
}

#[test]
fn test_autotype_variables_primitives_and_collections() {
    let unformatted = r#"
$x = '';
$y = 1;
$f = 3.14;
$b = true;
$empty = [];
$list = Arr::list([1, 2]);
$dict = Arr::dict(['name' => 'Alice']);
"#;

    let typed = add_variable_types(unformatted);

    assert!(typed.contains("string $x = '';"), "Expected string $x: {}", typed);
    assert!(typed.contains("int $y = 1;"), "Expected int $y: {}", typed);
    assert!(typed.contains("float $f = 3.14;"), "Expected float $f: {}", typed);
    assert!(typed.contains("bool $b = true;"), "Expected bool $b: {}", typed);
    assert!(typed.contains("array $empty = [];"), "Expected array $empty: {}", typed);
    assert!(typed.contains("list $list = Arr::list([1, 2]);"), "Expected list $list: {}", typed);
    assert!(typed.contains("dict $dict = Arr::dict(['name' => 'Alice']);"), "Expected dict $dict: {}", typed);
}

#[test]
fn test_autotype_recognizes_list_dict_hashmap_from_array_literals() {
    // Recognize list, dict, or hashMap if array is defined at declaration time and not empty
    let code = r#"
$numbers = [10, 20, 30];
$person = ['name' => 'Alice', 'role' => 'admin'];
$complexMap = [1 => 'first', 2 => 'second'];
$emptyArray = [];
"#;

    let typed = add_variable_types(code);

    assert!(typed.contains("list $numbers = [10, 20, 30];"), "Got: {}", typed);
    assert!(typed.contains("dict $person = ['name' => 'Alice', 'role' => 'admin'];"), "Got: {}", typed);
    assert!(typed.contains("hashMap $complexMap = [1 => 'first', 2 => 'second'];"), "Got: {}", typed);
    assert!(typed.contains("array $emptyArray = [];"), "Got: {}", typed);
}

#[test]
fn test_autotype_infers_default_parameter_types() {
    // If variables or parameters are defined with default values at declaration time, infer their types
    let code = r#"
function configure(
    $timeout = 30,
    $appName = 'my-app',
    $debug = false,
    $tags = ['prod', 'v1'],
    $headers = ['Content-Type' => 'application/json'],
    $portMap = [80 => 8080, 443 => 8443]
): void {
}
"#;

    let typed = add_variable_types(code);

    assert!(typed.contains("int $timeout = 30"), "Got: {}", typed);
    assert!(typed.contains("string $appName = 'my-app'"), "Got: {}", typed);
    assert!(typed.contains("bool $debug = false"), "Got: {}", typed);
    assert!(typed.contains("list $tags = ['prod', 'v1']"), "Got: {}", typed);
    assert!(typed.contains("dict $headers = ['Content-Type' => 'application/json']"), "Got: {}", typed);
    assert!(typed.contains("hashMap $portMap = [80 => 8080, 443 => 8443]"), "Got: {}", typed);
}

#[test]
fn test_existing_types_are_not_changed_and_mismatches_reported_in_json() {
    // If types are already written, DO NOT change them. Just show error if they are wrong in completion.
    let code = r#"
int $wrongVar = 'hello';

class Config {
    public int $wrongProp = [1, 2, 3];

    public function process(int $wrongParam = 'not-an-int'): void {
    }
}
"#;

    // 1. autotype must NOT rewrite or change the existing types
    let typed = add_variable_types(code);
    assert!(typed.contains("int $wrongVar = 'hello';"), "Should preserve int type: {}", typed);
    assert!(typed.contains("public int $wrongProp = [1, 2, 3];"), "Should preserve int property: {}", typed);
    assert!(typed.contains("int $wrongParam = 'not-an-int'"), "Should preserve int param: {}", typed);

    // 2. lint/typecheck must detect and report these mismatches in the JSON output
    let res = lint_source(code, "mismatch.rhp");
    let mut report = rhp::lint::LintReport::new();
    report.files_scanned = 1;
    for issue in res.issues {
        report.add_issue(issue);
    }

    let json = report.to_json();
    assert!(json.contains("\"status\": \"error\""), "Got: {}", json);
    assert!(json.contains("\"code\": \"variable-type-mismatch\""), "Got: {}", json);
    assert!(json.contains("\"code\": \"property-default-type-mismatch\""), "Got: {}", json);
    assert!(json.contains("\"code\": \"parameter-default-type-mismatch\""), "Got: {}", json);
}

#[test]
fn test_autotype_does_not_break_dynamic_reassignments() {
    // When a variable is reassigned to a different type in the same scope, do not add a locked type
    let conflicting = r#"
$data = 100;
$data = "ID-" . $data;
"#;

    let typed = add_variable_types(conflicting);
    // Should NOT lock $data to int
    assert!(!typed.contains("int $data = 100;"), "Should not lock dynamic variable: {}", typed);
    assert!(typed.contains("$data = 100;"));
}

#[test]
fn test_autotype_does_not_duplicate_existing_types() {
    let already_typed = r#"
string $msg = "hello";
int $counter = 0;
"#;

    let typed = add_variable_types(already_typed);
    assert!(!typed.contains("string string $msg"));
    assert!(!typed.contains("int int $counter"));
}

#[test]
fn test_autotype_class_properties() {
    let class_src = r#"
class User {
    public $name = 'guest';
    private $age = 20;
    protected $isAdmin = false;
    public $roles = ['editor', 'viewer'];
    public $meta = ['theme' => 'dark'];
}
"#;

    let typed = add_variable_types(class_src);
    assert!(typed.contains("public string $name = 'guest';"), "Got: {}", typed);
    assert!(typed.contains("private int $age = 20;"), "Got: {}", typed);
    assert!(typed.contains("protected bool $isAdmin = false;"), "Got: {}", typed);
    assert!(typed.contains("public list $roles = ['editor', 'viewer'];"), "Got: {}", typed);
    assert!(typed.contains("public dict $meta = ['theme' => 'dark'];"), "Got: {}", typed);
}

#[test]
fn test_lint_pipeline_end_to_end_clean_code() {
    let code = r#"
$x = '';
$y = 1;
if ($y > 0) {
    $x = 'positive';
}
"#;

    let res = lint_source(code, "test.rhp");
    // Code should be formatted and autotyped
    assert!(res.formatted_content.contains("string $x = '';"));
    assert!(res.formatted_content.contains("int $y = 1;"));
    // Clean code should have 0 issues
    assert_eq!(res.issues.len(), 0);
}

#[test]
fn test_lint_pipeline_detects_type_boundary_issues_in_json() {
    let bad_code = r#"
function calculate($a, $b) {
    return $a + $b;
}
"#;

    let res = lint_source(bad_code, "math.rhp");
    assert!(!res.issues.is_empty(), "Expected issues for missing parameter and return types");

    let mut report = rhp::lint::LintReport::new();
    report.files_scanned = 1;
    for issue in res.issues {
        report.add_issue(issue);
    }

    let json_output = report.to_json();
    assert!(json_output.contains("\"status\": \"error\""), "Got: {}", json_output);
    assert!(json_output.contains("\"code\": \"missing-parameter-type\""), "Got: {}", json_output);
    assert!(json_output.contains("\"code\": \"missing-return-type\""), "Got: {}", json_output);
    assert!(json_output.contains("\"file\": \"math.rhp\""), "Got: {}", json_output);
    assert!(json_output.contains("\"line\":"), "Got: {}", json_output);
    assert!(json_output.contains("\"column\":"), "Got: {}", json_output);
    assert!(json_output.contains("\"span\":"), "Got: {}", json_output);
}

#[test]
fn test_cli_rhp_run_lint_autoapplies_and_reports_json() {
    let temp_dir = std::env::temp_dir().join("rhp_lint_test");
    let _ = fs::create_dir_all(&temp_dir);
    let temp_file = temp_dir.join("sample.rhp");

    let unformatted_code = "$x='';\n$y=1;\n";
    fs::write(&temp_file, unformatted_code).expect("Failed to write temp file");

    let exe_path = env!("CARGO_BIN_EXE_rhp");
    let output = Command::new(exe_path)
        .arg("run")
        .arg("lint")
        .arg(temp_file.to_str().unwrap())
        .output()
        .expect("Failed to run rhp binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"status\": \"ok\""), "Output was: {}", stdout);
    assert!(stdout.contains("\"files_scanned\": 1"));
    assert!(stdout.contains("\"files_modified\": 1"));
    assert!(stdout.contains("\"total_issues\": 0"));

    // Check that the file on disk was automatically modified with indentation, formatting, and types!
    let updated_content = fs::read_to_string(&temp_file).expect("Failed to read updated file");
    assert_eq!(updated_content, "string $x = '';\nint $y = 1;\n");

    let _ = fs::remove_file(temp_file);
    let _ = fs::remove_dir(temp_dir);
}

#[test]
fn test_casing_variable_snake_case_conversion_and_superglobals_preservation() {
    let unformatted = r#"
$myVar = 10;
$totalSum = $myVar + 5;
$userXMLParser = null;
$thisValue = $this;
$getQuery = $_GET;
"#;

    let res = rhp::lint::lint_source(unformatted, "test.rhp");
    let content = res.formatted_content;

    // Check variables converted to snake_case
    assert!(content.contains("int $my_var = 10;"), "Got: {}", content);
    assert!(content.contains("$total_sum = $my_var + 5;"), "Got: {}", content);
    assert!(content.contains("$user_xml_parser = null;"), "Got: {}", content);

    // Special variables $this and $_GET must NEVER be mangled
    assert!(content.contains("$this"), "Expected $this to be preserved: {}", content);
    assert!(content.contains("$_GET"), "Expected $_GET to be preserved: {}", content);
}

#[test]
fn test_casing_class_pascal_case_and_function_camel_case() {
    let code = r#"
class user_account {
    public $userEmail = 'test@example.com';

    public function get_user_email(): string {
        return $this->userEmail;
    }
}

function calculate_user_tax(int $taxAmount): int {
    return $taxAmount * 2;
}

$acc = new user_account();
$res = calculate_user_tax(50);
"#;

    let res = rhp::lint::lint_source(code, "service.rhp");
    let content = res.formatted_content;

    // Class name to PascalCase
    assert!(content.contains("class UserAccount {"), "Got: {}", content);
    assert!(content.contains("new UserAccount()"), "Got: {}", content);

    // Function and method names to camelCase, with opening braces on the next line
    assert!(content.contains("function getUserEmail(): string\n    {"), "Got: {}", content);
    assert!(content.contains("function calculateUserTax(int $tax_amount): int\n{"), "Got: {}", content);
    assert!(content.contains("calculateUserTax(50);"), "Got: {}", content);

    // Variable and property to snake_case
    assert!(content.contains("public string $user_email = 'test@example.com';"), "Got: {}", content);
    assert!(content.contains("$this->user_email"), "Got: {}", content);
    assert!(content.contains("int $tax_amount"), "Got: {}", content);
}

#[test]
fn test_casing_test_function_snake_case_exception() {
    // Function name will have an exception that if it starts with test then it should be in snake_case
    let code = r#"
function testUserLogin(): void {
    $sessionToken = 'xyz';
}

function test_database_connection(): void {
    $dbHost = 'localhost';
}

function normal_utility_function(): void {
}
"#;

    let res = rhp::lint::lint_source(code, "test_suite.rhp");
    let content = res.formatted_content;

    // Functions starting with 'test' must be in snake_case, with opening braces on the next line
    assert!(content.contains("function test_user_login(): void\n{"), "Got: {}", content);
    assert!(content.contains("function test_database_connection(): void\n{"), "Got: {}", content);

    // Standard non-test functions must be in camelCase
    assert!(content.contains("function normalUtilityFunction(): void\n{"), "Got: {}", content);

    // Variables inside both are snake_case
    assert!(content.contains("string $session_token = 'xyz';"), "Got: {}", content);
    assert!(content.contains("string $db_host = 'localhost';"), "Got: {}", content);
}

#[test]
fn test_lint_no_fix_flag_reports_warnings_in_json_and_leaves_file_untouched() {
    let unformatted_code = r#"
class my_controller {
    function do_action($myInput = 10): void {
        $calculatedVal = $myInput * 2;
    }
}
"#;

    // Lint with --no-fix (fix = false, case_change = true)
    let opts = rhp::lint::LintOptions {
        fix: false,
        case_change: true,
    };
    let res = rhp::lint::lint_source_with_options(unformatted_code, "controller.rhp", opts);

    // File content must NOT be modified
    assert_eq!(res.modified, false);

    // Should have reported issues for casing violations
    assert!(!res.issues.is_empty());
    let has_class_issue = res.issues.iter().any(|i| i.code == "class-casing" && i.message.contains("MyController"));
    let has_func_issue = res.issues.iter().any(|i| i.code == "function-casing" && i.message.contains("doAction"));
    let has_var_issue = res.issues.iter().any(|i| i.code == "variable-casing" && i.message.contains("$my_input"));

    assert!(has_class_issue, "Expected class-casing issue. Issues: {:?}", res.issues);
    assert!(has_func_issue, "Expected function-casing issue. Issues: {:?}", res.issues);
    assert!(has_var_issue, "Expected variable-casing issue. Issues: {:?}", res.issues);
}

#[test]
fn test_lint_no_case_change_flag_skips_casing_conversion() {
    let code = r#"
class my_service {
    function calculate_total($itemPrice = 100): int {
        $myVar = $itemPrice;
        return $myVar;
    }
}
"#;

    // Lint with fix = true, case_change = false (--no-case-change)
    let opts = rhp::lint::LintOptions {
        fix: true,
        case_change: false,
    };
    let res = rhp::lint::lint_source_with_options(code, "service.rhp", opts);

    // Formatted and autotyped, but original casing preserved!
    let content = res.formatted_content;
    assert!(content.contains("class my_service {"), "Casing should NOT change: {}", content);
    assert!(content.contains("function calculate_total(int $itemPrice = 100): int\n    {"), "Casing should NOT change: {}", content);
    assert!(content.contains("int $myVar = $itemPrice;"), "Casing should NOT change: {}", content);
}

#[test]
fn test_cli_no_fix_and_no_case_change_options() {
    let temp_dir = std::env::temp_dir().join("rhp_lint_flags_test");
    let _ = fs::create_dir_all(&temp_dir);
    let temp_file = temp_dir.join("test_sample.rhp");

    let sample_code = "class my_demo { function testRun($userName = 'Alice'): void { $totalCount = 1; } }\n";
    fs::write(&temp_file, sample_code).expect("Failed to write temp file");

    let exe_path = env!("CARGO_BIN_EXE_rhp");

    // 1. Run with --no-fix: must report casing warnings in JSON and NOT alter file on disk
    let output_no_fix = Command::new(exe_path)
        .arg("run")
        .arg("lint")
        .arg(temp_file.to_str().unwrap())
        .arg("--no-fix")
        .output()
        .expect("Failed to run rhp binary");

    let stdout_no_fix = String::from_utf8_lossy(&output_no_fix.stdout);
    assert!(stdout_no_fix.contains("\"class-casing\""), "Output: {}", stdout_no_fix);
    assert!(stdout_no_fix.contains("\"variable-casing\""), "Output: {}", stdout_no_fix);

    // Verify file on disk is still the original unformatted/uncased code
    let unchanged = fs::read_to_string(&temp_file).unwrap();
    assert_eq!(unchanged, sample_code);

    // 2. Run with --no-case-change: must format & autotype but NOT rename identifiers
    let output_no_case = Command::new(exe_path)
        .arg("run")
        .arg("lint")
        .arg(temp_file.to_str().unwrap())
        .arg("--no-case-change")
        .output()
        .expect("Failed to run rhp binary");

    let stdout_no_case = String::from_utf8_lossy(&output_no_case.stdout);
    assert!(stdout_no_case.contains("\"files_modified\": 1"), "Output: {}", stdout_no_case);

    let content_no_case = fs::read_to_string(&temp_file).unwrap();
    assert!(content_no_case.contains("class my_demo {"), "Expected original class name: {}", content_no_case);
    assert!(content_no_case.contains("string $userName = 'Alice'"), "Expected original var name: {}", content_no_case);

    // 3. Run default (with fixes & casing enabled): must rename to PascalCase, snake_case, etc.
    let _output_fixed = Command::new(exe_path)
        .arg("run")
        .arg("lint")
        .arg(temp_file.to_str().unwrap())
        .output()
        .expect("Failed to run rhp binary");

    let content_fixed = fs::read_to_string(&temp_file).unwrap();
    assert!(content_fixed.contains("class MyDemo {"), "Expected PascalCase: {}", content_fixed);
    assert!(content_fixed.contains("function test_run(string $user_name = 'Alice'): void {"), "Expected snake_case test func: {}", content_fixed);
    assert!(content_fixed.contains("int $total_count = 1;"), "Expected snake_case var: {}", content_fixed);

    let _ = fs::remove_file(temp_file);
    let _ = fs::remove_dir(temp_dir);
}

#[test]
fn test_user_requested_format_code_snippet() {
    let unformatted = r#"int $int_value = 1;
float $float_value = 1.1;
bool $bool_value = true;
string $string_value = 'hello world';

dict $arr_dict = [
"name" => "RHP Playground",
"mode" => "Standalone",
"description" => "Easily run and test PHP & RHP code instantly",
];

list $arr_list = [ 1, 2, 3 ];

dd(
$int_value,
$float_value,
$bool_value,
$string_value,
$arr_dict,
$arr_list
);
"#;

    let res = rhp::lint::lint_source(unformatted, "playground.rhp");
    let expected = "\
int $int_value = 1;
float $float_value = 1.1;
bool $bool_value = true;
string $string_value = 'hello world';

dict $arr_dict = [
    \"name\" => \"RHP Playground\",
    \"mode\" => \"Standalone\",
    \"description\" => \"Easily run and test PHP & RHP code instantly\",
];

list $arr_list = [ 1, 2, 3 ];

dd(
    $int_value,
    $float_value,
    $bool_value,
    $string_value,
    $arr_dict,
    $arr_list
);
";
    assert_eq!(res.formatted_content, expected);
}

#[test]
fn test_multiline_function_params_and_next_line_curly_braces() {
    let unformatted = r#"
function computeTax(
int $subtotal,
float $rate,
list $discounts = [10, 20]
): float {
return $subtotal * $rate;
}
"#;

    let formatted = format_source(unformatted);
    let expected = "\
function computeTax(
    int $subtotal,
    float $rate,
    list $discounts = [10, 20]
): float
{
    return $subtotal * $rate;
}
";
    assert_eq!(formatted, expected);
}

#[test]
fn test_single_line_array_and_function_call_comma_spacing() {
    let unformatted = r#"
$nums=[1,2,3];
$names=['alpha','beta','gamma'];
$call=myFunction($a,$b,$c);
"#;

    let formatted = format_source(unformatted);
    assert!(formatted.contains("$nums = [1, 2, 3];"), "Got: {}", formatted);
    assert!(formatted.contains("$names = ['alpha', 'beta', 'gamma'];"), "Got: {}", formatted);
    assert!(formatted.contains("$call = myFunction($a, $b, $c);"), "Got: {}", formatted);
}

#[test]
fn test_formatter_wraps_if_and_for_conditions_with_round_brackets() {
    // 1. `if $i <10` becomes `if ($i < 10)`
    let code_if = r#"
if $i <10 {
    $x = $i;
}
"#;
    let formatted_if = format_source(code_if);
    assert!(
        formatted_if.contains("if ($i < 10) {"),
        "Expected `if ($i < 10) {{`, got:\n{}",
        formatted_if
    );

    // 2. `for $item in $items` becomes `for ($item in $items)`
    let code_for = r#"
for $item in $items {
    echo $item;
}
"#;
    let formatted_for = format_source(code_for);
    assert!(
        formatted_for.contains("for ($item in $items) {"),
        "Expected `for ($item in $items) {{`, got:\n{}",
        formatted_for
    );

    // 3. `for $k => $v in $items` becomes `for ($k => $v in $items)`
    let code_for_kv = r#"
for $k => $v in $items {
    echo $v;
}
"#;
    let formatted_for_kv = format_source(code_for_kv);
    assert!(
        formatted_for_kv.contains("for ($k => $v in $items) {"),
        "Expected `for ($k => $v in $items) {{`, got:\n{}",
        formatted_for_kv
    );

    // 4. Already wrapped conditions must not be double wrapped
    let code_already_wrapped = r#"
if ($i < 10) {
    $x = $i;
}
for ($item in $items) {
    echo $item;
}
"#;
    let formatted_already = format_source(code_already_wrapped);
    assert!(!formatted_already.contains("(("));
    assert!(formatted_already.contains("if ($i < 10) {"));
    assert!(formatted_already.contains("for ($item in $items) {"));
}

#[test]
fn test_formatter_moves_trailing_operators_to_next_line_and_indents_continuations() {
    // Exact user requirement:
    // Arr::list()->
    // map()
    // should become
    // Arr::list()
    //     ->map();
    let chained_call = "Arr::list()->\nmap()\n";
    let formatted_chained = format_source(chained_call);
    assert!(
        formatted_chained.contains("Arr::list()\n    ->map();"),
        "Expected chained method call with -> on next line indented, got:\n{}",
        formatted_chained
    );

    // Multiline boolean operator &&
    let bool_expr = "$x = $a &&\n$b\n";
    let formatted_bool = format_source(bool_expr);
    assert!(
        formatted_bool.contains("$x = $a\n    && $b;"),
        "Expected && on next line indented, got:\n{}",
        formatted_bool
    );

    // Multiline ternary ?:
    let ternary_expr = "$res = $cond ?\n$yes :\n$no\n";
    let formatted_ternary = format_source(ternary_expr);
    assert!(
        formatted_ternary.contains("$res = $cond\n    ? $yes\n    : $no;"),
        "Expected ternary on continuation lines indented, got:\n{}",
        formatted_ternary
    );
}

#[test]
fn test_formatter_inserts_missing_semicolons_relying_on_indentation_and_operators() {
    // Semicolon insertion on simple statements
    let code = r#"
$x = 10
$y = 20
return $x + $y
"#;
    let formatted = format_source(code);
    assert!(formatted.contains("$x = 10;"), "Got:\n{}", formatted);
    assert!(formatted.contains("$y = 20;"), "Got:\n{}", formatted);
    assert!(formatted.contains("return $x + $y;"), "Got:\n{}", formatted);

    // Multiline continuation statement should NOT receive semicolon on intermediate line
    let multiline_stmt = r#"
$val = $obj
    ->getName()
"#;
    let formatted_multi = format_source(multiline_stmt);
    assert!(!formatted_multi.contains("$val = $obj;"), "Line 1 should not have semicolon: {}", formatted_multi);
    assert!(formatted_multi.contains("->getName();"), "Line 2 should have semicolon: {}", formatted_multi);
}



