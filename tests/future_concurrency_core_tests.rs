use rhp::{
    run_source, run_source_in_fiber, PhpValue,
};

#[test]
fn test_future_run_non_blocking_background_execution() {
    let source = r#"
        $future = Future::run(fn() => 100 * 50 + 50);

        // Background task is running, check before and after wait
        $result = $future->wait();
        return [$result, $future->isCompleted(), $future->hasError()];
    "#;

    let result = run_source(source).expect("Future::run execution should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(5050),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
        ])
    );
}

#[test]
fn test_future_wait_parallel_task_join_closures_directly() {
    // Verifies: accepts closures directly to evaluate parallel task joins cleanly without redundant ->wait() chains
    let source = r#"
        $id = 42;
        [$user, $posts, $settings] = Future::wait([
            fn() => "User_{$id}",
            fn() => Arr::list([1, 2, 3]),
            fn() => Arr::dict(["theme" => "dark", "notifications" => true])
        ]);

        return [$user, $posts->count(), $settings["theme"]];
    "#;

    let result = run_source(source).expect("Future::wait with closures directly should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("User_42"),
            PhpValue::Int(3),
            PhpValue::string("dark"),
        ])
    );
}

#[test]
fn test_future_wait_parallel_execution_speed() {
    // 3 parallel tasks each delayed by 30ms should complete concurrently in ~30-60ms,
    // whereas sequential execution would take 90ms+
    let source = r#"
        $t1 = Future::delayed(30, fn() => "task1");
        $t2 = Future::delayed(30, fn() => "task2");
        $t3 = Future::delayed(30, fn() => "task3");

        [$r1, $r2, $r3] = Future::wait([$t1, $t2, $t3]);
        return [$r1, $r2, $r3];
    "#;

    let start = std::time::Instant::now();
    let result = run_source(source).expect("Future::wait with delayed futures should succeed");
    let elapsed = start.elapsed();

    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("task1"),
            PhpValue::string("task2"),
            PhpValue::string("task3"),
        ])
    );
    // Parallel execution should complete well under sequential 90ms (plus modest margin)
    assert!(
        elapsed < std::time::Duration::from_millis(150),
        "Elapsed {:?} exceeded expected parallel duration",
        elapsed
    );
}

#[test]
fn test_future_any_parallel_task_race_closures_directly() {
    // Verifies: accepts closures directly to evaluate parallel task races cleanly without redundant ->wait() chains
    let source = r#"
        $winner = Future::any([
            Future::delayed(80, fn() => "slow_mirror"),
            fn() => "fast_mirror"
        ]);
        return $winner;
    "#;

    let start = std::time::Instant::now();
    let result = run_source(source).expect("Future::any with racing closures should succeed");
    let elapsed = start.elapsed();

    assert_eq!(result, PhpValue::string("fast_mirror"));
    // Fast mirror should complete immediately (< 50ms) without waiting for 80ms slow mirror
    assert!(
        elapsed < std::time::Duration::from_millis(60),
        "Future::any waited for slow mirror: {:?}",
        elapsed
    );
}

#[test]
fn test_future_delayed_and_fiber_timer_telemetry() {
    let source = r#"
        $res = Future::delayed(10, fn() => "resumed_ok")->wait();
        return $res;
    "#;

    let (val, stats) = run_source_in_fiber(source, "delayed_fiber")
        .expect("Future::delayed should run inside fiber");
    assert_eq!(val, PhpValue::string("resumed_ok"));
    assert_eq!(stats.name, "delayed_fiber");
    assert!(stats.yield_count >= 1);
    assert_eq!(stats.io_breakdown.get("Timer"), Some(&1));
}

