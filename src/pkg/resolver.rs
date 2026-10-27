use std::path::{Path, PathBuf};

use super::error::{PkgError, Result};
use super::exporter::{discover_package_exports, generate_autoloader, InstalledPackageMeta};
use super::github::{clone_or_fetch_repo, remove_package_dir, PackageSourceSpec};
use super::lock::{LockedPackage, Lockfile, ALT_LOCK_FILENAME, LOCK_FILENAME};
use super::manifest::{
    Dependency, DetailedDependency, Manifest, MANIFEST_FILENAME,
};

#[derive(Debug, Default, Clone)]
pub struct AddOptions {
    pub git: Option<String>,
    pub branch: Option<String>,
    pub tag: Option<String>,
    pub rev: Option<String>,
    pub path: Option<String>,
    pub dev: bool,
}

#[derive(Debug, Default, Clone)]
pub struct InstallOptions {
    pub frozen: bool,
}

pub type PackageOptions = AddOptions;

#[derive(Debug, Clone)]
pub struct PackageOperationResult {
    pub name: String,
    pub version: String,
    pub commit: String,
    pub path: PathBuf,
}

/// Find the lockfile path to use (`rhp.lock` or fallback `rpm.lock`).
fn resolve_lock_path(root_dir: &Path) -> PathBuf {
    let rhp_lock = root_dir.join(LOCK_FILENAME);
    if rhp_lock.exists() {
        return rhp_lock;
    }
    let alt_lock = root_dir.join(ALT_LOCK_FILENAME);
    if alt_lock.exists() {
        return alt_lock;
    }
    rhp_lock
}

/// Install all dependencies declared in the lockfile / manifest (CI and fresh clone case).
pub fn install(root_dir: &Path, options: &InstallOptions) -> Result<Vec<PackageOperationResult>> {
    let manifest_path = Manifest::find_in_dir(root_dir).ok_or_else(|| {
        PkgError::ManifestNotFound(root_dir.join(MANIFEST_FILENAME))
    })?;

    let manifest = Manifest::load_from_file(&manifest_path)?;
    let lock_path = resolve_lock_path(root_dir);
    let mut lockfile = Lockfile::load_from_file(&lock_path)?;

    let mut results = Vec::new();
    let mut installed_metas = Vec::new();

    for (pkg_name, dep) in &manifest.dependencies {
        let spec = PackageSourceSpec::parse(pkg_name).unwrap_or_else(|_| {
            let url = dep.git_url(pkg_name);
            PackageSourceSpec {
                raw_name: pkg_name.clone(),
                vendor: "vendor".to_string(),
                repo: pkg_name.clone(),
                source_url: url,
                branch: dep.branch().map(|s| s.to_string()),
                tag: dep.tag().map(|s| s.to_string()),
                rev: dep.rev().map(|s| s.to_string()),
                is_local_path: false,
            }
        });

        let dest_dir = spec.destination_path(root_dir);

        let locked_commit = lockfile.get_locked_commit(pkg_name);
        let rev_to_use = if options.frozen {
            locked_commit.as_deref().or_else(|| dep.rev())
        } else {
            dep.rev().or(locked_commit.as_deref())
        };

        let branch_to_use = dep.branch().or(spec.branch.as_deref());
        let tag_to_use = dep.tag().or(spec.tag.as_deref());

        let source_url = if let Some(local_p) = dep.local_path() {
            local_p.to_string()
        } else {
            dep.git_url(pkg_name)
        };

        let repo_info = clone_or_fetch_repo(
            &source_url,
            &dest_dir,
            branch_to_use,
            tag_to_use,
            rev_to_use,
        )?;

        let auto_config = discover_package_exports(&dest_dir, pkg_name);

        let locked_pkg = LockedPackage {
            name: pkg_name.clone(),
            version: dep.version_req().unwrap_or("0.1.0").to_string(),
            source: Some(format!("git+{}#{}", source_url, repo_info.commit_hash)),
            checksum: Some(repo_info.checksum.clone()),
            dependencies: vec![],
        };
        lockfile.upsert_package(locked_pkg);

        let rel_path = dest_dir
            .strip_prefix(root_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| dest_dir.to_string_lossy().to_string());

        installed_metas.push(InstalledPackageMeta {
            name: pkg_name.clone(),
            version: dep.version_req().unwrap_or("0.1.0").to_string(),
            source: Some(source_url),
            checksum: Some(repo_info.checksum),
            path: rel_path,
            autoload: auto_config,
        });

        results.push(PackageOperationResult {
            name: pkg_name.clone(),
            version: dep.version_req().unwrap_or("0.1.0").to_string(),
            commit: repo_info.commit_hash,
            path: dest_dir,
        });
    }

    lockfile.save_to_file(&lock_path)?;
    generate_autoloader(root_dir, &installed_metas)?;

    Ok(results)
}

