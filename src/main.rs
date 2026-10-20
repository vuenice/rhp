use std::env;
use std::path::Path;
use std::process;

use rhp::lint::lint_path;
use rhp::vm::RunMode;

fn print_help() {
    println!(
        concat!(
            "RHP: High-Performance PHP-Family Programming Language Engine\n\n",
            "USAGE:\n",
            "    rhp run dev [PATH] [OPTIONS]     Execute in Development Mode (direct AST tree-walking, defaults to 'index.rhp')\n",
            "    rhp run build [PATH] [OPTIONS]   Execute in Production Mode (register VM bytecode + JIT, defaults to 'index.rhp')\n",
            "    rhp dev [PATH]                   Shorthand for 'rhp run dev'\n",
            "    rhp build [PATH]                 Shorthand for 'rhp run build'\n",
            "    rhp serve [PATH] [OPTIONS]       Start HTTP server for RHP web apps (defaults to 'examples/todo/app.rhp')\n",
            "    rhp run lint [PATH] [OPTIONS]    Lint, auto-format, auto-type variables, enforce casing, and output JSON issues\n",
            "    rhp --version                    Display version information\n",
            "    rhp --help                       Display this help message\n\n",
            "OPTIONS for 'serve':\n",
            "    --port <PORT>, -p <PORT>         Port to listen on (defaults to 8000)\n",
            "    --host <HOST>                    Host to bind (defaults to 127.0.0.1)\n\n",
            "OPTIONS for 'run dev' and 'run build':\n",
            "    --file <PATH>, -f <PATH>         Script file to execute (defaults to 'index.rhp' in current directory)\n",
            "    PATH                             Script file to execute (optional positional argument)\n\n",
            "OPTIONS for 'run lint':\n",
            "    PATH                             Target file or directory (defaults to current directory '.')\n",
            "    --check, --no-fix                Check only without modifying files on disk (dry run)\n",
            "    --no-case-change                 Skip changing identifier casing or reporting casing issues\n"
        )
    );
}

fn print_version() {
    println!("rhp {}", env!("CARGO_PKG_VERSION"));
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_help();
        process::exit(1);
    }

    match args[1].as_str() {
        "--version" | "-V" | "-v" => {
            print_version();
            process::exit(0);
        }
        "--help" | "-h" | "help" => {
            print_help();
            process::exit(0);
        }
        "serve" => {
            handle_serve(&args[2..]);
        }
        "dev" => {
            handle_run_script(&args[2..], RunMode::Dev { hot_reload: true, dap_port: None });
        }
        "build" => {
            handle_run_script(&args[2..], RunMode::Build { jit_enabled: true, opt_level: 2 });
        }
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: Missing subcommand for 'rhp run'. Expected 'lint', 'dev', 'build', or 'serve'.");
                process::exit(1);
            }
            match args[2].as_str() {
                "lint" => {
                    handle_run_lint(&args[3..]);
                }
                "dev" => {
                    handle_run_script(&args[3..], RunMode::Dev { hot_reload: true, dap_port: None });
                }
                "build" => {
                    handle_run_script(&args[3..], RunMode::Build { jit_enabled: true, opt_level: 2 });
                }
                "serve" => {
                    handle_serve(&args[3..]);
                }
                cmd => {
                    eprintln!("Error: Unknown run command '{}'. Expected 'lint', 'dev', 'build', or 'serve'.", cmd);
                    process::exit(1);
                }
            }
        }
        cmd => {
            eprintln!("Error: Unknown command '{}'. Run 'rhp --help' for usage.", cmd);
            process::exit(1);
        }
    }
}

