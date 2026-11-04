use rhp::run_source;
use rhp::vm::value::PhpValue;

#[test]
fn test_sorting_on_list_dict_set() {
    let code = r#"
        // 1. List sorting
        $orig_list = Arr::list([30, 10, 20]);
        $sorted_list1 = $orig_list->sort();
        $sorted_list2 = $orig_list->usort(fn($a, $b) => $b <=> $a);
        $sorted_list3 = usort($orig_list, fn($a, $b) => $a <=> $b);
        $sorted_list4 = asort($orig_list);

        // 2. Set sorting
        $orig_set = Arr::set([50, 10, 40, 20]);
        $sorted_set1 = $orig_set->sort();
        $sorted_set2 = $orig_set->usort(fn($a, $b) => $b <=> $a);
        $sorted_set3 = usort($orig_set, fn($a, $b) => $a <=> $b);
        $sorted_set4 = asort($orig_set);

        // 3. Dict sorting by value and by key
        $orig_dict = Arr::dict(['b' => 30, 'a' => 10, 'c' => 20]);
        $sorted_dict_val = $orig_dict->sort(); // asort: by value
        $sorted_dict_val_cb = $orig_dict->uasort(fn($a, $b) => $b <=> $a); // uasort
        $sorted_dict_val_proc = asort($orig_dict);
        $sorted_dict_val_usort = uasort($orig_dict, fn($a, $b) => $a <=> $b);

        $sorted_dict_key = $orig_dict->sortKeys(); // ksort: by key
        $sorted_dict_key_proc = ksort($orig_dict);
        $sorted_dict_key_cb = $orig_dict->sortKeysBy(fn($a, $b) => $b <=> $a); // uksort
        $sorted_dict_key_uksort = uksort($orig_dict, fn($a, $b) => $a <=> $b);

        return Arr::dict([
            'orig_list' => $orig_list,
            'list1' => $sorted_list1,
            'list2' => $sorted_list2,
            'list3' => $sorted_list3,
            'list4' => $sorted_list4,

            'orig_set' => $orig_set,
            'set1' => $sorted_set1,
            'set2' => $sorted_set2,
            'set3' => $sorted_set3,
            'set4' => $sorted_set4,

            'orig_dict' => $orig_dict,
            'dict_val' => $sorted_dict_val,
            'dict_val_cb' => $sorted_dict_val_cb,
            'dict_val_proc' => $sorted_dict_val_proc,
            'dict_val_usort' => $sorted_dict_val_usort,
            'dict_key' => $sorted_dict_key,
            'dict_key_proc' => $sorted_dict_key_proc,
            'dict_key_cb' => $sorted_dict_key_cb,
            'dict_key_uksort' => $sorted_dict_key_uksort,
        ]);
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Dict(dict) = res {
        let d = dict.borrow();

        // Check list immutability & values
        if let PhpValue::List(l) = d.get("orig_list").unwrap() {
            let b = l.borrow();
            assert_eq!(b[0], PhpValue::Int(30));
            assert_eq!(b[1], PhpValue::Int(10));
            assert_eq!(b[2], PhpValue::Int(20));
        } else {
            panic!("expected list");
        }

        if let PhpValue::List(l) = d.get("list1").unwrap() {
            let b = l.borrow();
            assert_eq!(b.as_slice(), &[PhpValue::Int(10), PhpValue::Int(20), PhpValue::Int(30)]);
        } else {
            panic!("expected list1");
        }

        if let PhpValue::List(l) = d.get("list2").unwrap() {
            let b = l.borrow();
            assert_eq!(b.as_slice(), &[PhpValue::Int(30), PhpValue::Int(20), PhpValue::Int(10)]);
        } else {
            panic!("expected list2");
        }

        if let PhpValue::List(l) = d.get("list3").unwrap() {
            let b = l.borrow();
            assert_eq!(b.as_slice(), &[PhpValue::Int(10), PhpValue::Int(20), PhpValue::Int(30)]);
        } else {
            panic!("expected list3");
        }

        // Check set
        if let PhpValue::Set(s) = d.get("orig_set").unwrap() {
            assert_eq!(s.borrow().len(), 4);
        } else {
            panic!("expected set");
        }

        if let PhpValue::Set(s) = d.get("set1").unwrap() {
            let b = s.borrow();
            assert_eq!(b.len(), 4);
            assert!(b.contains(&PhpValue::Int(10)));
            assert!(b.contains(&PhpValue::Int(20)));
            assert!(b.contains(&PhpValue::Int(40)));
            assert!(b.contains(&PhpValue::Int(50)));
        } else {
            panic!("expected set1");
        }

        if let PhpValue::Set(s) = d.get("set2").unwrap() {
            let b = s.borrow();
            assert_eq!(b.len(), 4);
            assert!(b.contains(&PhpValue::Int(10)));
            assert!(b.contains(&PhpValue::Int(20)));
            assert!(b.contains(&PhpValue::Int(40)));
            assert!(b.contains(&PhpValue::Int(50)));
        } else {
            panic!("expected set2");
        }

        // Check dict
        if let PhpValue::Dict(d_orig) = d.get("orig_dict").unwrap() {
            let b = d_orig.borrow();
            let keys: Vec<String> = b.keys().cloned().collect();
            assert_eq!(keys, vec!["b".to_string(), "a".to_string(), "c".to_string()]);
        } else {
            panic!("expected orig_dict");
        }

        if let PhpValue::Dict(d_val) = d.get("dict_val").unwrap() {
            let b = d_val.borrow();
            let keys: Vec<String> = b.keys().cloned().collect();
            let vals: Vec<PhpValue> = b.values().cloned().collect();
            assert_eq!(keys, vec!["a".to_string(), "c".to_string(), "b".to_string()]);
            assert_eq!(vals, vec![PhpValue::Int(10), PhpValue::Int(20), PhpValue::Int(30)]);
        } else {
            panic!("expected dict_val");
        }

        if let PhpValue::Dict(d_val_cb) = d.get("dict_val_cb").unwrap() {
            let b = d_val_cb.borrow();
            let keys: Vec<String> = b.keys().cloned().collect();
            assert_eq!(keys, vec!["b".to_string(), "c".to_string(), "a".to_string()]);
        } else {
            panic!("expected dict_val_cb");
        }

        if let PhpValue::Dict(d_key) = d.get("dict_key").unwrap() {
            let b = d_key.borrow();
            let keys: Vec<String> = b.keys().cloned().collect();
            assert_eq!(keys, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        } else {
            panic!("expected dict_key");
        }

        if let PhpValue::Dict(d_key_proc) = d.get("dict_key_proc").unwrap() {
            let b = d_key_proc.borrow();
            let keys: Vec<String> = b.keys().cloned().collect();
            assert_eq!(keys, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        } else {
            panic!("expected dict_key_proc");
        }

        if let PhpValue::Dict(d_key_cb) = d.get("dict_key_cb").unwrap() {
            let b = d_key_cb.borrow();
            let keys: Vec<String> = b.keys().cloned().collect();
            assert_eq!(keys, vec!["c".to_string(), "b".to_string(), "a".to_string()]);
        } else {
            panic!("expected dict_key_cb");
        }
    } else {
        panic!("expected result dict");
    }
}

#[test]
fn test_diff_and_diff_key_on_list_dict_set() {
    let code = r#"
        // 1. List diff
        $l1 = Arr::list([1, 2, 3, 4, 5]);
        $l2 = Arr::list([2, 4]);
        $diff_list_fluent = $l1->diff($l2);
        $diff_list_proc = array_diff($l1, $l2);
        $diff_list_arr = Arr::diff($l1, $l2);

        // 2. Dict diff & diffKey
        $d1 = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3, 'd' => 4]);
        $d2 = Arr::dict(['b' => 20, 'c' => 3, 'e' => 5]);
        // diff compares values: 3 has matching value 3, so 'c' is excluded. 'a', 'b', 'd' remain.
        $diff_dict_val = $d1->diff($d2);
        $diff_dict_val_proc = array_diff($d1, $d2);

        // diffKey compares keys: keys 'b' and 'c' exist in d2, so 'b' and 'c' are excluded. 'a', 'd' remain.
        $diff_dict_key = $d1->diffKey($d2);
        $diff_dict_key_proc = array_diff_key($d1, $d2);
        $diff_dict_key_arr = Arr::diffKey($d1, $d2);

        // 3. Set diff
        $s1 = Arr::set([10, 20, 30, 40]);
        $s2 = Arr::set([20, 40, 50]);
        $diff_set_fluent = $s1->diff($s2);
        $diff_set_proc = array_diff($s1, $s2);
        $diff_set_arr = Arr::diff($s1, $s2);

        return Arr::dict([
            'diff_list_fluent' => $diff_list_fluent,
            'diff_list_proc' => $diff_list_proc,
            'diff_list_arr' => $diff_list_arr,

            'diff_dict_val' => $diff_dict_val,
            'diff_dict_val_proc' => $diff_dict_val_proc,
            'diff_dict_key' => $diff_dict_key,
            'diff_dict_key_proc' => $diff_dict_key_proc,
            'diff_dict_key_arr' => $diff_dict_key_arr,

            'diff_set_fluent' => $diff_set_fluent,
            'diff_set_proc' => $diff_set_proc,
            'diff_set_arr' => $diff_set_arr,
        ]);
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Dict(dict) = res {
        let d = dict.borrow();

        // List diff results: [1, 3, 5]
        for key in &["diff_list_fluent", "diff_list_proc", "diff_list_arr"] {
            if let PhpValue::List(l) = d.get(*key).unwrap() {
                assert_eq!(l.borrow().as_slice(), &[PhpValue::Int(1), PhpValue::Int(3), PhpValue::Int(5)]);
            } else {
                panic!("expected list for {}", key);
            }
        }

        // Dict diff by value: 'a'=>1, 'b'=>2, 'd'=>4
        for key in &["diff_dict_val", "diff_dict_val_proc"] {
            if let PhpValue::Dict(sub_d) = d.get(*key).unwrap() {
                let b = sub_d.borrow();
                assert_eq!(b.get("a"), Some(&PhpValue::Int(1)));
                assert_eq!(b.get("b"), Some(&PhpValue::Int(2)));
                assert_eq!(b.get("c"), None);
                assert_eq!(b.get("d"), Some(&PhpValue::Int(4)));
            } else {
                panic!("expected dict for {}", key);
            }
        }

        // Dict diff by key: 'a'=>1, 'd'=>4
        for key in &["diff_dict_key", "diff_dict_key_proc", "diff_dict_key_arr"] {
            if let PhpValue::Dict(sub_d) = d.get(*key).unwrap() {
                let b = sub_d.borrow();
                assert_eq!(b.get("a"), Some(&PhpValue::Int(1)));
                assert_eq!(b.get("b"), None);
                assert_eq!(b.get("c"), None);
                assert_eq!(b.get("d"), Some(&PhpValue::Int(4)));
            } else {
                panic!("expected dict for {}", key);
            }
        }

        // Set diff: {10, 30}
        for key in &["diff_set_fluent", "diff_set_proc", "diff_set_arr"] {
            if let PhpValue::Set(s) = d.get(*key).unwrap() {
                let b = s.borrow();
                assert_eq!(b.len(), 2);
                assert!(b.contains(&PhpValue::Int(10)));
                assert!(b.contains(&PhpValue::Int(30)));
            } else {
                panic!("expected set for {}", key);
            }
        }
    } else {
        panic!("expected result dict");
    }
}