/// Backwards compatibility alias for `install`.
pub fn install_all(root_dir: &Path, options: &InstallOptions) -> Result<Vec<PackageOperationResult>> {
    install(root_dir, options)
}

/// Add a dependency to the manifest and lockfile (`rhp add <pkg>`).
pub fn add_package(
    root_dir: &Path,
    pkg_spec: &str,
    options: &AddOptions,
) -> Result<PackageOperationResult> {
    let manifest_path = Manifest::find_in_dir(root_dir).unwrap_or_else(|| {
        root_dir.join(MANIFEST_FILENAME)
    });

    let mut manifest = if manifest_path.exists() {
        Manifest::load_from_file(&manifest_path)?
    } else {
        let pkg_name = root_dir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "rhp-project".to_string());
        let m = Manifest::new(pkg_name);
        m.save_to_file(&manifest_path)?;
        m
    };

    let spec = PackageSourceSpec::parse(pkg_spec)?;
    let dest_dir = spec.destination_path(root_dir);

    let branch = options.branch.as_deref().or(spec.branch.as_deref());
    let tag = options.tag.as_deref().or(spec.tag.as_deref());
    let rev = options.rev.as_deref().or(spec.rev.as_deref());

    let source_url = if let Some(ref p) = options.path {
        p.as_str()
    } else if let Some(ref g) = options.git {
        g.as_str()
    } else {
        &spec.source_url
    };

    let repo_info = clone_or_fetch_repo(
        source_url,
        &dest_dir,
        branch,
        tag,
        rev,
    )?;

    let dep = if let Some(ref p) = options.path {
        Dependency::Detailed(DetailedDependency {
            path: Some(p.clone()),
            git: None,
            branch: None,
            tag: None,
            rev: None,
            version: None,
        })
    } else if spec.is_local_path {
        Dependency::Detailed(DetailedDependency {
            path: Some(source_url.to_string()),
            git: None,
            branch: None,
            tag: None,
            rev: None,
            version: None,
        })
    } else if branch.is_some() || tag.is_some() || rev.is_some() || options.git.is_some() {
        Dependency::Detailed(DetailedDependency {
            git: Some(source_url.to_string()),
            branch: branch.map(|s| s.to_string()),
            tag: tag.map(|s| s.to_string()),
            rev: rev.map(|s| s.to_string()),
            version: None,
            path: None,
        })
    } else {
        Dependency::Simple("main".to_string())
    };

    if options.dev {
        manifest.dev_dependencies.insert(spec.raw_name.clone(), dep);
    } else {
        manifest.dependencies.insert(spec.raw_name.clone(), dep);
    }

    manifest.save_to_file(&manifest_path)?;

    let lock_path = resolve_lock_path(root_dir);
    let mut lockfile = Lockfile::load_from_file(&lock_path)?;
    lockfile.upsert_package(LockedPackage {
        name: spec.raw_name.clone(),
        version: "0.1.0".to_string(),
        source: Some(format!("git+{}#{}", source_url, repo_info.commit_hash)),
        checksum: Some(repo_info.checksum),
        dependencies: vec![],
    });
    lockfile.save_to_file(&lock_path)?;

    refresh_autoloader(root_dir, &manifest, &lockfile)?;

    Ok(PackageOperationResult {
        name: spec.raw_name,
        version: "0.1.0".to_string(),
        commit: repo_info.commit_hash,
        path: dest_dir,
    })
}

