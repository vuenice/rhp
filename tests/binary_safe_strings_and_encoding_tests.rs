use rhp::run_source;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;

#[test]
fn test_underlying_raw_byte_representation_and_binary_safety() {
    clear_runtime_closure_macros();

    // 1. Raw binary payload containing null bytes and non-UTF-8 high bytes (e.g. 0x89, 0x00, 0xFF)
    let raw_bytes: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0xFF, 0xFE];
    let binary_str = PhpValue::bytes(raw_bytes.clone());

    assert_eq!(binary_str.raw_bytes().unwrap(), raw_bytes.as_slice());
    // Should not panic when accessed or formatted
    let display_str = binary_str.to_php_string();
    assert!(!display_str.is_empty());

    // 2. Length check via byte length and list of bytes
    let code = r#"
        $png = Str::fromBytes([137, 80, 78, 71, 13, 10, 26, 10, 0, 255]);
        $len = $png->byteLength();
        $isUtf = $png->isUtf8();
        $bytes = $png->bytes();
        return [$len, $isUtf, $bytes];
    "#;
    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(10));
        assert_eq!(b[1], PhpValue::Bool(false));
        if let PhpValue::List(bytes_list) = &b[2] {
            let bytes = bytes_list.borrow();
            assert_eq!(bytes.len(), 10);
            assert_eq!(bytes[0], PhpValue::Int(137));
            assert_eq!(bytes[1], PhpValue::Int(80)); // 'P'
            assert_eq!(bytes[8], PhpValue::Int(0));  // null byte
            assert_eq!(bytes[9], PhpValue::Int(255));
        } else {
            panic!("expected list of bytes");
        }
    } else {
        panic!("expected list return");
    }
}

#[test]
fn test_shift_jis_japanese_support_and_str_facade_constructors() {
    clear_runtime_closure_macros();

    // Shift-JIS bytes for "こんにちは世界"
    // こ: 0x82 0xB1
    // ん: 0x82 0xF1
    // に: 0x82 0xC9
    // ち: 0x82 0xBF
    // は: 0x82 0xCD
    // 世: 0x90 0xA2
    // 界: 0x8A 0x45
    let sjis_bytes: Vec<u8> = vec![
        0x82, 0xB1, 0x82, 0xF1, 0x82, 0xC9, 0x82, 0xBF, 0x82, 0xCD, 0x90, 0xA2, 0x8A, 0x45,
    ];

    // 1. Rust-level encoding module check
    let decoded = rhp::vm::encoding::decode_shift_jis(&sjis_bytes);
    assert_eq!(decoded, "こんにちは世界");

    let encoded = rhp::vm::encoding::encode_shift_jis("こんにちは世界");
    assert_eq!(encoded, sjis_bytes);

    // 2. Script execution with Str::fromEncoding and Str::utf8
    let code = r#"
        // 1. Standard string (UTF-8 safe raw bytes)
        $greeting = "こんにちは世界";

        // 2. Raw bytes array in Shift-JIS
        $rawBytes = Str::fromBytes([0x82, 0xB1, 0x82, 0xF1, 0x82, 0xC9, 0x82, 0xBF, 0x82, 0xCD, 0x90, 0xA2, 0x8A, 0x45]);

        // 3. Convert from Shift-JIS encoding facade constructor
        $jisText = Str::fromEncoding($rawBytes, 'Shift-JIS');

        // 4. Ensure UTF-8 compliance via explicit facade constructor
        $safeText = Str::utf8($jisText);

        // 5. Convert back to Shift-JIS bytes
        $backToJis = Str::toEncoding($greeting, 'Shift-JIS');
        $backBytes = $backToJis->bytes();

        return [$greeting, $jisText, $safeText, $backBytes];
    "#;
    let res = run_source(code).expect("script evaluation failed");

    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "こんにちは世界");
        assert_eq!(b[1].to_php_string(), "こんにちは世界");
        assert_eq!(b[2].to_php_string(), "こんにちは世界");

        if let PhpValue::List(items) = &b[3] {
            let bytes = items.borrow();
            assert_eq!(bytes.len(), sjis_bytes.len());
            for (i, &expected) in sjis_bytes.iter().enumerate() {
                assert_eq!(bytes[i], PhpValue::Int(expected as i32));
            }
        } else {
            panic!("expected list of bytes");
        }
    } else {
        panic!("expected list return");
    }
}

