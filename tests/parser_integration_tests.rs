use rhp::ast::{
    expr::{AssignOp, BinaryOp, ExprKind, Literal, StringPart, UnaryOp},
    stmt::{ClassMemberKind, StmtKind},
    types::{CastType, TypeHintKind, Visibility},
    AstVisitor,
};
use rhp::lexer::span::SourceMap;
use rhp::parser::{
    error::ParseErrorKind,
    parse, parse_expr, Parser,
};

#[test]
fn test_pratt_arithmetic_and_operator_precedence() {
    // 1 + 2 * 3 should parse as 1 + (2 * 3)
    let expr = parse_expr("1 + 2 * 3").expect("Failed to parse expression");
    match &expr.kind {
        ExprKind::Binary { op, left, right } => {
            assert_eq!(*op, BinaryOp::Add);
            assert!(matches!(left.kind, ExprKind::Literal(Literal::Int { value: 1, .. })));
            match &right.kind {
                ExprKind::Binary { op: mul_op, left: m_left, right: m_right } => {
                    assert_eq!(*mul_op, BinaryOp::Mul);
                    assert!(matches!(m_left.kind, ExprKind::Literal(Literal::Int { value: 2, .. })));
                    assert!(matches!(m_right.kind, ExprKind::Literal(Literal::Int { value: 3, .. })));
                }
                _ => panic!("Expected binary multiplication on right"),
            }
        }
        _ => panic!("Expected binary addition at root"),
    }

    // (1 + 2) * 3 should parse as (1 + 2) * 3
    let grouped = parse_expr("(1 + 2) * 3").expect("Failed to parse grouped expr");
    match &grouped.kind {
        ExprKind::Binary { op, left, right } => {
            assert_eq!(*op, BinaryOp::Mul);
            match &left.kind {
                ExprKind::Grouping(inner) => match &inner.kind {
                    ExprKind::Binary { op: add_op, .. } => assert_eq!(*add_op, BinaryOp::Add),
                    _ => panic!("Expected addition inside grouping"),
                },
                _ => panic!("Expected grouping on left"),
            }
            assert!(matches!(right.kind, ExprKind::Literal(Literal::Int { value: 3, .. })));
        }
        _ => panic!("Expected multiplication at root"),
    }
}

#[test]
fn test_pratt_right_associativity_power_and_assignment() {
    // 2 ** 3 ** 4 must be right-associative: 2 ** (3 ** 4)
    let pow_expr = parse_expr("2 ** 3 ** 4").expect("Failed to parse power expr");
    match &pow_expr.kind {
        ExprKind::Binary { op, left, right } => {
            assert_eq!(*op, BinaryOp::Pow);
            assert!(matches!(left.kind, ExprKind::Literal(Literal::Int { value: 2, .. })));
            match &right.kind {
                ExprKind::Binary { op: r_op, left: r_l, right: r_r } => {
                    assert_eq!(*r_op, BinaryOp::Pow);
                    assert!(matches!(r_l.kind, ExprKind::Literal(Literal::Int { value: 3, .. })));
                    assert!(matches!(r_r.kind, ExprKind::Literal(Literal::Int { value: 4, .. })));
                }
                _ => panic!("Expected right side to be 3 ** 4"),
            }
        }
        _ => panic!("Expected power at root"),
    }

    // $a = $b = 10 must be right-associative: $a = ($b = 10)
    let assign_expr = parse_expr("$a = $b = 10").expect("Failed to parse chained assign");
    match &assign_expr.kind {
        ExprKind::Assign { op, target, value } => {
            assert_eq!(*op, AssignOp::Assign);
            assert!(matches!(target.kind, ExprKind::Variable(ref v) if v.name == "$a"));
            match &value.kind {
                ExprKind::Assign { op: r_op, target: r_t, value: r_v } => {
                    assert_eq!(*r_op, AssignOp::Assign);
                    assert!(matches!(r_t.kind, ExprKind::Variable(ref v) if v.name == "$b"));
                    assert!(matches!(r_v.kind, ExprKind::Literal(Literal::Int { value: 10, .. })));
                }
                _ => panic!("Expected right side to be $b = 10"),
            }
        }
        _ => panic!("Expected assignment at root"),
    }

    // Null coalescing: $a ?? $b ?? $default must be right-associative: $a ?? ($b ?? $default)
    let coalesce_expr = parse_expr("$a ?? $b ?? $default").expect("Failed to parse coalesce");
    match &coalesce_expr.kind {
        ExprKind::Binary { op, left, right } => {
            assert_eq!(*op, BinaryOp::Coalesce);
            assert!(matches!(left.kind, ExprKind::Variable(ref v) if v.name == "$a"));
            assert!(matches!(right.kind, ExprKind::Binary { op: BinaryOp::Coalesce, .. }));
        }
        _ => panic!("Expected coalesce at root"),
    }
}

#[test]
fn test_pratt_postfix_and_prefix_unary_operators() {
    // $x++ * 2
    let expr = parse_expr("$x++ * 2").expect("Failed to parse postfix expr");
    match &expr.kind {
        ExprKind::Binary { op, left, right } => {
            assert_eq!(*op, BinaryOp::Mul);
            match &left.kind {
                ExprKind::Unary { op: u_op, expr: u_e, is_prefix } => {
                    assert_eq!(*u_op, UnaryOp::PostInc);
                    assert!(!is_prefix);
                    assert!(matches!(u_e.kind, ExprKind::Variable(ref v) if v.name == "$x"));
                }
                _ => panic!("Expected postfix unary increment"),
            }
            assert!(matches!(right.kind, ExprKind::Literal(Literal::Int { value: 2, .. })));
        }
        _ => panic!("Expected multiplication"),
    }

    // !$flag && $count > 0
    let logic_expr = parse_expr("!$flag && $count > 0").expect("Failed to parse logic expr");
    match &logic_expr.kind {
        ExprKind::Binary { op, left, right } => {
            assert_eq!(*op, BinaryOp::LogicalAnd);
            assert!(matches!(left.kind, ExprKind::Unary { op: UnaryOp::Not, is_prefix: true, .. }));
            assert!(matches!(right.kind, ExprKind::Binary { op: BinaryOp::Gt, .. }));
        }
        _ => panic!("Expected logical and"),
    }
}

