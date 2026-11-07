//! # RHPM: PM2-Style Process Supervisor for RHP Applications
//!
//! Provides production lifecycle management, background daemon spawning,
//! status monitoring, graceful stop/restart, and terminal visualization
//! matching PM2.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const BORDER_COLOR: &str = "\x1b[90m";
const HEADER_COLOR: &str = "\x1b[1;36m";
const RESET: &str = "\x1b[0m";

/// Operational status of a managed process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessStatus {
    Online,
    Stopped,
    Errored,
}

impl ProcessStatus {
    pub fn as_ansi(&self) -> String {
        match self {
            ProcessStatus::Online => "\x1b[1;32monline\x1b[0m".to_string(),
            ProcessStatus::Stopped => "\x1b[1;31mstopped\x1b[0m".to_string(),
            ProcessStatus::Errored => "\x1b[1;33merrored\x1b[0m".to_string(),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessStatus::Online => "online",
            ProcessStatus::Stopped => "stopped",
            ProcessStatus::Errored => "errored",
        }
    }
}

/// Metadata and state for a single managed RHP application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub id: usize,
    pub name: String,
    pub script_path: String,
    pub cwd: String,
    pub port: u16,
    pub host: String,
    pub pid: Option<u32>,
    pub status: ProcessStatus,
    pub mode: String, // "fork" or "cluster"
    pub version: String,
    pub user: String,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub restarts: u32,
    pub watching: bool,
    pub log_file: String,
    pub err_file: String,
}

/// Overall persisted state of the supervisor registry.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RhpmState {
    pub processes: Vec<ProcessInfo>,
    pub next_id: usize,
}

/// Root directory used to store RHPM state, logs, and pid files.
pub fn get_rhpm_dir() -> PathBuf {
    let candidates = [
        std::env::var("RHPM_HOME").ok().map(PathBuf::from),
        std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".rhpm")),
        Some(std::env::temp_dir().join(".rhpm")),
        std::env::current_dir().ok().map(|c| c.join(".rhpm")),
    ];

    for candidate in candidates.into_iter().flatten() {
        if fs::create_dir_all(&candidate).is_ok()
            && fs::create_dir_all(candidate.join("logs")).is_ok()
            && fs::create_dir_all(candidate.join("pids")).is_ok()
        {
            let test_file = candidate.join(".write_test");
            if fs::write(&test_file, b"ok").is_ok() {
                let _ = fs::remove_file(test_file);
                return candidate;
            }
        }
    }

    PathBuf::from(".rhpm")
}

/// Load the current supervisor state from disk.
pub fn load_state() -> RhpmState {
    let state_file = get_rhpm_dir().join("state.json");
    if state_file.exists() {
        if let Ok(content) = fs::read_to_string(&state_file) {
            if let Ok(state) = serde_json::from_str::<RhpmState>(&content) {
                return state;
            }
        }
    }
    RhpmState {
        processes: Vec::new(),
        next_id: 0,
    }
}

/// Save current supervisor state to disk.
pub fn save_state(state: &RhpmState) -> Result<(), String> {
    let state_file = get_rhpm_dir().join("state.json");
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize state: {}", e))?;
    fs::write(&state_file, json).map_err(|e| format!("Failed to write state: {}", e))
}

/// Check if a process ID is actively running on the OS.
pub fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    unsafe {
        if libc::kill(pid as i32, 0) == 0 {
            return true;
        }
        let err = std::io::Error::last_os_error().raw_os_error();
        err == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Terminate an OS process gracefully with SIGTERM, falling back to SIGKILL.
pub fn terminate_pid(pid: u32) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(pid as i32, libc::SIGTERM);
        for _ in 0..15 {
            if !is_pid_alive(pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if is_pid_alive(pid) {
            let _ = libc::kill(pid as i32, libc::SIGKILL);
        }
    }
}

/// Synchronize the status of all registered processes against the OS process table.
pub fn sync_process_states(state: &mut RhpmState) {
    for proc in &mut state.processes {
        if proc.status == ProcessStatus::Online {
            match proc.pid {
                Some(pid) => {
                    if !is_pid_alive(pid) {
                        proc.status = ProcessStatus::Stopped;
                        proc.pid = None;
                    }
                }
                None => {
                    proc.status = ProcessStatus::Stopped;
                }
            }
        }
    }
}

/// Read resident set memory for a given process PID.
pub fn read_process_memory(pid: u32) -> (u64, String) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = fs::read_to_string(format!("/proc/{}/statm", pid)) {
            let parts: Vec<&str> = statm.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(pages) = parts[1].parse::<u64>() {
                    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
                    let ps = if page_size > 0 { page_size as u64 } else { 4096 };
                    let bytes = pages * ps;
                    let mb = bytes as f64 / (1024.0 * 1024.0);
                    return (bytes, format!("{:.1}mb", mb));
                }
            }
        }
    }
    (0, "0.0mb".to_string())
}

