use rhp::run_source;
use rhp::vm::value::PhpValue;

#[test]
fn test_literal_parsing_utf8_and_bytes() {
    // 1. "\u{1F600}" is valid UTF-8
    let code = r#"
        $emoji = "\u{1F600}";
        return $emoji;
    "#;
    let res = run_source(code).expect("should parse valid unicode escape");
    assert_eq!(res.to_php_string(), "😀");
    assert!(res.is_utf8());

    // 2. b"\xFF" is valid Bytes
    let code_bytes = r#"
        $b = b"\xFF\xFE\x00\x01";
        return $b;
    "#;
    let res_bytes = run_source(code_bytes).expect("should parse raw bytes");
    assert!(res_bytes.is_bytes());
    assert_eq!(res_bytes.raw_bytes().unwrap(), &[0xFF, 0xFE, 0x00, 0x01]);

    // 3. "\xFF" in normal string produces a compile error with hint
    let invalid_utf8_code = r#"
        $s = "\xFF";
        return $s;
    "#;
    let err = run_source(invalid_utf8_code).expect_err("should reject invalid UTF-8 escape");
    let err_str = format!("{:?}", err);
    assert!(
        err_str.contains("Invalid UTF-8 escape") && err_str.contains("bytes literal"),
        "error message should hint to b\"...\", got: {}",
        err_str
    );

    // 4. b"\u{1F600}" in bytes produces a compile error
    let invalid_bytes_code = r#"
        $b = b"\u{1F600}";
        return $b;
    "#;
    let err_b = run_source(invalid_bytes_code).expect_err("should reject unicode escape in bytes");
    let err_b_str = format!("{:?}", err_b);
    assert!(
        err_b_str.contains("Unicode escape") || err_b_str.contains("not allowed in bytes"),
        "error should forbid unicode escape in bytes, got: {}",
        err_b_str
    );
}

#[test]
fn test_bytes_non_interpolation() {
    let code = r#"
        $name = "world";
        $b = b"hello $name {$name}";
        return $b;
    "#;
    let res = run_source(code).expect("evaluation failed");
    assert!(res.is_bytes());
    assert_eq!(res.raw_bytes().unwrap(), b"hello $name {$name}");
}

#[test]
fn test_string_concatenation_strictness() {
    // 1. Utf8 + Utf8 works
    let code = r#"
        $a = "hello ";
        $b = "world";
        return $a + $b;
    "#;
    let res = run_source(code).expect("concatenation should succeed");
    assert_eq!(res.to_php_string(), "hello world");
    assert!(res.is_utf8());

    // 2. Bytes + Bytes throws TypeError
    let code_bb = r#"
        $a = b"foo";
        $b = b"bar";
        return $a + $b;
    "#;
    let err_bb = run_source(code_bb).expect_err("Bytes + Bytes should fail");
    let msg = format!("{:?}", err_bb);
    assert!(
        msg.contains("Cannot concatenate bytes using the '+' operator"),
        "got: {}",
        msg
    );

    // 3. Utf8 + Bytes throws TypeError
    let code_sb = r#"
        $a = "foo";
        $b = b"bar";
        return $a + $b;
    "#;
    let err_sb = run_source(code_sb).expect_err("Utf8 + Bytes should fail");
    let msg_sb = format!("{:?}", err_sb);
    assert!(
        msg_sb.contains("Cannot concatenate string and bytes"),
        "got: {}",
        msg_sb
    );

    // 4. Bytes + Utf8 throws TypeError
    let code_bs = r#"
        $a = b"foo";
        $b = "bar";
        return $a + $b;
    "#;
    let err_bs = run_source(code_bs).expect_err("Bytes + Utf8 should fail");
    let msg_bs = format!("{:?}", err_bs);
    assert!(
        msg_bs.contains("Cannot concatenate string and bytes"),
        "got: {}",
        msg_bs
    );

    // 5. Dot concatenation operator is removed and produces compile error
    let code_dot = r#"
        $a = "hello ";
        $b = "world";
        return $a . $b;
    "#;
    let err_dot = run_source(code_dot).expect_err("Dot concat should fail");
    let msg_dot = format!("{:?}", err_dot);
    assert!(
        msg_dot.contains("The '.' concatenation operator has been removed. Use '+' for string concatenation."),
        "got: {}",
        msg_dot
    );
}

