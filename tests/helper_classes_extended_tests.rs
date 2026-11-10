use rhp::run_source;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;

#[test]
fn test_arr_extended_helpers() {
    clear_runtime_closure_macros();

    let code = r#"
        $arr = [10, 20, 30];
        $empty = [];
        $assoc = ['first' => 'apple', 'second' => 'banana'];

        $first_val = Arr::first($arr);
        $first_default = Arr::first($empty, 'fallback');
        $last_val = Arr::last($arr);
        $last_default = Arr::last($empty, 'fallback');

        $first_key = Arr::firstKey($assoc);
        $last_key = Arr::lastKey($assoc);
        $empty_key = Arr::firstKey($empty);

        $sum_int = Arr::sum([1, 2, 3, 4]);
        $sum_float = Arr::sum([1.5, 2.5]);
        $prod_int = Arr::product([2, 3, 4]);
        $prod_float = Arr::product([2.5, 2.0]);
        $prod_empty = Arr::product([]);

        $fill_zero = Arr::fill(0, 3, 'a');
        $fill_offset = Arr::fill(2, 2, 'b');
        $fill_keys = Arr::fillKeys(['x', 'y'], 99);

        $pad_right = Arr::pad([1, 2], 4, 0);
        $pad_left = Arr::pad([1, 2], -4, 0);

        return [
            $first_val, $first_default, $last_val, $last_default,
            $first_key, $last_key, $empty_key,
            $sum_int, $sum_float, $prod_int, $prod_float, $prod_empty,
            $fill_zero, $fill_offset, $fill_keys,
            $pad_right, $pad_left,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Int(10));
        assert_eq!(b[1], PhpValue::string("fallback"));
        assert_eq!(b[2], PhpValue::Int(30));
        assert_eq!(b[3], PhpValue::string("fallback"));
        assert_eq!(b[4], PhpValue::string("first"));
        assert_eq!(b[5], PhpValue::string("second"));
        assert_eq!(b[6], PhpValue::Null);
        assert_eq!(b[7], PhpValue::Int(10));
        assert_eq!(b[8], PhpValue::Float(4.0));
        assert_eq!(b[9], PhpValue::Int(24));
        assert_eq!(b[10], PhpValue::Float(5.0));
        assert_eq!(b[11], PhpValue::Int(1));
    } else {
        panic!("expected array result");
    }
}

#[test]
fn test_int_helpers() {
    clear_runtime_closure_macros();

    let code = r#"
        $abs1 = Int::abs(-42);
        $abs2 = Int::abs(42);

        $min_var = Int::min(10, 5, 20);
        $min_arr = Int::min([10, 5, 20]);
        $max_var = Int::max(10, 5, 20);
        $max_arr = Int::max([10, 5, 20]);

        $rand_range = Int::random(10, 20);
        $rand_default = Int::random();

        return [
            $abs1, $abs2,
            $min_var, $min_arr, $max_var, $max_arr,
            $rand_range, $rand_default,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Int(42));
        assert_eq!(b[1], PhpValue::Int(42));
        assert_eq!(b[2], PhpValue::Int(5));
        assert_eq!(b[3], PhpValue::Int(5));
        assert_eq!(b[4], PhpValue::Int(20));
        assert_eq!(b[5], PhpValue::Int(20));
        if let PhpValue::Int(r) = b[6] {
            assert!(r >= 10 && r <= 20);
        } else {
            panic!("expected int random");
        }
    } else {
        panic!("expected array result");
    }
}