/// Read CPU percentage usage for a given process PID.
pub fn read_process_cpu(pid: u32, started_at: Option<u64>) -> (f64, String) {
    #[cfg(target_os = "linux")]
    {
        let uptime_secs = match started_at {
            Some(t) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                now.saturating_sub(t).max(1)
            }
            None => 1,
        };

        if let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", pid)) {
            if let Some(idx) = stat.rfind(')') {
                let rest = &stat[idx + 1..];
                let fields: Vec<&str> = rest.split_whitespace().collect();
                if fields.len() > 13 {
                    if let (Ok(utime), Ok(stime)) = (fields[11].parse::<u64>(), fields[12].parse::<u64>()) {
                        let clk_tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
                        let ticks = if clk_tck > 0 { clk_tck as f64 } else { 100.0 };
                        let total_time_sec = (utime + stime) as f64 / ticks;
                        let cpu = (total_time_sec / uptime_secs as f64) * 100.0;
                        let cpu_rounded = (cpu.min(100.0) * 10.0).round() / 10.0;
                        return (cpu_rounded, format!("{:.1}%", cpu_rounded));
                    }
                }
            }
        }
    }
    (0.0, "0.0%".to_string())
}

/// Format duration since start time into a compact string (e.g. "2s", "5m", "1d").
pub fn format_uptime(started_at: Option<u64>) -> String {
    let started = match started_at {
        Some(t) => t,
        None => return "-".to_string(),
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(started);
    if diff < 60 {
        format!("{}s", diff)
    } else if diff < 3600 {
        format!("{}m", diff / 60)
    } else if diff < 86400 {
        format!("{}h", diff / 3600)
    } else {
        format!("{}d", diff / 86400)
    }
}

/// Retrieve the operating system user running the process.
pub fn get_process_user(pid: u32) -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = fs::read_to_string(format!("/proc/{}/status", pid)) {
            for line in status.lines() {
                if line.starts_with("Uid:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(uid) = parts[1].parse::<libc::uid_t>() {
                            unsafe {
                                let pw = libc::getpwuid(uid);
                                if !pw.is_null() && !(*pw).pw_name.is_null() {
                                    if let Ok(s) = std::ffi::CStr::from_ptr((*pw).pw_name).to_str() {
                                        return s.to_string();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Compute host level metrics (cpu %, mem %, disk %) for the table footer.
pub fn get_host_metrics_line() -> String {
    let mut cpu_str = "0.0%".to_string();
    let mut mem_str = "0.0%".to_string();
    let mut disk_str = "/ 0%".to_string();

    #[cfg(target_os = "linux")]
    {
        if let Ok(loadavg) = fs::read_to_string("/proc/loadavg") {
            let parts: Vec<&str> = loadavg.split_whitespace().collect();
            if let Some(load1) = parts.first().and_then(|s| s.parse::<f64>().ok()) {
                let num_cpus = std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(1);
                let cpu_pct = ((load1 / num_cpus as f64) * 100.0).min(100.0);
                cpu_str = format!("{:.1}%", cpu_pct);
            }
        }

        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            let mut total_kb: Option<u64> = None;
            let mut avail_kb: Option<u64> = None;
            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    total_kb = line.split_whitespace().nth(1).and_then(|s| s.parse().ok());
                } else if line.starts_with("MemAvailable:") {
                    avail_kb = line.split_whitespace().nth(1).and_then(|s| s.parse().ok());
                }
            }
            if let (Some(total), Some(avail)) = (total_kb, avail_kb) {
                if total > 0 {
                    let used_pct = ((total.saturating_sub(avail)) as f64 / total as f64) * 100.0;
                    mem_str = format!("{:.1}%", used_pct);
                }
            }
        }

        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            let root_path = std::ffi::CString::new("/").unwrap();
            if libc::statvfs(root_path.as_ptr(), &mut stat) == 0 {
                let total = stat.f_blocks as f64 * stat.f_frsize as f64;
                let free = stat.f_bfree as f64 * stat.f_frsize as f64;
                if total > 0.0 {
                    let used_pct = ((total - free) / total * 100.0).round() as u64;
                    disk_str = format!("/ {}%", used_pct);
                }
            }
        }
    }

    format!(
        "\x1b[1;36mhost metrics\x1b[0m \x1b[90m|\x1b[0m \x1b[1;37mcpu:\x1b[0m \x1b[32m{}\x1b[0m \x1b[90m|\x1b[0m \x1b[1;37mmem:\x1b[0m \x1b[32m{}\x1b[0m \x1b[90m|\x1b[0m \x1b[1;37mdisk:\x1b[0m \x1b[32m{}\x1b[0m \x1b[90m|\x1b[0m",
        cpu_str, mem_str, disk_str
    )
}

/// Calculate visible string width ignoring ANSI color sequences and treating ↺ as width 1.
pub fn visible_width(s: &str) -> usize {
    let mut in_escape = false;
    let mut count = 0;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else {
            count += 1;
        }
    }
    count
}

/// Left or right pad a string with spaces to reach a visual width.
fn pad_cell(content: &str, width: usize, left_align: bool) -> String {
    let vis = visible_width(content);
    if vis >= width {
        content.to_string()
    } else {
        let padding = " ".repeat(width - vis);
        if left_align {
            format!("{}{}", content, padding)
        } else {
            format!("{}{}", padding, content)
        }
    }
}

/// Print the tabular view matching PM2 exactly.
pub fn print_pm2_table(processes: &[ProcessInfo]) {
    struct RowData {
        id: String,
        name: String,
        version: String,
        mode: String,
        pid: String,
        uptime: String,
        restarts: String,
        status: String,
        cpu: String,
        mem: String,
        user: String,
        watching: String,
    }

    let headers = [
        "id", "name", "version", "mode", "pid", "uptime", "↺", "status", "cpu", "mem", "user",
        "watching",
    ];

    let mut widths = [4, 10, 9, 9, 7, 8, 3, 8, 6, 8, 6, 10];

    // Ensure widths accommodate headers
    for (i, h) in headers.iter().enumerate() {
        widths[i] = widths[i].max(visible_width(h));
    }

    let mut rows: Vec<RowData> = Vec::with_capacity(processes.len());

    for proc in processes {
        let (cpu_val, mem_val, uptime_val, pid_str) = if proc.status == ProcessStatus::Online {
            let pid = proc.pid.unwrap_or(0);
            let (_c, c_str) = read_process_cpu(pid, proc.started_at);
            let (_m, m_str) = read_process_memory(pid);
            let u_str = format_uptime(proc.started_at);
            (c_str, m_str, u_str, pid.to_string())
        } else {
            ("0.0%".to_string(), "0.0mb".to_string(), "-".to_string(), "-".to_string())
        };

        let mode_styled = if proc.mode == "fork" {
            format!("\x1b[7;37m{}\x1b[0m", proc.mode)
        } else {
            format!("\x1b[1;34m{}\x1b[0m", proc.mode)
        };

        let watching_styled = if proc.watching {
            "\x1b[32menabled\x1b[0m".to_string()
        } else {
            "\x1b[90mdisabled\x1b[0m".to_string()
        };

        let row = RowData {
            id: format!("\x1b[1;36m{}\x1b[0m", proc.id),
            name: format!("\x1b[1;37m{}\x1b[0m", proc.name),
            version: proc.version.clone(),
            mode: mode_styled,
            pid: pid_str,
            uptime: uptime_val,
            restarts: proc.restarts.to_string(),
            status: proc.status.as_ansi(),
            cpu: cpu_val,
            mem: mem_val,
            user: proc.user.clone(),
            watching: watching_styled,
        };

        widths[0] = widths[0].max(visible_width(&row.id));
        widths[1] = widths[1].max(visible_width(&row.name));
        widths[2] = widths[2].max(visible_width(&row.version));
        widths[3] = widths[3].max(visible_width(&row.mode));
        widths[4] = widths[4].max(visible_width(&row.pid));
        widths[5] = widths[5].max(visible_width(&row.uptime));
        widths[6] = widths[6].max(visible_width(&row.restarts));
        widths[7] = widths[7].max(visible_width(&row.status));
        widths[8] = widths[8].max(visible_width(&row.cpu));
        widths[9] = widths[9].max(visible_width(&row.mem));
        widths[10] = widths[10].max(visible_width(&row.user));
        widths[11] = widths[11].max(visible_width(&row.watching));

        rows.push(row);
    }

    // Top border
    let top_border = format!(
        "{}{}{}{}{}",
        BORDER_COLOR,
        "┌",
        widths
            .iter()
            .map(|w| "─".repeat(w + 2))
            .collect::<Vec<_>>()
            .join("┬"),
        "┐",
        RESET
    );

    // Header row
    let header_cells = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let styled = format!("{}{}{}", HEADER_COLOR, h, RESET);
            pad_cell(&styled, widths[i], true)
        })
        .collect::<Vec<_>>()
        .join(&format!(" {}│{} ", BORDER_COLOR, RESET));

    let header_row = format!(
        "{}│{} {} {}│{}",
        BORDER_COLOR, RESET, header_cells, BORDER_COLOR, RESET
    );

    // Mid divider
    let mid_border = format!(
        "{}{}{}{}{}",
        BORDER_COLOR,
        "├",
        widths
            .iter()
            .map(|w| "─".repeat(w + 2))
            .collect::<Vec<_>>()
            .join("┼"),
        "┤",
        RESET
    );

    // Bottom border
    let bottom_border = format!(
        "{}{}{}{}{}",
        BORDER_COLOR,
        "└",
        widths
            .iter()
            .map(|w| "─".repeat(w + 2))
            .collect::<Vec<_>>()
            .join("┴"),
        "┘",
        RESET
    );

    println!("{}", top_border);
    println!("{}", header_row);
    println!("{}", mid_border);

    if rows.is_empty() {
        // Center message or blank line
        let total_inner = widths.iter().map(|w| w + 2).sum::<usize>() + (widths.len() - 1);
        let msg = "No active applications registered in RHPM supervisor";
        let centered = pad_cell(msg, total_inner.saturating_sub(1), true);
        println!("{}│{} {}{}│{}", BORDER_COLOR, RESET, centered, BORDER_COLOR, RESET);
    } else {
        for row in rows {
            let cells = [
                pad_cell(&row.id, widths[0], true),
                pad_cell(&row.name, widths[1], true),
                pad_cell(&row.version, widths[2], true),
                pad_cell(&row.mode, widths[3], true),
                pad_cell(&row.pid, widths[4], true),
                pad_cell(&row.uptime, widths[5], true),
                pad_cell(&row.restarts, widths[6], true),
                pad_cell(&row.status, widths[7], true),
                pad_cell(&row.cpu, widths[8], true),
                pad_cell(&row.mem, widths[9], true),
                pad_cell(&row.user, widths[10], true),
                pad_cell(&row.watching, widths[11], true),
            ]
            .join(&format!(" {}│{} ", BORDER_COLOR, RESET));

            println!("{}│{} {} {}│{}", BORDER_COLOR, RESET, cells, BORDER_COLOR, RESET);
        }
    }

    println!("{}", bottom_border);
    println!("{}", get_host_metrics_line());
}