#[test]
fn test_constructors_str_utf8_and_str_bytes() {
    // 1. Str::utf8 on valid bytes converts to Utf8
    let code = r#"
        $b = b"valid text";
        $u = Str::utf8($b);
        return [$u, $u->len()];
    "#;
    let res = run_source(code).expect("Str::utf8 should convert valid bytes");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert!(b[0].is_utf8());
        assert_eq!(b[0].to_php_string(), "valid text");
        assert_eq!(b[1], PhpValue::Int(10));
    } else {
        panic!("expected list");
    }

    // 2. Str::utf8 on invalid bytes throws InvalidEncodingException
    let code_invalid = r#"
        $b = b"\xFF\xFE";
        return Str::utf8($b);
    "#;
    let err = run_source(code_invalid).expect_err("Str::utf8 should fail on invalid bytes");
    let err_str = format!("{:?}", err);
    assert!(
        err_str.contains("InvalidEncodingException"),
        "expected InvalidEncodingException, got: {}",
        err_str
    );

    // 3. Str::bytes on Utf8 converts to Bytes
    let code_to_bytes = r#"
        $s = "hello";
        $b = Str::bytes($s);
        return $b;
    "#;
    let res_b = run_source(code_to_bytes).expect("Str::bytes should convert Utf8 to Bytes");
    assert!(res_b.is_bytes());
    assert_eq!(res_b.raw_bytes().unwrap(), b"hello");

    // 4. Str::bytes on Bytes is a no-op / passthrough
    let code_passthrough = r#"
        $b1 = b"binary \x00 data";
        $b2 = Str::bytes($b1);
        return $b2;
    "#;
    let res_p = run_source(code_passthrough).expect("Str::bytes on bytes is a passthrough");
    assert!(res_p.is_bytes());
    assert_eq!(res_p.raw_bytes().unwrap(), b"binary \x00 data");
}

#[test]
fn test_type_hints_string_and_bytes() {
    // 1. function foo($s: string) accepts Utf8
    let code_ok = r#"
        function takeString($s: string): string {
            return $s;
        }
        return takeString("hello");
    "#;
    let res = run_source(code_ok).expect("takeString with Utf8 should succeed");
    assert_eq!(res.to_php_string(), "hello");

    // 2. function foo($s: string) rejects Bytes
    let code_reject_bytes = r#"
        function takeString($s: string): string {
            return $s;
        }
        return takeString(b"hello");
    "#;
    let err = run_source(code_reject_bytes).expect_err("takeString should reject Bytes");
    let err_str = format!("{:?}", err);
    assert!(
        err_str.contains("TypeError") || err_str.contains("must be of type string"),
        "expected TypeError, got: {}",
        err_str
    );

    // 3. function bar($b: bytes) accepts Bytes
    let code_bytes_ok = r#"
        function takeBytes($b: bytes): int {
            return $b->len();
        }
        return takeBytes(b"foobar");
    "#;
    let res_b = run_source(code_bytes_ok).expect("takeBytes with Bytes should succeed");
    assert_eq!(res_b, PhpValue::Int(6));

    // 4. function bar($b: bytes) rejects string variable
    let code_reject_string = r#"
        function takeBytes($b: bytes): int {
            return $b->len();
        }
        $s: string = "foobar";
        return takeBytes($s);
    "#;
    let err_b = run_source(code_reject_string).expect_err("takeBytes should reject string");
    let err_b_str = format!("{:?}", err_b);
    assert!(
        err_b_str.contains("TypeError") || err_b_str.contains("must be of type bytes"),
        "expected TypeError, got: {}",
        err_b_str
    );
}

