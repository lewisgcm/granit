//! Workspace discovery, loading, and validation.
//!
//! Locates the workspace root by walking up from a starting directory, parses
//! `granit.toml` and each member `package.toml`, expands glob members, and
//! validates the result into a processed [`Workspace`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use crate::model::{PackageFile, WorkspaceFile};

/// The filename of the workspace manifest.
pub const WORKSPACE_FILE: &str = "granit.toml";
/// The filename of a package manifest.
pub const PACKAGE_FILE: &str = "package.toml";

/// A parsed artifact-dependency reference of the form `package:label`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyRef {
    pub package: String,
    pub label: String,
}

impl DependencyRef {
    /// Parse a `package:label` string.
    pub fn parse(s: &str) -> Result<Self> {
        let mut parts = s.splitn(2, ':');
        let package = parts.next().unwrap_or("").trim();
        let label = parts.next().map(str::trim);
        match label {
            Some(label) if !package.is_empty() && !label.is_empty() => Ok(DependencyRef {
                package: package.to_string(),
                label: label.to_string(),
            }),
            _ => bail!(
                "invalid dependency reference `{s}`: expected the form `package:label` \
                 (e.g. `a:hello`)"
            ),
        }
    }
}

/// A fully-loaded and validated package.
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    /// Directory containing this package's `package.toml`.
    pub dir: PathBuf,
    pub tools: Vec<String>,
    pub dependencies: Vec<DependencyRef>,
    /// Gitignore-style exclude patterns for the build source (package-rooted).
    pub exclude: Vec<String>,
    /// label -> filename produced by the build.
    pub outputs: BTreeMap<String, String>,
    /// command name -> command string.
    pub commands: BTreeMap<String, String>,
}

/// A resolved overlay source (flake ref).
#[derive(Debug, Clone)]
pub struct Overlay {
    pub source: String,
}

/// A fully-loaded and validated workspace.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub name: String,
    /// Absolute path to the workspace root (directory containing granit.toml).
    pub root: PathBuf,
    /// The nixpkgs flake reference.
    pub nixpkgs_ref: String,
    pub overlays: Vec<Overlay>,
    /// Packages keyed by name, in a stable (sorted) order.
    pub packages: Vec<Package>,
}

impl Workspace {
    /// Find a package by name.
    pub fn package(&self, name: &str) -> Option<&Package> {
        self.packages.iter().find(|p| p.name == name)
    }
}

/// Walk up from `start` looking for a directory containing `granit.toml`.
pub fn find_workspace_root(start: &Path) -> Result<PathBuf> {
    let start = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir()
            .context("could not determine current directory")?
            .join(start)
    };

    let mut current: Option<&Path> = Some(start.as_path());
    while let Some(dir) = current {
        if dir.join(WORKSPACE_FILE).is_file() {
            return Ok(dir.to_path_buf());
        }
        current = dir.parent();
    }
    bail!(
        "could not find `{WORKSPACE_FILE}` in `{}` or any parent directory. \
         Are you inside a granit workspace? Run `granit init` to create one.",
        start.display()
    )
}

/// Load and validate the workspace whose root is `root`.
pub fn load_workspace(root: &Path) -> Result<Workspace> {
    let manifest_path = root.join(WORKSPACE_FILE);
    let text = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let wsfile: WorkspaceFile = toml::from_str(&text)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;

    let member_dirs = expand_members(root, &wsfile.workspace.members)?;

    let mut packages: Vec<Package> = Vec::new();
    for dir in member_dirs {
        let pkg = load_package(&dir)?;
        packages.push(pkg);
    }

    // Sort for deterministic ordering.
    packages.sort_by(|a, b| a.name.cmp(&b.name));

    // Validate: unique names.
    let mut seen: BTreeMap<&str, &Path> = BTreeMap::new();
    for pkg in &packages {
        if let Some(prev) = seen.insert(pkg.name.as_str(), pkg.dir.as_path()) {
            bail!(
                "duplicate package name `{}`: defined in `{}` and `{}`",
                pkg.name,
                prev.display(),
                pkg.dir.display()
            );
        }
    }

    let overlays = wsfile
        .overlays
        .into_iter()
        .map(|o| Overlay { source: o.source })
        .collect();

    Ok(Workspace {
        name: wsfile.workspace.name,
        root: root.to_path_buf(),
        nixpkgs_ref: wsfile.nixpkgs.reference().to_string(),
        overlays,
        packages,
    })
}