/// Locate the `rhp` binary to execute for background daemon workers.
pub fn resolve_rhp_executable() -> PathBuf {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(file_name) = current_exe.file_name().and_then(|f| f.to_str()) {
            if file_name == "rhp" || file_name == "rhp.exe" {
                return current_exe;
            }
        }
        if let Some(parent) = current_exe.parent() {
            let candidate = parent.join("rhp");
            if candidate.is_file() {
                return candidate;
            }
            #[cfg(windows)]
            {
                let candidate_win = parent.join("rhp.exe");
                if candidate_win.is_file() {
                    return candidate_win;
                }
            }
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let bin = dir.join("rhp");
            if bin.is_file() {
                return bin;
            }
        }
    }
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rhp"))
}

/// Find a suitable port if none was explicitly requested.
fn find_available_port(state: &RhpmState, requested_port: Option<u16>) -> u16 {
    if let Some(p) = requested_port {
        return p;
    }
    let mut port = 8080;
    let used_ports: HashSet<u16> = state
        .processes
        .iter()
        .filter(|p| p.status == ProcessStatus::Online)
        .map(|p| p.port)
        .collect();

    while used_ports.contains(&port) {
        port += 1;
    }
    port
}

/// Handle `rhp start {name} [--file="app.rhp"] [--port=PORT] [--host=HOST] [--watch] [--mode=fork|cluster]`
pub fn handle_start(args: &[String]) {
    let mut name_arg: Option<String> = None;
    let mut file_arg: Option<String> = None;
    let mut port_arg: Option<u16> = None;
    let mut host_arg: String = "127.0.0.1".to_string();
    let mut watch_arg = false;
    let mut mode_arg = "fork".to_string();

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--help" || arg == "-h" {
            println!(
                concat!(
                    "rhp start <NAME> [OPTIONS]\n\n",
                    "Starts a new RHP application instance in the background (PM2-style supervisor).\n\n",
                    "USAGE:\n",
                    "    rhp start my-app\n",
                    "    rhp start my-app --file=\"app.rhp\"\n",
                    "    rhp start my-app --port=3000\n\n",
                    "OPTIONS:\n",
                    "    --file <PATH>, -f <PATH>     Script file to run (defaults to 'index.rhp' or 'app.rhp')\n",
                    "    --port <PORT>, -p <PORT>     Port to bind web server (default: 8080 or next available)\n",
                    "    --host <HOST>                Host interface to bind (default: 127.0.0.1)\n",
                    "    --watch                      Enable file watching and auto-restart\n",
                    "    --mode <fork|cluster>        Execution mode: 'fork' (default) or 'cluster'\n",
                    "    --help, -h                   Show this help message\n"
                )
            );
            process::exit(0);
        } else if let Some(val) = arg.strip_prefix("--file=") {
            file_arg = Some(val.trim_matches('"').trim_matches('\'').to_string());
        } else if arg == "--file" || arg == "-f" {
            if let Some(next) = iter.next() {
                file_arg = Some(next.trim_matches('"').trim_matches('\'').to_string());
            }
        } else if let Some(val) = arg.strip_prefix("-f=") {
            file_arg = Some(val.trim_matches('"').trim_matches('\'').to_string());
        } else if let Some(val) = arg.strip_prefix("--port=") {
            port_arg = val.parse().ok();
        } else if arg == "--port" || arg == "-p" {
            if let Some(next) = iter.next() {
                port_arg = next.parse().ok();
            }
        } else if let Some(val) = arg.strip_prefix("-p=") {
            port_arg = val.parse().ok();
        } else if let Some(val) = arg.strip_prefix("--host=") {
            host_arg = val.to_string();
        } else if arg == "--host" {
            if let Some(next) = iter.next() {
                host_arg = next.clone();
            }
        } else if arg == "--watch" {
            watch_arg = true;
        } else if let Some(val) = arg.strip_prefix("--mode=") {
            mode_arg = val.to_string();
        } else if arg == "--mode" {
            if let Some(next) = iter.next() {
                mode_arg = next.clone();
            }
        } else if !arg.starts_with('-') && name_arg.is_none() {
            name_arg = Some(arg.clone());
        }
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // File resolution:
    // 1. Specified via --file
    // 2. Default to 'index.rhp' in cwd
    // 3. Fallback to 'app.rhp' in cwd
    // 4. If name was specified ending with '.rhp', use that as file
    let (target_file, app_name) = match (file_arg, name_arg) {
        (Some(f), Some(n)) => {
            let p = if Path::new(&f).is_relative() {
                cwd.join(&f)
            } else {
                PathBuf::from(&f)
            };
            if !p.exists() {
                eprintln!("Error: Specified script file '{}' does not exist.", f);
                process::exit(1);
            }
            (p, n)
        }
        (Some(f), None) => {
            let p = if Path::new(&f).is_relative() {
                cwd.join(&f)
            } else {
                PathBuf::from(&f)
            };
            if !p.exists() {
                eprintln!("Error: Specified script file '{}' does not exist.", f);
                process::exit(1);
            }
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("app")
                .to_string();
            (p, stem)
        }
        (None, Some(n)) => {
            if n.ends_with(".rhp") && cwd.join(&n).exists() {
                let p = cwd.join(&n);
                let stem = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("app")
                    .to_string();
                (p, stem)
            } else if cwd.join("index.rhp").exists() {
                (cwd.join("index.rhp"), n)
            } else if cwd.join("app.rhp").exists() {
                (cwd.join("app.rhp"), n)
            } else {
                eprintln!(
                    "Error: Default script 'index.rhp' not found in current directory '{}'.\nUse '--file=\"path.rhp\"' to specify the application script.",
                    cwd.display()
                );
                process::exit(1);
            }
        }
        (None, None) => {
            if cwd.join("index.rhp").exists() {
                let stem = cwd
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("app")
                    .to_string();
                (cwd.join("index.rhp"), stem)
            } else if cwd.join("app.rhp").exists() {
                let stem = cwd
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("app")
                    .to_string();
                (cwd.join("app.rhp"), stem)
            } else {
                eprintln!(
                    "Error: Default script 'index.rhp' not found in current directory '{}'.\nUsage: rhp start <NAME> [--file=\"app.rhp\"]",
                    cwd.display()
                );
                process::exit(1);
            }
        }
    };

    let mut state = load_state();
    sync_process_states(&mut state);

    let port = find_available_port(&state, port_arg);
    let rhp_bin = resolve_rhp_executable();
    let rhpm_dir = get_rhpm_dir();
    let log_file = rhpm_dir.join("logs").join(format!("{}-out.log", app_name));
    let err_file = rhpm_dir.join("logs").join(format!("{}-err.log", app_name));

    // Check if an app with this name already exists in state
    let existing_idx = state.processes.iter().position(|p| p.name == app_name);

    if let Some(idx) = existing_idx {
        let old = &mut state.processes[idx];
        if old.status == ProcessStatus::Online {
            if let Some(pid) = old.pid {
                terminate_pid(pid);
            }
        }
    }

    // Launch daemon process detached
    let out_f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .unwrap_or_else(|e| {
            eprintln!("Error opening log file: {}", e);
            process::exit(1);
        });

    let err_f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&err_file)
        .unwrap_or_else(|e| {
            eprintln!("Error opening error log file: {}", e);
            process::exit(1);
        });

    let mut cmd = Command::new(&rhp_bin);
    cmd.arg("serve");
    cmd.arg(target_file.to_string_lossy().to_string());
    cmd.arg("--port");
    cmd.arg(port.to_string());
    cmd.arg("--host");
    cmd.arg(&host_arg);
    cmd.current_dir(&cwd);
    cmd.stdin(Stdio::null());
    cmd.stdout(out_f);
    cmd.stderr(err_f);

    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setsid();
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            Ok(())
        });
    }

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error spawning application process '{}': {}", app_name, e);
            process::exit(1);
        }
    };

    let pid = child.id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Give process 200ms to initialize and verify alive
    std::thread::sleep(Duration::from_millis(200));
    let is_alive = is_pid_alive(pid);
    let final_status = if is_alive {
        ProcessStatus::Online
    } else {
        ProcessStatus::Errored
    };

    let current_user = get_process_user(pid);

    if let Some(idx) = existing_idx {
        let proc = &mut state.processes[idx];
        proc.script_path = target_file.to_string_lossy().to_string();
        proc.cwd = cwd.to_string_lossy().to_string();
        proc.port = port;
        proc.host = host_arg;
        proc.pid = if is_alive { Some(pid) } else { None };
        proc.status = final_status;
        proc.mode = mode_arg;
        proc.user = current_user;
        proc.started_at = Some(now);
        proc.restarts += 1;
        proc.watching = watch_arg;
        proc.log_file = log_file.to_string_lossy().to_string();
        proc.err_file = err_file.to_string_lossy().to_string();
    } else {
        let new_id = state.next_id;
        state.next_id += 1;
        state.processes.push(ProcessInfo {
            id: new_id,
            name: app_name.clone(),
            script_path: target_file.to_string_lossy().to_string(),
            cwd: cwd.to_string_lossy().to_string(),
            port,
            host: host_arg,
            pid: if is_alive { Some(pid) } else { None },
            status: final_status,
            mode: mode_arg,
            version: env!("CARGO_PKG_VERSION").to_string(),
            user: current_user,
            created_at: now,
            started_at: Some(now),
            restarts: 0,
            watching: watch_arg,
            log_file: log_file.to_string_lossy().to_string(),
            err_file: err_file.to_string_lossy().to_string(),
        });
    }

    let _ = save_state(&state);

    if !is_alive {
        eprintln!(
            "\x1b[1;33m[RHPM Warning]\x1b[0m Process '{}' exited shortly after start. Check log: {}",
            app_name,
            err_file.display()
        );
    }

    print_pm2_table(&state.processes);
}

