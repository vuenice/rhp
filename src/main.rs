use std::env;
use std::path::Path;
use std::process;

use rhp::lint::lint_path;
use rhp::vm::{run_source, RunMode};

fn print_help() {
    println!(
        concat!(
            "RHP: High-Performance PHP-Family Programming Language Engine\n\n",
            "USAGE:\n",
            "    rhp run lint [PATH] [OPTIONS]    Lint, auto-format, auto-type variables, enforce casing, and output JSON issues\n",
            "    rhp run dev [PATH]               Execute in Development Mode (direct AST tree-walking)\n",
            "    rhp run build [PATH]             Execute in Production Mode (register VM bytecode + JIT)\n",
            "    rhp --version                    Display version information\n",
            "    rhp --help                       Display this help message\n\n",
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
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: Missing subcommand for 'rhp run'. Expected 'lint', 'dev', or 'build'.");
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
                cmd => {
                    eprintln!("Error: Unknown run command '{}'. Expected 'lint', 'dev', or 'build'.", cmd);
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

/// Handle `rhp run dev [PATH]` or `rhp run build [PATH]`
fn handle_run_script(sub_args: &[String], _mode: RunMode) {
    if sub_args.is_empty() {
        eprintln!("Error: Missing script file to execute.");
        process::exit(1);
    }
    let script_path = &sub_args[0];
    let path = Path::new(script_path);
    if !path.exists() {
        eprintln!("Error: Script file '{}' does not exist.", script_path);
        process::exit(1);
    }

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading script '{}': {}", script_path, e);
            process::exit(1);
        }
    };

    match run_source(&source) {
        Ok(_) => process::exit(0),
        Err(e) => {
            eprintln!("Runtime error: {}", e);
            process::exit(1);
        }
    }
}