#[test]
fn test_intersect_and_intersect_key_on_list_dict_set() {
    let code = r#"
        // 1. List intersect
        $l1 = Arr::list([1, 2, 3, 4, 5]);
        $l2 = Arr::list([2, 4, 6]);
        $inter_list_fluent = $l1->intersect($l2);
        $inter_list_proc = array_intersect($l1, $l2);
        $inter_list_arr = Arr::intersect($l1, $l2);

        // 2. Dict intersect & intersectKey
        $d1 = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3, 'd' => 4]);
        $d2 = Arr::dict(['b' => 20, 'c' => 3, 'e' => 5]);
        // intersect compares values: 3 matches 3, so only 'c' matches
        $inter_dict_val = $d1->intersect($d2);
        $inter_dict_val_proc = array_intersect($d1, $d2);

        // intersectKey compares keys: 'b' and 'c' exist in both
        $inter_dict_key = $d1->intersectKey($d2);
        $inter_dict_key_proc = array_intersect_key($d1, $d2);
        $inter_dict_key_arr = Arr::intersectKey($d1, $d2);

        // 3. Set intersect
        $s1 = Arr::set([10, 20, 30, 40]);
        $s2 = Arr::set([20, 40, 50]);
        $inter_set_fluent = $s1->intersect($s2);
        $inter_set_proc = array_intersect($s1, $s2);
        $inter_set_arr = Arr::intersect($s1, $s2);

        return Arr::dict([
            'inter_list_fluent' => $inter_list_fluent,
            'inter_list_proc' => $inter_list_proc,
            'inter_list_arr' => $inter_list_arr,

            'inter_dict_val' => $inter_dict_val,
            'inter_dict_val_proc' => $inter_dict_val_proc,
            'inter_dict_key' => $inter_dict_key,
            'inter_dict_key_proc' => $inter_dict_key_proc,
            'inter_dict_key_arr' => $inter_dict_key_arr,

            'inter_set_fluent' => $inter_set_fluent,
            'inter_set_proc' => $inter_set_proc,
            'inter_set_arr' => $inter_set_arr,
        ]);
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Dict(dict) = res {
        let d = dict.borrow();

        // List intersect: [2, 4]
        for key in &["inter_list_fluent", "inter_list_proc", "inter_list_arr"] {
            if let PhpValue::List(l) = d.get(*key).unwrap() {
                assert_eq!(l.borrow().as_slice(), &[PhpValue::Int(2), PhpValue::Int(4)]);
            } else {
                panic!("expected list for {}", key);
            }
        }

        // Dict intersect by value: 'c' => 3
        for key in &["inter_dict_val", "inter_dict_val_proc"] {
            if let PhpValue::Dict(sub_d) = d.get(*key).unwrap() {
                let b = sub_d.borrow();
                assert_eq!(b.len(), 1);
                assert_eq!(b.get("c"), Some(&PhpValue::Int(3)));
            } else {
                panic!("expected dict for {}", key);
            }
        }

        // Dict intersect by key: 'b' => 2, 'c' => 3
        for key in &["inter_dict_key", "inter_dict_key_proc", "inter_dict_key_arr"] {
            if let PhpValue::Dict(sub_d) = d.get(*key).unwrap() {
                let b = sub_d.borrow();
                assert_eq!(b.len(), 2);
                assert_eq!(b.get("b"), Some(&PhpValue::Int(2)));
                assert_eq!(b.get("c"), Some(&PhpValue::Int(3)));
            } else {
                panic!("expected dict for {}", key);
            }
        }

        // Set intersect: {20, 40}
        for key in &["inter_set_fluent", "inter_set_proc", "inter_set_arr"] {
            if let PhpValue::Set(s) = d.get(*key).unwrap() {
                let b = s.borrow();
                assert_eq!(b.len(), 2);
                assert!(b.contains(&PhpValue::Int(20)));
                assert!(b.contains(&PhpValue::Int(40)));
            } else {
                panic!("expected set for {}", key);
            }
        }
    } else {
        panic!("expected result dict");
    }
}