#[test]
fn test_ternary_and_elvis_expressions() {
    // $is_active ? 'active' : 'inactive'
    let ternary = parse_expr("$is_active ? 'active' : 'inactive'").expect("Ternary parse");
    match &ternary.kind {
        ExprKind::Ternary { condition, if_true, if_false } => {
            assert!(matches!(condition.kind, ExprKind::Variable(_)));
            assert!(if_true.is_some());
            assert!(matches!(if_false.kind, ExprKind::Literal(Literal::String { .. })));
        }
        _ => panic!("Expected ternary expression"),
    }

    // $username ?: 'guest' (Elvis)
    let elvis = parse_expr("$username ?: 'guest'").expect("Elvis parse");
    match &elvis.kind {
        ExprKind::Ternary { condition, if_true, if_false } => {
            assert!(matches!(condition.kind, ExprKind::Variable(_)));
            assert!(if_true.is_none());
            assert!(matches!(if_false.kind, ExprKind::Literal(Literal::String { .. })));
        }
        _ => panic!("Expected elvis expression"),
    }
}

#[test]
fn test_spec_arr_facade_method_chaining() {
    let source = r#"
        $numbers = Arr::list([1, 2, 3, 4, 5])
            ->map(fn($n) => $n * 2)
            ->filter(fn($n) => $n > 4);
    "#;
    let program = parse(source).expect("Failed to parse Arr facade chaining");
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { op, target, value } => {
                assert_eq!(*op, AssignOp::Assign);
                assert!(matches!(target.kind, ExprKind::Variable(ref v) if v.name == "$numbers"));

                // Chained call structure: ->filter(...) on ->map(...) on Arr::list(...)
                match &value.kind {
                    ExprKind::MethodCall { object, method, args, is_nullsafe } => {
                        assert_eq!(method.name, "filter");
                        assert!(!is_nullsafe);
                        assert_eq!(args.len(), 1);
                        assert!(matches!(args[0].value.kind, ExprKind::ArrowFunction { .. }));

                        match &object.kind {
                            ExprKind::MethodCall { object: inner_obj, method: inner_method, args: inner_args, .. } => {
                                assert_eq!(inner_method.name, "map");
                                assert_eq!(inner_args.len(), 1);

                                match &inner_obj.kind {
                                    ExprKind::StaticCall { target: facade_target, method: facade_method, args: facade_args } => {
                                        assert!(matches!(facade_target.kind, ExprKind::Ident(ref id) if id.name == "Arr"));
                                        assert_eq!(facade_method.name, "list");
                                        assert_eq!(facade_args.len(), 1);
                                        match &facade_args[0].value.kind {
                                            ExprKind::Array(elements) => assert_eq!(elements.len(), 5),
                                            _ => panic!("Expected array literal in Arr::list"),
                                        }
                                    }
                                    _ => panic!("Expected StaticCall for Arr::list"),
                                }
                            }
                            _ => panic!("Expected MethodCall for ->map"),
                        }
                    }
                    _ => panic!("Expected outermost MethodCall for ->filter"),
                }
            }
            _ => panic!("Expected assign expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_spec_fluent_numeric_apis_and_facades() {
    let source = r#"
        $id = Int::i64(9223372036854775807);
        $price = Float::f64(99.99)->toDecimal(2);
        Int::i64()->random()->add()->subtract();
    "#;
    let program = parse(source).expect("Failed to parse numeric facades");
    assert_eq!(program.statements.len(), 3);

    // Stmt 1: Int::i64(...)
    match &program.statements[0].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { value, .. } => match &value.kind {
                ExprKind::StaticCall { target, method, args } => {
                    assert!(matches!(target.kind, ExprKind::Ident(ref id) if id.name == "Int"));
                    assert_eq!(method.name, "i64");
                    assert_eq!(args.len(), 1);
                    assert!(matches!(args[0].value.kind, ExprKind::Literal(Literal::Int { value: 9223372036854775807, .. })));
                }
                _ => panic!("Expected static call"),
            },
            _ => panic!("Expected assign"),
        },
        _ => panic!("Expected expr stmt"),
    }

    // Stmt 2: Float::f64(99.99)->toDecimal(2)
    match &program.statements[1].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { value, .. } => match &value.kind {
                ExprKind::MethodCall { object, method, args, .. } => {
                    assert_eq!(method.name, "toDecimal");
                    assert_eq!(args.len(), 1);
                    assert!(matches!(object.kind, ExprKind::StaticCall { .. }));
                }
                _ => panic!("Expected method call"),
            },
            _ => panic!("Expected assign"),
        },
        _ => panic!("Expected expr stmt"),
    }

    // Stmt 3: Int::i64()->random()->add()->subtract()
    match &program.statements[2].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::MethodCall { method, .. } => {
                assert_eq!(method.name, "subtract");
            }
            _ => panic!("Expected method call"),
        },
        _ => panic!("Expected expr stmt"),
    }
}

#[test]
fn test_spec_static_facade_instantiation_forbidden() {
    // Specification requirement: Instantiation of static facades via new is strictly forbidden
    let facades = ["Int", "Float", "Arr", "Str", "HTTP", "File", "Session", "Image", "URL", "Future", "Log", "DB", "Sys"];

    for facade in facades {
        let code = format!("$x = new {}();", facade);
        let result = parse(&code);
        assert!(result.is_err(), "Expected error instantiating facade {}", facade);
        let err = result.unwrap_err();
        match &err.kind {
            ParseErrorKind::StaticFacadeInstantiationForbidden { class_name } => {
                assert_eq!(class_name, facade);
            }
            other => panic!("Expected StaticFacadeInstantiationForbidden, got: {:?}", other),
        }
        let err_str = err.to_string();
        assert!(
            err_str.contains(&format!("Cannot instantiate static facade class '{}'. Use static methods directly.", facade)),
            "Error string '{}' must match required PDF message",
            err_str
        );
    }

    // Normal user classes must succeed
    assert!(parse("$x = new User('Alice');").is_ok());
    assert!(parse("$x = new DatabaseConnection();").is_ok());
}

