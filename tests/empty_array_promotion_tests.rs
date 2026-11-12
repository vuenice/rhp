use rhp::run_source;
use rhp::vm::value::PhpValue;

#[test]
fn empty_array_stays_unpromoted_until_the_first_write() {
    let code = r#"
        $items = [];
        $count = count($items);
        $is_empty = empty($items);
        $reads = 0;
        foreach ($items as $item) {
            $reads = $reads + 1;
        }
        $head = $items[0];
        $by_name = $items['name'];
        return [$items, $count, $is_empty, $reads, $head, $by_name];
    "#;

    let res = run_source(code).expect("empty array reads");
    let (PhpValue::Array(items) | PhpValue::List(items)) = res else {
        panic!("expected a list of results, got {res:?}");
    };
    let items = items.borrow();
    assert!(matches!(items[0], PhpValue::EmptyArray), "got {:?}", items[0]);
    assert_eq!(items[1], PhpValue::Int(0));
    assert_eq!(items[2], PhpValue::Bool(true));
    assert_eq!(items[3], PhpValue::Int(0));
    assert_eq!(items[4], PhpValue::Null);
    assert_eq!(items[5], PhpValue::Null);
}

#[test]
fn first_write_promotes_empty_array() {
    let code = r#"
        $by_push = [];
        $by_push[] = 7;

        $by_zero = [];
        $by_zero[0] = 'a';

        $by_name = [];
        $by_name['name'] = 'Ada';

        $by_gap = [];
        $by_gap[999] = 1;

        $kept = [];
        $copy = $kept;
        $copy[] = 1;

        return [$by_push, $by_zero, $by_name, $by_gap, $kept, $copy];
    "#;

    let res = run_source(code).expect("promotion");
    let (PhpValue::Array(items) | PhpValue::List(items)) = res else {
        panic!("expected a list of results, got {res:?}");
    };
    let items = items.borrow();

    match &items[0] {
        PhpValue::List(list) => assert_eq!(list.borrow().as_slice(), &[PhpValue::Int(7)]),
        other => panic!("append should be a list, got {other:?}"),
    }
    match &items[1] {
        PhpValue::List(list) => assert_eq!(list.borrow().as_slice(), &[PhpValue::string("a")]),
        other => panic!("index 0 should be a list, got {other:?}"),
    }
    match &items[2] {
        PhpValue::Dict(dict) => {
            assert_eq!(dict.borrow().get("name"), Some(&PhpValue::string("Ada")));
        }
        other => panic!("string key should be a dict, got {other:?}"),
    }
    match &items[3] {
        PhpValue::HashMap(map) => {
            assert_eq!(map.borrow().get(&PhpValue::Int(999)), Some(&PhpValue::Int(1)));
        }
        other => panic!("gapped key should be a hash map, got {other:?}"),
    }
    assert!(matches!(items[4], PhpValue::EmptyArray), "original stays empty");
    match &items[5] {
        PhpValue::List(list) => assert_eq!(list.borrow().as_slice(), &[PhpValue::Int(1)]),
        other => panic!("copy should promote on its own, got {other:?}"),
    }
}

#[test]
fn shared_list_and_cycle_keep_one_buffer() {
    let code = r#"
        $a = [1];
        $b = $a;
        $b[] = 2;

        $cycle = [1];
        $cycle[] = $cycle;
        $same = $cycle == $cycle;
        $cycle[0] = 9;
        return [$a[1], $same, $cycle[0], $cycle[1][0]];
    "#;
    let res = run_source(code).expect("shared list");
    let PhpValue::List(items) = res else {
        panic!("expected a list, got {res:?}");
    };
    let items = items.borrow();
    assert_eq!(items[0], PhpValue::Int(2));
    assert_eq!(items[1], PhpValue::Bool(true));
    assert_eq!(items[2], PhpValue::Int(9));
    assert_eq!(items[3], PhpValue::Int(9));
}

