use std::fs;
use std::path::Path;
use std::process::Command;

use rhp::rpm::{
    discover_package_exports, generate_autoloader, install_all, install_package,
    uninstall_package, update_all, AutoloadConfig, Dependency, DetailedDependency,
    GitHubPackageSpec, InstallOptions, InstalledPackageMeta, LockedPackage, Lockfile, Manifest,
    LOCK_FILENAME, MANIFEST_FILENAME,
};

fn init_git_repo(dir: &Path) {
    let _ = Command::new("git").args(["init", "-b", "main"]).current_dir(dir).output();
    let _ = Command::new("git").args(["config", "user.name", "Test"]).current_dir(dir).output();
    let _ = Command::new("git").args(["config", "user.email", "test@example.com"]).current_dir(dir).output();
    let _ = Command::new("git").args(["add", "."]).current_dir(dir).output();
    let _ = Command::new("git").args(["commit", "-m", "Initial commit"]).current_dir(dir).output();
}

#[test]
fn test_github_spec_parsing() {
    // 1. Standard owner/repo
    let spec = GitHubPackageSpec::parse("vuenice/rhp-router").unwrap();
    assert_eq!(spec.vendor, "vuenice");
    assert_eq!(spec.repo, "rhp-router");
    assert_eq!(spec.git_url, "https://github.com/vuenice/rhp-router.git");
    assert_eq!(spec.branch, None);
    assert_eq!(spec.tag, None);

    // 2. With branch
    let spec_branch = GitHubPackageSpec::parse("vuenice/rhp-router@feature-x").unwrap();
    assert_eq!(spec_branch.vendor, "vuenice");
    assert_eq!(spec_branch.repo, "rhp-router");
    assert_eq!(spec_branch.branch, Some("feature-x".to_string()));

    // 3. With tag
    let spec_tag = GitHubPackageSpec::parse("vuenice/rhp-router#v2.1.0").unwrap();
    assert_eq!(spec_tag.vendor, "vuenice");
    assert_eq!(spec_tag.repo, "rhp-router");
    assert_eq!(spec_tag.tag, Some("v2.1.0".to_string()));

    // 4. Full HTTPS URL
    let spec_https = GitHubPackageSpec::parse("https://github.com/laravel/framework.git").unwrap();
    assert_eq!(spec_https.vendor, "laravel");
    assert_eq!(spec_https.repo, "framework");
    assert_eq!(spec_https.git_url, "https://github.com/laravel/framework.git");

    // 5. Destination path
    let root = Path::new("/my-project");
    let dest = spec.destination_path(root);
    assert_eq!(dest, Path::new("/my-project/vendor/vuenice/rhp-router"));
}

