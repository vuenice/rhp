use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::error::{PkgError, Result};

/// Parsed package information from a Git URL, GitHub specifier, or local path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSourceSpec {
    pub raw_name: String,
    pub vendor: String,
    pub repo: String,
    pub source_url: String,
    pub branch: Option<String>,
    pub tag: Option<String>,
    pub rev: Option<String>,
    pub is_local_path: bool,
}

pub type GitHubPackageSpec = PackageSourceSpec;

impl PackageSourceSpec {
    /// Parse a package string like:
    /// - `"vuenice/rhp-router"`
    /// - `"vuenice/rhp-router@main"`
    /// - `"vuenice/rhp-router#v1.0.0"`
    /// - `"https://github.com/vuenice/rhp-router"`
    /// - `"https://github.com/vuenice/rhp-router.git"`
    /// - `"git@github.com:vuenice/rhp-router.git"`
    /// - `"./local-pkg"`, `"../shared-lib"`
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim().trim_matches(|c| c == '\'' || c == '"');
        if trimmed.is_empty() {
            return Err(PkgError::InvalidPackageSpec {
                spec: input.to_string(),
                reason: "Package specification cannot be empty".to_string(),
            });
        }

        let mut branch = None;
        let mut tag = None;
        let mut base = trimmed;

        if let Some((b, rest)) = base.split_once('@') {
            base = b;
            branch = Some(rest.to_string());
        } else if let Some((b, rest)) = base.split_once('#') {
            base = b;
            tag = Some(rest.to_string());
        }

        let (vendor, repo, source_url, is_local_path) = if base.starts_with("https://github.com/")
            || base.starts_with("http://github.com/")
        {
            let path_part = base
                .trim_start_matches("https://github.com/")
                .trim_start_matches("http://github.com/")
                .trim_end_matches(".git")
                .trim_end_matches('/');
            let parts: Vec<&str> = path_part.split('/').collect();
            if parts.len() < 2 {
                return Err(PkgError::InvalidPackageSpec {
                    spec: input.to_string(),
                    reason: "Expected 'https://github.com/<owner>/<repo>'".to_string(),
                });
            }
            (
                parts[0].to_string(),
                parts[1].to_string(),
                format!("https://github.com/{}/{}.git", parts[0], parts[1]),
                false,
            )
        } else if base.starts_with("git@github.com:") {
            let path_part = base
                .trim_start_matches("git@github.com:")
                .trim_end_matches(".git")
                .trim_end_matches('/');
            let parts: Vec<&str> = path_part.split('/').collect();
            if parts.len() < 2 {
                return Err(PkgError::InvalidPackageSpec {
                    spec: input.to_string(),
                    reason: "Expected 'git@github.com:<owner>/<repo>.git'".to_string(),
                });
            }
            (
                parts[0].to_string(),
                parts[1].to_string(),
                format!("https://github.com/{}/{}.git", parts[0], parts[1]),
                false,
            )
        } else if base.starts_with("file://")
            || base.starts_with('/')
            || base.starts_with("./")
            || base.starts_with("../")
            || Path::new(base).exists()
        {
            // Local file/git path
            let clean_path = base.trim_start_matches("file://");
            let p = Path::new(clean_path);
            let repo_name = p
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "local-pkg".to_string());
            ("local".to_string(), repo_name, clean_path.to_string(), true)
        } else {
            // Standard "owner/repo" or git URL
            let parts: Vec<&str> = base.split('/').collect();
            if parts.len() == 2 {
                (
                    parts[0].to_string(),
                    parts[1].to_string(),
                    format!("https://github.com/{}/{}.git", parts[0], parts[1]),
                    false,
                )
            } else if parts.len() == 1 {
                return Err(PkgError::InvalidPackageSpec {
                    spec: input.to_string(),
                    reason: "Package name must be in format 'owner/repo' (e.g. 'vuenice/rhp-router') or a git/path URL".to_string(),
                });
            } else {
                return Err(PkgError::InvalidPackageSpec {
                    spec: input.to_string(),
                    reason: format!("Unrecognized repository or package specifier '{}'", input),
                });
            }
        };

        let raw_name = format!("{}/{}", vendor, repo);

        Ok(Self {
            raw_name,
            vendor,
            repo,
            source_url,
            branch,
            tag,
            rev: None,
            is_local_path,
        })
    }

    /// Destination directory inside the `vendor/` tree.
    pub fn destination_path(&self, root_dir: &Path) -> PathBuf {
        root_dir.join("vendor").join(&self.vendor).join(&self.repo)
    }

    pub fn git_url(&self) -> &str {
        &self.source_url
    }
}

/// Metadata about a cloned repository.
#[derive(Debug, Clone)]
pub struct GitRepoInfo {
    pub commit_hash: String,
    pub checksum: String,
}

