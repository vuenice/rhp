use rhp::run_source;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;

#[test]
fn test_list_direct_vec_memory_layout_and_cow_detachment() {
    clear_runtime_closure_macros();

    // 1. Direct Rust Vec<PhpValue> inspection
    let list = PhpValue::list(vec![
        PhpValue::Int(10),
        PhpValue::Int(20),
        PhpValue::Int(30),
    ]);

    assert!(list.is_list());
    assert_eq!(list.list_len(), Some(3));
    assert_eq!(list.list_get(0), Some(PhpValue::Int(10)));
    assert_eq!(list.list_get(1), Some(PhpValue::Int(20)));
    assert_eq!(list.list_get(2), Some(PhpValue::Int(30)));
    assert_eq!(list.list_get(3), None);

    // Initial reference count should be 1
    assert_eq!(list.ref_count(), 1);
    assert!(!list.is_shared());

    // Cloning PhpValue::List creates an O(1) shared handle
    let mut list_alias = list.clone();
    assert_eq!(list.ref_count(), 2);
    assert_eq!(list_alias.ref_count(), 2);
    assert!(list.is_shared());
    assert!(list_alias.is_shared());

    // Copy-On-Write (COW): ensure_list_mut detaches the underlying Vec<PhpValue>
    assert!(list_alias.ensure_list_mut());
    assert_eq!(list.ref_count(), 1);
    assert_eq!(list_alias.ref_count(), 1);
    assert!(!list.is_shared());
    assert!(!list_alias.is_shared());

    // Mutate detached list
    if let PhpValue::List(ref rc) = list_alias {
        rc.borrow_mut().push(PhpValue::Int(999));
    }
    assert_eq!(list.list_len(), Some(3));
    assert_eq!(list_alias.list_len(), Some(4));
    assert_eq!(list_alias.list_get(3), Some(PhpValue::Int(999)));
}

#[test]
fn test_list_iteration_cpu_cache_locality() {
    clear_runtime_closure_macros();

    // Verify sequential foreach execution over contiguous memory
    let source = r#"
        $numbers = Arr::range(1, 100);
        $sum = 0;
        $count = 0;
        foreach ($numbers as $num) {
            $sum += $num;
            $count++;
        }

        $indexedSum = 0;
        foreach ($numbers as $idx => $val) {
            $indexedSum += $idx;
        }

        return [$sum, $count, $indexedSum];
    "#;

    let res = run_source(source).expect("Sequential iteration should succeed");
    // sum of 1..100 = 5050, count = 100, sum of 0..99 = 4950
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(5050),
            PhpValue::Int(100),
            PhpValue::Int(4950),
        ])
    );
}

#[test]
fn test_list_fluent_higher_order_methods() {
    clear_runtime_closure_macros();

    let source = r#"
        // 1. Chained map -> filter -> slice
        $pipeline = Arr::list([1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
            ->filter(fn($n) => $n % 2 == 0)
            ->map(fn($n) => $n * 10)
            ->slice(1, 3); // takes [40, 60, 80]

        // 2. Reject -> reduce
        $reduced = Arr::list([1, 2, 3, 4, 5, 6])
            ->reject(fn($n) => $n % 2 == 0) // [1, 3, 5]
            ->reduce(fn($acc, $n) => $acc + $n, 0); // 9

        // 3. Find, first, and last
        $found = Arr::list([10, 25, 40, 55])->find(fn($n) => $n > 30); // 40
        $firstItem = Arr::list(['first', 'middle', 'last'])->first();
        $lastItem = Arr::list(['first', 'middle', 'last'])->last();

        // 4. Take and Skip
        $taken = Arr::list([10, 20, 30, 40, 50])->take(3); // [10, 20, 30]
        $skipped = Arr::list([10, 20, 30, 40, 50])->skip(2); // [30, 40, 50]

        // 5. Unique, Reverse, Sort
        $uniques = Arr::list([1, 2, 2, 3, 1, 4, 3])->unique(); // [1, 2, 3, 4]
        $reversed = Arr::list([1, 2, 3])->reverse(); // [3, 2, 1]
        $sorted = Arr::list([5, 1, 4, 2, 3])->sort(); // [1, 2, 3, 4, 5]

        return [
            $pipeline,
            $reduced,
            $found,
            $firstItem,
            $lastItem,
            $taken,
            $skipped,
            $uniques,
            $reversed,
            $sorted,
        ];
    "#;

    let res = run_source(source).expect("Collection pipeline should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::list(vec![PhpValue::Int(40), PhpValue::Int(60), PhpValue::Int(80)]),
            PhpValue::Int(9),
            PhpValue::Int(40),
            PhpValue::string("first"),
            PhpValue::string("last"),
            PhpValue::list(vec![PhpValue::Int(10), PhpValue::Int(20), PhpValue::Int(30)]),
            PhpValue::list(vec![PhpValue::Int(30), PhpValue::Int(40), PhpValue::Int(50)]),
            PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3), PhpValue::Int(4)]),
            PhpValue::list(vec![PhpValue::Int(3), PhpValue::Int(2), PhpValue::Int(1)]),
            PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3), PhpValue::Int(4), PhpValue::Int(5)]),
        ])
    );
}