#[test]
fn test_euc_jp_and_iso_2022_jp_encodings() {
    clear_runtime_closure_macros();

    // EUC-JP for "日本語"
    // 日: 0xC6 0xFC (JIS 0x467C + 0x8080)
    // 本: 0xC3 0xDC (JIS 0x435C + 0x8080)
    // 語: 0xB8 0xEC (JIS 0x386C + 0x8080)
    let euc_bytes = vec![0xC6, 0xFC, 0xC3, 0xDC, 0xB8, 0xEC];
    let decoded_euc = rhp::vm::encoding::decode_euc_jp(&euc_bytes);
    assert_eq!(decoded_euc, "日本語");

    let code = r#"
        $eucRaw = Str::fromBytes([0xC6, 0xFC, 0xC3, 0xDC, 0xB8, 0xEC]);
        $japanese = Str::fromEncoding($eucRaw, 'EUC-JP');
        $eucEncoded = Str::toEncoding($japanese, 'EUC-JP');
        $eucBytes = $eucEncoded->bytes();

        $isoEncoded = Str::toEncoding('日本語', 'ISO-2022-JP');
        $isoDecoded = Str::fromEncoding($isoEncoded, 'ISO-2022-JP');

        return [$japanese, $isoDecoded];
    "#;
    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "日本語");
        assert_eq!(b[1].to_php_string(), "日本語");
    } else {
        panic!("expected list return");
    }
}

#[test]
fn test_immutable_string_helpers_and_explicit_reassignment() {
    clear_runtime_closure_macros();

    let code = r#"
        $text = "  hello world  ";

        // Method chaining returns a new instance; original $text is untouched
        $trimmed = $text->trim()->upper();

        // Check original was untouched
        $origUntouched = $text;

        // Explicit reassignment to update the variable in place cleanly
        $text = $text->trim()->upper();

        return [$origUntouched, $trimmed, $text];
    "#;
    let res = run_source(code).expect("script execution failed");

    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "  hello world  ");
        assert_eq!(b[1].to_php_string(), "HELLO WORLD");
        assert_eq!(b[2].to_php_string(), "HELLO WORLD");
    } else {
        panic!("expected list return");
    }
}

#[test]
fn test_string_fluent_chaining_and_transformations() {
    clear_runtime_closure_macros();

    let code = r#"
        $text = " Laravel Style Str Facade! ";
        $slug = $text->trim()->slug();
        $hasLaravel = $slug->contains("laravel");
        $starts = $slug->startsWith("laravel");
        $ends = $slug->endsWith("facade");
        $len = $slug->length();
        $replaced = $slug->replace("-", "_")->upper();
        $parts = $slug->split("-");
        $h = "hello";
        $reversed = $h->reverse();
        $abc = "abc";
        $repeated = $abc->repeat(3);
        $antigravity = "Antigravity";
        $sub = $antigravity->substr(0, 4);

        return [$slug, $hasLaravel, $starts, $ends, $replaced, $reversed, $repeated, $sub, $parts];
    "#;
    let res = run_source(code).expect("evaluation failed");

    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "laravel-style-str-facade");
        assert_eq!(b[1], PhpValue::Bool(true));
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(true));
        assert_eq!(b[4].to_php_string(), "LARAVEL_STYLE_STR_FACADE");
        assert_eq!(b[5].to_php_string(), "olleh");
        assert_eq!(b[6].to_php_string(), "abcabcabc");
        assert_eq!(b[7].to_php_string(), "Anti");

        if let PhpValue::List(items) = &b[8] {
            let p = items.borrow();
            assert_eq!(p.len(), 4);
            assert_eq!(p[0], PhpValue::string("laravel"));
            assert_eq!(p[3], PhpValue::string("facade"));
        } else {
            panic!("expected list of parts");
        }
    } else {
        panic!("expected list return");
    }
}

#[test]
fn test_bracket_indexing_and_byte_mutations() {
    clear_runtime_closure_macros();

    let code = r#"
        $s = "abcde";
        $first = $s[0];
        $last = $s[4];

        // Mutation via bracket assignment
        $s[0] = 'z';
        $modified = $s;

        return [$first, $last, $modified];
    "#;
    let res = run_source(code).expect("evaluation failed");

    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "a");
        assert_eq!(b[1].to_php_string(), "e");
        assert_eq!(b[2].to_php_string(), "zbcde");
    } else {
        panic!("expected list return");
    }
}

#[test]
fn test_str_macro_registration_and_fluent_execution() {
    clear_runtime_closure_macros();

    let code = r#"
        Str::macro('shout', fn() => $this->trim()->upper() . '!!!');
        $greeting = "  welcome to rhp  ";
        $shouted = $greeting->shout();
        return $shouted;
    "#;
    let res = run_source(code).expect("macro test failed");
    assert_eq!(res.to_php_string(), "WELCOME TO RHP!!!");
}
