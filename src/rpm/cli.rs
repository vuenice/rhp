use std::env;
use std::path::{Path, PathBuf};
use std::process;
use std::time::Instant;

use super::error::RpmError;
use super::manifest::{Manifest, MANIFEST_FILENAME};
use super::resolver::{
    init_project, install_all, install_package, uninstall_package, update_all, InstallOptions,
};

pub fn print_rpm_help() {
    println!(
        concat!(
            "RPM: High-Performance Package Manager for RHP\n\n",
            "USAGE:\n",
            "    rpm <COMMAND> [OPTIONS]\n",
            "    rhp rpm <COMMAND> [OPTIONS]\n\n",
            "COMMANDS:\n",
            "    install              Install all dependencies declared in rpm.toml (or rpm.lock)\n",
            "    install <PACKAGE>    Install package directly from GitHub (e.g. 'owner/repo')\n",
            "    update               Update all installed packages to latest GitHub versions\n",
            "    un <PACKAGE>         Uninstall package, remove from rpm.toml and delete from vendor/\n",
            "    uninstall <PACKAGE>  Alias for 'un'\n",
            "    remove <PACKAGE>     Alias for 'un'\n",
            "    init [NAME]          Initialize a new rpm.toml manifest in current directory\n",
            "    list                 List installed packages in vendor/\n",
            "    help, --help         Display this help message\n\n",
            "OPTIONS for 'install <PACKAGE>':\n",
            "    --branch <BRANCH>    Specify git branch to clone (e.g. --branch=main)\n",
            "    --tag <TAG>          Specify git tag or release (e.g. --tag=v1.2.0)\n",
            "    --rev <COMMIT>       Specify exact git commit hash\n",
            "    --git <URL>          Specify custom git repository URL\n",
            "    --dev                Add as development dependency in [dev-dependencies]\n\n",
            "OPTIONS for 'install':\n",
            "    --frozen             Strict install respecting exact commits in rpm.lock\n\n",
            "EXAMPLES:\n",
            "    rpm install\n",
            "    rpm install \"vuenice/rhp-router\"\n",
            "    rpm install \"vuenice/rhp-router\" --branch=main\n",
            "    rpm update\n",
            "    rpm un \"vuenice/rhp-router\"\n"
        )
    );
}

/// Entrypoint for RPM CLI subcommands.
pub fn run_rpm_cli(args: &[String]) {
    if args.is_empty() {
        print_rpm_help();
        process::exit(0);
    }

    let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match args[0].as_str() {
        "--help" | "-h" | "help" => {
            print_rpm_help();
            process::exit(0);
        }
        "--version" | "-v" | "-V" => {
            println!("rpm {} (rhp package manager)", env!("CARGO_PKG_VERSION"));
            process::exit(0);
        }
        "init" => {
            handle_init(&root_dir, &args[1..]);
        }
        "install" | "i" | "add" => {
            handle_install(&root_dir, &args[1..]);
        }
        "update" | "up" | "upgrade" => {
            handle_update(&root_dir);
        }
        "un" | "uninstall" | "remove" | "rm" => {
            handle_uninstall(&root_dir, &args[1..]);
        }
        "list" | "ls" => {
            handle_list(&root_dir);
        }
        unknown => {
            eprintln!("Error: Unknown RPM command '{}'. Run 'rpm --help' for usage.", unknown);
            process::exit(1);
        }
    }
}

fn handle_init(root_dir: &Path, args: &[String]) {
    let custom_name = args.first().map(|s| s.as_str());
    match init_project(root_dir, custom_name) {
        Ok(path) => {
            println!("✨ Created '{}'", path.display());
            println!("💡 Run 'rpm install <package>' to add your first dependency.");
            process::exit(0);
        }
        Err(e) => {
            eprintln!("Error initializing project: {}", e);
            process::exit(1);
        }
    }
}

