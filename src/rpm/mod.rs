//! # RPM: High-Performance Package Manager for RHP
//!
//! RPM manages RHP and PHP packages directly from GitHub or git repositories,
//! utilizing TOML manifests (`rpm.toml`) and lockfiles (`rpm.lock`) inspired by Cargo,
//! while enforcing PHP-standard export and autoloading rules (PSR-4, classmaps, and files).

pub mod cli;
pub mod error;
pub mod exporter;
pub mod github;
pub mod lock;
pub mod manifest;
pub mod resolver;

pub use cli::{print_rpm_help, run_rpm_cli};
pub use error::{Result, RpmError};
pub use exporter::{discover_package_exports, generate_autoloader, InstalledPackageMeta};
pub use github::{clone_or_fetch_repo, compute_directory_checksum, remove_package_dir, GitHubPackageSpec, GitRepoInfo};
pub use lock::{LockedPackage, Lockfile, LOCK_FILENAME};
pub use manifest::{
    AutoloadConfig, Dependency, DetailedDependency, Manifest, PackageMetadata,
    ALT_MANIFEST_FILENAME, MANIFEST_FILENAME,
};
pub use resolver::{
    init_project, install_all, install_package, uninstall_package, update_all, InstallOptions,
    PackageOperationResult,
};
