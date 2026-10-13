use rhp::{
    compile_source, compile_source_with_file_id, compile_to_bytecode, parse_expr, BinaryOp,
    CompilationError, Constant, IrCollectionKind, IrErrorKind, OpCode, Reg, RhpType,
    RustFloatKind, RustIntKind, SourceMap,
};

#[test]
fn test_flat_copy_opcode_and_standalone_expr_compilation() {
    // Verify OpCode is Copy and compact
    fn assert_copy<T: Copy>() {}
    assert_copy::<OpCode>();
    assert_copy::<Reg>();

    let expr = parse_expr("10 + 20 * 3").expect("Failed to parse expression");
    let chunk = compile_to_bytecode(&expr).expect("Failed to lower expression to bytecode");

    assert_eq!(chunk.instructions.len(), chunk.spans.len());
    assert_eq!(
        chunk.instructions,
        vec![
            OpCode::LoadInt {
                dst: Reg(0),
                value: 10
            },
            OpCode::LoadInt {
                dst: Reg(1),
                value: 20
            },
            OpCode::LoadInt {
                dst: Reg(2),
                value: 3
            },
            OpCode::Binary {
                dst: Reg(3),
                op: BinaryOp::Mul,
                lhs: Reg(1),
                rhs: Reg(2),
            },
            OpCode::Binary {
                dst: Reg(4),
                op: BinaryOp::Add,
                lhs: Reg(0),
                rhs: Reg(3),
            },
            OpCode::Return {
                value: Some(Reg(4))
            },
        ]
    );
}

#[test]
fn test_function_boundary_contracts_and_dynamic_body_register_allocation() {
    let source = r#"
        function calculateTotal(int $quantity, float $price, float $taxRate = 0.05): float {
            $subtotal = $quantity * $price;
            $tax = $subtotal * $taxRate;
            $total: float = $subtotal + $tax;
            return $total;
        }

        $ans = calculateTotal(4, 25.0);
    "#;

    let module = compile_source(source).expect("Compilation should succeed");

    let func = module
        .get_function_by_name("calculateTotal")
        .expect("calculateTotal must exist in module");

    // Verify formal parameter registers: r0 = $quantity, r1 = $price, r2 = $taxRate
    assert_eq!(func.params.len(), 3);
    assert_eq!(func.params[0].name, "$quantity");
    assert_eq!(func.params[0].reg, Reg(0));
    assert_eq!(func.params[0].rhp_type, RhpType::Int);

    assert_eq!(func.params[1].name, "$price");
    assert_eq!(func.params[1].reg, Reg(1));
    assert_eq!(func.params[1].rhp_type, RhpType::Float);

    assert_eq!(func.params[2].name, "$taxRate");
    assert_eq!(func.params[2].reg, Reg(2));
    assert_eq!(func.params[2].has_default, true);
    assert_eq!(func.return_type, RhpType::Float);

    // Verify dynamically inferred body variables got dedicated registers:
    let subtotal_reg = func
        .chunk
        .register_for_var("$subtotal")
        .expect("$subtotal register");
    let tax_reg = func.chunk.register_for_var("$tax").expect("$tax register");
    let total_reg = func
        .chunk
        .register_for_var("$total")
        .expect("$total register");

    assert_eq!(subtotal_reg, Reg(3));
    assert_eq!(tax_reg, Reg(4));
    assert_eq!(total_reg, Reg(5));

    // Verify inferred types carried over into RegisterInfo
    assert_eq!(
        func.chunk.register_info[subtotal_reg.index()].rhp_type,
        RhpType::Float
    );
    assert_eq!(
        func.chunk.register_info[total_reg.index()].is_locked_type,
        true
    );

    // Verify prologue emits JumpIfNotNull for default parameter $taxRate and AssertType for params + return
    let assert_count = func
        .chunk
        .instructions
        .iter()
        .filter(|op| matches!(op, OpCode::AssertType { .. }))
        .count();
    // 3 params + 1 locked local ($total: float) + 1 return boundary = 5 AssertType instructions
    assert_eq!(assert_count, 5);

    // Verify top-level call to `calculateTotal(4, 25.0)`
    assert!(module
        .main_chunk
        .instructions
        .iter()
        .any(|op| matches!(op, OpCode::CallFunction { arg_count: 2, .. })));
}

