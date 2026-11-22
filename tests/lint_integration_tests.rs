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

    assert!(typed.contains("$x: string = '';"), "Expected string $x: {}", typed);
    assert!(typed.contains("$y: int = 1;"), "Expected int $y: {}", typed);
    assert!(typed.contains("$f: float = 3.14;"), "Expected float $f: {}", typed);
    assert!(typed.contains("$b: bool = true;"), "Expected bool $b: {}", typed);
    assert!(typed.contains("$empty: array = [];"), "Expected array $empty: {}", typed);
    assert!(typed.contains("$list: list = Arr::list([1, 2]);"), "Expected list $list: {}", typed);
    assert!(typed.contains("$dict: dict = Arr::dict(['name' => 'Alice']);"), "Expected dict $dict: {}", typed);
}

#[test]
fn test_autotype_bytes_and_string_literals() {
    let unformatted = r#"
$utf8 = "Hello world";
$b1 = b"abc";
$b2 = byte("xyz");
$b3 = Str::toEncoding($utf8, "Shift-JIS");
"#;

    let typed = add_variable_types(unformatted);
    assert!(typed.contains("$utf8: string = \"Hello world\";"), "Got: {}", typed);
    assert!(typed.contains("$b1: byte = b\"abc\";"), "Got: {}", typed);
    assert!(typed.contains("$b2: byte = byte(\"xyz\");"), "Got: {}", typed);
    assert!(typed.contains("$b3: byte = Str::toEncoding($utf8, \"Shift-JIS\");"), "Got: {}", typed);
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

    assert!(typed.contains("$numbers: list = [10, 20, 30];"), "Got: {}", typed);
    assert!(typed.contains("$person: dict = ['name' => 'Alice', 'role' => 'admin'];"), "Got: {}", typed);
    assert!(typed.contains("$complexMap: hashMap = [1 => 'first', 2 => 'second'];"), "Got: {}", typed);
    assert!(typed.contains("$emptyArray: array = [];"), "Got: {}", typed);
}

#[test]
fn test_autotype_promotes_empty_array_from_first_write() {
    let code = r#"
$items = [];
$items[] = 1;

$named = [];
$named['name'] = 'Ada';

$sparse = [];
$sparse[999] = 1;

$at_zero = [];
$at_zero[0] = 'a';

$untouched = [];

$unique = Arr::set([1, 1, 2]);
"#;

    let typed = add_variable_types(code);
    assert!(typed.contains("$items: list = [];"), "Got: {}", typed);
    assert!(typed.contains("$named: dict = [];"), "Got: {}", typed);
    assert!(typed.contains("$sparse: hashMap = [];"), "Got: {}", typed);
    assert!(typed.contains("$at_zero: list = [];"), "Got: {}", typed);
    assert!(typed.contains("$untouched: array = [];"), "Got: {}", typed);
    assert!(typed.contains("$unique: set = Arr::set([1, 1, 2]);"), "Got: {}", typed);
}

#[test]
fn test_autotype_adds_return_types() {
    let code = r#"
function ids() {
    return [1, 2, 3];
}

function person() {
    $row = ['name' => 'Ada'];
    return $row;
}

function emptyBag() {
    return [];
}

function ping() {
    $n = 1;
}

function keep($items: list): list {
    return $items;
}
"#;

    let typed = add_variable_types(code);
    assert!(typed.contains("function ids(): list"), "Got: {}", typed);
    assert!(typed.contains("function person(): dict"), "Got: {}", typed);
    assert!(typed.contains("function emptyBag(): array"), "Got: {}", typed);
    assert!(typed.contains("function ping()"), "Got: {}", typed);
    assert!(!typed.contains("function ping():"), "void stays unannotated, got: {}", typed);
    assert!(typed.contains("function keep($items: list): list"), "Got: {}", typed);
    assert!(!typed.contains("): list: list"), "Got: {}", typed);
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

    assert!(typed.contains("$timeout: int = 30"), "Got: {}", typed);
    assert!(typed.contains("$appName: string = 'my-app'"), "Got: {}", typed);
    assert!(typed.contains("$debug: bool = false"), "Got: {}", typed);
    assert!(typed.contains("$tags: list = ['prod', 'v1']"), "Got: {}", typed);
    assert!(typed.contains("$headers: dict = ['Content-Type' => 'application/json']"), "Got: {}", typed);
    assert!(typed.contains("$portMap: hashMap = [80 => 8080, 443 => 8443]"), "Got: {}", typed);
}