#[test]
fn test_array_helpers_column_chunk_flip_combine() {
    let code = r#"
        // 1. Column / array_column
        $records = Arr::list([
            Arr::dict(['id' => 101, 'name' => 'Alice', 'role' => 'dev']),
            Arr::dict(['id' => 102, 'name' => 'Bob', 'role' => 'qa']),
            Arr::dict(['id' => 103, 'name' => 'Charlie', 'role' => 'pm']),
        ]);
        $names = $records->column('name');
        $proc_names = array_column($records, 'name');
        $indexed_names = array_column($records, 'name', 'id');

        // 2. Chunk / array_chunk
        $list = Arr::list([1, 2, 3, 4, 5]);
        $chunked_list = $list->chunk(2);
        $proc_chunked_list = array_chunk($list, 2);

        $dict = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3, 'd' => 4, 'e' => 5]);
        $chunked_dict = $dict->chunk(2);

        $set = Arr::set([10, 20, 30, 40, 50]);
        $chunked_set = $set->chunk(2);

        // 3. Flip / array_flip
        $orig_d = Arr::dict(['first' => 'a', 'second' => 'b']);
        $flipped_dict = $orig_d->flip();
        $proc_flipped = array_flip($orig_d);

        $list_str = Arr::list(['red', 'green', 'blue']);
        $flipped_list = $list_str->flip(); // Produces dict ['red' => 0, 'green' => 1, 'blue' => 2]

        // 4. Combine / array_combine
        $keys = Arr::list(['x', 'y', 'z']);
        $vals = Arr::list([100, 200, 300]);
        $combined = array_combine($keys, $vals);
        $fluent_combined = $keys->combine($vals);
        $arr_combined = Arr::combine($keys, $vals);

        return Arr::dict([
            'names' => $names,
            'proc_names' => $proc_names,
            'indexed_names' => $indexed_names,

            'chunked_list' => $chunked_list,
            'proc_chunked_list' => $proc_chunked_list,
            'chunked_dict' => $chunked_dict,
            'chunked_set' => $chunked_set,

            'flipped_dict' => $flipped_dict,
            'proc_flipped' => $proc_flipped,
            'flipped_list' => $flipped_list,

            'combined' => $combined,
            'fluent_combined' => $fluent_combined,
            'arr_combined' => $arr_combined,
        ]);
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Dict(dict) = res {
        let d = dict.borrow();

        // Check column
        for key in &["names", "proc_names"] {
            if let PhpValue::List(l) = d.get(*key).unwrap() {
                assert_eq!(
                    l.borrow().as_slice(),
                    &[
                        PhpValue::string("Alice"),
                        PhpValue::string("Bob"),
                        PhpValue::string("Charlie"),
                    ]
                );
            } else {
                panic!("expected list for {}", key);
            }
        }

        if let PhpValue::Dict(sub_d) = d.get("indexed_names").unwrap() {
            let b = sub_d.borrow();
            assert_eq!(b.get("101"), Some(&PhpValue::string("Alice")));
            assert_eq!(b.get("102"), Some(&PhpValue::string("Bob")));
            assert_eq!(b.get("103"), Some(&PhpValue::string("Charlie")));
        } else {
            panic!("expected dict for indexed_names");
        }

        // Check chunked list: 3 chunks [[1, 2], [3, 4], [5]]
        for key in &["chunked_list", "proc_chunked_list"] {
            if let PhpValue::List(chunks) = d.get(*key).unwrap() {
                let cb = chunks.borrow();
                assert_eq!(cb.len(), 3);
                if let PhpValue::List(c0) = &cb[0] {
                    assert_eq!(c0.borrow().as_slice(), &[PhpValue::Int(1), PhpValue::Int(2)]);
                } else {
                    panic!("expected list chunk");
                }
                if let PhpValue::List(c2) = &cb[2] {
                    assert_eq!(c2.borrow().as_slice(), &[PhpValue::Int(5)]);
                } else {
                    panic!("expected list chunk");
                }
            } else {
                panic!("expected chunked list");
            }
        }

        // Check chunked dict: 3 chunks of dicts
        if let PhpValue::List(chunks) = d.get("chunked_dict").unwrap() {
            let cb = chunks.borrow();
            assert_eq!(cb.len(), 3);
            if let PhpValue::Dict(cd0) = &cb[0] {
                let b = cd0.borrow();
                assert_eq!(b.len(), 2);
                assert_eq!(b.get("a"), Some(&PhpValue::Int(1)));
                assert_eq!(b.get("b"), Some(&PhpValue::Int(2)));
            } else {
                panic!("expected dict chunk");
            }
        } else {
            panic!("expected chunked dict");
        }

        // Check chunked set: 3 chunks of sets
        if let PhpValue::List(chunks) = d.get("chunked_set").unwrap() {
            let cb = chunks.borrow();
            assert_eq!(cb.len(), 3);
            if let PhpValue::Set(cs0) = &cb[0] {
                assert_eq!(cs0.borrow().len(), 2);
            } else {
                panic!("expected set chunk");
            }
        } else {
            panic!("expected chunked set");
        }

        // Check flip
        for key in &["flipped_dict", "proc_flipped"] {
            if let PhpValue::Dict(fd) = d.get(*key).unwrap() {
                let b = fd.borrow();
                assert_eq!(b.get("a"), Some(&PhpValue::string("first")));
                assert_eq!(b.get("b"), Some(&PhpValue::string("second")));
            } else {
                panic!("expected dict for {}", key);
            }
        }

        if let PhpValue::Dict(fd) = d.get("flipped_list").unwrap() {
            let b = fd.borrow();
            assert_eq!(b.get("red"), Some(&PhpValue::Int(0)));
            assert_eq!(b.get("green"), Some(&PhpValue::Int(1)));
            assert_eq!(b.get("blue"), Some(&PhpValue::Int(2)));
        } else {
            panic!("expected dict for flipped_list");
        }

        // Check combine
        for key in &["combined", "fluent_combined", "arr_combined"] {
            if let PhpValue::Dict(cd) = d.get(*key).unwrap() {
                let b = cd.borrow();
                assert_eq!(b.get("x"), Some(&PhpValue::Int(100)));
                assert_eq!(b.get("y"), Some(&PhpValue::Int(200)));
                assert_eq!(b.get("z"), Some(&PhpValue::Int(300)));
            } else {
                panic!("expected dict for {}", key);
            }
        }
    } else {
        panic!("expected result dict");
    }
}

