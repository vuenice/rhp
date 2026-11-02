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