/// Backwards compatibility alias for `add_package`.
pub fn install_package(
    root_dir: &Path,
    pkg_spec: &str,
    options: &AddOptions,
) -> Result<PackageOperationResult> {
    add_package(root_dir, pkg_spec, options)
}

/// Update dependencies within version constraints (`rhp update [pkg]`).
pub fn update(root_dir: &Path, target_pkg: Option<&str>) -> Result<Vec<PackageOperationResult>> {
    let manifest_path = Manifest::find_in_dir(root_dir).ok_or_else(|| {
        PkgError::ManifestNotFound(root_dir.join(MANIFEST_FILENAME))
    })?;

    let manifest = Manifest::load_from_file(&manifest_path)?;
    let lock_path = resolve_lock_path(root_dir);
    let mut lockfile = Lockfile::load_from_file(&lock_path)?;

    let mut results = Vec::new();
    let mut installed_metas = Vec::new();

    for (pkg_name, dep) in &manifest.dependencies {
        if let Some(target) = target_pkg {
            if pkg_name != target && !pkg_name.ends_with(&format!("/{}", target)) {
                continue;
            }
        }

        let spec = PackageSourceSpec::parse(pkg_name).unwrap_or_else(|_| {
            let url = dep.git_url(pkg_name);
            PackageSourceSpec {
                raw_name: pkg_name.clone(),
                vendor: "vendor".to_string(),
                repo: pkg_name.clone(),
                source_url: url,
                branch: dep.branch().map(|s| s.to_string()),
                tag: dep.tag().map(|s| s.to_string()),
                rev: dep.rev().map(|s| s.to_string()),
                is_local_path: false,
            }
        });

        let dest_dir = spec.destination_path(root_dir);
        let source_url = if let Some(local_p) = dep.local_path() {
            local_p.to_string()
        } else {
            dep.git_url(pkg_name)
        };

        let repo_info = clone_or_fetch_repo(
            &source_url,
            &dest_dir,
            dep.branch(),
            dep.tag(),
            dep.rev(),
        )?;

        let locked_pkg = LockedPackage {
            name: pkg_name.clone(),
            version: dep.version_req().unwrap_or("0.1.0").to_string(),
            source: Some(format!("git+{}#{}", source_url, repo_info.commit_hash)),
            checksum: Some(repo_info.checksum.clone()),
            dependencies: vec![],
        };
        lockfile.upsert_package(locked_pkg);

        let auto_config = discover_package_exports(&dest_dir, pkg_name);
        let rel_path = dest_dir
            .strip_prefix(root_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| dest_dir.to_string_lossy().to_string());

        installed_metas.push(InstalledPackageMeta {
            name: pkg_name.clone(),
            version: dep.version_req().unwrap_or("0.1.0").to_string(),
            source: Some(source_url),
            checksum: Some(repo_info.checksum),
            path: rel_path,
            autoload: auto_config,
        });

        results.push(PackageOperationResult {
            name: pkg_name.clone(),
            version: dep.version_req().unwrap_or("0.1.0").to_string(),
            commit: repo_info.commit_hash,
            path: dest_dir,
        });
    }

    lockfile.save_to_file(&lock_path)?;
    generate_autoloader(root_dir, &installed_metas)?;

    Ok(results)
}

/// Backwards compatibility alias for `update(root_dir, None)`.
pub fn update_all(root_dir: &Path) -> Result<Vec<PackageOperationResult>> {
    update(root_dir, None)
}

