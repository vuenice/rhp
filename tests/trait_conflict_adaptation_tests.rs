use rhp::{
    compile_source, run_source_with_vm, CompilationError, PhpValue, TypeErrorKind,
};

#[test]
fn test_insteadof_conflict_resolution() {
    let source = r#"
        trait A {
            public function smallTalk(): string {
                return 'a';
            }
            public function bigTalk(): string {
                return 'A';
            }
        }

        trait B {
            public function smallTalk(): string {
                return 'b';
            }
            public function bigTalk(): string {
                return 'B';
            }
        }

        class Talker {
            use A, B {
                B::smallTalk insteadof A;
                A::bigTalk insteadof B;
            }
        }

        $talker = new Talker();
        $small = $talker->smallTalk();
        $big = $talker->bigTalk();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Conflict resolution with insteadof should succeed");
    assert_eq!(vm.get_main_variable("$small"), Some(&PhpValue::String("b".into())));
    assert_eq!(vm.get_main_variable("$big"), Some(&PhpValue::String("A".into())));
}

#[test]
fn test_trait_aliasing_with_as() {
    let source = r#"
        trait A {
            public function smallTalk(): string {
                return 'a';
            }
            public function bigTalk(): string {
                return 'A';
            }
        }

        trait B {
            public function smallTalk(): string {
                return 'b';
            }
            public function bigTalk(): string {
                return 'B';
            }
        }

        class AliasedTalker {
            use A, B {
                B::smallTalk insteadof A;
                A::bigTalk insteadof B;
                B::bigTalk as talk;
            }
        }

        $at = new AliasedTalker();
        $small = $at->smallTalk();
        $big = $at->bigTalk();
        $talk = $at->talk();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Aliasing with 'as' should succeed");
    assert_eq!(vm.get_main_variable("$small"), Some(&PhpValue::String("b".into())));
    assert_eq!(vm.get_main_variable("$big"), Some(&PhpValue::String("A".into())));
    assert_eq!(vm.get_main_variable("$talk"), Some(&PhpValue::String("B".into())));
}

#[test]
fn test_insteadof_multiple_exclusions() {
    let source = r#"
        trait T1 {
            public function greet(): string {
                return 'Hello from T1';
            }
        }

        trait T2 {
            public function greet(): string {
                return 'Hello from T2';
            }
        }

        trait T3 {
            public function greet(): string {
                return 'Hello from T3';
            }
        }

        class Greeter {
            use T1, T2, T3 {
                T2::greet insteadof T1, T3;
            }
        }

        $g = new Greeter();
        $res = $g->greet();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Multi-exclusion insteadof should succeed");
    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::String("Hello from T2".into())));
}

#[test]
fn test_trait_method_visibility_modification_with_as() {
    let source = r#"
        trait HelloWorld {
            public function sayHello(): string {
                return 'Hello World!';
            }
        }

        class MyClass1 {
            use HelloWorld {
                sayHello as protected;
            }

            public function test(): string {
                return $this->sayHello();
            }
        }

        class MyClass2 {
            use HelloWorld {
                sayHello as private myPrivateHello;
            }

            public function testOriginal(): string {
                return $this->sayHello();
            }

            public function testPrivate(): string {
                return $this->myPrivateHello();
            }
        }

        $c1 = new MyClass1();
        $res1 = $c1->test();

        $c2 = new MyClass2();
        $res2_orig = $c2->testOriginal();
        $res2_priv = $c2->testPrivate();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Visibility modification with 'as' should succeed");
    assert_eq!(vm.get_main_variable("$res1"), Some(&PhpValue::String("Hello World!".into())));
    assert_eq!(vm.get_main_variable("$res2_orig"), Some(&PhpValue::String("Hello World!".into())));
    assert_eq!(vm.get_main_variable("$res2_priv"), Some(&PhpValue::String("Hello World!".into())));
}

#[test]
fn test_class_method_precedence_over_conflicting_traits() {
    let source = r#"
        trait TraitX {
            public function compute(): int {
                return 10;
            }
        }

        trait TraitY {
            public function compute(): int {
                return 20;
            }
        }

        class CustomComputer {
            use TraitX, TraitY;

            // Class method takes precedence over trait collision
            public function compute(): int {
                return 999;
            }
        }

        $comp = new CustomComputer();
        $res = $comp->compute();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Class method overriding conflicting traits should succeed");
    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::Int(999)));
}