#[test]
fn test_bytes_minimal_api_and_unsupported_method_rejection() {
    // 1. $b->len() and $b->slice() (JS String.slice: end is exclusive)
    let code_api = r#"
        $b = b"abcdefghij";
        $len = $b->len();
        $slice1 = $b->slice(2, 6);
        $slice2 = $b->slice(-4);
        return [$len, $slice1->decode(), $slice2->decode()];
    "#;
    let res = run_source(code_api).expect("bytes len and slice should work");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(10));
        assert_eq!(b[1].to_php_string(), "cdef");
        assert_eq!(b[2].to_php_string(), "ghij");
    } else {
        panic!("expected list");
    }

    // 2. $b->decode('UTF-8')
    let code_decode = r#"
        $b = b"Hello, World!";
        $s = $b->decode();
        return $s;
    "#;
    let res_dec = run_source(code_decode).expect("decode should work");
    assert_eq!(res_dec.to_php_string(), "Hello, World!");
    assert!(res_dec.is_utf8());

    // 3. $b->decode on invalid UTF-8 throws InvalidEncodingException
    let code_decode_fail = r#"
        $b = b"\xFF\xFE\xFD";
        return $b->decode();
    "#;
    let err_dec = run_source(code_decode_fail).expect_err("strict decode should fail");
    let err_dec_str = format!("{:?}", err_dec);
    assert!(
        err_dec_str.contains("InvalidEncodingException"),
        "expected InvalidEncodingException, got: {}",
        err_dec_str
    );

    // 4. Calling unsupported string method on Bytes throws TypeError with exact message
    let code_unsupported = r#"
        $b = b"hello";
        return $b->trim();
    "#;
    let err_unsupported = run_source(code_unsupported).expect_err("trim on bytes should fail");
    let err_msg = format!("{:?}", err_unsupported);
    assert!(
        err_msg.contains("Method trim() is not supported on bytes. Call ->decode() first to get a string."),
        "expected exact error message, got: {}",
        err_msg
    );

    let code_unsupported2 = r#"
        $b = b"hello";
        return $b->slug();
    "#;
    let err_unsupported2 = run_source(code_unsupported2).expect_err("slug on bytes should fail");
    let err_msg2 = format!("{:?}", err_unsupported2);
    assert!(
        err_msg2.contains("Method slug() is not supported on bytes. Call ->decode() first to get a string."),
        "expected exact error message, got: {}",
        err_msg2
    );
}

#[test]
fn test_multibyte_functions_and_ascii_fast_path() {
    let code = r#"
        // 1. ASCII string: O(1) fast paths
        $ascii = "abcdef";
        $len1 = $ascii->len();
        $mblen1 = $ascii->length();
        $grap1 = $ascii->graphemeLen();
        $sub1 = $ascii->substr(1, 3);
        $sub_neg = $ascii->substr(-4, 2);

        // 2. Multibyte string: UTF-8 characters
        $mb = "héllo 世界 😀";
        $len2 = $mb->len();              // byte count: 1 + 2 + 3 + 1 + 3 + 3 + 1 + 4 = 18
        $mblen2 = $mb->length();         // char count: 10
        $grap2 = $mb->graphemeLen();     // grapheme count: 10
        $sub2 = $mb->substr(6, 2);       // "世界"
        $sub3 = $mb->substr(-1);         // "😀"

        // 3. Combining character grapheme
        $combining = "e\u{0301}";         // 'é' as combining character
        $comb_mb = $combining->length();  // 2 code points
        $comb_grap = $combining->graphemeLen(); // 1 grapheme cluster

        return [
            $len1, $mblen1, $grap1, $sub1, $sub_neg,
            $len2, $mblen2, $grap2, $sub2, $sub3,
            $comb_mb, $comb_grap
        ];
    "#;
    let res = run_source(code).expect("multibyte functions should work");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(6));
        assert_eq!(b[1], PhpValue::Int(6));
        assert_eq!(b[2], PhpValue::Int(6));
        assert_eq!(b[3].to_php_string(), "bcd");
        assert_eq!(b[4].to_php_string(), "cd");

        assert_eq!(b[5], PhpValue::Int(18));
        assert_eq!(b[6], PhpValue::Int(10));
        assert_eq!(b[7], PhpValue::Int(10));
        assert_eq!(b[8].to_php_string(), "世界");
        assert_eq!(b[9].to_php_string(), "😀");

        assert_eq!(b[10], PhpValue::Int(2));
        assert_eq!(b[11], PhpValue::Int(1));
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_encoding_conversions_roundtrip_and_unmappable() {
    let code = r#"
        $text = "こんにちは世界";

        // 1. Convert UTF-8 to Shift-JIS bytes
        $sjis = Str::convertEncoding($text, 'Shift-JIS', 'UTF-8');
        // 2. Convert Shift-JIS back to UTF-8 string via decode
        $decoded = $sjis->decode('Shift-JIS');

        // 3. Convert UTF-8 to UTF-16LE bytes
        $u16 = Str::convertEncoding($text, 'UTF-16LE', 'UTF-8');
        $u16_back = $u16->decode('UTF-16LE');

        // 4. Convert UTF-8 to UTF-32LE bytes
        $u32 = Str::convertEncoding($text, 'UTF-32LE', 'UTF-8');
        $u32_back = $u32->decode('UTF-32LE');

        return [$decoded, $u16_back, $u32_back];
    "#;
    let res = run_source(code).expect("roundtrip encoding conversions should work");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "こんにちは世界");
        assert_eq!(b[1].to_php_string(), "こんにちは世界");
        assert_eq!(b[2].to_php_string(), "こんにちは世界");
    } else {
        panic!("expected list");
    }

    // Unmappable character tests:
    // Emoji 😀 cannot be encoded in Shift-JIS
    // 1. onUnmappable = 'throw' (default)
    let code_throw = r#"
        $emoji = "Hello 😀";
        return Str::convertEncoding($emoji, 'Shift-JIS', 'UTF-8', 'throw');
    "#;
    let err_throw = run_source(code_throw).expect_err("should throw on unmappable char");
    let err_str = format!("{:?}", err_throw);
    assert!(
        err_str.contains("InvalidEncodingException"),
        "expected InvalidEncodingException, got: {}",
        err_str
    );

    // 2. onUnmappable = 'skip'
    let code_skip = r#"
        $emoji = "Hello 😀 World";
        $bytes = Str::convertEncoding($emoji, 'Shift-JIS', 'UTF-8', 'skip');
        return $bytes->decode('Shift-JIS');
    "#;
    let res_skip = run_source(code_skip).expect("skip should drop unmappable chars");
    assert_eq!(res_skip.to_php_string(), "Hello  World");
}

