use rhp::{
    run_source_with_vm, FrontendError, PhpValue, TypeErrorKind, parse_and_check,
};

#[test]
fn test_pass_by_reference_basic_scalar_mutation() {
    let source = r#"
        function addTen(int &$val): void {
            $val = $val + 10;
        }

        $num = 25;
        addTen(&$num);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Script execution should succeed");
    assert_eq!(vm.get_main_variable("$num"), Some(&PhpValue::Int(35)));
}

#[test]
fn test_pass_by_reference_swap_multiple_arguments() {
    let source = r#"
        function swap(int &$a, int &$b): void {
            $temp = $a;
            $a = $b;
            $b = $temp;
        }

        $first = 100;
        $second = 200;
        swap(&$first, &$second);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Script execution should succeed");
    assert_eq!(vm.get_main_variable("$first"), Some(&PhpValue::Int(200)));
    assert_eq!(vm.get_main_variable("$second"), Some(&PhpValue::Int(100)));
}

#[test]
fn test_pass_by_reference_nested_call_frames() {
    let source = r#"
        function doubleVal(int &$x): void {
            $x = $x * 2;
        }

        function calculate(int &$val): void {
            doubleVal(&$val);
            $val = $val + 5;
        }

        $result = 10;
        calculate(&$result);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Script execution should succeed");
    // 10 * 2 = 20 + 5 = 25
    assert_eq!(vm.get_main_variable("$result"), Some(&PhpValue::Int(25)));
}

#[test]
fn test_pass_by_reference_array_mutation() {
    let source = r#"
        function appendNumber(array &$arr, int $num): void {
            $arr[] = $num;
        }

        $list = [10, 20];
        appendNumber(&$list, 30);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Script execution should succeed");
    assert_eq!(
        vm.get_main_variable("$list"),
        Some(&PhpValue::array(vec![
            PhpValue::Int(10),
            PhpValue::Int(20),
            PhpValue::Int(30),
        ]))
    );
}

#[test]
fn test_pass_by_reference_class_method() {
    let source = r#"
        class Modifier {
            public function increment(int &$target, int $step = 1): void {
                $target = $target + $step;
            }
        }

        $mod = new Modifier();
        $counter = 50;
        $mod->increment(&$counter, 15);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Script execution should succeed");
    assert_eq!(vm.get_main_variable("$counter"), Some(&PhpValue::Int(65)));
}

#[test]
fn test_pass_by_reference_static_method() {
    let source = r#"
        class MathHelper {
            public static function triple(int &$num): void {
                $num = $num * 3;
            }
        }

        $value = 7;
        MathHelper::triple(&$value);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Script execution should succeed");
    assert_eq!(vm.get_main_variable("$value"), Some(&PhpValue::Int(21)));
}

#[test]
fn test_pass_by_reference_named_arguments() {
    let source = r#"
        function applyBonus(int $bonus, int &$score): void {
            $score = $score + $bonus;
        }

        $playerScore = 100;
        applyBonus(score: &$playerScore, bonus: 50);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Script execution should succeed");
    assert_eq!(vm.get_main_variable("$playerScore"), Some(&PhpValue::Int(150)));
}

#[test]
fn test_pass_by_reference_rejection_missing_ampersand_at_call_site() {
    let source = r#"
        function modify(int &$num): void {
            $num = $num + 1;
        }

        $x = 10;
        modify($x);
    "#;

    let err = parse_and_check(source).expect_err("Call without & should be rejected by type checker");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::MissingReferenceArgument {
                callable_name,
                param_name,
                arg_index,
                ..
            } => {
                assert_eq!(callable_name, "modify");
                assert_eq!(param_name, "$num");
                assert_eq!(arg_index, 1);
            }
            other => panic!("Expected MissingReferenceArgument, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_pass_by_reference_rejection_unexpected_ampersand_at_call_site() {
    let source = r#"
        function display(int $num): void {}

        $x = 10;
        display(&$x);
    "#;

    let err = parse_and_check(source).expect_err("Call with unexpected & should be rejected by type checker");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::UnexpectedReferenceArgument {
                callable_name,
                param_name,
                arg_index,
            } => {
                assert_eq!(callable_name, "display");
                assert_eq!(param_name, "$num");
                assert_eq!(arg_index, 1);
            }
            other => panic!("Expected UnexpectedReferenceArgument, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_pass_by_reference_rejection_literal_argument() {
    let source = r#"
        function update(int &$num): void {
            $num = $num + 1;
        }

        update(&99);
    "#;

    let err = parse_and_check(source).expect_err("Passing literal by reference should be rejected");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::ReferenceArgumentMustBeVariable {
                callable_name,
                arg_index,
            } => {
                assert_eq!(callable_name, "update");
                assert_eq!(arg_index, 1);
            }
            other => panic!("Expected ReferenceArgumentMustBeVariable, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }
}
