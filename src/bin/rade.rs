//! # rade
//!
//! Rade CLI: High-performance RHP templating engine companion binary.
//! Used for template debugging, offline compilation, and AST inspection.

use std::env;
use std::fs;
use std::path::Path;
use std::process;

use rhp::rade::compile_to_rhp;
use rhp::rade::runtime::{cache_views, clear_cache};

fn print_help() {
    println!("Rade - High-performance RHP templating engine inspired by Blade\n");
    println!("USAGE:");
    println!("    rade compile <input.rade.rhp> [-o <output.rhp>]");
    println!("    rade cache <views_dir> <storage_dir>");
    println!("    rade clear <storage_dir>\n");
    println!("COMMANDS:");
    println!("    compile    Compiles a single template file and outputs the generated RHP code");
    println!("    cache      Recursively compiles all .rade.rhp files into hashed cache files");
    println!("    clear      Clears all cached .rhp view files\n");
    println!("OPTIONS:");
    println!("    -h, --help       Print help information");
    println!("    -V, --version    Print version information");
}

fn handle_compile(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("Error: Missing input template file for 'compile'.\nUsage: rade compile <input.rade.rhp> [-o <output.rhp>]".to_string());
    }

    let mut input_file: Option<String> = None;
    let mut output_file: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-o" || arg == "--output" {
            if i + 1 >= args.len() {
                return Err("Error: '-o' / '--output' requires a file path argument.".to_string());
            }
            output_file = Some(args[i + 1].clone());
            i += 2;
        } else if arg.starts_with("-o=") {
            output_file = Some(arg["-o=".len()..].to_string());
            i += 1;
        } else if arg.starts_with("--output=") {
            output_file = Some(arg["--output=".len()..].to_string());
            i += 1;
        } else if arg == "-h" || arg == "--help" {
            println!("Usage: rade compile <input.rade.rhp> [-o <output.rhp>]");
            return Ok(());
        } else if arg.starts_with('-') {
            return Err(format!("Error: Unrecognized option '{arg}'."));
        } else if input_file.is_none() {
            input_file = Some(arg.clone());
            i += 1;
        } else {
            return Err(format!("Error: Unexpected argument '{arg}'."));
        }
    }

    let input_path = match input_file {
        Some(path) => path,
        None => {
            return Err("Error: Missing input template file for 'compile'.\nUsage: rade compile <input.rade.rhp> [-o <output.rhp>]".to_string());
        }
    };

    let content = fs::read_to_string(&input_path)
        .map_err(|e| format!("Error reading input file '{input_path}': {e}"))?;

    let compiled = compile_to_rhp(&content)
        .map_err(|e| format!("Compilation error in '{input_path}': {e}"))?;

    if let Some(out_path) = output_file {
        if let Some(parent) = Path::new(&out_path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Error creating output directory '{parent:?}': {e}"))?;
            }
        }
        fs::write(&out_path, &compiled)
            .map_err(|e| format!("Error writing output file '{out_path}': {e}"))?;
    } else {
        println!("{compiled}");
    }

    Ok(())
}

fn handle_cache(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Error: Missing arguments for 'cache'.\nUsage: rade cache <views_dir> <storage_dir>".to_string());
    }

    let views_dir = &args[0];
    let storage_dir = &args[1];

    let count = cache_views(views_dir, storage_dir)
        .map_err(|e| format!("Error caching views: {e}"))?;

    println!("Cached {count} view(s) to '{storage_dir}'.");
    Ok(())
}

fn handle_clear(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("Error: Missing storage directory for 'clear'.\nUsage: rade clear <storage_dir>".to_string());
    }

    let storage_dir = &args[0];

    let count = clear_cache(storage_dir)
        .map_err(|e| format!("Error clearing cache: {e}"))?;

    println!("Cleared {count} cached view file(s) from '{storage_dir}'.");
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    let cmd = &args[1];
    let rest = &args[2..];

    let result = match cmd.as_str() {
        "compile" => handle_compile(rest),
        "cache" => handle_cache(rest),
        "clear" => handle_clear(rest),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        "-V" | "--version" | "version" => {
            println!("rade {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => {
            if other.ends_with(".rade.rhp") || other.ends_with(".rade") || other.ends_with(".blade.rhp") || other.ends_with(".rhp") {
                handle_compile(&args[1..])
            } else {
                eprintln!("Error: Unknown command '{other}'.\n");
                print_help();
                process::exit(1);
            }
        }
    };

    if let Err(err) = result {
        eprintln!("{err}");
        process::exit(1);
    }
}
