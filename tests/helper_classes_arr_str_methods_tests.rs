use rhp::run_source;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;

#[test]
fn test_arr_check_static_and_instance() {
    clear_runtime_closure_macros();

    let code = r#"
        $arr = [1, 2, 3];
        $list = Arr::list([1, 2, 3]);
        $dict = Arr::dict(['a' => 1, 'b' => 2]);
        $set = Arr::set([10, 20, 30]);
        $map = Arr::hashMap(['k' => 'v']);

        $c_arr = Arr::check($arr);
        $c_list = Arr::check($list);
        $c_dict = Arr::check($dict);
        $c_set = Arr::check($set);
        $c_map = Arr::check($map);

        $c_str = Arr::check("not an array");
        $c_int = Arr::check(42);
        $c_float = Arr::check(3.14);
        $c_null = Arr::check(null);

        // Also test instance calls
        $inst_list = $list->check();
        $inst_dict = $dict->check();
        $inst_set = $set->check();
        $inst_map = $map->check();

        return [
            $c_arr, $c_list, $c_dict, $c_set, $c_map,
            $c_str, $c_int, $c_float, $c_null,
            $inst_list, $inst_dict, $inst_set, $inst_map,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        // True checks:
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(true));
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(true));
        assert_eq!(b[4], PhpValue::Bool(true));
        // False checks:
        assert_eq!(b[5], PhpValue::Bool(false));
        assert_eq!(b[6], PhpValue::Bool(false));
        assert_eq!(b[7], PhpValue::Bool(false));
        assert_eq!(b[8], PhpValue::Bool(false));
        // Instance calls:
        assert_eq!(b[9], PhpValue::Bool(true));
        assert_eq!(b[10], PhpValue::Bool(true));
        assert_eq!(b[11], PhpValue::Bool(true));
        assert_eq!(b[12], PhpValue::Bool(true));
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_arr_length_static_and_unstatic() {
    clear_runtime_closure_macros();

    let code = r#"
        $arr = [10, 20, 30, 40];
        $list = Arr::list([1, 2, 3]);
        $dict = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3]);
        $set = Arr::set(['alpha', 'beta']);
        $map = Arr::hashMap(['k1' => 10, 'k2' => 20, 'k3' => 30, 'k4' => 40, 'k5' => 50]);

        // Static Arr::length()
        $len_arr = Arr::length($arr);
        $len_list = Arr::length($list);
        $len_dict = Arr::length($dict);
        $len_set = Arr::length($set);
        $len_map = Arr::length($map);

        // Unstatic ->length()
        $u_list = $list->length();
        $u_dict = $dict->length();
        $u_set = $set->length();
        $u_map = $map->length();

        return [
            $len_arr, $len_list, $len_dict, $len_set, $len_map,
            $u_list, $u_dict, $u_set, $u_map,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(4));
        assert_eq!(b[1], PhpValue::Int(3));
        assert_eq!(b[2], PhpValue::Int(3));
        assert_eq!(b[3], PhpValue::Int(2));
        assert_eq!(b[4], PhpValue::Int(5));
        assert_eq!(b[5], PhpValue::Int(3));
        assert_eq!(b[6], PhpValue::Int(3));
        assert_eq!(b[7], PhpValue::Int(2));
        assert_eq!(b[8], PhpValue::Int(5));
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_unstatic_subsets_map_filter_haskey_hasvalue_keys_values() {
    clear_runtime_closure_macros();

    let code = r#"
        // 1. List
        $list = Arr::list([10, 20, 30]);
        $l_mapped = $list->map(fn($n) => $n * 2);
        $l_filtered = $list->filter(fn($n) => $n > 15);
        $l_hasKey0 = $list->hasKey(0);
        $l_hasKey5 = $list->hasKey(5);
        $l_hasVal20 = $list->hasValue(20);
        $l_hasVal99 = $list->hasValue(99);
        $l_keys = $list->keys();
        $l_vals = $list->values();

        // 2. Dict
        $dict = Arr::dict(['x' => 10, 'y' => 20, 'z' => 30]);
        $d_mapped = $dict->map(fn($v) => $v * 3);
        $d_filtered = $dict->filter(fn($v) => $v > 10);
        $d_hasKeyX = $dict->hasKey('x');
        $d_hasKeyW = $dict->hasKey('w');
        $d_hasVal20 = $dict->hasValue(20);
        $d_hasVal99 = $dict->hasValue(99);
        $d_keys = $dict->keys();
        $d_vals = $dict->values();

        // 3. Set
        $set = Arr::set(['apple', 'banana', 'cherry']);
        $s_mapped = $set->map(fn($s) => Str::upper($s));
        $s_filtered = $set->filter(fn($s) => $s != 'banana');
        $s_hasKeyApple = $set->hasKey('apple');
        $s_hasKeyOrange = $set->hasKey('orange');
        $s_hasValCherry = $set->hasValue('cherry');
        $s_hasValGrape = $set->hasValue('grape');
        $s_keys = $set->keys();
        $s_vals = $set->values();

        // 4. HashMap
        $map = Arr::hashMap(['name' => 'RHP', 'version' => '2']);
        $m_mapped = $map->map(fn($v) => $v . '_ok');
        $m_filtered = $map->filter(fn($v) => $v == '2');
        $m_hasKeyName = $map->hasKey('name');
        $m_hasKeyMissing = $map->hasKey('missing');
        $m_hasVal2 = $map->hasValue('2');
        $m_hasVal99 = $map->hasValue('99');
        $m_keys = $map->keys();
        $m_vals = $map->values();

        return [
            $l_mapped, $l_filtered, $l_hasKey0, $l_hasKey5, $l_hasVal20, $l_hasVal99, $l_keys, $l_vals,
            $d_mapped, $d_filtered, $d_hasKeyX, $d_hasKeyW, $d_hasVal20, $d_hasVal99, $d_keys, $d_vals,
            $s_mapped, $s_filtered, $s_hasKeyApple, $s_hasKeyOrange, $s_hasValCherry, $s_hasValGrape, $s_keys, $s_vals,
            $m_mapped, $m_filtered, $m_hasKeyName, $m_hasKeyMissing, $m_hasVal2, $m_hasVal99, $m_keys, $m_vals,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();

        // List assertions
        assert_eq!(b[0], PhpValue::list(vec![PhpValue::Int(20), PhpValue::Int(40), PhpValue::Int(60)]));
        assert_eq!(b[1], PhpValue::list(vec![PhpValue::Int(20), PhpValue::Int(30)]));
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(false));
        assert_eq!(b[4], PhpValue::Bool(true));
        assert_eq!(b[5], PhpValue::Bool(false));
        assert_eq!(b[6], PhpValue::list(vec![PhpValue::Int(0), PhpValue::Int(1), PhpValue::Int(2)]));
        assert_eq!(b[7], PhpValue::list(vec![PhpValue::Int(10), PhpValue::Int(20), PhpValue::Int(30)]));

        // Dict assertions
        assert_eq!(b[10], PhpValue::Bool(true));
        assert_eq!(b[11], PhpValue::Bool(false));
        assert_eq!(b[12], PhpValue::Bool(true));
        assert_eq!(b[13], PhpValue::Bool(false));
        assert_eq!(b[14], PhpValue::list(vec![PhpValue::string("x"), PhpValue::string("y"), PhpValue::string("z")]));
        assert_eq!(b[15], PhpValue::list(vec![PhpValue::Int(10), PhpValue::Int(20), PhpValue::Int(30)]));

        // Set assertions
        assert_eq!(b[18], PhpValue::Bool(true));
        assert_eq!(b[19], PhpValue::Bool(false));
        assert_eq!(b[20], PhpValue::Bool(true));
        assert_eq!(b[21], PhpValue::Bool(false));
        if let PhpValue::List(keys_rc) = &b[22] {
            assert_eq!(keys_rc.borrow().len(), 3);
        } else {
            panic!("expected set keys to be a list");
        }

        // HashMap assertions
        assert_eq!(b[26], PhpValue::Bool(true));
        assert_eq!(b[27], PhpValue::Bool(false));
        assert_eq!(b[28], PhpValue::Bool(true));
        assert_eq!(b[29], PhpValue::Bool(false));
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_str_check_and_helpers_static_and_unstatic() {
    clear_runtime_closure_macros();

    let code = r#"
        $s1 = "  Hello World  ";
        $c_str = Str::check($s1);
        $c_arr = Str::check([1, 2]);
        $c_int = Str::check(100);
        $c_null = Str::check(null);
        $u_check = $s1->check();

        // Str trim, ltrim, rtrim static
        $t_trim = Str::trim($s1);
        $t_ltrim = Str::ltrim($s1);
        $t_rtrim = Str::rtrim($s1);

        // Str lower, upper static
        $t_lower = Str::lower("RHP LANGUAGE");
        $t_upper = Str::upper("rhp language");

        // Str replace static (both argument conventions)
        $t_rep1 = Str::replace("World", "Universe", $s1);
        $t_rep2 = Str::replace($s1, "World", "Universe");

        // Unstatic on string instance
        $u_trim = $s1->trim();
        $u_ltrim = $s1->ltrim();
        $u_rtrim = $s1->rtrim();
        $u_lower = "RHP"->toLower();
        $u_upper = "rhp"->toUpper();
        $u_replace = $s1->trim()->replace("World", "Galaxy");

        return [
            $c_str, $c_arr, $c_int, $c_null, $u_check,
            $t_trim, $t_ltrim, $t_rtrim,
            $t_lower, $t_upper,
            $t_rep1, $t_rep2,
            $u_trim, $u_ltrim, $u_rtrim,
            $u_lower, $u_upper, $u_replace,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(false));
        assert_eq!(b[2], PhpValue::Bool(false));
        assert_eq!(b[3], PhpValue::Bool(false));
        assert_eq!(b[4], PhpValue::Bool(true));

        assert_eq!(b[5].to_php_string(), "Hello World");
        assert_eq!(b[6].to_php_string(), "Hello World  ");
        assert_eq!(b[7].to_php_string(), "  Hello World");

        assert_eq!(b[8].to_php_string(), "rhp language");
        assert_eq!(b[9].to_php_string(), "RHP LANGUAGE");

        assert_eq!(b[10].to_php_string(), "  Hello Universe  ");
        assert_eq!(b[11].to_php_string(), "  Hello Universe  ");

        assert_eq!(b[12].to_php_string(), "Hello World");
        assert_eq!(b[13].to_php_string(), "Hello World  ");
        assert_eq!(b[14].to_php_string(), "  Hello World");

        assert_eq!(b[15].to_php_string(), "rhp");
        assert_eq!(b[16].to_php_string(), "RHP");
        assert_eq!(b[17].to_php_string(), "Hello Galaxy");
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_arr_check_subtypes() {
    clear_runtime_closure_macros();

    let code = r#"
        $l = Arr::list([1, 2, 3]);
        $d = Arr::dict(['a' => 1]);
        $s = Arr::set(['x', 'y']);
        $m = Arr::hashMap(['k' => 'v']);
        $a = [10, 20];

        // Static Arr::check with subtype arguments
        $l_is_list = Arr::check($l, 'list');
        $l_is_dict = Arr::check($l, 'dict');

        $d_is_dict = Arr::check($d, 'dict');
        $d_is_set = Arr::check($d, 'set');

        $s_is_set = Arr::check($s, 'set');
        $s_is_map = Arr::check($s, 'hashMap');

        $m_is_map = Arr::check($m, 'hashMap');
        $m_is_list = Arr::check($m, 'list');

        $a_is_array = Arr::check($a, 'array');
        $a_is_list = Arr::check($a, 'list');

        // Unstatic check with subtype
        $u_l_list = $l->check('list');
        $u_l_dict = $l->check('dict');
        $u_d_dict = $d->check('dict');
        $u_d_map = $d->check('map');
        $u_s_set = $s->check('set');
        $u_s_dict = $s->check('dict');
        $u_m_map = $m->check('hashMap');
        $u_m_set = $m->check('set');

        return [
            $l_is_list, $l_is_dict,
            $d_is_dict, $d_is_set,
            $s_is_set, $s_is_map,
            $m_is_map, $m_is_list,
            $a_is_array, $a_is_list,
            $u_l_list, $u_l_dict,
            $u_d_dict, $u_d_map,
            $u_s_set, $u_s_dict,
            $u_m_map, $u_m_set,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(false));
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(false));
        assert_eq!(b[4], PhpValue::Bool(true));
        assert_eq!(b[5], PhpValue::Bool(false));
        assert_eq!(b[6], PhpValue::Bool(true));
        assert_eq!(b[7], PhpValue::Bool(false));
        assert_eq!(b[8], PhpValue::Bool(true));
        assert_eq!(b[9], PhpValue::Bool(true));

        assert_eq!(b[10], PhpValue::Bool(true));
        assert_eq!(b[11], PhpValue::Bool(false));
        assert_eq!(b[12], PhpValue::Bool(true));
        assert_eq!(b[13], PhpValue::Bool(false));
        assert_eq!(b[14], PhpValue::Bool(true));
        assert_eq!(b[15], PhpValue::Bool(false));
        assert_eq!(b[16], PhpValue::Bool(true));
        assert_eq!(b[17], PhpValue::Bool(false));
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_str_check_encodings() {
    clear_runtime_closure_macros();

    let code = r#"
        $utf8_str = "Hello, 世界!";
        // Shift-JIS encoded bytes for Japanese "こんにちは" (Kon'nichiwa)
        $sjis_str = Str::fromBytes([0x82, 0xB1, 0x82, 0xF1, 0x82, 0xC9, 0x82, 0xBF, 0x82, 0xCD]);

        // Static Str::check with encodings
        $u8_as_u8 = Str::check($utf8_str, 'utf-8');
        $u8_as_ascii = Str::check($utf8_str, 'ascii'); // contains multi-byte unicode chars, so false

        $sjis_as_jshift = Str::check($sjis_str, 'jshift');
        $sjis_as_sjis = Str::check($sjis_str, 'shift_jis');
        $sjis_as_u8 = Str::check($sjis_str, 'utf-8');

        // Unstatic string calls
        $u_u8_as_u8 = $utf8_str->check('utf-8');
        $u_sjis_as_jshift = $sjis_str->check('jshift');
        $u_sjis_as_u8 = $sjis_str->check('utf-8');

        return [
            $u8_as_u8, $u8_as_ascii,
            $sjis_as_jshift, $sjis_as_sjis, $sjis_as_u8,
            $u_u8_as_u8, $u_sjis_as_jshift, $u_sjis_as_u8,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(false));
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(true));
        assert_eq!(b[4], PhpValue::Bool(false));
        assert_eq!(b[5], PhpValue::Bool(true));
        assert_eq!(b[6], PhpValue::Bool(true));
        assert_eq!(b[7], PhpValue::Bool(false));
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_string_non_static_helpers_and_regex_class() {
    clear_runtime_closure_macros();

    let code = r#"
        $text = "The quick brown fox jumps over the lazy dog";

        // 1. ->includes(substring)
        $inc_fox = $text->includes("fox");
        $inc_cat = $text->includes("cat");

        // 2. ->startsWith(substring)
        $sw_the = $text->startsWith("The");
        $sw_quick = $text->startsWith("quick");

        // 3. ->endsWith(substring)
        $ew_dog = $text->endsWith("dog");
        $ew_cat = $text->endsWith("cat");

        // 4. ->indexOf(substring)
        $idx_quick = $text->indexOf("quick"); // pos 4
        $idx_fox = $text->indexOf("fox");     // pos 16
        $idx_none = $text->indexOf("unicorn"); // -1

        // Multi-byte character indexOf test
        $utf8_text = "Hello 🌍 World!";
        $idx_world = $utf8_text->indexOf("World"); // char position 8

        // 5. ->testRegex(regex)
        $rx_match1 = $text->testRegex("/^The quick/i");
        $rx_match2 = $text->testRegex("/^fox/");
        $rx_match3 = $text->testRegex('/\\blazy\\b/');
        $rx_match4 = "user@example.com"->testRegex('/^[\\w\\.\\-]+@[\\w\\-]+\\.[a-zA-Z]{2,}$/');
        $rx_match5 = "invalid-email"->testRegex('/^[\\w\\.\\-]+@[\\w\\-]+\\.[a-zA-Z]{2,}$/');

        // 6. Regex::isValid($pattern)
        $v1 = Regex::isValid('/^[0-9]+$/');
        $v2 = Regex::isValid("/[a-z]+/i");
        $v3 = Regex::isValid("[unclosed-group");

        // 7. Regex::testString($pattern, $subject)
        $t1 = Regex::testString('/^\\d{4}-\\d{2}-\\d{2}$/', "2026-10-25");
        $t2 = Regex::testString('/^\\d{4}-\\d{2}-\\d{2}$/', "invalid-date");
        $t3 = Regex::testString("/rhp/i", "RHP high-performance runtime");

        return [
            $inc_fox, $inc_cat,
            $sw_the, $sw_quick,
            $ew_dog, $ew_cat,
            $idx_quick, $idx_fox, $idx_none, $idx_world,
            $rx_match1, $rx_match2, $rx_match3, $rx_match4, $rx_match5,
            $v1, $v2, $v3,
            $t1, $t2, $t3,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        // includes
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(false));

        // startsWith
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(false));

        // endsWith
        assert_eq!(b[4], PhpValue::Bool(true));
        assert_eq!(b[5], PhpValue::Bool(false));

        // indexOf
        assert_eq!(b[6], PhpValue::Int(4));
        assert_eq!(b[7], PhpValue::Int(16));
        assert_eq!(b[8], PhpValue::Int(-1));
        assert_eq!(b[9], PhpValue::Int(8));

        // testRegex
        assert_eq!(b[10], PhpValue::Bool(true));
        assert_eq!(b[11], PhpValue::Bool(false));
        assert_eq!(b[12], PhpValue::Bool(true));
        assert_eq!(b[13], PhpValue::Bool(true));
        assert_eq!(b[14], PhpValue::Bool(false));

        // Regex::isValid
        assert_eq!(b[15], PhpValue::Bool(true));
        assert_eq!(b[16], PhpValue::Bool(true));
        assert_eq!(b[17], PhpValue::Bool(false));

        // Regex::testString
        assert_eq!(b[18], PhpValue::Bool(true));
        assert_eq!(b[19], PhpValue::Bool(false));
        assert_eq!(b[20], PhpValue::Bool(true));
    } else {
        panic!("expected array/list result");
    }
}