#[test]
fn test_spec_async_concurrent_core_and_destructuring() {
    let source = r#"
        [$user, $posts] = Future::wait([
            fn() => DB::table('users')->find($id),
            fn() => HTTP::get("https://api.example.com/posts?user={$id}")->json()
        ]);

        $fastestResponse = Future::any([
            fn() => HTTP::get('https://primary-mirror.com/data')->json(),
            fn() => HTTP::get('https://secondary-mirror.com/data')->json()
        ]);

        $delayedResult = Future::delayed(500, fn() => 'Execution resumed after 500ms');

        Future::defer(fn() => Log::info('Request processed successfully'));
    "#;

    let program = parse(source).expect("Failed to parse async concurrent core");
    assert_eq!(program.statements.len(), 4);

    // Verify first statement: array destructuring assignment
    match &program.statements[0].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { target, value, .. } => {
                match &target.kind {
                    ExprKind::Array(elements) => {
                        assert_eq!(elements.len(), 2);
                        assert!(matches!(elements[0].value.kind, ExprKind::Variable(ref v) if v.name == "$user"));
                        assert!(matches!(elements[1].value.kind, ExprKind::Variable(ref v) if v.name == "$posts"));
                    }
                    _ => panic!("Expected array destructuring target"),
                }

                match &value.kind {
                    ExprKind::StaticCall { target: t, method: m, args: a } => {
                        assert!(matches!(t.kind, ExprKind::Ident(ref id) if id.name == "Future"));
                        assert_eq!(m.name, "wait");
                        assert_eq!(a.len(), 1);
                        // Check string interpolation inside second closure
                        match &a[0].value.kind {
                            ExprKind::Array(closures) => {
                                assert_eq!(closures.len(), 2);
                            }
                            _ => panic!("Expected array of tasks"),
                        }
                    }
                    _ => panic!("Expected Future::wait StaticCall"),
                }
            }
            _ => panic!("Expected assignment"),
        },
        _ => panic!("Expected expr stmt"),
    }
}

#[test]
fn test_spec_surface_level_gradual_typing_and_function_signatures() {
    let source = r#"
        function calculateTotal(int $quantity, float $unitPrice, string $currency): float {
            $subtotal = $quantity * $unitPrice;
            $tax = $subtotal * 0.18;
            return $subtotal + $tax;
        }
    "#;
    let program = parse(source).expect("Failed to parse function declaration");
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0].kind {
        StmtKind::FunctionDecl(f) => {
            assert_eq!(f.name.name, "calculateTotal");
            assert_eq!(f.params.len(), 3);

            // Parameter 1: int $quantity
            assert_eq!(f.params[0].name.name, "$quantity");
            assert_eq!(f.params[0].type_hint.as_ref().unwrap().simple_name(), Some("int"));

            // Parameter 2: float $unitPrice
            assert_eq!(f.params[1].name.name, "$unitPrice");
            assert_eq!(f.params[1].type_hint.as_ref().unwrap().simple_name(), Some("float"));

            // Parameter 3: string $currency
            assert_eq!(f.params[2].name.name, "$currency");
            assert_eq!(f.params[2].type_hint.as_ref().unwrap().simple_name(), Some("string"));

            // Return type: float
            assert_eq!(f.return_type.as_ref().unwrap().simple_name(), Some("float"));

            // Body
            let body = f.body.as_ref().unwrap();
            assert_eq!(body.len(), 3);
            assert!(matches!(body[2].kind, StmtKind::Return(_)));
        }
        _ => panic!("Expected FunctionDecl"),
    }
}

#[test]
fn test_typed_variable_declarations() {
    let source = r#"
        int $x = 10;
        float $y = 3.14;
        string $s = "hello";
        bool $flag;
    "#;
    let program = parse(source).expect("Failed to parse typed var decls");
    assert_eq!(program.statements.len(), 4);

    match &program.statements[0].kind {
        StmtKind::VarDecl { var, type_hint, init } => {
            assert_eq!(var.name, "$x");
            assert_eq!(type_hint.as_ref().unwrap().simple_name(), Some("int"));
            assert!(init.is_some());
        }
        _ => panic!("Expected VarDecl"),
    }

    match &program.statements[3].kind {
        StmtKind::VarDecl { var, type_hint, init } => {
            assert_eq!(var.name, "$flag");
            assert_eq!(type_hint.as_ref().unwrap().simple_name(), Some("bool"));
            assert!(init.is_none());
        }
        _ => panic!("Expected VarDecl without init"),
    }
}

#[test]
fn test_rejection_of_colon_typed_variable_declarations() {
    let bad_source = "$x: int = 10;";
    let err = parse(bad_source).expect_err("Colon type syntax must be rejected");
    assert!(err.to_string().contains("Colon type syntax '$x: Type' is not supported"));

    let bad_prop = "class Foo { public $x: int = 10; }";
    let err_prop = parse(bad_prop).expect_err("Colon type property must be rejected");
    assert!(err_prop.to_string().contains("Colon type syntax '$x: Type' is not supported"));

    let bad_param = "function bar($x: int) {}";
    let err_param = parse(bad_param).expect_err("Colon type parameter must be rejected");
    assert!(err_param.to_string().contains("Colon type syntax '$x: Type' is not supported"));
}

