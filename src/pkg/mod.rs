//! # RHP Package Management
//!
//! Manages RHP and PHP packages directly from GitHub, Git repositories, and local paths,
//! utilizing TOML manifests (`rhp.toml`) and lockfiles (`rhp.lock`) inspired by Cargo,
//! while enforcing PHP-standard export and autoloading rules (PSR-4, classmaps, and files).

pub mod cli;
pub mod error;
pub mod exporter;
pub mod github;
pub mod lock;
pub mod manifest;
pub mod resolver;

pub use cli::{handle_add, handle_init, handle_install, handle_list, handle_remove, handle_update};
pub use error::{PkgError, Result, RpmError};
pub use exporter::{discover_package_exports, generate_autoloader, InstalledPackageMeta};
pub use github::{
    clone_or_fetch_repo, compute_directory_checksum, remove_package_dir, GitHubPackageSpec,
    GitRepoInfo, PackageSourceSpec,
};
pub use lock::{LockedPackage, Lockfile, ALT_LOCK_FILENAME, LOCK_FILENAME};
pub use manifest::{
    AutoloadConfig, Dependency, DetailedDependency, Manifest, PackageMetadata,
    ALT_MANIFEST_FILENAME, MANIFEST_FILENAME,
};
pub use resolver::{
    add_package, init_project, install, install_all, install_package, remove_package,
    uninstall_package, update, update_all, AddOptions, InstallOptions, PackageOperationResult,
    PackageOptions,
};
