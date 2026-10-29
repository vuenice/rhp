use rhp::{run_source, PhpValue, ExecutionError, RuntimeErrorKind};

#[test]
fn test_enum_from_string_backed() {
    let source = r#"
        class UserRequest {
            public function __construct(public string $status) {}
        }

        enum SiteStatus: string {
            case Draft = 'draft';
            case Published = 'published';
            case Archived = 'archived';
        }

        $request = new UserRequest('published');
        $status = SiteStatus::from($request->status);

        $isPublished = ($status === SiteStatus::Published);
        $name = $status->name;
        $val = $status->value;

        return [$status, $isPublished, $name, $val];
    "#;

    let result = run_source(source).expect("Enum::from execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = result {
        let b = arr.borrow();
        assert_eq!(b[1], PhpValue::Bool(true));
        assert_eq!(b[2], PhpValue::string("Published"));
        assert_eq!(b[3], PhpValue::string("published"));
    } else {
        panic!("Expected array/list result, got {:?}", result);
    }
}

#[test]
fn test_enum_cases_unit_enum() {
    let source = r#"
        enum PlanType {
            case Free;
            case Pro;
            case Enterprise;
        }

        $cases = PlanType::cases();
        $count = count($cases);

        $names = [];
        foreach ($cases as $c) {
            $names[] = $c->name;
        }

        $isFirstFree = ($cases[0] === PlanType::Free);
        $isSecondPro = ($cases[1] === PlanType::Pro);

        return [$count, $names, $isFirstFree, $isSecondPro];
    "#;

    let result = run_source(source).expect("PlanType::cases execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = result {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(3));
        assert_eq!(
            b[1],
            PhpValue::list(vec![
                PhpValue::string("Free"),
                PhpValue::string("Pro"),
                PhpValue::string("Enterprise"),
            ])
        );
        assert_eq!(b[2], PhpValue::Bool(true));
        assert_eq!(b[3], PhpValue::Bool(true));
    } else {
        panic!("Expected array/list result, got {:?}", result);
    }
}

#[test]
fn test_enum_cases_backed_enum() {
    let source = r#"
        enum SiteStatus: string {
            case Draft = 'draft';
            case Published = 'published';
        }

        $cases = SiteStatus::cases();
        $first = $cases[0];
        $second = $cases[1];

        return [count($cases), $first->name, $first->value, $second->name, $second->value];
    "#;

    let result = run_source(source).expect("SiteStatus::cases execution should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(2),
            PhpValue::string("Draft"),
            PhpValue::string("draft"),
            PhpValue::string("Published"),
            PhpValue::string("published"),
        ])
    );
}

#[test]
fn test_enum_try_from() {
    let source = r#"
        enum SiteStatus: string {
            case Draft = 'draft';
            case Published = 'published';
        }

        $valid = SiteStatus::tryFrom('published');
        $invalid = SiteStatus::tryFrom('deleted');
        $nullVal = SiteStatus::tryFrom(null);

        $isValidPublished = ($valid === SiteStatus::Published);
        $isInvalidNull = ($invalid === null);
        $isNullValNull = ($nullVal === null);

        return [$isValidPublished, $isInvalidNull, $isNullValNull];
    "#;

    let result = run_source(source).expect("SiteStatus::tryFrom execution should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_enum_from_value_error_caught() {
    let source = r#"
        enum SiteStatus: string {
            case Draft = 'draft';
            case Published = 'published';
        }

        $caught = 'none';
        try {
            SiteStatus::from('invalid_value');
        } catch (ValueError $e) {
            $caught = $e->message;
        }

        return $caught;
    "#;

    let result = run_source(source).expect("ValueError catch execution should succeed");
    assert_eq!(
        result,
        PhpValue::string("\"invalid_value\" is not a valid backing value for enum SiteStatus")
    );
}

#[test]
fn test_enum_from_int_backed() {
    let source = r#"
        enum Priority: int {
            case Low = 1;
            case Medium = 2;
            case High = 3;
        }

        $p = Priority::from(2);
        $tryValid = Priority::tryFrom(3);
        $tryInvalid = Priority::tryFrom(99);

        $caught = 'none';
        try {
            Priority::from(100);
        } catch (ValueError $e) {
            $caught = $e->message;
        }

        return [
            $p === Priority::Medium,
            $p->name,
            $p->value,
            $tryValid === Priority::High,
            $tryInvalid === null,
            $caught
        ];
    "#;

    let result = run_source(source).expect("Int-backed enum execution should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::string("Medium"),
            PhpValue::Int(2),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::string("100 is not a valid backing value for enum Priority"),
        ])
    );
}

#[test]
fn test_pure_enum_from_throws_undefined_method() {
    let source = r#"
        enum PlanType {
            case Free;
            case Pro;
        }

        PlanType::from('Free');
    "#;

    let err = run_source(source).expect_err("Calling from() on non-backed enum should fail");
    match err {
        ExecutionError::Runtime(rt) => match rt.kind {
            RuntimeErrorKind::UndefinedMethod { class, method } => {
                assert_eq!(class, "PlanType");
                assert_eq!(method, "from");
            }
            other => panic!("Expected UndefinedMethod, got {:?}", other),
        },
        other => panic!("Expected RuntimeError, got {:?}", other),
    }
}

#[test]
fn test_enum_subtyping_and_instanceof() {
    let source = r#"
        enum SiteStatus: string {
            case Draft = 'draft';
        }

        enum PlanType {
            case Free;
        }

        $s = SiteStatus::Draft;
        $p = PlanType::Free;

        $sIsBacked = ($s instanceof BackedEnum);
        $sIsUnit = ($s instanceof UnitEnum);
        $pIsBacked = ($p instanceof BackedEnum);
        $pIsUnit = ($p instanceof UnitEnum);

        return [$sIsBacked, $sIsUnit, $pIsBacked, $pIsUnit];
    "#;

    let result = run_source(source).expect("instanceof execution should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_enum_methods_dynamic_class_variable() {
    let source = r#"
        enum SiteStatus: string {
            case Draft = 'draft';
            case Published = 'published';
        }

        $className = 'SiteStatus';
        $cases = $className::cases();
        $status = $className::from('draft');
        $maybe = $className::tryFrom('unknown');

        return [count($cases), $status->name, $maybe === null];
    "#;

    let result = run_source(source).expect("Dynamic class variable enum call should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(2),
            PhpValue::string("Draft"),
            PhpValue::Bool(true),
        ])
    );
}
