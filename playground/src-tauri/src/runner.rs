use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::process::Command;

#[derive(Clone)]
pub struct CancelHandle {
    cancelled: Arc<AtomicBool>,
}

impl CancelHandle {
    pub fn new() -> (Self, Arc<AtomicBool>) {
        let flag = Arc::new(AtomicBool::new(false));
        (
            Self {
                cancelled: flag.clone(),
            },
            flag,
        )
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    #[allow(dead_code)]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub fn resolve_rhp_binary(custom_path: &str) -> PathBuf {
    if !custom_path.trim().is_empty() {
        let p = PathBuf::from(custom_path.trim());
        if p.exists() {
            return p;
        }
    }

    // Check sibling binary in the same directory as rhp-play (e.g. target/debug/, target/release/, ~/.cargo/bin/)
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let sibling = parent.join("rhp");
            if sibling.exists() {
                return sibling;
            }
            let sibling_exe = parent.join("rhp.exe");
            if sibling_exe.exists() {
                return sibling_exe;
            }

            // Also check relative to playground/target/{debug,release} -> root target/{debug,release}
            let rel_candidates = [
                parent.join("../../../target/debug/rhp"),
                parent.join("../../../target/release/rhp"),
                parent.join("../../../target/debug/rhp.exe"),
                parent.join("../../../target/release/rhp.exe"),
            ];
            for p in &rel_candidates {
                if p.exists() {
                    return p.clone();
                }
            }
        }
    }

    let candidates = [
        "target/debug/rhp",
        "target/release/rhp",
        "../target/debug/rhp",
        "../target/release/rhp",
        "rhp",
        "rhp.exe",
    ];

    for c in &candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return p;
        }
    }

    PathBuf::from("rhp")
}

static TEMP_FILE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunResponse {
    pub formatted_code: Option<String>,
    pub result: Result<String, String>,
}

