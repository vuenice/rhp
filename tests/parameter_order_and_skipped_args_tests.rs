use rhp::lint::{check_parameter_order, fix_parameter_order, lint_source, lint_source_with_options, LintOptions};
use rhp::parser::parse;
use rhp::type_system::check_program;
use rhp::{run_source_with_vm, PhpValue};

#[test]
fn test_linter_fixes_function_parameter_order_required_before_optional() {
    let code = r#"
function calculatePrice($taxRate: float = 0.05, $basePrice: float, $discount: float = 0.0, $currency: string): float {
    return ($basePrice + ($basePrice * $taxRate)) - $discount;
}
"#;

    let fixed = fix_parameter_order(code);
    assert!(
        fixed.contains("function calculatePrice($basePrice: float, $currency: string, $taxRate: float = 0.05, $discount: float = 0.0): float"),
        "Parameters should be reordered so required come before optional: {}",
        fixed
    );
}

#[test]
fn test_linter_fixes_class_method_parameter_order() {
    let code = r#"
class PaymentGateway {
    public function charge($provider: string = 'stripe', $amount: int, $capture: bool = true, $customerId: string): bool {
        return true;
    }
}
"#;

    let fixed = fix_parameter_order(code);
    assert!(
        fixed.contains("public function charge($amount: int, $customerId: string, $provider: string = 'stripe', $capture: bool = true): bool"),
        "Method parameters must have required before optional: {}",
        fixed
    );
}

#[test]
fn test_linter_fixes_closures_and_arrow_functions() {
    let code = r#"
$calc = function($factor: int = 2, $value: int): int {
    return $value * $factor;
};

$arrow = fn($step: int = 1, $x: int) => $x + $step;
"#;

    let fixed = fix_parameter_order(code);
    assert!(
        fixed.contains("function($value: int, $factor: int = 2): int"),
        "Closure parameters reordered: {}",
        fixed
    );
    assert!(
        fixed.contains("fn($x: int, $step: int = 1) => $x + $step"),
        "Arrow function parameters reordered: {}",
        fixed
    );
}

#[test]
fn test_linter_pipeline_end_to_end_fixes_parameter_order() {
    let code = r#"
function greet($greeting: string = 'Hello', $name: string): string {
    return $greeting + ' ' + $name;
}
"#;

    let result = lint_source(code, "test.rhp");
    assert!(
        result.formatted_content.contains("function greet($name: string, $greeting: string = 'Hello')"),
        "Linter pipeline auto-fixed parameter order: {}",
        result.formatted_content
    );
}

#[test]
fn test_linter_check_mode_reports_issue_for_bad_parameter_order() {
    let code = r#"
function badOrder($opt: int = 10, $req: int): int {
    return $opt + $req;
}
"#;

    let issues = check_parameter_order(code, "bad.rhp");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "required-param-after-optional");
    assert!(issues[0].message.contains("Required parameter '$req' cannot follow optional parameter '$opt'"));

    // Also verify --no-fix lint_source_with_options
    let result = lint_source_with_options(
        code,
        "bad.rhp",
        LintOptions {
            fix: false,
            case_change: true,
            stream: false,
        },
    );
    assert!(
        result.issues.iter().any(|i| i.code == "required-param-after-optional"),
        "Check mode must report required-param-after-optional issue"
    );
}

#[test]
fn test_type_checker_rejects_required_param_after_optional() {
    let code = r#"
function invalidSignature($a: int = 1, $b: int): int {
    return $a + $b;
}
"#;

    let program = parse(code).expect("Parse succeeds");
    let type_err = check_program(&program).expect_err("Type check must fail for invalid parameter order");
    assert!(
        type_err.to_string().contains("Required parameter '$b' cannot follow optional parameter '$a'"),
        "Type error message: {}",
        type_err
    );
}

#[test]
fn test_named_arguments_bind_in_any_order() {
    let code = r#"
function add($a: int, $b: int = 10): int {
    return $a + $b;
}
$val = add('b' => 20, 'a' => 5);
"#;

    let (vm, _) = run_source_with_vm(code).expect("Named call should run");
    assert_eq!(vm.get_main_variable("$val"), Some(&PhpValue::Int(25)));
}

#[test]
fn test_colon_named_arguments_are_rejected() {
    let code = r#"
function add($a: int, $b: int = 10): int {
    return $a + $b;
}
$val = add(b: 20, a: 5);
"#;

    let parse_res = parse(code);
    assert!(parse_res.is_err(), "Colon named arguments must be rejected");
    let err_str = parse_res.unwrap_err().to_string();
    assert!(
        err_str.contains("'b' => value"),
        "Error should point at the arrow form: {}",
        err_str
    );
}

