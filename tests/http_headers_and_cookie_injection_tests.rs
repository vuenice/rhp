use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use rhp::{
    execute_isolated_request, ParentProcess, WorkerRequest, WorkerResponseStatus,
};

#[test]
fn test_incoming_headers_and_cookie_injection_in_worker_request() {
    let script = r#"<?php
$token = Request::bearerToken();
$inertia = Request::header('X-Inertia');
$cookieFoo = Request::cookie('foo');
$cookieSession = Cookie::get('session');
$serverAuth = Request::server('HTTP_AUTHORIZATION');
$serverInertia = Request::server('HTTP_X_INERTIA');
$serverCookie = Request::server('HTTP_COOKIE');

// Access inside function using static facades (no global keywords required)
function checkInsideFunction(): array {
    return [
        'func_cookie' => Cookie::get('foo'),
        'func_auth' => Request::server('HTTP_AUTHORIZATION'),
        'func_inertia' => Request::header('X-Inertia'),
    ];
}
$funcRes = checkInsideFunction();

return [
    'token' => $token,
    'inertia' => $inertia,
    'cookieFoo' => $cookieFoo,
    'cookieSession' => $cookieSession,
    'serverAuth' => $serverAuth,
    'serverInertia' => $serverInertia,
    'serverCookie' => $serverCookie,
    'func_cookie' => $funcRes['func_cookie'],
    'func_auth' => $funcRes['func_auth'],
    'func_inertia' => $funcRes['func_inertia'],
];
"#;

    let state = ParentProcess::boot(script).expect("Script compilation failed");

    let req = WorkerRequest::new(script)
        .with_header("Authorization", "Bearer secret-jwt-token-123")
        .with_header("X-Inertia", "true")
        .with_header("Cookie", "foo=bar; session=sess_abc987")
        .with_server("HTTP_AUTHORIZATION", "Bearer secret-jwt-token-123")
        .with_server("HTTP_X_INERTIA", "true")
        .with_server("HTTP_COOKIE", "foo=bar; session=sess_abc987")
        .with_cookie("foo", "bar")
        .with_cookie("session", "sess_abc987");

    assert_eq!(req.header("Authorization"), Some("Bearer secret-jwt-token-123"));
    assert_eq!(req.header("x-inertia"), Some("true"));
    assert_eq!(req.cookie("foo"), Some("bar"));
    assert_eq!(req.cookie("session"), Some("sess_abc987"));
    assert_eq!(req.server("HTTP_X_INERTIA"), Some("true"));

    let resp = execute_isolated_request(&state, &req, 0);
    assert_eq!(resp.status, WorkerResponseStatus::Success);

    let val = resp.to_php_value();
    if let rhp::PhpValue::Dict(d) = val {
        let b = d.borrow();
        assert_eq!(b.get("token").map(|v| v.to_php_string()), Some("secret-jwt-token-123".to_string()));
        assert_eq!(b.get("inertia").map(|v| v.to_php_string()), Some("true".to_string()));
        assert_eq!(b.get("cookieFoo").map(|v| v.to_php_string()), Some("bar".to_string()));
        assert_eq!(b.get("cookieSession").map(|v| v.to_php_string()), Some("sess_abc987".to_string()));
        assert_eq!(b.get("serverAuth").map(|v| v.to_php_string()), Some("Bearer secret-jwt-token-123".to_string()));
        assert_eq!(b.get("serverInertia").map(|v| v.to_php_string()), Some("true".to_string()));
        assert_eq!(b.get("serverCookie").map(|v| v.to_php_string()), Some("foo=bar; session=sess_abc987".to_string()));
        assert_eq!(b.get("func_cookie").map(|v| v.to_php_string()), Some("bar".to_string()));
        assert_eq!(b.get("func_auth").map(|v| v.to_php_string()), Some("Bearer secret-jwt-token-123".to_string()));
        assert_eq!(b.get("func_inertia").map(|v| v.to_php_string()), Some("true".to_string()));
    } else {
        panic!("Expected dict returned, got {:?}", val);
    }
}

