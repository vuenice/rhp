use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::error::{Result, RpmError};

pub const MANIFEST_FILENAME: &str = "rpm.toml";
pub const ALT_MANIFEST_FILENAME: &str = "rhp.toml";

/// Root configuration of an RHP project (`rpm.toml` or `rhp.toml`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub package: PackageMetadata,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(default, rename = "dev-dependencies")]
    pub dev_dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    pub autoload: AutoloadConfig,
    #[serde(default)]
    pub export: Option<AutoloadConfig>,
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
                description: Some("An RHP project managed with RPM".to_string()),
                authors: vec![],
                license: Some("MIT".to_string()),
                repository: None,
            },
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

    /// Search for `rpm.toml` or `rhp.toml` in `dir`.
    pub fn find_in_dir(dir: &Path) -> Option<PathBuf> {
        let rpm_path = dir.join(MANIFEST_FILENAME);
        if rpm_path.exists() {
            return Some(rpm_path);
        }
        let rhp_path = dir.join(ALT_MANIFEST_FILENAME);
        if rhp_path.exists() {
            return Some(rhp_path);
        }
        None
    }

    /// Load and parse manifest from file path.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(RpmError::ManifestNotFound(path.to_path_buf()));
        }
        let content = fs::read_to_string(path)?;
        let manifest: Manifest = toml::from_str(&content).map_err(|e| RpmError::ManifestParseError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        Ok(manifest)
    }

    /// Serialize and write manifest to file path.
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).map_err(|e| RpmError::ManifestWriteError {
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
