use std::path::Path;
use std::process;
use std::time::Instant;

use super::error::PkgError;
use super::manifest::{Manifest, MANIFEST_FILENAME};
use super::resolver::{
    add_package, init_project, install, remove_package, update, AddOptions, InstallOptions,
};

pub fn handle_add(root_dir: &Path, args: &[String]) {
    let start = Instant::now();
    let mut package_arg: Option<String> = None;
    let mut options = AddOptions::default();

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--help" || arg == "-h" {
            println!("Usage: rhp add <PACKAGE> [--branch BRANCH] [--tag TAG] [--rev COMMIT] [--git URL] [--path PATH] [--dev]");
            println!("\nExamples:");
            println!("    rhp add vuenice/rhp-router");
            println!("    rhp add vuenice/rhp-router --branch=main");
            println!("    rhp add https://github.com/symfony/yaml.git");
            println!("    rhp add ./packages/my-lib");
            process::exit(0);
        } else if arg == "--dev" {
            options.dev = true;
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
        } else if let Some(val) = arg.strip_prefix("--path=") {
            options.path = Some(val.to_string());
        } else if arg == "--path" {
            if let Some(next) = iter.next() {
                options.path = Some(next.clone());
            }
        } else if !arg.starts_with('-') && package_arg.is_none() {
            package_arg = Some(arg.clone());
        }
    }

    let pkg_spec = match package_arg {
        Some(pkg) => pkg,
        None => {
            eprintln!("Error: Missing package name. Usage: rhp add <PACKAGE>");
            process::exit(1);
        }
    };

    println!("📦 Resolving '{}'...", pkg_spec);
    match add_package(root_dir, &pkg_spec, &options) {
        Ok(res) => {
            let short_commit = if res.commit.len() >= 7 {
                &res.commit[..7]
            } else {
                &res.commit
            };
            let elapsed = start.elapsed().as_secs_f64();
            println!(
                "✅ Added {} v{} ({}) in {:.2}s",
                res.name, res.version, short_commit, elapsed
            );
            println!("📁 Files placed in: {}", res.path.display());
            println!("🚀 Autoloader updated at: vendor/autoload.rhp");
            process::exit(0);
        }
        Err(e) => {
            eprintln!("❌ Failed to add '{}': {}", pkg_spec, e);
            process::exit(1);
        }
    }
}

pub fn handle_remove(root_dir: &Path, args: &[String]) {
    let start = Instant::now();
    let package_name = match args.first() {
        Some(name) if name == "--help" || name == "-h" => {
            println!("Usage: rhp remove <PACKAGE>");
            process::exit(0);
        }
        Some(name) => name.trim().trim_matches(|c| c == '\'' || c == '"'),
        None => {
            eprintln!("Error: Missing package name to remove. Usage: rhp remove <PACKAGE>");
            process::exit(1);
        }
    };

    println!("🗑️  Removing '{}'...", package_name);
    match remove_package(root_dir, package_name) {
        Ok(_) => {
            let elapsed = start.elapsed().as_secs_f64();
            println!("✅ Removed '{}' in {:.2}s.", package_name, elapsed);
            println!("🚀 Autoloader refreshed at: vendor/autoload.rhp");
            process::exit(0);
        }
        Err(e) => {
            eprintln!("❌ Failed to remove '{}': {}", package_name, e);
            process::exit(1);
        }
    }
}

pub fn handle_install(root_dir: &Path, args: &[String]) {
    let start = Instant::now();
    let mut options = InstallOptions::default();

    for arg in args {
        if arg == "--help" || arg == "-h" {
            println!("Usage: rhp install [--frozen]");
            println!("\nInstalls all dependencies from rhp.lock (or rhp.toml).");
            println!("Options:");
            println!("    --frozen    Enforce exact lockfile matches without resolving new versions");
            process::exit(0);
        } else if arg == "--frozen" {
            options.frozen = true;
        } else if !arg.starts_with('-') {
            eprintln!("Error: 'rhp install' installs all dependencies from the lockfile.");
            eprintln!("💡 To add a new dependency, use 'rhp add {}'", arg);
            process::exit(1);
        }
    }

    println!("📦 Reading manifest and installing dependencies...");
    match install(root_dir, &options) {
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
        Err(PkgError::ManifestNotFound(_)) => {
            eprintln!("❌ No '{}' found in current directory.", MANIFEST_FILENAME);
            eprintln!("💡 Run 'rhp init' to initialize an RHP project, or 'rhp add <package>' to add a dependency.");
            process::exit(1);
        }
        Err(e) => {
            eprintln!("❌ Installation failed: {}", e);
            process::exit(1);
        }
    }
}

pub fn handle_update(root_dir: &Path, args: &[String]) {
    let start = Instant::now();
    let mut target_pkg: Option<&str> = None;

    for arg in args {
        if arg == "--help" || arg == "-h" {
            println!("Usage: rhp update [PACKAGE]");
            println!("\nUpdates dependencies to latest versions within constraints.");
            process::exit(0);
        } else if !arg.starts_with('-') && target_pkg.is_none() {
            target_pkg = Some(arg.as_str());
        }
    }

    if let Some(pkg) = target_pkg {
        println!("🔄 Checking for updates for '{}'...", pkg);
    } else {
        println!("🔄 Checking for updates for all dependencies...");
    }

    match update(root_dir, target_pkg) {
        Ok(results) => {
            let elapsed = start.elapsed().as_secs_f64();
            if results.is_empty() {
                println!("ℹ️  No packages updated.");
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

pub fn handle_init(root_dir: &Path, args: &[String]) {
    let custom_name = args.first().and_then(|s| if s.starts_with('-') { None } else { Some(s.as_str()) });
    match init_project(root_dir, custom_name) {
        Ok(path) => {
            println!("✨ Created '{}'", path.display());
            println!("💡 Run 'rhp add <package>' to add your first dependency.");
            process::exit(0);
        }
        Err(e) => {
            eprintln!("Error initializing project: {}", e);
            process::exit(1);
        }
    }
}

pub fn handle_list(root_dir: &Path) {
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