#[test]
fn test_multiple_set_cookie_emission_from_response_methods() {
    let script = r#"<?php
$response = Response::json(['status' => 'ok']);
$response->cookie('user_id', '42', 3600, '/', 'example.com', true, true);
$response->withCookie('theme', 'dark');
$response->withoutCookie('old_session');
return $response;
"#;

    let state = ParentProcess::boot(script).expect("Script compilation failed");
    let req = WorkerRequest::new(script);
    let resp = execute_isolated_request(&state, &req, 0);
    assert_eq!(resp.status, WorkerResponseStatus::Success);

    let cookies = resp.cookies();
    assert_eq!(cookies.len(), 3, "Expected 3 Set-Cookie headers, got {:?}", cookies);

    assert!(cookies[0].starts_with("user_id=42"));
    assert!(cookies[0].contains("Path=/"));
    assert!(cookies[0].contains("Domain=example.com"));
    assert!(cookies[0].contains("Secure"));
    assert!(cookies[0].contains("HttpOnly"));

    assert!(cookies[1].starts_with("theme=dark"));
    assert!(cookies[1].contains("Path=/"));

    assert!(cookies[2].starts_with("old_session=deleted"));
    assert!(cookies[2].contains("Max-Age=0"));
    assert!(cookies[2].contains("1970"));
}

#[test]
fn test_multiple_set_cookie_emission_from_facades_and_legacy_php_functions() {
    let script = r#"<?php
Cookie::set('cart_id', 'cart_999', 120);
Cookie::forget('legacy_id');
setcookie('lang', 'es');
header('Set-Cookie: custom_flag=1; Path=/app');
header('X-Custom-Header: Active');
http_response_code(201);

echo "done";
"#;

    let state = ParentProcess::boot(script).expect("Script compilation failed");
    let req = WorkerRequest::new(script);
    let resp = execute_isolated_request(&state, &req, 0);
    assert_eq!(resp.status, WorkerResponseStatus::Success);
    assert_eq!(resp.status_code(), 201);
    assert_eq!(resp.header("X-Custom-Header"), Some("Active"));

    let cookies = resp.cookies();
    assert_eq!(cookies.len(), 4, "Expected 4 Set-Cookie headers, got {:?}", cookies);
    assert!(cookies.iter().any(|c| c.starts_with("cart_id=cart_999")));
    assert!(cookies.iter().any(|c| c.starts_with("legacy_id=") && c.contains("1970")));
    assert!(cookies.iter().any(|c| c.starts_with("lang=es")));
    assert!(cookies.iter().any(|c| c.starts_with("custom_flag=1")));
}

