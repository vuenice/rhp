use rhp::{run_source, PhpValue};

#[test]
fn test_range_operator_standalone() {
    let source = r#"
        $nums = 0..5;
        return $nums;
    "#;
    let result = run_source(source).expect("Range operator 0..5 should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(0),
            PhpValue::Int(1),
            PhpValue::Int(2),
            PhpValue::Int(3),
            PhpValue::Int(4),
        ])
    );
}

#[test]
fn test_range_operator_descending() {
    let source = r#"
        $nums = Arr::range(5, 2, -1);
        return $nums;
    "#;
    let result = run_source(source).expect("Descending range 5..2 with -1 step should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(5),
            PhpValue::Int(4),
            PhpValue::Int(3),
        ])
    );
}

#[test]
fn test_range_operator_characters() {
    let source = r#"
        $chars = 'a'..'d';
        return $chars;
    "#;
    let result = run_source(source).expect("Character range 'a'..'d' should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("a"),
            PhpValue::string("b"),
            PhpValue::string("c"),
        ])
    );
}

#[test]
fn test_range_inside_array_literal() {
    let source = r#"
        $arr = [0..5];
        return $arr;
    "#;
    let result = run_source(source).expect("[0..5] should unpack into array");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(0),
            PhpValue::Int(1),
            PhpValue::Int(2),
            PhpValue::Int(3),
            PhpValue::Int(4),
        ])
    );
}

#[test]
fn test_range_mixed_in_array_literal() {
    let source = r#"
        $arr = [100, 1..3, 200];
        return $arr;
    "#;
    let result = run_source(source).expect("[100, 1..3, 200] should expand properly");
    assert_eq!(
        result,
        PhpValue::array(vec![
            PhpValue::Int(100),
            PhpValue::Int(1),
            PhpValue::Int(2),
            PhpValue::Int(200),
        ])
    );
}

#[test]
fn test_for_in_range_array() {
    let source = r#"
        $collected = [];
        for ($i in [0..5]) {
            $collected[] = $i;
        }
        return $collected;
    "#;
    let result = run_source(source).expect("for ($i in [0..5]) should execute");
    assert_eq!(
        result,
        PhpValue::array(vec![
            PhpValue::Int(0),
            PhpValue::Int(1),
            PhpValue::Int(2),
            PhpValue::Int(3),
            PhpValue::Int(4),
        ])
    );
}

#[test]
fn test_for_in_bare_range() {
    let source = r#"
        $sum = 0;
        for ($i in 1..4) {
            $sum += $i;
        }
        return $sum;
    "#;
    let result = run_source(source).expect("for ($i in 1..4) should execute");
    assert_eq!(result, PhpValue::Int(6));
}

#[test]
fn test_for_in_items_collection() {
    let source = r#"
        $items = ["apple", "banana", "cherry"];
        $out = [];
        for ($item in $items) {
            $out[] = $item;
        }
        return $out;
    "#;
    let result = run_source(source).expect("for ($item in $items) should execute");
    assert_eq!(
        result,
        PhpValue::array(vec![
            PhpValue::string("apple"),
            PhpValue::string("banana"),
            PhpValue::string("cherry"),
        ])
    );
}

#[test]
fn test_for_in_key_value() {
    let source = r#"
        $items = ["x" => 10, "y" => 20];
        $keys = [];
        $values = [];
        for ($k => $v in $items) {
            $keys[] = $k;
            $values[] = $v;
        }
        return [$keys, $values];
    "#;
    let result = run_source(source).expect("for ($k => $v in $items) should execute");
    assert_eq!(
        result,
        PhpValue::array(vec![
            PhpValue::array(vec![PhpValue::string("x"), PhpValue::string("y")]),
            PhpValue::array(vec![PhpValue::Int(10), PhpValue::Int(20)]),
        ])
    );
}

#[test]
fn test_rust_style_loop_and_break_with_single_line_if() {
    let source = r#"
        $i = 0;
        loop {
            $i++;
            if ($i > 10) break;
        }
        return $i;
    "#;
    let result = run_source(source).expect("Rust-style loop should execute cleanly");
    assert_eq!(result, PhpValue::Int(11));
}

#[test]
fn test_if_without_parentheses_is_rejected() {
    let source = r#"
        $i = 0;
        loop {
            $i++;
            if $i > 5 break;
        }
        return $i;
    "#;
    assert!(run_source(source).is_err(), "if without parentheses should be rejected");
}

#[test]
fn test_for_without_parentheses_is_rejected() {
    let source = r#"
        for $i in [0..5] {
            print($i);
        }
    "#;
    assert!(run_source(source).is_err(), "for without parentheses should be rejected");
}

