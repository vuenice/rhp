use indexmap::IndexMap;
use rhp::lint::lint_source;
use rhp::{
    clear_request_context, init_request_params, run_source, ParentProcess, PhpValue, WorkerRequest,
    WorkerResponseStatus,
};

#[test]
fn test_session_facade_get_set_has_all_forget() {
    clear_request_context();

    let script = r#"
        Session::set('user_id', 101);
        Session::set('role', 'admin');

        $userId = Session::get('user_id');
        $role = Session::get('role');
        $hasUser = Session::has('user_id');
        $missing = Session::get('non_existent', 'guest');

        Session::forget('role');
        $hasRoleAfterForget = Session::has('role');

        return [
            'userId' => $userId,
            'role' => $role,
            'hasUser' => $hasUser,
            'missing' => $missing,
            'hasRoleAfterForget' => $hasRoleAfterForget,
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("userId"), Some(&PhpValue::Int(101)));
        assert_eq!(map.get("role"), Some(&PhpValue::string("admin")));
        assert_eq!(map.get("hasUser"), Some(&PhpValue::Bool(true)));
        assert_eq!(map.get("missing"), Some(&PhpValue::string("guest")));
        assert_eq!(map.get("hasRoleAfterForget"), Some(&PhpValue::Bool(false)));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_cookie_facade_get_set_has_all() {
    clear_request_context();

    let script = r#"
        Cookie::set('theme', 'dark');
        Cookie::set('fontSize', 16);

        $theme = Cookie::get('theme');
        $fontSize = Cookie::get('fontSize');
        $hasTheme = Cookie::has('theme');
        $defaultLang = Cookie::get('lang', 'en');

        Cookie::forget('theme');
        $hasThemeAfterForget = Cookie::has('theme');

        return [
            'theme' => $theme,
            'fontSize' => $fontSize,
            'hasTheme' => $hasTheme,
            'defaultLang' => $defaultLang,
            'hasThemeAfterForget' => $hasThemeAfterForget,
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("theme"), Some(&PhpValue::string("dark")));
        assert_eq!(map.get("fontSize"), Some(&PhpValue::string("16")));
        assert_eq!(map.get("hasTheme"), Some(&PhpValue::Bool(true)));
        assert_eq!(map.get("defaultLang"), Some(&PhpValue::string("en")));
        assert_eq!(map.get("hasThemeAfterForget"), Some(&PhpValue::Bool(false)));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_request_values_are_strings_by_default() {
    clear_request_context();

    let mut initial_params = IndexMap::new();
    initial_params.insert("id".to_string(), PhpValue::Int(42));
    initial_params.insert("active".to_string(), PhpValue::Bool(true));
    initial_params.insert("price".to_string(), PhpValue::Float(19.99));
    init_request_params(initial_params);

    let script = r#"
        $id = Request::get('id');
        $active = Request::get('active');
        $price = Request::get('price');
        $all = Request::all();

        return [
            'id' => $id,
            'active' => $active,
            'price' => $price,
            'all' => $all,
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        // All request values must be by default string values
        assert_eq!(map.get("id"), Some(&PhpValue::string("42")));
        assert_eq!(map.get("active"), Some(&PhpValue::string("1")));
        assert_eq!(map.get("price"), Some(&PhpValue::string("19.99")));

        if let Some(PhpValue::Dict(all_dict)) = map.get("all") {
            let all_map = all_dict.borrow();
            assert_eq!(all_map.get("id"), Some(&PhpValue::string("42")));
            assert_eq!(all_map.get("active"), Some(&PhpValue::string("1")));
            assert_eq!(all_map.get("price"), Some(&PhpValue::string("19.99")));
        } else {
            panic!("Expected all to be dict");
        }
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_request_format_with_types_and_unquoted_types() {
    clear_request_context();

    let mut initial_params = IndexMap::new();
    initial_params.insert("param1".to_string(), PhpValue::string("123"));
    initial_params.insert("param2".to_string(), PhpValue::string("1"));
    initial_params.insert("rate".to_string(), PhpValue::string("4.75"));
    initial_params.insert("flag_false".to_string(), PhpValue::string("false"));
    initial_params.insert("title".to_string(), PhpValue::string("my title"));
    init_request_params(initial_params);

    // Testing Request::format(['param1'=>'int', 'param2'=>bool]); as requested
    let script = r#"
        Request::format([
            'param1' => 'int',
            'param2' => bool,
            'rate' => float,
            'flag_false' => bool,
            'title' => string,
        ]);

        $param1 = Request::get('param1');
        $param2 = Request::get('param2');
        $rate = Request::get('rate');
        $flagFalse = Request::get('flag_false');
        $title = Request::get('title');

        return [
            'param1' => $param1,
            'param2' => $param2,
            'rate' => $rate,
            'flagFalse' => $flagFalse,
            'title' => $title,
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("param1"), Some(&PhpValue::Int(123)));
        assert_eq!(map.get("param2"), Some(&PhpValue::Bool(true)));
        assert_eq!(map.get("rate"), Some(&PhpValue::Float(4.75)));
        assert_eq!(map.get("flagFalse"), Some(&PhpValue::Bool(false)));
        assert_eq!(map.get("title"), Some(&PhpValue::string("my title")));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_no_parser_confusion_between_type_declarations_and_bare_type_keywords() {
    clear_request_context();

    let mut initial_params = IndexMap::new();
    initial_params.insert("count".to_string(), PhpValue::string("10"));
    initial_params.insert("verified".to_string(), PhpValue::string("true"));
    init_request_params(initial_params);

    let script = r#"
        // 1. Prefix typed variable declaration
        int $typedVar = 50;

        // 2. Another prefix typed variable declaration
        bool $colonTyped = true;

        // 3. Typed function signature
        function compute(int $multiplier): int {
            return $multiplier * 2;
        }

        // 4. Request format using bare type keywords
        Request::format([
            'count' => int,
            'verified' => bool,
        ]);

        // 5. Functional type cast
        $castValue = int("99");

        $reqCount = Request::get('count');
        $reqVerified = Request::get('verified');
        $computed = compute($typedVar);

        return [
            'typedVar' => $typedVar,
            'colonTyped' => $colonTyped,
            'computed' => $computed,
            'castValue' => $castValue,
            'reqCount' => $reqCount,
            'reqVerified' => $reqVerified,
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("typedVar"), Some(&PhpValue::Int(50)));
        assert_eq!(map.get("colonTyped"), Some(&PhpValue::Bool(true)));
        assert_eq!(map.get("computed"), Some(&PhpValue::Int(100)));
        assert_eq!(map.get("castValue"), Some(&PhpValue::Int(99)));
        assert_eq!(map.get("reqCount"), Some(&PhpValue::Int(10)));
        assert_eq!(map.get("reqVerified"), Some(&PhpValue::Bool(true)));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_request_only_filter() {
    clear_request_context();

    let mut initial_params = IndexMap::new();
    initial_params.insert("name".to_string(), PhpValue::string("Alice"));
    initial_params.insert("email".to_string(), PhpValue::string("alice@example.com"));
    initial_params.insert("password".to_string(), PhpValue::string("secret"));
    initial_params.insert("token".to_string(), PhpValue::string("xyz123"));
    init_request_params(initial_params);

    let script = r#"
        $publicInfo = Request::only(['name', 'email']);
        $nameOnly = Request::only('name');
        $multiArgs = Request::only('email', 'token');

        return [
            'publicInfo' => $publicInfo,
            'nameOnly' => $nameOnly,
            'multiArgs' => $multiArgs,
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();

        if let Some(PhpValue::Dict(pub_dict)) = map.get("publicInfo") {
            let m = pub_dict.borrow();
            assert_eq!(m.get("name"), Some(&PhpValue::string("Alice")));
            assert_eq!(m.get("email"), Some(&PhpValue::string("alice@example.com")));
            assert_eq!(m.get("password"), None);
            assert_eq!(m.get("token"), None);
        } else {
            panic!("Expected publicInfo dict");
        }

        if let Some(PhpValue::Dict(name_dict)) = map.get("nameOnly") {
            let m = name_dict.borrow();
            assert_eq!(m.get("name"), Some(&PhpValue::string("Alice")));
            assert_eq!(m.len(), 1);
        } else {
            panic!("Expected nameOnly dict");
        }

        if let Some(PhpValue::Dict(multi_dict)) = map.get("multiArgs") {
            let m = multi_dict.borrow();
            assert_eq!(m.get("email"), Some(&PhpValue::string("alice@example.com")));
            assert_eq!(m.get("token"), Some(&PhpValue::string("xyz123")));
            assert_eq!(m.len(), 2);
        } else {
            panic!("Expected multiArgs dict");
        }
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_worker_request_facades_integration() {
    let parent_code = r#"
        class ApiController {
            public static function handle(): dict {
                // Formatting request inputs
                Request::format([
                    'page' => 'int',
                    'limit' => int,
                    'active' => bool,
                ]);

                $page = Request::get('page');
                $limit = Request::get('limit');
                $active = Request::get('active');
                $sessionUser = Session::get('auth_user', 'anonymous');
                $cookieToken = Cookie::get('session_token', 'none');

                return [
                    'page' => $page,
                    'limit' => $limit,
                    'active' => $active,
                    'sessionUser' => $sessionUser,
                    'cookieToken' => $cookieToken,
                ];
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("Parent boot");
    let pool = state.spawn_worker_threads(2).expect("Pool spawn");

    let req = WorkerRequest::new("return ApiController::handle();")
        .with_id("req_facade_test")
        .with_get("page", "2")
        .with_get("limit", 50)
        .with_post("active", "1")
        .with_session("auth_user", "developer")
        .with_cookie("session_token", "jwt_token_999");

    let resp = pool.execute(req);
    assert_eq!(resp.status, WorkerResponseStatus::Success);

    let val = resp.to_php_value();
    if let PhpValue::Dict(dict) = val {
        let map = dict.borrow();
        assert_eq!(map.get("page"), Some(&PhpValue::Int(2)));
        assert_eq!(map.get("limit"), Some(&PhpValue::Int(50)));
        assert_eq!(map.get("active"), Some(&PhpValue::Bool(true)));
        assert_eq!(map.get("sessionUser"), Some(&PhpValue::string("developer")));
        assert_eq!(map.get("cookieToken"), Some(&PhpValue::string("jwt_token_999")));
    } else {
        panic!("Expected Dict return, got: {:?}", val);
    }
}

#[test]
fn test_linter_warns_on_deprecated_superglobals() {
    let code = r#"
function handle_request(): void {
    global $_GET, $_POST;
    $id = $_GET['id'];
    $name = $_POST['name'];
    $sess = $_SESSION['user'];
    $cookie = $_COOKIE['theme'];
}
"#;

    let res = lint_source(code, "controller.rhp");
    let deprecated_warnings: Vec<_> = res
        .issues
        .iter()
        .filter(|i| i.code == "deprecated-superglobal")
        .collect();

    assert!(
        !deprecated_warnings.is_empty(),
        "Expected deprecated-superglobal warnings, got: {:?}",
        res.issues
    );

    let messages: Vec<_> = deprecated_warnings.iter().map(|i| i.message.as_str()).collect();
    assert!(messages.iter().any(|m| m.contains("Request::get()")), "Expected recommendation for Request::get(): {:?}", messages);
    assert!(messages.iter().any(|m| m.contains("Session::get()")), "Expected recommendation for Session::get(): {:?}", messages);
    assert!(messages.iter().any(|m| m.contains("Cookie::get()")), "Expected recommendation for Cookie::get(): {:?}", messages);
}