#[test]
fn test_detect_encoding() {
    let code = r#"
        // 1. UTF-8 BOM
        $utf8_bom = b"\xEF\xBB\xBFHello";
        $d1 = Str::detectEncoding($utf8_bom);

        // 2. UTF-16LE BOM
        $u16le_bom = b"\xFF\xFEH\x00e\x00l\x00l\x00o\x00";
        $d2 = Str::detectEncoding($u16le_bom);

        // 3. UTF-32BE BOM
        $u32be_bom = b"\x00\x00\xFE\xFF\x00\x00\x00H";
        $d3 = Str::detectEncoding($u32be_bom);

        // 4. Valid UTF-8 without BOM
        $utf8_plain = "Simple UTF-8 text with é and 日本語";
        $d4 = Str::detectEncoding($utf8_plain);

        // 5. Shift-JIS with candidates list
        $sjis_bytes = b"\x82\xB1\x82\xF1\x82\xC9\x82\xBF\x82\xCD";
        $d5 = Str::detectEncoding($sjis_bytes, ['UTF-8', 'Shift_JIS', 'ISO-8859-1']);

        return [$d1, $d2, $d3, $d4, $d5];
    "#;
    let res = run_source(code).expect("encoding detection should work");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "UTF-8");
        assert_eq!(b[1].to_php_string(), "UTF-16LE");
        assert_eq!(b[2].to_php_string(), "UTF-32BE");
        assert_eq!(b[3].to_php_string(), "UTF-8");
        assert_eq!(b[4].to_php_string(), "Shift_JIS");
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_file_io_strict_lossy_and_bytes() {
    let temp_dir = std::env::temp_dir();
    let file_path = temp_dir.join("rhp_test_string_subtypes.bin");
    let file_path_str = file_path.to_str().unwrap().replace('\\', "/");

    // Write raw invalid UTF-8 bytes: "hello \xFF\xFE world"
    std::fs::write(&file_path, b"hello \xFF\xFE world").unwrap();

    // 1. File::read with strict (default) throws InvalidEncodingException
    let code_strict = format!(
        r#"
            return File::read('{}', null, 'strict');
        "#,
        file_path_str
    );
    let err_strict = run_source(&code_strict).expect_err("strict read should fail on invalid bytes");
    let err_str = format!("{:?}", err_strict);
    assert!(
        err_str.contains("InvalidEncodingException"),
        "expected InvalidEncodingException, got: {}",
        err_str
    );

    // 2. File::read with 'lossy' replaces invalid bytes with U+FFFD
    let code_lossy = format!(
        r#"
            return File::read('{}', null, 'lossy');
        "#,
        file_path_str
    );
    let res_lossy = run_source(&code_lossy).expect("lossy read should succeed");
    assert!(res_lossy.to_php_string().contains('\u{FFFD}'));

    // 3. File::readBytes returns raw Bytes without decoding
    let code_bytes = format!(
        r#"
            $b = File::readBytes('{}');
            return [$b->len(), $b];
        "#,
        file_path_str
    );
    let res_b = run_source(&code_bytes).expect("readBytes should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res_b {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(14));
        assert!(b[1].is_bytes());
        assert_eq!(b[1].raw_bytes().unwrap(), b"hello \xFF\xFE world");
    } else {
        panic!("expected list");
    }

    // 4. File::write with encoding transcoding
    let out_path = temp_dir.join("rhp_test_transcode.bin");
    let out_path_str = out_path.to_str().unwrap().replace('\\', "/");
    let code_write = format!(
        r#"
            $written = File::write('{}', 'こんにちは', 'Shift-JIS');
            return $written;
        "#,
        out_path_str
    );
    let res_w = run_source(&code_write).expect("write with encoding should succeed");
    assert_eq!(res_w, PhpValue::Int(10)); // 5 chars * 2 bytes = 10 bytes

    let read_back_raw = std::fs::read(&out_path).unwrap();
    assert_eq!(read_back_raw.len(), 10);
    // Decode with shift-jis
    let decoded_back = rhp::vm::encoding::decode_shift_jis(&read_back_raw);
    assert_eq!(decoded_back, "こんにちは");

    let _ = std::fs::remove_file(file_path);
    let _ = std::fs::remove_file(out_path);
}

