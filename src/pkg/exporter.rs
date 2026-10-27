use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::error::{PkgError, Result};
use super::manifest::{AutoloadConfig, Manifest};

/// Metadata regarding an installed package discovered in `vendor/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackageMeta {
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    pub checksum: Option<String>,
    pub path: String,
    pub autoload: AutoloadConfig,
}

/// Information parsed from a PHP `composer.json` for interoperability.
#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
struct ComposerJson {
    pub name: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub autoload: Option<ComposerAutoload>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ComposerAutoload {
    #[serde(default, rename = "psr-4")]
    pub psr4: BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "psr-0")]
    pub psr0: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub classmap: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

/// Inspect an installed package directory in `vendor/` and determine its autoload & export config.
pub fn discover_package_exports(pkg_dir: &Path, pkg_name: &str) -> AutoloadConfig {
    // 1. Try reading `rpm.toml` or `rhp.toml`
    if let Some(manifest_path) = Manifest::find_in_dir(pkg_dir) {
        if let Ok(manifest) = Manifest::load_from_file(&manifest_path) {
            let auto = manifest.effective_autoload();
            if !auto.psr4.is_empty() || !auto.files.is_empty() || !auto.classmap.is_empty() {
                return auto;
            }
        }
    }

    // 2. Try reading `composer.json` (PHP standard)
    let composer_path = pkg_dir.join("composer.json");
    if composer_path.exists() {
        if let Ok(content) = fs::read_to_string(&composer_path) {
            if let Ok(composer) = serde_json::from_str::<ComposerJson>(&content) {
                if let Some(auto) = composer.autoload {
                    let mut config = AutoloadConfig::default();
                    for (k, v) in auto.psr4 {
                        if let Some(path_str) = v.as_str() {
                            config.psr4.insert(k, path_str.to_string());
                        } else if let Some(arr) = v.as_array() {
                            if let Some(first) = arr.first().and_then(|x| x.as_str()) {
                                config.psr4.insert(k, first.to_string());
                            }
                        }
                    }
                    for (k, v) in auto.psr0 {
                        if let Some(path_str) = v.as_str() {
                            config.psr0.insert(k, path_str.to_string());
                        }
                    }
                    config.classmap = auto.classmap;
                    config.files = auto.files;

                    if !config.psr4.is_empty() || !config.files.is_empty() || !config.classmap.is_empty() {
                        return config;
                    }
                }
            }
        }
    }

    // 3. Fallback: Auto-discovery following standard PHP conventions
    let mut config = AutoloadConfig::default();
    let src_dir = pkg_dir.join("src");
    if src_dir.is_dir() {
        // Derive PSR-4 namespace from package name, e.g. "vuenice/rhp-router" -> "Vuenice\\Router\\"
        let ns = package_name_to_psr4_namespace(pkg_name);
        config.psr4.insert(ns, "src/".to_string());
        config.classmap.push("src/".to_string());
    } else {
        // Map root
        config.classmap.push(".".to_string());
    }

    config
}

/// Convert package name like "vuenice/rhp-router" to a default PSR-4 namespace like "Vuenice\\RhpRouter\\"
pub fn package_name_to_psr4_namespace(pkg_name: &str) -> String {
    let parts: Vec<&str> = pkg_name.split('/').collect();
    let mut ns_parts = Vec::new();
    for part in parts {
        let clean = part.replace(['-', '_'], " ");
        let words: Vec<String> = clean
            .split_whitespace()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            })
            .collect();
        ns_parts.push(words.join(""));
    }
    format!("{}\\", ns_parts.join("\\"))
}

