use std::fs;
use std::process::Command;

#[test]
fn test_cli_run_dev_defaults_to_index_rhp_in_pwd() {
    let temp_dir = std::env::temp_dir().join("rhp_cli_test_dev_pwd");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let index_file = temp_dir.join("index.rhp");
    fs::write(&index_file, "echo 'Hello from index.rhp in pwd!';\n")
        .expect("Failed to write index.rhp");

    let exe_path = env!("CARGO_BIN_EXE_rhp");
    let output = Command::new(exe_path)
        .arg("run")
        .arg("dev")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to run rhp binary");

    assert!(output.status.success(), "Command failed: {:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "Hello from index.rhp in pwd!");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_run_build_defaults_to_index_rhp_in_pwd() {
    let temp_dir = std::env::temp_dir().join("rhp_cli_test_build_pwd");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let index_file = temp_dir.join("index.rhp");
    fs::write(&index_file, "echo 'Hello from build index.rhp!';\n")
        .expect("Failed to write index.rhp");

    let exe_path = env!("CARGO_BIN_EXE_rhp");
    let output = Command::new(exe_path)
        .arg("run")
        .arg("build")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to run rhp binary");

    assert!(output.status.success(), "Command failed: {:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "Hello from build index.rhp!");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_run_missing_index_rhp_in_pwd_fails_gracefully() {
    let temp_dir = std::env::temp_dir().join("rhp_cli_test_empty_dir");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let exe_path = env!("CARGO_BIN_EXE_rhp");
    let output = Command::new(exe_path)
        .arg("run")
        .arg("dev")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to run rhp binary");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Error: Script file 'index.rhp' does not exist."),
        "Expected missing index.rhp error, got: {}",
        stderr
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_run_build_with_file_equals_flag() {
    let temp_dir = std::env::temp_dir().join("rhp_cli_test_file_flag");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let custom_file = temp_dir.join("custom.rhp");
    fs::write(&custom_file, "echo 'Server started successfully!';\n")
        .expect("Failed to write custom.rhp");

    let exe_path = env!("CARGO_BIN_EXE_rhp");

    // Test --file="/your-path/custom.rhp"
    let file_arg = format!("--file={}", custom_file.to_str().unwrap());
    let output = Command::new(exe_path)
        .arg("run")
        .arg("build")
        .arg(&file_arg)
        .output()
        .expect("Failed to run rhp binary");

    assert!(output.status.success(), "Command failed: {:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "Server started successfully!");

    // Test --file="/your-path/custom.rhp" with quoted string inside arg
    let file_arg_quoted = format!("--file=\"{}\"", custom_file.to_str().unwrap());
    let output_quoted = Command::new(exe_path)
        .arg("run")
        .arg("dev")
        .arg(&file_arg_quoted)
        .output()
        .expect("Failed to run rhp binary");

    assert!(output_quoted.status.success(), "Command failed: {:?}", output_quoted);
    let stdout_quoted = String::from_utf8_lossy(&output_quoted.stdout);
    assert_eq!(stdout_quoted.trim(), "Server started successfully!");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_run_dev_with_file_space_and_short_flag() {
    let temp_dir = std::env::temp_dir().join("rhp_cli_test_space_flag");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let app_file = temp_dir.join("app.rhp");
    fs::write(&app_file, "echo 'App running!';\n").expect("Failed to write app.rhp");

    let exe_path = env!("CARGO_BIN_EXE_rhp");

    // Test --file app.rhp
    let output = Command::new(exe_path)
        .arg("run")
        .arg("dev")
        .arg("--file")
        .arg(app_file.to_str().unwrap())
        .output()
        .expect("Failed to run rhp binary");

    assert!(output.status.success(), "Command failed: {:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "App running!");

    // Test -f app.rhp
    let output_f = Command::new(exe_path)
        .arg("run")
        .arg("build")
        .arg("-f")
        .arg(app_file.to_str().unwrap())
        .output()
        .expect("Failed to run rhp binary");

    assert!(output_f.status.success(), "Command failed: {:?}", output_f);
    let stdout_f = String::from_utf8_lossy(&output_f.stdout);
    assert_eq!(stdout_f.trim(), "App running!");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_run_with_positional_path_fallback() {
    let temp_dir = std::env::temp_dir().join("rhp_cli_test_positional");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let custom_file = temp_dir.join("custom.rhp");
    fs::write(&custom_file, "echo 'Custom file executed!';\n").expect("Failed to write custom.rhp");

    let exe_path = env!("CARGO_BIN_EXE_rhp");

    let output = Command::new(exe_path)
        .arg("run")
        .arg("dev")
        .arg(custom_file.to_str().unwrap())
        .output()
        .expect("Failed to run rhp binary");

    assert!(output.status.success(), "Command failed: {:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "Custom file executed!");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_run_help_dialog() {
    let exe_path = env!("CARGO_BIN_EXE_rhp");

    let output_dev = Command::new(exe_path)
        .arg("run")
        .arg("dev")
        .arg("--help")
        .output()
        .expect("Failed to run rhp binary");

    assert!(output_dev.status.success());
    let stdout_dev = String::from_utf8_lossy(&output_dev.stdout);
    assert!(stdout_dev.contains("rhp run dev"));
    assert!(stdout_dev.contains("--file"));

    let output_build = Command::new(exe_path)
        .arg("run")
        .arg("build")
        .arg("-h")
        .output()
        .expect("Failed to run rhp binary");

    assert!(output_build.status.success());
    let stdout_build = String::from_utf8_lossy(&output_build.stdout);
    assert!(stdout_build.contains("rhp run build"));
    assert!(stdout_build.contains("--file"));
}

#[test]
fn test_cli_dev_and_build_top_level_shorthands() {
    let temp_dir = std::env::temp_dir().join("rhp_cli_test_shorthands");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let script = temp_dir.join("test.rhp");
    fs::write(&script, "echo 'Testing shorthand execution!';\n")
        .expect("Failed to write test.rhp");

    let exe_path = env!("CARGO_BIN_EXE_rhp");

    // Test `rhp dev test.rhp`
    let out_dev = Command::new(exe_path)
        .arg("dev")
        .arg(&script)
        .output()
        .expect("Failed to run rhp binary");
    assert!(out_dev.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out_dev.stdout).trim(),
        "Testing shorthand execution!"
    );

    // Test `rhp build test.rhp`
    let out_build = Command::new(exe_path)
        .arg("build")
        .arg(&script)
        .output()
        .expect("Failed to run rhp binary");
    assert!(out_build.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out_build.stdout).trim(),
        "Testing shorthand execution!"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_directory_target_resolution() {
    let temp_dir = std::env::temp_dir().join("rhp_cli_test_dir_resolution");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let app_file = temp_dir.join("app.rhp");
    fs::write(&app_file, "echo 'Resolved app.rhp inside directory!';\n")
        .expect("Failed to write app.rhp");

    let exe_path = env!("CARGO_BIN_EXE_rhp");

    let output = Command::new(exe_path)
        .arg("run")
        .arg("dev")
        .arg(&temp_dir)
        .output()
        .expect("Failed to run rhp binary");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "Resolved app.rhp inside directory!"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_run_dev_web_app_starts_server_with_port() {
    use std::io::{BufRead, BufReader};

    let temp_dir = std::env::temp_dir().join("rhp_cli_test_web_app_server");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let app_file = temp_dir.join("app.rhp");
    fs::write(
        &app_file,
        "global $_SERVER;\necho '<h1>Hello from web app</h1>';\n",
    )
    .expect("Failed to write app.rhp");

    let test_port = 18000 + (std::process::id() % 10000) as u16;
    let port_str = test_port.to_string();

    let exe_path = env!("CARGO_BIN_EXE_rhp");

    let mut child = Command::new(exe_path)
        .arg("run")
        .arg("dev")
        .arg(&temp_dir)
        .arg("--port")
        .arg(&port_str)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn rhp binary");

    let stdout = child.stdout.take().expect("Child stdout missing");
    let reader = BufReader::new(stdout);
    let mut banner_lines = Vec::new();

    for line in reader.lines() {
        if let Ok(l) = line {
            let reached_end = l.contains("Press Ctrl+C to terminate the server");
            banner_lines.push(l);
            if reached_end {
                break;
            }
        } else {
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    let combined = banner_lines.join("\n");
    assert!(
        combined.contains("RHP Development Server"),
        "Banner should mention Development Server: {}",
        combined
    );
    assert!(
        combined.contains(&format!("Listening on: http://127.0.0.1:{}", test_port)),
        "Banner should show listening port {}: {}",
        test_port,
        combined
    );

    let _ = fs::remove_dir_all(&temp_dir);
}