/// Handle `rhp stop {name|id|all}`
pub fn handle_stop(args: &[String]) {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!(
            concat!(
                "rhp stop <NAME|ID|all>\n\n",
                "Stops a running application instance managed by RHPM.\n\n",
                "USAGE:\n",
                "    rhp stop my-app\n",
                "    rhp stop 0\n",
                "    rhp stop all\n"
            )
        );
        process::exit(0);
    }

    let target = &args[0];
    let mut state = load_state();
    sync_process_states(&mut state);

    let mut found = false;

    if target == "all" {
        for proc in &mut state.processes {
            if proc.status == ProcessStatus::Online {
                if let Some(pid) = proc.pid {
                    terminate_pid(pid);
                }
                proc.status = ProcessStatus::Stopped;
                proc.pid = None;
                found = true;
            }
        }
    } else {
        let is_numeric = target.parse::<usize>().ok();
        for proc in &mut state.processes {
            if is_numeric == Some(proc.id) || proc.name.eq_ignore_ascii_case(target) {
                if proc.status == ProcessStatus::Online {
                    if let Some(pid) = proc.pid {
                        terminate_pid(pid);
                    }
                }
                proc.status = ProcessStatus::Stopped;
                proc.pid = None;
                found = true;
                break;
            }
        }
    }

    if !found {
        eprintln!("Error: Process '{}' not found in supervisor registry.", target);
        process::exit(1);
    }

    let _ = save_state(&state);
    print_pm2_table(&state.processes);
}

