use rhp::run_source;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;
use std::collections::HashSet;

#[test]
fn test_set_direct_hashset_memory_layout_and_cow_detachment() {
    clear_runtime_closure_macros();

    // 1. Direct Rust HashSet<PhpValue> inspection
    let mut initial_set = HashSet::new();
    initial_set.insert(PhpValue::Int(10));
    initial_set.insert(PhpValue::Int(20));
    initial_set.insert(PhpValue::Int(30));
    let set = PhpValue::set_from_hash_set(initial_set);

    assert!(set.is_set());
    assert_eq!(set.set_len(), Some(3));
    assert!(set.set_contains(&PhpValue::Int(10)));
    assert!(set.set_contains(&PhpValue::Int(20)));
    assert!(set.set_contains(&PhpValue::Int(30)));
    assert!(!set.set_contains(&PhpValue::Int(40)));

    // Initial reference count should be 1
    assert_eq!(set.ref_count(), 1);
    assert!(!set.is_shared());

    // Cloning PhpValue::Set creates an O(1) shared handle
    let mut set_alias = set.clone();
    assert_eq!(set.ref_count(), 2);
    assert_eq!(set_alias.ref_count(), 2);
    assert!(set.is_shared());
    assert!(set_alias.is_shared());

    // Copy-On-Write (COW): ensure_set_mut detaches the underlying HashSet<PhpValue>
    assert!(set_alias.ensure_set_mut());
    assert_eq!(set.ref_count(), 1);
    assert_eq!(set_alias.ref_count(), 1);
    assert!(!set.is_shared());
    assert!(!set_alias.is_shared());

    // Mutate detached set
    if let PhpValue::Set(ref rc) = set_alias {
        rc.borrow_mut().insert(PhpValue::Int(999));
    }
    assert_eq!(set.set_len(), Some(3));
    assert_eq!(set_alias.set_len(), Some(4));
    assert!(set_alias.set_contains(&PhpValue::Int(999)));
    assert!(!set.set_contains(&PhpValue::Int(999)));
}

#[test]
fn test_set_uniqueness_and_duplicate_elimination() {
    clear_runtime_closure_macros();

    let source = r#"
        // 1. Duplicate elimination on initial construction
        $set = Arr::set([10, 20, 20, 30, 10, 'apple', 'apple', 'banana']);
        $initialCount = $set->count();

        // 2. Append duplicate via bracket push syntax
        $set[] = 20;
        $countAfterPush = $set->count();

        // 3. Append duplicate via ->add() method
        $set->add('apple');
        $countAfterAddDuplicate = $set->count();

        // 4. Add unique new elements
        $set[] = 40;
        $set->add('cherry');
        $finalCount = $set->count();

        return [$initialCount, $countAfterPush, $countAfterAddDuplicate, $finalCount];
    "#;

    let res = run_source(source).expect("Set duplicate elimination execution should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(5), // 10, 20, 30, 'apple', 'banana'
            PhpValue::Int(5), // 20 was a duplicate
            PhpValue::Int(5), // 'apple' was a duplicate
            PhpValue::Int(7), // added 40 and 'cherry'
        ])
    );
}

#[test]
fn test_set_o1_membership_and_bracket_index() {
    clear_runtime_closure_macros();

    let source = r#"
        $set = Arr::set(['alpha', 'beta', 'gamma']);

        // 1. Fluent membership checks via contains() and has()
        $hasAlpha = $set->contains('alpha');
        $hasBeta = $set->has('beta');
        $hasDelta = $set->contains('delta');

        // 2. Bracket indexing membership check
        $idxAlpha = $set['alpha'];
        $idxDelta = $set['delta'];

        // 3. Bracket assignment inserts into the set
        $set['delta'] = true;
        $hasDeltaAfter = $set->contains('delta');
        $idxDeltaAfter = $set['delta'];

        return [
            $hasAlpha,
            $hasBeta,
            $hasDelta,
            $idxAlpha,
            $idxDelta,
            $hasDeltaAfter,
            $idxDeltaAfter,
        ];
    "#;

    let res = run_source(source).expect("Set membership checks should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_set_element_removal() {
    clear_runtime_closure_macros();

    let source = r#"
        $set = Arr::set([1, 2, 3, 4, 5]);

        // remove() returns bool indicating whether the element was present
        $removed3 = $set->remove(3);
        $removed99 = $set->remove(99);
        $count = $set->count();
        $has3 = $set->contains(3);

        // delete() alias
        $deleted4 = $set->delete(4);
        $countAfterDelete = $set->count();
        $has4 = $set->contains(4);

        return [$removed3, $removed99, $count, $has3, $deleted4, $countAfterDelete, $has4];
    "#;

    let res = run_source(source).expect("Set element removal should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Int(4),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Int(3),
            PhpValue::Bool(false),
        ])
    );
}

