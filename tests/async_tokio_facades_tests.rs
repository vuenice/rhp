use rhp::{run_source, FutureState, PhpValue, VmFuture};

#[test]
fn test_rust_future_state_enum() {
    assert_eq!(FutureState::Pending.as_str(), "Pending");
    assert_eq!(FutureState::Ready.as_str(), "Ready");

    let mut fut_resolved = VmFuture::resolved(PhpValue::Int(123));
    assert_eq!(fut_resolved.state(), FutureState::Ready);
    assert!(fut_resolved.is_ready());
    assert!(!fut_resolved.is_pending());

    let (_tx, rx) = std::sync::mpsc::channel();
    let mut fut_pending = VmFuture::pending(rx);
    assert_eq!(fut_pending.state(), FutureState::Pending);
    assert!(fut_pending.is_pending());
    assert!(!fut_pending.is_ready());
}

#[test]
fn test_afile_tokio_async_non_blocking_and_future_state() {
    let test_path = format!("/tmp/rhp_test_afile_{}.txt", std::process::id());
    let source = format!(
        r#"
        $path = "{test_path}";
        // 1. Asynchronously write file on Tokio
        $writeFut = AFile::write($path, "Hello Async Tokio File!");
        
        // Before wait, future state is Pending or Ready (runs asynchronously on Tokio worker pool)
        $initState = $writeFut->state;
        $initPending = $writeFut->isPending();
        $initReady = $writeFut->isReady();
        
        // Await/wait for write
        $writeOk = $writeFut->wait();
        $postState = $writeFut->state();
        $postReady = $writeFut->isReady();

        // 2. Asynchronously check existence
        $existsFut = AFile::exists($path);
        $exists = $existsFut->wait();

        // 3. Asynchronously read file
        $readFut = AFile::read($path);
        $content = $readFut->wait();

        // 4. Asynchronously append
        $appendFut = AFile::append($path, " Appended.");
        $appendOk = $appendFut->wait();

        // 5. Asynchronously check size
        $sizeFut = AFile::size($path);
        $size = $sizeFut->wait();

        // 6. Asynchronously delete
        $delFut = AFile::delete($path);
        $delOk = $delFut->wait();

        return [
            $writeOk,
            $postState,
            $postReady,
            $exists,
            $content,
            $appendOk,
            $size > 20,
            $delOk
        ];
        "#
    );

    let result = run_source(&source).expect("AFile operations should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::string("Ready"),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::string("Hello Async Tokio File!"),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_ahttp_tokio_async_future_and_response() {
    let source = r#"
        $fut = AHTTP::get("https://api.example.com/tokio-data");

        // Verify non-blocking return of Future object
        $isFutureState = ($fut->state == "Pending" || $fut->state == "Ready");
        
        // Await the HTTP request
        $response = $fut->wait();
        $finalState = $fut->state();

        $status = $response->status();
        $ok = $response->ok();
        $body = $response->body();
        $headers = $response->headers();

        // Test withHeaders client builder
        $client = AHTTP::withHeaders(["Authorization" => "Bearer secret_tokio_token"]);
        $postFut = $client->post("https://api.example.com/submit", '{"msg":"hello"}');
        $postResp = $postFut->wait();
        $postBody = $postResp->body();

        return [
            $isFutureState,
            $finalState,
            $status,
            $ok,
            $body,
            $headers["X-Async-Engine"],
            $postBody
        ];
    "#;

    let result = run_source(source).expect("AHTTP operations should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::string("Ready"),
            PhpValue::Int(200),
            PhpValue::Bool(true),
            PhpValue::string("{\"status\":\"ok\"}"),
            PhpValue::string("tokio"),
            PhpValue::string("{\"msg\":\"hello\"}"),
        ])
    );
}

#[test]
fn test_adb_tokio_async_query_builder_and_records() {
    let source = r#"
        // 1. Test table query builder
        $query = ADB::table("users")->where("active", 1)->limit(10);
        $futRecords = $query->get();

        // Future state check
        $records = $futRecords->wait();
        $firstRec = $records[0];

        // 2. Test find by id
        $findFut = ADB::table("orders")->find(99);
        $order = $findFut->wait();

        // 3. Test insert & update
        $insFut = ADB::insert("logs", ["action" => "login"]);
        $insId = $insFut->wait();

        $updFut = ADB::update("users", ["last_seen" => "now"]);
        $updRows = $updFut->wait();

        // 4. Test select
        $selFut = ADB::select("SELECT * FROM products WHERE in_stock = 1");
        $products = $selFut->wait();

        return [
            $firstRec->table,
            $firstRec->id,
            $order->table,
            $order->id,
            $insId,
            $updRows,
            $products->count()
        ];
    "#;

    let result = run_source(source).expect("ADB operations should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("users"),
            PhpValue::Int(1),
            PhpValue::string("orders"),
            PhpValue::Int(99),
            PhpValue::Int(1),
            PhpValue::Int(1),
            PhpValue::Int(0),
        ])
    );
}

#[test]
fn test_future_state_inspection_and_poll_contract() {
    let source = r#"
        // 1. Immediately resolved future
        $f1 = Future::value(42);
        $s1 = $f1->state;
        $isPending1 = $f1->isPending();
        $isReady1 = $f1->isReady();
        $poll1 = $f1->poll();

        // 2. Delayed future
        $f2 = Future::delayed(10, fn() => "delayed_ok");
        // State should be Pending or Ready
        $val2 = $f2->wait();
        $s2 = $f2->state();
        $poll2 = $f2->poll();

        return [
            $s1,
            $isPending1,
            $isReady1,
            $poll1["state"],
            $poll1["value"],
            $val2,
            $s2,
            $poll2["state"]
        ];
    "#;

    let result = run_source(source).expect("Future state and poll contract should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("Ready"),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
            PhpValue::string("Ready"),
            PhpValue::Int(42),
            PhpValue::string("delayed_ok"),
            PhpValue::string("Ready"),
            PhpValue::string("Ready"),
        ])
    );
}

#[test]
fn test_async_facades_concurrent_composition_with_future_wait() {
    let test_file = format!("/tmp/rhp_test_composition_{}.txt", std::process::id());
    std::fs::write(&test_file, "Composition Test Content").unwrap();

    let source = format!(
        r#"
        $path = "{test_file}";

        // Concurrently run AFile, AHTTP, and ADB operations inside Future::wait
        [$fileData, $httpResp, $dbUser] = Future::wait([
            AFile::read($path),
            AHTTP::get("https://api.example.com/status"),
            ADB::table("users")->find(101)
        ]);

        AFile::delete($path)->wait();

        return [
            $fileData,
            $httpResp->status(),
            $httpResp->ok(),
            $dbUser->table,
            $dbUser->id
        ];
        "#
    );

    let result = run_source(&source).expect("Concurrent composition with Future::wait should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("Composition Test Content"),
            PhpValue::Int(200),
            PhpValue::Bool(true),
            PhpValue::string("users"),
            PhpValue::Int(101),
        ])
    );
}

#[test]
fn test_static_facade_instantiation_guards_on_new_async_facades() {
    let programs = [
        "new ADB();",
        "new AFile();",
        "new AHTTP();",
    ];

    for prog in programs {
        let err = run_source(prog).expect_err("Instantiating static async facade must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("Cannot instantiate static facade class"),
            "Expected static facade instantiation error, got: {}",
            msg
        );
    }
}