fn handle_install(root_dir: &Path, args: &[String]) {
    let start = Instant::now();
    let mut package_arg: Option<String> = None;
    let mut options = InstallOptions::default();

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--help" || arg == "-h" {
            println!("Usage: rpm install [PACKAGE] [--branch BRANCH] [--tag TAG] [--rev COMMIT] [--git URL] [--dev] [--frozen]");
            process::exit(0);
        } else if arg == "--dev" {
            options.dev = true;
        } else if arg == "--frozen" {
            options.frozen = true;
        } else if let Some(val) = arg.strip_prefix("--branch=") {
            options.branch = Some(val.to_string());
        } else if arg == "--branch" || arg == "-b" {
            if let Some(next) = iter.next() {
                options.branch = Some(next.clone());
            }
        } else if let Some(val) = arg.strip_prefix("--tag=") {
            options.tag = Some(val.to_string());
        } else if arg == "--tag" || arg == "-t" {
            if let Some(next) = iter.next() {
                options.tag = Some(next.clone());
            }
        } else if let Some(val) = arg.strip_prefix("--rev=") {
            options.rev = Some(val.to_string());
        } else if arg == "--rev" {
            if let Some(next) = iter.next() {
                options.rev = Some(next.clone());
            }
        } else if let Some(val) = arg.strip_prefix("--git=") {
            options.git = Some(val.to_string());
        } else if arg == "--git" {
            if let Some(next) = iter.next() {
                options.git = Some(next.clone());
            }
        } else if !arg.starts_with('-') && package_arg.is_none() {
            package_arg = Some(arg.clone());
        }
    }

    if let Some(pkg_spec) = package_arg {
        // Install single package
        println!("📦 Resolving '{}' from GitHub...", pkg_spec);
        match install_package(root_dir, &pkg_spec, &options) {
            Ok(res) => {
                let short_commit = if res.commit.len() >= 7 {
                    &res.commit[..7]
                } else {
                    &res.commit
                };
                let elapsed = start.elapsed().as_secs_f64();
                println!(
                    "✅ Installed {} v{} ({}) in {:.2}s",
                    res.name, res.version, short_commit, elapsed
                );
                println!("📁 Files placed in: {}", res.path.display());
                println!("🚀 Autoloader generated at: vendor/autoload.rhp");
                process::exit(0);
            }
            Err(e) => {
                eprintln!("❌ Failed to install '{}': {}", pkg_spec, e);
                process::exit(1);
            }
        }
    } else {
        // Install all dependencies declared in rpm.toml
        println!("📦 Reading manifest and resolving dependencies...");
        match install_all(root_dir, &options) {
            Ok(results) => {
                let elapsed = start.elapsed().as_secs_f64();
                if results.is_empty() {
                    println!("ℹ️  No dependencies listed in '{}'.", MANIFEST_FILENAME);
                } else {
                    println!("✅ Installed {} package(s) in {:.2}s:", results.len(), elapsed);
                    for item in results {
                        let short_commit = if item.commit.len() >= 7 {
                            &item.commit[..7]
                        } else {
                            &item.commit
                        };
                        println!("  • {} v{} ({})", item.name, item.version, short_commit);
                    }
                    println!("🚀 Autoloader generated at: vendor/autoload.rhp");
                }
                process::exit(0);
            }
            Err(RpmError::ManifestNotFound(_)) => {
                eprintln!("❌ No '{}' found in current directory.", MANIFEST_FILENAME);
                eprintln!("💡 Run 'rpm init' to initialize an RHP project, or run 'rpm install <package>' directly.");
                process::exit(1);
            }
            Err(e) => {
                eprintln!("❌ Installation failed: {}", e);
                process::exit(1);
            }
        }
    }
}

fn handle_update(root_dir: &Path) {
    let start = Instant::now();
    println!("🔄 Checking for updates from GitHub...");
    match update_all(root_dir) {
        Ok(results) => {
            let elapsed = start.elapsed().as_secs_f64();
            if results.is_empty() {
                println!("ℹ️  No packages to update.");
            } else {
                println!("✅ Updated {} package(s) in {:.2}s:", results.len(), elapsed);
                for item in results {
                    let short_commit = if item.commit.len() >= 7 {
                        &item.commit[..7]
                    } else {
                        &item.commit
                    };
                    println!("  • {} -> {}", item.name, short_commit);
                }
                println!("🚀 Autoloader refreshed at: vendor/autoload.rhp");
            }
            process::exit(0);
        }
        Err(e) => {
            eprintln!("❌ Update failed: {}", e);
            process::exit(1);
        }
    }
}

fn handle_uninstall(root_dir: &Path, args: &[String]) {
    let start = Instant::now();
    let package_name = match args.first() {
        Some(name) => name.trim().trim_matches(|c| c == '\'' || c == '"'),
        None => {
            eprintln!("Error: Missing package name to uninstall. Usage: rpm un <package_name>");
            process::exit(1);
        }
    };

    println!("🗑️  Uninstalling '{}'...", package_name);
    match uninstall_package(root_dir, package_name) {
        Ok(_) => {
            let elapsed = start.elapsed().as_secs_f64();
            println!("✅ Uninstalled '{}' in {:.2}s.", package_name, elapsed);
            println!("🚀 Autoloader refreshed at: vendor/autoload.rhp");
            process::exit(0);
        }
        Err(e) => {
            eprintln!("❌ Failed to uninstall '{}': {}", package_name, e);
            process::exit(1);
        }
    }
}

fn handle_list(root_dir: &Path) {
    let manifest_path = match Manifest::find_in_dir(root_dir) {
        Some(p) => p,
        None => {
            eprintln!("No '{}' found in current directory.", MANIFEST_FILENAME);
            process::exit(1);
        }
    };

    match Manifest::load_from_file(&manifest_path) {
        Ok(m) => {
            println!("📦 Project: {} (v{})", m.package.name, m.package.version);
            if m.dependencies.is_empty() {
                println!("  No dependencies installed.");
            } else {
                println!("  Dependencies ({}):", m.dependencies.len());
                for (name, dep) in &m.dependencies {
                    let target_dir = root_dir.join("vendor").join(name);
                    let status = if target_dir.exists() { "installed" } else { "missing" };
                    println!("    • {} ({:?}) [{}]", name, dep, status);
                }
            }
            process::exit(0);
        }
        Err(e) => {
            eprintln!("Failed to read manifest: {}", e);
            process::exit(1);
        }
    }
}