#[test]
fn test_list_math_search_and_chunk_helpers() {
    clear_runtime_closure_macros();

    let source = r#"
        $nums = Arr::list([5, 15, 25, 35, 45]);
        $sum = $nums->sum();
        $avg = $nums->avg();
        $min = $nums->min();
        $max = $nums->max();
        $count = $nums->count();
        $isEmpty = $nums->isEmpty();
        $isNotEmpty = $nums->isNotEmpty();
        $hasVal = $nums->contains(25);
        $missingVal = $nums->contains(999);

        // Chunks and Flatten
        $chunked = Arr::list([1, 2, 3, 4, 5, 6, 7])->chunk(3);
        $flattened = Arr::list([1, [2, [3, 4]], 5])->flatten();

        // Concat
        $merged = Arr::list([1, 2])->concat([3, 4], [5]);

        return [
            $sum,
            $avg,
            $min,
            $max,
            $count,
            $isEmpty,
            $isNotEmpty,
            $hasVal,
            $missingVal,
            $chunked,
            $flattened,
            $merged,
        ];
    "#;

    let res = run_source(source).expect("Math and chunk operations should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(125),
            PhpValue::Float(25.0),
            PhpValue::Int(5),
            PhpValue::Int(45),
            PhpValue::Int(5),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::list(vec![
                PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3)]),
                PhpValue::list(vec![PhpValue::Int(4), PhpValue::Int(5), PhpValue::Int(6)]),
                PhpValue::list(vec![PhpValue::Int(7)]),
            ]),
            PhpValue::list(vec![
                PhpValue::Int(1),
                PhpValue::Int(2),
                PhpValue::Int(3),
                PhpValue::Int(4),
                PhpValue::Int(5),
            ]),
            PhpValue::list(vec![
                PhpValue::Int(1),
                PhpValue::Int(2),
                PhpValue::Int(3),
                PhpValue::Int(4),
                PhpValue::Int(5),
            ]),
        ])
    );
}

#[test]
fn test_arr_static_factory_methods() {
    clear_runtime_closure_macros();

    let source = r#"
        // 1. Range
        $rangeAsc = Arr::range(1, 5);
        $rangeStep = Arr::range(10, 20, 3);
        $rangeDesc = Arr::range(5, 1, -1);

        // 2. Wrap & Repeat
        $wrappedScalar = Arr::wrap(42);
        $wrappedNull = Arr::wrap(null);
        $repeated = Arr::repeat('hello', 3);

        // 3. From
        $fromArray = Arr::from([100, 200, 300]);

        return [
            $rangeAsc,
            $rangeStep,
            $rangeDesc,
            $wrappedScalar,
            $wrappedNull,
            $repeated,
            $fromArray,
        ];
    "#;

    let res = run_source(source).expect("Static factory methods should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::list(vec![
                PhpValue::Int(1),
                PhpValue::Int(2),
                PhpValue::Int(3),
                PhpValue::Int(4),
                PhpValue::Int(5),
            ]),
            PhpValue::list(vec![
                PhpValue::Int(10),
                PhpValue::Int(13),
                PhpValue::Int(16),
                PhpValue::Int(19),
            ]),
            PhpValue::list(vec![
                PhpValue::Int(5),
                PhpValue::Int(4),
                PhpValue::Int(3),
                PhpValue::Int(2),
                PhpValue::Int(1),
            ]),
            PhpValue::list(vec![PhpValue::Int(42)]),
            PhpValue::list(vec![]),
            PhpValue::list(vec![
                PhpValue::string("hello"),
                PhpValue::string("hello"),
                PhpValue::string("hello"),
            ]),
            PhpValue::list(vec![
                PhpValue::Int(100),
                PhpValue::Int(200),
                PhpValue::Int(300),
            ]),
        ])
    );
}

