use rhp::run_source;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;

#[test]
fn test_sys_get_temp_dir() {
    clear_runtime_closure_macros();

    let code = r#"
        $t1 = sys_get_temp_dir();
        $t2 = File::sysGetTempDir();
        $t3 = File::sys_get_temp_dir();
        $t4 = File::getTempDir();
        $t5 = File::tempDir();

        $is_dir1 = File::isDirectory($t1);
        $is_dir2 = File::isDirectory($t2);

        return [$t1, $t2, $t3, $t4, $t5, $is_dir1, $is_dir2];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        let expected_dir = std::env::temp_dir().to_string_lossy().to_string();
        let mut clean_expected = expected_dir;
        if clean_expected.len() > 1 && (clean_expected.ends_with('/') || clean_expected.ends_with('\\')) {
            clean_expected.pop();
        }

        assert_eq!(b[0], PhpValue::string(&clean_expected));
        assert_eq!(b[1], PhpValue::string(&clean_expected));
        assert_eq!(b[2], PhpValue::string(&clean_expected));
        assert_eq!(b[3], PhpValue::string(&clean_expected));
        assert_eq!(b[4], PhpValue::string(&clean_expected));
        assert_eq!(b[5], PhpValue::Bool(true));
        assert_eq!(b[6], PhpValue::Bool(true));
    } else {
        panic!("expected array return value, got {:?}", res);
    }
}

#[test]
fn test_tempnam_creation_and_fallback() {
    clear_runtime_closure_macros();

    let code = r#"
        // 1. Procedural tempnam with sys temp dir
        $f1 = tempnam(sys_get_temp_dir(), "rhp_");
        $exists1 = File::exists($f1);
        $name1 = File::basename($f1);

        // 2. File facade tempnam and aliases
        $f2 = File::tempnam(null, "custom_");
        $exists2 = File::exists($f2);

        $f3 = File::createTempFile(null, "alias1_");
        $exists3 = File::exists($f3);

        $f4 = File::tempFile(null, "alias2_");
        $exists4 = File::exists($f4);

        // 3. Fallback to system temp directory when provided dir doesn't exist
        $f5 = tempnam("/non_existent_rhp_directory_xyz123", "fallback_");
        $exists5 = File::exists($f5);

        // Clean up files
        File::delete($f1);
        File::delete($f2);
        File::delete($f3);
        File::delete($f4);
        File::delete($f5);

        return [
            $exists1, Str::startsWith($name1, "rhp_"),
            $exists2, File::exists($f2),
            $exists3, File::exists($f3),
            $exists4, File::exists($f4),
            $exists5, File::exists($f5),
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(true));
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(false)); // deleted
        assert_eq!(b[4], PhpValue::Bool(true));
        assert_eq!(b[5], PhpValue::Bool(false)); // deleted
        assert_eq!(b[6], PhpValue::Bool(true));
        assert_eq!(b[7], PhpValue::Bool(false)); // deleted
        assert_eq!(b[8], PhpValue::Bool(true));  // created via fallback
        assert_eq!(b[9], PhpValue::Bool(false)); // deleted
    } else {
        panic!("expected array return value, got {:?}", res);
    }
}

#[test]
fn test_tmpfile_stream_lifecycle_and_deletion() {
    clear_runtime_closure_macros();

    let code = r#"
        // 1. Procedural tmpfile
        $fp = tmpfile();
        $is_res = is_resource($fp);

        // Write and read
        $written = fwrite($fp, "hello rhp tempfile stream");
        rewind($fp);
        $content = fread($fp, 1024);

        // Close stream
        $closed = fclose($fp);
        $closed_again = fclose($fp); // Should return false when already closed

        // 2. File::tmpfile() facade
        $fp2 = File::tmpfile();
        $written2 = File::writeStream($fp2, "facade stream test");
        File::rewind($fp2);
        $content2 = File::readStream($fp2, 1024);
        $closed2 = File::close($fp2);

        return [
            $is_res, $written, $content, $closed, $closed_again,
            $written2, $content2, $closed2
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Int(25));
        assert_eq!(b[2], PhpValue::string("hello rhp tempfile stream"));
        assert_eq!(b[3], PhpValue::Bool(true));
        assert_eq!(b[4], PhpValue::Bool(false));
        assert_eq!(b[5], PhpValue::Int(18));
        assert_eq!(b[6], PhpValue::string("facade stream test"));
        assert_eq!(b[7], PhpValue::Bool(true));
    } else {
        panic!("expected array return value, got {:?}", res);
    }
}

