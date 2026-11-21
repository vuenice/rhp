use rhp::{parse_and_check, run_source, FrontendError, PhpValue, TypeErrorKind};
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::lint::migrate::{replace_type_casts, check_rune_and_ctype_usage};

#[test]
fn test_rune_static_facade_creation() {
    clear_runtime_closure_macros();

    let code = r#"
        $a = Rune::from(65);
        $rocket = Rune::from(0x1F680);
        $valid_try = Rune::tryFrom(66);
        $invalid_try = Rune::tryFrom(0xD800);
        $first = Rune::ofFirst("hello");
        $first_emoji = Rune::ofFirst("🚀 world");

        return [
            $a->toInt(),
            $rocket->toInt(),
            $valid_try->toInt(),
            $invalid_try,
            $first->toString(),
            $first_emoji->toString(),
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(65));
        assert_eq!(b[1], PhpValue::Int(0x1F680));
        assert_eq!(b[2], PhpValue::Int(66));
        assert_eq!(b[3], PhpValue::Null);
        assert_eq!(b[4], PhpValue::string("h"));
        assert_eq!(b[5], PhpValue::string("🚀"));
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_rune_invalid_creation_exceptions() {
    clear_runtime_closure_macros();

    // 1. Surrogate rejection
    let surrogate_code = r#"
        return Rune::from(0xD800);
    "#;
    let err = run_source(surrogate_code).expect_err("expected exception for surrogate codepoint");
    let err_str = format!("{:?}", err);
    assert!(err_str.contains("InvalidScalarValueException"));

    // 2. Negative codepoint
    let neg_code = r#"
        return Rune::from(-1);
    "#;
    let err = run_source(neg_code).expect_err("expected exception for negative codepoint");
    let err_str = format!("{:?}", err);
    assert!(err_str.contains("InvalidScalarValueException"));

    // 3. Beyond Unicode maximum 0x10FFFF
    let overflow_code = r#"
        return Rune::from(0x110000);
    "#;
    let err = run_source(overflow_code).expect_err("expected exception for codepoint > 0x10FFFF");
    let err_str = format!("{:?}", err);
    assert!(err_str.contains("InvalidScalarValueException"));

    // 4. ofFirst with empty string
    let empty_code = r#"
        return Rune::ofFirst("");
    "#;
    let err = run_source(empty_code).expect_err("expected exception for empty string in ofFirst");
    let err_str = format!("{:?}", err);
    assert!(err_str.contains("InvalidArgumentException"));
}

#[test]
fn test_rune_fluent_methods() {
    clear_runtime_closure_macros();

    let code = r#"
        $alpha = Rune::from(65); // 'A'
        $digit = Rune::from(53); // '5'
        $space = Rune::from(32); // ' '
        $rocket = Rune::from(0x1F680); // '🚀'
        $eszett = Rune::from(223); // 'ß'
        $cjk = Rune::ofFirst("你");

        return [
            $alpha->isAlpha(),
            $digit->isDigit(),
            $space->isWhitespace(),
            $alpha->isAscii(),
            $rocket->isAscii(),
            $alpha->isUpper(),
            $alpha->isLower(),
            $alpha->utf8Len(),
            $rocket->utf8Len(),
            $eszett->toUpper(),
            $alpha->toLower(),
            $alpha->width(),
            $cjk->width(),
            $rocket->width(),
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(true));
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(true));
        assert_eq!(b[4], PhpValue::Bool(false));
        assert_eq!(b[5], PhpValue::Bool(true));
        assert_eq!(b[6], PhpValue::Bool(false));
        assert_eq!(b[7], PhpValue::Int(1));
        assert_eq!(b[8], PhpValue::Int(4));
        assert_eq!(b[9], PhpValue::string("SS")); // German eszett uppercases to "SS"
        assert_eq!(b[10], PhpValue::string("a"));
        assert_eq!(b[11], PhpValue::Int(1)); // 'A' width = 1
        assert_eq!(b[12], PhpValue::Int(2)); // '你' width = 2
        assert_eq!(b[13], PhpValue::Int(2)); // '🚀' width = 2
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_str_runes_and_bytes() {
    clear_runtime_closure_macros();

    let code = r#"
        $str = "Hi 🚀";
        $runes = $str->runes();
        $rune_len = $str->runeLen();
        $byte_len = $str->byteLen();
        $reconstructed = Str::fromRunes($runes);
        $first_byte = Str::byteAt("ABC", 0);
        $second_byte = string("ABC")->byteAt(1);
        $byte_str = Str::fromByte(65);

        return [
            $rune_len,
            $byte_len,
            $reconstructed,
            $first_byte,
            $second_byte,
            $byte_str,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(4)); // "Hi 🚀" has 4 runes: 'H', 'i', ' ', '🚀'
        assert_eq!(b[1], PhpValue::Int(7)); // 1 + 1 + 1 + 4 = 7 bytes
        assert_eq!(b[2], PhpValue::string("Hi 🚀"));
        assert_eq!(b[3], PhpValue::Int(65));
        assert_eq!(b[4], PhpValue::Int(66));
        assert_eq!(b[5], PhpValue::bytes(vec![65]));
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_rune_type_hint_contract() {
    clear_runtime_closure_macros();

    let code = r#"
        function getCode($r: rune): int {
            return $r->toInt();
        }

        $r = Rune::from(65);
        return getCode($r);
    "#;

    let res = run_source(code).expect("evaluation failed");
    assert_eq!(res, PhpValue::Int(65));
}

#[test]
fn test_rune_strict_type_checking() {
    // 1. $r + 1 is rejected at compile time (no implicit arithmetic on runes)
    let arith_code = r#"
        function addOne($r: rune): int {
            return $r + 1;
        }
    "#;
    let err = parse_and_check(arith_code).expect_err("expected type error on rune + int");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::Custom { message } => {
                assert!(message.contains("Cannot concatenate or add rune"));
            }
            other => panic!("expected Custom error for rune '+', got {:?}", other),
        },
        other => panic!("expected TypeError, got {:?}", other),
    }

    // 2. $r == "a" is rejected at compile time (cannot compare rune to string)
    let cmp_code = r#"
        function isA($r: rune): bool {
            return $r == "a";
        }
    "#;
    let err = parse_and_check(cmp_code).expect_err("expected type error on rune == string");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::InvalidBinaryOperation { op, left_type, right_type } => {
                assert_eq!(op, "==");
                assert_eq!(left_type, "rune");
                assert_eq!(right_type, "string");
            }
            other => panic!("expected InvalidBinaryOperation, got {:?}", other),
        },
        other => panic!("expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_rune_json_and_debug_type() {
    clear_runtime_closure_macros();

    let code = r#"
        $r = Rune::from(65);
        $type = Debug::type($r);
        $json = json_encode(['char' => $r]);

        return [$type, $json];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::string("rune"));
        assert_eq!(b[1], PhpValue::string("{\"char\":\"A\"}"));
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_legacy_char_bridging() {
    clear_runtime_closure_macros();

    let code = r#"
        $r1 = mb_ord("hello");
        $r2 = mb_chr(65);
        $w = mb_strwidth("你好");
        $b = ord("A");
        $c = chr(65);

        return [
            $r1->toInt(),
            $r2->toString(),
            $w,
            $b,
            $c,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(104)); // 'h' = 104
        assert_eq!(b[1], PhpValue::string("A"));
        assert_eq!(b[2], PhpValue::Int(2));
        assert_eq!(b[3], PhpValue::Int(65));
        assert_eq!(b[4], PhpValue::bytes(vec![65]));
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_rune_migration_tooling() {
    // 1. Parenthesized cast (rune)$x -> rune($x)
    let src = "function test($x) { $r = (rune)$x; }";
    let (migrated, count) = replace_type_casts(src);
    assert_eq!(count, 1);
    assert_eq!(migrated, "function test($x) { $r = rune($x); }");

    // 2. ctype_* usage notice
    let ctype_src = "if (ctype_digit($str)) { echo 'digits'; }";
    let warnings = check_rune_and_ctype_usage(ctype_src);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("ctype_digit"));
    assert!(warnings[0].contains("Rune::is*"));
}

