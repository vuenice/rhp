use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::error::{PkgError, Result};

pub const MANIFEST_FILENAME: &str = "rhp.toml";
pub const ALT_MANIFEST_FILENAME: &str = "rpm.toml";

/// Root configuration of an RHP project (`rhp.toml` or `rpm.toml`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub package: PackageMetadata,
    #[serde(default)]
    pub capabilities: Option<CapabilitiesConfig>,
    #[serde(default)]
    pub native: Option<NativeConfig>,
    #[serde(default)]
    pub exports: Option<ExportsConfig>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(default, rename = "dev-dependencies")]
    pub dev_dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    pub autoload: AutoloadConfig,
    #[serde(default)]
    pub export: Option<AutoloadConfig>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PackageType {
    #[default]
    Rhp,
    NativeRust,
    Wasm,
    Hybrid,
}

impl PackageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PackageType::Rhp => "rhp",
            PackageType::NativeRust => "native-rust",
            PackageType::Wasm => "wasm",
            PackageType::Hybrid => "hybrid",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "rhp" | "pure" | "pure-rhp" => Some(PackageType::Rhp),
            "native-rust" | "native" | "rust" => Some(PackageType::NativeRust),
            "wasm" | "webassembly" => Some(PackageType::Wasm),
            "hybrid" => Some(PackageType::Hybrid),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PackageSafety {
    #[default]
    Safe,
    Unsafe,
}