/// Handle `rhp run lint [PATH] [--check] [--no-fix] [--no-case-change]`
fn handle_run_lint(sub_args: &[String]) {
    let mut check_only = false;
    let mut no_fix = false;
    let mut no_case_change = false;
    let mut target_path = ".".to_string();

    for arg in sub_args {
        if arg == "--help" || arg == "-h" {
            println!(
                concat!(
                    "rhp run lint [PATH] [--check] [--no-fix] [--no-case-change]\n\n",
                    "Auto-formats code (4 spaces, operator spacing, brace normalization),\n",
                    "enforces naming conventions (snake_case variables, PascalCase classes, camelCase functions/test_ in snake_case),\n",
                    "infers and inserts types for variables, array literals (list, dict, hashMap), and parameters,\n",
                    "preserves existing user types, and outputs typecheck & lint issues in JSON format.\n\n",
                    "OPTIONS:\n",
                    "    PATH               Target file or directory (default: current directory '.')\n",
                    "    --check, --no-fix  Check only without writing changes to disk\n",
                    "    --no-case-change   Do not change identifier casing or report casing issues\n",
                    "    --help             Show this help message\n"
                )
            );
            process::exit(0);
        } else if arg == "--check" {
            check_only = true;
        } else if arg == "--no-fix" {
            no_fix = true;
        } else if arg == "--no-case-change" {
            no_case_change = true;
        } else if !arg.starts_with("--") {
            target_path = arg.clone();
        }
    }

    let path = Path::new(&target_path);
    if !path.exists() {
        eprintln!("Error: Target path '{}' does not exist.", target_path);
        process::exit(1);
    }

    let options = rhp::lint::LintOptions {
        fix: !check_only && !no_fix,
        case_change: !no_case_change,
    };

    match lint_path(path, options) {
        Ok(report) => {
            // Output issues and stats in JSON format
            println!("{}", report.to_json());
            if report.errors_count() > 0 || (!options.fix && report.total_issues() > 0) {
                process::exit(1);
            } else {
                process::exit(0);
            }
        }
        Err(e) => {
            eprintln!("Error during lint execution: {}", e);
            process::exit(1);
        }
    }
}

#[derive(Debug, Clone)]
struct ScriptRunOptions {
    script_path: String,
    port: Option<u16>,
    host: Option<String>,
    force_cli: bool,
    force_server: bool,
}

/// Parse options for `rhp run dev`, `rhp run build`, `rhp dev`, and `rhp build`.
fn parse_script_run_options(sub_args: &[String]) -> Result<ScriptRunOptions, String> {
    let mut file_arg: Option<String> = None;
    let mut positional_arg: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut host: Option<String> = None;
    let mut force_cli = false;
    let mut force_server = false;

    let mut iter = sub_args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--help" || arg == "-h" {
            return Err("help".to_string());
        } else if arg == "--cli" {
            force_cli = true;
        } else if arg == "--server" || arg == "--serve" {
            force_server = true;
        } else if let Some(val) = arg.strip_prefix("--port=") {
            port = Some(val.parse().unwrap_or(8080));
        } else if arg == "--port" || arg == "-p" {
            if let Some(next_val) = iter.next() {
                port = Some(next_val.parse().unwrap_or(8080));
            }
        } else if let Some(val) = arg.strip_prefix("--host=") {
            host = Some(val.to_string());
        } else if arg == "--host" {
            if let Some(next_val) = iter.next() {
                host = Some(next_val.clone());
            }
        } else if let Some(val) = arg.strip_prefix("--file=") {
            let clean = val.trim().trim_matches(|c| c == '\'' || c == '"');
            if clean.is_empty() {
                return Err("Error: File path specified for '--file' is empty.".to_string());
            }
            file_arg = Some(clean.to_string());
        } else if arg == "--file" || arg == "-f" {
            if let Some(next_val) = iter.next() {
                let clean = next_val.trim().trim_matches(|c| c == '\'' || c == '"');
                if clean.is_empty() {
                    return Err(format!("Error: File path specified for '{}' is empty.", arg));
                }
                file_arg = Some(clean.to_string());
            } else {
                return Err(format!("Error: Missing file path for '{}'.", arg));
            }
        } else if let Some(val) = arg.strip_prefix("-f=") {
            let clean = val.trim().trim_matches(|c| c == '\'' || c == '"');
            if clean.is_empty() {
                return Err("Error: File path specified for '-f' is empty.".to_string());
            }
            file_arg = Some(clean.to_string());
        } else if let Some(val) = arg.strip_prefix("file=") {
            let clean = val.trim().trim_matches(|c| c == '\'' || c == '"');
            if clean.is_empty() {
                return Err("Error: File path specified for 'file=' is empty.".to_string());
            }
            file_arg = Some(clean.to_string());
        } else if !arg.starts_with('-') {
            if positional_arg.is_none() {
                positional_arg = Some(arg.clone());
            }
        }
    }

    let script_path = file_arg
        .or(positional_arg)
        .unwrap_or_else(|| "index.rhp".to_string());

    Ok(ScriptRunOptions {
        script_path,
        port,
        host,
        force_cli,
        force_server,
    })
}

