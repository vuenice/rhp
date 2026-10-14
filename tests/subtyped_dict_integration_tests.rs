use rhp::run_source;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;

#[test]
fn test_dict_indexmap_memory_layout_and_cow_detachment() {
    clear_runtime_closure_macros();

    // 1. Direct Rust IndexMap<String, PhpValue> inspection
    let dict = PhpValue::dict(vec![
        ("host".to_string(), PhpValue::string("127.0.0.1")),
        ("port".to_string(), PhpValue::Int(8080)),
        ("active".to_string(), PhpValue::Bool(true)),
    ]);

    assert!(dict.is_dict());
    assert_eq!(dict.dict_len(), Some(3));
    assert_eq!(dict.dict_get("host"), Some(PhpValue::string("127.0.0.1")));
    assert_eq!(dict.dict_get("port"), Some(PhpValue::Int(8080)));
    assert_eq!(dict.dict_get("active"), Some(PhpValue::Bool(true)));
    assert_eq!(dict.dict_get("missing"), None);

    assert_eq!(
        dict.dict_get_index(0),
        Some(("host".to_string(), PhpValue::string("127.0.0.1")))
    );
    assert_eq!(
        dict.dict_get_index(1),
        Some(("port".to_string(), PhpValue::Int(8080)))
    );
    assert_eq!(
        dict.dict_get_index(2),
        Some(("active".to_string(), PhpValue::Bool(true)))
    );
    assert_eq!(dict.dict_get_index(3), None);

    assert!(dict.dict_contains_key("host"));
    assert!(dict.dict_contains_key("port"));
    assert!(!dict.dict_contains_key("nonexistent"));

    // Initial reference count should be 1
    assert_eq!(dict.ref_count(), 1);
    assert!(!dict.is_shared());

    // Cloning PhpValue::Dict creates an O(1) shared handle
    let mut dict_alias = dict.clone();
    assert_eq!(dict.ref_count(), 2);
    assert_eq!(dict_alias.ref_count(), 2);
    assert!(dict.is_shared());
    assert!(dict_alias.is_shared());

    // Copy-On-Write (COW): ensure_dict_mut detaches the underlying IndexMap
    assert!(dict_alias.ensure_dict_mut());
    assert_eq!(dict.ref_count(), 1);
    assert_eq!(dict_alias.ref_count(), 1);
    assert!(!dict.is_shared());
    assert!(!dict_alias.is_shared());

    // Mutate detached dict
    if let PhpValue::Dict(ref rc) = dict_alias {
        rc.borrow_mut()
            .insert("retries".to_string(), PhpValue::Int(3));
    }
    assert_eq!(dict.dict_len(), Some(3));
    assert_eq!(dict_alias.dict_len(), Some(4));
    assert_eq!(dict_alias.dict_get("retries"), Some(PhpValue::Int(3)));
    assert_eq!(dict.dict_get("retries"), None);
}

#[test]
fn test_dict_strict_insertion_order_and_streaming_iteration() {
    clear_runtime_closure_macros();

    // Verify sequential foreach execution over IndexMap with strict insertion order
    let source = r#"
        $dict = Arr::dict([
            'c' => 300,
            'a' => 100,
            'b' => 200,
        ]);

        // 1. Iteration order matches initial insertion order (c, a, b)
        $keys = [];
        $vals = [];
        foreach ($dict as $k => $v) {
            $keys[] = $k;
            $vals[] = $v;
        }

        // 2. Updating an existing key ('a') preserves its original position
        $dict['a'] = 999;
        // Adding a new key ('d') appends to the end
        $dict['d'] = 400;

        $keysAfter = [];
        $valsAfter = [];
        foreach ($dict as $k => $v) {
            $keysAfter[] = $k;
            $valsAfter[] = $v;
        }

        return [
            $keys,
            $vals,
            $keysAfter,
            $valsAfter,
        ];
    "#;

    let res = run_source(source).expect("Strict insertion ordering iteration must succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::list(vec![
                PhpValue::string("c"),
                PhpValue::string("a"),
                PhpValue::string("b"),
            ]),
            PhpValue::list(vec![
                PhpValue::Int(300),
                PhpValue::Int(100),
                PhpValue::Int(200),
            ]),
            PhpValue::list(vec![
                PhpValue::string("c"),
                PhpValue::string("a"),
                PhpValue::string("b"),
                PhpValue::string("d"),
            ]),
            PhpValue::list(vec![
                PhpValue::Int(300),
                PhpValue::Int(999),
                PhpValue::Int(200),
                PhpValue::Int(400),
            ]),
        ])
    );
}

