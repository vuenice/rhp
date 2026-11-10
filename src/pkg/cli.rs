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
    let mut custom_name: Option<String> = None;
    let mut package_type = super::manifest::PackageType::Rhp;
    let mut safety = super::manifest::PackageSafety::Safe;
    let mut has_type_flag = false;

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--help" || arg == "-h" {
            println!("Usage: rhp init [PACKAGE_NAME] [--type=rhp|native-rust|wasm|hybrid] [--safety=safe|unsafe]");
            println!("\nExamples:");
            println!("    rhp init");
            println!("    rhp init my-package");
            println!("    rhp init my-mongo --type=native-rust --safety=unsafe");
            println!("    rhp init my-filter --type=wasm");
            println!("    rhp init my-driver --type=hybrid");
            process::exit(0);
        } else if let Some(val) = arg.strip_prefix("--type=") {
            if let Some(t) = super::manifest::PackageType::from_str_loose(val) {
                package_type = t;
                has_type_flag = true;
            }
        } else if arg == "--type" {
            if let Some(next) = iter.next() {
                if let Some(t) = super::manifest::PackageType::from_str_loose(next) {
                    package_type = t;
                    has_type_flag = true;
                }
            }
        } else if let Some(val) = arg.strip_prefix("--safety=") {
            if let Some(s) = super::manifest::PackageSafety::from_str_loose(val) {
                safety = s;
            }
        } else if arg == "--safety" {
            if let Some(next) = iter.next() {
                if let Some(s) = super::manifest::PackageSafety::from_str_loose(next) {
                    safety = s;
                }
            }
        } else if !arg.starts_with('-') && custom_name.is_none() {
            custom_name = Some(arg.clone());
        }
    }

    if has_type_flag || (custom_name.is_some() && package_type != super::manifest::PackageType::Rhp) {
        let (target, name) = if let Some(ref raw_name) = custom_name {
            let p = Path::new(raw_name);
            let dir = if p.is_absolute() {
                p.to_path_buf()
            } else {
                root_dir.join(raw_name)
            };
            let pkg_ident = p.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(raw_name.as_str())
                .to_string();
            (dir, pkg_ident)
        } else {
            let ident = root_dir.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("my-package")
                .to_string();
            (root_dir.to_path_buf(), ident)
        };

        let opts = super::scaffold::ScaffoldOptions {
            name: name.clone(),
            package_type,
            safety,
            description: Some(format!("{} package", package_type.as_str())),
        };

        match super::scaffold::scaffold_package(&target, &opts) {
            Ok(()) => {
                println!("✨ Initialized {} [{}] (safety: {}) in '{}'", name, package_type.as_str(), safety.as_str(), target.display());
                if package_type == super::manifest::PackageType::NativeRust {
                    println!("🚀 Built with Typed C-ABI and Opaque Handle registry.");
                } else if package_type == super::manifest::PackageType::Wasm {
                    println!("🛡️  Configured with sandboxed WebAssembly execution and MessagePack marshalling.");
                }
                process::exit(0);
            }
            Err(e) => {
                eprintln!("Error scaffolding package: {}", e);
                process::exit(1);
            }
        }
    }

    match init_project(root_dir, custom_name.as_deref()) {
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

pub fn handle_audit(root_dir: &Path, args: &[String]) {
    let mut target_path = root_dir.to_path_buf();
    let mut as_json = false;

    for arg in args {
        if arg == "--help" || arg == "-h" {
            println!("Usage: rhp audit [PATH] [--json]");
            println!("\nAudits package and dependencies for security, safety compliance, and network egress.");
            process::exit(0);
        } else if arg == "--json" {
            as_json = true;
        } else if !arg.starts_with('-') {
            target_path = Path::new(arg).to_path_buf();
        }
    }

    match super::audit::audit_package(&target_path) {
        Ok(report) => {
            if as_json {
                let json = serde_json::to_string_pretty(&report).unwrap_or_default();
                println!("{}", json);
            } else {
                print!("{}", super::audit::format_audit_report(&report));
            }

            if report.is_clean() {
                process::exit(0);
            } else {
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("❌ Audit failed: {}", e);
            process::exit(1);
        }
    }
}

pub fn handle_package_command(root_dir: &Path, args: &[String]) {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!("Usage: rhp package <SUBCOMMAND>");
        println!("\nSubcommands:");
        println!("    init <NAME> [--type=...] [--safety=...]  Scaffold a new RHP or native package");
        println!("    audit [PATH] [--json]                   Audit safety rules and network egress");
        println!("    gen-types [PATH]                        Generate .d.rhp static type definitions");
        println!("    list                                    List installed dependencies");
        process::exit(0);
    }

    match args[0].as_str() {
        "init" => handle_init(root_dir, &args[1..]),
        "audit" => handle_audit(root_dir, &args[1..]),
        "gen-types" | "types" => {
            let target = args.get(1).map(Path::new).unwrap_or(root_dir);
            match super::types::generate_type_declarations(target) {
                Ok(path) => {
                    println!("📝 Generated type signatures at '{}'", path.display());
                    process::exit(0);
                }
                Err(e) => {
                    eprintln!("❌ Failed to generate type signatures: {}", e);
                    process::exit(1);
                }
            }
        }
        "list" | "ls" => handle_list(root_dir),
        _ => {
            eprintln!("Unknown package subcommand: '{}'. Run 'rhp package --help' for usage.", args[0]);
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
            let pkg_type = m.package.package_type().as_str();
            let safety = if m.package.is_safe() { "safe" } else { "unsafe" };
            println!("📦 Project: {} (v{}) [type: {}, safety: {}]", m.package.name, m.package.version, pkg_type, safety);
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
