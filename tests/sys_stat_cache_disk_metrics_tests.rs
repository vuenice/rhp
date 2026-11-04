use rhp::vm::facade::stat_cache_cleared_count;
use rhp::vm::run_source_with_vm;
use rhp::vm::value::PhpValue;

#[test]
fn test_clearstatcache_facade_and_procedural() {
    let initial_cleared = stat_cache_cleared_count();

    // 1. Sys::clearstatcache() with various arguments
    let source = r#"
        Sys::clearstatcache();
        Sys::clearstatcache(true);
        Sys::clearstatcache(false, "foo.txt");
        Sys::clearStatCache();
        clearstatcache();
        clearstatcache(true, "bar.txt");
        File::clearStatCache();

        echo "CLEARED";
    "#;

    let (vm, val) = run_source_with_vm(source).expect("Execution should succeed");
    assert_eq!(vm.output(), "CLEARED");
    assert_eq!(val, PhpValue::Null);

    let after_cleared = stat_cache_cleared_count();
    assert!(after_cleared >= initial_cleared + 7);
}

#[test]
fn test_disk_free_space_facade_and_procedural() {
    let source = r#"
        $free1 = Sys::disk_free_space(".");
        $free2 = Sys::diskFreeSpace(".");
        $free3 = Sys::diskfreespace(".");
        $free4 = disk_free_space(".");
        $free5 = diskfreespace(".");
        $free6 = File::diskFreeSpace(".");

        $is_float1 = Validate::float($free1);
        $is_positive = $free1 > 0.0;

        echo $is_float1 ? "F" : "X";
        echo $is_positive ? "P" : "X";
        echo $free2 > 0.0 ? "2" : "X";
        echo $free3 > 0.0 ? "3" : "X";
        echo $free4 > 0.0 ? "4" : "X";
        echo $free5 > 0.0 ? "5" : "X";
        echo $free6 > 0.0 ? "6" : "X";
    "#;

    let (vm, _) = run_source_with_vm(source).expect("disk_free_space should succeed");
    assert_eq!(vm.output(), "FP23456");
}

#[test]
fn test_disk_total_space_facade_and_procedural() {
    let source = r#"
        $total1 = Sys::disk_total_space(".");
        $total2 = Sys::diskTotalSpace(".");
        $total3 = disk_total_space(".");
        $total4 = File::diskTotalSpace(".");

        $is_float1 = Validate::float($total1);
        $is_positive = $total1 > 0.0;

        echo $is_float1 ? "F" : "X";
        echo $is_positive ? "P" : "X";
        echo $total2 > 0.0 ? "2" : "X";
        echo $total3 > 0.0 ? "3" : "X";
        echo $total4 > 0.0 ? "4" : "X";
    "#;

    let (vm, _) = run_source_with_vm(source).expect("disk_total_space should succeed");
    assert_eq!(vm.output(), "FP234");
}

#[test]
fn test_disk_space_total_vs_free_relation() {
    let source = r#"
        $total = Sys::disk_total_space(".");
        $free = Sys::disk_free_space(".");

        $valid_relation = $total >= $free && $free > 0.0;
        echo $valid_relation ? "VALID" : "INVALID";
    "#;

    let (vm, _) = run_source_with_vm(source).expect("relation check should succeed");
    assert_eq!(vm.output(), "VALID");
}

#[test]
fn test_disk_space_invalid_paths() {
    let source = r#"
        $f_invalid = Sys::disk_free_space("/non_existent_directory_rhp_test_12345");
        $t_invalid = Sys::disk_total_space("/non_existent_directory_rhp_test_12345");
        $f_empty = Sys::disk_free_space("");
        $t_empty = Sys::disk_total_space("");

        echo $f_invalid == false ? "1" : "0";
        echo $t_invalid == false ? "1" : "0";
        echo $f_empty == false ? "1" : "0";
        echo $t_empty == false ? "1" : "0";
    "#;

    let (vm, _) = run_source_with_vm(source).expect("invalid paths should return false");
    assert_eq!(vm.output(), "1111");
}
