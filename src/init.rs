//! Workspace scaffolding (`granit init`).

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::workspace::{PACKAGE_FILE, WORKSPACE_FILE};

/// Default nixpkgs ref used in a freshly scaffolded workspace.
const DEFAULT_NIXPKGS_REF: &str = "github:NixOS/nixpkgs/nixpkgs-unstable";

/// The workspace manifest template.
fn workspace_toml() -> String {
    format!(
        r#"# granit workspace manifest.
[workspace]
name = "my-workspace"
# Member package directories. Globs (e.g. "packages/*") and explicit paths work.
members = ["packages/*"]

[nixpkgs]
# A flake reference. Use a branch (mutable, locked by granit.lock) or a commit
# SHA (`github:NixOS/nixpkgs/<sha>`) to pin exactly.
ref = "{DEFAULT_NIXPKGS_REF}"

# Optional: extra overlays (e.g. proprietary package sets) composed over nixpkgs.
# [[overlays]]
# source = "git+https://example.com/my-nix-overlay"
"#
    )
}

/// The sample package manifest template.
fn sample_package_toml() -> &'static str {
    r#"[package]
name = "hello"
# Build-time tools by nixpkgs attribute name.
tools = ["coreutils"]
# Artifact dependencies on other packages, as "package:label".
dependencies = []

# Labeled outputs: label = "filename produced by the build".
# granit collects these into the package's output automatically.
[outputs]
message = "message.txt"

[commands]
# The build command runs in a Nix-provisioned environment. Produce your declared
# output files by their natural names; granit collects them.
build = "echo 'hello from granit' > message.txt"
# A `test` command (run via `granit test`). Each command runs in a fresh working
# directory, so a self-contained test re-creates what it needs and still produces
# the package's declared outputs.
test = "echo 'hello from granit' > message.txt && test -s message.txt"
"#
}

/// Scaffold a new workspace rooted at `root`.
pub fn init_workspace(root: &Path) -> Result<()> {
    let manifest = root.join(WORKSPACE_FILE);
    if manifest.exists() {
        bail!(
            "{} already exists here — refusing to overwrite an existing workspace",
            WORKSPACE_FILE
        );
    }

    std::fs::write(&manifest, workspace_toml())
        .with_context(|| format!("failed to write {}", manifest.display()))?;

    let pkg_dir = root.join("packages").join("hello");
    std::fs::create_dir_all(&pkg_dir)
        .with_context(|| format!("failed to create {}", pkg_dir.display()))?;
    let pkg_manifest = pkg_dir.join(PACKAGE_FILE);
    std::fs::write(&pkg_manifest, sample_package_toml())
        .with_context(|| format!("failed to write {}", pkg_manifest.display()))?;

    // A helpful .gitignore for generated artifacts.
    let gitignore = root.join(".gitignore");
    if !gitignore.exists() {
        let _ = std::fs::write(
            &gitignore,
            "# granit generates these; granit.lock is committed.\nflake.nix\nflake.lock\n.granit/\nresult\nresult-*\n",
        );
    }

    println!("Initialized granit workspace:");
    println!("  {}", manifest.display());
    println!("  {}", pkg_manifest.display());
    println!();
    println!("Next steps:");
    println!("  granit doctor   # check your nix environment");
    println!("  granit graph    # view the dependency graph");
    println!("  granit build    # build all packages");
    Ok(())
}

/// Entry point for `granit init` in the current directory.
pub fn init_command() -> Result<()> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    init_workspace(&cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_scaffolds_parseable_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        init_workspace(tmp.path()).unwrap();

        // Files exist.
        assert!(tmp.path().join("granit.toml").is_file());
        assert!(tmp
            .path()
            .join("packages/hello/package.toml")
            .is_file());

        // The scaffolded workspace round-trips through the loader.
        let ws = crate::workspace::load_workspace(tmp.path()).unwrap();
        assert_eq!(ws.packages.len(), 1);
        let hello = ws.package("hello").unwrap();
        assert!(hello.commands.contains_key("build"));
        assert!(hello.commands.contains_key("test"));
        assert_eq!(hello.outputs.get("message").unwrap().path, "message.txt");
    }

    #[test]
    fn init_refuses_to_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("granit.toml"), "existing").unwrap();
        let err = init_workspace(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }
}
