use rhp::run_source;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;

#[test]
fn test_dynamic_typing_untyped_variable_flexible_reassignment() {
    clear_runtime_closure_macros();

    // Reassigning untyped $arr from sequential list to dictionary
    let source = r#"
        $arr = [1, 2, 3];
        $initial = $arr;
        $arr = ['name' => 'yogesh'];
        return [$initial, $arr];
    "#;

    let res = run_source(source).expect("Flexible reassignment of untyped variable must succeed");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3)]),
            PhpValue::dict(vec![("name".to_string(), PhpValue::string("yogesh"))]),
        ])
    );
}

#[test]
fn test_array_type_hint_as_permissive_parent_variable() {
    clear_runtime_closure_macros();

    // Variable explicitly declared with permissive parent type `array`
    let source = r#"
        array $arr = [1, 2, 3];
        $first = $arr;
        $arr = ['name' => 'yogesh'];
        $second = $arr;
        $arr = Arr::list([10, 20]);
        $third = $arr;
        $arr = Arr::dict(['env' => 'production']);
        $fourth = $arr;
        return [$first, $second, $third, $fourth];
    "#;

    let res = run_source(source).expect("Permissive array variable must allow all collection variants");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3)]),
            PhpValue::dict(vec![("name".to_string(), PhpValue::string("yogesh"))]),
            PhpValue::list(vec![PhpValue::Int(10), PhpValue::Int(20)]),
            PhpValue::dict(vec![("env".to_string(), PhpValue::string("production"))]),
        ])
    );
}

#[test]
fn test_function_which_returns_array_type_hint() {
    clear_runtime_closure_macros();

    let source = r#"
        function myfuncWhichreturnsArray(): array {
            return [1, 2, 3];
        }

        function myfuncWhichreturnsArrayDict(): array {
            return ['name' => 'yogesh'];
        }

        function myfuncWhichreturnsArrayList(): array {
            return Arr::list([4, 5, 6]);
        }

        function myfuncWhichreturnsArrayCustomDict(): array {
            return Arr::dict(['role' => 'admin']);
        }

        return [
            myfuncWhichreturnsArray(),
            myfuncWhichreturnsArrayDict(),
            myfuncWhichreturnsArrayList(),
            myfuncWhichreturnsArrayCustomDict(),
        ];
    "#;

    let res = run_source(source).expect("Functions returning array must accept any collection variant");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3)]),
            PhpValue::dict(vec![("name".to_string(), PhpValue::string("yogesh"))]),
            PhpValue::list(vec![PhpValue::Int(4), PhpValue::Int(5), PhpValue::Int(6)]),
            PhpValue::dict(vec![("role".to_string(), PhpValue::string("admin"))]),
        ])
    );
}

#[test]
fn test_permissive_array_parameter_accepts_all_collection_variants() {
    clear_runtime_closure_macros();

    let source = r#"
        function getCount(array $items): int {
            return $items->count();
        }

        $c1 = getCount([1, 2, 3]);
        $c2 = getCount(['a' => 10, 'b' => 20]);
        $c3 = getCount(Arr::list([100, 200, 300, 400]));
        $c4 = getCount(Arr::dict(['x' => 1, 'y' => 2, 'z' => 3]));

        return [$c1, $c2, $c3, $c4];
    "#;

    let res = run_source(source).expect("Permissive array parameter must accept all collection subtypes");
    assert_eq!(
        res,
        PhpValue::list(vec![
            PhpValue::Int(3),
            PhpValue::Int(2),
            PhpValue::Int(4),
            PhpValue::Int(3),
        ])
    );
}

#[test]
fn test_strict_type_bounds_vs_dynamic_variables() {
    clear_runtime_closure_macros();

    // The written type decides the collection. A literal is only an array.
    let source_list_reassign_dict = r#"
        list $arr = [1, 2, 3];
        $arr = ['name' => 'yogesh'];
        return $arr;
    "#;
    let list_again = run_source(source_list_reassign_dict)
        .expect("a list variable stays a list");
    match list_again {
        PhpValue::List(items) => {
            assert_eq!(items.borrow().as_slice(), &[PhpValue::string("yogesh")]);
        }
        other => panic!("expected list, got {other:?}"),
    }

    let source_dict_reassign_list = r#"
        dict $arr = ['name' => 'yogesh'];
        $arr = [1, 2, 3];
        return $arr;
    "#;
    let dict_again = run_source(source_dict_reassign_list)
        .expect("a dict variable stays a dict");
    match dict_again {
        PhpValue::Dict(entries) => {
            let entries = entries.borrow();
            assert_eq!(entries.get("0"), Some(&PhpValue::Int(1)));
            assert_eq!(entries.get("1"), Some(&PhpValue::Int(2)));
            assert_eq!(entries.get("2"), Some(&PhpValue::Int(3)));
        }
        other => panic!("expected dict, got {other:?}"),
    }

    // 3. Passing Dict to function requiring strict List fails
    let source_strict_list_param = r#"
        function requireStrictList(list $items): int {
            return $items->count();
        }

        requireStrictList(['name' => 'yogesh']);
    "#;
    let err3 = run_source(source_strict_list_param)
        .expect_err("Passing Dict to strict List parameter must fail");
    let err3_text = format!("{:?}", err3);
    assert!(
        err3_text.contains("ArgumentTypeMismatch")
            || err3_text.contains("TypeContractViolation")
            || err3.to_string().contains("Argument #1"),
        "Unexpected error: {:?}",
        err3
    );

    // 4. Strict scalar bounds (int $x cannot take string)
    let source_strict_scalar = r#"
        int $x = 0;
        $x = 'hello';
    "#;
    let err4 = run_source(source_strict_scalar)
        .expect_err("Assigning string to locked int must fail");
    assert!(
        format!("{:?}", err4).contains("VariableTypeMismatch")
            || err4.to_string().contains("Cannot assign value of type"),
        "Unexpected error: {:?}",
        err4
    );
}
