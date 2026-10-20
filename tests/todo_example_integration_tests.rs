use rhp::{execute_isolated_request, FrozenValue, ParentProcess, WorkerRequest};
use std::fs;

#[test]
fn test_todo_example_script_compilation_and_rendering() {
    let source = fs::read_to_string("examples/todo/app.rhp")
        .expect("examples/todo/app.rhp must exist and be readable");

    let state = ParentProcess::boot(&source).expect("examples/todo/app.rhp should boot");
    let req = WorkerRequest::new(&source);
    let resp = execute_isolated_request(&state, &req, 0);

    let output = resp.output();
    assert!(
        output.contains("TaskFlow"),
        "Rendered output should include 'TaskFlow', got:\n{}",
        output
    );
    assert!(
        output.contains("todo-list-container"),
        "Rendered output should contain todo-list-container HTMX target"
    );
    assert!(
        output.contains("hx-post=\"/todos\""),
        "Rendered output should include HTMX form action"
    );
    assert!(
        output.contains("Alpine"),
        "Rendered output should reference Alpine.js reactive components"
    );
}

#[test]
fn test_todo_example_json_api_mode() {
    let source = fs::read_to_string("examples/todo/app.rhp")
        .expect("examples/todo/app.rhp must exist");

    let state = ParentProcess::boot(&source).expect("examples/todo/app.rhp should boot");
    let req = WorkerRequest::new(&source)
        .with_server("REQUEST_METHOD", "GET")
        .with_server("REQUEST_URI", "/api/todos");
    let resp = execute_isolated_request(&state, &req, 0);

    let output = resp.output();
    assert!(
        output.contains("\"total\":"),
        "JSON output should contain total key: {}",
        output
    );
    assert!(
        output.contains("\"active\":"),
        "JSON output should contain active key: {}",
        output
    );
    assert!(
        output.contains("\"completed\":"),
        "JSON output should contain completed key: {}",
        output
    );
}

#[test]
fn test_todo_example_htmx_post_mode() {
    let source = fs::read_to_string("examples/todo/app.rhp")
        .expect("examples/todo/app.rhp must exist");

    let test_storage = "target/test_todos_temp.json";
    if std::path::Path::new(test_storage).exists() {
        let _ = std::fs::remove_file(test_storage);
    }

    let state = ParentProcess::boot(&source).expect("examples/todo/app.rhp should boot");
    let req = WorkerRequest::new(&source)
        .with_server("REQUEST_METHOD", "POST")
        .with_server("REQUEST_URI", "/todos")
        .with_server("HTTP_HX_REQUEST", "true")
        .with_server("TODOS_FILE", test_storage)
        .with_post("title", FrozenValue::String("Integration Test Task".to_string()))
        .with_post("category", FrozenValue::String("Engine".to_string()));
    let resp = execute_isolated_request(&state, &req, 0);

    let output = resp.output();
    assert!(
        output.contains("Integration Test Task"),
        "Output should include newly added todo title: {}",
        output
    );
    assert!(
        output.contains("id=\"todo-list-container\""),
        "Output should be the HTMX partial fragment without full html/body tags"
    );
    assert!(
        !output.contains("<!DOCTYPE html>"),
        "HTMX partial response must not contain full <!DOCTYPE html> wrapper"
    );
}