#[test]
fn test_dict_fluent_laravel_collection_methods() {
    clear_runtime_closure_macros();

    let source = r#"
        $product = Arr::dict([
            'name' => 'Desk',
            'price' => 100,
            'discount' => 10,
            'tax' => 5,
        ]);

        // 1. keys() and values()
        $keys = $product->keys();
        $values = $product->values();

        // 2. get(), has(), hasAny()
        $price = $product->get('price');
        $missing = $product->get('sku', 'DEFAULT-SKU');
        $hasName = $product->has('name');
        $hasMissing = $product->has('unknown');
        $hasAny = $product->hasAny(['sku', 'discount']);

        // 3. put(), pull(), forget()
        $product->put('in_stock', true);
        $pulledDiscount = $product->pull('discount');
        $product->forget('tax');

        // 4. map() with ($val, $key) and keys preserved
        $prices = Arr::dict(['item1' => 10, 'item2' => 20])
            ->map(fn($v, $k) => $v * 2);

        // 5. mapWithKeys()
        $keyed = Arr::dict(['first' => 1, 'second' => 2])
            ->mapWithKeys(fn($v, $k) => ['key_' . $k => $v * 10]);

        // 6. filter() and reject()
        $filtered = Arr::dict(['a' => 10, 'b' => 5, 'c' => 20])
            ->filter(fn($v, $k) => $v > 8);

        $rejected = Arr::dict(['a' => 10, 'b' => 5, 'c' => 20])
            ->reject(fn($v, $k) => $v > 8);

        // 7. reduce()
        $reduced = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3])
            ->reduce(fn($acc, $v, $k) => $acc + $v, 0);

        // 8. first(), last(), firstKey(), lastKey()
        $firstVal = Arr::dict(['x' => 'first_val', 'y' => 'second_val'])->first();
        $lastVal = Arr::dict(['x' => 'first_val', 'y' => 'second_val'])->last();
        $firstK = Arr::dict(['alpha' => 1, 'beta' => 2])->firstKey();
        $lastK = Arr::dict(['alpha' => 1, 'beta' => 2])->lastKey();

        // 9. only(), except(), flip(), merge()
        $onlyName = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3])->only(['a', 'c']);
        $exceptB = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3])->except(['b']);
        $flipped = Arr::dict(['role' => 'admin', 'status' => 'active'])->flip();
        $merged = Arr::dict(['a' => 1])->merge(Arr::dict(['b' => 2, 'a' => 99]));

        return [
            $keys,
            $values,
            $price,
            $missing,
            $hasName,
            $hasMissing,
            $hasAny,
            $product,
            $pulledDiscount,
            $prices,
            $keyed,
            $filtered,
            $rejected,
            $reduced,
            $firstVal,
            $lastVal,
            $firstK,
            $lastK,
            $onlyName,
            $exceptB,
            $flipped,
            $merged,
        ];
    "#;

    let res = run_source(source).expect("Laravel collection methods for Dict must succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::list(vec![
                PhpValue::string("name"),
                PhpValue::string("price"),
                PhpValue::string("discount"),
                PhpValue::string("tax"),
            ]),
            PhpValue::list(vec![
                PhpValue::string("Desk"),
                PhpValue::Int(100),
                PhpValue::Int(10),
                PhpValue::Int(5),
            ]),
            PhpValue::Int(100),
            PhpValue::string("DEFAULT-SKU"),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            // $product after put('in_stock', true), pull('discount'), forget('tax')
            PhpValue::dict(vec![
                ("name".to_string(), PhpValue::string("Desk")),
                ("price".to_string(), PhpValue::Int(100)),
                ("in_stock".to_string(), PhpValue::Bool(true)),
            ]),
            PhpValue::Int(10),
            // $prices
            PhpValue::dict(vec![
                ("item1".to_string(), PhpValue::Int(20)),
                ("item2".to_string(), PhpValue::Int(40)),
            ]),
            // $keyed
            PhpValue::dict(vec![
                ("key_first".to_string(), PhpValue::Int(10)),
                ("key_second".to_string(), PhpValue::Int(20)),
            ]),
            // $filtered
            PhpValue::dict(vec![
                ("a".to_string(), PhpValue::Int(10)),
                ("c".to_string(), PhpValue::Int(20)),
            ]),
            // $rejected
            PhpValue::dict(vec![("b".to_string(), PhpValue::Int(5))]),
            // $reduced
            PhpValue::Int(6),
            // firstVal, lastVal, firstK, lastK
            PhpValue::string("first_val"),
            PhpValue::string("second_val"),
            PhpValue::string("alpha"),
            PhpValue::string("beta"),
            // onlyName
            PhpValue::dict(vec![
                ("a".to_string(), PhpValue::Int(1)),
                ("c".to_string(), PhpValue::Int(3)),
            ]),
            // exceptB
            PhpValue::dict(vec![
                ("a".to_string(), PhpValue::Int(1)),
                ("c".to_string(), PhpValue::Int(3)),
            ]),
            // flipped
            PhpValue::dict(vec![
                ("admin".to_string(), PhpValue::string("role")),
                ("active".to_string(), PhpValue::string("status")),
            ]),
            // merged
            PhpValue::dict(vec![
                ("a".to_string(), PhpValue::Int(99)),
                ("b".to_string(), PhpValue::Int(2)),
            ]),
        ])
    );
}