/// Remove a package: remove from manifest, lockfile, and `vendor/` (`rhp remove <pkg>`).
pub fn remove_package(root_dir: &Path, pkg_name: &str) -> Result<bool> {
    let manifest_path = Manifest::find_in_dir(root_dir).ok_or_else(|| {
        PkgError::ManifestNotFound(root_dir.join(MANIFEST_FILENAME))
    })?;

    let mut manifest = Manifest::load_from_file(&manifest_path)?;

    let target_key = if manifest.dependencies.contains_key(pkg_name) {
        Some(pkg_name.to_string())
    } else {
        manifest
            .dependencies
            .keys()
            .find(|k| *k == pkg_name || k.ends_with(&format!("/{}", pkg_name)))
            .cloned()
    };

    let actual_name = match target_key {
        Some(k) => k,
        None => return Err(PkgError::PackageNotFound(pkg_name.to_string())),
    };

    manifest.remove_dependency(&actual_name);
    manifest.save_to_file(&manifest_path)?;

    let lock_path = resolve_lock_path(root_dir);
    let mut lockfile = Lockfile::load_from_file(&lock_path)?;
    lockfile.remove_package(&actual_name);
    lockfile.save_to_file(&lock_path)?;

    if let Ok(spec) = PackageSourceSpec::parse(&actual_name) {
        let dest = spec.destination_path(root_dir);
        let _ = remove_package_dir(&dest);
    } else {
        let dest = root_dir.join("vendor").join(&actual_name);
        let _ = remove_package_dir(&dest);
    }

    refresh_autoloader(root_dir, &manifest, &lockfile)?;

    Ok(true)
}

/// Backwards compatibility alias for `remove_package`.
pub fn uninstall_package(root_dir: &Path, pkg_name: &str) -> Result<bool> {
    remove_package(root_dir, pkg_name)
}

/// Initialize a new `rhp.toml` manifest in `root_dir` (`rhp init`).
pub fn init_project(root_dir: &Path, custom_name: Option<&str>) -> Result<PathBuf> {
    let manifest_path = root_dir.join(MANIFEST_FILENAME);
    if manifest_path.exists() {
        return Err(PkgError::General(format!(
            "Manifest '{}' already exists.",
            manifest_path.display()
        )));
    }

    let name = custom_name
        .map(|s| s.to_string())
        .or_else(|| {
            root_dir
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "rhp-project".to_string());

    let manifest = Manifest::new(name);
    manifest.save_to_file(&manifest_path)?;
    Ok(manifest_path)
}

/// Refresh the autoloader based on all currently configured packages.
fn refresh_autoloader(root_dir: &Path, manifest: &Manifest, lockfile: &Lockfile) -> Result<()> {
    let mut installed_metas = Vec::new();
    for pkg_name in manifest.dependencies.keys() {
        let spec = PackageSourceSpec::parse(pkg_name).ok();
        let dest_dir = if let Some(ref s) = spec {
            s.destination_path(root_dir)
        } else {
            root_dir.join("vendor").join(pkg_name)
        };

        if dest_dir.exists() {
            let auto = discover_package_exports(&dest_dir, pkg_name);
            let locked = lockfile.find_package(pkg_name);
            let version = locked.map(|p| p.version.clone()).unwrap_or_else(|| "0.1.0".to_string());
            let checksum = locked.and_then(|p| p.checksum.clone());
            let source = locked.and_then(|p| p.source.clone());

            let rel_path = dest_dir
                .strip_prefix(root_dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| dest_dir.to_string_lossy().to_string());

            installed_metas.push(InstalledPackageMeta {
                name: pkg_name.clone(),
                version,
                source,
                checksum,
                path: rel_path,
                autoload: auto,
            });
        }
    }
    generate_autoloader(root_dir, &installed_metas)?;
    Ok(())
}
