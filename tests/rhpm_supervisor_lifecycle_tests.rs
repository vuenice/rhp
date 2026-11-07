use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU16, Ordering};

use rhp::runtime::supervisor::{
    format_uptime, get_host_metrics_line, load_state, print_pm2_table, save_state, visible_width,
    ProcessInfo, ProcessStatus,
};

static TEST_PORT: AtomicU16 = AtomicU16::new(9910);

fn next_port() -> u16 {
    TEST_PORT.fetch_add(1, Ordering::SeqCst)
}

#[test]
fn test_visible_string_width_and_ansi_handling() {
    assert_eq!(visible_width("hello"), 5);
    assert_eq!(visible_width("\x1b[1;32monline\x1b[0m"), 6);
    assert_eq!(visible_width("\x1b[1;31mstopped\x1b[0m"), 7);
    assert_eq!(visible_width("↺"), 1);
    assert_eq!(visible_width("\x1b[7;37mfork\x1b[0m"), 4);
}

#[test]
fn test_uptime_formatting() {
    assert_eq!(format_uptime(None), "-");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    assert_eq!(format_uptime(Some(now - 5)), "5s");
    assert_eq!(format_uptime(Some(now - 120)), "2m");
    assert_eq!(format_uptime(Some(now - 7200)), "2h");
    assert_eq!(format_uptime(Some(now - 86400 * 3)), "3d");
}

#[test]
fn test_host_metrics_line_generation() {
    let metrics = get_host_metrics_line();
    assert!(metrics.contains("host metrics"));
    assert!(metrics.contains("cpu:"));
    assert!(metrics.contains("mem:"));
    assert!(metrics.contains("disk:"));
}