#[test]
fn test_dict_sorting_reverse_and_json_methods() {
    clear_runtime_closure_macros();

    let source = r#"
        $dict = Arr::dict([
            'c' => 30,
            'a' => 10,
            'b' => 20,
        ]);

        // 1. sortKeys() and sortKeysDesc()
        $byKeyAsc = $dict->sortKeys();
        $byKeyDesc = $dict->sortKeysDesc();

        // 2. sort() (by values)
        $byValAsc = $dict->sort();

        // 3. reverse()
        $reversed = $dict->reverse();

        // 4. Numeric aggregates: sum, avg, min, max, count
        $sum = $dict->sum();
        $avg = $dict->avg();
        $min = $dict->min();
        $max = $dict->max();
        $count = $dict->count();

        // 5. chunk(), take(), skip(), slice()
        $chunked = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3, 'd' => 4])->chunk(2);
        $taken = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3])->take(2);
        $skipped = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3])->skip(1);
        $sliced = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3])->slice(1, 1);

        // 6. toJson()
        $json = Arr::dict(['name' => 'John', 'age' => 30, 'active' => true])->toJson();

        return [
            $byKeyAsc,
            $byKeyDesc,
            $byValAsc,
            $reversed,
            $sum,
            $avg,
            $min,
            $max,
            $count,
            $chunked,
            $taken,
            $skipped,
            $sliced,
            $json,
        ];
    "#;

    let res = run_source(source).expect("Sorting, reverse, and JSON must succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            // byKeyAsc
            PhpValue::dict(vec![
                ("a".to_string(), PhpValue::Int(10)),
                ("b".to_string(), PhpValue::Int(20)),
                ("c".to_string(), PhpValue::Int(30)),
            ]),
            // byKeyDesc
            PhpValue::dict(vec![
                ("c".to_string(), PhpValue::Int(30)),
                ("b".to_string(), PhpValue::Int(20)),
                ("a".to_string(), PhpValue::Int(10)),
            ]),
            // byValAsc
            PhpValue::dict(vec![
                ("a".to_string(), PhpValue::Int(10)),
                ("b".to_string(), PhpValue::Int(20)),
                ("c".to_string(), PhpValue::Int(30)),
            ]),
            // reversed
            PhpValue::dict(vec![
                ("b".to_string(), PhpValue::Int(20)),
                ("a".to_string(), PhpValue::Int(10)),
                ("c".to_string(), PhpValue::Int(30)),
            ]),
            // sum, avg, min, max, count
            PhpValue::Int(60),
            PhpValue::Float(20.0),
            PhpValue::Int(10),
            PhpValue::Int(30),
            PhpValue::Int(3),
            // chunked (List of Dicts)
            PhpValue::list(vec![
                PhpValue::dict(vec![
                    ("a".to_string(), PhpValue::Int(1)),
                    ("b".to_string(), PhpValue::Int(2)),
                ]),
                PhpValue::dict(vec![
                    ("c".to_string(), PhpValue::Int(3)),
                    ("d".to_string(), PhpValue::Int(4)),
                ]),
            ]),
            // taken
            PhpValue::dict(vec![
                ("a".to_string(), PhpValue::Int(1)),
                ("b".to_string(), PhpValue::Int(2)),
            ]),
            // skipped
            PhpValue::dict(vec![
                ("b".to_string(), PhpValue::Int(2)),
                ("c".to_string(), PhpValue::Int(3)),
            ]),
            // sliced
            PhpValue::dict(vec![("b".to_string(), PhpValue::Int(2))]),
            // json
            PhpValue::string(r#"{"name":"John","age":30,"active":true}"#),
        ])
    );
}

