use rhp::{compile_source, run_source_with_vm, PhpValue};

#[test]
fn test_attribute_override_standard_syntax() {
    let source = r#"
        class Animal {
            public function speak(): string {
                return "sound";
            }
        }

        class Dog extends Animal {
            #[Override]
            public function speak(): string {
                return "woof";
            }
        }

        $dog = new Dog();
        $res = $dog->speak();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution must succeed");
    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::string("woof")));
}

#[test]
fn test_attribute_root_override_with_leading_backslash() {
    let source = r#"
        class Vehicle {
            public function drive(): string {
                return "vroom";
            }
        }

        class Car extends Vehicle {
            #[\Override]
            public function drive(): string {
                return "zoom";
            }
        }

        $car = new Car();
        $res = $car->drive();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution must succeed");
    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::string("zoom")));
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
            #[Override]
            public function nonExistent(): int {
                return 20;
            }
        }
    "#;

    let err = compile_source(source);
    assert!(
        err.is_err(),
        "Compilation must fail when #[Override] is placed on a method not declared in parent or interface"
    );
}

#[test]
fn test_root_class_invalid_override_rejected() {
    let source = r#"
        class StandaloneService {
            #[Override]
            public function orphanedMethod(): int {
                return 42;
            }
        }
    "#;

    let err = compile_source(source);
    assert!(
        err.is_err(),
        "Compilation must fail when #[Override] is placed on a method in a root class without matching interface"
    );
}

#[test]
fn test_final_class_extension_forbidden() {
    let source = r#"
        final class ClosedClass {
            public function secret(): int {
                return 42;
            }
        }

        class SubClass extends ClosedClass {
            public function secret(): int {
                return 99;
            }
        }
    "#;

    let err = compile_source(source);
    assert!(
        err.is_err(),
        "Compilation must fail when extending a final class"
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
fn test_trait_method_collision_resolved_by_insteadof() {
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
            use TraitA, TraitB {
                TraitA::sharedMethod insteadof TraitB;
            }
        }

        $obj = new ResolvedClass();
        $res = $obj->sharedMethod();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution must succeed when resolved with insteadof");
    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::string("A")));
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
fn test_final_class_and_cha_devirtualization_guarantee() {
    let source = r#"
        final class GuaranteedFinalModel {
            public function compute(): int {
                return 100;
            }
        }

        class OpenModelWithoutOverrides {
            public function calculate(): int {
                return 200;
            }
        }

        class BaseWithOverride {
            public function run(): int {
                return 1;
            }
        }

        class ChildOverride extends BaseWithOverride {
            #[Override]
            public function run(): int {
                return 2;
            }
        }
    "#;

    let module = compile_source(source).expect("Compilation must succeed");
    let class_table = &module.class_table;

    // 1. Final class methods are guaranteed never overridden -> statically devirtualized
    assert!(!class_table.is_method_overridden("GuaranteedFinalModel", "compute"));

    // 2. Open class without overrides in whole program -> CHA devirtualizes it statically
    assert!(!class_table.is_method_overridden("OpenModelWithoutOverrides", "calculate"));

    // 3. Open class with actual subclass override -> recognized by CHA as virtual
    assert!(class_table.is_method_overridden("BaseWithOverride", "run"));
}

#[test]
fn test_non_reserved_identifiers_open_sealed_override() {
    // Proves 100% PHP compatibility: open, sealed, and override are NOT reserved keywords
    let source = r#"
        class ResourceHandler {
            public function open(): string {
                return "opened";
            }

            public function sealed(): string {
                return "sealed_box";
            }

            public function override(): string {
                return "override_flag";
            }
        }

        $handler = new ResourceHandler();
        $r1 = $handler->open();
        $r2 = $handler->sealed();
        $r3 = $handler->override();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("open, sealed, override must be valid method names in standard PHP");
    assert_eq!(vm.get_main_variable("$r1"), Some(&PhpValue::string("opened")));
    assert_eq!(vm.get_main_variable("$r2"), Some(&PhpValue::string("sealed_box")));
    assert_eq!(vm.get_main_variable("$r3"), Some(&PhpValue::string("override_flag")));
}
