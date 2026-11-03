use rhp::run_source;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;

#[test]
fn test_file_parse_mode() {
    clear_runtime_closure_macros();

    let code = r#"
        // 1. Octal string permissions
        $p1 = File::parseMode("0755");
        $p2 = File::parseMode("755");
        $p3 = File::parseMode("0o755");
        $p4 = File::parseMode("0644");
        $p5 = File::parseMode("0777");
        $p6 = File::parseMode("0700");
        $p7 = File::parseMode("1777");
        $p8 = File::parseMode("0000");

        // 2. Symbolic string permissions
        $s1 = File::parseMode("rwxr-xr-x");
        $s2 = File::parseMode("-rwxr-xr-x");
        $s3 = File::parseMode("drwxr-xr-x");
        $s4 = File::parseMode("rw-r--r--");
        $s5 = File::parseMode("rwxrwxrwx");
        $s6 = File::parseMode("rwx------");
        $s7 = File::parseMode("---------");

        // 3. Failure cases (must return false)
        $f_int = File::parseMode(0o755);
        $f_dec = File::parseMode(755);
        $f_bool = File::parseMode(true);
        $f_inv_str = File::parseMode("invalid");
        $f_non_oct = File::parseMode("0855");
        $f_short_sym = File::parseMode("rwxr-x");
        $f_empty = File::parseMode("");

        // 4. Procedural parse_mode helper
        $proc_ok = parse_mode("0755");
        $proc_sym = parse_mode("rwxr-xr-x");
        $proc_fail = parse_mode("not_a_mode");

        return [
            $p1, $p2, $p3, $p4, $p5, $p6, $p7, $p8,
            $s1, $s2, $s3, $s4, $s5, $s6, $s7,
            $f_int, $f_dec, $f_bool, $f_inv_str, $f_non_oct, $f_short_sym, $f_empty,
            $proc_ok, $proc_sym, $proc_fail,
        ];
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        // Octal strings
        assert_eq!(b[0], PhpValue::Int(0o755)); // 493
        assert_eq!(b[1], PhpValue::Int(0o755));
        assert_eq!(b[2], PhpValue::Int(0o755));
        assert_eq!(b[3], PhpValue::Int(0o644)); // 420
        assert_eq!(b[4], PhpValue::Int(0o777)); // 511
        assert_eq!(b[5], PhpValue::Int(0o700)); // 448
        assert_eq!(b[6], PhpValue::Int(0o1777)); // 1023
        assert_eq!(b[7], PhpValue::Int(0));

        // Symbolic strings
        assert_eq!(b[8], PhpValue::Int(0o755));
        assert_eq!(b[9], PhpValue::Int(0o755));
        assert_eq!(b[10], PhpValue::Int(0o755));
        assert_eq!(b[11], PhpValue::Int(0o644));
        assert_eq!(b[12], PhpValue::Int(0o777));
        assert_eq!(b[13], PhpValue::Int(0o700));
        assert_eq!(b[14], PhpValue::Int(0));

        // Failures return false
        assert_eq!(b[15], PhpValue::Bool(false));
        assert_eq!(b[16], PhpValue::Bool(false));
        assert_eq!(b[17], PhpValue::Bool(false));
        assert_eq!(b[18], PhpValue::Bool(false));
        assert_eq!(b[19], PhpValue::Bool(false));
        assert_eq!(b[20], PhpValue::Bool(false));
        assert_eq!(b[21], PhpValue::Bool(false));

        // Procedural
        assert_eq!(b[22], PhpValue::Int(0o755));
        assert_eq!(b[23], PhpValue::Int(0o755));
        assert_eq!(b[24], PhpValue::Bool(false));
    } else {
        panic!("expected array return value");
    }
}