#[test]
fn test_supervisor_state_roundtrip() {
    let temp_dir = std::env::temp_dir().join(format!("rhpm_test_{}", next_port()));
    let _ = std::fs::create_dir_all(&temp_dir);
    unsafe {
        std::env::set_var("RHPM_HOME", temp_dir.to_str().unwrap());
    }

    let mut state = load_state();
    assert_eq!(state.processes.len(), 0);

    state.processes.push(ProcessInfo {
        id: 0,
        name: "test-app".to_string(),
        script_path: "/tmp/index.rhp".to_string(),
        cwd: "/tmp".to_string(),
        port: 8080,
        host: "127.0.0.1".to_string(),
        pid: Some(12345),
        status: ProcessStatus::Online,
        mode: "fork".to_string(),
        version: "0.1.0".to_string(),
        user: "testuser".to_string(),
        created_at: 100000,
        started_at: Some(100000),
        restarts: 0,
        watching: false,
        log_file: "/tmp/out.log".to_string(),
        err_file: "/tmp/err.log".to_string(),
    });
    state.next_id = 1;

    save_state(&state).expect("Save state should succeed");

    let loaded = load_state();
    assert_eq!(loaded.processes.len(), 1);
    assert_eq!(loaded.processes[0].name, "test-app");
    assert_eq!(loaded.processes[0].port, 8080);
    assert_eq!(loaded.processes[0].status, ProcessStatus::Online);

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_pm2_table_rendering_does_not_panic() {
    let processes = vec![
        ProcessInfo {
            id: 0,
            name: "Worker".to_string(),
            script_path: "/app/index.rhp".to_string(),
            cwd: "/app".to_string(),
            port: 8080,
            host: "127.0.0.1".to_string(),
            pid: Some(40209),
            status: ProcessStatus::Online,
            mode: "fork".to_string(),
            version: "4.0.0".to_string(),
            user: "alex".to_string(),
            created_at: 1000,
            started_at: Some(1000),
            restarts: 0,
            watching: false,
            log_file: "/tmp/Worker-out.log".to_string(),
            err_file: "/tmp/Worker-err.log".to_string(),
        },
        ProcessInfo {
            id: 1,
            name: "HTTP-API".to_string(),
            script_path: "/app/api.rhp".to_string(),
            cwd: "/app".to_string(),
            port: 8081,
            host: "127.0.0.1".to_string(),
            pid: Some(40208),
            status: ProcessStatus::Online,
            mode: "cluster".to_string(),
            version: "4.0.0".to_string(),
            user: "alex".to_string(),
            created_at: 1000,
            started_at: Some(1000),
            restarts: 2,
            watching: true,
            log_file: "/tmp/API-out.log".to_string(),
            err_file: "/tmp/API-err.log".to_string(),
        },
        ProcessInfo {
            id: 2,
            name: "Worker-Legacy".to_string(),
            script_path: "/app/old.rhp".to_string(),
            cwd: "/app".to_string(),
            port: 8082,
            host: "127.0.0.1".to_string(),
            pid: None,
            status: ProcessStatus::Stopped,
            mode: "fork".to_string(),
            version: "4.0.0".to_string(),
            user: "alex".to_string(),
            created_at: 1000,
            started_at: None,
            restarts: 1,
            watching: false,
            log_file: "/tmp/Legacy-out.log".to_string(),
            err_file: "/tmp/Legacy-err.log".to_string(),
        },
    ];

    // Verify rendering all processes with PM2 layout executes cleanly
    print_pm2_table(&processes);
}

#[test]
fn test_cli_supervisor_lifecycle() {
    let port = next_port();
    let rhpm_home = std::env::temp_dir().join(format!("rhpm_cli_test_{}", port));
    let _ = std::fs::create_dir_all(&rhpm_home);

    let rhp_bin = PathBuf::from(env!("CARGO_BIN_EXE_rhp"));
    let rhpm_bin = PathBuf::from(env!("CARGO_BIN_EXE_rhpm"));

    // 1. `rhp status` with empty registry
    let out = Command::new(&rhp_bin)
        .arg("status")
        .env("RHPM_HOME", rhpm_home.to_str().unwrap())
        .output()
        .expect("rhp status should execute");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("id"));
    assert!(stdout.contains("name"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("host metrics"));

    // 2. `rhp start Worker --file="examples/todo/app.rhp"`
    let out = Command::new(&rhp_bin)
        .args(&[
            "start",
            "Worker",
            "--file=examples/todo/app.rhp",
            &format!("--port={}", port),
        ])
        .env("RHPM_HOME", rhpm_home.to_str().unwrap())
        .output()
        .expect("rhp start should execute");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Worker"));
    assert!(stdout.contains("online"));

    // 3. `rhpm status`
    let out = Command::new(&rhpm_bin)
        .arg("status")
        .env("RHPM_HOME", rhpm_home.to_str().unwrap())
        .output()
        .expect("rhpm status should execute");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Worker"));

    // 4. `rhp stop Worker`
    let out = Command::new(&rhp_bin)
        .args(&["stop", "Worker"])
        .env("RHPM_HOME", rhpm_home.to_str().unwrap())
        .output()
        .expect("rhp stop should execute");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("stopped"));

    // 5. `rhpm restart Worker`
    let out = Command::new(&rhpm_bin)
        .args(&["restart", "Worker"])
        .env("RHPM_HOME", rhpm_home.to_str().unwrap())
        .output()
        .expect("rhpm restart should execute");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Worker"));

    // 6. `rhp delete Worker`
    let out = Command::new(&rhp_bin)
        .args(&["delete", "Worker"])
        .env("RHPM_HOME", rhpm_home.to_str().unwrap())
        .output()
        .expect("rhp delete should execute");
    assert!(out.status.success());

    // 7. Final status should be empty
    let out = Command::new(&rhp_bin)
        .arg("status")
        .env("RHPM_HOME", rhpm_home.to_str().unwrap())
        .output()
        .expect("rhp status should execute");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("No active applications registered"));

    let _ = std::fs::remove_dir_all(rhpm_home);
}