/// Convenience: discover the root from `start` and load it.
pub fn discover_and_load(start: &Path) -> Result<Workspace> {
    let root = find_workspace_root(start)?;
    load_workspace(&root)
}

/// Expand member patterns (globs or explicit paths) into concrete directories,
/// each of which must contain a `package.toml`.
fn expand_members(root: &Path, members: &[String]) -> Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut seen: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();

    for pattern in members {
        // Resolve pattern relative to root.
        let full = root.join(pattern);
        let full_str = full
            .to_str()
            .ok_or_else(|| anyhow!("member pattern `{pattern}` is not valid UTF-8"))?;

        if full.is_dir() {
            // Explicit directory path (no glob metacharacters or a real dir).
            add_member_dir(&full, pattern, &mut dirs, &mut seen)?;
            continue;
        }

        // Treat as a glob pattern.
        let mut matched_any = false;
        for entry in glob::glob(full_str)
            .with_context(|| format!("invalid member glob pattern `{pattern}`"))?
        {
            let path = entry.with_context(|| format!("error reading glob match for `{pattern}`"))?;
            if path.is_dir() {
                matched_any = true;
                add_member_dir(&path, pattern, &mut dirs, &mut seen)?;
            }
        }

        if !matched_any {
            bail!(
                "member pattern `{pattern}` matched no package directories under `{}`",
                root.display()
            );
        }
    }

    Ok(dirs)
}

fn add_member_dir(
    dir: &Path,
    pattern: &str,
    dirs: &mut Vec<PathBuf>,
    seen: &mut std::collections::BTreeSet<PathBuf>,
) -> Result<()> {
    let canonical = dir.to_path_buf();
    if !canonical.join(PACKAGE_FILE).is_file() {
        bail!(
            "member `{}` (from pattern `{pattern}`) has no `{PACKAGE_FILE}`",
            canonical.display()
        );
    }
    if seen.insert(canonical.clone()) {
        dirs.push(canonical);
    }
    Ok(())
}