#[test]
fn test_float_helpers() {
    clear_runtime_closure_macros();

    let code = r#"
        $fmt1 = Float::format(1234.567, 2, '.', ',');
        $fmt2 = Float::format(1234.56, 1, ',', ' ');
        $fmt3 = Float::format(1000.0, 0);

        $abs = Float::abs(-3.14);

        $min_var = Float::min(3.14, 1.25, 5.8);
        $min_arr = Float::min([3.14, 1.25, 5.8]);
        $max_var = Float::max(3.14, 1.25, 5.8);
        $max_arr = Float::max([3.14, 1.25, 5.8]);

        $rand_default = Float::random();
        $rand_range = Float::random(10.0, 20.0);

        return [
            $fmt1, $fmt2, $fmt3,
            $abs,
            $min_var, $min_arr, $max_var, $max_arr,
            $rand_default, $rand_range,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::string("1,234.57"));
        assert_eq!(b[1], PhpValue::string("1 234,6"));
        assert_eq!(b[2], PhpValue::string("1,000"));
        assert_eq!(b[3], PhpValue::Float(3.14));
        assert_eq!(b[4], PhpValue::Float(1.25));
        assert_eq!(b[5], PhpValue::Float(1.25));
        assert_eq!(b[6], PhpValue::Float(5.8));
        assert_eq!(b[7], PhpValue::Float(5.8));
        if let PhpValue::Float(f) = b[8] {
            assert!(f >= 0.0 && f <= 1.0);
        } else {
            panic!("expected float random default");
        }
        if let PhpValue::Float(f) = b[9] {
            assert!(f >= 10.0 && f <= 20.0);
        } else {
            panic!("expected float random range");
        }
    } else {
        panic!("expected array result");
    }
}

#[test]
fn test_str_extended_helpers() {
    clear_runtime_closure_macros();

    let code = r#"
        $pad_right = Str::pad("Alien", 10, "_", "right");
        $pad_left = Str::pad("Alien", 10, "_", "left");
        $pad_both = Str::pad("Alien", 10, "_", "both");
        $pad_mb = Str::pad("🦀", 4, "✨", "right");

        $repeat = Str::repeat("-=", 3);

        $count = Str::countOccurrences("banana", "an");

        $rep_at1 = Str::replaceAt("abcdef", "123", 2, 2);
        $rep_at2 = Str::replaceAt("abcdef", "123", 2, 0);

        return [
            $pad_right, $pad_left, $pad_both, $pad_mb,
            $repeat,
            $count,
            $rep_at1, $rep_at2,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::string("Alien_____"));
        assert_eq!(b[1], PhpValue::string("_____Alien"));
        assert_eq!(b[2], PhpValue::string("__Alien___"));
        assert_eq!(b[3], PhpValue::string("🦀✨✨✨"));
        assert_eq!(b[4], PhpValue::string("-=-=-="));
        assert_eq!(b[5], PhpValue::Int(2));
        assert_eq!(b[6], PhpValue::string("ab123ef"));
        assert_eq!(b[7], PhpValue::string("ab123cdef"));
    } else {
        panic!("expected array result");
    }
}

#[test]
fn test_file_extended_helpers() {
    clear_runtime_closure_macros();

    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("rhp_test_file_ext.txt");
    std::fs::write(&test_file, "testing extended file helpers").unwrap();
    let file_path = test_file.to_str().unwrap().replace("\\", "/");

    let code = format!(r#"
        $path = "{}";
        $name = File::name($path);
        $base = File::basename($path);
        $dir = File::dirname($path);
        $ext = File::extension($path);
        $info = File::info($path);
        $mtime = File::modifiedAt($path);
        $readable = File::isReadable($path);
        $writable = File::isWritable($path);

        return [
            $name, $base, $dir, $ext,
            $info['dirname'], $info['basename'], $info['extension'], $info['filename'],
            $mtime > 0,
            $readable, $writable,
        ];
    "#, file_path);

    let res = run_source(&code).expect("evaluation failed");
    let _ = std::fs::remove_file(&test_file);

    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::string("rhp_test_file_ext"));
        assert_eq!(b[1], PhpValue::string("rhp_test_file_ext.txt"));
        assert_eq!(b[3], PhpValue::string("txt"));
        assert_eq!(b[5], PhpValue::string("rhp_test_file_ext.txt"));
        assert_eq!(b[6], PhpValue::string("txt"));
        assert_eq!(b[7], PhpValue::string("rhp_test_file_ext"));
        assert_eq!(b[8], PhpValue::Bool(true));
        assert_eq!(b[9], PhpValue::Bool(true));
        assert_eq!(b[10], PhpValue::Bool(true));
    } else {
        panic!("expected array result");
    }
}