#[test]
fn test_advanced_type_hints_nullable_union_intersection() {
    let source = r#"
        function find(?int $id): ?User {}
        function process(int|float $val): string|bool {}
        function handle(Countable&ArrayAccess $items): void {}
    "#;
    let program = parse(source).expect("Failed to parse advanced type hints");
    assert_eq!(program.statements.len(), 3);

    // Nullable ?int and ?User
    if let StmtKind::FunctionDecl(ref f) = program.statements[0].kind {
        assert!(f.params[0].type_hint.as_ref().unwrap().is_nullable());
        assert!(f.return_type.as_ref().unwrap().is_nullable());
    } else {
        panic!("Expected FunctionDecl");
    }

    // Union int|float and string|bool
    if let StmtKind::FunctionDecl(ref f) = program.statements[1].kind {
        match &f.params[0].type_hint.as_ref().unwrap().kind {
            TypeHintKind::Union(types) => assert_eq!(types.len(), 2),
            _ => panic!("Expected Union type hint"),
        }
        match &f.return_type.as_ref().unwrap().kind {
            TypeHintKind::Union(types) => assert_eq!(types.len(), 2),
            _ => panic!("Expected Union return type"),
        }
    } else {
        panic!("Expected FunctionDecl");
    }

    // Intersection Countable&ArrayAccess
    if let StmtKind::FunctionDecl(ref f) = program.statements[2].kind {
        match &f.params[0].type_hint.as_ref().unwrap().kind {
            TypeHintKind::Intersection(types) => assert_eq!(types.len(), 2),
            _ => panic!("Expected Intersection type hint"),
        }
    } else {
        panic!("Expected FunctionDecl");
    }
}

#[test]
fn test_string_interpolation_parsing() {
    let source = r#"
        $url = "https://api.example.com/posts?user={$id}";
    "#;
    let program = parse(source).expect("Failed to parse interpolated string");
    match &program.statements[0].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { value, .. } => match &value.kind {
                ExprKind::Literal(Literal::InterpolatedString { parts, .. }) => {
                    assert_eq!(parts.len(), 2);
                    match &parts[0] {
                        StringPart::Literal(s) => assert_eq!(s, "https://api.example.com/posts?user="),
                        _ => panic!("Expected literal part"),
                    }
                    match &parts[1] {
                        StringPart::Expr(e) => match &e.kind {
                            ExprKind::Variable(v) => assert_eq!(v.name, "$id"),
                            _ => panic!("Expected variable expression"),
                        },
                        _ => panic!("Expected expression part"),
                    }
                }
                _ => panic!("Expected InterpolatedString"),
            },
            _ => panic!("Expected assign"),
        },
        _ => panic!("Expected expr stmt"),
    }
}

#[test]
fn test_control_flow_structures() {
    let source = r#"
        if ($score >= 90) {
            $grade = "A";
        } elseif ($score >= 80) {
            $grade = "B";
        } else {
            $grade = "F";
        }

        while ($running) {
            $count++;
            if ($count > 10) break;
        }

        do {
            $retry--;
        } while ($retry > 0);

        for ($i = 0; $i < 10; $i++) {
            if ($i == 5) continue;
        }

        foreach ($items as $key => $val) {
            echo $key, $val;
        }

        switch ($status) {
            case 200:
                $msg = "OK";
                break;
            default:
                $msg = "Unknown";
        }

        try {
            DB::query();
        } catch (DatabaseException $e) {
            Log::error($e);
        } finally {
            DB::close();
        }
    "#;

    let program = parse(source).expect("Failed to parse control flow statements");
    assert_eq!(program.statements.len(), 7);

    assert!(matches!(program.statements[0].kind, StmtKind::If { .. }));
    assert!(matches!(program.statements[1].kind, StmtKind::While { .. }));
    assert!(matches!(program.statements[2].kind, StmtKind::DoWhile { .. }));
    assert!(matches!(program.statements[3].kind, StmtKind::For { .. }));
    assert!(matches!(program.statements[4].kind, StmtKind::Foreach { .. }));
    assert!(matches!(program.statements[5].kind, StmtKind::Switch { .. }));
    assert!(matches!(program.statements[6].kind, StmtKind::TryCatch { .. }));
}

#[test]
fn test_object_oriented_classes_attributes_and_interfaces() {
    let source = r#"
        #[Route('/api/orders')]
        final class OrderService extends BaseService implements IOrderService {
            public const MAX_RETRIES = 3;
            private readonly string $apiKey;
            public static int $instances = 0;

            public function __construct(string $apiKey) {
                $this->apiKey = $apiKey;
                self::$instances++;
            }

            #[Authorize('admin')]
            public function process(int $orderId): bool {
                return true;
            }
        }
    "#;

    let program = parse(source).expect("Failed to parse class declaration");
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0].kind {
        StmtKind::ClassDecl(c) => {
            assert_eq!(c.name.name, "OrderService");
            assert!(c.modifiers.is_final);
            assert_eq!(c.attributes.len(), 1);
            assert_eq!(c.attributes[0].name.name, "Route");
            assert_eq!(c.extends.as_ref().unwrap().name, "BaseService");
            assert_eq!(c.implements.len(), 1);
            assert_eq!(c.implements[0].name, "IOrderService");
            assert_eq!(c.members.len(), 5);

            // Member 0: const MAX_RETRIES = 3
            assert!(matches!(c.members[0].kind, ClassMemberKind::Constant(_)));

            // Member 1: private readonly String $apiKey
            match &c.members[1].kind {
                ClassMemberKind::Property(p) => {
                    assert_eq!(p.visibility, Visibility::Private);
                    assert!(p.is_readonly);
                    assert_eq!(p.name.name, "$apiKey");
                }
                _ => panic!("Expected property"),
            }

            // Member 2: public static int $instances = 0
            match &c.members[2].kind {
                ClassMemberKind::Property(p) => {
                    assert_eq!(p.visibility, Visibility::Public);
                    assert!(p.is_static);
                    assert_eq!(p.name.name, "$instances");
                }
                _ => panic!("Expected property"),
            }

            // Member 3: __construct
            match &c.members[3].kind {
                ClassMemberKind::Method(m) => {
                    assert_eq!(m.function.name.name, "__construct");
                }
                _ => panic!("Expected constructor method"),
            }

            // Member 4: process with #[Authorize('admin')]
            match &c.members[4].kind {
                ClassMemberKind::Method(m) => {
                    assert_eq!(m.function.name.name, "process");
                    assert_eq!(m.attributes.len(), 1);
                    assert_eq!(m.attributes[0].name.name, "Authorize");
                }
                _ => panic!("Expected process method"),
            }
        }
        _ => panic!("Expected ClassDecl"),
    }
}