/// Scans installed packages, discovers export rules following PHP standards,
/// and writes `vendor/autoload.rhp`, `vendor/autoload.php`, and `vendor/rhp/` files.
pub fn generate_autoloader(
    root_dir: &Path,
    installed: &[InstalledPackageMeta],
) -> Result<PathBuf> {
    let vendor_dir = root_dir.join("vendor");
    let rhp_dir = vendor_dir.join("rhp");
    fs::create_dir_all(&rhp_dir)?;

    // 1. Write `vendor/rhp/installed.json`
    let installed_json = serde_json::to_string_pretty(installed)
        .map_err(|e| PkgError::Autoload(e.to_string()))?;
    fs::write(rhp_dir.join("installed.json"), &installed_json)?;
    // Backward compat alias
    let rpm_dir = vendor_dir.join("rpm");
    if let Ok(_) = fs::create_dir_all(&rpm_dir) {
        let _ = fs::write(rpm_dir.join("installed.json"), &installed_json);
    }

    // 2. Collect PSR-4 mappings, classmap mappings, and files to load
    let mut psr4_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut files_to_load: Vec<String> = Vec::new();
    let mut class_map: BTreeMap<String, String> = BTreeMap::new();
    let mut code_chunks: Vec<String> = Vec::new();

    for pkg in installed {
        let pkg_rel_path = &pkg.path;
        let pkg_full_dir = root_dir.join(pkg_rel_path);

        // Collect files
        for f in &pkg.autoload.files {
            let rel_file = format!("{}/{}", pkg_rel_path.trim_end_matches('/'), f.trim_start_matches('/'));
            files_to_load.push(rel_file);
        }

        // Collect PSR-4
        for (ns, dir) in &pkg.autoload.psr4 {
            let rel_target = format!("{}/{}", pkg_rel_path.trim_end_matches('/'), dir.trim_start_matches('/'));
            psr4_map.entry(ns.clone()).or_default().push(rel_target);
        }

        // Scan package `.rhp` and `.php` files to discover declared classes/functions
        scan_and_collect_package_code(&pkg_full_dir, pkg_rel_path, &mut class_map, &mut code_chunks);
    }

    // 3. Write `vendor/rhp/autoload_psr4.rhp`
    let mut psr4_code = String::from("<?php\n// PSR-4 Autoload Map\nreturn [\n");
    for (ns, dirs) in &psr4_map {
        let dirs_quoted: Vec<String> = dirs.iter().map(|d| format!("$vendorDir . '/{}'", d.trim_start_matches("vendor/"))).collect();
        psr4_code.push_str(&format!("    '{}' => [{}],\n", ns.replace('\\', "\\\\"), dirs_quoted.join(", ")));
    }
    psr4_code.push_str("];\n");
    fs::write(rhp_dir.join("autoload_psr4.rhp"), &psr4_code)?;
    let _ = fs::write(rpm_dir.join("autoload_psr4.rhp"), &psr4_code);

    // 4. Write `vendor/rhp/autoload_files.rhp`
    let mut files_code = String::from("<?php\n// Autoload Files\nreturn [\n");
    for f in &files_to_load {
        files_code.push_str(&format!("    $vendorDir . '/{}',\n", f.trim_start_matches("vendor/")));
    }
    files_code.push_str("];\n");
    fs::write(rhp_dir.join("autoload_files.rhp"), &files_code)?;
    let _ = fs::write(rpm_dir.join("autoload_files.rhp"), &files_code);

    // 5. Write `vendor/rhp/autoload_classmap.rhp`
    let mut classmap_code = String::from("<?php\n// Classmap Autoload\nreturn [\n");
    for (cls, path) in &class_map {
        classmap_code.push_str(&format!("    '{}' => $vendorDir . '/{}',\n", cls.replace('\\', "\\\\"), path.trim_start_matches("vendor/")));
    }
    classmap_code.push_str("];\n");
    fs::write(rhp_dir.join("autoload_classmap.rhp"), &classmap_code)?;
    let _ = fs::write(rpm_dir.join("autoload_classmap.rhp"), &classmap_code);

    // 6. Write `vendor/autoload.rhp`
    let mut rhp_autoloader = String::new();
    rhp_autoloader.push_str("// RHP Autoloader\n");
    rhp_autoloader.push_str("// This file is automatically generated by rhp. Do not edit manually.\n\n");

    // Pre-declare and export all package symbols so they are in global/namespace scope
    if !code_chunks.is_empty() {
        rhp_autoloader.push_str("// -------------------------------------------------------------------------\n");
        rhp_autoloader.push_str("// Exported Package Symbols\n");
        rhp_autoloader.push_str("// -------------------------------------------------------------------------\n\n");
        for chunk in code_chunks {
            rhp_autoloader.push_str(&chunk);
            rhp_autoloader.push_str("\n\n");
        }
    }

    // Reset namespace back to root so consumer script runs in global scope
    rhp_autoloader.push_str("namespace;\n\n");

    // Standard RHP Package Helper Class
    rhp_autoloader.push_str("class RHP {\n");
    rhp_autoloader.push_str("    public static function version(): string {\n");
    rhp_autoloader.push_str("        return \"0.1.0\";\n");
    rhp_autoloader.push_str("    }\n\n");
    rhp_autoloader.push_str("    public static function packages(): array {\n");
    rhp_autoloader.push_str("        return [\n");
    for pkg in installed {
        rhp_autoloader.push_str(&format!("            \"{}\" => \"{}\",\n", pkg.name, pkg.version));
    }
    rhp_autoloader.push_str("        ];\n");
    rhp_autoloader.push_str("    }\n");
    rhp_autoloader.push_str("}\n\n");

    // Backwards compatibility alias
    rhp_autoloader.push_str("class RPM extends RHP {}\n\n");

    let autoload_rhp_path = vendor_dir.join("autoload.rhp");
    fs::write(&autoload_rhp_path, rhp_autoloader)?;

    // 7. Write `vendor/autoload.php` for PHP compatibility
    let mut php_autoloader = String::new();
    php_autoloader.push_str("<?php\n");
    php_autoloader.push_str("// PHP Autoloader generated by RHP\n\n");
    php_autoloader.push_str("$vendorDir = __DIR__;\n\n");
    php_autoloader.push_str("// Load autoload files\n");
    php_autoloader.push_str("$files = require __DIR__ . '/rhp/autoload_files.rhp';\n");
    php_autoloader.push_str("foreach ($files as $file) {\n");
    php_autoloader.push_str("    if (file_exists($file)) {\n");
    php_autoloader.push_str("        require_once $file;\n");
    php_autoloader.push_str("    }\n");
    php_autoloader.push_str("}\n\n");
    php_autoloader.push_str("// Register PSR-4 Autoloader\n");
    php_autoloader.push_str("spl_autoload_register(function ($class) {\n");
    php_autoloader.push_str("    $vendorDir = __DIR__;\n");
    php_autoloader.push_str("    $psr4 = require __DIR__ . '/rhp/autoload_psr4.rhp';\n");
    php_autoloader.push_str("    foreach ($psr4 as $prefix => $dirs) {\n");
    php_autoloader.push_str("        $len = strlen($prefix);\n");
    php_autoloader.push_str("        if (strncmp($prefix, $class, $len) !== 0) {\n");
    php_autoloader.push_str("            continue;\n");
    php_autoloader.push_str("        }\n");
    php_autoloader.push_str("        $relativeClass = substr($class, $len);\n");
    php_autoloader.push_str("        foreach ($dirs as $dir) {\n");
    php_autoloader.push_str("            $file = $dir . '/' . str_replace('\\\\', '/', $relativeClass) . '.rhp';\n");
    php_autoloader.push_str("            if (file_exists($file)) {\n");
    php_autoloader.push_str("                require_once $file;\n");
    php_autoloader.push_str("                return true;\n");
    php_autoloader.push_str("            }\n");
    php_autoloader.push_str("            $phpFile = $dir . '/' . str_replace('\\\\', '/', $relativeClass) . '.php';\n");
    php_autoloader.push_str("            if (file_exists($phpFile)) {\n");
    php_autoloader.push_str("                require_once $phpFile;\n");
    php_autoloader.push_str("                return true;\n");
    php_autoloader.push_str("            }\n");
    php_autoloader.push_str("        }\n");
    php_autoloader.push_str("    }\n");
    php_autoloader.push_str("    return false;\n");
    php_autoloader.push_str("});\n");

    let autoload_php_path = vendor_dir.join("autoload.php");
    fs::write(autoload_php_path, php_autoloader)?;

    Ok(autoload_rhp_path)
}

