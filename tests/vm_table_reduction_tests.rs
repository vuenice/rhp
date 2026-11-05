use rhp::{compile_source, run_source_with_vm, PhpValue};

#[test]
fn test_open_class_and_override_keyword() {
    let source = r#"
        open class Animal {
            public function speak(): string {
                return "sound";
            }
        }

        class Dog extends Animal {
            override public function speak(): string {
                return "woof";
            }
        }

        $dog = new Dog();
        $res = $dog->speak();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution must succeed");
    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::string("woof")));

    let animal_info = vm.module().class_table.get_by_name("Animal").expect("Animal class exists");
    assert!(animal_info.is_open);
}

#[test]
fn test_attribute_open_and_override() {
    let source = r#"
        #[Open]
        class Vehicle {
            public function drive(): string {
                return "vroom";
            }
        }

        class Car extends Vehicle {
            #[Override]
            public function drive(): string {
                return "zoom";
            }
        }

        $car = new Car();
        $res = $car->drive();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution must succeed");
    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::string("zoom")));

    let vehicle_info = vm.module().class_table.get_by_name("Vehicle").expect("Vehicle class exists");
    assert!(vehicle_info.is_open);
}

#[test]
fn test_invalid_override_without_parent_method_rejected() {
    let source = r#"
        class BaseService {
            public function execute(): int {
                return 10;
            }
        }

        class MyService extends BaseService {
            override public function nonExistent(): int {
                return 20;
            }
        }
    "#;

    let err = compile_source(source);
    assert!(
        err.is_err(),
        "Compilation must fail when 'override' is placed on a method not declared in parent"
    );
}

#[test]
fn test_sealed_class_extension_forbidden() {
    let source = r#"
        sealed class ClosedClass {
            public function secret(): int {
                return 42;
            }
        }

        class HackerClass extends ClosedClass {
            public function secret(): int {
                return 99;
            }
        }
    "#;

    let err = compile_source(source);
    assert!(
        err.is_err(),
        "Compilation must fail when extending a sealed class"
    );
}

#[test]
fn test_trait_method_collision_detected_and_enforced() {
    let source = r#"
        trait TraitA {
            public function sharedMethod(): string {
                return "A";
            }
        }

        trait TraitB {
            public function sharedMethod(): string {
                return "B";
            }
        }

        class CollidingClass {
            use TraitA, TraitB;
        }
    "#;

    let err = compile_source(source);
    assert!(
        err.is_err(),
        "Compilation must fail on trait method collision without class resolution"
    );
}

#[test]
fn test_trait_method_collision_resolved_by_class_override() {
    let source = r#"
        trait TraitA {
            public function sharedMethod(): string {
                return "A";
            }
        }

        trait TraitB {
            public function sharedMethod(): string {
                return "B";
            }
        }

        class ResolvedClass {
            use TraitA, TraitB;

            public function sharedMethod(): string {
                return "Resolved";
            }
        }

        $obj = new ResolvedClass();
        $res = $obj->sharedMethod();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution must succeed when class overrides collision");
    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::string("Resolved")));
}

#[test]
fn test_sealed_class_devirtualization_guarantee() {
    let source = r#"
        sealed class StaticGuaranteedModel {
            public function compute(): int {
                return 100;
            }
        }
    "#;

    let module = compile_source(source).expect("Compilation must succeed");
    let class_table = &module.class_table;

    // Sealed class methods are guaranteed never overridden
    assert!(!class_table.is_method_overridden("StaticGuaranteedModel", "compute"));
}