#[test]
fn test_http_server_connection_full_roundtrip() {
    let unique_id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("rhp_cookie_test_{}", unique_id));
    let _ = std::fs::create_dir_all(&temp_dir);
    let script_file = temp_dir.join("index.rhp");
    std::fs::write(
        &script_file,
        r#"<?php
$token = Request::bearerToken();
$inertia = Request::header('X-Inertia');
$userCookie = Request::cookie('user');
$sessionCookie = Cookie::get('session_id');

setcookie('visited', 'true');
header('Set-Cookie: tracker=xyz; Path=/');
header('X-Powered-By: RHP');

echo json_encode([
    'token' => $token,
    'inertia' => $inertia,
    'user' => $userCookie,
    'session' => $sessionCookie,
    'server_auth' => Request::server('HTTP_AUTHORIZATION'),
    'server_inertia' => Request::server('HTTP_X_INERTIA'),
]);
"#,
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let script_path = script_file.to_str().unwrap().to_string();

    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Call the same function run by rhp start / rhp serve
            // Note: `rhp start` forks `rhp serve`, both executing `serve_http_connection`.
            // We simulate the exact HTTP connection handler logic here.
            use std::io::{BufRead, BufReader};
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut req_line = String::new();
            reader.read_line(&mut req_line).unwrap();

            let parts: Vec<&str> = req_line.split_whitespace().collect();
            let method = parts[0].to_uppercase();
            let full_uri = parts[1].to_string();

            let mut raw_headers: Vec<(String, String)> = Vec::new();
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() || line == "\r\n" || line == "\n" || line.is_empty() {
                    break;
                }
                let trimmed = line.trim();
                if let Some((k, v)) = trimmed.split_once(':') {
                    let key = k.trim().to_string();
                    let val = v.trim().to_string();
                    if key.eq_ignore_ascii_case("content-length") {
                        content_length = val.parse().unwrap_or(0);
                    }
                    raw_headers.push((key, val));
                }
            }

            let mut body_bytes = vec![0u8; content_length];
            if content_length > 0 {
                let _ = reader.read_exact(&mut body_bytes);
            }

            let source = std::fs::read_to_string(&script_path).unwrap();
            let mut req = WorkerRequest::new(&source)
                .with_server("REQUEST_METHOD", &method)
                .with_server("REQUEST_URI", &full_uri)
                .with_server("SERVER_PORT", &port.to_string())
                .with_server("SERVER_NAME", "127.0.0.1");

            for (k, v) in &raw_headers {
                req = req.with_header(k.clone(), v.clone());
                let upper_k = k.to_ascii_uppercase().replace('-', "_");
                if upper_k == "CONTENT_TYPE" {
                    req = req.with_server("CONTENT_TYPE", v.clone());
                } else if upper_k == "CONTENT_LENGTH" {
                    req = req.with_server("CONTENT_LENGTH", v.clone());
                } else {
                    let cgi_name = format!("HTTP_{}", upper_k);
                    req = req.with_server(cgi_name, v.clone());
                }
                req = req.with_server(k.clone(), v.clone());

                if k.eq_ignore_ascii_case("cookie") {
                    for cookie_pair in v.split(';') {
                        let pair = cookie_pair.trim();
                        if let Some((c_name, c_val)) = pair.split_once('=') {
                            let c_name = c_name.trim();
                            let c_val = c_val.trim();
                            if !c_name.is_empty() {
                                req = req.with_cookie(c_name, c_val);
                            }
                        }
                    }
                }
            }

            let state = ParentProcess::boot(&source).unwrap();
            let resp = execute_isolated_request(&state, &req, 0);

            let status_code = resp.status_code;
            let status_line = format!("HTTP/1.1 {} OK\r\n", status_code);
            let mut http_resp = status_line;
            for (k, v) in &resp.headers {
                http_resp.push_str(&format!("{}: {}\r\n", k, v));
            }
            let body = resp.output();
            http_resp.push_str(&format!("Content-Length: {}\r\n\r\n{}", body.len(), body));
            stream.write_all(http_resp.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
    });

    let mut client = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    let request_data = "GET / HTTP/1.1\r\n\
Host: 127.0.0.1\r\n\
Authorization: Bearer test_auth_token_xyz\r\n\
X-Inertia: true\r\n\
Cookie: user=alice; session_id=session_987\r\n\
\r\n";
    client.write_all(request_data.as_bytes()).unwrap();
    client.flush().unwrap();

    let mut response_data = String::new();
    client.read_to_string(&mut response_data).unwrap();
    handle.join().unwrap();

    // Verify multiple Set-Cookie lines emitted
    let set_cookie_matches: Vec<&str> = response_data
        .lines()
        .filter(|line| line.to_ascii_lowercase().starts_with("set-cookie:"))
        .collect();

    assert_eq!(set_cookie_matches.len(), 2, "Expected 2 Set-Cookie header lines, got: {:?}", set_cookie_matches);
    assert!(set_cookie_matches.iter().any(|line| line.contains("visited=true")));
    assert!(set_cookie_matches.iter().any(|line| line.contains("tracker=xyz")));

    // Verify body parsed incoming headers correctly
    let body = response_data.split("\r\n\r\n").nth(1).unwrap();
    assert!(body.contains("\"token\":\"test_auth_token_xyz\""));
    assert!(body.contains("\"inertia\":\"true\""));
    assert!(body.contains("\"user\":\"alice\""));
    assert!(body.contains("\"session\":\"session_987\""));
    assert!(body.contains("\"server_auth\":\"Bearer test_auth_token_xyz\""));
    assert!(body.contains("\"server_inertia\":\"true\""));
}