#[test]
fn test_url_parse_query() {
    clear_runtime_closure_macros();

    let code = r#"
        $q1 = Url::parseQuery("first=value&arr[]=foo+bar&arr[]=baz");
        $q2 = Url::parseQuery("https://example.com/search?q=rhp&page=2#top");

        return [$q1, $q2];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        if let PhpValue::Dict(d1) = &b[0] {
            let m1 = d1.borrow();
            assert_eq!(m1.get("first"), Some(&PhpValue::string("value")));
            if let Some(PhpValue::List(arr)) = m1.get("arr") {
                let items = arr.borrow();
                assert_eq!(items[0], PhpValue::string("foo bar"));
                assert_eq!(items[1], PhpValue::string("baz"));
            } else {
                panic!("expected arr list");
            }
        }
        if let PhpValue::Dict(d2) = &b[1] {
            let m2 = d2.borrow();
            assert_eq!(m2.get("q"), Some(&PhpValue::string("rhp")));
            assert_eq!(m2.get("page"), Some(&PhpValue::string("2")));
        }
    } else {
        panic!("expected array result");
    }
}

#[test]
fn test_ip_facade() {
    clear_runtime_closure_macros();

    let code = r#"
        $long = Ip::toLong("127.0.0.1");
        $ip = Ip::fromLong($long);

        $long_all = Ip::toLong("255.255.255.255");
        $ip_all = Ip::fromLong($long_all);

        $caught = false;
        try {
            Ip::toLong("not.an.ip");
        } catch (\Throwable $e) {
            $caught = true;
        }

        return [$long, $ip, $ip_all, $caught];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Int(2130706433));
        assert_eq!(b[1], PhpValue::string("127.0.0.1"));
        assert_eq!(b[2], PhpValue::string("255.255.255.255"));
        assert_eq!(b[3], PhpValue::Bool(true));
    } else {
        panic!("expected array result");
    }
}

#[test]
fn test_compress_facade() {
    clear_runtime_closure_macros();

    let code = r#"
        $text = "The quick brown fox jumps over the lazy dog";

        $zlib = Compress::zlib($text, 6);
        $unzlib = Compress::unzlib($zlib);

        $gzip = Compress::gzip($text, 6);
        $gunzip = Compress::gunzip($gzip);

        $corrupt_caught = false;
        try {
            Compress::unzlib("not compressed data");
        } catch (\Throwable $e) {
            $corrupt_caught = true;
        }

        return [$unzlib == $text, $gunzip == $text, $corrupt_caught];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(true));
        assert_eq!(b[2], PhpValue::Bool(true));
    } else {
        panic!("expected array result");
    }
}

#[test]
fn test_legacy_procedural_functions() {
    clear_runtime_closure_macros();

    let code = r#"
        $nf = number_format(1234.56, 2, ',', ' ');
        $pi = pathinfo('/www/htdocs/inc/lib.inc.php');
        $bn = basename('/etc/sudoers.d/custom', '');

        $a = abs(-15);
        $af = abs(-3.5);
        $mi = min(10, 2, 8);
        $ma = max([10, 2, 8]);
        $r = rand(1, 10);
        $mr = mt_rand(1, 10);

        $ipl = ip2long('192.168.1.1');
        $lip = long2ip($ipl);

        $out_arr = parse_str('name=John&age=30');

        $sp = str_pad('test', 8, '*', 0); // left pad
        $sr = str_repeat('xo', 3);
        $sc = substr_count('this is a test', 'is');
        $srep = substr_replace('hello world', 'there', 6);

        $asum = array_sum([10, 20, 30]);
        $aprod = array_product([2, 5, 3]);
        $afill = array_fill(0, 2, 'z');
        $afkeys = array_fill_keys(['k1', 'k2'], 1);
        $apad = array_pad([1], 3, 0);

        $orig = "data to compress";
        $z = gzcompress($orig);
        $uz = gzuncompress($z);
        $g = gzencode($orig);
        $ug = gzdecode($g);

        return [
            $nf, $pi['extension'], $bn,
            $a, $af, $mi, $ma, $r >= 1 && $r <= 10, $mr >= 1 && $mr <= 10,
            $lip,
            $out_arr['name'], $out_arr['age'],
            $sp, $sr, $sc, $srep,
            $asum, $aprod, $uz == $orig, $ug == $orig,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::string("1 234,56"));
        assert_eq!(b[1], PhpValue::string("php"));
        assert_eq!(b[2], PhpValue::string("custom"));
        assert_eq!(b[3], PhpValue::Int(15));
        assert_eq!(b[4], PhpValue::Float(3.5));
        assert_eq!(b[5], PhpValue::Int(2));
        assert_eq!(b[6], PhpValue::Int(10));
        assert_eq!(b[7], PhpValue::Bool(true));
        assert_eq!(b[8], PhpValue::Bool(true));
        assert_eq!(b[9], PhpValue::string("192.168.1.1"));
        assert_eq!(b[10], PhpValue::string("John"));
        assert_eq!(b[11], PhpValue::string("30"));
        assert_eq!(b[12], PhpValue::string("****test"));
        assert_eq!(b[13], PhpValue::string("xoxoxo"));
        assert_eq!(b[14], PhpValue::Int(2));
        assert_eq!(b[15], PhpValue::string("hello there"));
        assert_eq!(b[16], PhpValue::Int(60));
        assert_eq!(b[17], PhpValue::Int(30));
        assert_eq!(b[18], PhpValue::Bool(true));
        assert_eq!(b[19], PhpValue::Bool(true));
    } else {
        panic!("expected array result");
    }
}