#[test]
fn test_array_count_values_on_list_dict_set() {
    let code = r#"
        // 1. List
        $list = Arr::list(['apple', 'banana', 'apple', 'orange', 'banana', 'apple']);
        $list_cnt_fluent = $list->countValues();
        $list_cnt_proc = array_count_values($list);
        $list_cnt_arr = Arr::countValues($list);

        // 2. Dict (counts frequencies of values)
        $dict = Arr::dict(['a' => 'cat', 'b' => 'dog', 'c' => 'cat', 'd' => 'bird', 'e' => 'dog']);
        $dict_cnt_fluent = $dict->countValues();
        $dict_cnt_proc = array_count_values($dict);
        $dict_cnt_arr = Arr::countValues($dict);

        // 3. Set (each unique element has frequency 1)
        $set = Arr::set(['red', 'green', 'blue']);
        $set_cnt_fluent = $set->countValues();
        $set_cnt_proc = array_count_values($set);
        $set_cnt_arr = Arr::countValues($set);

        return Arr::dict([
            'list_fluent' => $list_cnt_fluent,
            'list_proc' => $list_cnt_proc,
            'list_arr' => $list_cnt_arr,

            'dict_fluent' => $dict_cnt_fluent,
            'dict_proc' => $dict_cnt_proc,
            'dict_arr' => $dict_cnt_arr,

            'set_fluent' => $set_cnt_fluent,
            'set_proc' => $set_cnt_proc,
            'set_arr' => $set_cnt_arr,
        ]);
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Dict(dict) = res {
        let d = dict.borrow();

        // Verify List results
        for key in &["list_fluent", "list_proc", "list_arr"] {
            if let PhpValue::Dict(sub_d) = d.get(*key).unwrap() {
                let b = sub_d.borrow();
                assert_eq!(b.get("apple"), Some(&PhpValue::Int(3)));
                assert_eq!(b.get("banana"), Some(&PhpValue::Int(2)));
                assert_eq!(b.get("orange"), Some(&PhpValue::Int(1)));
            } else {
                panic!("expected dict for {}", key);
            }
        }

        // Verify Dict results
        for key in &["dict_fluent", "dict_proc", "dict_arr"] {
            if let PhpValue::Dict(sub_d) = d.get(*key).unwrap() {
                let b = sub_d.borrow();
                assert_eq!(b.get("cat"), Some(&PhpValue::Int(2)));
                assert_eq!(b.get("dog"), Some(&PhpValue::Int(2)));
                assert_eq!(b.get("bird"), Some(&PhpValue::Int(1)));
            } else {
                panic!("expected dict for {}", key);
            }
        }

        // Verify Set results
        for key in &["set_fluent", "set_proc", "set_arr"] {
            if let PhpValue::Dict(sub_d) = d.get(*key).unwrap() {
                let b = sub_d.borrow();
                assert_eq!(b.get("red"), Some(&PhpValue::Int(1)));
                assert_eq!(b.get("green"), Some(&PhpValue::Int(1)));
                assert_eq!(b.get("blue"), Some(&PhpValue::Int(1)));
            } else {
                panic!("expected dict for {}", key);
            }
        }
    } else {
        panic!("expected result dict");
    }
}

