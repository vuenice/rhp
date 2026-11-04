use rhp::{is_legacy_php_function, is_static_facade, is_static_only, run_source_with_vm, PhpValue};

#[test]
fn test_csv_facade_registration_and_signature_mapping() {
    // 1. Verify Csv is registered as a native static facade
    assert!(is_static_facade("Csv"));
    assert!(is_static_only("Csv"));

    // 2. Verify legacy CSV and stream PHP functions are recognized
    let legacy_funcs = [
        ("fgetcsv", "Csv", "fgetcsv"),
        ("fputcsv", "Csv", "fputcsv"),
        ("str_getcsv", "Csv", "parseRow"),
        ("fopen", "File", "open"),
        ("fclose", "File", "close"),
        ("feof", "File", "eof"),
        ("rewind", "File", "rewind"),
        ("is_resource", "Validate", "resource"),
    ];

    for (php_fn, facade, method) in legacy_funcs {
        assert!(
            is_legacy_php_function(php_fn),
            "Function {} should be recognized as legacy PHP function",
            php_fn
        );
        let mapping = rhp::get_legacy_php_function(php_fn)
            .unwrap_or_else(|| panic!("Mapping for {} should exist", php_fn));
        assert_eq!(mapping.php_name, php_fn);
        assert_eq!(mapping.rhp_facade, facade);
        assert_eq!(mapping.rhp_method, method);
    }
}

#[test]
fn test_csv_facade_memory_stream_fputcsv_and_fgetcsv() {
    let source = r#"
        $fp = Csv::memory();
        $isRes = Validate::resource($fp);

        $bytes1 = Csv::fputcsv($fp, ["id", "name", "role"]);
        $bytes2 = Csv::fputcsv($fp, [1, "Alice, Senior Eng", "Admin"]);
        $bytes3 = Csv::fputcsv($fp, [2, "Bob", "User"]);

        Csv::rewind($fp);

        $row1 = Csv::fgetcsv($fp);
        $row2 = Csv::fgetcsv($fp);
        $row3 = Csv::fgetcsv($fp);
        $row4 = Csv::fgetcsv($fp); // EOF -> false

        $eof = Csv::feof($fp);
        $closed = Csv::close($fp);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Csv memory stream processing should execute");

    assert_eq!(vm.get_main_variable("$isRes"), Some(&PhpValue::Bool(true)));

    if let Some(PhpValue::Int(b1)) = vm.get_main_variable("$bytes1") {
        assert!(*b1 > 0);
    } else {
        panic!("expected int bytes written for bytes1");
    }

    // Row 1: id, name, role
    if let Some(PhpValue::List(r1)) = vm.get_main_variable("$row1") {
        let items: Vec<String> = r1.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["id", "name", "role"]);
    } else {
        panic!("expected list for row1");
    }

    // Row 2: 1, Alice, Senior Eng, Admin
    if let Some(PhpValue::List(r2)) = vm.get_main_variable("$row2") {
        let items: Vec<String> = r2.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["1", "Alice, Senior Eng", "Admin"]);
    } else {
        panic!("expected list for row2");
    }

    // Row 3: 2, Bob, User
    if let Some(PhpValue::List(r3)) = vm.get_main_variable("$row3") {
        let items: Vec<String> = r3.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["2", "Bob", "User"]);
    } else {
        panic!("expected list for row3");
    }

    // Row 4: EOF -> false
    assert_eq!(vm.get_main_variable("$row4"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$eof"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$closed"), Some(&PhpValue::Bool(true)));
}

#[test]
fn test_csv_fgetcsv_multiline_and_escaping() {
    let source = r#"
        $csvData = "id,description,status\n1,\"Line 1\nLine 2\",active\n2,\"He said \"\"Hello\"\"\",pending\n";
        $stream = Csv::memory($csvData);

        $row1 = Csv::fgetcsv($stream);
        $row2 = Csv::fgetcsv($stream);
        $row3 = Csv::fgetcsv($stream);
        $row4 = Csv::fgetcsv($stream);

        Csv::close($stream);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("CSV with multiline and quotes should parse");

    // Row 1
    if let Some(PhpValue::List(r1)) = vm.get_main_variable("$row1") {
        let items: Vec<String> = r1.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["id", "description", "status"]);
    } else {
        panic!("expected list for row1");
    }

    // Row 2: multiline quoted field
    if let Some(PhpValue::List(r2)) = vm.get_main_variable("$row2") {
        let items: Vec<String> = r2.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["1", "Line 1\nLine 2", "active"]);
    } else {
        panic!("expected list for row2");
    }

    // Row 3: doubled quotes
    if let Some(PhpValue::List(r3)) = vm.get_main_variable("$row3") {
        let items: Vec<String> = r3.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["2", "He said \"Hello\"", "pending"]);
    } else {
        panic!("expected list for row3");
    }

    // Row 4: EOF
    assert_eq!(vm.get_main_variable("$row4"), Some(&PhpValue::Bool(false)));
}

