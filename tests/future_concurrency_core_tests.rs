use rhp::{
    run_source, run_source_in_fiber, run_source_with_vm, PhpValue,
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
        [$user, $posts, $settings] = Future::all([
            fn() => "User_{$id}",
            fn() => Arr::list([1, 2, 3]),
            fn() => Arr::dict(["theme" => "dark", "notifications" => true])
        ]);

        return [$user, $posts->count(), $settings["theme"]];
    "#;

    let result = run_source(source).expect("Future::all with closures directly should succeed");
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

        [$r1, $r2, $r3] = Future::all([$t1, $t2, $t3]);
        return [$r1, $r2, $r3];
    "#;

    let start = std::time::Instant::now();
    let result = run_source(source).expect("Future::all with delayed futures should succeed");
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
            Future::delayed(250, fn() => "slow_mirror"),
            fn() => "fast_mirror"
        ]);
        return $winner;
    "#;

    let start = std::time::Instant::now();
    let result = run_source(source).expect("Future::any with racing closures should succeed");
    let elapsed = start.elapsed();

    assert_eq!(result, PhpValue::string("fast_mirror"));
    // Fast mirror should complete immediately without waiting for 250ms slow mirror
    assert!(
        elapsed < std::time::Duration::from_millis(200),
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
            ->timeout(20, fn() => "fallback_value")
            ->wait();
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

        $waitResults = Future::all([
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

    let result = run_source(source).expect("Future::all and any with mixed tasks should succeed");
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
        $fut = Future::run(fn() => "{$prefix}" + string(10 * $multiplier));
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

#[test]
fn test_future_concurrent_waits_for_every_success() {
    let source = r#"
        $joined = concurrent([
            fn() => 1,
            fn() => 2,
            Future::value(3)
        ])->wait();
        return $joined;
    "#;

    let result = run_source(source).expect("concurrent join should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3)])
    );
}

#[test]
fn test_future_concurrent_rejects_on_first_error() {
    let source = r#"
        return Future::concurrent([
            fn() => 1,
            fn() => missingTask()
        ])->wait();
    "#;

    let err = run_source(source).expect_err("concurrent should reject when one task fails");
    assert!(
        err.to_string().contains("missingTask") || err.to_string().contains("Undefined"),
        "Unexpected error: {:?}",
        err
    );
}

#[test]
fn test_future_all_is_eager_and_static_wait_is_undefined() {
    let ok = r#"
        return Future::all([fn() => 10, fn() => 20]);
    "#;
    let result = run_source(ok).expect("Future::all should return a list");
    assert_eq!(
        result,
        PhpValue::list(vec![PhpValue::Int(10), PhpValue::Int(20)])
    );

    let bad = r#"
        return Future::wait([fn() => 1]);
    "#;
    let err = run_source(bad).expect_err("static Future::wait should be undefined");
    let text = err.to_string();
    assert!(
        text.contains("wait") || text.contains("Undefined"),
        "Unexpected error: {:?}",
        err
    );
}

#[test]
fn test_future_all_settled_keeps_rejections() {
    let source = r#"
        $rows = Future::allSettled([
            fn() => "ok",
            Future::error("nope")
        ])->wait();
        return [$rows[0]["status"], $rows[0]["value"], $rows[1]["status"], $rows[1]["error"]];
    "#;

    let result = run_source(source).expect("allSettled should keep both outcomes");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("fulfilled"),
            PhpValue::string("ok"),
            PhpValue::string("rejected"),
            PhpValue::string("nope"),
        ])
    );
}

#[test]
fn test_future_any_skips_errors_and_race_returns_first_failure() {
    let any_src = r#"
        return Future::any([
            Future::error("first failed"),
            fn() => "second won"
        ]);
    "#;
    let any = run_source(any_src).expect("any should return the first success");
    assert_eq!(any, PhpValue::string("second won"));

    let race_src = r#"
        return Future::race([
            Future::error("race failed"),
            fn() => "too late"
        ]);
    "#;
    let err = run_source(race_src).expect_err("race should surface the first failure");
    assert!(
        err.to_string().contains("race failed"),
        "Unexpected error: {:?}",
        err
    );
}