#[test]
fn test_shuffle_on_list_dict_set() {
    let code = r#"
        // 1. List
        $l = Arr::list([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        $shuffled_l1 = $l->shuffle();
        $shuffled_l2 = shuffle($l);
        $shuffled_l3 = Arr::shuffle($l);

        // 2. Dict
        $d = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3, 'd' => 4, 'e' => 5]);
        $shuffled_d1 = $d->shuffle();
        $shuffled_d2 = shuffle($d);
        $shuffled_d3 = Arr::shuffle($d);

        // 3. Set
        $s = Arr::set([10, 20, 30, 40, 50]);
        $shuffled_s1 = $s->shuffle();
        $shuffled_s2 = shuffle($s);
        $shuffled_s3 = Arr::shuffle($s);

        return Arr::dict([
            'orig_l' => $l,
            'shuffled_l1' => $shuffled_l1,
            'shuffled_l2' => $shuffled_l2,
            'shuffled_l3' => $shuffled_l3,

            'orig_d' => $d,
            'shuffled_d1' => $shuffled_d1,
            'shuffled_d2' => $shuffled_d2,
            'shuffled_d3' => $shuffled_d3,

            'orig_s' => $s,
            'shuffled_s1' => $shuffled_s1,
            'shuffled_s2' => $shuffled_s2,
            'shuffled_s3' => $shuffled_s3,
        ]);
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Dict(dict) = res {
        let d = dict.borrow();

        // 1. Verify original list is unchanged
        if let PhpValue::List(orig) = d.get("orig_l").unwrap() {
            assert_eq!(
                orig.borrow().as_slice(),
                &[
                    PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3), PhpValue::Int(4), PhpValue::Int(5),
                    PhpValue::Int(6), PhpValue::Int(7), PhpValue::Int(8), PhpValue::Int(9), PhpValue::Int(10)
                ]
            );
        } else {
            panic!("expected orig_l to be list");
        }

        // Shuffled lists contain all elements 1..10
        for key in &["shuffled_l1", "shuffled_l2", "shuffled_l3"] {
            if let PhpValue::List(shuf) = d.get(*key).unwrap() {
                let b = shuf.borrow();
                assert_eq!(b.len(), 10);
                for i in 1..=10 {
                    assert!(b.contains(&PhpValue::Int(i)));
                }
            } else {
                panic!("expected list for {}", key);
            }
        }

        // 2. Dict: original is intact
        if let PhpValue::Dict(orig_d) = d.get("orig_d").unwrap() {
            let b = orig_d.borrow();
            let keys: Vec<String> = b.keys().cloned().collect();
            assert_eq!(keys, vec!["a", "b", "c", "d", "e"]);
        }

        for key in &["shuffled_d1", "shuffled_d2", "shuffled_d3"] {
            if let PhpValue::Dict(shuf_d) = d.get(*key).unwrap() {
                let b = shuf_d.borrow();
                assert_eq!(b.len(), 5);
                assert_eq!(b.get("a"), Some(&PhpValue::Int(1)));
                assert_eq!(b.get("b"), Some(&PhpValue::Int(2)));
                assert_eq!(b.get("c"), Some(&PhpValue::Int(3)));
                assert_eq!(b.get("d"), Some(&PhpValue::Int(4)));
                assert_eq!(b.get("e"), Some(&PhpValue::Int(5)));
            } else {
                panic!("expected dict for {}", key);
            }
        }

        // 3. Set
        for key in &["shuffled_s1", "shuffled_s2", "shuffled_s3"] {
            if let PhpValue::Set(shuf_s) = d.get(*key).unwrap() {
                let b = shuf_s.borrow();
                assert_eq!(b.len(), 5);
                for val in &[10, 20, 30, 40, 50] {
                    assert!(b.contains(&PhpValue::Int(*val)));
                }
            } else {
                panic!("expected set for {}", key);
            }
        }
    } else {
        panic!("expected result dict");
    }
}