#[allow(dead_code)]
/// Parse the target script path from CLI sub-arguments.
/// Defaults to "index.rhp" in the current working directory if not specified.
fn parse_script_target(sub_args: &[String]) -> Result<String, String> {
    parse_script_run_options(sub_args).map(|o| o.script_path)
}

fn print_run_script_help(mode: &RunMode) {
    let mode_name = match mode {
        RunMode::Dev { .. } => "dev",
        RunMode::Build { .. } => "build",
    };
    let mode_desc = match mode {
        RunMode::Dev { .. } => "Execute in Development Mode (direct AST tree-walking).",
        RunMode::Build { .. } => "Execute in Production Mode (register VM bytecode + JIT).",
    };
    println!(
        concat!(
            "rhp run {mode} [PATH] [OPTIONS]\n\n",
            "{desc}\n\n",
            "Defaults to 'index.rhp' in current working directory if no file is specified.\n",
            "Web applications (using $_SERVER, $_GET, $_POST, or rendering HTML) automatically\n",
            "launch the interactive HTTP server on port 8080.\n\n",
            "OPTIONS:\n",
            "    --port <PORT>, -p <PORT>  Port for web server mode (default: 8080)\n",
            "    --host <HOST>             Host interface to bind (default: 127.0.0.1)\n",
            "    --file <PATH>, -f <PATH>  Script file to execute (e.g. --file=\"/your-path/server.rhp\")\n",
            "    --cli                     Force one-shot CLI execution even if web application\n",
            "    --server                  Force web server mode\n",
            "    PATH                      Positional script file to execute (alternative to --file)\n",
            "    --help, -h                Display this help message\n"
        ),
        mode = mode_name,
        desc = mode_desc,
    );
}

/// Handle `rhp run dev [PATH]` or `rhp run build [PATH]`
fn handle_run_script(sub_args: &[String], mode: RunMode) {
    let opts = match parse_script_run_options(sub_args) {
        Ok(opts) => opts,
        Err(err) if err == "help" => {
            print_run_script_help(&mode);
            process::exit(0);
        }
        Err(err) => {
            eprintln!("{}", err);
            process::exit(1);
        }
    };

    let target_path = Path::new(&opts.script_path);
    let resolved_path = if target_path.is_dir() {
        if target_path.join("index.rhp").exists() {
            target_path.join("index.rhp")
        } else if target_path.join("app.rhp").exists() {
            target_path.join("app.rhp")
        } else {
            eprintln!("Error: Directory '{}' does not contain 'index.rhp' or 'app.rhp'.", opts.script_path);
            process::exit(1);
        }
    } else if !target_path.exists() && opts.script_path == "index.rhp" && Path::new("app.rhp").exists() {
        std::path::PathBuf::from("app.rhp")
    } else if !target_path.exists() && opts.script_path == "index.rhp" && Path::new("examples/todo/index.rhp").exists() {
        std::path::PathBuf::from("examples/todo/index.rhp")
    } else if !target_path.exists() && opts.script_path == "index.rhp" && Path::new("examples/todo/app.rhp").exists() {
        std::path::PathBuf::from("examples/todo/app.rhp")
    } else if !target_path.exists() {
        eprintln!("Error: Script file '{}' does not exist.", opts.script_path);
        process::exit(1);
    } else {
        target_path.to_path_buf()
    };

    let source = match std::fs::read_to_string(&resolved_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading script '{}': {}", resolved_path.display(), e);
            process::exit(1);
        }
    };

    // Auto-detect if this is a web application or explicitly requested as server
    let is_web_app = opts.force_server
        || opts.port.is_some()
        || (!opts.force_cli
            && (source.contains("$_SERVER")
                || source.contains("$_GET")
                || source.contains("$_POST")
                || source.contains("HTTP_HX_REQUEST")
                || source.contains("<!DOCTYPE html>")
                || source.contains("<html")));

    if is_web_app && !opts.force_cli {
        let port = opts.port.unwrap_or(8080);
        let host = opts.host.unwrap_or_else(|| "127.0.0.1".to_string());
        start_server(
            &resolved_path.to_string_lossy(),
            &host,
            port,
            Some(mode),
        );
        return;
    }

    let jit_enabled = match mode {
        RunMode::Dev { .. } => false,
        RunMode::Build { jit_enabled, .. } => jit_enabled,
    };

    match rhp::vm::run_source_with_vm_and_jit(&source, jit_enabled) {
        Ok((vm, val)) => {
            let out = vm.output();
            if !out.is_empty() {
                print!("{}", out);
            } else if val != rhp::PhpValue::Null {
                println!("{}", val.to_php_string());
            }
            process::exit(0);
        }
        Err(e) => {
            eprintln!("Runtime error: {}", e);
            process::exit(1);
        }
    }
}