#[test]
fn test_dict_diff_and_intersect_helpers() {
    clear_runtime_closure_macros();

    let source = r#"
        $dict1 = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3]);
        $dict2 = Arr::dict(['b' => 2, 'c' => 4, 'd' => 5]);

        // diff: entries in dict1 whose values are not in dict2
        $diff = $dict1->diff($dict2);

        // diffKeys: entries in dict1 whose keys are not in dict2
        $diffKeys = $dict1->diffKeys($dict2);

        // intersect: entries in dict1 whose values are in dict2
        $intersect = $dict1->intersect($dict2);

        // intersectKeys: entries in dict1 whose keys are in dict2
        $intersectKeys = $dict1->intersectKeys($dict2);

        return [
            $diff,
            $diffKeys,
            $intersect,
            $intersectKeys,
        ];
    "#;

    let res = run_source(source).expect("Diff and intersect operations must succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            // diff: 1 and 3 are not in [2, 4, 5]
            PhpValue::dict(vec![
                ("a".to_string(), PhpValue::Int(1)),
                ("c".to_string(), PhpValue::Int(3)),
            ]),
            // diffKeys: 'a' is not in ['b', 'c', 'd']
            PhpValue::dict(vec![("a".to_string(), PhpValue::Int(1))]),
            // intersect: 2 is in [2, 4, 5]
            PhpValue::dict(vec![("b".to_string(), PhpValue::Int(2))]),
            // intersectKeys: 'b' and 'c' are in ['b', 'c', 'd']
            PhpValue::dict(vec![
                ("b".to_string(), PhpValue::Int(2)),
                ("c".to_string(), PhpValue::Int(3)),
            ]),
        ])
    );
}