#[test]
fn test_array_rand_on_list_dict_set() {
    let code = r#"
        // 1. List
        $l = Arr::list(['apple', 'banana', 'cherry', 'date']);
        $rand_key1 = $l->randomKey();
        $rand_key2 = array_rand($l);
        $rand_key3 = Arr::randomKey($l);
        $rand_keys_multi = array_rand($l, 2);
        $rand_val = $l->random();
        $rand_vals = $l->random(2);

        // 2. Dict
        $d = Arr::dict(['x' => 10, 'y' => 20, 'z' => 30]);
        $rand_dict_key1 = $d->randomKey();
        $rand_dict_key2 = array_rand($d);
        $rand_dict_key3 = Arr::randomKey($d);
        $rand_dict_keys_multi = array_rand($d, 2);
        $rand_dict_val = $d->random();
        $rand_dict_subset = $d->random(2);

        // 3. Set
        $s = Arr::set(['alpha', 'beta', 'gamma', 'delta']);
        $rand_set_item1 = $s->randomKey();
        $rand_set_item2 = array_rand($s);
        $rand_set_items_multi = array_rand($s, 2);

        return Arr::dict([
            'rand_key1' => $rand_key1,
            'rand_key2' => $rand_key2,
            'rand_key3' => $rand_key3,
            'rand_keys_multi' => $rand_keys_multi,
            'rand_val' => $rand_val,
            'rand_vals' => $rand_vals,

            'rand_dict_key1' => $rand_dict_key1,
            'rand_dict_key2' => $rand_dict_key2,
            'rand_dict_key3' => $rand_dict_key3,
            'rand_dict_keys_multi' => $rand_dict_keys_multi,
            'rand_dict_val' => $rand_dict_val,
            'rand_dict_subset' => $rand_dict_subset,

            'rand_set_item1' => $rand_set_item1,
            'rand_set_item2' => $rand_set_item2,
            'rand_set_items_multi' => $rand_set_items_multi,
        ]);
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Dict(dict) = res {
        let d = dict.borrow();

        // 1. List checks: randomKey returns integer index 0..3
        for key in &["rand_key1", "rand_key2", "rand_key3"] {
            if let Some(PhpValue::Int(idx)) = d.get(*key) {
                assert!(*idx >= 0 && *idx <= 3, "index out of range: {}", idx);
            } else {
                panic!("expected int index for {}", key);
            }
        }

        // Multiple keys from list: returns list of 2 distinct indices
        if let PhpValue::List(indices) = d.get("rand_keys_multi").unwrap() {
            let b = indices.borrow();
            assert_eq!(b.len(), 2);
            assert_ne!(b[0], b[1]);
        } else {
            panic!("expected list for rand_keys_multi");
        }

        // random() returns a valid element from list
        let valid_l = [
            PhpValue::string("apple"),
            PhpValue::string("banana"),
            PhpValue::string("cherry"),
            PhpValue::string("date"),
        ];
        assert!(valid_l.contains(d.get("rand_val").unwrap()));

        if let PhpValue::List(vals) = d.get("rand_vals").unwrap() {
            let b = vals.borrow();
            assert_eq!(b.len(), 2);
            assert_ne!(b[0], b[1]);
        } else {
            panic!("expected list for rand_vals");
        }

        // 2. Dict checks: randomKey returns string key from 'x', 'y', 'z'
        let valid_keys = [
            PhpValue::string("x"),
            PhpValue::string("y"),
            PhpValue::string("z"),
        ];
        for key in &["rand_dict_key1", "rand_dict_key2", "rand_dict_key3"] {
            assert!(valid_keys.contains(d.get(*key).unwrap()));
        }

        if let PhpValue::List(keys) = d.get("rand_dict_keys_multi").unwrap() {
            let b = keys.borrow();
            assert_eq!(b.len(), 2);
            assert_ne!(b[0], b[1]);
            assert!(valid_keys.contains(&b[0]));
            assert!(valid_keys.contains(&b[1]));
        } else {
            panic!("expected list for rand_dict_keys_multi");
        }

        // random() on dict returns a value (10, 20, or 30)
        let valid_vals = [PhpValue::Int(10), PhpValue::Int(20), PhpValue::Int(30)];
        assert!(valid_vals.contains(d.get("rand_dict_val").unwrap()));

        if let PhpValue::Dict(sub_d) = d.get("rand_dict_subset").unwrap() {
            assert_eq!(sub_d.borrow().len(), 2);
        } else {
            panic!("expected dict for rand_dict_subset");
        }

        // 3. Set checks
        let valid_set_items = [
            PhpValue::string("alpha"),
            PhpValue::string("beta"),
            PhpValue::string("gamma"),
            PhpValue::string("delta"),
        ];
        assert!(valid_set_items.contains(d.get("rand_set_item1").unwrap()));
        assert!(valid_set_items.contains(d.get("rand_set_item2").unwrap()));

        if let PhpValue::Set(sub_s) = d.get("rand_set_items_multi").unwrap() {
            assert_eq!(sub_s.borrow().len(), 2);
        } else {
            panic!("expected set for rand_set_items_multi");
        }
    } else {
        panic!("expected result dict");
    }
}