#[test]
fn test_match_expression() {
    let source = r#"
        $statusText = match ($code) {
            200 => 'OK',
            400, 401 => 'Client Error',
            500 => 'Server Error',
            default => 'Unknown Status',
        };
    "#;
    let program = parse(source).expect("Failed to parse match expr");
    assert_eq!(program.statements.len(), 1);

    match &program.statements[0].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { value, .. } => match &value.kind {
                ExprKind::Match { condition, arms } => {
                    assert!(matches!(condition.kind, ExprKind::Variable(_)));
                    assert_eq!(arms.len(), 4);
                    assert_eq!(arms[0].patterns.len(), 1);
                    assert_eq!(arms[1].patterns.len(), 2); // 400, 401
                    assert!(arms[3].is_default);
                }
                _ => panic!("Expected match expression"),
            },
            _ => panic!("Expected assignment"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_exact_source_span_tracking_and_diagnostics() {
    let source = "function test(): void {\n    $invalidVar = 42;\n}";
    let mut sm = SourceMap::new();
    let file_id = sm.add_file("test.rhp", source);

    let program = Parser::from_source_with_file_id(source, file_id)
        .expect("Parser init failed")
        .parse_program()
        .expect("Parsing failed");

    match &program.statements[0].kind {
        StmtKind::FunctionDecl(f) => {
            assert_eq!(f.span.file_id, file_id);
            assert_eq!(f.span.line, 1);

            let body = f.body.as_ref().unwrap();
            let assign_stmt = &body[0];
            assert_eq!(assign_stmt.span.line, 2);

            let diag = sm.format_diagnostic(&assign_stmt.span, "assignment in test function");
            assert!(diag.contains("--> test.rhp:2"));
            assert!(diag.contains("$invalidVar = 42;"));
        }
        _ => panic!("Expected FunctionDecl"),
    }
}

#[test]
fn test_ast_visitor_traversal() {
    let source = r#"
        function calculate(int $x, int $y): int {
            return $x + $y;
        }
        $total = calculate($a, $b) + 10;
        if ($total > 100) {
            logTotal($total);
        }
    "#;
    let program = parse(source).expect("Parsing failed");

    #[derive(Default)]
    struct CallCollector {
        calls: Vec<String>,
        static_calls: Vec<String>,
        variables: Vec<String>,
    }

    impl<'s> AstVisitor<'s> for CallCollector {
        fn visit_expr(&mut self, expr: &rhp::ast::Expr<'s>) {
            match &expr.kind {
                ExprKind::Call { callee, .. } => {
                    if let ExprKind::Ident(ref id) = callee.kind {
                        self.calls.push(id.name.to_string());
                    }
                }
                ExprKind::StaticCall { target, method, .. } => {
                    if let ExprKind::Ident(ref id) = target.kind {
                        self.static_calls.push(format!("{}::{}", id.name, method.name));
                    }
                }
                ExprKind::Variable(v) => {
                    self.variables.push(v.name.to_string());
                }
                _ => {}
            }
            rhp::ast::walk_expr(self, expr);
        }
    }

    let mut collector = CallCollector::default();
    collector.visit_program(&program);

    assert_eq!(collector.calls, vec!["calculate"]);
    assert_eq!(collector.static_calls, vec!["Glob::customHelper"]);
    assert!(collector.variables.contains(&"$total".to_string()));
    assert!(collector.variables.contains(&"$a".to_string()));
    assert!(collector.variables.contains(&"$b".to_string()));
}

#[test]
fn test_keyword_as_method_and_property_name_resolution() {
    let source = r#"
        $obj->match();
        $obj->default();
        $obj->from();
        $obj->for();
        $obj->class;
        $obj->new();
        $obj->static();
        $obj->clone();
        Glob::getClass($obj);
        Glob::match($pattern, $subject);
    "#;
    let program = parse(source).expect("Keywords after -> or :: must parse as identifiers");
    assert_eq!(program.statements.len(), 10);
}

#[test]
fn test_invalid_capitalized_and_legacy_type_aliases_rejected() {
    for invalid_type in ["String", "Boolean", "Integer", "boolean", "integer"] {
        let code = format!("function test({} $x): void {{}}", invalid_type);
        let err = parse(&code).expect_err(&format!("Expected type error for {}", invalid_type));
        assert!(
            err.to_string().contains("primitive types must be strictly lowercase"),
            "Unexpected error message: {}",
            err
        );
    }
}

#[test]
fn test_elseif_normalization_rejects_else_if() {
    let valid = "if ($a) { $x = 1; } elseif ($b) { $x = 2; } else { $x = 3; }";
    assert!(parse(valid).is_ok());

    let invalid = "if ($a) { $x = 1; } else if ($b) { $x = 2; }";
    let err = parse(invalid).expect_err("Multi-token 'else if' must be rejected in favor of 'elseif'");
    assert!(err.to_string().contains("elseif"));
}

#[test]
fn test_spec_static_facade_inheritance_forbidden() {
    // Traditional Class Extension (extends Arr, extends Str, etc.) is forbidden to avoid
    // tight coupling to Rust internals (Box<Vec<PhpValue>>) and stateless design violations.
    let facades = [
        "Int", "Float", "Arr", "Str", "HTTP", "File", "Session", "Image", "URL", "Future", "Log",
        "DB", "Sys", "Glob",
    ];

    for facade in facades {
        let code = format!("class Custom{0} extends {0} {{}}", facade);
        let result = parse(&code);
        assert!(
            result.is_err(),
            "Expected error extending static facade {}",
            facade
        );
        let err = result.unwrap_err();
        match &err.kind {
            ParseErrorKind::StaticFacadeInheritanceForbidden { class_name } => {
                assert_eq!(class_name, facade);
            }
            other => panic!(
                "Expected StaticFacadeInheritanceForbidden for {}, got: {:?}",
                facade, other
            ),
        }
        let err_str = err.to_string();
        assert!(
            err_str.contains(&format!(
                "Cannot extend static facade class '{0}'. Use '{0}::macro()' to register custom methods dynamically.",
                facade
            )),
            "Unexpected error string: {}",
            err_str
        );
    }

    // Extending normal userland classes remains valid
    assert!(parse("class AdminUser extends BaseUser {}").is_ok());
}

#[test]
fn test_spec_runtime_macro_registration_and_fluent_usage() {
    use rhp::{get_macro, has_macro};

    let source = r#"
        // Define a custom macro on the Arr facade
        Arr::macro('toCsv', function ($delimiter = ',') {
            // Custom logic accessing the underlying collection
            return implode($delimiter, $this->all());
        });

        // Usage anywhere in your application
        $csv = Arr::list([1, 2, 3])->toCsv();
    "#;

    let program = parse(source).expect("Failed to parse Arr::macro registration and fluent usage");
    assert_eq!(program.statements.len(), 2);

    // Verify statement 1: Arr::macro('toCsv', function ($delimiter = ',') { ... })
    match &program.statements[0].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::StaticCall {
                target,
                method,
                args,
            } => {
                assert!(matches!(&target.kind, ExprKind::Ident(id) if id.name == "Arr"));
                assert_eq!(method.name, "macro");
                assert_eq!(args.len(), 2);
                assert_eq!(
                    args[0].value.kind,
                    ExprKind::Literal(Literal::String {
                        raw: "'toCsv'",
                        value: "toCsv".to_string(),
                    })
                );
                match &args[1].value.kind {
                    ExprKind::Closure { params, body, .. } => {
                        assert_eq!(params.len(), 1);
                        assert_eq!(params[0].name.name, "$delimiter");
                        assert_eq!(
                            params[0].default_value.as_ref().map(|e| &e.kind),
                            Some(&ExprKind::Literal(Literal::String {
                                raw: "','",
                                value: ",".to_string(),
                            }))
                        );
                        assert_eq!(body.len(), 1);
                    }
                    other => panic!("Expected Closure in Arr::macro second argument, got {:?}", other),
                }
            }
            other => panic!("Expected StaticCall for Arr::macro, got {:?}", other),
        },
        other => panic!("Expected Expr statement, got {:?}", other),
    }

    // Verify statement 2: $csv = Arr::list([1, 2, 3])->toCsv();
    match &program.statements[1].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { target, value, .. } => {
                assert!(matches!(&target.kind, ExprKind::Variable(v) if v.name == "$csv"));
                match &value.kind {
                    ExprKind::MethodCall {
                        object,
                        method,
                        args,
                        ..
                    } => {
                        assert_eq!(method.name, "toCsv");
                        assert!(args.is_empty());
                        assert!(matches!(&object.kind, ExprKind::StaticCall { method: m, .. } if m.name == "list"));
                    }
                    other => panic!("Expected MethodCall ->toCsv(), got {:?}", other),
                }
            }
            other => panic!("Expected Assign, got {:?}", other),
        },
        other => panic!("Expected Expr statement, got {:?}", other),
    }

    // Verify the macro was automatically registered in the thread-safe global runtime macro registry
    assert!(has_macro("Arr", "toCsv"));
    let entry = get_macro("Arr", "toCsv").expect("Expected Arr::toCsv in global MacroRegistry");
    assert_eq!(entry.facade, "Arr");
    assert_eq!(entry.method_name, "toCsv");
    assert_eq!(entry.params.len(), 1);
    assert_eq!(entry.params[0].0, "$delimiter");

    // Verify invalid macro registrations fail at parse time
    let missing_handler = parse("Arr::macro('toCsv');");
    assert!(matches!(
        missing_handler.unwrap_err().kind,
        ParseErrorKind::InvalidFacadeMacroRegistration { .. }
    ));

    let empty_name = parse("Arr::macro('', fn() => 42);");
    assert!(matches!(
        empty_name.unwrap_err().kind,
        ParseErrorKind::InvalidFacadeMacroRegistration { .. }
    ));
}