#[test]
fn test_mixed_and_non_literal_named_arguments_are_rejected() {
    let mixed = r#"
function add($a: int, $b: int = 10): int {
    return $a + $b;
}
$val = add(5, 'b' => 20);
"#;
    let mixed_err = parse(mixed).unwrap_err().to_string();
    assert!(mixed_err.contains("entirely positional or entirely named"), "{}", mixed_err);

    let variable_label = r#"
function add($a: int, $b: int = 10): int {
    return $a + $b;
}
$key = 'a';
$val = add($key => 5);
"#;
    let label_err = parse(variable_label).unwrap_err().to_string();
    assert!(label_err.contains("string literals"), "{}", label_err);

    let empty = r#"
function add($a: int, $b: int = 10): int {
    return $a + $b;
}
$val = add(5, ,);
"#;
    let empty_err = parse(empty).unwrap_err().to_string();
    assert!(empty_err.contains("Empty argument slots"), "{}", empty_err);
}

#[test]
fn test_call_with_skipped_optional_arguments_using_extra_comma() {
    let code = r#"
function configureApp(
    $name: string,
    $env: string = 'production',
    $port: int = 8080,
    $debug: bool = false,
    $logLevel: string = 'info'
): string {
    $d = $debug ? '1' : '0';
    return "{$name}:{$env}:{$port}:{$d}:{$logLevel}";
}

$cfg1 = configureApp('name' => 'myApp', 'debug' => true);
$cfg2 = configureApp('logLevel' => 'debug', 'name' => 'apiServer', 'env' => 'staging');
$cfg3 = configureApp('name' => 'webServer', 'port' => 9000);
"#;

    let (vm, _) = run_source_with_vm(code).expect("Execution with skipped args should succeed");
    assert_eq!(
        vm.get_main_variable("$cfg1"),
        Some(&PhpValue::string("myApp:production:8080:1:info"))
    );
    assert_eq!(
        vm.get_main_variable("$cfg2"),
        Some(&PhpValue::string("apiServer:staging:8080:0:debug"))
    );
    assert_eq!(
        vm.get_main_variable("$cfg3"),
        Some(&PhpValue::string("webServer:production:9000:0:info"))
    );
}

#[test]
fn test_call_method_and_constructor_with_skipped_optional_parameters() {
    let code = r#"
class Service {
    public function __construct(
        public $host: string,
        public $port: int = 3306,
        public $user: string = 'root',
        public $database: string = 'main'
    ) {}

    public function query($sql: string, $timeout: int = 30, $fetchAssoc: bool = true): string {
        $mode = $fetchAssoc ? 'assoc' : 'list';
        return "{$this->host}:{$this->port}:{$this->user}:{$this->database}|{$sql}|{$timeout}|{$mode}";
    }
}

$svc = new Service('database' => 'ecommerce', 'host' => 'db.local');
$res = $svc->query('sql' => 'SELECT 1', 'fetchAssoc' => false);
"#;

    let (vm, _) = run_source_with_vm(code).expect("Method and constructor calls with skipped args should succeed");
    assert_eq!(
        vm.get_main_variable("$res"),
        Some(&PhpValue::string("db.local:3306:root:ecommerce|SELECT 1|30|list"))
    );
}

#[test]
fn test_call_closure_with_skipped_optional_parameter() {
    let code = r#"
$calc = function($base: int, $multiplier: int = 2, $offset: int = 5): int {
    return ($base * $multiplier) + $offset;
};

$val = $calc('base' => 10, 'offset' => 100);
"#;

    let (vm, _) = run_source_with_vm(code).expect("Closure call with skipped args should succeed");
    assert_eq!(vm.get_main_variable("$val"), Some(&PhpValue::Int(120)));
}

#[test]
fn test_call_all_optional_parameters_with_leading_skipped_comma() {
    let code = r#"
function coords($x: int = 0, $y: int = 0, $z: int = 0): string {
    return "{$x},{$y},{$z}";
}

$c1 = coords('y' => 50);
$c2 = coords('z' => 100);
"#;

    let (vm, _) = run_source_with_vm(code).expect("Leading skipped commas should succeed");
    assert_eq!(vm.get_main_variable("$c1"), Some(&PhpValue::string("0,50,0")));
    assert_eq!(vm.get_main_variable("$c2"), Some(&PhpValue::string("0,0,100")));
}

#[test]
fn test_skipping_required_parameter_is_rejected() {
    let code = r#"
function requireFirst($required: int, $optional: int = 42): int {
    return $required + $optional;
}
$val = requireFirst('optional' => 100);
"#;

    let program = parse(code).expect("Parse succeeds");
    let type_err = check_program(&program).expect_err("Skipping required parameter must fail type checking");
    assert!(
        type_err.to_string().contains("Cannot skip required parameter '$required'"),
        "Error message: {}",
        type_err
    );
}
