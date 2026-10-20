use rhp::{execute, parse_expr, run_source, Environment, PhpValue, RunMode};

#[test]
fn test_strict_equality_and_identity() {
    let source = r#"
        $intStrictMatch = (42 === 42);
        $intStrictMismatch = (42 === '42');
        $stringStrictMatch = ('hello' === 'hello');
        $stringStrictMismatch = ('hello' === 'world');
        $boolStrictMatch = (true === true);
        $boolIntStrictMismatch = (true === 1);
        $boolStringStrictMismatch = (true === '1');
        $boolEmptyStrictMismatch = (false === '');
        $nullStrictMatch = (null === null);
        $nullEmptyStrictMismatch = (null === '');
        $nullFalseStrictMismatch = (null === false);

        // Not identical !==
        $intNotIdentical = (42 !== '42');
        $intIdenticalNegation = (42 !== 42);

        return [
            $intStrictMatch,
            $intStrictMismatch,
            $stringStrictMatch,
            $stringStrictMismatch,
            $boolStrictMatch,
            $boolIntStrictMismatch,
            $boolStringStrictMismatch,
            $boolEmptyStrictMismatch,
            $nullStrictMatch,
            $nullEmptyStrictMismatch,
            $nullFalseStrictMismatch,
            $intNotIdentical,
            $intIdenticalNegation,
        ];
    "#;

    let result = run_source(source).expect("Strict equality checks should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
        ])
    );
}

#[test]
fn test_loose_equality_strict_then_string_fallback() {
    let source = r#"
        // 1. First strict match passes immediately
        $intInt = (100 == 100);
        $strStr = ('rhp' == 'rhp');
        $boolBool = (true == true);

        // 2. Strict match fails, falls back to string conversion
        $intString = (42 == '42');
        $stringInt = ('42' == 42);
        $boolOne = (true == '1');
        $boolEmpty = (false == '');
        $nullEmpty = (null == '');
        $nullFalse = (null == false); // both convert to ""

        // 3. String conversion comparison returns false
        $diffInts = (42 == 43);
        $diffIntStr = (42 == '99');
        $nullZero = (null == 0); // "" == "0" -> false
        $falseZero = (false == '0'); // "" == "0" -> false

        // 4. Negated loose equality != and <>
        $notEqFalse = (42 != '42');
        $notEqAltFalse = (42 <> '42');
        $notEqTrue = (42 != '99');
        $notEqAltTrue = (42 <> '99');

        return [
            $intInt,
            $strStr,
            $boolBool,
            $intString,
            $stringInt,
            $boolOne,
            $boolEmpty,
            $nullEmpty,
            $nullFalse,
            $diffInts,
            $diffIntStr,
            $nullZero,
            $falseZero,
            $notEqFalse,
            $notEqAltFalse,
            $notEqTrue,
            $notEqAltTrue,
        ];
    "#;

    let result = run_source(source).expect("Loose equality checks should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            // Strict matches
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            // String fallback matches
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            // Mismatches
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            // Negations
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_collections_equality() {
    let source = r#"
        $arr1 = [1, 2, 3];
        $arr2 = ['1', '2', '3'];
        $arr3 = [1, 2, 3];
        $arr4 = [4, 5, 6];

        $looseMatch = ($arr1 == $arr2);
        $strictMatch = ($arr1 === $arr3);
        $strictMismatch = ($arr1 === $arr2);
        $looseMismatch = ($arr1 == $arr4);

        $dict1 = Arr::dict(['id' => 10, 'role' => 'admin']);
        $dict2 = Arr::dict(['id' => '10', 'role' => 'admin']);
        $dict3 = Arr::dict(['id' => 10, 'role' => 'admin']);

        $dictLooseMatch = ($dict1 == $dict2);
        $dictStrictMatch = ($dict1 === $dict3);
        $dictStrictMismatch = ($dict1 === $dict2);

        return [
            $looseMatch,
            $strictMatch,
            $strictMismatch,
            $looseMismatch,
            $dictLooseMatch,
            $dictStrictMatch,
            $dictStrictMismatch,
        ];
    "#;

    let result = run_source(source).expect("Collection equality should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
        ])
    );
}

#[test]
fn test_switch_and_match_semantics() {
    let source = r#"
        // Switch uses == (loose equality with string fallback)
        $code = 200;
        $switchRes = '';
        switch ($code) {
            case '200':
                $switchRes = 'matched_string_case';
                break;
            default:
                $switchRes = 'default';
                break;
        }

        // Match uses === (strict identity check)
        $matchRes = match ($code) {
            '200' => 'matched_string',
            200 => 'matched_int',
            default => 'other',
        };

        return [$switchRes, $matchRes];
    "#;

    let result = run_source(source).expect("Switch and match should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("matched_string_case"),
            PhpValue::string("matched_int"),
        ])
    );
}

#[test]
fn test_ast_dev_mode_evaluation() {
    let mut env = Environment::new();

    let expr_strict_true = parse_expr("10 === 10").unwrap();
    let expr_strict_false = parse_expr("10 === '10'").unwrap();
    let expr_loose_true = parse_expr("10 == '10'").unwrap();
    let expr_loose_false = parse_expr("10 == '20'").unwrap();

    let res_strict_true = execute(RunMode::Dev { hot_reload: false, dap_port: None }, &expr_strict_true, &mut env).unwrap();
    let res_strict_false = execute(RunMode::Dev { hot_reload: false, dap_port: None }, &expr_strict_false, &mut env).unwrap();
    let res_loose_true = execute(RunMode::Dev { hot_reload: false, dap_port: None }, &expr_loose_true, &mut env).unwrap();
    let res_loose_false = execute(RunMode::Dev { hot_reload: false, dap_port: None }, &expr_loose_false, &mut env).unwrap();

    assert_eq!(res_strict_true, PhpValue::Bool(true));
    assert_eq!(res_strict_false, PhpValue::Bool(false));
    assert_eq!(res_loose_true, PhpValue::Bool(true));
    assert_eq!(res_loose_false, PhpValue::Bool(false));
}
