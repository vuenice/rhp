---
title: "Package Manager"
description: "Unified package manager, Git and path dependencies, lockfile mechanics, and PSR-4 autoloader generation."
section: "Tooling"
order: 1
---

## Overview

RHP includes a high-performance package manager built directly into the unified `rhp` binary, following modern packaging conventions from Cargo, Go, and Bun.

Dependencies are defined in `rhp.toml` with deterministic resolutions recorded in `rhp.lock`. Installed packages follow standard PHP PSR-4 autoloader conventions, creating zero-friction interop for existing PHP and Laravel libraries.

For full CLI options and flag syntax, see the [CLI Reference](/docs/getting-started/cli-reference/#package-management).

---

## Core Commands

| Command | Description |
| :--- | :--- |
| `rhp init [NAME]` | Initialize a new `rhp.toml` manifest in the current directory |
| `rhp add <PACKAGE>` | Add a dependency from GitHub (`owner/repo`), Git URL, or local path |
| `rhp remove <PACKAGE>` | Remove a dependency from manifest, lockfile, and `vendor/` |
| `rhp install` | Install all dependencies strictly from `rhp.lock` (for CI / fresh clones) |
| `rhp update [PACKAGE]` | Update dependencies within version constraints and refresh `rhp.lock` |
| `rhp list` | List installed dependencies and dev-dependencies |

---

## Managing Dependencies

### Initializing a Project

Run `rhp init` to generate a starter `rhp.toml`:

```bash
rhp init my-rhp-app
```

```toml
[package]
name = "my-rhp-app"
version = "0.1.0"
authors = []

[dependencies]

[dev-dependencies]
```

### Adding Packages

You can add packages using GitHub shorthand, Git URLs, or local filesystem paths:

```bash
# GitHub shorthand (owner/repo)
rhp add symfony/var-dumper

# Specific Git release tag or branch
rhp add spatie/ray --tag v1.40.0
rhp add laravel/framework --branch 11.x

# Exact commit hash
rhp add owner/repo --rev a1b2c3d4e5f6

# Custom Git repository
rhp add --git https://github.com/vuenice/rhp-helpers.git

# Local path dependency
rhp add ./packages/local-sdk
rhp add my-library --path ../shared-lib

# Development dependency
rhp add phpunit/phpunit --dev
```

When added, RHP records the dependency in `rhp.toml`:

```toml
[dependencies]
"symfony/var-dumper" = { git = "https://github.com/symfony/var-dumper.git", rev = "v7.1.0" }
"local-sdk" = { path = "./packages/local-sdk" }

[dev-dependencies]
"phpunit/phpunit" = { git = "https://github.com/sebastianbergmann/phpunit.git", rev = "11.0.0" }
```

### Installing from Lockfile (CI / Production)

For reproducible builds in CI and production environments, use `rhp install`:

```bash
# Install locked dependencies into vendor/
rhp install

# Strict CI install (fails if lockfile is modified)
rhp install --frozen
```

> **Design Choice:** Like Cargo and Bun, `rhp install` exclusively installs dependencies from `rhp.lock`. To add new packages, use `rhp add <package>`.

---

## PSR-4 Autoloading Integration

Whenever dependencies are installed, added, or removed, RHP generates standard PHP autoload files in `vendor/`:
- `vendor/autoload.rhp`: Main entry point class autoloader.
- `vendor/rhp/installed.json`: Package metadata and installation manifests.
- `vendor/rhp/autoload_psr4.rhp`: PSR-4 namespace mapping table.
- `vendor/rhp/autoload_files.rhp`: Static helper files autoloader.

To consume installed packages in any RHP application or script:

```php
<?php
require_once __DIR__ . '/vendor/autoload.rhp';

use Symfony\Component\VarDumper\VarDumper;

VarDumper::dump(['status' => 'success', 'engine' => 'rhp']);
```

---

## Linking Native Rust Crates (Tier 2 Plugins)

In addition to PHP/RHP packages, developers can write high-performance custom extensions in Rust and link them at build time:

```toml
# In Cargo.toml
[dependencies]
rhp-plugin-sdk = "0.1"
image = "0.25"
```

```rust
use rhp_plugin_sdk::prelude::*;

#[rhp_macro("Image::blur")]
pub fn native_blur(args: &[PhpValue]) -> Result<PhpValue, RuntimeError> {
    // Zero-copy Rust image processing
    Ok(PhpValue::Bool(true))
}
```
