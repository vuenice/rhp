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