#[test]
fn test_subtyped_collections_and_fluent_arrow_closures() {
    let source = r#"
        $factor = 2;
        $numbers = Arr::list([1, 2, 3, 4, 5])
            ->map(fn($n) => $n * $factor)
            ->filter(fn($n) => $n > 4);

        $dict = Arr::dict(['a' => 1, 'b' => 2]);
        $set = Arr::set([10, 20, 30]);
        $map = Arr::hashMap(['k' => 'v']);
    "#;

    let module = compile_source(source).expect("Collection lowering should succeed");

    // Verify all 4 explicit collection subtypes lower to `OpCode::NewCollection`
    let collection_kinds: Vec<IrCollectionKind> = module
        .main_chunk
        .instructions
        .iter()
        .filter_map(|op| match op {
            OpCode::NewCollection { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect();

    assert_eq!(
        collection_kinds,
        vec![
            IrCollectionKind::List,
            IrCollectionKind::Dict,
            IrCollectionKind::Set,
            IrCollectionKind::HashMap,
        ]
    );

    // Verify the two arrow functions were compiled into `module.functions`
    assert_eq!(module.functions.len(), 2);
    let map_arrow = &module.functions[0];
    let filter_arrow = &module.functions[1];

    // First arrow function `fn($n) => $n * $factor` automatically captures `$factor` from outer scope!
    assert!(map_arrow.is_closure);
    assert_eq!(map_arrow.upvalues, vec!["$factor".to_string()]);

    // Second arrow function `fn($n) => $n > 4` has no outer captures
    assert!(filter_arrow.is_closure);
    assert!(filter_arrow.upvalues.is_empty());

    // Verify CaptureUpvalue and MakeClosure are emitted in main_chunk
    assert!(module
        .main_chunk
        .instructions
        .iter()
        .any(|op| matches!(op, OpCode::CaptureUpvalue { .. })));
    assert!(module
        .main_chunk
        .instructions
        .iter()
        .any(|op| matches!(op, OpCode::MakeClosure { upvalue_count: 1, .. })));
}

#[test]
fn test_rust_numeric_wrappers_and_fluent_methods() {
    let source = r#"
        $big = Int::i64(100)->random()->add(50)->subtract(10);
        $dec = Float::f64(3.14159)->toDecimal(2);
        $u = Int::u32(42);
    "#;

    let module = compile_source(source).expect("Numeric wrappers should compile");

    let has_i64_wrap = module.main_chunk.instructions.iter().any(|op| {
        matches!(
            op,
            OpCode::WrapRustInt {
                kind: RustIntKind::I64,
                src: Some(_),
                ..
            }
        )
    });
    let has_u32_wrap = module.main_chunk.instructions.iter().any(|op| {
        matches!(
            op,
            OpCode::WrapRustInt {
                kind: RustIntKind::U32,
                src: Some(_),
                ..
            }
        )
    });
    let has_f64_wrap = module.main_chunk.instructions.iter().any(|op| {
        matches!(
            op,
            OpCode::WrapRustFloat {
                kind: RustFloatKind::F64,
                src: Some(_),
                ..
            }
        )
    });

    assert!(has_i64_wrap);
    assert!(has_u32_wrap);
    assert!(has_f64_wrap);
}

#[test]
fn test_global_helpers_legacy_phpfun_canonicalization_and_custom_helpers() {
    let source = r#"
        $mode = env('APP_ENV', 'production');
        $merged = array_merge([1, 2], [3, 4]);
        $len = strlen('hello world');
        $custom = custom_user_helper('arg1', 42);
    "#;

    let module = compile_source(source).expect("Global helper lowering should succeed");

    let facade_calls: Vec<(String, String, u16)> = module
        .main_chunk
        .instructions
        .iter()
        .filter_map(|op| match op {
            OpCode::FacadeCall {
                facade,
                method,
                arg_count,
                ..
            } => {
                let f_name = module
                    .main_chunk
                    .get_string_constant(*facade)
                    .unwrap()
                    .to_string();
                let m_name = module
                    .main_chunk
                    .get_string_constant(*method)
                    .unwrap()
                    .to_string();
                Some((f_name, m_name, *arg_count))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        facade_calls,
        vec![
            ("Sys".to_string(), "env".to_string(), 2),
            ("Arr".to_string(), "merge".to_string(), 2),
            ("Str".to_string(), "length".to_string(), 1),
            ("Glob".to_string(), "customHelper".to_string(), 3),
        ]
    );
}

#[test]
fn test_async_future_concurrent_core_destructuring_and_defer_hooks() {
    let source = r#"
        $id = 7;
        [$user, $posts] = Future::wait([
            fn() => DB::table('users')->find($id),
            fn() => HTTP::get("https://api.example.com/posts?user={$id}")->json()
        ]);

        $first = Future::any([
            fn() => HTTP::get('https://primary.example.com')->body(),
            fn() => HTTP::get('https://secondary.example.com')->body()
        ]);

        defer {
            Log::info('Request completed');
        }
    "#;

    let module = compile_source(source).expect("Async Future and defer lowering should succeed");

    // Verify string interpolation in second closure produced `OpCode::Interpolate`
    let has_interpolate = module.functions.iter().any(|f| {
        f.chunk
            .instructions
            .iter()
            .any(|op| matches!(op, OpCode::Interpolate { part_count: 2, .. }))
    });
    assert!(has_interpolate, "Expected Interpolate opcode in closure");

    // Verify destructuring of `[$user, $posts]` emitted two `ArrayGet` instructions into `$user` and `$posts`
    let user_reg = module
        .main_chunk
        .register_for_var("$user")
        .expect("$user reg");
    let posts_reg = module
        .main_chunk
        .register_for_var("$posts")
        .expect("$posts reg");

    let gets_user = module
        .main_chunk
        .instructions
        .iter()
        .any(|op| matches!(op, OpCode::ArrayGet { dst, .. } if *dst == user_reg));
    let gets_posts = module
        .main_chunk
        .instructions
        .iter()
        .any(|op| matches!(op, OpCode::ArrayGet { dst, .. } if *dst == posts_reg));
    assert!(gets_user && gets_posts);

    // Verify `defer { ... }` emitted `OpCode::Defer` referencing the compiled defer function
    let defer_func_id = module
        .main_chunk
        .instructions
        .iter()
        .find_map(|op| match op {
            OpCode::Defer { func_id } => Some(*func_id),
            _ => None,
        })
        .expect("Expected Defer opcode in main_chunk");

    let defer_fn = module.get_function(defer_func_id).expect("Defer function");
    assert!(defer_fn.name.starts_with("<defer#"));
}

#[test]
fn test_control_flow_loops_switch_match_and_jump_integrity() {
    let source = r#"
        $sum = 0;
        for ($i = 0; $i < 10; $i++) {
            if ($i == 2) {
                continue;
            } elseif ($i == 8) {
                break;
            } else {
                $sum += $i;
            }
        }

        $items = [10, 20, 30];
        foreach ($items as $idx => $val) {
            while ($val > 0) {
                $val--;
                if ($val == 5) {
                    break 2;
                }
            }
        }

        $status = 200;
        $label = match ($status) {
            200, 201 => 'OK',
            404 => 'Not Found',
            default => 'Unknown',
        };

        switch ($status) {
            case 200:
                $code = 1;
                break;
            default:
                $code = 0;
                break;
        }
    "#;

    let module = compile_source(source).expect("Control flow lowering should succeed");
    let len = module.main_chunk.len() as u32;

    // Verify every jump target in `main_chunk` points to a valid instruction offset `< len`
    for (ip, op) in module.main_chunk.instructions.iter().enumerate() {
        match op {
            OpCode::Jump { target }
            | OpCode::JumpIfTrue { target, .. }
            | OpCode::JumpIfFalse { target, .. }
            | OpCode::JumpIfNull { target, .. }
            | OpCode::JumpIfNotNull { target, .. } => {
                assert!(
                    *target < len,
                    "Jump at ip {} has out-of-bounds target {} (len {})",
                    ip,
                    target,
                    len
                );
            }
            OpCode::IterNext { jump_end, .. } => {
                assert!(
                    *jump_end < len,
                    "IterNext at ip {} has out-of-bounds jump_end {} (len {})",
                    ip,
                    jump_end,
                    len
                );
            }
            _ => {}
        }
    }
}

#[test]
fn test_break_and_continue_validation_errors() {
    let bad_break = "break;";
    let err = compile_source(bad_break).expect_err("break outside loop must fail");
    match err {
        CompilationError::Ir(ir_err) => {
            assert_eq!(
                ir_err.kind,
                IrErrorKind::ControlFlowOutsideLoop {
                    statement: "break".to_string()
                }
            );
        }
        other => panic!("Expected IrError, got {:?}", other),
    }

    let bad_level = r#"
        while (true) {
            break 3;
        }
    "#;
    let err = compile_source(bad_level).expect_err("break 3 inside single while must fail");
    match err {
        CompilationError::Ir(ir_err) => {
            assert_eq!(
                ir_err.kind,
                IrErrorKind::InvalidBreakContinueLevel {
                    statement: "break".to_string(),
                    requested_level: 3,
                    available_depth: 1,
                }
            );
        }
        other => panic!("Expected IrError, got {:?}", other),
    }
}

#[test]
fn test_classes_constructor_promotion_enums_and_try_catch_exception_table() {
    let source = r#"
        enum OrderStatus: string {
            case Pending = 'pending';
            case Paid = 'paid';
        }

        interface Payable {
            public function total(): float;
        }

        class Order implements Payable {
            public const CURRENCY = 'USD';

            public function __construct(
                public readonly int $id,
                private float $amount
            ) {}

            public function total(): float {
                return $this->amount;
            }
        }

        try {
            $order = new Order(1, 99.95);
            $t = $order?->total() ?? 0.0;
        } catch (RuntimeException $e) {
            Log::error('failed');
        } finally {
            Log::info('cleanup');
        }
    "#;

    let module = compile_source(source).expect("OOP and try/catch lowering should succeed");

    // 1. Verify Enum compilation
    let enm = module.enums.get("OrderStatus").expect("OrderStatus enum");
    assert_eq!(enm.backing_type, Some(RhpType::String));
    assert_eq!(enm.cases.len(), 2);
    assert_eq!(enm.cases[0].name, "Pending");

    // 2. Verify Interface compilation
    assert!(module.interfaces.contains_key("Payable"));

    // 3. Verify Class compilation and promoted constructor properties
    let cls = module.get_class_by_name("Order").expect("Order class");
    assert_eq!(cls.properties.len(), 2);
    assert_eq!(cls.properties[0].name, "id");
    assert!(cls.properties[0].is_readonly);
    assert_eq!(cls.properties[1].name, "amount");
    assert_eq!(cls.constants.len(), 1);
    assert_eq!(cls.constants[0].name, "CURRENCY");

    // 4. Verify `Order::__construct` binds `$this` to `r0` and emits `SetProp` for promoted properties
    let ctor = module
        .get_function_by_name("Order::__construct")
        .expect("Order::__construct");
    assert_eq!(ctor.chunk.register_for_var("$this"), Some(Reg(0)));
    assert_eq!(ctor.chunk.register_for_var("$id"), Some(Reg(1)));
    assert_eq!(ctor.chunk.register_for_var("$amount"), Some(Reg(2)));

    let set_prop_count = ctor
        .chunk
        .instructions
        .iter()
        .filter(|op| matches!(op, OpCode::SetProp { object: Reg(0), .. }))
        .count();
    assert_eq!(set_prop_count, 2);

    // 5. Verify ExceptionHandler table in main_chunk
    assert_eq!(module.main_chunk.exception_handlers.len(), 1);
    let handler = &module.main_chunk.exception_handlers[0];
    assert!(handler.try_start < handler.try_end);
    assert_eq!(handler.catches.len(), 1);
    assert_eq!(handler.catches[0].catch_types, vec!["RuntimeException"]);
    assert!(handler.finally_target.is_some());
}

#[test]
fn test_source_span_mapping_and_disassembler() {
    let source = r#"function add(int $a, int $b): int {
    return $a + $b;
}
$res = add(10, 20);
"#;

    let mut source_map = SourceMap::new();
    let file_id = source_map.add_file("math.rhp", source);

    let module =
        compile_source_with_file_id(source, file_id).expect("Compilation with file_id should pass");

    // Every instruction in both chunks must have an exact source span with file_id
    for span in &module.main_chunk.spans {
        assert_eq!(span.file_id, file_id);
        assert!(span.line >= 1);
    }

    let add_fn = module.get_function_by_name("add").unwrap();
    assert_eq!(add_fn.chunk.instructions.len(), add_fn.chunk.spans.len());
    for span in &add_fn.chunk.spans {
        assert_eq!(span.file_id, file_id);
    }

    // Verify constant pool deduplication and disassembler output
    let disasm = module.disassemble();
    assert!(disasm.contains("=== <main>"));
    assert!(disasm.contains("=== fn add"));
    assert!(disasm.contains("ASSERT_TYPE"));
    assert!(disasm.contains("BINARY"));
    assert!(disasm.contains("CALL_FUNC"));

    // Verify type constant is stored in `add` function's constant pool
    assert!(add_fn
        .chunk
        .constants
        .iter()
        .any(|c| matches!(c, Constant::Type(RhpType::Int))));
}