#[test]
fn nested_list_holds_a_list_and_a_dict() {
    let code = r#"
        $rows = [[1, 2], ['name' => 'Ada']];
        return [$rows[0][1], $rows[1]['name']];
    "#;
    let res = run_source(code).expect("nested list");
    assert_eq!(
        res,
        PhpValue::list(vec![PhpValue::Int(2), PhpValue::string("Ada")])
    );
}

#[test]
fn list_index_past_the_end_is_an_error() {
    let code = r#"
        list $a = [1];
        $a[2] = 9;
    "#;
    let err = run_source(code).expect_err("gapped list index");
    assert!(
        err.to_string().contains("List index must be in 0.."),
        "{err}"
    );
}

#[test]
fn remove_deletes_from_dict_and_hashmap() {
    let dict = run_source(
        r#"
        $dict = ['k' => 1, 'm' => 2];
        $dict->remove('k');
        return $dict;
    "#,
    )
    .expect("dict remove");
    match dict {
        PhpValue::Dict(map) => {
            assert!(map.borrow().get("k").is_none());
            assert_eq!(map.borrow().get("m"), Some(&PhpValue::Int(2)));
        }
        other => panic!("expected dict, got {other:?}"),
    }

    let map = run_source(
        r#"
        $map = [1 => 'a', 2 => 'b'];
        $map->remove(1);
        return $map;
    "#,
    )
    .expect("hashmap remove");
    match map {
        PhpValue::HashMap(map) => {
            assert!(map.borrow().get(&PhpValue::Int(1)).is_none());
            assert_eq!(map.borrow().get(&PhpValue::Int(2)), Some(&PhpValue::string("b")));
        }
        other => panic!("expected hashmap, got {other:?}"),
    }

    let list = run_source(
        r#"
        $items = [10, 20, 30];
        $items->remove(1);
        return $items;
    "#,
    )
    .expect("list remove");
    assert_eq!(
        list,
        PhpValue::list(vec![PhpValue::Int(10), PhpValue::Int(30)])
    );

    for code in [
        "list $items = [1]; unset($items[0]);",
        "$set = Arr::set([1]); unset($set[1]);",
        "$tuple = [1, 'a']; unset($tuple[0]);",
        "$dict = ['k' => 1]; unset($dict['k']);",
        "$a = []; unset($a[0]);",
    ] {
        let err = run_source(code).expect_err(code);
        assert!(
            err.to_string().contains("unset() cannot remove an element"),
            "{code}: {err}"
        );
    }
}

#[test]
fn remove_argument_follows_the_collection() {
    let issues = rhp::run_typecheck(
        r#"
        $list = [10, 20];
        $list->remove(0);
        $dict = ['k' => 1];
        $dict->remove('k');
        $set = Arr::set([1]);
        $set->remove(1);
        $map = [1 => 'a'];
        $map->remove(1);
    "#,
        "remove.rhp",
    );
    let warnings: Vec<_> = issues
        .iter()
        .filter(|issue| issue.level == rhp::IssueLevel::Warning)
        .map(|issue| issue.message.clone())
        .collect();
    assert_eq!(warnings.len(), 2, "{issues:?}");
    assert!(warnings.iter().all(|msg| msg.contains("slower than set and hashMap")));
    assert!(issues.iter().all(|issue| issue.level != rhp::IssueLevel::Error));

    let bad_list = run_source("$list = [10, 20]; $list->remove('no');").expect_err("list key");
    assert!(bad_list.to_string().contains("list->remove"), "{bad_list}");

    let bad_dict = run_source("$dict = ['k' => 1]; $dict->remove(1);").expect_err("dict key");
    assert!(bad_dict.to_string().contains("dict->remove"), "{bad_dict}");

    let bad_set = run_source("$set = Arr::set([1]); $set->remove('no');").expect_err("set value");
    assert!(bad_set.to_string().contains("set->remove"), "{bad_set}");

    let bad_tuple = run_source("$tuple = [1, 'a']; $tuple->remove(0);").expect_err("tuple");
    assert!(bad_tuple.to_string().contains("fixed length"), "{bad_tuple}");
}
