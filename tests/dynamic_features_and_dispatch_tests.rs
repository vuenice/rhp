use rhp::{compile_source, run_source_with_vm, PhpValue};

#[test]
fn test_instance_magic_call_forwarding() {
    let source = r#"
        class EloquentModel {
            public string $table = 'users';

            public function __call(string $name, array $args): string {
                return "dynamic_call:" . $name . ":" . implode(",", $args);
            }
        }

        $user = new EloquentModel();
        $res1 = $user->scopeActive('admin', 'verified');
        $res2 = $user->whereStatus(1, 2, 3);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution must succeed");

    assert_eq!(
        vm.get_main_variable("$res1"),
        Some(&PhpValue::string("dynamic_call:scopeActive:admin,verified"))
    );
    assert_eq!(
        vm.get_main_variable("$res2"),
        Some(&PhpValue::string("dynamic_call:whereStatus:1,2,3"))
    );
}

#[test]
fn test_class_name_as_value_and_escape_tracking() {
    let source = r#"
        class UserRepository {
            public string $name = 'UserRepository';
        }

        class OrderRepository {
            public string $name = 'OrderRepository';
        }

        $user_class = UserRepository::class;
        $order_class = OrderRepository::class;
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution must succeed");

    assert_eq!(
        vm.get_main_variable("$user_class"),
        Some(&PhpValue::string("UserRepository"))
    );
    assert_eq!(
        vm.get_main_variable("$order_class"),
        Some(&PhpValue::string("OrderRepository"))
    );

    // Verify escape tracking in class_table
    assert!(vm.module().class_table.is_class_escaped("UserRepository"));
    assert!(vm.module().class_table.is_class_escaped("OrderRepository"));
}

#[test]
fn test_di_container_app_make_and_bind() {
    let source = r#"
        class ServiceA {
            public int $val = 100;
        }

        class ServiceB {
            public int $val = 200;
        }

        // Test 1: Direct app(Foo::class) resolution
        $instA = app(ServiceA::class);
        $valA = $instA->val;

        // Test 2: app()->make(Foo::class) resolution
        $app = app();
        $instB = $app->make(ServiceB::class);
        $valB = $instB->val;
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution must succeed");

    assert_eq!(vm.get_main_variable("$valA"), Some(&PhpValue::Int(100)));
    assert_eq!(vm.get_main_variable("$valB"), Some(&PhpValue::Int(200)));
}

#[test]
fn test_class_hierarchy_analysis_and_devirtualization_detection() {
    let source = r#"
        class BaseEntity {
            public function save(): string {
                return "base_saved";
            }

            public function customHook(): string {
                return "base_hook";
            }
        }

        class SubEntityA extends BaseEntity {
            public function customHook(): string {
                return "sub_a_hook";
            }
        }

        class SubEntityB extends BaseEntity {}
    "#;

    let module = compile_source(source).expect("Compilation must succeed");
    let class_table = &module.class_table;

    // `save` is declared in BaseEntity and NOT overridden by SubEntityA or SubEntityB.
    // Whole-program CHA confirms it is safe for devirtualization (single target)!
    assert!(!class_table.is_method_overridden("BaseEntity", "save"));

    // `customHook` IS overridden by SubEntityA, so CHA recognizes multiple overrides.
    assert!(class_table.is_method_overridden("BaseEntity", "customHook"));

    // Verify dense slot tables and cached magic slots
    let base_info = class_table.get_by_name("BaseEntity").expect("BaseEntity must exist");
    assert!(base_info.method_slots.len() >= 2);
    assert!(base_info.method_indices.contains_key("save"));
    assert!(base_info.method_indices.contains_key("customHook"));
}

#[test]
fn test_polymorphic_method_dispatch_runtime() {
    let source = r#"
        abstract class Animal {
            public abstract function speak(): string;
        }

        class Dog extends Animal {
            public function speak(): string {
                return "Woof";
            }
        }

        class Cat extends Animal {
            public function speak(): string {
                return "Meow";
            }
        }

        function makeItSpeak(Animal $a): string {
            return $a->speak();
        }

        $dog = new Dog();
        $cat = new Cat();

        $dog_sound = makeItSpeak($dog);
        $cat_sound = makeItSpeak($cat);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution must succeed");

    assert_eq!(vm.get_main_variable("$dog_sound"), Some(&PhpValue::string("Woof")));
    assert_eq!(vm.get_main_variable("$cat_sound"), Some(&PhpValue::string("Meow")));
}
