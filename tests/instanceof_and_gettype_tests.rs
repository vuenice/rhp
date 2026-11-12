use rhp::{
    is_legacy_php_function, is_static_facade, is_static_only, run_source_with_vm, PhpValue,
};

#[test]
fn test_debug_facade_and_gettype_registration() {
    assert!(is_static_facade("Debug"));
    assert!(is_static_only("Debug"));

    assert!(is_legacy_php_function("gettype"));
    assert!(is_legacy_php_function("getType"));
    assert!(is_legacy_php_function("get_debug_type"));
    assert!(is_legacy_php_function("is_a"));

    let mapping = rhp::get_legacy_php_function("gettype").expect("gettype mapping");
    assert_eq!(mapping.rhp_facade, "Debug");
    assert_eq!(mapping.rhp_method, "type");

    let mapping_isa = rhp::get_legacy_php_function("is_a").expect("is_a mapping");
    assert_eq!(mapping_isa.rhp_facade, "Validate");
    assert_eq!(mapping_isa.rhp_method, "isA");
}

#[test]
fn test_gettype_and_debug_type_primitives() {
    let source = r#"
        $t_null = gettype(null);
        $t_bool1 = gettype(true);
        $t_bool2 = gettype(false);
        $t_int = gettype(42);
        $t_float = gettype(3.14);
        $t_str = gettype("hello");
        $t_arr = gettype([1, 2, 3]);

        // Case-insensitive getType
        $t_case = getType(100);

        // Direct Debug::type call
        $d_null = Debug::type(null);
        $d_int = Debug::type(999);
        $d_float = Debug::type(2.718);
        $d_str = Debug::type("rhp");

        // get_debug_type
        $g_int = get_debug_type(55);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution should succeed");

    assert_eq!(vm.get_main_variable("$t_null"), Some(&PhpValue::string("null")));
    assert_eq!(vm.get_main_variable("$t_bool1"), Some(&PhpValue::string("bool")));
    assert_eq!(vm.get_main_variable("$t_bool2"), Some(&PhpValue::string("bool")));
    assert_eq!(vm.get_main_variable("$t_int"), Some(&PhpValue::string("int")));
    assert_eq!(vm.get_main_variable("$t_float"), Some(&PhpValue::string("float")));
    assert_eq!(vm.get_main_variable("$t_str"), Some(&PhpValue::string("string")));
    assert_eq!(vm.get_main_variable("$t_case"), Some(&PhpValue::string("int")));

    assert_eq!(vm.get_main_variable("$d_null"), Some(&PhpValue::string("null")));
    assert_eq!(vm.get_main_variable("$d_int"), Some(&PhpValue::string("int")));
    assert_eq!(vm.get_main_variable("$d_float"), Some(&PhpValue::string("float")));
    assert_eq!(vm.get_main_variable("$d_str"), Some(&PhpValue::string("string")));

    assert_eq!(vm.get_main_variable("$g_int"), Some(&PhpValue::string("int")));
}

#[test]
fn test_instanceof_operator_and_functional() {
    let source = r#"
        class User {}
        class Admin extends User {}
        class Other {}

        $user = new User();
        $admin = new Admin();

        // 1. Standard infix operator
        $t1 = ($user instanceof User);
        $t2 = ($admin instanceof User);
        $t3 = ($admin instanceof Admin);
        $f1 = ($user instanceof Admin);
        $f2 = ($user instanceof Other);

        // 2. Class const ::class
        $t4 = ($user instanceof User::class);
        $t5 = ($admin instanceof User::class);

        // 3. String literal
        $t6 = ($user instanceof "User");
        $t7 = ($admin instanceof "User");

        // 4. Dynamic class variable
        $clsUser = "User";
        $clsAdmin = "Admin";
        $t8 = ($user instanceof $clsUser);
        $t9 = ($admin instanceof $clsUser);
        $t10 = ($admin instanceof $clsAdmin);
        $f3 = ($user instanceof $clsAdmin);

        // 5. Functional instanceof()
        $t11 = instanceof($user, User::class);
        $t12 = instanceof($admin, "User");
        $t13 = instanceof($admin, $clsUser);
        $f4 = instanceof($user, Admin::class);

        // 6. Functional is_a()
        $t14 = is_a($user, User::class);
        $t15 = is_a($admin, "User");
        $t16 = is_a($admin, $clsAdmin);
        $f5 = is_a($user, Admin::class);

        // 7. Object gettype() returns class name
        $user_type = gettype($user);
        $admin_type = gettype($admin);
        $debug_user_type = Debug::type($user);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Instanceof and gettype should execute");

    // Infix
    assert_eq!(vm.get_main_variable("$t1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t2"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t3"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f1"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$f2"), Some(&PhpValue::Bool(false)));

    // Class const
    assert_eq!(vm.get_main_variable("$t4"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t5"), Some(&PhpValue::Bool(true)));

    // String literal
    assert_eq!(vm.get_main_variable("$t6"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t7"), Some(&PhpValue::Bool(true)));

    // Dynamic class variable
    assert_eq!(vm.get_main_variable("$t8"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t9"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t10"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f3"), Some(&PhpValue::Bool(false)));

    // Functional instanceof()
    assert_eq!(vm.get_main_variable("$t11"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t12"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t13"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f4"), Some(&PhpValue::Bool(false)));

    // Functional is_a()
    assert_eq!(vm.get_main_variable("$t14"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t15"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t16"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f5"), Some(&PhpValue::Bool(false)));

    // Object types
    assert_eq!(vm.get_main_variable("$user_type"), Some(&PhpValue::string("User")));
    assert_eq!(vm.get_main_variable("$admin_type"), Some(&PhpValue::string("Admin")));
    assert_eq!(vm.get_main_variable("$debug_user_type"), Some(&PhpValue::string("User")));
}

#[test]
fn test_instanceof_in_if_condition() {
    let source = r#"
        class User {
            public string $name = "Alice";
        }
        class Guest {}

        function checkUser(mixed $obj): string {
            if ($obj instanceof User) {
                return "is_user:" + $obj->name;
            } else {
                return "not_user";
            }
        }

        $res1 = checkUser(new User());
        $res2 = checkUser(new Guest());
        $res3 = checkUser(null);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Conditional instanceof test should succeed");

    assert_eq!(vm.get_main_variable("$res1"), Some(&PhpValue::string("is_user:Alice")));
    assert_eq!(vm.get_main_variable("$res2"), Some(&PhpValue::string("not_user")));
    assert_eq!(vm.get_main_variable("$res3"), Some(&PhpValue::string("not_user")));
}
