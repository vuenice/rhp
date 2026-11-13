use rhp::{compile_source, run_source_with_vm, PhpValue};

#[test]
fn test_instance_magic_call_forwarding() {
    let source = r#"
        class EloquentModel {
            public string $table = 'users';

            public function __call(string $name, array $args): string {
                return "dynamic_call:" + $name + ":" + implode(",", $args);
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

#[test]
fn test_dynamic_method_and_property_with_curly_braces() {
    let source = r#"
        class ServiceHandler {
            public string $status = 'initial';
            public int $count = 10;

            public function calculate(int $a, int $b): int {
                return $a * $b;
            }

            public function getSecret(): string {
                return "secret_value";
            }

            public function __call(string $name, array $args): string {
                return "fallback:" + $name + ":" + implode("-", $args);
            }
        }

        $svc = new ServiceHandler();

        // 1. Dynamic method call with variable in braces: $obj->{$method}(...)
        $method1 = 'calculate';
        $res1 = $svc->{$method1}(6, 7);

        // 2. Dynamic method call with expression inside braces: $obj->{'get' + 'Secret'}()
        $res2 = $svc->{'get' + 'Secret'}();

        // 3. Dynamic method call forwarding to __call
        $unknown = 'whereUser';
        $res3 = $svc->{$unknown}('active', 'verified');

        // 4. Dynamic property read: $obj->{$prop}
        $prop_name = 'status';
        $prop_val = $svc->{$prop_name};

        // 5. Dynamic property write: $obj->{$prop} = 42
        $count_prop = 'count';
        $svc->{$count_prop} = 42;
        $new_count = $svc->count;
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution must succeed");

    assert_eq!(vm.get_main_variable("$res1"), Some(&PhpValue::Int(42)));
    assert_eq!(
        vm.get_main_variable("$res2"),
        Some(&PhpValue::string("secret_value"))
    );
    assert_eq!(
        vm.get_main_variable("$res3"),
        Some(&PhpValue::string("fallback:whereUser:active-verified"))
    );
    assert_eq!(
        vm.get_main_variable("$prop_val"),
        Some(&PhpValue::string("initial"))
    );
    assert_eq!(vm.get_main_variable("$new_count"), Some(&PhpValue::Int(42)));
}

#[test]
fn test_final_method_non_overriding_and_devirtualization() {
    // 1. Attempting to override a final method must fail compilation
    let invalid_override_source = r#"
        class ParentClass {
            final public function saveRecord(): string {
                return "saved";
            }
        }

        class ChildClass extends ParentClass {
            public function saveRecord(): string {
                return "hacked";
            }
        }
    "#;

    let err = compile_source(invalid_override_source);
    assert!(
        err.is_err(),
        "Compilation must fail when a child class tries to override a final method"
    );

    // 2. Final methods and methods in final classes are safely devirtualized and callable directly
    let valid_final_source = r#"
        class Model {
            final public function save(): string {
                return "model_saved";
            }

            final public function update(string $status): string {
                return "updated_to_" + $status;
            }
        }

        class UserModel extends Model {
            public function profile(): string {
                // Call final method on $this
                return $this->save();
            }
        }

        final class FinalService {
            public function execute(): int {
                return 12345;
            }
        }

        $user = new UserModel();
        $saved = $user->profile();
        $updated = $user->update("active");

        $fin = new FinalService();
        $val = $fin->execute();
    "#;

    let (vm, _module) = run_source_with_vm(valid_final_source).expect("Execution must succeed");

    assert_eq!(
        vm.get_main_variable("$saved"),
        Some(&PhpValue::string("model_saved"))
    );
    assert_eq!(
        vm.get_main_variable("$updated"),
        Some(&PhpValue::string("updated_to_active"))
    );
    assert_eq!(vm.get_main_variable("$val"), Some(&PhpValue::Int(12345)));

    // Verify CHA analysis confirms final methods are not overridden
    assert!(!vm.module().class_table.is_method_overridden("Model", "save"));
    assert!(!vm.module().class_table.is_method_overridden("Model", "update"));
    assert!(!vm.module().class_table.is_method_overridden("FinalService", "execute"));

    // Verify final_methods set on Model
    let model_info = vm.module().class_table.get_by_name("Model").unwrap();
    assert!(model_info.final_methods.contains("save"));
    assert!(model_info.final_methods.contains("update"));

    // Verify UserModel inherited final_methods from Model
    let user_model_info = vm.module().class_table.get_by_name("UserModel").unwrap();
    assert!(user_model_info.final_methods.contains("save"));
    assert!(user_model_info.final_methods.contains("update"));
}