impl PackageSafety {
    pub fn as_str(&self) -> &'static str {
        match self {
            PackageSafety::Safe => "safe",
            PackageSafety::Unsafe => "unsafe",
        }
    }

    pub fn is_safe(&self) -> bool {
        matches!(self, PackageSafety::Safe)
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "safe" => Some(PackageSafety::Safe),
            "unsafe" => Some(PackageSafety::Unsafe),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CapabilitiesConfig {
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub filesystem: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct NativeConfig {
    #[serde(default, rename = "crate-name")]
    pub crate_name: Option<String>,
    #[serde(default, rename = "target-dir")]
    pub target_dir: Option<String>,
    #[serde(default, rename = "abi-version")]
    pub abi_version: Option<u32>,
    #[serde(default, rename = "entry-symbol")]
    pub entry_symbol: Option<String>,
    #[serde(default, rename = "thread-safe")]
    pub thread_safe: Option<bool>,
    #[serde(default, rename = "execution-target")]
    pub execution_target: Option<String>, // "native" | "wasm"
    #[serde(default)]
    pub marshalling: Option<String>, // "typed-c-abi" | "msgpack" | "cbor"
    #[serde(default)]
    pub facade: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ExportsConfig {
    #[serde(default)]
    pub facades: Vec<String>,
    #[serde(default)]
    pub namespaces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageMetadata {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub edition: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default, rename = "type")]
    pub package_type: Option<PackageType>,
    #[serde(default)]
    pub safety: Option<PackageSafety>,
    #[serde(default, rename = "rhp-version")]
    pub rhp_version: Option<String>,
}

impl PackageMetadata {
    pub fn is_safe(&self) -> bool {
        self.safety.unwrap_or(PackageSafety::Safe).is_safe()
    }

    pub fn package_type(&self) -> PackageType {
        self.package_type.unwrap_or(PackageType::Rhp)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Dependency {
    /// Shorthand string: version constraint, branch name, or tag.
    /// E.g.: `"vuenice/rhp-router" = "main"` or `"^1.0"`
    Simple(String),
    /// Detailed table configuration with explicit git/branch/tag/rev/path options.
    /// E.g.: `"vuenice/rhp-router" = { git = "https://github.com/vuenice/rhp-router.git", branch = "main" }`
    Detailed(DetailedDependency),
}

impl Dependency {
    pub fn git_url(&self, package_name: &str) -> String {
        match self {
            Dependency::Simple(_) => {
                if package_name.starts_with("http://")
                    || package_name.starts_with("https://")
                    || package_name.starts_with("git@")
                {
                    package_name.to_string()
                } else {
                    format!("https://github.com/{}.git", package_name)
                }
            }
            Dependency::Detailed(d) => {
                if let Some(ref g) = d.git {
                    g.clone()
                } else if package_name.starts_with("http://")
                    || package_name.starts_with("https://")
                    || package_name.starts_with("git@")
                {
                    package_name.to_string()
                } else {
                    format!("https://github.com/{}.git", package_name)
                }
            }
        }
    }

    pub fn branch(&self) -> Option<&str> {
        match self {
            Dependency::Simple(s) => {
                // If it looks like a branch name rather than semver (e.g. main, master, dev)
                if s == "main" || s == "master" || s == "dev" || s == "develop" {
                    Some(s.as_str())
                } else {
                    None
                }
            }
            Dependency::Detailed(d) => d.branch.as_deref(),
        }
    }

    pub fn tag(&self) -> Option<&str> {
        match self {
            Dependency::Simple(s) => {
                if s.starts_with('v') && s.chars().nth(1).map_or(false, |c| c.is_ascii_digit()) {
                    Some(s.as_str())
                } else {
                    None
                }
            }
            Dependency::Detailed(d) => d.tag.as_deref(),
        }
    }

    pub fn rev(&self) -> Option<&str> {
        match self {
            Dependency::Simple(_) => None,
            Dependency::Detailed(d) => d.rev.as_deref(),
        }
    }

    pub fn version_req(&self) -> Option<&str> {
        match self {
            Dependency::Simple(s) => Some(s.as_str()),
            Dependency::Detailed(d) => d.version.as_deref(),
        }
    }

    pub fn local_path(&self) -> Option<&str> {
        match self {
            Dependency::Simple(_) => None,
            Dependency::Detailed(d) => d.path.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DetailedDependency {
    #[serde(default)]
    pub git: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub rev: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AutoloadConfig {
    #[serde(default, rename = "psr-4")]
    pub psr4: BTreeMap<String, String>,
    #[serde(default, rename = "psr-0")]
    pub psr0: BTreeMap<String, String>,
    #[serde(default)]
    pub classmap: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

impl Manifest {
    /// Create a new default manifest with the given package name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            package: PackageMetadata {
                name: name.into(),
                version: "0.1.0".to_string(),
                edition: Some("2024".to_string()),
                description: Some("An RHP project".to_string()),
                authors: vec![],
                license: Some("MIT".to_string()),
                repository: None,
                package_type: Some(PackageType::Rhp),
                safety: Some(PackageSafety::Safe),
                rhp_version: Some(">=0.8.0".to_string()),
            },
            capabilities: None,
            native: None,
            exports: None,
            dependencies: BTreeMap::new(),
            dev_dependencies: BTreeMap::new(),
            autoload: AutoloadConfig {
                psr4: BTreeMap::new(),
                psr0: BTreeMap::new(),
                classmap: vec![],
                files: vec![],
            },
            export: None,
        }
    }

    /// Search for `rhp.toml` or `rpm.toml` in `dir`.
    pub fn find_in_dir(dir: &Path) -> Option<PathBuf> {
        let rhp_path = dir.join(MANIFEST_FILENAME);
        if rhp_path.exists() {
            return Some(rhp_path);
        }
        let alt_path = dir.join(ALT_MANIFEST_FILENAME);
        if alt_path.exists() {
            return Some(alt_path);
        }
        None
    }

    /// Load and parse manifest from file path.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(PkgError::ManifestNotFound(path.to_path_buf()));
        }
        let content = fs::read_to_string(path)?;
        let manifest: Manifest = toml::from_str(&content).map_err(|e| PkgError::ManifestParseError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        Ok(manifest)
    }

    /// Serialize and write manifest to file path.
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).map_err(|e| PkgError::ManifestWriteError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Add or update a dependency.
    pub fn add_dependency(&mut self, name: &str, dep: Dependency) {
        self.dependencies.insert(name.to_string(), dep);
    }

    /// Remove a dependency by name. Returns true if it existed.
    pub fn remove_dependency(&mut self, name: &str) -> bool {
        let removed_main = self.dependencies.remove(name).is_some();
        let removed_dev = self.dev_dependencies.remove(name).is_some();
        removed_main || removed_dev
    }

    /// Effective autoload config, merging `[export]` into `[autoload]` if both are present.
    pub fn effective_autoload(&self) -> AutoloadConfig {
        let mut config = self.autoload.clone();
        if let Some(ref exp) = self.export {
            for (k, v) in &exp.psr4 {
                config.psr4.insert(k.clone(), v.clone());
            }
            for (k, v) in &exp.psr0 {
                config.psr0.insert(k.clone(), v.clone());
            }
            for c in &exp.classmap {
                if !config.classmap.contains(c) {
                    config.classmap.push(c.clone());
                }
            }
            for f in &exp.files {
                if !config.files.contains(f) {
                    config.files.push(f.clone());
                }
            }
        }
        config
    }
}
