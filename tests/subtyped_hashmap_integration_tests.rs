use rhp::run_source;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;
use std::collections::HashMap;

#[test]
fn test_hashmap_direct_hashmap_memory_layout_and_cow_detachment() {
    clear_runtime_closure_macros();

    // 1. Direct Rust HashMap<PhpValue, PhpValue> inspection
    let mut initial_map = HashMap::new();
    initial_map.insert(PhpValue::Int(1), PhpValue::string("one"));
    initial_map.insert(PhpValue::Int(2), PhpValue::string("two"));
    initial_map.insert(
        PhpValue::list(vec![PhpValue::Int(10), PhpValue::Int(20)]),
        PhpValue::string("coordinates"),
    );

    let map = PhpValue::hash_map_from_map(initial_map);

    assert!(map.is_hash_map());
    assert_eq!(map.hash_map_len(), Some(3));
    assert_eq!(
        map.hash_map_get(&PhpValue::Int(1)),
        Some(PhpValue::string("one"))
    );
    assert_eq!(
        map.hash_map_get(&PhpValue::list(vec![PhpValue::Int(10), PhpValue::Int(20)])),
        Some(PhpValue::string("coordinates"))
    );
    assert!(map.hash_map_contains_key(&PhpValue::Int(2)));
    assert!(!map.hash_map_contains_key(&PhpValue::Int(999)));

    // Initial reference count should be 1
    assert_eq!(map.ref_count(), 1);
    assert!(!map.is_shared());

    // Cloning PhpValue::HashMap creates an O(1) shared handle
    let mut map_alias = map.clone();
    assert_eq!(map.ref_count(), 2);
    assert_eq!(map_alias.ref_count(), 2);
    assert!(map.is_shared());
    assert!(map_alias.is_shared());

    // Copy-On-Write (COW): ensure_hash_map_mut detaches the underlying HashMap
    assert!(map_alias.ensure_hash_map_mut());
    assert_eq!(map.ref_count(), 1);
    assert_eq!(map_alias.ref_count(), 1);
    assert!(!map.is_shared());
    assert!(!map_alias.is_shared());

    // Mutate detached map
    if let PhpValue::HashMap(ref rc) = map_alias {
        rc.borrow_mut()
            .insert(PhpValue::Int(999), PhpValue::string("detached_val"));
    }
    assert_eq!(map.hash_map_len(), Some(3));
    assert_eq!(map_alias.hash_map_len(), Some(4));
    assert_eq!(
        map_alias.hash_map_get(&PhpValue::Int(999)),
        Some(PhpValue::string("detached_val"))
    );
    assert_eq!(map.hash_map_get(&PhpValue::Int(999)), None);
}

#[test]
fn test_hashmap_complex_keys_support() {
    clear_runtime_closure_macros();

    let source = r#"
        class Entity {
            public int $id;
            public function __construct(int $id) {
                $this->id = $id;
            }
        }

        // Initialize empty HashMap
        $map = Arr::hashMap();

        // 1. Array / List as complex key
        $keyArr1 = [10, 20];
        $keyArr2 = [30, 40];
        $map[$keyArr1] = 'point A';
        $map[$keyArr2] = 'point B';

        // 2. Object / Class instance as complex key
        $e1 = new Entity(101);
        $e2 = new Entity(102);
        $map[$e1] = 'entity 101';
        $map[$e2] = 'entity 102';

        // 3. Various scalar keys (Int, Float, String, Bool)
        $map[100] = 'int key';
        $map[3.14] = 'float key';
        $map['normal_string'] = 'string key';
        $map[true] = 'bool key';

        // Lookups using identical structural keys
        $getPointA = $map[[10, 20]];
        $getPointB = $map[[30, 40]];
        $getEntity1 = $map[$e1];
        $getEntity2 = $map[$e2];
        $getInt = $map[100];
        $getFloat = $map[3.14];
        $getStr = $map['normal_string'];
        $getBool = $map[true];
        $count = $map->count();

        return [
            $getPointA,
            $getPointB,
            $getEntity1,
            $getEntity2,
            $getInt,
            $getFloat,
            $getStr,
            $getBool,
            $count,
        ];
    "#;

    let res = run_source(source).expect("Complex keys hash map execution should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::string("point A"),
            PhpValue::string("point B"),
            PhpValue::string("entity 101"),
            PhpValue::string("entity 102"),
            PhpValue::string("int key"),
            PhpValue::string("float key"),
            PhpValue::string("string key"),
            PhpValue::string("bool key"),
            PhpValue::Int(8),
        ])
    );
}