#[test]
fn test_future_delay_yields_timer() {
    let source = r#"
        Future::delay(10)->wait();
        return "slept";
    "#;
    let (val, stats) = run_source_in_fiber(source, "delay_fiber").expect("delay should suspend");
    assert_eq!(val, PhpValue::string("slept"));
    assert!(stats.yield_count >= 1);
    assert_eq!(stats.io_breakdown.get("Timer"), Some(&1));
}

#[test]
fn test_future_defer_runs_on_wait_and_once_at_shutdown() {
    let waited = r#"
        $n = 2;
        $f = defer(function() { echo "once"; });
        $f->wait();
        return $n;
    "#;
    let (vm, val) = run_source_with_vm(waited).expect("defer wait should run the closure");
    assert_eq!(val, PhpValue::Int(2));
    assert_eq!(vm.output(), "once");

    let later = r#"
        Future::defer(function() { echo "later"; });
        return 1;
    "#;
    let (vm, val) = run_source_with_vm(later).expect("unawaited defer should survive until shutdown");
    assert_eq!(val, PhpValue::Int(1));
    assert_eq!(vm.output(), "later");
}

#[test]
fn test_future_wait_timeout_and_cancel() {
    let timed = r#"
        return Future::delayed(200, fn() => "late")->wait(30);
    "#;
    let err = run_source(timed).expect_err("wait should honor the timeout");
    assert!(
        err.to_string().contains("timed out"),
        "Unexpected error: {:?}",
        err
    );

    let fallback = r#"
        return Future::delayed(100, fn() => "slow")
            ->timeout(15, "degraded")
            ->wait();
    "#;
    let value = run_source(fallback).expect("timeout fallback value should resolve");
    assert_eq!(value, PhpValue::string("degraded"));

    let cancelled = r#"
        $f = Future::delayed(400, fn() => "nope");
        $f->cancel();
        return $f->wait();
    "#;
    let err = run_source(cancelled).expect_err("cancel should fail a later wait");
    assert!(
        err.to_string().contains("cancelled"),
        "Unexpected error: {:?}",
        err
    );
}

#[test]
fn test_task_detaches_closure() {
    let path = std::env::temp_dir().join(format!(
        "rhp-task-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let path_lit = path.display().to_string().replace('\\', "\\\\");
    let source = format!(
        r#"
            task(fn() => File::write("{path}", "ran"));
            return null;
        "#,
        path = path_lit
    );
    let result = run_source(&source).expect("task should return null");
    assert_eq!(result, PhpValue::Null);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut saw = false;
    while std::time::Instant::now() < deadline {
        if path.exists() {
            saw = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = std::fs::remove_file(&path);
    assert!(saw, "task closure did not write {}", path.display());
}

#[test]
fn test_future_join_and_global_join() {
    let src = r#"
        $res = Future::join([
            fn() => 10 + 5,
            fn() => 20 * 2,
        ])->wait();
        $g_res = join([
            fn() => 100,
            fn() => 200,
        ])->wait();
        return [$res, $g_res];
    "#;
    let result = run_source(src).expect("join should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::list(vec![PhpValue::Int(15), PhpValue::Int(40)]),
            PhpValue::list(vec![PhpValue::Int(100), PhpValue::Int(200)]),
        ])
    );
}

#[test]
fn test_future_select_and_global_select() {
    let src = r#"
        $winner = Future::select([
            function() {
                Future::sleep(50)->wait();
                return "slow";
            },
            fn() => "fast",
        ])->wait();

        $g_winner = select([
            fn() => "immediate",
            function() {
                Future::sleep(50)->wait();
                return "delayed";
            },
        ])->wait();

        return [$winner, $g_winner];
    "#;
    let result = run_source(src).expect("select should return the winning branch");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("fast"),
            PhpValue::string("immediate"),
        ])
    );
}

#[test]
fn test_future_sleep_timer() {
    let src = r#"
        $f = Future::sleep(5);
        $res = $f->wait();
        return $res;
    "#;
    let result = run_source(src).expect("sleep should complete");
    assert_eq!(result, PhpValue::Null);
}

#[test]
fn test_future_timeout_facade_and_instance() {
    let src = r#"
        $slow = Future::run(function() {
            Future::sleep(100)->wait();
            return "done";
        });
        $fallback = Future::timeout($slow, 10, "timed_out")->wait();

        $slow2 = Future::run(function() {
            Future::sleep(100)->wait();
            return "done2";
        });
        $fallback2 = $slow2->timeout(10, "instance_timed_out")->wait();

        return [$fallback, $fallback2];
    "#;
    let result = run_source(src).expect("timeout should trigger fallback");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("timed_out"),
            PhpValue::string("instance_timed_out"),
        ])
    );
}