/// Handle `rhp status` (or `rhp ps`)
pub fn handle_status(args: &[String]) {
    if !args.is_empty() && (args[0] == "--help" || args[0] == "-h") {
        println!(
            concat!(
                "rhp status\n\n",
                "Displays the PM2-style tabular status view of all managed applications.\n"
            )
        );
        process::exit(0);
    }

    let mut state = load_state();
    sync_process_states(&mut state);
    let _ = save_state(&state);
    print_pm2_table(&state.processes);
}

/// Handle `rhp restart {name|id|all}`
pub fn handle_restart(args: &[String]) {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!(
            concat!(
                "rhp restart <NAME|ID|all>\n\n",
                "Restarts an application instance managed by RHPM.\n\n",
                "USAGE:\n",
                "    rhp restart my-app\n",
                "    rhp restart 0\n",
                "    rhp restart all\n"
            )
        );
        process::exit(0);
    }

    let target = &args[0];
    let mut state = load_state();
    sync_process_states(&mut state);

    let rhp_bin = resolve_rhp_executable();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut restarted_any = false;

    for proc in &mut state.processes {
        let matches = target == "all"
            || target.parse::<usize>().ok() == Some(proc.id)
            || proc.name.eq_ignore_ascii_case(target);

        if matches {
            if proc.status == ProcessStatus::Online {
                if let Some(pid) = proc.pid {
                    terminate_pid(pid);
                }
            }

            let out_f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&proc.log_file)
                .unwrap_or_else(|_| File::create(&proc.log_file).unwrap());

            let err_f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&proc.err_file)
                .unwrap_or_else(|_| File::create(&proc.err_file).unwrap());

            let mut cmd = Command::new(&rhp_bin);
            cmd.arg("serve");
            cmd.arg(&proc.script_path);
            cmd.arg("--port");
            cmd.arg(proc.port.to_string());
            cmd.arg("--host");
            cmd.arg(&proc.host);
            cmd.current_dir(&proc.cwd);
            cmd.stdin(Stdio::null());
            cmd.stdout(out_f);
            cmd.stderr(err_f);

            #[cfg(unix)]
            unsafe {
                use std::os::unix::process::CommandExt;
                cmd.pre_exec(|| {
                    libc::setsid();
                    libc::signal(libc::SIGHUP, libc::SIG_IGN);
                    Ok(())
                });
            }

            if let Ok(child) = cmd.spawn() {
                let pid = child.id();
                std::thread::sleep(Duration::from_millis(150));
                let is_alive = is_pid_alive(pid);
                proc.pid = if is_alive { Some(pid) } else { None };
                proc.status = if is_alive {
                    ProcessStatus::Online
                } else {
                    ProcessStatus::Errored
                };
                proc.started_at = Some(now);
                proc.restarts += 1;
                proc.user = get_process_user(pid);
                restarted_any = true;
            }
        }
    }

    if !restarted_any {
        eprintln!("Error: Process '{}' not found in supervisor registry.", target);
        process::exit(1);
    }

    let _ = save_state(&state);
    print_pm2_table(&state.processes);
}

