use rhp::{
    is_legacy_php_function, is_static_facade, run_source, run_source_with_vm,
    CompilationError, ExecutionError, ParseErrorKind, PhpValue, RuntimeErrorKind, TypeErrorKind,
};

#[test]
fn test_reflect_facade_registration_and_signature_mapping() {
    // 1. Verify Reflect is registered as a native static facade
    assert!(is_static_facade("Reflect"));

    // 2. Verify class_exists and function_exists are recognized as legacy PHP functions
    assert!(is_legacy_php_function("class_exists"));
    assert!(is_legacy_php_function("function_exists"));

    // 3. Verify mappings point to Reflect facade
    let class_exists_mapping = rhp::get_legacy_php_function("class_exists")
        .expect("class_exists mapping should exist");
    assert_eq!(class_exists_mapping.php_name, "class_exists");
    assert_eq!(class_exists_mapping.rhp_facade, "Reflect");
    assert_eq!(class_exists_mapping.rhp_method, "classExists");

    let function_exists_mapping = rhp::get_legacy_php_function("function_exists")
        .expect("function_exists mapping should exist");
    assert_eq!(function_exists_mapping.php_name, "function_exists");
    assert_eq!(function_exists_mapping.rhp_facade, "Reflect");
    assert_eq!(function_exists_mapping.rhp_method, "functionExists");
}

#[test]
fn test_reflect_class_exists_user_classes_enums_traits_interfaces() {
    let source = r#"
        class User {
            public string $name = "Alice";
        }

        trait LoggerTrait {
            public function log() {}
        }

        interface GreeterInterface {
            public function greet(): string;
        }

        enum Status: string {
            case Active = "active";
            case Inactive = "inactive";
        }

        $userExists = Reflect::classExists('User');
        $userLowerExists = Reflect::classExists('user');
        $userBackslashExists = Reflect::classExists('\\User');
        $userConstExists = Reflect::classExists(User::class);
        $noSuchClass = Reflect::classExists('NonExistentModel');

        // Traits and interfaces are not classes
        $traitExists = Reflect::classExists('LoggerTrait');
        $interfaceExists = Reflect::classExists('GreeterInterface');

        // Enums count as classes
        $enumExists = Reflect::classExists('Status');
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Reflect::classExists should succeed");

    assert_eq!(vm.get_main_variable("$userExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$userLowerExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$userBackslashExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$userConstExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$noSuchClass"), Some(&PhpValue::Bool(false)));

    assert_eq!(vm.get_main_variable("$traitExists"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$interfaceExists"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$enumExists"), Some(&PhpValue::Bool(true)));
}