#[test]
fn test_existing_types_are_not_changed_and_mismatches_reported_in_json() {
    // If types are already written, DO NOT change them. Just show error if they are wrong in completion.
    let code = r#"
$wrongVar: int = 'hello';

class Config {
    public $wrongProp: int = [1, 2, 3];

    public function process($wrongParam: int = 'not-an-int'): void {
    }
}
"#;

    // 1. autotype must NOT rewrite or change the existing types
    let typed = add_variable_types(code);
    assert!(typed.contains("$wrongVar: int = 'hello';"), "Should preserve int type: {}", typed);
    assert!(typed.contains("public $wrongProp: int = [1, 2, 3];"), "Should preserve int property: {}", typed);
    assert!(typed.contains("$wrongParam: int = 'not-an-int'"), "Should preserve int param: {}", typed);

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
$data = "ID-" + string($data);
"#;

    let typed = add_variable_types(conflicting);
    // Should NOT lock $data to int
    assert!(!typed.contains("$data: int = 100;"), "Should not lock dynamic variable: {}", typed);
    assert!(typed.contains("$data = 100;"));
}

#[test]
fn test_autotype_does_not_duplicate_existing_types() {
    let already_typed = r#"
$msg: string = "hello";
$counter: int = 0;
"#;

    let typed = add_variable_types(already_typed);
    assert!(!typed.contains("$msg: string: string"));
    assert!(!typed.contains("$counter: int: int"));
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
    assert!(typed.contains("public $name: string = 'guest';"), "Got: {}", typed);
    assert!(typed.contains("private $age: int = 20;"), "Got: {}", typed);
    assert!(typed.contains("protected $isAdmin: bool = false;"), "Got: {}", typed);
    assert!(typed.contains("public $roles: list = ['editor', 'viewer'];"), "Got: {}", typed);
    assert!(typed.contains("public $meta: dict = ['theme' => 'dark'];"), "Got: {}", typed);
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
    assert!(res.formatted_content.contains("$x: string = '';"));
    assert!(res.formatted_content.contains("$y: int = 1;"));
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
    assert!(
        res.formatted_content.contains("function calculate($a, $b): int"),
        "return type comes from the linter, got: {}",
        res.formatted_content
    );
    assert!(!res.issues.is_empty(), "Expected issues for missing parameter types");

    let mut report = rhp::lint::LintReport::new();
    report.files_scanned = 1;
    for issue in res.issues {
        report.add_issue(issue);
    }

    let json_output = report.to_json();
    assert!(json_output.contains("\"status\": \"error\""), "Got: {}", json_output);
    assert!(json_output.contains("\"code\": \"missing-parameter-type\""), "Got: {}", json_output);
    assert!(!json_output.contains("\"code\": \"missing-return-type\""), "Got: {}", json_output);
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
    assert_eq!(updated_content, "$x: string = '';\n$y: int = 1;\n");

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
    assert!(content.contains("$my_var: int = 10;"), "Got: {}", content);
    assert!(content.contains("$total_sum: int = $my_var + 5;"), "Got: {}", content);
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

function calculate_user_tax($taxAmount: int): int {
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
    assert!(content.contains("function calculateUserTax($tax_amount: int): int\n{"), "Got: {}", content);
    assert!(content.contains("calculateUserTax(50);"), "Got: {}", content);

    // Variable and property to snake_case
    assert!(content.contains("public $user_email: string = 'test@example.com';"), "Got: {}", content);
    assert!(content.contains("$this->user_email"), "Got: {}", content);
    assert!(content.contains("$tax_amount: int"), "Got: {}", content);
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
    assert!(content.contains("$session_token: string = 'xyz';"), "Got: {}", content);
    assert!(content.contains("$db_host: string = 'localhost';"), "Got: {}", content);
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
    assert!(content.contains("function calculate_total($itemPrice: int = 100): int\n    {"), "Casing should NOT change: {}", content);
    assert!(content.contains("$myVar: int = $itemPrice;"), "Casing should NOT change: {}", content);
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
    assert!(content_no_case.contains("$userName: string = 'Alice'"), "Expected original var name: {}", content_no_case);

    // 3. Run default (with fixes & casing enabled): must rename to PascalCase, snake_case, etc.
    let _output_fixed = Command::new(exe_path)
        .arg("run")
        .arg("lint")
        .arg(temp_file.to_str().unwrap())
        .output()
        .expect("Failed to run rhp binary");

    let content_fixed = fs::read_to_string(&temp_file).unwrap();
    assert!(content_fixed.contains("class MyDemo {"), "Expected PascalCase: {}", content_fixed);
    assert!(content_fixed.contains("function test_run($user_name: string = 'Alice'): void {"), "Expected snake_case test func: {}", content_fixed);
    assert!(content_fixed.contains("$total_count: int = 1;"), "Expected snake_case var: {}", content_fixed);

    let _ = fs::remove_file(temp_file);
    let _ = fs::remove_dir(temp_dir);
}

#[test]
fn test_user_requested_format_code_snippet() {
    let unformatted = r#"$int_value: int = 1;
$float_value: float = 1.1;
$bool_value: bool = true;
$string_value: string = 'hello world';

$arr_dict: dict = [
"name" => "RHP Playground",
"mode" => "Standalone",
"description" => "Easily run and test PHP & RHP code instantly",
];

$arr_list: list = [ 1, 2, 3 ];

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
$int_value: int = 1;
$float_value: float = 1.1;
$bool_value: bool = true;
$string_value: string = 'hello world';

$arr_dict: dict = [
    \"name\" => \"RHP Playground\",
    \"mode\" => \"Standalone\",
    \"description\" => \"Easily run and test PHP & RHP code instantly\",
];

$arr_list: list = [ 1, 2, 3 ];

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
$subtotal: int,
$rate: float,
$discounts: list = [10, 20]
): float {
return $subtotal * $rate;
}
"#;

    let formatted = format_source(unformatted);
    let expected = "\
function computeTax(
    $subtotal: int,
    $rate: float,
    $discounts: list = [10, 20]
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
fn test_formatter_braces_indented_for_in_only() {
    let indented = "for $item in $items\n    echo $item;\n";
    let formatted = format_source(indented);
    assert!(
        formatted.contains("for ($item in $items) {"),
        "Expected parentheses and an opening brace, got:\n{}",
        formatted
    );
    assert!(
        formatted.contains("echo $item;"),
        "Expected the indented body inside the loop, got:\n{}",
        formatted
    );
    assert!(formatted.contains('}'), "Expected a closing brace, got:\n{}", formatted);

    let sibling = "for $item in $items\n    echo $item;\necho $other;\n";
    let formatted_sibling = format_source(sibling);
    let brace_at = formatted_sibling.find('}').expect("closing brace");
    let other_at = formatted_sibling.find("echo $other").expect("sibling statement");
    assert!(
        brace_at < other_at,
        "The following statement stays outside the loop:\n{}",
        formatted_sibling
    );

    let branched = "for $item in $items\n    if ($item) {\n        echo $item;\n    }\n";
    let formatted_branch = format_source(branched);
    let open = formatted_branch.find("for ($item in $items) {").expect(&formatted_branch);
    let close = formatted_branch.rfind('}').expect(&formatted_branch);
    let echo_at = formatted_branch.find("echo $item").expect(&formatted_branch);
    assert!(open < echo_at && echo_at < close, "if body must stay inside for-in:\n{}", formatted_branch);

    let c_style = "for ($i = 0; $i < 10; $i++) {\n    echo $i;\n}\n";
    let formatted_c = format_source(c_style);
    assert!(
        formatted_c.contains("for ($i = 0; $i < 10; $i++) {"),
        "C-style header must stay intact:\n{}",
        formatted_c
    );
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

#[test]
fn test_unassigned_collection_calls_auto_assigned_and_statements_split() {
    // User request:
    // $x->map();Arr::list($z)->map();
    // without any output value assignment should become:
    // $x = $x->map();
    // $z = Arr::list($z)->map();
    let code = "$x->map();Arr::list($z)->map();";
    let res = lint_source(code, "test.rhp");

    assert!(
        res.formatted_content.contains("$x = $x->map();"),
        "Expected $x = $x->map():\n{}",
        res.formatted_content
    );
    assert!(
        res.formatted_content.contains("$z = Arr::list($z)->map();"),
        "Expected $z = Arr::list($z)->map():\n{}",
        res.formatted_content
    );

    // Statements with semicolon should be on different lines
    let lines: Vec<&str> = res.formatted_content.lines().collect();
    assert!(
        lines.iter().any(|l| l.trim() == "$x = $x->map();"),
        "Expected '$x = $x->map();' on its own line in:\n{}",
        res.formatted_content
    );
    assert!(
        lines.iter().any(|l| l.trim() == "$z = Arr::list($z)->map();"),
        "Expected '$z = Arr::list($z)->map();' on its own line in:\n{}",
        res.formatted_content
    );
}

#[test]
fn test_mutated_array_parameters_converted_to_copy_by_reference_and_comma_spacing() {
    // User request:
    // myFunction($x: array, $y: list, $z: dict)
    // should become
    // myFunction(&$x: array, &$y: list, &$z: dict)
    let code = r#"
function myFunction($x: array, $y: list, $z: dict) {
    $x[0] = 1;
    $y[] = 2;
    $z['key'] = 'val';
}
"#;

    let res = lint_source(code, "test.rhp");
    assert!(
        res.formatted_content.contains("function myFunction(&$x: array, &$y: list, &$z: dict)"),
        "Expected 'function myFunction(&$x: array, &$y: list, &$z: dict)', got:\n{}",
        res.formatted_content
    );
}

#[test]
fn test_combined_user_request_array_mutation_and_auto_assign() {
    // Both features combined:
    // 1. $x->map();Arr::list($z)->map(); are auto-assigned to $x and $z on separate lines
    // 2. $x and $z are mutated by the assignments, so parameters become copy-by-reference
    // 3. $y is mutated via index assignment
    // 4. Space added between parameters and statements on different lines
    let code = r#"
function myFunction($x: array, $y: list, $z: dict) {
    $x->map();Arr::list($z)->map();
    $y[0] = 100;
}
"#;

    let res = lint_source(code, "test.rhp");
    assert!(
        res.formatted_content.contains("function myFunction(&$x: array, &$y: list, &$z: dict)"),
        "Expected signature with reference params and space between commas:\n{}",
        res.formatted_content
    );
    assert!(
        res.formatted_content.contains("$x = $x->map();"),
        "Expected $x = $x->map():\n{}",
        res.formatted_content
    );
    assert!(
        res.formatted_content.contains("$z = Arr::list($z)->map();"),
        "Expected $z = Arr::list($z)->map():\n{}",
        res.formatted_content
    );
    assert!(
        res.formatted_content.contains("$y[0] = 100;"),
        "Expected $y[0] = 100:\n{}",
        res.formatted_content
    );
}

#[test]
fn test_non_mutated_arrays_and_scalars_remain_unchanged() {
    let code = r#"
function process($x: array, $y: list, $count: int): void {
    $x[] = 'modified';
    echo $y[0];
    $count = $count + 1;
}
"#;

    let res = lint_source(code, "test.rhp");
    // $x is mutated array -> becomes &$x: array
    // $y is read-only array -> remains $y: list
    // $count is scalar -> remains $count: int
    assert!(
        res.formatted_content.contains("function process(&$x: array, $y: list, $count: int): void"),
        "Got:\n{}",
        res.formatted_content
    );
}

#[test]
fn test_call_sites_updated_with_references_when_variables_passed() {
    let code = r#"
function appendVal($arr: array): void {
    $arr[] = 42;
}

$myList = [1, 2, 3];
appendVal($myList);
"#;

    let res = lint_source(code, "test.rhp");
    assert!(
        res.formatted_content.contains("function appendVal(&$arr: array): void"),
        "Got:\n{}",
        res.formatted_content
    );
    assert!(
        res.formatted_content.contains("appendVal(&$my_list);"),
        "Got:\n{}",
        res.formatted_content
    );
}

#[test]
fn test_semicolon_splitting_does_not_split_for_loop_or_strings() {
    let code = r#"
for ($i = 0; $i < 10; $i++) {
    $msg = "one;two;three";
}
"#;

    let res = lint_source(code, "test.rhp");
    assert!(
        res.formatted_content.contains("for ($i = 0; $i < 10; $i++) {"),
        "For loop header must not be split on semicolons:\n{}",
        res.formatted_content
    );
    assert!(
        res.formatted_content.contains(r#"$msg = "one;two;three";"#),
        "String with semicolons must not be split:\n{}",
        res.formatted_content
    );
}

#[test]
fn test_division_stored_in_int_warns_without_type_error() {
    let code = r#"
$exact: int = 6 / 2;
$n: int = (5 / 2) + 1;
$later: int = 4;
$later = $later / 2;
$later /= 2;
$truncated: int = int(5 / 2);
$f: float = 5 / 2;
"#;

    let res = lint_source(code, "division.rhp");
    let warnings: Vec<_> = res
        .issues
        .iter()
        .filter(|issue| issue.code == "division-stored-in-int")
        .collect();
    assert_eq!(warnings.len(), 4, "issues: {:?}", res.issues);
    for warning in &warnings {
        assert_eq!(warning.level, rhp::lint::IssueLevel::Warning);
        assert_eq!(
            warning.message,
            "The division operation might result into float value instead of int."
        );
    }
    assert!(
        res.issues.iter().all(|issue| issue.code != "variable-type-mismatch"),
        "division into int is a warning, not a type error: {:?}",
        res.issues
    );
}

#[test]
fn test_explicit_void_return_and_void_assignment() {
    let code = r#"
function ping(): void {
}

function titled(): Void {
}
"#;

    let res = lint_source(code, "void.rhp");
    let redundant: Vec<_> = res
        .issues
        .iter()
        .filter(|issue| issue.code == "redundant-void-return")
        .collect();
    assert_eq!(redundant.len(), 2, "issues: {:?}", res.issues);
    for issue in &redundant {
        assert_eq!(issue.message, "no need to mention return type for void");
    }

    let assigned = "function ping() {\n}\n$x = ping();\n";
    let assigned_res = lint_source(assigned, "assign.rhp");
    let assignments: Vec<_> = assigned_res
        .issues
        .iter()
        .filter(|issue| issue.code == "void-assignment")
        .collect();
    assert_eq!(assignments.len(), 1, "issues: {:?}", assigned_res.issues);
    assert_eq!(
        assignments[0].message,
        "Type Error: Cannot assign the result of void function 'ping'."
    );

    let declared = "function ping() {\n}\n$y: int = ping();\n";
    let declared_res = lint_source(declared, "declare.rhp");
    let declared_assignments: Vec<_> = declared_res
        .issues
        .iter()
        .filter(|issue| issue.code == "void-assignment")
        .collect();
    assert_eq!(declared_assignments.len(), 1, "issues: {:?}", declared_res.issues);
    assert!(
        declared_res.issues.iter().all(|issue| issue.code != "variable-type-mismatch"),
        "void assignment is reported before a type mismatch: {:?}",
        declared_res.issues
    );
}

#[test]
fn test_omitted_void_call_is_not_an_error() {
    let code = "function ping() {\n}\nping();\n";
    let res = lint_source(code, "void.rhp");
    assert!(
        res.issues.iter().all(|issue| {
            issue.code != "void-assignment"
                && issue.code != "redundant-void-return"
                && issue.code != "missing-return-type"
        }),
        "issues: {:?}",
        res.issues
    );
}

#[test]
fn test_formatter_colon_type_spacing() {
    let input = r#"
$x : int = 10;
$y:string = "hello";
function compute($a : int, &$b:string, ...$c : list) : int {
    return $a;
}
class User {
    public $name : string = "";
    private readonly $id:int;
}
$ternary = $cond ? $x : $y;
"#;

    let formatted = format_source(input);
    assert!(formatted.contains("$x: int = 10;"), "got:\n{}", formatted);
    assert!(formatted.contains("$y: string = \"hello\";"), "got:\n{}", formatted);
    assert!(
        formatted.contains("function compute($a: int, &$b: string, ...$c: list): int\n{"),
        "got:\n{}",
        formatted
    );
    assert!(formatted.contains("public $name: string = \"\";"), "got:\n{}", formatted);
    assert!(formatted.contains("private readonly $id: int;"), "got:\n{}", formatted);
    assert!(formatted.contains("$ternary = $cond ? $x : $y;"), "got:\n{}", formatted);
}

