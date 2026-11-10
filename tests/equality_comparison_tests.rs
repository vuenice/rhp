use rhp::{
    lint::replace_strict_equality_operators, lint_source, parse, parse_and_check, parse_expr,
    run_source, ParseErrorKind, PhpValue, TypeErrorKind,
};

#[test]
fn test_strict_equality_operator_rejection_at_parse_time() {
    let err_eq = parse("<?php $x = 1 === 2;").unwrap_err();
    match err_eq.kind {
        ParseErrorKind::UnsupportedOperator { op, suggested } => {
            assert_eq!(op, "===");
            assert_eq!(suggested, "==");
        }
        other => panic!("Expected UnsupportedOperator, got {:?}", other),
    }

    let err_neq = parse("<?php $x = 1 !== 2;").unwrap_err();
    match err_neq.kind {
        ParseErrorKind::UnsupportedOperator { op, suggested } => {
            assert_eq!(op, "!==");
            assert_eq!(suggested, "!=");
        }
        other => panic!("Expected UnsupportedOperator, got {:?}", other),
    }

    let expr_err = parse_expr("42 === 42").unwrap_err();
    match expr_err.kind {
        ParseErrorKind::UnsupportedOperator { op, suggested } => {
            assert_eq!(op, "===");
            assert_eq!(suggested, "==");
        }
        other => panic!("Expected UnsupportedOperator, got {:?}", other),
    }
}

#[test]
fn test_rhp_lint_converts_strict_operators() {
    let source = r#"<?php
if ($a === 1 && $b !== 2) {
    $c = $a === $b;
    $msg = "=== not an operator in string ===";
    $neq = '!== not an operator either';
    // === comment should stay ===
    /* !== block comment !== */
}
"#;

    let lint_res = lint_source(source, "test.php");
    let linted = &lint_res.formatted_content;
    assert!(linted.contains("if ($a == 1 && $b != 2)"));
    assert!(linted.contains("$c = $a == $b;"));
    assert!(linted.contains("\"=== not an operator in string ===\""));
    assert!(linted.contains("'!== not an operator either'"));
    assert!(linted.contains("// === comment should stay ==="));
    assert!(linted.contains("/* !== block comment !== */"));

    // Also verify direct function
    let converted = replace_strict_equality_operators("$a === $b && $c !== $d");
    assert_eq!(converted, "$a == $b && $c != $d");
}

#[test]
fn test_strict_type_comparison_enforcement() {
    // 1. Same types succeed at compile time and evaluate correctly
    let valid_source = r#"
        $intSame = (42 == 42);
        $intDiff = (42 == 43);
        $intNotEq = (42 != 43);
        $strSame = ('hello' == 'hello');
        $strDiff = ('hello' == 'world');
        $boolSame = (true == true);
        $boolDiff = (true == false);
        $nullSame = (null == null);

        return [
            $intSame,
            $intDiff,
            $intNotEq,
            $strSame,
            $strDiff,
            $boolSame,
            $boolDiff,
            $nullSame,
        ];
    "#;

    let res = run_source(valid_source).expect("Strict same-type comparisons must succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
        ])
    );

    // 2. Mismatched types must produce compile/type error
    let invalid_cases = [
        "42 == '42';",
        "'42' == 42;",
        "true == 1;",
        "false == '';",
        "null == '';",
        "null == false;",
        "42 != '42';",
        "true != 0;",
        "1 == 1.0;",
        "1.0 == 1;",
        "1 != 1.0;",
        "1.0 != 1;",
    ];

    for code in invalid_cases {
        let full = format!("<?php $x = {};", code);
        let check_res = parse_and_check(&full);
        assert!(
            check_res.is_err(),
            "Expected compile error for mismatched types in '{}', but got Ok",
            code
        );
        match check_res.unwrap_err() {
            rhp::FrontendError::Type(err) => match err.kind {
                TypeErrorKind::InvalidBinaryOperation { op, left_type, right_type } => {
                    assert!(op == "==" || op == "!=");
                    assert_ne!(left_type, right_type);
                }
                other => panic!("Expected InvalidBinaryOperation for '{}', got {:?}", code, other),
            },
            other => panic!("Expected TypeError for '{}', got {:?}", code, other),
        }
    }
}