#[test]
fn test_file_mkdir_and_failure_handling() {
    clear_runtime_closure_macros();

    let temp = std::env::temp_dir();
    let unique_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let dir1 = temp.join(format!("rhp_test_mkdir_1_{}", unique_id));
    let dir2 = temp.join(format!("rhp_test_mkdir_2_{}", unique_id));
    let dir3_parent = temp.join(format!("rhp_test_mkdir_nested_{}", unique_id));
    let dir3_child = dir3_parent.join("sub").join("nested");
    let dir_int = temp.join(format!("rhp_test_mkdir_int_{}", unique_id));
    let dir_inv = temp.join(format!("rhp_test_mkdir_inv_{}", unique_id));
    let dir_alias = temp.join(format!("rhp_test_mkdir_alias_{}", unique_id));

    let d1_str = dir1.to_str().unwrap().replace("\\", "/");
    let d2_str = dir2.to_str().unwrap().replace("\\", "/");
    let d3_str = dir3_child.to_str().unwrap().replace("\\", "/");
    let d_int_str = dir_int.to_str().unwrap().replace("\\", "/");
    let d_inv_str = dir_inv.to_str().unwrap().replace("\\", "/");
    let d_alias_str = dir_alias.to_str().unwrap().replace("\\", "/");

    let code = format!(
        r#"
        // 1. File::mkdir with "0755"
        $ok1 = File::mkdir("{d1}", "0755");
        $exists1 = File::isDirectory("{d1}");
        $recreate = File::mkdir("{d1}", "0755"); // should fail because already exists

        // 2. Procedural mkdir with symbolic format "rwxr-xr-x"
        $ok2 = mkdir("{d2}", "rwxr-xr-x");
        $exists2 = File::isDirectory("{d2}");

        // 3. Recursive directory creation
        $ok3 = mkdir("{d3}", "0755", true);
        $exists3 = File::isDirectory("{d3}");

        // 4. Integer format permission MUST be rejected -> returns false, dir not created
        $fail_int = File::mkdir("{d_int}", 0o755);
        $exists_int = File::isDirectory("{d_int}");

        // 5. Invalid string format permission MUST be rejected -> returns false, dir not created
        $fail_inv = File::mkdir("{d_inv}", "not_a_valid_mode");
        $exists_inv = File::isDirectory("{d_inv}");

        // 6. File::makeDirectory alias
        $ok_alias = File::makeDirectory("{d_alias}", "0755");
        $exists_alias = File::isDirectory("{d_alias}");

        // 7. Non-recursive into nonexistent parent fails
        $fail_nonrec = File::mkdir("/nonexistent_parent_xyz_123/child", "0755", false);

        return [
            $ok1, $exists1, $recreate,
            $ok2, $exists2,
            $ok3, $exists3,
            $fail_int, $exists_int,
            $fail_inv, $exists_inv,
            $ok_alias, $exists_alias,
            $fail_nonrec,
        ];
        "#,
        d1 = d1_str,
        d2 = d2_str,
        d3 = d3_str,
        d_int = d_int_str,
        d_inv = d_inv_str,
        d_alias = d_alias_str,
    );

    let res = run_source(&code).expect("evaluation failed");

    // Cleanup created directories
    let _ = std::fs::remove_dir(&dir1);
    let _ = std::fs::remove_dir(&dir2);
    let _ = std::fs::remove_dir_all(&dir3_parent);
    let _ = std::fs::remove_dir(&dir_int);
    let _ = std::fs::remove_dir(&dir_inv);
    let _ = std::fs::remove_dir(&dir_alias);

    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        // 1. File::mkdir
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(true));
        assert_eq!(b[2], PhpValue::Bool(false)); // already exists

        // 2. Procedural mkdir with symbolic format
        assert_eq!(b[3], PhpValue::Bool(true));
        assert_eq!(b[4], PhpValue::Bool(true));

        // 3. Recursive directory creation
        assert_eq!(b[5], PhpValue::Bool(true));
        assert_eq!(b[6], PhpValue::Bool(true));

        // 4. Integer format rejected -> returns false and does not create dir
        assert_eq!(b[7], PhpValue::Bool(false));
        assert_eq!(b[8], PhpValue::Bool(false));

        // 5. Invalid string format rejected -> returns false and does not create dir
        assert_eq!(b[9], PhpValue::Bool(false));
        assert_eq!(b[10], PhpValue::Bool(false));

        // 6. File::makeDirectory alias
        assert_eq!(b[11], PhpValue::Bool(true));
        assert_eq!(b[12], PhpValue::Bool(true));

        // 7. Non-recursive into nonexistent parent fails
        assert_eq!(b[13], PhpValue::Bool(false));
    } else {
        panic!("expected array return value");
    }
}