#[test]
fn test_unresolved_trait_collision_fails_compilation() {
    let source = r#"
        trait First {
            public function doWork(): string {
                return 'first';
            }
        }

        trait Second {
            public function doWork(): string {
                return 'second';
            }
        }

        class Worker {
            use First, Second;
        }
    "#;

    let result = compile_source(source);
    match result {
        Err(CompilationError::Type(err)) => match err.kind {
            TypeErrorKind::TraitMethodCollision {
                ref class_name,
                ref method_name,
                ..
            } => {
                assert_eq!(class_name, "Worker");
                assert_eq!(method_name, "doWork");
            }
            other => panic!("Expected TraitMethodCollision, got {:?}", other),
        },
        other => panic!("Expected CompilationError::Type, got {:?}", other),
    }
}

#[test]
fn test_traits_composing_traits_with_adaptations() {
    let source = r#"
        trait BaseA {
            public function ping(): string {
                return 'pongA';
            }
        }

        trait BaseB {
            public function ping(): string {
                return 'pongB';
            }
            public function extra(): string {
                return 'extraB';
            }
        }

        trait Composite {
            use BaseA, BaseB {
                BaseA::ping insteadof BaseB;
                BaseB::ping as pingB;
            }
        }

        class Service {
            use Composite;
        }

        $s = new Service();
        $p1 = $s->ping();
        $p2 = $s->pingB();
        $ex = $s->extra();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Trait composing other traits with adaptations should succeed");
    assert_eq!(vm.get_main_variable("$p1"), Some(&PhpValue::String("pongA".into())));
    assert_eq!(vm.get_main_variable("$p2"), Some(&PhpValue::String("pongB".into())));
    assert_eq!(vm.get_main_variable("$ex"), Some(&PhpValue::String("extraB".into())));
}

#[test]
fn test_namespaced_trait_adaptations() {
    let source = r#"
        namespace App\Traits {
            trait Alpha {
                public function message(): string {
                    return 'alpha';
                }
            }
            trait Beta {
                public function message(): string {
                    return 'beta';
                }
            }
        }

        namespace App\Services {
            class Messenger {
                use \App\Traits\Alpha, \App\Traits\Beta {
                    \App\Traits\Beta::message insteadof \App\Traits\Alpha;
                    \App\Traits\Alpha::message as alphaMessage;
                }
            }

            $m = new Messenger();
            $main = $m->message();
            $alpha = $m->alphaMessage();
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Namespaced trait adaptations should succeed");
    assert_eq!(vm.get_main_variable("$main"), Some(&PhpValue::String("beta".into())));
    assert_eq!(vm.get_main_variable("$alpha"), Some(&PhpValue::String("alpha".into())));
}

#[test]
fn test_invalid_insteadof_target_fails_compilation() {
    let source = r#"
        trait T1 {
            public function foo(): void {}
        }
        trait T2 {
            public function foo(): void {}
        }
        class Bad {
            use T1, T2 {
                T1::foo insteadof NonExistentTrait;
            }
        }
    "#;

    let result = compile_source(source);
    match result {
        Err(CompilationError::Type(err)) => match err.kind {
            TypeErrorKind::TraitAdaptationError { ref reason, .. } => {
                assert!(reason.contains("not used by"));
            }
            other => panic!("Expected TraitAdaptationError, got {:?}", other),
        },
        other => panic!("Expected CompilationError::Type, got {:?}", other),
    }
}

#[test]
fn test_invalid_insteadof_method_fails_compilation() {
    let source = r#"
        trait T1 {
            public function foo(): void {}
        }
        trait T2 {
            public function foo(): void {}
        }
        class Bad {
            use T1, T2 {
                T1::nonExistentMethod insteadof T2;
            }
        }
    "#;

    let result = compile_source(source);
    match result {
        Err(CompilationError::Type(err)) => match err.kind {
            TypeErrorKind::TraitAdaptationError { ref reason, .. } => {
                assert!(reason.contains("does not exist"));
            }
            other => panic!("Expected TraitAdaptationError, got {:?}", other),
        },
        other => panic!("Expected CompilationError::Type, got {:?}", other),
    }
}
