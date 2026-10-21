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

pub async fn run_script_async(
    code: String,
    project_path: String,
    rhp_path: String,
    cancelled: Arc<AtomicBool>,
) -> Result<String, String> {
    let bin_path = resolve_rhp_binary(&rhp_path);
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!(
        "rhp_exec_{}_{}.rhp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));

    // Write temp script synchronously
    if let Err(e) = fs::write(&temp_file, code.as_bytes()) {
        return Err(format!("Failed to create temporary script file: {}", e));
    }

    let mut cmd = Command::new(&bin_path);
    cmd.arg("run")
        .arg("dev")
        .arg("--cli")
        .arg(&temp_file);

    // Set working directory if valid
    let work_dir = Path::new(&project_path);
    if work_dir.exists() && work_dir.is_dir() {
        cmd.current_dir(work_dir);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => {
            // Graceful fallback: run directly inside the in-process RHP VM
            let _ = fs::remove_file(&temp_file);
            return match rhp::vm::run_source_with_vm_and_jit(&code, false) {
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
        }
    };

    // Monitor execution and cancellation
    let check_interval = std::time::Duration::from_millis(50);
    loop {
        if cancelled.load(Ordering::SeqCst) {
            let _ = child.kill().await;
            let _ = fs::remove_file(&temp_file);
            return Err("Execution cancelled by user.".to_string());
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
                return Err(format!("Process error: {}", e));
            }
        }
    }

    let output = match child.wait_with_output().await {
        Ok(out) => out,
        Err(e) => {
            let _ = fs::remove_file(&temp_file);
            return Err(format!("Failed to read process output: {}", e));
        }
    };

    let _ = fs::remove_file(&temp_file);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !stderr.is_empty() && stdout.is_empty() {
        Ok(stderr)
    } else if !stderr.is_empty() {
        Ok(format!("{}\n\n[STDERR]:\n{}", stdout, stderr))
    } else if stdout.is_empty() {
        Ok("(Script executed successfully with empty output)".to_string())
    } else {
        Ok(stdout)
    }
}