#[test]
fn test_spec_global_helpers_reserved_intrinsics_and_custom_helper_lowering() {
    use rhp::{get_custom_helper, has_custom_helper, is_reserved_intrinsic, register_custom_helper};

    // 1. Verify reserved intrinsics metadata
    assert!(is_reserved_intrinsic("env"));
    assert!(is_reserved_intrinsic("print"));
    assert!(is_reserved_intrinsic("dd"));
    assert!(!is_reserved_intrinsic("my_custom_function"));

    let source = r#"
        function calculateTax(float $amount): float {
            return $amount * 0.18;
        }

        // Reserved intrinsics -> rewritten to Sys::env(), Sys::print(), Sys::dd()
        $appEnv = env('APP_ENV', 'production');
        print("Booting application...");
        dd($appEnv, 123);

        // Declared user function -> preserved as standard ExprKind::Call
        $tax = calculateTax(100.0);

        // Unrecognized global helper -> rewritten to Glob::customHelper('my_custom_function', $arg1, $arg2)
        $res = my_custom_function($appEnv, $tax);
    "#;

    let program = parse(source).expect("Failed to parse global helpers and custom helper fallback");
    assert_eq!(program.statements.len(), 6);

    // Statement 1: env('APP_ENV', 'production') -> Sys::env('APP_ENV', 'production')
    match &program.statements[1].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { value, .. } => match &value.kind {
                ExprKind::StaticCall { target, method, args } => {
                    assert!(matches!(&target.kind, ExprKind::Ident(id) if id.name == "Sys"));
                    assert_eq!(method.name, "env");
                    assert_eq!(args.len(), 2);
                    assert_eq!(
                        args[0].value.kind,
                        ExprKind::Literal(Literal::String {
                            raw: "'APP_ENV'",
                            value: "APP_ENV".to_string(),
                        })
                    );
                }
                other => panic!("Expected StaticCall Sys::env, got {:?}", other),
            },
            other => panic!("Expected Assign, got {:?}", other),
        },
        other => panic!("Expected Expr statement, got {:?}", other),
    }

    // Statement 2: print("Booting application...") -> Sys::print("Booting application...")
    match &program.statements[2].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::StaticCall { target, method, args } => {
                assert!(matches!(&target.kind, ExprKind::Ident(id) if id.name == "Sys"));
                assert_eq!(method.name, "print");
                assert_eq!(args.len(), 1);
            }
            other => panic!("Expected StaticCall Sys::print, got {:?}", other),
        },
        other => panic!("Expected Expr statement, got {:?}", other),
    }

    // Statement 3: dd($appEnv, 123) -> Sys::dd($appEnv, 123)
    match &program.statements[3].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::StaticCall { target, method, args } => {
                assert!(matches!(&target.kind, ExprKind::Ident(id) if id.name == "Sys"));
                assert_eq!(method.name, "dd");
                assert_eq!(args.len(), 2);
            }
            other => panic!("Expected StaticCall Sys::dd, got {:?}", other),
        },
        other => panic!("Expected Expr statement, got {:?}", other),
    }

    // Statement 4: calculateTax(100.0) -> standard user function call ExprKind::Call
    match &program.statements[4].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { value, .. } => match &value.kind {
                ExprKind::Call { callee, args } => {
                    assert!(matches!(&callee.kind, ExprKind::Ident(id) if id.name == "calculateTax"));
                    assert_eq!(args.len(), 1);
                }
                other => panic!("Expected standard user Call calculateTax, got {:?}", other),
            },
            other => panic!("Expected Assign, got {:?}", other),
        },
        other => panic!("Expected Expr statement, got {:?}", other),
    }

    // Statement 5: my_custom_function($appEnv, $tax) -> Glob::customHelper('my_custom_function', $appEnv, $tax)
    match &program.statements[5].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { value, .. } => match &value.kind {
                ExprKind::StaticCall { target, method, args } => {
                    assert!(matches!(&target.kind, ExprKind::Ident(id) if id.name == "Glob"));
                    assert_eq!(method.name, "customHelper");
                    assert_eq!(args.len(), 3);
                    assert_eq!(
                        args[0].value.kind,
                        ExprKind::Literal(Literal::String {
                            raw: "my_custom_function",
                            value: "my_custom_function".to_string(),
                        })
                    );
                    assert!(matches!(&args[1].value.kind, ExprKind::Variable(v) if v.name == "$appEnv"));
                    assert!(matches!(&args[2].value.kind, ExprKind::Variable(v) if v.name == "$tax"));
                }
                other => panic!("Expected StaticCall Glob::customHelper, got {:?}", other),
            },
            other => panic!("Expected Assign, got {:?}", other),
        },
        other => panic!("Expected Expr statement, got {:?}", other),
    }

    // Verify custom helper registration on Glob facade via Glob::macro or register_custom_helper
    register_custom_helper(
        "my_custom_function",
        vec![("$env".to_string(), None), ("$tax".to_string(), None)],
        "return $env . ':' . $tax;",
    )
    .expect("Failed to register custom helper on Glob facade");
    assert!(has_custom_helper("my_custom_function"));
    let helper_entry = get_custom_helper("my_custom_function").unwrap();
    assert_eq!(helper_entry.facade, "Glob");
    assert_eq!(helper_entry.method_name, "my_custom_function");
}