/// Handle `rhp delete {name|id|all}` (or `rhp remove`)
pub fn handle_delete(args: &[String]) {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!(
            concat!(
                "rhp delete <NAME|ID|all>\n\n",
                "Stops and deletes an application instance from RHPM registry.\n\n",
                "USAGE:\n",
                "    rhp delete my-app\n",
                "    rhp delete 0\n",
                "    rhp delete all\n"
            )
        );
        process::exit(0);
    }

    let target = &args[0];
    let mut state = load_state();
    sync_process_states(&mut state);

    if target == "all" {
        for proc in &state.processes {
            if proc.status == ProcessStatus::Online {
                if let Some(pid) = proc.pid {
                    terminate_pid(pid);
                }
            }
        }
        state.processes.clear();
    } else {
        let is_numeric = target.parse::<usize>().ok();
        let idx = state
            .processes
            .iter()
            .position(|p| is_numeric == Some(p.id) || p.name.eq_ignore_ascii_case(target));

        match idx {
            Some(i) => {
                let proc = &state.processes[i];
                if proc.status == ProcessStatus::Online {
                    if let Some(pid) = proc.pid {
                        terminate_pid(pid);
                    }
                }
                state.processes.remove(i);
            }
            None => {
                eprintln!("Error: Process '{}' not found in supervisor registry.", target);
                process::exit(1);
            }
        }
    }

    let _ = save_state(&state);
    print_pm2_table(&state.processes);
}