/// Launch HTTP server serving an RHP application.
fn start_server(script_path: &str, host: &str, port: u16, mode: Option<RunMode>) {
    let addr = format!("{}:{}", host, port);
    let listener = match std::net::TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Error binding server to {}: {}", addr, e);
            process::exit(1);
        }
    };

    let title = match mode {
        Some(RunMode::Dev { .. }) => "🚀 RHP Development Server (Dev Mode)",
        Some(RunMode::Build { .. }) => "🚀 RHP Production Server (Build Mode + JIT)",
        None => "🚀 RHP High-Performance Web Server",
    };

    println!("=======================================================================");
    println!("{}", title);
    println!("📡 Listening on: http://{}", addr);
    println!("📂 Serving application: {}", script_path);
    println!("⚡ Reactive UI: HTMX + Alpine.js");
    println!("⌨️  Press Ctrl+C to terminate the server.");
    println!("=======================================================================");

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };

        let script = script_path.to_string();
        serve_http_connection(&mut stream, &script, host, port);
    }
}

/// Handle `rhp serve [PATH] [--port PORT] [--host HOST]`
fn handle_serve(sub_args: &[String]) {
    let mut port: u16 = 8080;
    let mut host = "127.0.0.1".to_string();
    let mut custom_path: Option<String> = None;

    let mut iter = sub_args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--help" || arg == "-h" {
            println!(
                concat!(
                    "rhp serve [PATH] [OPTIONS]\n\n",
                    "Starts an HTTP server to run RHP web applications.\n\n",
                    "OPTIONS:\n",
                    "    PATH               Script file to serve (default: 'examples/todo/app.rhp' or 'index.rhp')\n",
                    "    --port, -p <PORT>  Port to listen on (default: 8080)\n",
                    "    --host <HOST>      Host interface to bind (default: 127.0.0.1)\n",
                    "    --help, -h         Show this help message\n"
                )
            );
            process::exit(0);
        } else if let Some(val) = arg.strip_prefix("--port=") {
            port = val.parse().unwrap_or(8080);
        } else if arg == "--port" || arg == "-p" {
            if let Some(next_val) = iter.next() {
                port = next_val.parse().unwrap_or(8080);
            }
        } else if let Some(val) = arg.strip_prefix("--host=") {
            host = val.to_string();
        } else if arg == "--host" {
            if let Some(next_val) = iter.next() {
                host = next_val.clone();
            }
        } else if !arg.starts_with('-') && custom_path.is_none() {
            custom_path = Some(arg.clone());
        }
    }

    let script_path = custom_path.unwrap_or_else(|| {
        if Path::new("examples/todo/app.rhp").exists() {
            "examples/todo/app.rhp".to_string()
        } else if Path::new("index.rhp").exists() {
            "index.rhp".to_string()
        } else {
            "examples/todo/app.rhp".to_string()
        }
    });

    let target_file = Path::new(&script_path);
    let resolved_file = if target_file.is_dir() {
        if target_file.join("app.rhp").exists() {
            target_file.join("app.rhp")
        } else if target_file.join("index.rhp").exists() {
            target_file.join("index.rhp")
        } else {
            eprintln!("Error: Directory '{}' does not contain 'app.rhp' or 'index.rhp'.", script_path);
            process::exit(1);
        }
    } else if !target_file.exists() && Path::new("app.rhp").exists() {
        std::path::PathBuf::from("app.rhp")
    } else if !target_file.exists() {
        eprintln!("Error: Target script '{}' does not exist.", script_path);
        process::exit(1);
    } else {
        target_file.to_path_buf()
    };
    let script_path = resolved_file.to_string_lossy().to_string();

    start_server(&script_path, &host, port, None);
}