#[test]
fn test_natsort_and_natcasesort_on_list_dict_set() {
    let code = r#"
        // 1. List natural order sorting
        $files = Arr::list(['img10.png', 'img1.png', 'img2.png', 'img20.png']);
        $nat_fluent = $files->natsort();
        $nat_alias = $files->sortNatural();
        $nat_proc = natsort($files);
        $nat_facade = Arr::natsort($files);
        $nat_facade_alias = Arr::sortNatural($files);

        // Case-insensitive natural order sorting
        $files_case = Arr::list(['IMG10.png', 'img1.png', 'img2.png', 'img20.png']);
        $natcase_fluent = $files_case->natcasesort();
        $natcase_alias = $files_case->sortNaturalCase();
        $natcase_proc = natcasesort($files_case);
        $natcase_facade = Arr::natcasesort($files_case);
        $natcase_facade_alias = Arr::sortNaturalCase($files_case);

        // 2. Dict natural order sorting (sorts by value)
        $d = Arr::dict(['x' => 'img10.png', 'y' => 'img1.png', 'z' => 'img2.png']);
        $d_nat_fluent = $d->natsort();
        $d_nat_proc = natsort($d);
        $d_nat_facade = Arr::natsort($d);

        $d_case = Arr::dict(['x' => 'IMG10.png', 'y' => 'img1.png', 'z' => 'img2.png']);
        $d_natcase_fluent = $d_case->natcasesort();
        $d_natcase_proc = natcasesort($d_case);
        $d_natcase_facade = Arr::natcasesort($d_case);

        // 3. Set natural sorting (returns new immutable Set)
        $s = Arr::set(['img10.png', 'img1.png', 'img2.png']);
        $s_nat_fluent = $s->natsort();
        $s_nat_proc = natsort($s);
        $s_nat_facade = Arr::natsort($s);
        $s_natcase_fluent = $s->natcasesort();

        // 4. strnatcmp and strnatcasecmp
        $cmp1 = strnatcmp('img2.png', 'img10.png');
        $cmp2 = strnatcmp('img10.png', 'img2.png');
        $cmp3 = strnatcmp('img1.png', 'img1.png');
        $cmp_case = strnatcasecmp('IMG10.png', 'img2.png');
        $cmp_facade1 = Str::natCompare('img2.png', 'img10.png');
        $cmp_facade2 = Str::natCaseCompare('IMG10.png', 'img10.png');

        return Arr::dict([
            'nat_fluent' => $nat_fluent,
            'nat_alias' => $nat_alias,
            'nat_proc' => $nat_proc,
            'nat_facade' => $nat_facade,
            'nat_facade_alias' => $nat_facade_alias,

            'natcase_fluent' => $natcase_fluent,
            'natcase_alias' => $natcase_alias,
            'natcase_proc' => $natcase_proc,
            'natcase_facade' => $natcase_facade,
            'natcase_facade_alias' => $natcase_facade_alias,

            'd_nat_fluent' => $d_nat_fluent,
            'd_nat_proc' => $d_nat_proc,
            'd_nat_facade' => $d_nat_facade,

            'd_natcase_fluent' => $d_natcase_fluent,
            'd_natcase_proc' => $d_natcase_proc,
            'd_natcase_facade' => $d_natcase_facade,

            's_nat_fluent' => $s_nat_fluent,
            's_nat_proc' => $s_nat_proc,
            's_nat_facade' => $s_nat_facade,
            's_natcase_fluent' => $s_natcase_fluent,

            'cmp1' => $cmp1,
            'cmp2' => $cmp2,
            'cmp3' => $cmp3,
            'cmp_case' => $cmp_case,
            'cmp_facade1' => $cmp_facade1,
            'cmp_facade2' => $cmp_facade2,
        ]);
    "#;

    let res = run_source(code).expect("evaluation failed");
    if let PhpValue::Dict(dict) = res {
        let d = dict.borrow();

        let expected_nat = vec![
            PhpValue::string("img1.png"),
            PhpValue::string("img2.png"),
            PhpValue::string("img10.png"),
            PhpValue::string("img20.png"),
        ];

        for key in &["nat_fluent", "nat_alias", "nat_proc", "nat_facade", "nat_facade_alias"] {
            if let PhpValue::List(l) = d.get(*key).unwrap() {
                assert_eq!(l.borrow().as_slice(), expected_nat.as_slice(), "Mismatch on key {}", key);
            } else {
                panic!("expected list for {}", key);
            }
        }

        let expected_natcase = vec![
            PhpValue::string("img1.png"),
            PhpValue::string("img2.png"),
            PhpValue::string("IMG10.png"),
            PhpValue::string("img20.png"),
        ];

        for key in &["natcase_fluent", "natcase_alias", "natcase_proc", "natcase_facade", "natcase_facade_alias"] {
            if let PhpValue::List(l) = d.get(*key).unwrap() {
                assert_eq!(l.borrow().as_slice(), expected_natcase.as_slice(), "Mismatch on key {}", key);
            } else {
                panic!("expected list for {}", key);
            }
        }

        // Dict natural order checks: entries sorted by value
        for key in &["d_nat_fluent", "d_nat_proc", "d_nat_facade"] {
            if let PhpValue::Dict(sub_d) = d.get(*key).unwrap() {
                let entries: Vec<(String, PhpValue)> = sub_d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                assert_eq!(entries, vec![
                    ("y".to_string(), PhpValue::string("img1.png")),
                    ("z".to_string(), PhpValue::string("img2.png")),
                    ("x".to_string(), PhpValue::string("img10.png")),
                ], "Mismatch on dict key {}", key);
            } else {
                panic!("expected dict for {}", key);
            }
        }

        for key in &["d_natcase_fluent", "d_natcase_proc", "d_natcase_facade"] {
            if let PhpValue::Dict(sub_d) = d.get(*key).unwrap() {
                let entries: Vec<(String, PhpValue)> = sub_d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                assert_eq!(entries, vec![
                    ("y".to_string(), PhpValue::string("img1.png")),
                    ("z".to_string(), PhpValue::string("img2.png")),
                    ("x".to_string(), PhpValue::string("IMG10.png")),
                ], "Mismatch on dict case key {}", key);
            } else {
                panic!("expected dict for {}", key);
            }
        }

        // Set natural order checks: returns Set with same elements
        for key in &["s_nat_fluent", "s_nat_proc", "s_nat_facade", "s_natcase_fluent"] {
            if let PhpValue::Set(s) = d.get(*key).unwrap() {
                let b = s.borrow();
                assert_eq!(b.len(), 3);
                assert!(b.contains(&PhpValue::string("img10.png")));
                assert!(b.contains(&PhpValue::string("img1.png")));
                assert!(b.contains(&PhpValue::string("img2.png")));
            } else {
                panic!("expected set for {}", key);
            }
        }

        // strnatcmp / strnatcasecmp checks
        assert_eq!(d.get("cmp1"), Some(&PhpValue::Int(-1)));
        assert_eq!(d.get("cmp2"), Some(&PhpValue::Int(1)));
        assert_eq!(d.get("cmp3"), Some(&PhpValue::Int(0)));
        assert_eq!(d.get("cmp_case"), Some(&PhpValue::Int(1)));
        assert_eq!(d.get("cmp_facade1"), Some(&PhpValue::Int(-1)));
        assert_eq!(d.get("cmp_facade2"), Some(&PhpValue::Int(0)));
    } else {
        panic!("expected result dict");
    }
}