/// Clone, fetch, or copy a repository into `target_dir`.
pub fn clone_or_fetch_repo(
    source_url: &str,
    target_dir: &Path,
    branch: Option<&str>,
    tag: Option<&str>,
    rev: Option<&str>,
) -> Result<GitRepoInfo> {
    if let Some(parent) = target_dir.parent() {
        fs::create_dir_all(parent)?;
    }

    // Check if source is a local directory without git
    let local_path = Path::new(source_url.trim_start_matches("file://"));
    if local_path.is_dir() && !local_path.join(".git").exists() {
        if target_dir.exists() {
            let _ = fs::remove_dir_all(target_dir);
        }
        copy_dir_recursive(local_path, target_dir)?;
        let checksum = compute_directory_checksum(target_dir)?;
        return Ok(GitRepoInfo {
            commit_hash: "local".to_string(),
            checksum,
        });
    }

    let is_existing_git = target_dir.join(".git").exists();

    if is_existing_git {
        // Fetch updates
        let mut fetch_cmd = Command::new("git");
        fetch_cmd.current_dir(target_dir).arg("fetch");
        if let Some(b) = branch.or(tag) {
            fetch_cmd.args(["origin", b]);
        }
        let output = fetch_cmd.output().map_err(|e| {
            PkgError::Git(format!("Failed to execute 'git fetch' at {}: {}", target_dir.display(), e))
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PkgError::Git(format!("git fetch failed: {}", stderr)));
        }

        // Checkout target ref
        let target_ref = rev.or(branch).or(tag).unwrap_or("origin/HEAD");
        let mut checkout_cmd = Command::new("git");
        checkout_cmd.current_dir(target_dir).args(["checkout", target_ref]);
        let output = checkout_cmd.output().map_err(|e| {
            PkgError::Git(format!("Failed to execute 'git checkout' at {}: {}", target_dir.display(), e))
        })?;
        if !output.status.success() {
            // Try reset --hard in case of detached HEAD / origin
            let _ = Command::new("git")
                .current_dir(target_dir)
                .args(["reset", "--hard", target_ref])
                .output();
        }
    } else {
        // Clone fresh repository
        if target_dir.exists() {
            let _ = fs::remove_dir_all(target_dir);
        }

        let mut clone_cmd = Command::new("git");
        clone_cmd.arg("clone");

        if let Some(b) = branch.or(tag) {
            clone_cmd.args(["--branch", b]);
        }
        // Use shallow clone for fast downloads
        clone_cmd.args(["--depth", "1", source_url]);
        clone_cmd.arg(target_dir.to_string_lossy().as_ref());

        let output = clone_cmd.output().map_err(|e| {
            PkgError::Git(format!(
                "Failed to execute 'git clone' for '{}': {}",
                source_url, e
            ))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PkgError::Git(format!(
                "Failed to clone repository '{}': {}",
                source_url, stderr
            )));
        }

        // If specific revision was requested, checkout that revision
        if let Some(r) = rev {
            let output = Command::new("git")
                .current_dir(target_dir)
                .args(["checkout", r])
                .output()
                .map_err(|e| PkgError::Git(format!("Failed to checkout revision {}: {}", r, e)))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(PkgError::Git(format!(
                    "Failed to checkout revision '{}': {}",
                    r, stderr
                )));
            }
        }
    }

    let commit_hash = get_head_commit(target_dir)?;
    let checksum = compute_directory_checksum(target_dir)?;

    Ok(GitRepoInfo {
        commit_hash,
        checksum,
    })
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            if entry.file_name() != ".git" {
                copy_dir_recursive(&src_path, &dst_path)?;
            }
        } else if ty.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Retrieve the HEAD commit hash using `git rev-parse HEAD`.
pub fn get_head_commit(dir: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|e| {
            PkgError::Git(format!(
                "Failed to run 'git rev-parse HEAD' at {}: {}",
                dir.display(),
                e
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PkgError::Git(format!("git rev-parse HEAD failed: {}", stderr)));
    }

    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(commit)
}

/// Compute a deterministic SHA-256 checksum across all files in `dir`, skipping `.git`.
pub fn compute_directory_checksum(dir: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_files_recursive(dir, &mut files)?;
    files.sort();

    let mut hasher = Sha256::new();
    for file_path in files {
        if let Ok(rel) = file_path.strip_prefix(dir) {
            hasher.update(rel.to_string_lossy().as_bytes());
        }
        if let Ok(content) = fs::read(&file_path) {
            hasher.update(&content);
        }
    }

    let result = hasher.finalize();
    Ok(format!("sha256:{}", hex::encode(result)))
}

fn collect_files_recursive(dir: &Path, list: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if name_str == ".git" {
            continue;
        }
        if path.is_dir() {
            collect_files_recursive(&path, list)?;
        } else if path.is_file() {
            list.push(path);
        }
    }
    Ok(())
}

/// Uninstall/delete a package directory from `vendor/`.
pub fn remove_package_dir(target_dir: &Path) -> Result<bool> {
    if target_dir.exists() {
        fs::remove_dir_all(target_dir)?;
        // Clean up empty parent vendor/<vendor> directory if empty
        if let Some(parent) = target_dir.parent() {
            if let Ok(mut read) = fs::read_dir(parent) {
                if read.next().is_none() {
                    let _ = fs::remove_dir(parent);
                }
            }
        }
        Ok(true)
    } else {
        Ok(false)
    }
}