/// Handle `rhp logs {name|id} [-n LINES]`
pub fn handle_logs(args: &[String]) {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!(
            concat!(
                "rhp logs <NAME|ID> [OPTIONS]\n\n",
                "Display recent output and error logs for a managed application.\n\n",
                "OPTIONS:\n",
                "    -n, --lines <N>   Number of lines to show (default: 30)\n"
            )
        );
        process::exit(0);
    }

    let target = &args[0];
    let lines_to_show: usize = args
        .iter()
        .position(|a| a == "-n" || a == "--lines")
        .and_then(|idx| args.get(idx + 1))
        .and_then(|val| val.parse().ok())
        .unwrap_or(30);

    let state = load_state();
    let is_numeric = target.parse::<usize>().ok();
    let proc = state
        .processes
        .iter()
        .find(|p| is_numeric == Some(p.id) || p.name.eq_ignore_ascii_case(target));

    let proc = match proc {
        Some(p) => p,
        None => {
            eprintln!("Error: Process '{}' not found.", target);
            process::exit(1);
        }
    };

    println!("\x1b[1;36m=== Standard Output: {} ===\x1b[0m", proc.log_file);
    if let Ok(out) = fs::read_to_string(&proc.log_file) {
        let lines: Vec<&str> = out.lines().collect();
        let start = lines.len().saturating_sub(lines_to_show);
        for l in &lines[start..] {
            println!("{}", l);
        }
    } else {
        println!("(No output log recorded yet)");
    }

    println!("\n\x1b[1;31m=== Standard Error: {} ===\x1b[0m", proc.err_file);
    if let Ok(err) = fs::read_to_string(&proc.err_file) {
        let lines: Vec<&str> = err.lines().collect();
        let start = lines.len().saturating_sub(lines_to_show);
        for l in &lines[start..] {
            println!("{}", l);
        }
    } else {
        println!("(No error log recorded yet)");
    }
}
