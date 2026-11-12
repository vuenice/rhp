use rhp::run_source;
use rhp::vm::value::PhpValue;

#[test]
fn test_path_facade_helpers() {
    let code = r#"
        $b1 = Path::basename('/foo/bar/baz.html');
        $b2 = Path::basename('/foo/bar/baz.html', '.html');
        $b3 = Path::basename('baz.html');
        $b4 = Path::basename('baz.html', '.html');

        $d1 = Path::dirname('/foo/bar/baz.html');
        $d2 = Path::dirname('/foo');
        $d3 = Path::dirname('foo.txt');

        $e1 = Path::extname('index.html');
        $e2 = Path::extname('index.coffee.md');
        $e3 = Path::extname('index.');
        $e4 = Path::extname('index');
        $e5 = Path::extname('.gitignore');

        $abs1 = Path::isAbsolute('/foo/bar');
        $abs2 = Path::isAbsolute('foo/bar');

        $p1 = Path::parse('/home/user/dir/file.txt');
        $p2 = Path::parse('file.txt');

        $tmp = Path::tempDir();
        $tmp_legacy = sys_get_temp_dir();

        return [
            $b1, $b2, $b3, $b4,
            $d1, $d2, $d3,
            $e1, $e2, $e3, $e4, $e5,
            $abs1, $abs2,
            $p1, $p2,
            $tmp, $tmp_legacy
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::string("baz.html"));
        assert_eq!(b[1], PhpValue::string("baz"));
        assert_eq!(b[2], PhpValue::string("baz.html"));
        assert_eq!(b[3], PhpValue::string("baz"));

        assert_eq!(b[4], PhpValue::string("/foo/bar"));
        assert_eq!(b[5], PhpValue::string("/"));
        assert_eq!(b[6], PhpValue::string("."));

        assert_eq!(b[7], PhpValue::string(".html"));
        assert_eq!(b[8], PhpValue::string(".md"));
        assert_eq!(b[9], PhpValue::string("."));
        assert_eq!(b[10], PhpValue::string(""));
        assert_eq!(b[11], PhpValue::string(""));

        assert_eq!(b[12], PhpValue::Bool(true));
        assert_eq!(b[13], PhpValue::Bool(false));

        // p1: /home/user/dir/file.txt
        if let PhpValue::Dict(d) = &b[14] {
            let map = d.borrow();
            assert_eq!(map.get("root"), Some(&PhpValue::string("/")));
            assert_eq!(map.get("dir"), Some(&PhpValue::string("/home/user/dir")));
            assert_eq!(map.get("base"), Some(&PhpValue::string("file.txt")));
            assert_eq!(map.get("ext"), Some(&PhpValue::string(".txt")));
            assert_eq!(map.get("name"), Some(&PhpValue::string("file")));
        } else {
            panic!("Expected Dict for p1, got {:?}", b[14]);
        }

        // p2: file.txt
        if let PhpValue::Dict(d) = &b[15] {
            let map = d.borrow();
            assert_eq!(map.get("root"), Some(&PhpValue::string("")));
            assert_eq!(map.get("dir"), Some(&PhpValue::string("")));
            assert_eq!(map.get("base"), Some(&PhpValue::string("file.txt")));
            assert_eq!(map.get("ext"), Some(&PhpValue::string(".txt")));
            assert_eq!(map.get("name"), Some(&PhpValue::string("file")));
        } else {
            panic!("Expected Dict for p2, got {:?}", b[15]);
        }

        // temp dir
        assert!(!b[16].to_php_string().is_empty());
        assert_eq!(b[16], b[17]);
    } else {
        panic!("Expected array result, got {:?}", res);
    }
}

#[test]
fn test_binary_facade_and_legacy_helpers() {
    let code = r#"
        $b = Binary::bin2hex('Hello World');
        $orig = Binary::hex2bin($b);
        $rand1 = Binary::randomBytes(8);
        $hex1 = Binary::bin2hex($rand1);

        $legacy_rand = random_bytes(6);
        $legacy_hex = bin2hex($legacy_rand);
        $legacy_orig = hex2bin($legacy_hex);

        return [
            $b,
            $orig,
            strlen($hex1),
            strlen($legacy_hex),
            $legacy_orig == $legacy_rand
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::string("48656c6c6f20576f726c64"));
        assert_eq!(b[1].to_php_string(), "Hello World");
        assert_eq!(b[2], PhpValue::Int(16));
        assert_eq!(b[3], PhpValue::Int(12));
        assert_eq!(b[4], PhpValue::Bool(true));
    } else {
        panic!("Expected array result, got {:?}", res);
    }
}

#[test]
fn test_str_from_last_and_strrchr() {
    let code = r#"
        $sample = '127.0.0.1:54321';
        $from_last = Str::fromLast($sample, ':');
        $after_last = Str::afterLast($sample, ':');
        $legacy_chr = strrchr($sample, ':');
        $not_found = strrchr($sample, '?');

        return [
            $from_last,
            $after_last,
            $legacy_chr,
            $not_found
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::string(":54321"));
        assert_eq!(b[1], PhpValue::string("54321"));
        assert_eq!(b[2], PhpValue::string(":54321"));
        assert_eq!(b[3], PhpValue::Bool(false));
    } else {
        panic!("Expected array result, got {:?}", res);
    }
}

#[test]
fn test_socket_facade_and_port_discovery() {
    let code = r#"
        $probe = stream_socket_server('tcp://127.0.0.1:0');
        $name = stream_socket_get_name($probe, false);
        $chr = strrchr($name, ':');
        $port_str = substr($chr, 1);
        $port = int($port_str);
        $closed = fclose($probe);

        return [
            is_resource($probe),
            $name,
            $port > 0,
            $closed
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        assert!(b[1].to_php_string().starts_with("127.0.0.1:"));
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(true));
    } else {
        panic!("Expected array result, got {:?}", res);
    }
}

#[test]
fn test_file_rmdir_and_path_glob_workflow() {
    let code = r#"
        $dir = Path::tempDir() + '/rhp-test-glob-' + bin2hex(random_bytes(6));
        mkdir($dir);
        file_put_contents("{$dir}/a.txt", "file A");
        file_put_contents("{$dir}/b.txt", "file B");

        $files = Path::glob("{$dir}/*");
        $files_legacy = glob("{$dir}/*.txt");

        // Delete with array_map using string callable
        array_map('unlink', $files);

        $empty_check = glob("{$dir}/*");
        $removed = rmdir($dir);

        return [
            count($files),
            count($files_legacy),
            count($empty_check),
            $removed
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Int(2));
        assert_eq!(b[1], PhpValue::Int(2));
        assert_eq!(b[2], PhpValue::Int(0));
        assert_eq!(b[3], PhpValue::Bool(true));
    } else {
        panic!("Expected array result, got {:?}", res);
    }
}

#[test]
fn test_usleep_and_sleep() {
    let code = r#"
        $start = Time::now()->getTimestampMs();
        usleep(5000); // 5ms
        Sys::usleep(5000); // 5ms
        $diff = Time::now()->getTimestampMs() - $start;

        return [
            $diff >= 8
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
    } else {
        panic!("Expected array result, got {:?}", res);
    }
}