#[test]
fn test_hashmap_crud_and_fluent_methods() {
    clear_runtime_closure_macros();

    let source = r#"
        $map = Arr::hashMap();

        // 1. put() and get()
        $map->put('name', 'Antigravity');
        $map->put('version', 2);
        $name = $map->get('name');
        $version = $map->get('version');
        $fallback = $map->get('missing', 'default_val');

        // 2. has() / contains() / hasKey()
        $hasName = $map->has('name');
        $containsVer = $map->contains('version');
        $hasMissing = $map->hasKey('missing');

        // 3. pull() removes and returns value
        $pulledVer = $map->pull('version', 0);
        $hasVerAfterPull = $map->has('version');

        // 4. remove() / forget()
        $map->put('temp', 'to_delete');
        $map->remove('temp');
        $hasTemp = $map->has('temp');

        // 5. count(), isEmpty(), isNotEmpty()
        $count = $map->count();
        $empty = $map->isEmpty();
        $notEmpty = $map->isNotEmpty();

        return [
            $name, $version, $fallback,
            $hasName, $containsVer, $hasMissing,
            $pulledVer, $hasVerAfterPull,
            $hasTemp,
            $count, $empty, $notEmpty,
        ];
    "#;

    let res = run_source(source).expect("HashMap CRUD operations should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::string("Antigravity"),
            PhpValue::Int(2),
            PhpValue::string("default_val"),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Int(2),
            PhpValue::Bool(false),
            PhpValue::Bool(false),
            PhpValue::Int(1),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_hashmap_keys_values_entries() {
    clear_runtime_closure_macros();

    let source = r#"
        $map = Arr::hashMap([
            'x' => 10,
            'y' => 20,
        ]);

        $keys = $map->keys();
        $values = $map->values();
        $count = $map->count();

        return [$count, $keys->count(), $values->count()];
    "#;

    let res = run_source(source).expect("Keys and values extraction should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(2),
            PhpValue::Int(2),
            PhpValue::Int(2),
        ])
    );
}

#[test]
fn test_hashmap_fluent_higher_order_methods() {
    clear_runtime_closure_macros();

    let source = r#"
        $map = Arr::hashMap([
            'a' => 10,
            'b' => 25,
            'c' => 30,
            'd' => 45,
        ]);

        // 1. map: transforms values
        $doubled = $map->map(fn($v, $k) => $v * 2);
        $doubledA = $doubled->get('a');
        $doubledB = $doubled->get('b');

        // 2. filter: keeps entries matching predicate
        $filtered = $map->filter(fn($v, $k) => $v >= 30);
        $filteredCount = $filtered->count();
        $filteredHasA = $filtered->has('a');
        $filteredHasC = $filtered->has('c');

        // 3. reject: drops entries matching predicate
        $rejected = $map->reject(fn($v, $k) => $v >= 30);
        $rejectedCount = $rejected->count();
        $rejectedHasA = $rejected->has('a');
        $rejectedHasC = $rejected->has('c');

        // 4. reduce: folds over entries
        $sum = $map->reduce(fn($acc, $v, $k) => $acc + $v, 0);

        // 5. diff & intersect against another map
        $other = Arr::hashMap([
            'b' => 999,
            'c' => 888,
            'z' => 777,
        ]);
        $diff = $map->diff($other); // keys in $map but not in $other: 'a', 'd'
        $diffCount = $diff->count();
        $diffHasA = $diff->has('a');
        $diffHasB = $diff->has('b');

        $intersect = $map->intersect($other); // keys in both: 'b', 'c'
        $intersectCount = $intersect->count();
        $intersectHasB = $intersect->has('b');
        $intersectHasA = $intersect->has('a');

        // 6. merge
        $merged = $map->merge($other);
        $mergedCount = $merged->count(); // 'a', 'b', 'c', 'd', 'z' = 5
        $mergedValB = $merged->get('b'); // updated from $other

        return [
            $doubledA, $doubledB,
            $filteredCount, $filteredHasA, $filteredHasC,
            $rejectedCount, $rejectedHasA, $rejectedHasC,
            $sum,
            $diffCount, $diffHasA, $diffHasB,
            $intersectCount, $intersectHasB, $intersectHasA,
            $mergedCount, $mergedValB,
        ];
    "#;

    let res = run_source(source).expect("Higher order methods on HashMap should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(20),
            PhpValue::Int(50),
            PhpValue::Int(2),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::Int(2),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Int(110),
            PhpValue::Int(2),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Int(2),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Int(5),
            PhpValue::Int(999),
        ])
    );
}