#[test]
fn test_file_chmod_string_format_only() {
    clear_runtime_closure_macros();

    let temp = std::env::temp_dir();
    let unique_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let file_path = temp.join(format!("rhp_test_chmod_{}.txt", unique_id));
    std::fs::write(&file_path, "testing chmod").unwrap();
    let f_str = file_path.to_str().unwrap().replace("\\", "/");

    let code = format!(
        r#"
        // 1. Octal string format
        $ok_oct = File::chmod("{f}", "0644");
        $mode_oct = File::chmod("{f}");

        // 2. Symbolic string format
        $ok_sym = File::chmod("{f}", "rwxr-xr-x");
        $mode_sym = File::chmod("{f}");

        // 3. Integer mode is rejected -> returns false
        $fail_int = File::chmod("{f}", 0o755);

        // 4. Invalid string mode is rejected -> returns false
        $fail_inv = File::chmod("{f}", "invalid_permissions");

        // 5. Procedural chmod
        $proc_ok = chmod("{f}", "0600");
        $proc_fail = chmod("{f}", 0o600);

        return [
            $ok_oct, $mode_oct,
            $ok_sym, $mode_sym,
            $fail_int, $fail_inv,
            $proc_ok, $proc_fail,
        ];
        "#,
        f = f_str,
    );

    let res = run_source(&code).expect("evaluation failed");
    let _ = std::fs::remove_file(&file_path);

    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        #[cfg(unix)]
        assert_eq!(b[1].to_php_string(), "0644");

        assert_eq!(b[2], PhpValue::Bool(true));
        #[cfg(unix)]
        assert_eq!(b[3].to_php_string(), "0755");

        // Integers and invalid formats are rejected
        assert_eq!(b[4], PhpValue::Bool(false));
        assert_eq!(b[5], PhpValue::Bool(false));

        // Procedural
        assert_eq!(b[6], PhpValue::Bool(true));
        assert_eq!(b[7], PhpValue::Bool(false));
    } else {
        panic!("expected array return value");
    }
}

#[test]
fn test_afile_mkdir_and_chmod() {
    clear_runtime_closure_macros();

    let temp = std::env::temp_dir();
    let unique_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let adir = temp.join(format!("rhp_test_afile_mkdir_{}", unique_id));
    let adir_str = adir.to_str().unwrap().replace("\\", "/");

    let code = format!(
        r#"
        $fut1 = AFile::mkdir("{d}", "0755");
        $ok1 = $fut1->wait();

        $fut2 = AFile::chmod("{d}", "0700");
        $ok2 = $fut2->wait();

        // Integer mode rejected in async mkdir as well
        $fut3 = AFile::mkdir("{d}_fail", 0o755);
        $fail_int = $fut3->wait();

        return [$ok1, $ok2, $fail_int];
        "#,
        d = adir_str,
    );

    let res = run_source(&code).expect("evaluation failed");
    let _ = std::fs::remove_dir(&adir);

    if let PhpValue::Array(a) | PhpValue::List(a) = res {
        let b = a.borrow();
        assert_eq!(b[0], PhpValue::Bool(true));
        assert_eq!(b[1], PhpValue::Bool(true));
        assert_eq!(b[2], PhpValue::Bool(false));
    } else {
        panic!("expected array return value");
    }
}