#[test]
fn test_reflect_class_exists_builtin_classes_and_facades() {
    let source = r#"
        $stdClassExists = Reflect::classExists('stdClass');
        $exceptionExists = Reflect::classExists('Exception');
        $errorExists = Reflect::classExists('Error');
        $runtimeExceptionExists = Reflect::classExists('RuntimeException');
        $futureExists = Reflect::classExists('Future');
        $closureExists = Reflect::classExists('Closure');

        // Static Facades count as classes
        $strFacade = Reflect::classExists('Str');
        $arrFacade = Reflect::classExists('Arr');
        $reflectFacade = Reflect::classExists('Reflect');
        $sysFacade = Reflect::classExists('Sys');
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Reflect::classExists built-in checks should succeed");

    assert_eq!(vm.get_main_variable("$stdClassExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$exceptionExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$errorExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$runtimeExceptionExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$futureExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$closureExists"), Some(&PhpValue::Bool(true)));

    assert_eq!(vm.get_main_variable("$strFacade"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$arrFacade"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$reflectFacade"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$sysFacade"), Some(&PhpValue::Bool(true)));
}

#[test]
fn test_reflect_function_exists_user_functions_and_builtins() {
    let source = r#"
        function calculateTotal(int $a, int $b): int {
            return $a + $b;
        }

        class Order {
            public function process() {}
        }

        $calcExists = Reflect::functionExists('calculateTotal');
        $calcLowerExists = Reflect::functionExists('calculatetotal');
        $calcBackslash = Reflect::functionExists('\\calculateTotal');
        $noSuchFunc = Reflect::functionExists('nonExistentFunction');

        // Methods are not standalone functions
        $methodExists = Reflect::functionExists('Order::process');
        $bareMethodExists = Reflect::functionExists('process');

        // Standard legacy PHP functions and intrinsics
        $strlenExists = Reflect::functionExists('strlen');
        $mergeExists = Reflect::functionExists('array_merge');
        $printExists = Reflect::functionExists('print');
        $echoExists = Reflect::functionExists('echo');
        $classExistsFunc = Reflect::functionExists('class_exists');
        $funcExistsFunc = Reflect::functionExists('function_exists');
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Reflect::functionExists checks should succeed");

    assert_eq!(vm.get_main_variable("$calcExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$calcLowerExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$calcBackslash"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$noSuchFunc"), Some(&PhpValue::Bool(false)));

    assert_eq!(vm.get_main_variable("$methodExists"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$bareMethodExists"), Some(&PhpValue::Bool(false)));

    assert_eq!(vm.get_main_variable("$strlenExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$mergeExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$printExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$echoExists"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$classExistsFunc"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$funcExistsFunc"), Some(&PhpValue::Bool(true)));
}

#[test]
fn test_php_syntax_class_exists_and_function_exists_rewriting() {
    let source = r#"
        class Article {
            public string $title = 'News';
        }

        function publishArticle(): string {
            return 'published';
        }

        $c1 = class_exists('Article');
        $c2 = class_exists(Article::class);
        $c3 = class_exists('NonExistent');
        $c4 = class_exists('stdClass');

        $f1 = function_exists('publishArticle');
        $f2 = function_exists('strlen');
        $f3 = function_exists('array_merge');
        $f4 = function_exists('unknown_func');
    "#;

    let (vm, _) = run_source_with_vm(source).expect("class_exists and function_exists should succeed");

    assert_eq!(vm.get_main_variable("$c1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$c2"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$c3"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$c4"), Some(&PhpValue::Bool(true)));

    assert_eq!(vm.get_main_variable("$f1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f2"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f3"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f4"), Some(&PhpValue::Bool(false)));
}

#[test]
fn test_reflect_method_aliases_and_namespaced_classes() {
    let source = r#"
        namespace App\Domain {
            class Customer {
                public int $id = 42;
            }

            function getCustomer(): string {
                return 'Customer42';
            }
        }

        namespace App\Services {
            use App\Domain\Customer;

            $hasFqn = Reflect::classExists('App\\Domain\\Customer');
            $hasLeadBackslash = Reflect::classExists('\\App\\Domain\\Customer');
            $hasAliasClass = Reflect::classExists(Customer::class);

            $hasSnakeClass = Reflect::class_exists('App\\Domain\\Customer');
            $hasSnakeFunc = Reflect::function_exists('App\\Domain\\getCustomer');
        }
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Namespaced Reflect lookups should succeed");

    assert_eq!(vm.get_main_variable("$hasFqn"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$hasLeadBackslash"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$hasAliasClass"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$hasSnakeClass"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$hasSnakeFunc"), Some(&PhpValue::Bool(true)));
}

#[test]
fn test_reflect_facade_instantiation_forbidden() {
    let source = r#"
        $r = new Reflect();
    "#;

    let err = run_source(source).expect_err("new Reflect() must fail with fatal error");
    match err {
        ExecutionError::Compilation(CompilationError::Parse(parse_err)) => {
            assert!(matches!(
                parse_err.kind,
                ParseErrorKind::StaticFacadeInstantiationForbidden { ref class_name } if class_name == "Reflect"
            ));
        }
        ExecutionError::Compilation(CompilationError::Type(type_err)) => {
            assert!(matches!(
                type_err.kind,
                TypeErrorKind::StaticFacadeInstantiationForbidden { ref class_name } if class_name == "Reflect"
            ));
        }
        ExecutionError::Runtime(rt_err) => {
            assert!(matches!(
                rt_err.kind,
                RuntimeErrorKind::StaticFacadeInstantiationForbidden { ref class } if class == "Reflect"
            ));
        }
        other => panic!("Expected StaticFacadeInstantiationForbidden, got {:?}", other),
    }
}