#[test]
fn test_csv_custom_delimiter_and_enclosure() {
    let source = r#"
        $data = "apple|'red; delicious'|100\nbanana|yellow|200\n";
        $stream = Csv::memory($data);

        $row1 = Csv::fgetcsv($stream, null, "|", "'");
        $row2 = Csv::fgetcsv($stream, null, "|", "'");
        $row3 = Csv::fgetcsv($stream, null, "|", "'");

        Csv::close($stream);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Custom delimiter CSV should parse");

    if let Some(PhpValue::List(r1)) = vm.get_main_variable("$row1") {
        let items: Vec<String> = r1.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["apple", "red; delicious", "100"]);
    } else {
        panic!("expected list for row1");
    }

    if let Some(PhpValue::List(r2)) = vm.get_main_variable("$row2") {
        let items: Vec<String> = r2.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["banana", "yellow", "200"]);
    } else {
        panic!("expected list for row2");
    }

    assert_eq!(vm.get_main_variable("$row3"), Some(&PhpValue::Bool(false)));
}

#[test]
fn test_csv_blank_lines() {
    let source = r#"
        $data = "a,b\n\nc,d\n";
        $stream = Csv::memory($data);

        $row1 = Csv::fgetcsv($stream);
        $row2 = Csv::fgetcsv($stream); // blank line -> [null]
        $row3 = Csv::fgetcsv($stream);
        $row4 = Csv::fgetcsv($stream); // EOF -> false

        Csv::close($stream);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Blank lines test should execute");

    if let Some(PhpValue::List(r1)) = vm.get_main_variable("$row1") {
        let items: Vec<String> = r1.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["a", "b"]);
    } else {
        panic!("expected list for row1");
    }

    // Blank line in PHP returns [null]
    if let Some(PhpValue::List(r2)) = vm.get_main_variable("$row2") {
        assert_eq!(r2.borrow().len(), 1);
        assert_eq!(r2.borrow()[0], PhpValue::Null);
    } else {
        panic!("expected list for row2");
    }

    if let Some(PhpValue::List(r3)) = vm.get_main_variable("$row3") {
        let items: Vec<String> = r3.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["c", "d"]);
    } else {
        panic!("expected list for row3");
    }

    assert_eq!(vm.get_main_variable("$row4"), Some(&PhpValue::Bool(false)));
}