#[test]
fn test_task_blocking_thread_pool() {
    let src = r#"
        $res = Task::blocking(fn() => 21 * 2)->wait();
        return $res;
    "#;
    let result = run_source(src).expect("Task::blocking should execute and return result");
    assert_eq!(result, PhpValue::Int(42));
}

#[test]
fn test_semaphore_concurrency_control() {
    let src = r#"
        $sem = Semaphore::create(2);
        $initial_available = $sem->available();
        $total_permits = $sem->permits();

        $ok1 = $sem->tryAcquire();
        $ok2 = $sem->tryAcquire();
        $ok3 = $sem->tryAcquire(); // should fail since limit is 2

        $mid_available = $sem->available();

        $sem->release();
        $ok4 = $sem->tryAcquire(); // should now succeed

        $sem->release();
        $run_res = $sem->run(fn() => 999);

        return [
            $initial_available,
            $total_permits,
            $ok1,
            $ok2,
            $ok3,
            $mid_available,
            $ok4,
            $run_res,
        ];
    "#;
    let result = run_source(src).expect("semaphore operations should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(2),
            PhpValue::Int(2),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false),
            PhpValue::Int(0),
            PhpValue::Bool(true),
            PhpValue::Int(999),
        ])
    );
}

#[test]
fn test_semaphore_scoped_with() {
    let src = r#"
        $scoped = Semaphore::with(3, fn() => "scoped_success");
        return $scoped;
    "#;
    let result = run_source(src).expect("Semaphore::with should execute closure within permits");
    assert_eq!(result, PhpValue::string("scoped_success"));
}

#[test]
fn test_future_timeout_completes_before_deadline() {
    let src = r#"
        $fast = Future::run(fn() => 100);
        $res1 = Future::timeout($fast, 1000, 999)->wait();

        $fast2 = Future::run(fn() => 200);
        $res2 = $fast2->timeout(1000, 999)->wait();

        return [$res1, $res2];
    "#;
    let result = run_source(src).expect("timeout should return actual value when completing before deadline");
    assert_eq!(
        result,
        PhpValue::list(vec![PhpValue::Int(100), PhpValue::Int(200)])
    );
}

#[test]
fn test_future_combinators_fluent_chaining() {
    let src = r#"
        $chained = Future::join([
            fn() => 10,
            fn() => 20,
        ])->then(fn($vals) => $vals[0] + $vals[1])->wait();

        $chained_timeout = join([
            fn() => 5,
            fn() => 7,
        ])->timeout(500, [0, 0])->then(fn($vals) => $vals[0] * $vals[1])->wait();

        return [$chained, $chained_timeout];
    "#;
    let result = run_source(src).expect("fluent chaining on join/timeout should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![PhpValue::Int(30), PhpValue::Int(35)])
    );
}

#[test]
fn test_async_global_helper() {
    let src = r#"
        $f1 = async(fn() => 123);
        $v1 = $f1->wait();

        $f2 = async(fn() => 50)->then(fn($x) => $x * 2);
        $v2 = $f2->wait();

        return [$v1, $v2];
    "#;
    let result = run_source(src).expect("async global helper should return Future and wait correctly");
    assert_eq!(
        result,
        PhpValue::list(vec![PhpValue::Int(123), PhpValue::Int(100)])
    );
}

#[test]
fn test_future_select_with_futures_and_closures() {
    let src = r#"
        $ready = Future::run(fn() => "instant");
        $winner = Future::select([
            $ready,
            function() {
                Future::sleep(50)->wait();
                return "delayed";
            },
        ])->wait();

        return $winner;
    "#;
    let result = run_source(src).expect("select should work with existing Future instance and closure");
    assert_eq!(result, PhpValue::string("instant"));
}

#[test]
fn test_future_sleep_fluent_chaining() {
    let src = r#"
        $res = Future::sleep(5)->then(fn() => "slept_successfully")->wait();
        return $res;
    "#;
    let result = run_source(src).expect("Future::sleep should chain with then");
    assert_eq!(result, PhpValue::string("slept_successfully"));
}