pub async fn run_script_async(
    code: String,
    project_path: String,
    rhp_path: String,
    cancelled: Arc<AtomicBool>,
) -> RunResponse {
    let bin_path = resolve_rhp_binary(&rhp_path);
    let temp_dir = std::env::temp_dir();
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_file = temp_dir.join(format!(
        "rhp_exec_{}_{}_{}.rhp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        counter
    ));

    // Write temp script synchronously
    if let Err(e) = fs::write(&temp_file, code.as_bytes()) {
        return RunResponse {
            formatted_code: None,
            result: Err(format!("Failed to create temporary script file: {}", e)),
        };
    }

    let work_dir = Path::new(&project_path);

    // 1. Automatically run "rhp run lint" to format the script
    let mut formatted_code = None;
    let mut lint_cmd = Command::new(&bin_path);
    lint_cmd.arg("run").arg("lint").arg(&temp_file);

    if work_dir.exists() && work_dir.is_dir() {
        lint_cmd.current_dir(work_dir);
    }
    lint_cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    match lint_cmd.spawn() {
        Ok(mut lint_child) => {
            let check_interval = std::time::Duration::from_millis(20);
            loop {
                if cancelled.load(Ordering::SeqCst) {
                    let _ = lint_child.kill().await;
                    let _ = fs::remove_file(&temp_file);
                    return RunResponse {
                        formatted_code: None,
                        result: Err("Execution cancelled by user.".to_string()),
                    };
                }
                match lint_child.try_wait() {
                    Ok(Some(_status)) => break,
                    Ok(None) => tokio::time::sleep(check_interval).await,
                    Err(_) => break,
                }
            }
            if let Ok(new_code) = fs::read_to_string(&temp_file) {
                if !new_code.is_empty() {
                    formatted_code = Some(new_code);
                }
            }
        }
        Err(_) => {
            // Fallback: run in-process rhp linter if binary spawn failed
            let lint_res = rhp::lint::lint_source(&code, "playground.rhp");
            let _ = fs::write(&temp_file, lint_res.formatted_content.as_bytes());
            formatted_code = Some(lint_res.formatted_content);
        }
    }

    if cancelled.load(Ordering::SeqCst) {
        let _ = fs::remove_file(&temp_file);
        return RunResponse {
            formatted_code,
            result: Err("Execution cancelled by user.".to_string()),
        };
    }

    // 2. Run the script with `rhp run dev --cli`
    let mut cmd = Command::new(&bin_path);
    cmd.arg("run")
        .arg("dev")
        .arg("--cli")
        .arg(&temp_file);

    if work_dir.exists() && work_dir.is_dir() {
        cmd.current_dir(work_dir);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => {
            // Graceful fallback: run directly inside the in-process RHP VM
            let _ = fs::remove_file(&temp_file);
            let run_code = formatted_code.as_deref().unwrap_or(&code);
            let res = match rhp::vm::run_source_with_vm_and_jit(run_code, false) {
                Ok((vm, val)) => {
                    let out = vm.output();
                    if !out.is_empty() {
                        Ok(out.to_string())
                    } else if val != rhp::PhpValue::Null {
                        Ok(val.to_php_string())
                    } else {
                        Ok("(Script executed successfully with empty output)".to_string())
                    }
                }
                Err(e) => Err(format!("Runtime error: {}", e)),
            };
            return RunResponse {
                formatted_code,
                result: res,
            };
        }
    };

    // Monitor execution and cancellation
    let check_interval = std::time::Duration::from_millis(50);
    loop {
        if cancelled.load(Ordering::SeqCst) {
            let _ = child.kill().await;
            let _ = fs::remove_file(&temp_file);
            return RunResponse {
                formatted_code,
                result: Err("Execution cancelled by user.".to_string()),
            };
        }

        match child.try_wait() {
            Ok(Some(_status)) => {
                break;
            }
            Ok(None) => {
                tokio::time::sleep(check_interval).await;
            }
            Err(e) => {
                let _ = fs::remove_file(&temp_file);
                return RunResponse {
                    formatted_code,
                    result: Err(format!("Process error: {}", e)),
                };
            }
        }
    }

    let output = match child.wait_with_output().await {
        Ok(out) => out,
        Err(e) => {
            let _ = fs::remove_file(&temp_file);
            return RunResponse {
                formatted_code,
                result: Err(format!("Failed to read process output: {}", e)),
            };
        }
    };

    let _ = fs::remove_file(&temp_file);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let res = if !stderr.is_empty() && stdout.is_empty() {
        Ok(stderr)
    } else if !stderr.is_empty() {
        Ok(format!("{}\n\n[STDERR]:\n{}", stdout, stderr))
    } else if stdout.is_empty() {
        Ok("(Script executed successfully with empty output)".to_string())
    } else {
        Ok(stdout)
    };

    RunResponse {
        formatted_code,
        result: res,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_script_async_lints_and_executes() {
        let unformatted_code = "$int_val=42;\n$str_val='test';\necho $int_val;\n";
        let (_, cancelled) = CancelHandle::new();
        let res = run_script_async(unformatted_code.to_string(), String::new(), String::new(), cancelled).await;

        assert!(res.formatted_code.is_some());
        let formatted = res.formatted_code.unwrap();
        assert!(formatted.contains("int $int_val = 42;"));
        assert!(formatted.contains("string $str_val = 'test';"));

        assert!(res.result.is_ok());
        let output = res.result.unwrap();
        assert_eq!(output.trim(), "42");
    }

    #[tokio::test]
    async fn test_run_script_async_initial_screen_code() {
        let code = crate::models::default_standalone_code();
        let (_, cancelled) = CancelHandle::new();
        let res = run_script_async(code, String::new(), String::new(), cancelled).await;

        assert!(res.formatted_code.is_some());
        let formatted = res.formatted_code.unwrap();
        assert!(formatted.contains("int $int_value = 1;"));
        assert!(formatted.contains("float $float_value = 1.1;"));
        assert!(formatted.contains("bool $bool_value = true;"));
        assert!(formatted.contains("string $string_value = 'hello world';"));
        assert!(formatted.contains("dict $arr_dict = ["));
        assert!(formatted.contains("list $arr_list = ["));

        assert!(res.result.is_ok());
        let output = res.result.unwrap();
        assert!(output.contains("RHP Playground"));
        assert!(output.contains("hello world"));
    }
}