/// Load and validate a single package from its directory.
pub fn load_package(dir: &Path) -> Result<Package> {
    let manifest_path = dir.join(PACKAGE_FILE);
    let text = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let file: PackageFile = toml::from_str(&text)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;

    let name = file.package.name.trim().to_string();
    if name.is_empty() {
        bail!("package at `{}` has an empty name", dir.display());
    }

    // Parse dependency refs.
    let mut dependencies = Vec::new();
    for dep in &file.package.dependencies {
        let parsed = DependencyRef::parse(dep)
            .with_context(|| format!("in package `{name}` ({})", manifest_path.display()))?;
        dependencies.push(parsed);
    }

    Ok(Package {
        name,
        dir: dir.to_path_buf(),
        tools: file.package.tools,
        dependencies,
        exclude: file.package.exclude,
        outputs: file.outputs,
        commands: file.commands,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    /// Build a minimal valid workspace under `root`.
    fn scaffold(root: &Path) {
        write(
            &root.join("granit.toml"),
            r#"
[workspace]
name = "example"
members = ["packages/*"]

[nixpkgs]
ref = "github:NixOS/nixpkgs/nixpkgs-unstable"
"#,
        );
        write(
            &root.join("packages/a/package.toml"),
            r#"
[package]
name = "a"
tools = ["coreutils"]

[outputs]
hello = "hello.txt"

[commands]
build = "echo 'hello world' > hello.txt"
"#,
        );
        write(
            &root.join("packages/b/package.toml"),
            r#"
[package]
name = "b"
tools = ["coreutils"]
dependencies = ["a:hello"]

[outputs]
result = "from-a.txt"

[commands]
build = "cat $GRANIT_DEPENDENCIES/a/hello > from-a.txt"
"#,
        );
    }

    #[test]
    fn parses_dependency_ref() {
        let d = DependencyRef::parse("a:hello").unwrap();
        assert_eq!(d.package, "a");
        assert_eq!(d.label, "hello");
    }

    #[test]
    fn rejects_bad_dependency_refs() {
        assert!(DependencyRef::parse("a").is_err());
        assert!(DependencyRef::parse("a:").is_err());
        assert!(DependencyRef::parse(":hello").is_err());
        assert!(DependencyRef::parse("").is_err());
    }

    #[test]
    fn loads_valid_workspace_with_globs() {
        let tmp = tempfile::tempdir().unwrap();
        scaffold(tmp.path());
        let ws = load_workspace(tmp.path()).unwrap();
        assert_eq!(ws.name, "example");
        assert_eq!(ws.nixpkgs_ref, "github:NixOS/nixpkgs/nixpkgs-unstable");
        assert_eq!(ws.packages.len(), 2);
        let a = ws.package("a").unwrap();
        assert_eq!(a.tools, vec!["coreutils"]);
        assert_eq!(a.outputs.get("hello").unwrap(), "hello.txt");
        let b = ws.package("b").unwrap();
        assert_eq!(b.dependencies.len(), 1);
        assert_eq!(b.dependencies[0].package, "a");
        assert_eq!(b.dependencies[0].label, "hello");
    }

    #[test]
    fn discovers_root_from_nested_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        scaffold(tmp.path());
        let nested = tmp.path().join("packages/a");
        let root = find_workspace_root(&nested).unwrap();
        // Compare canonicalized to avoid symlink differences (e.g. /var vs /private/var on macOS).
        assert_eq!(
            root.canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn errors_when_no_workspace_found() {
        let tmp = tempfile::tempdir().unwrap();
        let err = find_workspace_root(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("could not find"));
    }

    #[test]
    fn errors_on_malformed_toml() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("granit.toml"), "this is not valid = = toml");
        let err = load_workspace(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("failed to parse"));
    }

    #[test]
    fn errors_on_missing_required_field() {
        let tmp = tempfile::tempdir().unwrap();
        // Missing [nixpkgs].
        write(
            &tmp.path().join("granit.toml"),
            r#"
[workspace]
name = "x"
members = []
"#,
        );
        let err = load_workspace(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("failed to parse"));
    }

    #[test]
    fn errors_on_matched_dir_missing_package_toml() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("granit.toml"),
            r#"
[workspace]
name = "x"
members = ["packages/*"]

[nixpkgs]
ref = "github:NixOS/nixpkgs/nixpkgs-unstable"
"#,
        );
        // Create a dir under packages/ with no package.toml.
        fs::create_dir_all(tmp.path().join("packages/empty")).unwrap();
        let err = load_workspace(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("has no `package.toml`"));
    }

    #[test]
    fn errors_on_duplicate_package_names() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("granit.toml"),
            r#"
[workspace]
name = "x"
members = ["packages/*"]

[nixpkgs]
ref = "github:NixOS/nixpkgs/nixpkgs-unstable"
"#,
        );
        write(
            &tmp.path().join("packages/one/package.toml"),
            "[package]\nname = \"dup\"\n",
        );
        write(
            &tmp.path().join("packages/two/package.toml"),
            "[package]\nname = \"dup\"\n",
        );
        let err = load_workspace(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("duplicate package name"));
    }

    #[test]
    fn errors_on_pattern_matching_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("granit.toml"),
            r#"
[workspace]
name = "x"
members = ["nonexistent/*"]

[nixpkgs]
ref = "github:NixOS/nixpkgs/nixpkgs-unstable"
"#,
        );
        let err = load_workspace(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("matched no package directories"));
    }
}