#[test]
fn test_manifest_roundtrip_and_manipulation() {
    let temp_dir = std::env::temp_dir().join(format!("rpm_test_manifest_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(&temp_dir).unwrap();

    let manifest_path = temp_dir.join(MANIFEST_FILENAME);
    let mut manifest = Manifest::new("my-cool-app");
    manifest.package.description = Some("A test application".to_string());
    manifest.package.version = "1.0.0".to_string();

    // Add simple dependency
    manifest.add_dependency("vuenice/rhp-router", Dependency::Simple("main".to_string()));

    // Add detailed dependency
    manifest.add_dependency(
        "symfony/yaml",
        Dependency::Detailed(DetailedDependency {
            git: Some("https://github.com/symfony/yaml.git".to_string()),
            branch: Some("v6.4".to_string()),
            tag: None,
            rev: None,
            version: Some("6.4.0".to_string()),
            path: None,
        }),
    );

    // Add autoload config
    manifest.autoload.psr4.insert("App\\".to_string(), "src/".to_string());
    manifest.autoload.files.push("src/helpers.rhp".to_string());

    // Save
    manifest.save_to_file(&manifest_path).unwrap();

    // Verify file content is valid TOML
    let loaded = Manifest::load_from_file(&manifest_path).unwrap();
    assert_eq!(loaded.package.name, "my-cool-app");
    assert_eq!(loaded.package.version, "1.0.0");
    assert_eq!(loaded.dependencies.len(), 2);
    assert!(loaded.dependencies.contains_key("vuenice/rhp-router"));
    assert!(loaded.dependencies.contains_key("symfony/yaml"));
    assert_eq!(loaded.autoload.psr4.get("App\\").unwrap(), "src/");
    assert_eq!(loaded.autoload.files, vec!["src/helpers.rhp"]);

    // Remove dependency
    assert!(manifest.remove_dependency("vuenice/rhp-router"));
    assert_eq!(manifest.dependencies.len(), 1);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_lockfile_roundtrip_and_cargo_lock_compatibility() {
    let temp_dir = std::env::temp_dir().join(format!("rpm_test_lock_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(&temp_dir).unwrap();

    let lock_path = temp_dir.join(LOCK_FILENAME);
    let mut lockfile = Lockfile::new();
    lockfile.upsert_package(LockedPackage {
        name: "vuenice/rhp-router".to_string(),
        version: "0.1.0".to_string(),
        source: Some("git+https://github.com/vuenice/rhp-router.git#e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string()),
        checksum: Some("sha256:d41d8cd98f00b204e9800998ecf8427e".to_string()),
        dependencies: vec![],
    });

    lockfile.save_to_file(&lock_path).unwrap();

    let content = fs::read_to_string(&lock_path).unwrap();
    assert!(content.contains("# This file is automatically @generated by rpm."));
    assert!(content.contains("version = 1"));
    assert!(content.contains("[[package]]"));
    assert!(content.contains("name = \"vuenice/rhp-router\""));
    assert!(content.contains("checksum = \"sha256:d41d8cd98f00b204e9800998ecf8427e\""));

    // Reload
    let reloaded = Lockfile::load_from_file(&lock_path).unwrap();
    let pkg = reloaded.find_package("vuenice/rhp-router").unwrap();
    assert_eq!(pkg.version, "0.1.0");
    assert_eq!(
        reloaded.get_locked_commit("vuenice/rhp-router"),
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string())
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_php_rules_export_discovery_from_composer_json() {
    let temp_dir = std::env::temp_dir().join(format!("rpm_test_composer_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(&temp_dir).unwrap();

    let composer_content = r#"{
        "name": "acme/math-lib",
        "description": "Math library for testing",
        "autoload": {
            "psr-4": {
                "Acme\\Math\\": "src/"
            },
            "files": [
                "src/functions.php"
            ]
        }
    }"#;
    fs::write(temp_dir.join("composer.json"), composer_content).unwrap();

    let exports = discover_package_exports(&temp_dir, "acme/math-lib");
    assert_eq!(exports.psr4.get("Acme\\Math\\").unwrap(), "src/");
    assert_eq!(exports.files, vec!["src/functions.php"]);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_autoloader_generation_and_execution() {
    let temp_dir = std::env::temp_dir().join(format!("rpm_test_autoload_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let pkg_dir = temp_dir.join("vendor").join("acme").join("greeting");
    fs::create_dir_all(pkg_dir.join("src")).unwrap();

    // Create a package file declaring a namespaced class
    let pkg_code = r#"
namespace Acme\Greeting;

class Greeter {
    public static function sayHello(): string {
        return "Hello from Acme Greeting!";
    }
}
"#;
    fs::write(pkg_dir.join("src").join("Greeter.rhp"), pkg_code).unwrap();

    let mut auto = AutoloadConfig::default();
    auto.psr4.insert("Acme\\Greeting\\".to_string(), "src/".to_string());

    let installed = vec![InstalledPackageMeta {
        name: "acme/greeting".to_string(),
        version: "1.0.0".to_string(),
        source: Some("https://github.com/acme/greeting.git".to_string()),
        checksum: Some("sha256:1234567890abcdef".to_string()),
        path: "vendor/acme/greeting".to_string(),
        autoload: auto,
    }];

    let autoload_path = generate_autoloader(&temp_dir, &installed).unwrap();
    assert!(autoload_path.exists());
    assert!(temp_dir.join("vendor/autoload.php").exists());
    assert!(temp_dir.join("vendor/rpm/installed.json").exists());
    assert!(temp_dir.join("vendor/rpm/autoload_psr4.rhp").exists());
    assert!(temp_dir.join("vendor/rpm/autoload_classmap.rhp").exists());

    // Verify autoloader content contains the exported class and namespace reset
    let autoload_content = fs::read_to_string(&autoload_path).unwrap();
    assert!(autoload_content.contains("class Greeter"));
    assert!(autoload_content.contains("namespace;"));
    assert!(autoload_content.contains("class RPM"));

    // Execute consumer script that uses Acme\Greeting\Greeter
    let consumer_script = format!(
        "{}\n\nuse Acme\\Greeting\\Greeter;\n$msg = Greeter::sayHello();\necho $msg;\n",
        autoload_content
    );

    let (vm, _) = rhp::vm::run_source_with_path_vm_and_jit(&consumer_script, "test.rhp", false).unwrap();
    assert_eq!(vm.output(), "Hello from Acme Greeting!");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_end_to_end_install_update_uninstall_workflow() {
    let base_temp = std::env::temp_dir().join(format!("rpm_test_e2e_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let remote_repo = base_temp.join("mock_github_repo");
    let project_dir = base_temp.join("my_rhp_app");

    fs::create_dir_all(remote_repo.join("src")).unwrap();
    fs::create_dir_all(&project_dir).unwrap();

    // Create package in mock remote repo
    let composer_json = r#"{
        "name": "calc/operations",
        "autoload": {
            "psr-4": {
                "Calc\\Operations\\": "src/"
            }
        }
    }"#;
    fs::write(remote_repo.join("composer.json"), composer_json).unwrap();

    let class_file = r#"
namespace Calc\Operations;

class Calculator {
    public static function multiply(int $a, int $b): int {
        return $a * $b;
    }
}
"#;
    fs::write(remote_repo.join("src").join("Calculator.rhp"), class_file).unwrap();

    // Initialize mock git repo
    init_git_repo(&remote_repo);

    // 1. Install package using local git repository URL (simulating GitHub)
    let git_url = format!("file://{}", remote_repo.display());
    let mut options = InstallOptions::default();
    options.git = Some(git_url.clone());

    let res = install_package(&project_dir, "calc/operations", &options).unwrap();
    assert_eq!(res.name, "calc/operations");
    assert!(project_dir.join("rpm.toml").exists());
    assert!(project_dir.join("rpm.lock").exists());
    assert!(project_dir.join("vendor/calc/operations/src/Calculator.rhp").exists());
    assert!(project_dir.join("vendor/autoload.rhp").exists());

    // 2. Run user script using the installed package!
    let autoload_code = fs::read_to_string(project_dir.join("vendor/autoload.rhp")).unwrap();
    let user_code = format!(
        "{}\n\nuse Calc\\Operations\\Calculator;\n$result = Calculator::multiply(6, 7);\necho string($result);\n",
        autoload_code
    );
    let (vm, _) = rhp::vm::run_source_with_path_vm_and_jit(&user_code, "index.rhp", false).unwrap();
    assert_eq!(vm.output(), "42");

    // 3. Test `install_all`
    // Add another mock repo
    let remote_repo2 = base_temp.join("mock_github_repo2");
    fs::create_dir_all(remote_repo2.join("src")).unwrap();
    fs::write(remote_repo2.join("composer.json"), r#"{"name": "helper/strings"}"#).unwrap();
    fs::write(remote_repo2.join("src").join("Strings.rhp"), r#"
namespace Helper\Strings;

class Strings {
    public static function shout(string $s): string {
        return $s . "!";
    }
}
"#).unwrap();
    init_git_repo(&remote_repo2);

    let mut manifest = Manifest::load_from_file(&project_dir.join("rpm.toml")).unwrap();
    manifest.add_dependency("helper/strings", Dependency::Detailed(DetailedDependency {
        git: Some(format!("file://{}", remote_repo2.display())),
        branch: None,
        tag: None,
        rev: None,
        version: Some("0.1.0".to_string()),
        path: None,
    }));
    manifest.save_to_file(&project_dir.join("rpm.toml")).unwrap();

    let all_installed = install_all(&project_dir, &InstallOptions::default()).unwrap();
    assert_eq!(all_installed.len(), 2);
    assert!(project_dir.join("vendor/helper/strings/src/Strings.rhp").exists());

    // 4. Test `update_all`
    // Make a change in the remote repo
    fs::write(remote_repo.join("src").join("Calculator.rhp"), r#"
namespace Calc\Operations;

class Calculator {
    public static function multiply(int $a, int $b): int {
        return $a * $b;
    }
    public static function add(int $a, int $b): int {
        return $a + $b;
    }
}
"#).unwrap();
    let _ = Command::new("git").args(["add", "."]).current_dir(&remote_repo).output();
    let _ = Command::new("git").args(["commit", "-m", "Add add method"]).current_dir(&remote_repo).output();

    let update_results = update_all(&project_dir).unwrap();
    assert_eq!(update_results.len(), 2);
    assert!(update_results.iter().any(|r| r.name == "calc/operations"));

    // Verify updated method exists
    let updated_autoload = fs::read_to_string(project_dir.join("vendor/autoload.rhp")).unwrap();
    assert!(updated_autoload.contains("public static function add"));

    // 4. Test `uninstall_package`
    let uninstalled = uninstall_package(&project_dir, "calc/operations").unwrap();
    assert!(uninstalled);

    // Verify rpm.toml and rpm.lock no longer have the package
    let manifest = Manifest::load_from_file(&project_dir.join("rpm.toml")).unwrap();
    assert!(!manifest.dependencies.contains_key("calc/operations"));

    let lockfile = Lockfile::load_from_file(&project_dir.join("rpm.lock")).unwrap();
    assert!(lockfile.find_package("calc/operations").is_none());

    // Verify vendor/ directory was cleaned up
    assert!(!project_dir.join("vendor/calc/operations").exists());

    let _ = fs::remove_dir_all(&base_temp);
}