#[test]
fn test_set_algebra_diff_intersect_union() {
    clear_runtime_closure_macros();

    let source = r#"
        $setA = Arr::set([1, 2, 3, 4]);
        $setB = Arr::set([3, 4, 5, 6]);

        // 1. Difference: in A but not in B -> {1, 2}
        $diff = $setA->diff($setB);
        $diffCount = $diff->count();
        $diffHas1 = $diff->contains(1);
        $diffHas2 = $diff->contains(2);
        $diffHas3 = $diff->contains(3);

        // 2. Intersection: in both A and B -> {3, 4}
        $intersect = $setA->intersect($setB);
        $intersectCount = $intersect->count();
        $intersectHas3 = $intersect->contains(3);
        $intersectHas4 = $intersect->contains(4);
        $intersectHas1 = $intersect->contains(1);

        // 3. Union: in A or B -> {1, 2, 3, 4, 5, 6}
        $union = $setA->union($setB);
        $unionCount = $union->count();
        $unionHas1 = $union->contains(1);
        $unionHas6 = $union->contains(6);

        // 4. Cross-collection algebra with plain array
        $diffWithArr = $setA->diff([2, 4]);
        $diffWithArrCount = $diffWithArr->count();
        $diffWithArrHas1 = $diffWithArr->contains(1);
        $diffWithArrHas3 = $diffWithArr->contains(3);

        $unionWithArr = $setA->union([7, 8]);
        $unionWithArrCount = $unionWithArr->count();
        $unionWithArrHas7 = $unionWithArr->contains(7);

        return [
            $diffCount, $diffHas1, $diffHas2, $diffHas3,
            $intersectCount, $intersectHas3, $intersectHas4, $intersectHas1,
            $unionCount, $unionHas1, $unionHas6,
            $diffWithArrCount, $diffWithArrHas1, $diffWithArrHas3,
            $unionWithArrCount, $unionWithArrHas7,
        ];
    "#;

    let res = run_source(source).expect("Set algebra operations should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            // Diff
            PhpValue::Int(2),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            // Intersect
            PhpValue::Int(2),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            // Union
            PhpValue::Int(6),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            // Diff with plain array
            PhpValue::Int(2),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            // Union with plain array
            PhpValue::Int(6),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_set_fluent_higher_order_methods() {
    clear_runtime_closure_macros();

    let source = r#"
        // 1. Map: transforms elements and enforces uniqueness in resulting set
        $mapped = Arr::set([1, 2, 3, 4])->map(fn($n) => $n % 2);
        $mappedCount = $mapped->count(); // only 0 and 1
        $mappedHas0 = $mapped->contains(0);
        $mappedHas1 = $mapped->contains(1);

        // 2. Filter: keeps elements matching predicate
        $filtered = Arr::set([10, 15, 20, 25, 30])->filter(fn($n) => $n >= 20);
        $filteredCount = $filtered->count();
        $filteredHas10 = $filtered->contains(10);
        $filteredHas20 = $filtered->contains(20);

        // 3. Reject: drops elements matching predicate
        $rejected = Arr::set([10, 15, 20, 25, 30])->reject(fn($n) => $n >= 20);
        $rejectedCount = $rejected->count();
        $rejectedHas10 = $rejected->contains(10);
        $rejectedHas15 = $rejected->contains(15);
        $rejectedHas20 = $rejected->contains(20);

        // 4. Reduce: accumulates elements
        $sum = Arr::set([1, 2, 3, 4])->reduce(fn($acc, $n) => $acc + $n, 0);

        // 5. first() and isEmpty()
        $first = Arr::set([42])->first();
        $empty = Arr::set([])->isEmpty();
        $notEmpty = Arr::set([1])->isNotEmpty();

        return [
            $mappedCount, $mappedHas0, $mappedHas1,
            $filteredCount, $filteredHas10, $filteredHas20,
            $rejectedCount, $rejectedHas10, $rejectedHas15, $rejectedHas20,
            $sum,
            $first, $empty, $notEmpty,
        ];
    "#;

    let res = run_source(source).expect("Set higher-order methods should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(2),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Int(3),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Int(2),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Int(10),
            PhpValue::Int(42),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_set_foreach_iteration() {
    clear_runtime_closure_macros();

    let source = r#"
        $set = Arr::set([10, 20, 30, 40]);
        $sum = 0;
        $count = 0;
        foreach ($set as $item) {
            $sum += $item;
            $count++;
        }

        $indexedCount = 0;
        foreach ($set as $idx => $item) {
            $indexedCount++;
        }

        return [$sum, $count, $indexedCount];
    "#;

    let res = run_source(source).expect("Set foreach iteration should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(100),
            PhpValue::Int(4),
            PhpValue::Int(4),
        ])
    );
}

#[test]
fn test_set_type_boundary_contracts() {
    clear_runtime_closure_macros();

    // 1. Strict `set $s` declaration and return type hint
    let valid_source = r#"
        function processSet(set $items): int {
            return $items->count();
        }

        function buildUniqueSet(): set {
            return Arr::set(['x', 'y', 'z', 'x']);
        }

        set $s = buildUniqueSet();
        return processSet($s);
    "#;

    let res = run_source(valid_source).expect("Valid set type contracts must succeed");
    assert_eq!(res, PhpValue::Int(3));

    // 2. Passing integer to function expecting `set` must fail static or runtime type checks
    let invalid_param_source = r#"
        function requireSet(set $items): int {
            return $items->count();
        }

        requireSet(12345);
    "#;
    let err = run_source(invalid_param_source).expect_err("Passing scalar to set parameter must fail");
    let err_str = format!("{:?}", err);
    assert!(
        err_str.contains("ArgumentTypeMismatch")
            || err_str.contains("TypeMismatch")
            || err_str.contains("Cannot assign"),
        "Unexpected error: {:?}",
        err
    );

    // 3. Passing Dict to function expecting `set` must fail
    let invalid_dict_param_source = r#"
        function requireSet(set $items): int {
            return $items->count();
        }

        requireSet(['a' => 1, 'b' => 2]);
    "#;
    let err2 = run_source(invalid_dict_param_source).expect_err("Passing Dict to set parameter must fail");
    let err2_str = format!("{:?}", err2);
    assert!(
        err2_str.contains("ArgumentTypeMismatch")
            || err2_str.contains("TypeMismatch")
            || err2_str.contains("Cannot assign"),
        "Unexpected error: {:?}",
        err2
    );
}

#[test]
fn test_set_permissive_parent_array_type_acceptance() {
    clear_runtime_closure_macros();

    let source = r#"
        function countAnyCollection(array $items): int {
            return $items->count();
        }

        function returnSetAsArray(): array {
            return Arr::set([10, 20, 30, 20]);
        }

        array $arr = Arr::set(['a', 'b', 'c']);
        $c1 = countAnyCollection($arr);
        $c2 = countAnyCollection(returnSetAsArray());

        return [$c1, $c2];
    "#;

    let res = run_source(source).expect("Permissive array type must accept Set collections");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(3),
            PhpValue::Int(3),
        ])
    );
}

#[test]
fn test_set_laravel_arr_facade_static_methods() {
    clear_runtime_closure_macros();

    let source = r#"
        $s = Arr::set(['red', 'green', 'blue']);

        $c = Arr::count($s);
        $hasGreen = Arr::contains($s, 'green');
        $hasYellow = Arr::contains($s, 'yellow');

        return [$c, $hasGreen, $hasYellow];
    "#;

    let res = run_source(source).expect("Arr facade methods on Set should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(3),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
        ])
    );
}

#[test]
fn test_arr_macro_registration_and_set_execution() {
    clear_runtime_closure_macros();

    let source = r#"
        Arr::macro('countPlusTen', function() {
            return $this->count() + 10;
        });

        $s = Arr::set([1, 2, 3]);
        return $s->countPlusTen();
    "#;

    let res = run_source(source).expect("Macro execution on Set should succeed");
    assert_eq!(res, PhpValue::Int(13));
}

#[test]
fn test_set_json_serialization() {
    clear_runtime_closure_macros();

    let source = r#"
        $s = Arr::set([42]);
        return $s->toJson();
    "#;

    let res = run_source(source).expect("JSON serialization of Set should succeed");
    assert_eq!(res, PhpValue::string("[42]"));
}
