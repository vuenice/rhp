use rhp::vm::run_source_with_vm;

#[test]
fn test_sys_print_and_echo_formatting_examples() {
    // 1. Floating-point precision (C: %.2f)
    let source1 = r#"
        Sys::print("{:.2}", 3.141592);
    "#;
    let (vm1, _) = run_source_with_vm(source1).expect("Sys::print float precision should succeed");
    assert_eq!(vm1.output(), "3.14\n");

    let source1_echo = r#"
        Sys::echo("{:.2}", 3.141592);
    "#;
    let (vm1_echo, _) = run_source_with_vm(source1_echo).expect("Sys::echo float precision should succeed");
    assert_eq!(vm1_echo.output(), "3.14");

    // 2. Integer padding with zeros (C: %05d)
    let source2 = r#"
        Sys::print("{:05}", 42);
    "#;
    let (vm2, _) = run_source_with_vm(source2).expect("Sys::print zero padding should succeed");
    assert_eq!(vm2.output(), "00042\n");

    let source2_echo = r#"
        Sys::echo("{:05}", 42);
    "#;
    let (vm2_echo, _) = run_source_with_vm(source2_echo).expect("Sys::echo zero padding should succeed");
    assert_eq!(vm2_echo.output(), "00042");

    // 3. Hexadecimal lowercase (C: %x)
    let source3 = r#"
        Sys::print("{:x}", 255);
    "#;
    let (vm3, _) = run_source_with_vm(source3).expect("Sys::print hex should succeed");
    assert_eq!(vm3.output(), "ff\n");

    let source3_echo = r#"
        Sys::echo("{:x}", 255);
    "#;
    let (vm3_echo, _) = run_source_with_vm(source3_echo).expect("Sys::echo hex should succeed");
    assert_eq!(vm3_echo.output(), "ff");

    // 4. Binary formatting
    let source4 = r#"
        Sys::print("{:b}", 5);
    "#;
    let (vm4, _) = run_source_with_vm(source4).expect("Sys::print binary should succeed");
    assert_eq!(vm4.output(), "101\n");

    let source4_echo = r#"
        Sys::echo("{:b}", 5);
    "#;
    let (vm4_echo, _) = run_source_with_vm(source4_echo).expect("Sys::echo binary should succeed");
    assert_eq!(vm4_echo.output(), "101");

    // 5. General multiple arguments
    let source5 = r#"
        Sys::print("Age: {}, Name: {}", 30, "Bob");
    "#;
    let (vm5, _) = run_source_with_vm(source5).expect("Sys::print multi-arg should succeed");
    assert_eq!(vm5.output(), "Age: 30, Name: Bob\n");

    let source5_echo = r#"
        Sys::echo("Age: {}, Name: {}", 30, "Bob");
    "#;
    let (vm5_echo, _) = run_source_with_vm(source5_echo).expect("Sys::echo multi-arg should succeed");
    assert_eq!(vm5_echo.output(), "Age: 30, Name: Bob");
}

#[test]
fn test_rust_macro_invocation_syntax() {
    let source = r#"
        println!("Age: {}, Name: {}", 30, "Bob");
        println!("{:.2}", 3.141592);
        println!("{:05}", 42);
        println!("{:x}", 255);
        println!("{:b}", 5);
        print!("Same line: {}", 123);
        print!(" -- continuing same line");
        println!("");
    "#;
    let (vm, _) = run_source_with_vm(source).expect("Rust macro syntax execution should succeed");
    let expected = concat!(
        "Age: 30, Name: Bob\n",
        "3.14\n",
        "00042\n",
        "ff\n",
        "101\n",
        "Same line: 123 -- continuing same line\n"
    );
    assert_eq!(vm.output(), expected);
}

#[test]
fn test_global_intrinsics_and_echo_statement_formatting() {
    let source = r#"
        echo("Echo function: {}\n", "Works");
        echo "Echo statement: {}\n", 99;
        print("Print intrinsic: {}\n", 100);
        println("Println intrinsic: {}", 200);
    "#;
    let (vm, _) = run_source_with_vm(source).expect("Global helpers should succeed");
    let expected = concat!(
        "Echo function: Works\n",
        "Echo statement: 99\n",
        "Print intrinsic: 100\n\n",
        "Println intrinsic: 200\n"
    );
    assert_eq!(vm.output(), expected);
}

#[test]
fn test_echo_and_print_contrast() {
    let source = r#"
        Sys::echo("A");
        Sys::echo("B");
        Sys::echo("C");
        Sys::print("D");
        Sys::print("E");
    "#;
    let (vm, _) = run_source_with_vm(source).expect("Echo vs Print newline contrast test should succeed");
    assert_eq!(vm.output(), "ABCD\nE\n");
}

#[test]
fn test_sys_println_does_not_exist() {
    let source = r#"
        Sys::println("This should fail because Sys::println does not exist");
    "#;
    let res = run_source_with_vm(source);
    assert!(res.is_err(), "Sys::println should not exist, only echo and print");
}

#[test]
fn test_print_r_array_and_dict() {
    let source = r#"
        $data = [
            "name" => "RHP Playground",
            "mode" => "Standalone",
        ];
        print_r($data);
    "#;
    let (vm, _) = run_source_with_vm(source).expect("print_r on dict should succeed");
    let expected = concat!(
        "Array\n",
        "(\n",
        "    [name] => RHP Playground\n",
        "    [mode] => Standalone\n",
        ")\n"
    );
    assert_eq!(vm.output(), expected);

    let source2 = r#"
        $nums = [1, 2, 3];
        print_r($nums);
    "#;
    let (vm2, _) = run_source_with_vm(source2).expect("print_r on array should succeed");
    let expected2 = concat!(
        "Array\n",
        "(\n",
        "    [0] => 1\n",
        "    [1] => 2\n",
        "    [2] => 3\n",
        ")\n"
    );
    assert_eq!(vm2.output(), expected2);
}

#[test]
fn test_print_r_return_string_flag() {
    let source = r#"
        $data = ["key" => "value"];
        $str = print_r($data, true);
        echo $str;
    "#;
    let (vm, _) = run_source_with_vm(source).expect("print_r return flag should succeed");
    let expected = concat!(
        "Array\n",
        "(\n",
        "    [key] => value\n",
        ")"
    );
    assert_eq!(vm.output(), expected);
}