#[test]
fn test_dict_laravel_arr_facade_static_methods() {
    clear_runtime_closure_macros();

    let source = r#"
        // 1. Nested Dict with dot notation Arr::get, Arr::has, Arr::set, Arr::forget
        $data = Arr::dict([
            'user' => Arr::dict([
                'profile' => Arr::dict([
                    'name' => 'Alice',
                    'email' => 'alice@example.com',
                ]),
                'role' => 'admin',
            ]),
        ]);

        $name = Arr::get($data, 'user.profile.name');
        $defaultFallback = Arr::get($data, 'user.profile.age', 25);
        $hasEmail = Arr::has($data, 'user.profile.email');
        $hasMissing = Arr::has($data, 'user.profile.missing');

        // Arr::set with dot notation
        $modified = Arr::set($data, 'user.profile.city', 'Amsterdam');
        $city = Arr::get($modified, 'user.profile.city');

        // Arr::forget with dot notation
        $forgotten = Arr::forget($modified, 'user.role');
        $hasRole = Arr::has($forgotten, 'user.role');

        // 2. Arr::dot and Arr::undot
        $flat = Arr::dot(Arr::dict([
            'db' => Arr::dict([
                'host' => '127.0.0.1',
                'port' => 3306,
            ]),
        ]));

        $expanded = Arr::undot($flat);

        // 3. Arr::divide
        $divided = Arr::divide(Arr::dict(['x' => 10, 'y' => 20]));

        // 4. Arr::keyBy
        $users = Arr::list([
            Arr::dict(['id' => 'u1', 'name' => 'John']),
            Arr::dict(['id' => 'u2', 'name' => 'Jane']),
        ]);
        $keyedUsers = Arr::keyBy($users, 'id');

        // 5. Arr::pluck (both list and keyed dict)
        $names = Arr::pluck($users, 'name');
        $keyedNames = Arr::pluck($users, 'name', 'id');

        // 6. Arr::isAssoc and Arr::isList
        $isAssocDict = Arr::isAssoc(Arr::dict(['k' => 'v']));
        $isListDict = Arr::isList(Arr::dict(['k' => 'v']));
        $isListReal = Arr::isList(Arr::list([1, 2, 3]));

        return [
            $name,
            $defaultFallback,
            $hasEmail,
            $hasMissing,
            $city,
            $hasRole,
            $flat,
            $expanded,
            $divided,
            $keyedUsers,
            $names,
            $keyedNames,
            $isAssocDict,
            $isListDict,
            $isListReal,
        ];
    "#;

    let res = run_source(source).expect("Laravel Arr facade static methods must succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::string("Alice"),
            PhpValue::Int(25),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::string("Amsterdam"),
            PhpValue::Bool(false),
            // $flat: Arr::dot
            PhpValue::dict(vec![
                ("db.host".to_string(), PhpValue::string("127.0.0.1")),
                ("db.port".to_string(), PhpValue::Int(3306)),
            ]),
            // $expanded: Arr::undot
            PhpValue::dict(vec![(
                "db".to_string(),
                PhpValue::dict(vec![
                    ("host".to_string(), PhpValue::string("127.0.0.1")),
                    ("port".to_string(), PhpValue::Int(3306)),
                ]),
            )]),
            // $divided: [[x, y], [10, 20]]
            PhpValue::list(vec![
                PhpValue::list(vec![PhpValue::string("x"), PhpValue::string("y")]),
                PhpValue::list(vec![PhpValue::Int(10), PhpValue::Int(20)]),
            ]),
            // $keyedUsers
            PhpValue::dict(vec![
                (
                    "u1".to_string(),
                    PhpValue::dict(vec![
                        ("id".to_string(), PhpValue::string("u1")),
                        ("name".to_string(), PhpValue::string("John")),
                    ]),
                ),
                (
                    "u2".to_string(),
                    PhpValue::dict(vec![
                        ("id".to_string(), PhpValue::string("u2")),
                        ("name".to_string(), PhpValue::string("Jane")),
                    ]),
                ),
            ]),
            // $names: unkeyed list
            PhpValue::list(vec![PhpValue::string("John"), PhpValue::string("Jane")]),
            // $keyedNames: keyed dict
            PhpValue::dict(vec![
                ("u1".to_string(), PhpValue::string("John")),
                ("u2".to_string(), PhpValue::string("Jane")),
            ]),
            // isAssocDict, isListDict, isListReal
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_arr_macro_registration_and_dict_execution() {
    clear_runtime_closure_macros();

    let source = r#"
        Arr::macro('upperValues', function () {
            return $this->map(fn($v, $k) => strtoupper($v));
        });

        $dict = Arr::dict([
            'greeting' => 'hello',
            'farewell' => 'goodbye',
        ]);

        $upper = $dict->upperValues();

        Arr::macro('toHeaderLine', function ($sep = ': ') {
            $lines = [];
            foreach ($this->all() as $k => $v) {
                $lines[] = $k . $sep . $v;
            }
            return implode("\n", $lines);
        });

        $headers = Arr::dict([
            'Content-Type' => 'application/json',
            'Authorization' => 'Bearer token123',
        ])->toHeaderLine();

        return Arr::list([$upper, $headers]);
    "#;

    let res = run_source(source).expect("Macro execution on Dict must succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::dict(vec![
                ("greeting".to_string(), PhpValue::string("HELLO")),
                ("farewell".to_string(), PhpValue::string("GOODBYE")),
            ]),
            PhpValue::string("Content-Type: application/json\nAuthorization: Bearer token123"),
        ])
    );
}