#[test]
fn test_contextual_target_typing_bytes_and_string() {
    // 1. Variable type hint `byte` allows normal double quotes without `b` prefix
    let code_byte_var = r#"
        $header: byte = "GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        return $header->len();
    "#;
    let res = run_source(code_byte_var).expect("byte variable with double quotes should work");
    assert_eq!(res, PhpValue::Int(37));

    // 2. Binary magic bytes in `byte` variable with normal double quotes
    let code_magic = r#"
        $magic: byte = "\x89PNG\r\n\x1a\n";
        return $magic->len();
    "#;
    let res = run_source(code_magic).expect("byte variable with binary hex escapes should work");
    assert_eq!(res, PhpValue::Int(8));

    // 3. Single-quoted literal target-typed to byte
    let code_single = r#"
        $greeting: byte = 'hello world';
        return $greeting->len();
    "#;
    let res = run_source(code_single).expect("single-quoted byte should work");
    assert_eq!(res, PhpValue::Int(11));

    // 4. Function parameter target-typed to byte
    let code_fn_param = r#"
        function sendPayload($data: byte): int {
            return $data->len();
        }
        return sendPayload("PING \x00\x01\r\n");
    "#;
    let res = run_source(code_fn_param).expect("function parameter target-typing should work");
    assert_eq!(res, PhpValue::Int(9));

    // 5. Explicit functional conversion byte(...)
    let code_fn_cast = r#"
        $payload = byte("\x89PNG\r\n");
        return $payload->len();
    "#;
    let res = run_source(code_fn_cast).expect("byte(...) cast should work");
    assert_eq!(res, PhpValue::Int(6));

    // 6. Explicit functional conversion bytes(...)
    let code_fn_cast_bytes = r#"
        $payload = bytes("\x89PNG\r\n");
        return $payload->len();
    "#;
    let res = run_source(code_fn_cast_bytes).expect("bytes(...) cast should work");
    assert_eq!(res, PhpValue::Int(6));

    // 6b. C-style cast (byte) is rejected with clear guidance
    let code_c_cast = r#"
        $payload = (byte) "\x89PNG\r\n";
    "#;
    let err = run_source(code_c_cast).expect_err("C-style (byte) cast should be rejected");
    let err_str = format!("{:?}", err);
    assert!(err_str.contains("functional conversion 'bytes(...)'"), "got: {}", err_str);

    // 7. Untyped variable with invalid UTF-8 escape raises compile error with hint
    let code_untyped_err = r#"
        $s = "\xFF";
    "#;
    let err = run_source(code_untyped_err).expect_err("untyped invalid UTF-8 should fail");
    let err_str = format!("{:?}", err);
    assert!(err_str.contains("Invalid UTF-8 escape") && err_str.contains("bytes literal"), "got: {}", err_str);

    // 8. Explicit string variable with invalid UTF-8 escape raises compile error
    let code_string_err = r#"
        $s: string = "foo \x89 bar";
    "#;
    let err = run_source(code_string_err).expect_err("string invalid UTF-8 should fail");
    let err_str = format!("{:?}", err);
    assert!(err_str.contains("Invalid UTF-8 escape"), "got: {}", err_str);

    // 9. Valid string variable works as expected
    let code_string_ok = r#"
        $s: string = "Hello \u{1F600} world!";
        return $s->length();
    "#;
    let res = run_source(code_string_ok).expect("valid string should succeed");
    assert_eq!(res, PhpValue::Int(14)); // 14 characters

    // 10. Slice and byte indexing on byte values
    let code_slice = r#"
        $data: byte = "\x01\x02\x03\x04\x05";
        $sub: byte = $data->slice(1, 3);
        return $sub->len();
    "#;
    let res = run_source(code_slice).expect("byte slice should succeed");
    assert_eq!(res, PhpValue::Int(2));
}