#[test]
fn test_hashmap_foreach_iteration_with_complex_keys() {
    clear_runtime_closure_macros();

    let source = r#"
        $map = Arr::hashMap();
        $map[[1, 2]] = 10;
        $map[[3, 4]] = 20;
        $map[[5, 6]] = 30;

        $totalVal = 0;
        $keyCount = 0;
        $keysSum = 0;

        foreach ($map as $k => $v) {
            $totalVal += $v;
            $keyCount++;
            $keysSum += $k[0] + $k[1];
        }

        $valOnlyTotal = 0;
        foreach ($map as $v) {
            $valOnlyTotal += $v;
        }

        return [$totalVal, $keyCount, $keysSum, $valOnlyTotal];
    "#;

    let res = run_source(source).expect("Foreach iteration with complex keys should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(60),
            PhpValue::Int(3),
            PhpValue::Int(21), // (1+2) + (3+4) + (5+6) = 21
            PhpValue::Int(60),
        ])
    );
}

#[test]
fn test_hashmap_type_boundary_contracts() {
    clear_runtime_closure_macros();

    // 1. Strict `hashmap $m` declaration, parameter, and return type contracts
    let valid_source = r#"
        function processMap(hashmap $m): int {
            return $m->count();
        }

        function createMap(): hashmap {
            return Arr::hashMap([
                'x' => 1,
                'y' => 2,
            ]);
        }

        hashmap $myMap = createMap();
        return processMap($myMap);
    "#;

    let res = run_source(valid_source).expect("Valid hashmap type contracts must succeed");
    assert_eq!(res, PhpValue::Int(2));

    // 2. Strict `map $m` alias
    let valid_alias_source = r#"
        function processMapAlias(map $m): int {
            return $m->count();
        }

        map $myMap = Arr::map(['a' => 10, 'b' => 20, 'c' => 30]);
        return processMapAlias($myMap);
    "#;

    let res2 = run_source(valid_alias_source).expect("Valid map alias contract must succeed");
    assert_eq!(res2, PhpValue::Int(3));

    // 3. Passing integer to function expecting `hashmap` must fail
    let invalid_scalar_source = r#"
        function requireHashMap(hashmap $m): int {
            return $m->count();
        }

        requireHashMap(42);
    "#;
    let err1 = run_source(invalid_scalar_source).expect_err("Passing scalar to hashmap parameter must fail");
    let err1_str = format!("{:?}", err1);
    assert!(
        err1_str.contains("ArgumentTypeMismatch")
            || err1_str.contains("TypeMismatch")
            || err1_str.contains("Cannot assign"),
        "Unexpected error: {:?}",
        err1
    );

    // 4. Passing sequential List to function expecting `hashmap` must fail
    let invalid_list_source = r#"
        function requireHashMap(hashmap $m): int {
            return $m->count();
        }

        requireHashMap(Arr::list([1, 2, 3]));
    "#;
    let err2 = run_source(invalid_list_source).expect_err("Passing List to hashmap parameter must fail");
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
fn test_hashmap_permissive_parent_array_type_acceptance() {
    clear_runtime_closure_macros();

    let source = r#"
        function countAnyCollection(array $items): int {
            return $items->count();
        }

        function returnHashMapAsArray(): array {
            return Arr::hashMap([
                'host' => '127.0.0.1',
                'port' => 8080,
            ]);
        }

        array $arr = Arr::hashMap(['k' => 'v']);
        $c1 = countAnyCollection($arr);
        $c2 = countAnyCollection(returnHashMapAsArray());

        return [$c1, $c2];
    "#;

    let res = run_source(source).expect("Permissive array type must accept HashMap collections");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(1),
            PhpValue::Int(2),
        ])
    );
}

#[test]
fn test_arr_macro_registration_and_hashmap_execution() {
    clear_runtime_closure_macros();

    let source = r#"
        Arr::macro('countPlusFifty', function() {
            return $this->count() + 50;
        });

        $map = Arr::hashMap([
            'a' => 1,
            'b' => 2,
        ]);

        return $map->countPlusFifty();
    "#;

    let res = run_source(source).expect("Macro execution on HashMap should succeed");
    assert_eq!(res, PhpValue::Int(52));
}

#[test]
fn test_hashmap_laravel_facade_and_json() {
    clear_runtime_closure_macros();

    let source = r#"
        $map = Arr::hashMap(['name' => 'RHP']);

        $c = Arr::count($map);
        $hasKey = Arr::has($map, 'name');
        $hasMissing = Arr::has($map, 'missing');
        $json = $map->toJson();

        return [$c, $hasKey, $hasMissing, $json];
    "#;

    let res = run_source(source).expect("Facade methods and JSON on HashMap should succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(1),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::string("{\"name\":\"RHP\"}"),
        ])
    );
}