#[test]
fn test_int_non_static_operations() {
    clear_runtime_closure_macros();

    let code = r#"
        $n = 10;
        $add = $n->add(5);
        $sub = $n->sub(3);
        $mul = $n->mul(4);
        $div = $n->div(2);

        $chain = $n->add(10)->sub(5)->mul(2)->div(3);

        $caught = false;
        try {
            $n->div(0);
        } catch (\DivisionByZeroError $e) {
            $caught = true;
        }

        return [$add, $sub, $mul, $div, $chain, $caught];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Int(15));
        assert_eq!(b[1], PhpValue::Int(7));
        assert_eq!(b[2], PhpValue::Int(40));
        assert_eq!(b[3], PhpValue::Int(5));
        assert_eq!(b[4], PhpValue::Int(10));
        assert_eq!(b[5], PhpValue::Bool(true));
    } else {
        panic!("expected array result");
    }
}

#[test]
fn test_float_binary_and_decimal_operations() {
    clear_runtime_closure_macros();

    let code = r#"
        float $x = 0.1;
        $b_add = $x->bAdd(0.2); // float(0.30000000000000004)
        $b_sub = $x->bSub(0.05);
        $b_mul = $x->bMul(2.0);
        $b_div = $x->bDiv(2.0);

        $d_add = $x->add(0.2); // float(0.3)
        $d_sub = $x->sub(0.05);
        $d_mul = $x->mul(0.2);
        $d_div = 0.3->div(0.1);

        $b_add_str = string($b_add);
        $d_add_str = string($d_add);

        $caught = false;
        try {
            $x->div(0.0);
        } catch (\DivisionByZeroError $e) {
            $caught = true;
        }

        $b_caught = false;
        try {
            $x->bDiv(0.0);
        } catch (\DivisionByZeroError $e) {
            $b_caught = true;
        }

        return [
            $b_add_str, $d_add_str,
            $d_add == $b_add,
            $caught, $b_caught,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::string("0.30000000000000004"));
        assert_eq!(b[1], PhpValue::string("0.30000000000000004"));
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(true));
        assert_eq!(b[4], PhpValue::Bool(true));
    } else {
        panic!("expected array result");
    }
}

#[test]
fn test_bcmath_procedural_helpers() {
    clear_runtime_closure_macros();

    let code = r#"
        $sum = bcadd('0.1', '0.2');
        $diff = bcsub('0.3', '0.2');
        $prod = bcmul('0.1', '0.2');
        $quot = bcdiv('0.3', '0.1');
        $cmp_eq = bccomp('0.3', '0.30');
        $cmp_gt = bccomp('0.4', '0.3');
        $cmp_lt = bccomp('0.2', '0.3');

        return [$sum, $diff, $prod, $quot, $cmp_eq, $cmp_gt, $cmp_lt];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::string("0.3"));
        assert_eq!(b[1], PhpValue::string("0.1"));
        assert_eq!(b[2], PhpValue::string("0.02"));
        assert_eq!(b[3], PhpValue::string("3"));
        assert_eq!(b[4], PhpValue::Int(0));
        assert_eq!(b[5], PhpValue::Int(1));
        assert_eq!(b[6], PhpValue::Int(-1));
    } else {
        panic!("expected array result");
    }
}