fn url_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(' ');
        } else {
            out.push(bytes[i] as char);
        }
        i += 1;
    }
    out
}

fn parse_query_or_form(query: &str) -> indexmap::IndexMap<String, rhp::FrozenValue> {
    let mut map = indexmap::IndexMap::new();
    if query.is_empty() {
        return map;
    }
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let raw_k = parts.next().unwrap_or("");
        let raw_v = parts.next().unwrap_or("");
        let k = url_decode(raw_k);
        let v = url_decode(raw_v);
        map.insert(k, rhp::FrozenValue::String(v));
    }
    map
}

fn serve_http_connection(
    stream: &mut std::net::TcpStream,
    script_path: &str,
    host: &str,
    port: u16,
) {
    use std::io::{BufRead, BufReader, Read, Write};

    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut req_line = String::new();
    if reader.read_line(&mut req_line).is_err() || req_line.is_empty() {
        return;
    }

    let parts: Vec<&str> = req_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }
    let method = parts[0].to_uppercase();
    let full_uri = parts[1].to_string();

    let mut headers = indexmap::IndexMap::new();
    let mut content_length: usize = 0;
    let mut is_htmx = false;

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        let trimmed = line.trim();
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_string();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            } else if key == "hx-request" && val.eq_ignore_ascii_case("true") {
                is_htmx = true;
            }
            headers.insert(key, val);
        }
    }

    let mut body_bytes = vec![0u8; content_length];
    if content_length > 0 {
        let _ = reader.read_exact(&mut body_bytes);
    }
    let body_str = String::from_utf8_lossy(&body_bytes).into_owned();

    let (path, query_str) = match full_uri.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (full_uri.clone(), String::new()),
    };

    // Static file serving: if requested path is a file in the same directory (e.g. index.html or todos.json)
    let parent_dir = Path::new(script_path).parent().unwrap_or(Path::new("."));
    let static_candidate = if path == "/" || path.is_empty() {
        None
    } else {
        let clean_path = path.trim_start_matches('/');
        let file_path = parent_dir.join(clean_path);
        if file_path.is_file() && !clean_path.ends_with(".rhp") {
            Some(file_path)
        } else {
            None
        }
    };

    if let Some(static_file) = static_candidate {
        if let Ok(contents) = std::fs::read(&static_file) {
            let mime = if static_file.extension().and_then(|e| e.to_str()) == Some("json") {
                "application/json; charset=utf-8"
            } else if static_file.extension().and_then(|e| e.to_str()) == Some("html") {
                "text/html; charset=utf-8"
            } else if static_file.extension().and_then(|e| e.to_str()) == Some("css") {
                "text/css; charset=utf-8"
            } else if static_file.extension().and_then(|e| e.to_str()) == Some("js") {
                "application/javascript; charset=utf-8"
            } else {
                "application/octet-stream"
            };

            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                mime,
                contents.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(&contents);
            let _ = stream.flush();
            return;
        }
    }

    // Dynamic execution of RHP script:
    let source = match std::fs::read_to_string(script_path) {
        Ok(s) => s,
        Err(e) => {
            let err_body = format!("<h1>500 Internal Server Error</h1><p>Failed to read script '{}': {}</p>", script_path, e);
            let resp = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                err_body.len(),
                err_body
            );
            let _ = stream.write_all(resp.as_bytes());
            return;
        }
    };

    let get_params = parse_query_or_form(&query_str);
    let mut post_params = parse_query_or_form(&body_str);

    // If body is JSON, parse keys into post_params as well
    if let Ok(rhp::PhpValue::Dict(d)) = rhp::parse_json_value(&body_str) {
        for (k, v) in d.borrow().iter() {
            post_params.insert(k.clone(), rhp::FrozenValue::from_php_value(v));
        }
    }

    let mut req = rhp::WorkerRequest::new(&source);
    req = req
        .with_server("REQUEST_METHOD", &method)
        .with_server("REQUEST_URI", &full_uri)
        .with_server("PATH_INFO", &path)
        .with_server("QUERY_STRING", &query_str)
        .with_server("SERVER_PORT", &port.to_string())
        .with_server("SERVER_NAME", host);

    if is_htmx {
        req = req.with_server("HTTP_HX_REQUEST", "true");
    }

    for (k, v) in get_params {
        req = req.with_get(k, v);
    }
    for (k, v) in post_params {
        req = req.with_post(k, v);
    }

    let state = match rhp::ParentProcess::boot(&source) {
        Ok(s) => s,
        Err(e) => {
            let err_body = format!("<h1>500 Compilation Error</h1><pre>{}</pre>", e);
            let resp = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                err_body.len(),
                err_body
            );
            let _ = stream.write_all(resp.as_bytes());
            return;
        }
    };

    let resp = rhp::execute_isolated_request(&state, &req, 0);

    let (status_line, body_content, content_type) = if resp.status == rhp::WorkerResponseStatus::Success {
        let out = resp.output();
        let content = if !out.is_empty() {
            out.to_string()
        } else {
            let val = resp.to_php_value();
            if val != rhp::PhpValue::Null {
                val.to_php_string()
            } else {
                String::new()
            }
        };

        let trimmed = content.trim();
        let mime = if trimmed.starts_with('{') || trimmed.starts_with('[') {
            "application/json; charset=utf-8"
        } else {
            "text/html; charset=utf-8"
        };
        ("HTTP/1.1 200 OK", content, mime)
    } else {
        let err_msg = resp.error.clone().unwrap_or_else(|| "Unknown runtime error".to_string());
        let err_body = format!("<h1>500 Server Error</h1><pre>{}</pre>", err_msg);
        ("HTTP/1.1 500 Internal Server Error", err_body, "text/html; charset=utf-8")
    };

    let http_resp = format!(
        "{}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status_line,
        content_type,
        body_content.len(),
        body_content
    );

    let _ = stream.write_all(http_resp.as_bytes());
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_script_target_defaults_to_index_rhp() {
        let args: Vec<String> = vec![];
        assert_eq!(parse_script_target(&args).unwrap(), "index.rhp");
    }

    #[test]
    fn test_parse_script_target_file_flag_equals() {
        let args = vec!["--file=/your-path/server.rhp".to_string()];
        assert_eq!(parse_script_target(&args).unwrap(), "/your-path/server.rhp");

        let args_quoted = vec!["--file=\"/your-path/server.rhp\"".to_string()];
        assert_eq!(parse_script_target(&args_quoted).unwrap(), "/your-path/server.rhp");

        let args_single_quoted = vec!["--file='server.rhp'".to_string()];
        assert_eq!(parse_script_target(&args_single_quoted).unwrap(), "server.rhp");
    }

    #[test]
    fn test_parse_script_target_file_flag_space() {
        let args = vec!["--file".to_string(), "/your-path/server.rhp".to_string()];
        assert_eq!(parse_script_target(&args).unwrap(), "/your-path/server.rhp");
    }

    #[test]
    fn test_parse_script_target_f_flag() {
        let args = vec!["-f".to_string(), "app.rhp".to_string()];
        assert_eq!(parse_script_target(&args).unwrap(), "app.rhp");

        let args2 = vec!["-f=app.rhp".to_string()];
        assert_eq!(parse_script_target(&args2).unwrap(), "app.rhp");
    }

    #[test]
    fn test_parse_script_target_positional() {
        let args = vec!["custom.rhp".to_string()];
        assert_eq!(parse_script_target(&args).unwrap(), "custom.rhp");
    }

    #[test]
    fn test_parse_script_target_file_flag_precedence_over_positional() {
        let args = vec!["positional.rhp".to_string(), "--file=flag.rhp".to_string()];
        assert_eq!(parse_script_target(&args).unwrap(), "flag.rhp");
    }

    #[test]
    fn test_parse_script_target_empty_file_flag_errors() {
        let args = vec!["--file=".to_string()];
        assert!(parse_script_target(&args).is_err());

        let args2 = vec!["--file".to_string()];
        assert!(parse_script_target(&args2).is_err());
    }
}