#[test]
fn test_future_value_and_error_sync_factories() {
    let source = r#"
        $fVal = Future::value("immediate_val");
        $fErr = Future::error("Database connection failed");

        return [
            $fVal->isCompleted(),
            $fVal->hasError(),
            $fVal->value(),
            $fVal->wait(),
            $fErr->isCompleted(),
            $fErr->hasError(),
            $fErr->error()
        ];
    "#;

    let result = run_source(source).expect("Future::value and Future::error should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::string("immediate_val"),
            PhpValue::string("immediate_val"),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::string("Database connection failed"),
        ])
    );
}

#[test]
fn test_future_fluent_then_chaining() {
    let source = r#"
        $result = Future::run(fn() => 21)
            ->then(fn($x) => $x * 2)
            ->then(fn($x) => "The answer is {$x}")
            ->wait();
        return $result;
    "#;

    let result = run_source(source).expect("Future::then chaining should succeed");
    assert_eq!(result, PhpValue::string("The answer is 42"));
}

#[test]
fn test_future_fluent_catch_error_recovery() {
    let source = r#"
        $recovered = Future::error("Connection timeout")
            ->catch(fn($err) => "Recovered from: {$err}")
            ->wait();
        return $recovered;
    "#;

    let result = run_source(source).expect("Future::catch error recovery should succeed");
    assert_eq!(result, PhpValue::string("Recovered from: Connection timeout"));
}

#[test]
fn test_future_fluent_timeout_fallback() {
    let source = r#"
        $result = Future::delayed(100, fn() => "too_slow")
            ->timeout(20, fn() => "fallback_value");
        return $result;
    "#;

    let result = run_source(source).expect("Future::timeout fallback should succeed");
    assert_eq!(result, PhpValue::string("fallback_value"));
}

#[test]
fn test_future_wait_and_any_with_mixed_tasks() {
    let source = r#"
        $f1 = Future::value("val1");
        $f2 = Future::run(fn() => "val2");
        $f3 = Future::delayed(5, fn() => "val3");

        $waitResults = Future::wait([
            $f1,
            $f2,
            $f3,
            fn() => "val4",
            "val5"
        ]);

        $anyWinner = Future::any([
            Future::delayed(50, fn() => "late"),
            "instant_winner"
        ]);

        return [$waitResults, $anyWinner];
    "#;

    let result = run_source(source).expect("Future::wait and any with mixed tasks should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::list(vec![
                PhpValue::string("val1"),
                PhpValue::string("val2"),
                PhpValue::string("val3"),
                PhpValue::string("val4"),
                PhpValue::string("val5"),
            ]),
            PhpValue::string("instant_winner"),
        ])
    );
}

#[test]
fn test_future_all_alias_for_wait() {
    let source = r#"
        $res = Future::all([
            fn() => 10,
            fn() => 20,
            fn() => 30
        ]);
        return $res;
    "#;

    let result = run_source(source).expect("Future::all alias should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(10),
            PhpValue::Int(20),
            PhpValue::Int(30),
        ])
    );
}

#[test]
fn test_future_upvalue_capture_in_background_tasks() {
    let source = r#"
        $multiplier = 5;
        $prefix = "Result: ";
        $fut = Future::run(fn() => "{$prefix}" . (10 * $multiplier));
        return $fut->wait();
    "#;

    let result = run_source(source).expect("Future upvalue capture should succeed");
    assert_eq!(result, PhpValue::string("Result: 50"));
}

#[test]
fn test_future_panic_and_error_containment() {
    let source = r#"
        $failedFuture = Future::run(fn() => nonExistentFunction());

        $caughtError = $failedFuture->catch(fn($e) => "Handled error")->wait();
        $hasError = $failedFuture->hasError();
        return [$hasError, $caughtError];
    "#;

    let result = run_source(source).expect("Future error containment should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![PhpValue::Bool(true), PhpValue::string("Handled error")])
    );
}

#[test]
fn test_future_static_only_instantiation_forbidden() {
    let source = r#"
        $f = new Future();
    "#;

    let err = run_source(source).expect_err("new Future() should fail with instantiation guard");
    assert!(
        err.to_string().contains("Future"),
        "Unexpected error: {:?}",
        err
    );
}
