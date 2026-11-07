//! # rhpm
//!
//! RHP Process Manager CLI.
//!
//! Production web application supervisor acting like PM2 for RHP applications.

use std::env;
use std::process;

use rhp::runtime::supervisor::{
    handle_delete, handle_logs, handle_restart, handle_start, handle_status, handle_stop,
};

fn print_rhpm_help() {
    println!(
        concat!(
            "RHPM: RHP Process Manager & Supervisor\n\n",
            "USAGE:\n",
            "    rhpm <COMMAND> [OPTIONS]\n\n",
            "COMMANDS:\n",
            "    start <NAME> [OPTIONS]           Start application instance in background (PM2 supervisor)\n",
            "    stop <NAME|ID|all>               Stop running application instance\n",
            "    restart <NAME|ID|all>            Restart running application instance\n",
            "    delete <NAME|ID|all>             Stop and remove application instance from supervisor\n",
            "    status, ps, ls, list             Display tabular status of all managed applications\n",
            "    logs <NAME|ID> [OPTIONS]         Display recent logs for managed application\n\n",
            "OPTIONS for 'start':\n",
            "    --file <PATH>, -f <PATH>         Target script file (defaults to 'index.rhp' or 'app.rhp')\n",
            "    --port <PORT>, -p <PORT>         Port for web server (defaults to 8080 or next available)\n",
            "    --host <HOST>                    Host interface to bind (defaults to 127.0.0.1)\n",
            "    --watch                          Enable source file watching\n",
            "    --mode <fork|cluster>            Process execution mode (defaults to 'fork')\n\n",
            "GLOBAL OPTIONS:\n",
            "    --version, -v                    Display version information\n",
            "    --help, -h                       Display this help message\n"
        )
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        handle_status(&[]);
        return;
    }

    match args[1].as_str() {
        "--version" | "-V" | "-v" => {
            println!("rhpm {}", env!("CARGO_PKG_VERSION"));
        }
        "--help" | "-h" | "help" => {
            print_rhpm_help();
        }
        "start" => {
            handle_start(&args[2..]);
        }
        "stop" => {
            handle_stop(&args[2..]);
        }
        "status" | "ps" | "ls" | "list" => {
            handle_status(&args[2..]);
        }
        "restart" => {
            handle_restart(&args[2..]);
        }
        "delete" | "rm" | "kill" => {
            handle_delete(&args[2..]);
        }
        "logs" => {
            handle_logs(&args[2..]);
        }
        name if !name.starts_with('-') => {
            // Shorthand e.g. `rhpm app` or `rhpm app.rhp`
            handle_start(&args[1..]);
        }
        cmd => {
            eprintln!("Error: Unknown rhpm command '{}'. Run 'rhpm --help' for usage.", cmd);
            process::exit(1);
        }
    }
}