#[test]
fn test_legacy_php_functions_lowering_csv_stream() {
    let source = r#"
        $fp = fopen("php://memory", "r+");
        $isRes = is_resource($fp);

        fputcsv($fp, ["name", "email", "notes"]);
        fputcsv($fp, ["John Doe", "john@example.com", "Engineer, Lead"]);
        rewind($fp);

        $rows = [];
        while (($row = fgetcsv($fp)) != false) {
            $rows[] = $row;
        }

        $atEof = feof($fp);
        fclose($fp);

        $single = str_getcsv("10,20,30,40");
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Legacy PHP functions should execute");

    assert_eq!(vm.get_main_variable("$isRes"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$atEof"), Some(&PhpValue::Bool(true)));

    if let Some(PhpValue::Array(rows) | PhpValue::List(rows)) = vm.get_main_variable("$rows") {
        assert_eq!(rows.borrow().len(), 2);
        if let PhpValue::List(ref r0) = rows.borrow()[0] {
            let items: Vec<String> = r0.borrow().iter().map(|v| v.to_php_string()).collect();
            assert_eq!(items, vec!["name", "email", "notes"]);
        }
        if let PhpValue::List(ref r1) = rows.borrow()[1] {
            let items: Vec<String> = r1.borrow().iter().map(|v| v.to_php_string()).collect();
            assert_eq!(items, vec!["John Doe", "john@example.com", "Engineer, Lead"]);
        }
    } else {
        panic!("expected array for rows");
    }

    if let Some(PhpValue::List(s)) = vm.get_main_variable("$single") {
        let items: Vec<String> = s.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["10", "20", "30", "40"]);
    } else {
        panic!("expected list for single");
    }
}

#[test]
fn test_csv_parse_and_generate_helpers() {
    let source = r#"
        $csvText = "id,name\n1,Alice\n2,Bob\n";
        $parsed = Csv::parse($csvText);

        $generated = Csv::generate($parsed);

        $rowText = Csv::formatRow(["hello", "world, wide"]);
        $parsedRow = Csv::parseRow($rowText);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Csv helpers should execute");

    if let Some(PhpValue::List(p)) = vm.get_main_variable("$parsed") {
        assert_eq!(p.borrow().len(), 3);
    } else {
        panic!("expected list for parsed");
    }

    if let Some(PhpValue::String(generated_str)) = vm.get_main_variable("$generated") {
        assert_eq!(generated_str.as_str(), "id,name\n1,Alice\n2,Bob\n");
    } else {
        panic!("expected string for generated");
    }

    if let Some(PhpValue::String(row_str)) = vm.get_main_variable("$rowText") {
        assert_eq!(row_str.as_str(), "hello,\"world, wide\"\n");
    } else {
        panic!("expected string for rowText");
    }

    if let Some(PhpValue::List(row)) = vm.get_main_variable("$parsedRow") {
        let items: Vec<String> = row.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(items, vec!["hello", "world, wide"]);
    } else {
        panic!("expected list for parsedRow");
    }
}

#[test]
fn test_csv_real_file_stream_io() {
    let test_file = "test_stream_processing_temp.csv";
    if std::path::Path::new(test_file).exists() {
        let _ = std::fs::remove_file(test_file);
    }

    let source = r#"
        $fp = Csv::open("test_stream_processing_temp.csv", "w");
        Csv::fputcsv($fp, ["colA", "colB", "colC"]);
        Csv::fputcsv($fp, ["val1", "val2, with comma", "val3"]);
        Csv::close($fp);

        $fp2 = Csv::open("test_stream_processing_temp.csv", "r");
        $all = [];
        while (($row = Csv::fgetcsv($fp2)) != false) {
            $all[] = $row;
        }
        Csv::close($fp2);

        File::delete("test_stream_processing_temp.csv");
    "#;

    let (vm, _) = run_source_with_vm(source).expect("File stream CSV processing should execute");

    if let Some(PhpValue::Array(all) | PhpValue::List(all)) = vm.get_main_variable("$all") {
        assert_eq!(all.borrow().len(), 2);
        if let PhpValue::List(ref r0) = all.borrow()[0] {
            let items: Vec<String> = r0.borrow().iter().map(|v| v.to_php_string()).collect();
            assert_eq!(items, vec!["colA", "colB", "colC"]);
        }
        if let PhpValue::List(ref r1) = all.borrow()[1] {
            let items: Vec<String> = r1.borrow().iter().map(|v| v.to_php_string()).collect();
            assert_eq!(items, vec!["val1", "val2, with comma", "val3"]);
        }
    } else {
        panic!("expected array for all");
    }

    assert!(!std::path::Path::new(test_file).exists());
}

#[test]
fn test_csv_read_write_file_convenience() {
    let test_file = "test_read_write_convenience.csv";
    if std::path::Path::new(test_file).exists() {
        let _ = std::fs::remove_file(test_file);
    }

    let source = r#"
        $rows = [
            ["SKU", "Item", "Price"],
            ["A100", "Widget", "19.99"],
            ["B200", "Gadget, Pro", "49.50"]
        ];
        $written = Csv::writeFile("test_read_write_convenience.csv", $rows);
        $readRows = Csv::readFile("test_read_write_convenience.csv");
        File::delete("test_read_write_convenience.csv");
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Csv writeFile and readFile should execute");

    assert_eq!(vm.get_main_variable("$written"), Some(&PhpValue::Bool(true)));

    if let Some(PhpValue::List(r)) = vm.get_main_variable("$readRows") {
        assert_eq!(r.borrow().len(), 3);
        if let PhpValue::List(ref r2) = r.borrow()[2] {
            let items: Vec<String> = r2.borrow().iter().map(|v| v.to_php_string()).collect();
            assert_eq!(items, vec!["B200", "Gadget, Pro", "49.50"]);
        }
    } else {
        panic!("expected list for readRows");
    }

    assert!(!std::path::Path::new(test_file).exists());
}

#[test]
fn test_csv_empty_stream_and_type_error() {
    let source = r#"
        $empty = Csv::memory("");
        $row = Csv::fgetcsv($empty);
        Csv::close($empty);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Empty stream should return false");
    assert_eq!(vm.get_main_variable("$row"), Some(&PhpValue::Bool(false)));

    let bad_source = r#"
        $res = Csv::fgetcsv("not_a_resource");
    "#;
    let err = run_source_with_vm(bad_source);
    assert!(err.is_err(), "Passing string to fgetcsv should error with TypeError");
}