#[test]
fn test_spec_legacy_php_functions_phpfun_lowering_and_signature_map() {
    use rhp::{
        get_legacy_php_function, is_legacy_php_function, is_static_facade,
        php_function_signature_map,
    };

    // 1. Verify phpFun is a registered static facade and key-value signature map works
    assert!(is_static_facade("phpFun"));
    assert!(is_legacy_php_function("array_merge"));
    assert!(is_legacy_php_function("implode"));
    assert!(is_legacy_php_function("strlen"));
    assert!(is_legacy_php_function("file_get_contents"));
    assert!(!is_legacy_php_function("my_custom_function"));

    let merge_mapping = get_legacy_php_function("array_merge").expect("Expected array_merge mapping");
    assert_eq!(merge_mapping.php_name, "array_merge");
    assert_eq!(merge_mapping.rhp_facade, "Arr");
    assert_eq!(merge_mapping.rhp_method, "merge");
    assert_eq!(merge_mapping.rhp_signature, "Arr::merge(array ...$arrays): array");

    let sig_map = php_function_signature_map();
    assert_eq!(
        sig_map.get("array_merge"),
        Some(&"Arr::merge(array ...$arrays): array")
    );
    assert_eq!(
        sig_map.get("implode"),
        Some(&"Str::join(string $separator, array $array): string")
    );
    assert_eq!(
        sig_map.get("file_get_contents"),
        Some(&"File::read(string $path): string")
    );

    // 2. Verify Pratt parser rewrites legacy PHP functions to phpFun::<func_name>(args)
    let source = r#"
        $merged = array_merge($arr1, $arr2);
        $joined = implode(',', $merged);
        $len = strlen($joined);
        $direct = phpFun::array_merge($a, $b);
    "#;

    let program = parse(source).expect("Failed to parse legacy PHP function calls");
    assert_eq!(program.statements.len(), 4);

    // Statement 0: array_merge($arr1, $arr2) -> phpFun::array_merge($arr1, $arr2)
    match &program.statements[0].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { value, .. } => match &value.kind {
                ExprKind::StaticCall { target, method, args } => {
                    assert!(matches!(&target.kind, ExprKind::Ident(id) if id.name == "phpFun"));
                    assert_eq!(method.name, "array_merge");
                    assert_eq!(args.len(), 2);
                }
                other => panic!("Expected StaticCall phpFun::array_merge, got {:?}", other),
            },
            other => panic!("Expected Assign, got {:?}", other),
        },
        other => panic!("Expected Expr statement, got {:?}", other),
    }

    // Statement 1: implode(',', $merged) -> phpFun::implode(',', $merged)
    match &program.statements[1].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { value, .. } => match &value.kind {
                ExprKind::StaticCall { target, method, args } => {
                    assert!(matches!(&target.kind, ExprKind::Ident(id) if id.name == "phpFun"));
                    assert_eq!(method.name, "implode");
                    assert_eq!(args.len(), 2);
                }
                other => panic!("Expected StaticCall phpFun::implode, got {:?}", other),
            },
            other => panic!("Expected Assign, got {:?}", other),
        },
        other => panic!("Expected Expr statement, got {:?}", other),
    }

    // Statement 2: strlen($joined) -> phpFun::strlen($joined)
    match &program.statements[2].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { value, .. } => match &value.kind {
                ExprKind::StaticCall { target, method, args } => {
                    assert!(matches!(&target.kind, ExprKind::Ident(id) if id.name == "phpFun"));
                    assert_eq!(method.name, "strlen");
                    assert_eq!(args.len(), 1);
                }
                other => panic!("Expected StaticCall phpFun::strlen, got {:?}", other),
            },
            other => panic!("Expected Assign, got {:?}", other),
        },
        other => panic!("Expected Expr statement, got {:?}", other),
    }

    // Statement 3: explicit phpFun::array_merge($a, $b) also parses as StaticCall phpFun::array_merge
    match &program.statements[3].kind {
        StmtKind::Expr(expr) => match &expr.kind {
            ExprKind::Assign { value, .. } => match &value.kind {
                ExprKind::StaticCall { target, method, args } => {
                    assert!(matches!(&target.kind, ExprKind::Ident(id) if id.name == "phpFun"));
                    assert_eq!(method.name, "array_merge");
                    assert_eq!(args.len(), 2);
                }
                other => panic!("Expected StaticCall phpFun::array_merge, got {:?}", other),
            },
            other => panic!("Expected Assign, got {:?}", other),
        },
        other => panic!("Expected Expr statement, got {:?}", other),
    }
}

