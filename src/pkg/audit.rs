//! # Package Safety & Security Audit Engine
//!
//! Validates package manifests and source trees against declared safety levels (`safety = "safe" | "unsafe"`):
//! 1. Packages declared as `safety = "safe"` are forbidden from performing outbound network I/O,
//!    raw socket access, or untrusted native FFI.
//! 2. Unverified native packages running without Wasm sandboxing are flagged with warnings.
//! 3. Audits all installed vendor dependencies and reports high-risk network capabilities.

use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use super::error::{PkgError, Result};
use super::manifest::{Manifest, PackageSafety, PackageType};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecurityViolation {
    pub file: PathBuf,
    pub line: usize,
    pub pattern: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecurityWarning {
    pub package: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyAudit {
    pub name: String,
    pub version: String,
    pub package_type: PackageType,
    pub safety: PackageSafety,
    pub network_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditReport {
    pub package_name: String,
    pub safety: PackageSafety,
    pub package_type: PackageType,
    pub violations: Vec<SecurityViolation>,
    pub warnings: Vec<SecurityWarning>,
    pub dependencies: Vec<DependencyAudit>,
}

impl AuditReport {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Known network egress patterns forbidden in `safety = "safe"` packages.
const FORBIDDEN_NETWORK_PATTERNS: &[(&str, &str)] = &[
    ("Http::", "Direct invocation of the HTTP facade"),
    ("HTTP::", "Direct invocation of the HTTP facade"),
    ("curl_init", "Native cURL handle initialization"),
    ("curl_exec", "Native cURL execution"),
    ("fsockopen", "Raw network socket creation"),
    ("pfsockopen", "Persistent raw socket creation"),
    ("stream_socket_client", "Stream socket client creation"),
    ("file_get_contents(\"http://", "Remote URL file fetch over HTTP"),
    ("file_get_contents('http://", "Remote URL file fetch over HTTP"),
    ("file_get_contents(\"https://", "Remote URL file fetch over HTTPS"),
    ("file_get_contents('https://", "Remote URL file fetch over HTTPS"),
    ("fopen(\"http://", "Remote stream opening over HTTP"),
    ("fopen('http://", "Remote stream opening over HTTP"),
    ("fopen(\"https://", "Remote stream opening over HTTPS"),
    ("fopen('https://", "Remote stream opening over HTTPS"),
    ("Socket::", "Direct invocation of raw Socket facade"),
];

/// Audit a package located at `root_dir`.
pub fn audit_package(root_dir: &Path) -> Result<AuditReport> {
    let manifest_path = Manifest::find_in_dir(root_dir).ok_or_else(|| {
        PkgError::ManifestNotFound(root_dir.join("rhp.toml"))
    })?;

    let manifest = Manifest::load_from_file(&manifest_path)?;
    let safety = manifest.package.safety.unwrap_or(PackageSafety::Safe);
    let pkg_type = manifest.package.package_type.unwrap_or(PackageType::Rhp);

    let mut violations = Vec::new();
    let mut warnings = Vec::new();

    // 1. If package is declared "safe", scan source tree for illegal network calls
    if safety.is_safe() {
        let mut source_files = Vec::new();
        collect_source_files(root_dir, &mut source_files);

        for file in source_files {
            if let Ok(content) = fs::read_to_string(&file) {
                for (line_idx, line) in content.lines().enumerate() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with('*') {
                        continue;
                    }

                    for (pattern, reason) in FORBIDDEN_NETWORK_PATTERNS {
                        if line.contains(pattern) {
                            violations.push(SecurityViolation {
                                file: file.clone(),
                                line: line_idx + 1,
                                pattern: pattern.to_string(),
                                reason: reason.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    // 2. Warn if native execution is selected without official verification
    if pkg_type == PackageType::NativeRust {
        if let Some(ref native) = manifest.native {
            if native.execution_target.as_deref() == Some("native") {
                let name = &manifest.package.name;
                let is_verified_namespace = name.starts_with("rhp/") || name.starts_with("@rhp/") || name.starts_with("vuenice/");
                if !is_verified_namespace {
                    warnings.push(SecurityWarning {
                        package: name.clone(),
                        message: "Package uses native dynamic loading ('execution-target = native') without verified vendor namespace. Recommend 'execution-target = wasm' for sandboxing.".to_string(),
                    });
                }
            }
        }
    }

    // 3. Audit dependencies in vendor/ directory
    let mut dependencies = Vec::new();
    let vendor_dir = root_dir.join("vendor");
    if vendor_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&vendor_dir) {
            for entry in entries.flatten() {
                let vendor_sub = entry.path();
                if vendor_sub.is_dir() {
                    // Check sub-packages (vendor/author/pkg)
                    if let Ok(sub_entries) = fs::read_dir(&vendor_sub) {
                        for sub_entry in sub_entries.flatten() {
                            let dep_path = sub_entry.path();
                            if dep_path.is_dir() {
                                if let Some(dep_manifest_path) = Manifest::find_in_dir(&dep_path) {
                                    if let Ok(dep_m) = Manifest::load_from_file(&dep_manifest_path) {
                                        let dep_safety = dep_m.package.safety.unwrap_or(PackageSafety::Safe);
                                        let dep_type = dep_m.package.package_type.unwrap_or(PackageType::Rhp);
                                        let net_caps = dep_m.capabilities
                                            .map(|c| c.network)
                                            .unwrap_or_default();

                                        if !dep_safety.is_safe() {
                                            warnings.push(SecurityWarning {
                                                package: dep_m.package.name.clone(),
                                                message: format!("Dependency is marked 'unsafe' and possesses network egress capabilities ({:?}).", net_caps),
                                            });
                                        }

                                        dependencies.push(DependencyAudit {
                                            name: dep_m.package.name,
                                            version: dep_m.package.version,
                                            package_type: dep_type,
                                            safety: dep_safety,
                                            network_capabilities: net_caps,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(AuditReport {
        package_name: manifest.package.name,
        safety,
        package_type: pkg_type,
        violations,
        warnings,
        dependencies,
    })
}

/// Recursively find all `.rhp` and `.php` files, skipping `vendor/`, `target/`, and `.git/`.
fn collect_source_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name != "vendor" && name != "target" && name != ".git" && name != "node_modules" {
                collect_source_files(&path, files);
            }
        } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            if ext == "rhp" || ext == "php" {
                files.push(path);
            }
        }
    }
}

/// Format an audit report for terminal output.
pub fn format_audit_report(report: &AuditReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("\n🛡️  RHP Security & Safety Audit for '{}'\n", report.package_name));
    out.push_str(&format!("   Package Type: {}\n", report.package_type.as_str()));
    out.push_str(&format!("   Safety Class: {}\n", report.safety.as_str()));
    out.push_str("────────────────────────────────────────────────────────────\n");

    if report.violations.is_empty() {
        out.push_str("✅ No safety violations found in package source code.\n");
    } else {
        out.push_str(&format!("🚨 Found {} SECURITY VIOLATIONS (Package marked 'safe' but executes network calls):\n", report.violations.len()));
        for v in &report.violations {
            out.push_str(&format!(
                "   ❌ {}:{}: Illegal call to '{}' ({})\n",
                v.file.display(),
                v.line,
                v.pattern,
                v.reason
            ));
        }
    }

    if !report.warnings.is_empty() {
        out.push_str(&format!("\n⚠️  Warnings ({}):\n", report.warnings.len()));
        for w in &report.warnings {
            out.push_str(&format!("   - [{}]: {}\n", w.package, w.message));
        }
    }

    if !report.dependencies.is_empty() {
        out.push_str(&format!("\n📦 Audited Dependencies ({}):\n", report.dependencies.len()));
        for d in &report.dependencies {
            let icon = if d.safety.is_safe() { "🟢" } else { "🟠" };
            out.push_str(&format!(
                "   {} {} v{} [{}] (safety: {})\n",
                icon, d.name, d.version, d.package_type.as_str(), d.safety.as_str()
            ));
        }
    }

    out.push_str("\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_audit_safe_package_catches_http_call() {
        let temp_dir = std::env::temp_dir().join(format!("rhp_audit_test_{}", std::time::SystemTime::now().elapsed().unwrap().as_nanos()));
        fs::create_dir_all(temp_dir.join("src")).unwrap();

        let rhp_toml = r#"
[package]
name = "test/math-utils"
version = "1.0.0"
type = "rhp"
safety = "safe"
"#;
        fs::write(temp_dir.join("rhp.toml"), rhp_toml).unwrap();

        // Write a file that violates safety by making an Http::get call
        let code = r#"<?php
namespace Test;
class Calculator {
    public function add($a, $b) {
        // Illegal network call in safe package!
        Http::post("https://telemetry.example.com", ["data" => $a]);
        return $a + $b;
    }
}
"#;
        fs::write(temp_dir.join("src/Calculator.rhp"), code).unwrap();

        let report = audit_package(&temp_dir).expect("Audit should complete");
        assert!(!report.is_clean());
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].pattern, "Http::");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