#[test]
fn test_arr_macro_registration_and_list_execution() {
    clear_runtime_closure_macros();

    // The canonical architecture specification example:
    // Arr::macro('toCsv', function ($delimiter = ',') { return implode($delimiter, $this->all()); });
    // $csv = Arr::list([1, 2, 3])->toCsv();
    let source = r#"
        Arr::macro('toCsv', function ($delimiter = ',') {
            return implode($delimiter, $this->all());
        });

        $csvDefault = Arr::list([1, 2, 3])->toCsv();
        $csvPipe = Arr::list(['red', 'green', 'blue'])->toCsv('|');

        Arr::macro('sumDoubled', function () {
            return $this->map(fn($x) => $x * 2)->sum();
        });

        $doubledSum = Arr::list([10, 20, 30])->sumDoubled();

        return Arr::list([$csvDefault, $csvPipe, $doubledSum]);
    "#;

    let res = run_source(source).expect("Macro registration and execution should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::string("1,2,3"),
            PhpValue::string("red|green|blue"),
            PhpValue::Int(120),
        ])
    );
}

#[test]
fn test_list_sequential_index_integrity_and_string_key_rejection() {
    clear_runtime_closure_macros();

    // 1. Valid sequential index reads, updates, and appends
    let source_valid = r#"
        $list = Arr::list([10, 20, 30]);
        $val0 = $list[0];
        $val2 = $list[2];
        $list[1] = 999;
        $list[] = 40;
        return [$val0, $val2, $list];
    "#;

    let res = run_source(source_valid).expect("Valid indexing should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(10),
            PhpValue::Int(30),
            PhpValue::list(vec![
                PhpValue::Int(10),
                PhpValue::Int(999),
                PhpValue::Int(30),
                PhpValue::Int(40),
            ]),
        ])
    );

    // 2. Setting a non-numeric string key on Arr::list is rejected
    // (Arr::list is strictly sequential, preserving contiguous memory layout)
    let source_invalid_key = r#"
        $list = Arr::list([1, 2, 3]);
        $list['non_numeric_key'] = 42;
    "#;

    let err = run_source(source_invalid_key).expect_err("Non-numeric key on Arr::list must fail");
    assert!(
        err.to_string().contains("Undefined index or key 'non_numeric_key'"),
        "Unexpected error: {:?}",
        err
    );
}

#[test]
fn test_list_type_boundary_contracts() {
    clear_runtime_closure_macros();

    // 1. Function boundary contract with `List` parameter and return type
    let source = r#"
        function doubleElements(List $items): List {
            return $items->map(fn($x) => $x * 2);
        }

        $input = Arr::list([5, 10, 15]);
        $output = doubleElements($input);
        return $output;
    "#;

    let res = run_source(source).expect("Boundary contract with List should pass");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(10),
            PhpValue::Int(20),
            PhpValue::Int(30),
        ])
    );

    // 2. Static type boundary contract rejection at compile-time
    let source_compile_invalid = r#"
        function requireList(List $items): int {
            return $items->count();
        }

        requireList('not a list');
    "#;

    let err_compile = run_source(source_compile_invalid).expect_err("Invalid type must fail static type checking");
    assert!(
        err_compile.to_string().contains("expected 'List'"),
        "Expected type mismatch on 'List', got: {:?}",
        err_compile
    );

    // 3. Dynamic runtime boundary contract rejection with AssertType
    let source_runtime_invalid = r#"
        function requireListDynamic(List $items): int {
            return $items->count();
        }

        function testBridge(mixed $x): int {
            return requireListDynamic($x);
        }

        testBridge('not a list');
    "#;

    let err_runtime = run_source(source_runtime_invalid).expect_err("Dynamic invalid type must violate runtime boundary contract");
    assert!(
        err_runtime.to_string().contains("Boundary contract violation"),
        "Expected Boundary contract violation, got: {:?}",
        err_runtime
    );
}