#[test]
fn test_functional_type_casting_syntax_and_rejection_of_c_style_casts() {
    // 1. Functional type casting parses cleanly: int($raw), string($raw), float($raw), bool($raw)
    let source = r#"
        $intVal = int($raw);
        $strVal = string($raw);
        $floatVal = float($raw);
        $boolVal = bool($raw);
    "#;
    let program = parse(source).expect("Functional type casting syntax must parse");
    assert_eq!(program.statements.len(), 4);

    for (stmt, expected_kind) in program.statements.iter().zip([
        CastType::Int,
        CastType::String,
        CastType::Float,
        CastType::Bool,
    ]) {
        match &stmt.kind {
            StmtKind::Expr(expr) => match &expr.kind {
                ExprKind::Assign { value, .. } => match &value.kind {
                    ExprKind::Cast { kind, .. } => assert_eq!(*kind, expected_kind),
                    other => panic!("Expected ExprKind::Cast, got {:?}", other),
                },
                other => panic!("Expected Assign, got {:?}", other),
            },
            other => panic!("Expected Expr stmt, got {:?}", other),
        }
    }

    // 2. PHP-style cast syntax (int)$x, (string)$x is rejected with informative error
    let bad_int = "$x = (int)$val;";
    let err_int = parse(bad_int).expect_err("(int)$val must be rejected");
    assert!(err_int.to_string().contains("Type cast syntax '(int)' is not supported"));

    let bad_str = "$x = (string)$val;";
    let err_str = parse(bad_str).expect_err("(string)$val must be rejected");
    assert!(err_str.to_string().contains("Type cast syntax '(string)' is not supported"));

    let bad_spaced_int = "$x = (int) $val;";
    let err_spaced_int = parse(bad_spaced_int).expect_err("(int) $val must be rejected");
    assert!(err_spaced_int.to_string().contains("Type cast syntax '(int)' is not supported"));
}

#[test]
fn test_scalar_and_collection_type_hint_casing_rules() {
    // 1. Valid function boundary signatures:
    // Strictly lowercase primitives: int, string, float, bool
    // Collections: list, dict, set, hashmap (both lowercase native and capitalized supported)
    let valid_source = r#"
        function processItems(int $id, string $name, list $items, dict $meta): list {
            list $scoped_list = [];
            dict $scoped_dict = [];
            return $items;
        }
    "#;
    let program = parse(valid_source).expect("Valid scalar and collection type hints must parse");
    if let StmtKind::FunctionDecl(ref f) = program.statements[0].kind {
        assert_eq!(f.params[0].type_hint.as_ref().unwrap().simple_name(), Some("int"));
        assert_eq!(f.params[1].type_hint.as_ref().unwrap().simple_name(), Some("string"));
        assert_eq!(f.params[2].type_hint.as_ref().unwrap().simple_name(), Some("list"));
        assert_eq!(f.params[3].type_hint.as_ref().unwrap().simple_name(), Some("dict"));
        assert_eq!(f.return_type.as_ref().unwrap().simple_name(), Some("list"));
    } else {
        panic!("Expected FunctionDecl");
    }

    // 2. Reject capitalized primitives
    for invalid_prim in ["String", "Integer", "Boolean", "Float"] {
        let code = format!("function bad({} $x): void {{}}", invalid_prim);
        let err = parse(&code).expect_err(&format!("Expected rejection for {}", invalid_prim));
        assert!(err.to_string().contains("primitive types must be strictly lowercase"));
    }

    // 3. Accept lowercase collections (list, dict, set, hashmap)
    for col in ["list", "dict", "set", "hashmap"] {
        let code = format!("function process({} $arr): void {{ {} $local = []; }}", col, col);
        let program = parse(&code).expect(&format!("Lowercase collection {} must parse", col));
        if let StmtKind::FunctionDecl(ref f) = program.statements[0].kind {
            assert_eq!(f.params[0].type_hint.as_ref().unwrap().simple_name(), Some(col));
        } else {
            panic!("Expected FunctionDecl");
        }
    }
}





