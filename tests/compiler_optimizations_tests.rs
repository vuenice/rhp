use rhp::{compile_source, PhpValue, VM};

#[test]
fn test_stream_fusion_pipeline_executes_and_increments_counter() {
    let code = r#"
    $nums = Arr::list([1, 2, 3, 4, 5]);
    $res = $nums->map(fn($x) => $x * 2)->filter(fn($x) => $x > 4)->take(2);
    "#;
    let module = compile_source(code).expect("Compile failed");
    let mut vm = VM::new(module, false);
    vm.run().expect("Run failed");
    let res = vm.get_main_variable("res").cloned().expect("res not found");

    if let PhpValue::List(l) = res {
        assert_eq!(*l.borrow(), vec![PhpValue::Int(6), PhpValue::Int(8)]);
    } else {
        panic!("Expected List result, got {:?}", res);
    }

    assert_eq!(
        vm.stream_fusions, 1,
        "Expected exactly 1 stream fusion executed on this VM"
    );
}

#[test]
fn test_fbip_inplace_sort_unique_buffer() {
    let code = r#"
    $nums = Arr::list([5, 2, 9, 1, 3]);
    $nums = $nums->sort();
    "#;
    let module = compile_source(code).expect("Compile failed");
    let mut vm = VM::new(module, false);
    vm.run().expect("Run failed");
    let nums = vm.get_main_variable("nums").cloned().expect("nums not found");

    if let PhpValue::List(l) = nums {
        assert_eq!(
            *l.borrow(),
            vec![
                PhpValue::Int(1),
                PhpValue::Int(2),
                PhpValue::Int(3),
                PhpValue::Int(5),
                PhpValue::Int(9)
            ]
        );
    } else {
        panic!("Expected List result, got {:?}", nums);
    }

    // $nums = $nums->sort() with refcount == 1 triggers FBIP in-place sort!
    assert_eq!(
        vm.fbip_inplace_sorts, 1,
        "Expected exactly 1 FBIP in-place sort executed on this VM"
    );
}

#[test]
fn test_fbip_shared_buffer_safety_cow() {
    let code = r#"
    $nums = Arr::list([5, 2, 9, 1, 3]);
    $alias = $nums; // creates second alias, ref_count is 2!
    $sorted = $nums->sort();
    "#;
    let module = compile_source(code).expect("Compile failed");
    let mut vm = VM::new(module, false);
    vm.run().expect("Run failed");
    let sorted = vm.get_main_variable("sorted").cloned().expect("sorted not found");
    let alias = vm.get_main_variable("alias").cloned().expect("alias not found");

    if let PhpValue::List(l) = sorted {
        assert_eq!(
            *l.borrow(),
            vec![
                PhpValue::Int(1),
                PhpValue::Int(2),
                PhpValue::Int(3),
                PhpValue::Int(5),
                PhpValue::Int(9)
            ]
        );
    } else {
        panic!("Expected List result, got {:?}", sorted);
    }

    if let PhpValue::List(l) = alias {
        // Alias is STILL the original unsorted list!
        assert_eq!(
            *l.borrow(),
            vec![
                PhpValue::Int(5),
                PhpValue::Int(2),
                PhpValue::Int(9),
                PhpValue::Int(1),
                PhpValue::Int(3)
            ]
        );
    } else {
        panic!("Expected List alias, got {:?}", alias);
    }

    // Because $alias exists, $nums was shared! So FBIP in-place sort must NOT mutate the shared buffer!
    assert_eq!(
        vm.fbip_inplace_sorts, 0,
        "Shared buffer must NOT trigger FBIP in-place sort"
    );
}

#[test]
fn test_stream_fusion_early_exit_short_circuits() {
    let code = r#"
    $big = Arr::list([]);
    for ($i = 0; $i < 1000; $i++) {
        $big[] = $i;
    }
    $res = $big->map(fn($x) => $x * 10)->filter(fn($x) => $x >= 100)->take(3);
    "#;
    let module = compile_source(code).expect("Compile failed");
    let mut vm = VM::new(module, false);
    vm.run().expect("Run failed");
    let res = vm.get_main_variable("res").cloned().expect("res not found");

    if let PhpValue::List(l) = res {
        assert_eq!(
            *l.borrow(),
            vec![PhpValue::Int(100), PhpValue::Int(110), PhpValue::Int(120)]
        );
    } else {
        panic!("Expected List result, got {:?}", res);
    }
    assert_eq!(vm.stream_fusions, 1);
}

#[test]
fn test_stream_fusion_skip_and_reject() {
    let code = r#"
    $items = Arr::list([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    $res = $items->reject(fn($x) => $x % 2 == 0)->skip(1)->take(3);
    "#;
    let module = compile_source(code).expect("Compile failed");
    let mut vm = VM::new(module, false);
    vm.run().expect("Run failed");
    let res = vm.get_main_variable("res").cloned().expect("res not found");

    // Odds: [1, 3, 5, 7, 9]
    // Skip 1: [3, 5, 7, 9]
    // Take 3: [3, 5, 7]
    if let PhpValue::List(l) = res {
        assert_eq!(
            *l.borrow(),
            vec![PhpValue::Int(3), PhpValue::Int(5), PhpValue::Int(7)]
        );
    } else {
        panic!("Expected List result, got {:?}", res);
    }
    assert_eq!(vm.stream_fusions, 1);
}

#[test]
fn test_stream_fusion_does_not_fuse_impure_closures() {
    // Closure with by-reference capture is impure (ImpureReference)
    let code = r#"
    $counter = 0;
    $items = Arr::list([1, 2, 3, 4]);
    $res = $items->map(function($x) use (&$counter) {
        $counter++;
        return $x * 2;
    })->filter(fn($x) => $x > 4);
    "#;
    let program = rhp::parse(code).expect("Parse failed");
    let module = rhp::lower_program(&program, None).expect("Lower failed");
    let mut vm = VM::new(module, false);
    vm.run().expect("Run failed");
    let res = vm.get_main_variable("res").cloned().expect("res not found");

    if let PhpValue::List(l) = res {
        assert_eq!(*l.borrow(), vec![PhpValue::Int(6), PhpValue::Int(8)]);
    } else {
        panic!("Expected List result, got {:?}", res);
    }

    // Impure closure must fall back to normal chained execution (vm.stream_fusions == 0)
    assert_eq!(
        vm.stream_fusions, 0,
        "Impure reference closure must NOT be fused"
    );
}

#[test]
fn test_fbip_natsort_inplace_unique_buffer() {
    let code = r#"
    $files = Arr::list(["img12.png", "img10.png", "img2.png", "img1.png"]);
    $files = $files->natsort();
    "#;
    let module = compile_source(code).expect("Compile failed");
    let mut vm = VM::new(module, false);
    vm.run().expect("Run failed");
    let files = vm.get_main_variable("files").cloned().expect("files not found");

    if let PhpValue::List(l) = files {
        let items: Vec<String> = l.borrow().iter().map(|v| v.to_php_string()).collect();
        assert_eq!(
            items,
            vec!["img1.png", "img2.png", "img10.png", "img12.png"]
        );
    } else {
        panic!("Expected List result, got {:?}", files);
    }

    assert_eq!(
        vm.fbip_inplace_sorts, 1,
        "Expected 1 FBIP in-place natsort"
    );
}