#[test]
fn test_single_line_if_else() {
    let source = r#"
        $x = 42;
        $msg = "";
        if ($x > 50)
            $msg = "greater";
        else
            $msg = "smaller";
        return $msg;
    "#;
    let result = run_source(source).expect("single line if-else should execute");
    assert_eq!(result, PhpValue::string("smaller"));
}

#[test]
fn test_loop_with_continue() {
    let source = r#"
        $sum = 0;
        $i = 0;
        loop {
            $i++;
            if ($i % 2 === 0) continue;
            if ($i > 10) break;
            $sum += $i;
        }
        return $sum;
    "#;
    let result = run_source(source).expect("loop with continue and break should execute");
    // $i = 1 (1), 3 (4), 5 (9), 7 (16), 9 (25), 11 (breaks)
    assert_eq!(result, PhpValue::Int(25));
}

#[test]
fn test_break_and_continue_in_for_in_loop() {
    let source = r#"
        $sum = 0;
        for ($i in [0..20]) {
            if ($i % 2 !== 0) continue;
            if ($i >= 10) break;
            $sum += $i;
        }
        return $sum;
    "#;
    let result = run_source(source).expect("for-in with break and continue should execute");
    // evens < 10: 0 + 2 + 4 + 6 + 8 = 20
    assert_eq!(result, PhpValue::Int(20));
}

#[test]
fn test_nested_loop_and_multi_level_break() {
    let source = r#"
        $count = 0;
        loop {
            loop {
                $count++;
                if ($count >= 5) break 2;
            }
        }
        return $count;
    "#;
    let result = run_source(source).expect("nested loop with break 2 should execute");
    assert_eq!(result, PhpValue::Int(5));
}

#[test]
fn test_user_requested_exact_code_snippets() {
    let source = r#"
        $out1 = [];
        for ($i in [0..10]){
            $out1[] = $i;
        }

        $items = ["a", "b", "c"];
        $out2 = [];
        for ($item in $items){
            $out2[] = $item;
        }

        $i = 0;
        loop {
            $i++;
            if($i>10) break;
        }

        return [$out1, $out2, $i];
    "#;
    let result = run_source(source).expect("Exact code snippets from prompt must execute cleanly");
    assert_eq!(
        result,
        PhpValue::array(vec![
            PhpValue::array((0..10).map(PhpValue::Int).collect()),
            PhpValue::array(vec![
                PhpValue::string("a"),
                PhpValue::string("b"),
                PhpValue::string("c"),
            ]),
            PhpValue::Int(11),
        ])
    );
}

#[test]
fn test_arr_range_pythonic_behavior() {
    let source = r#"
        $stopOnly = Arr::range(5);
        $startStop = Arr::range(2, 6);
        $stepJump = Arr::range(0, 10, 2);
        $negativeStep = Arr::range(10, 0, -2);
        $emptyRange = Arr::range(5, 2);
        return [$stopOnly, $startStop, $stepJump, $negativeStep, $emptyRange];
    "#;
    let result = run_source(source).expect("Arr::range pythonic tests should succeed");
    assert_eq!(
        result,
        PhpValue::array(vec![
            PhpValue::list(vec![
                PhpValue::Int(0),
                PhpValue::Int(1),
                PhpValue::Int(2),
                PhpValue::Int(3),
                PhpValue::Int(4),
            ]),
            PhpValue::list(vec![
                PhpValue::Int(2),
                PhpValue::Int(3),
                PhpValue::Int(4),
                PhpValue::Int(5),
            ]),
            PhpValue::list(vec![
                PhpValue::Int(0),
                PhpValue::Int(2),
                PhpValue::Int(4),
                PhpValue::Int(6),
                PhpValue::Int(8),
            ]),
            PhpValue::list(vec![
                PhpValue::Int(10),
                PhpValue::Int(8),
                PhpValue::Int(6),
                PhpValue::Int(4),
                PhpValue::Int(2),
            ]),
            PhpValue::list(vec![]),
        ])
    );
}

#[test]
fn test_range_syntax_step_jump() {
    let source = r#"
        $step1 = [0..10..2];
        $step2 = [10..0..-3];
        return [$step1, $step2];
    "#;
    let result = run_source(source).expect("[0..10..2] range step jump should succeed");
    assert_eq!(
        result,
        PhpValue::array(vec![
            PhpValue::list(vec![
                PhpValue::Int(0),
                PhpValue::Int(2),
                PhpValue::Int(4),
                PhpValue::Int(6),
                PhpValue::Int(8),
            ]),
            PhpValue::list(vec![
                PhpValue::Int(10),
                PhpValue::Int(7),
                PhpValue::Int(4),
                PhpValue::Int(1),
            ]),
        ])
    );
}

#[test]
fn test_arr_range_zero_step_error() {
    let source = r#"
        return Arr::range(0, 10, 0);
    "#;
    assert!(run_source(source).is_err());
}