#[test]
fn test_primitive_loose_comparison_helpers() {
    let source = r#"
        // Bool::compare
        $b1 = Bool::compare(1, true);
        $b2 = Bool::compare(0, false);
        $b3 = Bool::compare('', false);
        $b4 = Bool::compare('1', true);
        $b5 = Bool::compare('0', false);
        $b6 = Bool::compare(null, false);
        $b7 = Bool::compare('hello', true);
        $b8 = Bool::compare(100, false);

        // Str::compare
        $s1 = Str::compare('hello', 'hello');
        $s2 = Str::compare('hello', 'HELLO');
        $s3 = Str::compare('hello', 'HELLO', true);
        $s4 = Str::compare(42, '42');
        $s5 = Str::compare(42, 43);

        // Int::compare
        $i1 = Int::compare(42, '42');
        $i2 = Int::compare('100', 100);
        $i3 = Int::compare('042', 42);
        $i4 = Int::compare(true, 1);
        $i5 = Int::compare(false, 0);
        $i6 = Int::compare(42, 43);

        // Float::compare
        $f1 = Float::compare(3.14, '3.14');
        $f2 = Float::compare(10, 10.0);
        $f3 = Float::compare(1.00001, 1.00002);
        $f4 = Float::compare(1.00001, 1.00002, 0.001);

        return [
            $b1, $b2, $b3, $b4, $b5, $b6, $b7, $b8,
            $s1, $s2, $s3, $s4, $s5,
            $i1, $i2, $i3, $i4, $i5, $i6,
            $f1, $f2, $f3, $f4,
        ];
    "#;

    let res = run_source(source).expect("Loose comparison helpers must execute cleanly");
    assert_eq!(
        res,
        PhpValue::list(vec![
            // Bool::compare
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            // Str::compare
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            // Int::compare
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            // Float::compare
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_array_static_comparison_helpers() {
    let source = r#"
        $list1 = [1, 2, 3];
        $list2 = [1, 2, 3];
        $list3 = [3, 2, 1];
        $list4 = [1, 2];

        // 1. JSON comparison
        $jsonSame = Array::compareJson($list1, $list2);
        $jsonDiff = Array::compareJson($list1, $list3);
        $jsonArrSame = Arr::compareJson($list1, $list2);

        // 2. Reference comparison
        $refA = [10, 20];
        $refB = $refA;
        $refC = [10, 20];
        $refSame = Array::compareRef($refA, $refB);
        $refDiff = Array::compareRef($refA, $refC);
        $refArrSame = Arr::compareRef($refA, $refB);

        // 3. Joined string comparison
        $joinedSame = Array::compareJoined([1, 2, 3], ['1', '2', '3']);
        $joinedCustomSep = Array::compareJoined([1, 2, 3], ['1', '2', '3'], '-');
        $joinedDiff = Array::compareJoined([1, 2, 3], [1, 2]);

        // 4. Unordered comparison (list and set)
        $set1 = Arr::set([1, 2, 3]);
        $set2 = Arr::set([3, 1, 2]);
        $unorderListList = Array::compareUnordered($list1, $list3);
        $unorderListSet = Array::compareUnordered($list1, $set2);
        $unorderSetSet = Array::compareUnordered($set1, $set2);
        $unorderDiff = Array::compareUnordered($list1, $list4);

        return [
            $jsonSame, $jsonDiff, $jsonArrSame,
            $refSame, $refDiff, $refArrSame,
            $joinedSame, $joinedCustomSep, $joinedDiff,
            $unorderListList, $unorderListSet, $unorderSetSet, $unorderDiff,
        ];
    "#;

    let res = run_source(source).expect("Array static comparison helpers must execute cleanly");
    assert_eq!(
        res,
        PhpValue::list(vec![
            // JSON
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            // Ref
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            // Joined
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            // Unordered
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
        ])
    );
}

#[test]
fn test_array_compare_unordered_rejects_dict() {
    let source = r#"
        $d1 = Arr::dict(['a' => 1]);
        $d2 = Arr::dict(['a' => 1]);
        Array::compareUnordered($d1, $d2);
    "#;

    let res = run_source(source);
    assert!(res.is_err(), "compareUnordered must reject dict with runtime error");
}

#[test]
fn test_float_comparison_checks_maximum_decimal_point() {
    let source = r#"
        // 1. Same decimal points matching
        $c1 = (1.23 == 1.23);
        $c2 = (3.14159 == 3.14159);
        $c3 = (0.0005 == 0.0005);

        // 2. Different values with same decimal points
        $c4 = (1.23 == 1.24);
        $c5 = (3.14159 == 3.14158);
        $c6 = (0.0005 == 0.0006);

        // 3. Different decimal points: checked up to maximum decimal point
        // 1.23 (2 decimals) vs 1.234 (3 decimals) -> max is 3 -> 1.230 != 1.234
        $c7 = (1.23 == 1.234);
        $c8 = (1.23 != 1.234);

        // 4. Zero decimal points
        $c9 = (10.0 == 10.0);
        $c10 = (10.0 == 10.1);
        $c11 = (10.0 != 10.1);

        // 5. Cross Float and Float::f64 (RustFloat) with matching decimal point
        $f32Val = 3.14;
        $f64Val = 3.14;
        $c12 = ($f32Val == $f64Val);

        // 6. Signed zero
        $c13 = (-0.0 == 0.0);

        return [
            $c1, $c2, $c3,
            $c4, $c5, $c6,
            $c7, $c8,
            $c9, $c10, $c11,
            $c12, $c13,
        ];
    "#;

    let res = run_source(source).expect("Float max-decimal comparisons must execute cleanly");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_strict_eq_and_hash_invariants() {
    use std::collections::{HashMap, HashSet};
    use std::hash::{Hash, Hasher};

    fn calculate_hash<T: Hash>(t: &T) -> u64 {
        let mut s = std::collections::hash_map::DefaultHasher::new();
        t.hash(&mut s);
        s.finish()
    }

    // 1. Int(1) and Float(1.0) must NOT be equal
    let i1 = PhpValue::Int(1);
    let f1 = PhpValue::Float(1.0);
    assert_ne!(i1, f1);
    assert!(!i1.strict_eq(&f1));
    assert!(!f1.strict_eq(&i1));

    // 2. HashMap must not confuse Int(1) and Float(1.0)
    let mut map = HashMap::new();
    map.insert(i1.clone(), "int_one");
    assert_eq!(map.get(&i1), Some(&"int_one"));
    assert_eq!(map.get(&f1), None);

    // 3. Values that are strictly equal must have identical hashes
    let f_zero = PhpValue::Float(0.0);
    let f_neg_zero = PhpValue::Float(-0.0);
    assert_eq!(f_zero, f_neg_zero);
    assert_eq!(calculate_hash(&f_zero), calculate_hash(&f_neg_zero));

    let mut set = HashSet::new();
    set.insert(f_zero);
    assert!(set.contains(&f_neg_zero));
}