#[test]
fn test_byte_fluent_helpers_and_shared_text_api() {
    // 1. Shared slice and substr on "The quick brown fox" for both string and byte
    let code_shared = r#"
        $s: string = "The quick brown fox";
        $b: byte = byte("The quick brown fox");

        $s_slice1 = $s->slice(4, 9);
        $s_slice2 = $s->slice(10);
        $s_slice3 = $s->slice(-3);

        $b_slice1 = $b->slice(4, 9)->decode();
        $b_slice2 = $b->slice(10)->decode();
        $b_slice3 = $b->slice(-3)->decode();

        $s_sub1 = $s->substr(4, 5);
        $s_sub2 = $s->substr(10);
        $s_sub3 = $s->substr(-3);

        $b_sub1 = $b->substr(4, 5)->decode();
        $b_sub2 = $b->substr(10)->decode();
        $b_sub3 = $b->substr(-3)->decode();

        $static_sub1 = Str::substr("The quick brown fox", 4, 5);
        $static_sub2 = Str::substr("The quick brown fox", 10);
        $static_sub3 = Str::substr("The quick brown fox", -3);

        return [
            $s_slice1, $s_slice2, $s_slice3,
            $b_slice1, $b_slice2, $b_slice3,
            $s_sub1, $s_sub2, $s_sub3,
            $b_sub1, $b_sub2, $b_sub3,
            $static_sub1, $static_sub2, $static_sub3,
        ];
    "#;
    let res = run_source(code_shared).expect("shared slicing should work");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let items = arr.borrow();
        assert_eq!(items[0].to_php_string(), "quick");
        assert_eq!(items[1].to_php_string(), "brown fox");
        assert_eq!(items[2].to_php_string(), "fox");

        assert_eq!(items[3].to_php_string(), "quick");
        assert_eq!(items[4].to_php_string(), "brown fox");
        assert_eq!(items[5].to_php_string(), "fox");

        assert_eq!(items[6].to_php_string(), "quick");
        assert_eq!(items[7].to_php_string(), "brown fox");
        assert_eq!(items[8].to_php_string(), "fox");

        assert_eq!(items[9].to_php_string(), "quick");
        assert_eq!(items[10].to_php_string(), "brown fox");
        assert_eq!(items[11].to_php_string(), "fox");

        assert_eq!(items[12].to_php_string(), "quick");
        assert_eq!(items[13].to_php_string(), "brown fox");
        assert_eq!(items[14].to_php_string(), "fox");
    } else {
        panic!("expected list");
    }

    // 2. take and takeLast
    let code_take = r#"
        $s = "The quick brown fox";
        $b = byte("The quick brown fox");
        return [
            $s->take(3), $s->takeLast(3), $s->take(-1),
            $b->take(3)->decode(), $b->takeLast(3)->decode(), $b->take(0)->len()
        ];
    "#;
    let res_take = run_source(code_take).expect("take should work");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res_take {
        let items = arr.borrow();
        assert_eq!(items[0].to_php_string(), "The");
        assert_eq!(items[1].to_php_string(), "fox");
        assert_eq!(items[2].to_php_string(), "");
        assert_eq!(items[3].to_php_string(), "The");
        assert_eq!(items[4].to_php_string(), "fox");
        assert_eq!(items[5], PhpValue::Int(0));
    }

    // 3. indexOf and lastIndexOf (including null on missing)
    let code_index = r#"
        $s = "hello world hello";
        $b = byte("hello world hello");
        return [
            $s->indexOf("hello"),
            $s->indexOf("hello", 1),
            $s->indexOf("missing"),
            $s->lastIndexOf("hello"),
            $s->lastIndexOf("hello", 11),
            $s->lastIndexOf("missing"),
            $b->indexOf("hello"),
            $b->lastIndexOf("hello"),
            $b->indexOf("missing"),
        ];
    "#;
    let res_index = run_source(code_index).expect("index search should work");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res_index {
        let items = arr.borrow();
        assert_eq!(items[0], PhpValue::Int(0));
        assert_eq!(items[1], PhpValue::Int(12));
        assert_eq!(items[2], PhpValue::Null);
        assert_eq!(items[3], PhpValue::Int(12));
        assert_eq!(items[4], PhpValue::Int(0));
        assert_eq!(items[5], PhpValue::Null);
        assert_eq!(items[6], PhpValue::Int(0));
        assert_eq!(items[7], PhpValue::Int(12));
        assert_eq!(items[8], PhpValue::Null);
    }

    // 4. toHex and toBase64 on \xFF\xFE (comparing fluent and Binary facade)
    let code_hex_b64 = r#"
        $b = b"\xFF\xFE";
        $h1 = $b->toHex();
        $h2 = Binary::toHex($b);
        $b64 = $b->toBase64();
        return [$h1, $h2, $b64];
    "#;
    let res_hex = run_source(code_hex_b64).expect("toHex and toBase64 should work");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res_hex {
        let items = arr.borrow();
        assert_eq!(items[0].to_php_string(), "fffe");
        assert_eq!(items[1].to_php_string(), "fffe");
        assert_eq!(items[2].to_php_string(), "//4=");
    }

    // 5. toString() fallibility, toStringLossy(), and isValidUtf8() on \xFF\xFE
    let code_tostring_fail = r#"
        $b = b"\xFF\xFE";
        return $b->toString();
    "#;
    let err_ts = run_source(code_tostring_fail).expect_err("toString on invalid UTF-8 should fail");
    assert!(format!("{:?}", err_ts).contains("InvalidEncodingException"));

    let code_tostring_lossy = r#"
        $b = b"\xFF\xFE";
        $lossy = $b->toStringLossy();
        $valid = $b->isValidUtf8();
        $valid_str = string("hello")->isValidUtf8();
        return [$lossy, $valid, $valid_str];
    "#;
    let res_lossy = run_source(code_tostring_lossy).expect("lossy conversion should work");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res_lossy {
        let items = arr.borrow();
        assert!(items[0].to_php_string().contains('\u{FFFD}'));
        assert_eq!(items[1], PhpValue::Bool(false));
        assert_eq!(items[2], PhpValue::Bool(true));
    }

    // 6. metaData() dict
    let code_meta = r#"
        $b = b"hello";
        $m = $b->metaData();
        return [$m['length'], $m['byteLength'], $m['encoding'], $m['validUtf8'], $m['bom']];
    "#;
    let res_meta = run_source(code_meta).expect("metaData should work");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res_meta {
        let items = arr.borrow();
        assert_eq!(items[0], PhpValue::Int(5));
        assert_eq!(items[1], PhpValue::Int(5));
        assert_eq!(items[2].to_php_string(), "ASCII");
        assert_eq!(items[3], PhpValue::Bool(true));
        assert_eq!(items[4], PhpValue::Null);
    }

    // 7. upperCase on a Shift-JIS buffer
    // Shift-JIS bytes for "hello 世界"
    // "hello" is ASCII 0x68, 0x65, 0x6c, 0x6c, 0x6f
    // "世界" in Shift-JIS is 0x90, 0xA2, 0x8A, 0x45
    let code_sjis = r#"
        $sjis = b"hello \x90\xA2\x8A\x45";
        $upper = $sjis->upperCase();
        return [$sjis->metaData()['encoding'], $upper->toHex()];
    "#;
    let res_sjis = run_source(code_sjis).expect("Shift-JIS upperCase should work");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res_sjis {
        let items = arr.borrow();
        assert_eq!(items[0].to_php_string(), "Shift-JIS");
        // "HELLO " is 0x48, 0x45, 0x4c, 0x4c, 0x4f, 0x20, then 世界 in Shift-JIS 90a28a45
        assert_eq!(items[1].to_php_string(), "48454c4c4f2090a28a45");
    }

    // 8. trim still rejected on byte
    let code_trim_byte = r#"
        $b = byte("  hello  ");
        return $b->trim();
    "#;
    let err_trim = run_source(code_trim_byte).expect_err("trim on byte must fail");
    assert!(format!("{:?}", err_trim).contains("Method trim() is not supported on bytes. Call ->decode() first to get a string."));
}