#[test]
fn test_user_requested_lowercase_list_and_dict_syntax() {
    clear_runtime_closure_macros();

    // 1. The user's exact specification syntax:
    let source = r#"
        function process_items(list $my_arr, dict $my_dict) {
            list $my_new_scoped_arr = [];
            dict $my_new_scoped_dict = [];
        }

        $input_arr = Arr::list([1, 2, 3]);
        $input_dict = Arr::dict(['status' => 'active']);
        process_items($input_arr, $input_dict);
    "#;

    let res = run_source(source).expect("process_items with lowercase list and dict must execute");
    assert_eq!(res, PhpValue::Null);

    // 2. Verifying mutations and operations on locally scoped list and dict:
    let source_returns = r#"
        function process_items_and_return(list $my_arr, dict $my_dict): list {
            list $my_new_scoped_arr = [];
            dict $my_new_scoped_dict = [];

            $my_new_scoped_arr[] = 100;
            $my_new_scoped_arr[] = 200;

            $my_new_scoped_dict['total'] = 300;

            return $my_new_scoped_arr;
        }

        $arr = Arr::list([1, 2]);
        $dict = Arr::dict(['k' => 'v']);
        return process_items_and_return($arr, $dict);
    "#;

    let res_returns = run_source(source_returns).expect("process_items_and_return must execute");
    assert_eq!(
        res_returns,
        PhpValue::list(vec![PhpValue::Int(100), PhpValue::Int(200)])
    );

    // 3. Parameter type safety rejects non-collection arguments:
    let source_invalid_list = r#"
        function process_items(list $my_arr, dict $my_dict) {
            list $my_new_scoped_arr = [];
            dict $my_new_scoped_dict = [];
        }

        process_items('not a list', Arr::dict([]));
    "#;
    let err = run_source(source_invalid_list).expect_err("Non-list argument must fail type check");
    assert!(
        err.to_string().contains("expected 'list'")
            || err.to_string().contains("expected 'List'")
            || format!("{:?}", err).contains("ArgumentTypeMismatch"),
        "Unexpected error: {:?}",
        err
    );
}

#[test]
fn test_array_immutable_helper_methods_and_explicit_reassignment() {
    // 1. Calling array helper methods on standard array never mutates original array in place
    let source_direct = r#"
        $my_array = [3, 1, 2];
        $sorted = $my_array->sort();
        $original_preserved = $my_array;
        $my_array = $my_array->sort();
        return [$sorted, $original_preserved, $my_array];
    "#;

    let res_direct = run_source(source_direct).expect("Direct array sort must succeed");
    assert_eq!(
        res_direct,
        PhpValue::list(vec![
            PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3)]),
            PhpValue::list(vec![PhpValue::Int(3), PhpValue::Int(1), PhpValue::Int(2)]),
            PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3)]),
        ])
    );

    // 2. Calling Arr::list($my_array)->sort() returns a brand-new instance and preserves $my_array
    let source_facade = r#"
        $my_array = [50, 10, 40, 20];
        $sorted = Arr::list($my_array)->sort();
        $original_preserved = $my_array;
        $my_array = Arr::list($my_array)->sort();
        return [$sorted, $original_preserved, $my_array];
    "#;

    let res_facade = run_source(source_facade).expect("Arr::list sort must succeed");
    assert_eq!(
        res_facade,
        PhpValue::list(vec![
            PhpValue::list(vec![
                PhpValue::Int(10),
                PhpValue::Int(20),
                PhpValue::Int(40),
                PhpValue::Int(50)
            ]),
            PhpValue::list(vec![
                PhpValue::Int(50),
                PhpValue::Int(10),
                PhpValue::Int(40),
                PhpValue::Int(20)
            ]),
            PhpValue::list(vec![
                PhpValue::Int(10),
                PhpValue::Int(20),
                PhpValue::Int(40),
                PhpValue::Int(50)
            ]),
        ])
    );
}