#[test]
fn test_float_round_floor_ceil_ciel_helpers() {
    clear_runtime_closure_macros();

    let code = r#"
        $pi = 3.14159;
        $num1 = 3.6;
        $num2 = 3.2;
        $neg = -3.2;

        // 1. Fluent unstatic calls on float
        $r1 = $pi->round(2);
        $r2 = $num1->round();
        $f1 = $num1->floor();
        $f2 = $neg->floor();
        $c1 = $num2->ceil();
        $c2 = $num2->ciel(); // user-specified alias

        // 2. Static Float facade calls
        $sf_round = Float::round($pi, 2);
        $sf_floor = Float::floor(3.9);
        $sf_ceil = Float::ceil(3.1);
        $sf_ciel = Float::ciel(3.1);

        // 3. Global function calls
        $g_round = round($pi, 2);
        $g_floor = floor(3.9);
        $g_ceil = ceil(3.1);
        $g_ciel = ciel(3.1);

        // 4. Fluent calls on Float::f64 wrapper
        $w_round = Float::f64($pi)->round(2)->toFloat();
        $w_floor = Float::f64(3.9)->floor()->toFloat();
        $w_ceil = Float::f64(3.1)->ceil()->toFloat();
        $w_ciel = Float::f64(3.1)->ciel()->toFloat();

        return [
            $r1, $r2, $f1, $f2, $c1, $c2,
            $sf_round, $sf_floor, $sf_ceil, $sf_ciel,
            $g_round, $g_floor, $g_ceil, $g_ciel,
            $w_round, $w_floor, $w_ceil, $w_ciel,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert!((b[0].to_f32().unwrap() - 3.14).abs() < 1e-4);
        assert!((b[1].to_f32().unwrap() - 4.0).abs() < 1e-4);
        assert!((b[2].to_f32().unwrap() - 3.0).abs() < 1e-4);
        assert!((b[3].to_f32().unwrap() - (-4.0)).abs() < 1e-4);
        assert!((b[4].to_f32().unwrap() - 4.0).abs() < 1e-4);
        assert!((b[5].to_f32().unwrap() - 4.0).abs() < 1e-4);

        assert!((b[6].to_f32().unwrap() - 3.14).abs() < 1e-4);
        assert!((b[7].to_f32().unwrap() - 3.0).abs() < 1e-4);
        assert!((b[8].to_f32().unwrap() - 4.0).abs() < 1e-4);
        assert!((b[9].to_f32().unwrap() - 4.0).abs() < 1e-4);

        assert!((b[10].to_f32().unwrap() - 3.14).abs() < 1e-4);
        assert!((b[11].to_f32().unwrap() - 3.0).abs() < 1e-4);
        assert!((b[12].to_f32().unwrap() - 4.0).abs() < 1e-4);
        assert!((b[13].to_f32().unwrap() - 4.0).abs() < 1e-4);

        assert!((b[14].to_f32().unwrap() - 3.14).abs() < 1e-4);
        assert!((b[15].to_f32().unwrap() - 3.0).abs() < 1e-4);
        assert!((b[16].to_f32().unwrap() - 4.0).abs() < 1e-4);
        assert!((b[17].to_f32().unwrap() - 4.0).abs() < 1e-4);
    } else {
        panic!("expected array/list result");
    }
}