/// Recursively scan `.rhp` and `.php` files in `dir`, finding declared classes/functions,
/// and collecting code chunks for the generated autoloader.
fn scan_and_collect_package_code(
    dir: &Path,
    rel_base: &str,
    class_map: &mut BTreeMap<String, String>,
    code_chunks: &mut Vec<String>,
) {
    if !dir.is_dir() {
        return;
    }

    let mut files = Vec::new();
    collect_source_files(dir, &mut files);
    files.sort();

    for file_path in files {
        if let Ok(content) = fs::read_to_string(&file_path) {
            let rel = if let Ok(stripped) = file_path.strip_prefix(dir) {
                format!("{}/{}", rel_base.trim_end_matches('/'), stripped.to_string_lossy())
            } else {
                file_path.to_string_lossy().to_string()
            };

            // Detect namespace
            let current_ns = extract_namespace(&content);

            // Detect classes, interfaces, traits, enums
            let classes = extract_classes(&content);
            for cls in classes {
                let fqcn = if let Some(ref ns) = current_ns {
                    format!("{}\\{}", ns, cls)
                } else {
                    cls
                };
                class_map.insert(fqcn, rel.clone());
            }

            // Clean code chunk (strip <?php if present, as RHP doesn't use opening tags)
            let cleaned = strip_php_tags(&content);
            if !cleaned.trim().is_empty() {
                code_chunks.push(cleaned);
            }
        }
    }
}

fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "tests" || name == "vendor" {
                continue;
            }
            if path.is_dir() {
                collect_source_files(&path, out);
            } else if path.is_file() {
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    if ext == "rhp" || ext == "php" {
                        out.push(path);
                    }
                }
            }
        }
    }
}

fn extract_namespace(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("namespace ") {
            let rest = trimmed.trim_start_matches("namespace ").trim_start();
            let ns = rest.trim_end_matches(';').trim_end_matches('{').trim();
            if !ns.is_empty() {
                return Some(ns.to_string());
            }
        }
    }
    None
}

fn extract_classes(content: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        for i in 0..tokens.len() {
            let t = tokens[i];
            if t == "class" || t == "interface" || t == "trait" || t == "enum" {
                if let Some(next) = tokens.get(i + 1) {
                    let clean = next.trim_end_matches('{').trim_end_matches(';').trim();
                    if !clean.is_empty() && clean.chars().next().map_or(false, |c| c.is_alphabetic() || c == '_') {
                        names.push(clean.to_string());
                    }
                }
            }
        }
    }
    names
}

fn strip_php_tags(content: &str) -> String {
    let mut s = content.trim();
    if s.starts_with("<?php") {
        s = s.trim_start_matches("<?php").trim_start();
    } else if s.starts_with("<?") {
        s = s.trim_start_matches("<?").trim_start();
    }
    if s.ends_with("?>") {
        s = s.trim_end_matches("?>").trim_end();
    }
    s.to_string()
}