#[test]
fn test_rune_static_facade_helpers() {
    clear_runtime_closure_macros();

    let code = r#"
        $r = Rune::from(65);
        return [
            Rune::toInt($r),
            Rune::toString($r),
            Rune::utf8Len($r),
            Rune::isAlpha($r),
            Rune::isDigit($r),
            Rune::width($r),
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(65));
        assert_eq!(b[1], PhpValue::string("A"));
        assert_eq!(b[2], PhpValue::Int(1));
        assert_eq!(b[3], PhpValue::Bool(true));
        assert_eq!(b[4], PhpValue::Bool(false));
        assert_eq!(b[5], PhpValue::Int(1));
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_from_runes_type_error() {
    clear_runtime_closure_macros();

    let code = r#"
        return Str::fromRunes([65, 66]); // Ints instead of runes
    "#;
    let err = run_source(code).expect_err("expected TypeError on non-rune in Str::fromRunes");
    assert!(format!("{:?}", err).contains("TypeError"));
}

#[test]
fn test_byte_at_out_of_bounds() {
    clear_runtime_closure_macros();

    let code = r#"
        return Str::byteAt("abc", 10);
    "#;
    let err = run_source(code).expect_err("expected out of bounds on Str::byteAt");
    assert!(format!("{:?}", err).contains("UndefinedIndex"));
}

#[test]
fn test_from_byte_invalid_range() {
    clear_runtime_closure_macros();

    let code = r#"
        return Str::fromByte(300);
    "#;
    let err = run_source(code).expect_err("expected out of range on Str::fromByte(300)");
    assert!(format!("{:?}", err).contains("InvalidArgumentException"));
}

#[test]
fn test_rune_for_in_loop() {
    clear_runtime_closure_macros();

    let code = r#"
        $str = "abc";
        $ints = [];
        for ($r in $str->runes()) {
            $ints[] = $r->toInt();
        }
        return $ints;
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(97));
        assert_eq!(b[1], PhpValue::Int(98));
        assert_eq!(b[2], PhpValue::Int(99));
    } else {
        panic!("expected array/list result");
    }
}

