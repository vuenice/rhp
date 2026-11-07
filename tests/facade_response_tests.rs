use rhp::{
    clear_request_context, execute_isolated_request, http_status_reason, run_source, ParentProcess,
    PhpValue, WorkerRequest, WorkerResponseStatus,
};

#[test]
fn test_response_json_with_chained_header() {
    clear_request_context();

    let script = r#"
        $resp = Response::json(['message' => 'Created successfully', 'id' => 42], 201)
            ->header('X-Inertia', 'true')
            ->header('X-Custom-Header', 'custom-value');

        return [
            'status' => $resp->getStatusCode(),
            'inertia' => $resp->header('X-Inertia'),
            'custom' => $resp->header('X-Custom-Header'),
            'content' => $resp->getContent(),
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("status"), Some(&PhpValue::Int(201)));
        assert_eq!(map.get("inertia"), Some(&PhpValue::string("true")));
        assert_eq!(map.get("custom"), Some(&PhpValue::string("custom-value")));
        let content_str = map.get("content").unwrap().to_php_string();
        assert!(content_str.contains("\"message\":\"Created successfully\""));
        assert!(content_str.contains("\"id\":42"));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_response_html_with_custom_header_and_status() {
    clear_request_context();

    let script = r#"
        $resp = Response::html('<h1>Hello World</h1>', 200)
            ->header('X-Frame-Options', 'DENY');

        return [
            'status' => $resp->getStatusCode(),
            'content' => $resp->getContent(),
            'frame_opt' => $resp->header('X-Frame-Options'),
            'content_type' => $resp->header('Content-Type'),
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("status"), Some(&PhpValue::Int(200)));
        assert_eq!(map.get("content"), Some(&PhpValue::string("<h1>Hello World</h1>")));
        assert_eq!(map.get("frame_opt"), Some(&PhpValue::string("DENY")));
        assert_eq!(map.get("content_type"), Some(&PhpValue::string("text/html; charset=utf-8")));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_response_redirect() {
    clear_request_context();

    let script = r#"
        $resp = Response::redirect('/login', 302)
            ->header('X-Redirect-Reason', 'session_expired');

        return [
            'status' => $resp->getStatusCode(),
            'location' => $resp->header('Location'),
            'reason' => $resp->header('X-Redirect-Reason'),
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("status"), Some(&PhpValue::Int(302)));
        assert_eq!(map.get("location"), Some(&PhpValue::string("/login")));
        assert_eq!(map.get("reason"), Some(&PhpValue::string("session_expired")));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_response_status_and_with_headers_chaining() {
    clear_request_context();

    let script = r#"
        $resp = Response::json(['error' => 'Unprocessable Entity'])
            ->status(422)
            ->withHeaders([
                'X-Error-Code' => 'VALIDATION_FAILED',
                'X-Inertia' => 'true',
            ]);

        return [
            'status' => $resp->getStatusCode(),
            'code' => $resp->header('X-Error-Code'),
            'inertia' => $resp->header('X-Inertia'),
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("status"), Some(&PhpValue::Int(422)));
        assert_eq!(map.get("code"), Some(&PhpValue::string("VALIDATION_FAILED")));
        assert_eq!(map.get("inertia"), Some(&PhpValue::string("true")));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_response_static_status_and_header_calls() {
    clear_request_context();

    let script = r#"
        Response::status(401);
        Response::header('WWW-Authenticate', 'Bearer');
        Response::header('X-Auth-Realm', 'api');

        return [
            'status' => Response::getStatusCode(),
            'auth' => Response::header('WWW-Authenticate'),
            'realm' => Response::header('X-Auth-Realm'),
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("status"), Some(&PhpValue::Int(401)));
        assert_eq!(map.get("auth"), Some(&PhpValue::string("Bearer")));
        assert_eq!(map.get("realm"), Some(&PhpValue::string("api")));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_response_worker_execution_returns_custom_status_and_headers() {
    clear_request_context();

    let parent_code = r#"
        class OrderController {
            public static function store(): object {
                return Response::json(['order_id' => 999], 201)
                    ->header('X-Inertia', 'true')
                    ->header('Location', '/orders/999');
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("Parent boot");
    let pool = state.spawn_worker_threads(1).expect("Pool spawn");

    let req = WorkerRequest::new("return OrderController::store();")
        .with_id("req_order_test");

    let resp = pool.execute(req);
    assert_eq!(resp.status, WorkerResponseStatus::Success);
    assert_eq!(resp.status_code(), 201);
    assert_eq!(resp.header("X-Inertia"), Some("true"));
    assert_eq!(resp.header("Location"), Some("/orders/999"));
    assert_eq!(
        resp.header("Content-Type"),
        Some("application/json; charset=utf-8")
    );

    // Verify string conversion on returned Response object prints content
    let val = resp.to_php_value();
    let body = val.to_php_string();
    assert!(body.contains("\"order_id\":999"));
}

#[test]
fn test_response_worker_execution_with_redirect_302() {
    clear_request_context();

    let parent_code = r#"
        class AuthController {
            public static function logout(): object {
                return Response::redirect('/login', 302)
                    ->header('X-Logged-Out', '1');
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("Parent boot");
    let pool = state.spawn_worker_threads(1).expect("Pool spawn");

    let req = WorkerRequest::new("return AuthController::logout();")
        .with_id("req_logout_test");

    let resp = pool.execute(req);
    assert_eq!(resp.status, WorkerResponseStatus::Success);
    assert_eq!(resp.status_code(), 302);
    assert_eq!(resp.header("Location"), Some("/login"));
    assert_eq!(resp.header("X-Logged-Out"), Some("1"));
}

#[test]
fn test_response_worker_static_status_and_header_without_returning_object() {
    clear_request_context();

    let script = r#"
        Response::status(422);
        Response::header('X-Validation-Error', 'email_taken');
        return ['error' => 'Email already registered'];
    "#;

    let state = ParentProcess::boot("").expect("Parent boot");
    let req = WorkerRequest::new(script);
    let resp = execute_isolated_request(&state, &req, 0);

    assert_eq!(resp.status, WorkerResponseStatus::Success);
    assert_eq!(resp.status_code(), 422);
    assert_eq!(resp.header("X-Validation-Error"), Some("email_taken"));
}

#[test]
fn test_response_facade_cannot_be_instantiated() {
    clear_request_context();

    let script = "new Response();";
    let res = run_source(script);
    assert!(
        res.is_err(),
        "Response is a static facade and cannot be instantiated with 'new'"
    );
    let err = res.unwrap_err().to_string();
    assert!(
        err.contains("Cannot instantiate static facade") || err.contains("Response"),
        "Unexpected error: {}",
        err
    );
}

#[test]
fn test_http_status_reason_mapping() {
    assert_eq!(http_status_reason(200), "OK");
    assert_eq!(http_status_reason(201), "Created");
    assert_eq!(http_status_reason(204), "No Content");
    assert_eq!(http_status_reason(302), "Found");
    assert_eq!(http_status_reason(401), "Unauthorized");
    assert_eq!(http_status_reason(404), "Not Found");
    assert_eq!(http_status_reason(422), "Unprocessable Content");
    assert_eq!(http_status_reason(500), "Internal Server Error");
}

#[test]
fn test_response_no_content() {
    clear_request_context();

    let script = r#"
        $resp = Response::noContent(204)
            ->header('X-Action', 'deleted');

        return [
            'status' => $resp->getStatusCode(),
            'action' => $resp->header('X-Action'),
            'content' => $resp->getContent(),
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("status"), Some(&PhpValue::Int(204)));
        assert_eq!(map.get("action"), Some(&PhpValue::string("deleted")));
        assert_eq!(map.get("content"), Some(&PhpValue::string("")));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_response_macro_custom_inertia() {
    clear_request_context();

    let script = r#"
        Response::macro('inertia', function ($component, $props = []) {
            return Response::json([
                'component' => $component,
                'props' => $props,
            ], 200)->header('X-Inertia', 'true');
        });

        $resp = Response::inertia('Dashboard', ['user' => 'Alex']);
        return [
            'status' => $resp->getStatusCode(),
            'inertia' => $resp->header('X-Inertia'),
            'content' => $resp->getContent(),
        ];
    "#;

    let res = run_source(script).expect("Execution failed");
    if let PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("status"), Some(&PhpValue::Int(200)));
        assert_eq!(map.get("inertia"), Some(&PhpValue::string("true")));
        let c = map.get("content").unwrap().to_php_string();
        assert!(c.contains("\"component\":\"Dashboard\""));
        assert!(c.contains("\"user\":\"Alex\""));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_response_wire_http_formatting() {
    clear_request_context();

    let script = r#"
        return Response::json(['auth' => true], 201)
            ->header('X-Inertia', 'true')
            ->header('Location', '/home');
    "#;

    let state = ParentProcess::boot("").expect("Parent boot");
    let req = WorkerRequest::new(script);
    let resp = execute_isolated_request(&state, &req, 0);

    assert_eq!(resp.status, WorkerResponseStatus::Success);
    assert_eq!(resp.status_code(), 201);

    // Simulate main.rs wire formatting
    let status_code = resp.status_code;
    let status_line = format!("HTTP/1.1 {} {}", status_code, http_status_reason(status_code));
    assert_eq!(status_line, "HTTP/1.1 201 Created");

    let val = resp.to_php_value();
    let content = if let PhpValue::Object(ref o) = val {
        if o.borrow().class_name == "Response" {
            o.borrow().get_property("content").map(|v| v.to_php_string()).unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let mut http_resp = format!("{}\r\n", status_line);
    for (k, v) in &resp.headers {
        http_resp.push_str(&format!("{}: {}\r\n", k, v));
    }
    http_resp.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        content.len(),
        content
    ));

    assert!(http_resp.starts_with("HTTP/1.1 201 Created\r\n"));
    assert!(http_resp.contains("Content-Type: application/json; charset=utf-8\r\n"));
    assert!(http_resp.contains("X-Inertia: true\r\n"));
    assert!(http_resp.contains("Location: /home\r\n"));
    assert!(http_resp.ends_with("{\"auth\":true}"));
}