#[test]
fn test_dict_type_boundary_contracts() {
    clear_runtime_closure_macros();

    // 1. Function boundary contract with `Dict` parameter and return type
    let source = r#"
        function mergeWithDefaults(Dict $options): Dict {
            return $options->merge(Arr::dict(['timeout' => 30, 'retries' => 3]));
        }

        $input = Arr::dict(['timeout' => 60, 'debug' => true]);
        $output = mergeWithDefaults($input);
        return $output;
    "#;

    let res = run_source(source).expect("Boundary contract with Dict should pass");
    assert_eq!(
        res,
        PhpValue::dict(vec![
            ("timeout".to_string(), PhpValue::Int(30)),
            ("debug".to_string(), PhpValue::Bool(true)),
            ("retries".to_string(), PhpValue::Int(3)),
        ])
    );

    // 2. Static type boundary contract rejection at compile-time
    let source_compile_invalid = r#"
        function requireDict(Dict $items): int {
            return $items->count();
        }

        requireDict('not a dict');
    "#;

    let err_compile =
        run_source(source_compile_invalid).expect_err("Invalid type must fail static type checking");
    assert!(
        err_compile.to_string().contains("expected 'Dict'")
            || err_compile.to_string().contains("expected 'dict'"),
        "Expected type mismatch on 'Dict', got: {:?}",
        err_compile
    );

    // 3. Dynamic runtime boundary contract rejection
    let source_runtime_invalid = r#"
        function requireDictDynamic(Dict $items): int {
            return $items->count();
        }

        function testBridge(mixed $x): int {
            return requireDictDynamic($x);
        }

        testBridge('not a dict');
    "#;

    let err_runtime = run_source(source_runtime_invalid)
        .expect_err("Dynamic invalid type must violate runtime boundary contract");
    assert!(
        err_runtime.to_string().contains("Boundary contract violation"),
        "Expected Boundary contract violation, got: {:?}",
        err_runtime
    );
}

#[test]
fn test_dict_lowercase_type_hint_and_local_scope() {
    clear_runtime_closure_macros();

    let source = r#"
        function configureApp(dict $config): dict {
            dict $scoped = [];
            $scoped['app_name'] = $config->get('name', 'MyApp');
            $scoped['env'] = $config->get('env', 'production');
            $scoped['ready'] = true;
            return $scoped;
        }

        $input = Arr::dict(['name' => 'SuperEngine', 'env' => 'staging']);
        return configureApp($input);
    "#;

    let res = run_source(source).expect("configureApp with lowercase dict must succeed");
    assert_eq!(
        res,
        PhpValue::dict(vec![
            ("app_name".to_string(), PhpValue::string("SuperEngine")),
            ("env".to_string(), PhpValue::string("staging")),
            ("ready".to_string(), PhpValue::Bool(true)),
        ])
    );
}