#[test]
fn test_temporary_directory_creation() {
    clear_runtime_closure_macros();

    let code = r#"
        // 1. Create temp directory with default mode ("0700")
        $d1 = File::createTempDirectory(null, "rhp_dir_");
        $exists1 = File::isDirectory($d1);

        // 2. Create temp directory with explicit valid string permissions ("0755")
        $d2 = File::tempDirectory(null, "rhp_d2_", "0755");
        $exists2 = File::isDirectory($d2);

        // 3. Symbolic permission string ("rwxr-xr-x")
        $d3 = File::createTempDir(null, "rhp_d3_", "rwxr-xr-x");
        $exists3 = File::isDirectory($d3);

        // 4. Invalid permission mode (integer or invalid string must return false)
        $f_int = File::createTempDirectory(null, "bad_", 0755);
        $f_invalid = File::createTempDirectory(null, "bad_", "invalid_mode");

        // Clean up created directories
        File::delete($d1); // rmdir or delete
        File::delete($d2);
        File::delete($d3);

        return [
            $exists1, $exists2, $exists3,
            $f_int, $f_invalid
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(true));
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(false)); // integer rejected
        assert_eq!(b[4], PhpValue::Bool(false)); // invalid string rejected
    } else {
        panic!("expected array return value, got {:?}", res);
    }
}

#[test]
fn test_afile_temp_helpers() {
    clear_runtime_closure_macros();

    let code = r#"
        $fut_dir = AFile::sysGetTempDir();
        $sys_tmp = $fut_dir->wait();

        $fut_file = AFile::tempnam(null, "async_tmp_");
        $file_path = $fut_file->wait();
        $file_exists = File::exists($file_path);

        $fut_tdir = AFile::createTempDirectory(null, "async_tdir_", "0700");
        $tdir_path = $fut_tdir->wait();
        $tdir_exists = File::isDirectory($tdir_path);

        // Clean up
        File::delete($file_path);
        File::delete($tdir_path);

        return [
            File::isDirectory($sys_tmp),
            $file_exists,
            $tdir_exists,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(true));
        assert_eq!(b[2], PhpValue::Bool(true));
    } else {
        panic!("expected array return value, got {:?}", res);
    }
}

#[test]
fn test_realpath() {
    clear_runtime_closure_macros();

    let code = r#"
        // 1. Procedural realpath
        $p1 = realpath(".");
        $p2 = realpath("./tests/../src");
        $bad1 = realpath("/non_existent_path_rhp_xyz_12345");

        // 2. File facade realpath & aliases
        $f1 = File::realpath(".");
        $f2 = File::realPath("./tests/../src");
        $f3 = File::canonicalize("./tests/../src");
        $bad2 = File::realpath("/non_existent_path_rhp_xyz_12345");

        // 3. AFile async realpath
        $afut1 = AFile::realpath(".");
        $ares1 = $afut1->wait();
        $afut2 = AFile::realpath("/non_existent_path_rhp_xyz_12345");
        $ares2 = $afut2->wait();

        return [
            $p1, $p2, $bad1,
            $f1, $f2, $f3, $bad2,
            $ares1, $ares2
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        let cwd = std::fs::canonicalize(".").expect("cwd canonicalize");
        let cwd_str = cwd.to_string_lossy().to_string();
        let src_path = std::fs::canonicalize("src").expect("src canonicalize");
        let src_str = src_path.to_string_lossy().to_string();

        // Procedural
        assert_eq!(b[0], PhpValue::string(&cwd_str));
        assert_eq!(b[1], PhpValue::string(&src_str));
        assert_eq!(b[2], PhpValue::Bool(false));

        // Facade
        assert_eq!(b[3], PhpValue::string(&cwd_str));
        assert_eq!(b[4], PhpValue::string(&src_str));
        assert_eq!(b[5], PhpValue::string(&src_str));
        assert_eq!(b[6], PhpValue::Bool(false));

        // Async
        assert_eq!(b[7], PhpValue::string(&cwd_str));
        assert_eq!(b[8], PhpValue::Bool(false));
    } else {
        panic!("expected array return value, got {:?}", res);
    }
}

#[test]
fn test_dirname_levels() {
    clear_runtime_closure_macros();

    let code = r#"
        $d1 = dirname("/a/b/c");
        $d2 = dirname("/a/b/c", 1);
        $d3 = dirname("/a/b/c", 2);
        $fd1 = File::dirname("/a/b/c");
        $fd2 = File::dirname("/a/b/c", 2);

        return [$d1, $d2, $d3, $fd1, $fd2];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::string("/a/b"));
        assert_eq!(b[1], PhpValue::string("/a/b"));
        assert_eq!(b[2], PhpValue::string("/a"));
        assert_eq!(b[3], PhpValue::string("/a/b"));
        assert_eq!(b[4], PhpValue::string("/a"));
    } else {
        panic!("expected array return value, got {:?}", res);
    }
}

