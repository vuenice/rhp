use rhp::{
    compile_source, lower_program, ClosureCategory, PhpValue, VM,
};

#[test]
fn test_closure_category_classification_in_ir() {
    let code = r#"
    class Service {
        public int $factor = 10;

        public function testCategories(): array {
            // 1. Pure non-capturing closure (no $this, no upvalues) -> should NOT capture $this!
            $pureNonCap = function(int $x): int {
                return $x * 2;
            };

            // 2. Pure non-capturing arrow function
            $pureArrow = fn(int $x): int => $x + 5;

            // 3. Pure $this-capturing closure
            $thisClosure = function(): int {
                return $this->factor;
            };

            // 4. Pure value-capturing small closure (1 variable)
            $multiplier = 3;
            $valClosure = function(int $x) use ($multiplier): int {
                return $x * $multiplier;
            };

            // 5. Pure value-capturing small arrow function (2 variables)
            $a = 2;
            $b = 4;
            $valArrow = fn(int $x): int => $x + $a + $b;

            return [
                $pureNonCap(7),
                $pureArrow(10),
                $thisClosure(),
                $valClosure(6),
                $valArrow(1),
            ];
        }
    }

    $svc = new Service();
    $res = $svc->testCategories();
    "#;

    let module = compile_source(code).expect("Compile source failed");

    // Inspect closure categories in compiled module
    let mut non_cap_count = 0;
    let mut this_only_count = 0;
    let mut val_small_count = 0;

    for func in &module.functions {
        if func.is_closure {
            if let Some(cat) = func.closure_category {
                match cat {
                    ClosureCategory::PureNonCapturing => non_cap_count += 1,
                    ClosureCategory::PureThisOnly => this_only_count += 1,
                    ClosureCategory::PureValueSmall(_) => val_small_count += 1,
                    _ => {}
                }
            }
        }
    }

    // Assert that each type of pure closure was properly recognized!
    assert!(non_cap_count >= 2, "Expected at least 2 PureNonCapturing closures, got {}", non_cap_count);
    assert!(this_only_count >= 1, "Expected at least 1 PureThisOnly closure, got {}", this_only_count);
    assert!(val_small_count >= 2, "Expected at least 2 PureValueSmall closures, got {}", val_small_count);

    // Run VM and assert results
    let mut vm = VM::new(module, false);
    let _ = vm.run().expect("VM execution failed");
    let res = vm.get_main_variable("res").cloned().expect("res variable not found");

    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let items = arr.borrow();
        assert_eq!(items.get(0), Some(&PhpValue::Int(14))); // 7 * 2
        assert_eq!(items.get(1), Some(&PhpValue::Int(15))); // 10 + 5
        assert_eq!(items.get(2), Some(&PhpValue::Int(10))); // this->factor = 10
        assert_eq!(items.get(3), Some(&PhpValue::Int(18))); // 6 * 3
        assert_eq!(items.get(4), Some(&PhpValue::Int(7)));  // 1 + 2 + 4
    } else {
        panic!("Expected array result, got {:?}", res);
    }
}

#[test]
fn test_this_elision_optimization() {
    // When a closure is inside a class method but DOES NOT use $this,
    // the compiler must elide $this from upvalues, making it PureNonCapturing!
    let code = r#"
    class DataFilter {
        public function makeGreeter(): callable {
            return function(string $name): string {
                return "Hello " . $name;
            };
        }
    }

    $df = new DataFilter();
    $cb = $df->makeGreeter();
    "#;

    let module = compile_source(code).expect("Compilation failed");

    // Find the closure function in compiled functions
    let closure_fn = module
        .functions
        .iter()
        .find(|f| f.is_closure && f.name.starts_with("<closure#"))
        .expect("Closure function not found");

    assert_eq!(
        closure_fn.closure_category,
        Some(ClosureCategory::PureNonCapturing),
        "Closure without $this usage must be PureNonCapturing"
    );
    assert!(
        closure_fn.upvalues.is_empty(),
        "Upvalues must be empty (no $this capture)"
    );

    // Execute in VM
    let mut vm = VM::new(module, false);
    let _ = vm.run().expect("VM run failed");
    let cb = vm.get_main_variable("cb").cloned().expect("cb not found");

    if let PhpValue::Closure(c) = cb {
        assert!(c.is_pure());
        assert!(c.is_non_capturing());
        assert!(c.is_zero_allocation());
        assert_eq!(c.category, ClosureCategory::PureNonCapturing);
        assert_eq!(c.upvalues.len(), 0);
    } else {
        panic!("Expected PhpValue::Closure");
    }
}

#[test]
fn test_impure_reference_closure_lowering_category() {
    let code = r#"
    $counter = 0;
    $refClosure = function() use (&$counter) {
        $counter = $counter + 1;
    };
    "#;

    // Parsing directly and lowering without the strict static type checker
    let program = rhp::parse(code).expect("Parse failed");
    let module = lower_program(&program, None).expect("Lower program failed");

    let closure_fn = module
        .functions
        .iter()
        .find(|f| f.is_closure)
        .expect("Closure not found");

    assert_eq!(
        closure_fn.closure_category,
        Some(ClosureCategory::ImpureReference(1)),
        "Closure with reference capture must be ImpureReference"
    );
    assert!(!closure_fn.closure_category.unwrap().is_pure());
}

#[test]
fn test_zero_allocation_closure_properties() {
    assert!(ClosureCategory::PureNonCapturing.is_zero_allocation());
    assert!(ClosureCategory::PureThisOnly.is_zero_allocation());
    assert!(ClosureCategory::PureValueSmall(1).is_zero_allocation());
    assert!(ClosureCategory::PureValueSmall(2).is_zero_allocation());
    assert!(!ClosureCategory::PureValueLarge(3).is_zero_allocation());
    assert!(!ClosureCategory::ImpureReference(1).is_zero_allocation());

    assert!(ClosureCategory::PureNonCapturing.is_pure());
    assert!(ClosureCategory::PureThisOnly.is_pure());
    assert!(ClosureCategory::PureValueSmall(1).is_pure());
    assert!(!ClosureCategory::ImpureReference(1).is_pure());
}
