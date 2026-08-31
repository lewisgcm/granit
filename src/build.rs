//! Build orchestration: generate the flake and invoke `nix build`.
//!
//! Handles subdirectory-aware targeting: when invoked inside a package
//! directory, that package is the default target; from the workspace root with
//! no target, all packages are built. The workspace-root `granit.lock` is
//! always used.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::graph::{self, Graph};
use crate::lock;
use crate::nixgen;
use crate::runner::CommandRunner;
use crate::workspace::{self, Workspace};

/// The generated flake filename, written at the workspace root.
///
/// It must be named exactly `flake.nix` (Nix requires this) and live at the
/// workspace root so the flake's own source tree contains the member packages
/// — required for pure-evaluation builds that reference package sources.
pub const FLAKE_FILE: &str = "flake.nix";

/// The directory the generated flake lives in (the workspace root).
fn flake_dir(workspace: &Workspace) -> PathBuf {
    workspace.root.clone()
}

/// Determine which package (if any) contains `cwd`, given the workspace.
/// Returns the package name whose directory is `cwd` or an ancestor of `cwd`.
pub fn package_containing(workspace: &Workspace, cwd: &Path) -> Option<String> {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    // Choose the most specific (longest path) matching package dir.
    let mut best: Option<(&str, usize)> = None;
    for pkg in &workspace.packages {
        let pdir = pkg.dir.canonicalize().unwrap_or_else(|_| pkg.dir.clone());
        if cwd == pdir || cwd.starts_with(&pdir) {
            let depth = pdir.components().count();
            if best.map(|(_, d)| depth > d).unwrap_or(true) {
                best = Some((pkg.name.as_str(), depth));
            }
        }
    }
    best.map(|(name, _)| name.to_string())
}

/// Resolve the set of package names to build.
///
/// - If `explicit` is `Some`, that package (validated to exist) is the target.
/// - Else if `cwd` is inside a package directory, that package is the target.
/// - Else (workspace root, no arg) all packages are built.
pub fn resolve_targets(
    workspace: &Workspace,
    explicit: Option<&str>,
    cwd: &Path,
) -> Result<Vec<String>> {
    if let Some(name) = explicit {
        if workspace.package(name).is_none() {
            bail!(
                "no package named `{name}` in workspace `{}`",
                workspace.name
            );
        }
        return Ok(vec![name.to_string()]);
    }
    if let Some(name) = package_containing(workspace, cwd) {
        return Ok(vec![name]);
    }
    // All packages.
    Ok(workspace.packages.iter().map(|p| p.name.clone()).collect())
}

/// Discover the workspace from `cwd` and build its validated dependency graph.
/// Centralizes the load+validate step shared by nearly every command.
pub fn load_workspace_and_graph(cwd: &Path) -> Result<(Workspace, Graph)> {
    let ws = workspace::discover_and_load(cwd)?;
    let g = graph::build(&ws)?;
    Ok((ws, g))
}

/// Query the current Nix system double (e.g. `aarch64-darwin`).
pub fn current_system<R: CommandRunner>(runner: &R) -> Result<String> {
    let out = crate::nix::run_captured(
        runner,
        &["eval", "--impure", "--raw", "--expr", "builtins.currentSystem"],
    )
    .context("failed to query current system from nix")?;
    if !out.success {
        bail!(
            "could not determine current system via nix:\n{}",
            out.stderr.trim()
        );
    }
    Ok(out.stdout.trim().to_string())
}

/// Write the generated flake into the workspace's `.granit` directory,
/// creating the directory if needed. Returns the flake directory path.
fn write_flake(workspace: &Workspace, contents: &str) -> Result<PathBuf> {
    let dir = flake_dir(workspace);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;
    let path = dir.join(FLAKE_FILE);
    std::fs::write(&path, contents)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(dir)
}

/// The result of building one package: its name, the resulting store path, and
/// the labeled outputs available under that path (`$out/<label>`).
#[derive(Debug, Clone)]
pub struct BuiltPackage {
    pub name: String,
    pub out_path: String,
    /// Declared output labels, sorted.
    pub labels: Vec<String>,
}

/// The full build flow for the `build`/`test`/`run` commands.
///
/// `command` is the named command each derivation runs (`build`, `test`, or a
/// custom name). Streams Nix build logs to the terminal so the user sees build
/// output live, then captures each package's output store path. Returns the
/// built packages in order.
pub fn run_build<R: CommandRunner>(
    runner: &R,
    workspace: &Workspace,
    graph: &Graph,
    explicit_target: Option<&str>,
    command: &str,
    cwd: &Path,
) -> Result<Vec<BuiltPackage>> {
    let targets = resolve_targets(workspace, explicit_target, cwd)?;
    let system = current_system(runner)?;

    let lock = lock::ensure_lock(workspace, runner)?;
    let flake = nixgen::generate(workspace, graph, &lock, command, &system)?;
    let flake_dir = write_flake(workspace, &flake)?;

    let flake_dir_str = flake_dir
        .to_str()
        .context("generated flake directory path is not valid UTF-8")?
        .to_string();

    let mut built = Vec::new();
    for target in &targets {
        // Use a `path:` flake ref so Nix treats the workspace root as a plain
        // directory. The generated flake.nix is gitignored, so a bare path ref
        // would fail with "not tracked by Git" inside a repository; `path:`
        // bypasses that while still evaluating in pure mode.
        let attr = format!("path:{flake_dir_str}#packages.{system}.{target}");
        println!("granit: building `{target}` ...");

        // Stream the build with `--print-build-logs` so the user sees the actual
        // build-command output live (nix is quiet about logs by default).
        let out = crate::nix::run_streamed(
            runner,
            &["build", &attr, "--no-link", "--print-build-logs"],
            Some(&workspace.root),
        )
        .with_context(|| format!("failed to invoke `nix build` for `{target}`"))?;
        if !out.success {
            bail!(
                "nix build failed for package `{target}` (exit {:?})",
                out.code
            );
        }

        // The build is now in the store; query its out path without rebuilding.
        let out_path = query_out_path(runner, &attr)
            .with_context(|| format!("determining output path for `{target}`"))?;

        let labels = workspace
            .package(target)
            .map(|p| p.outputs.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();

        println!("granit: built `{target}` -> {out_path}");
        built.push(BuiltPackage {
            name: target.clone(),
            out_path,
            labels,
        });
    }

    Ok(built)
}

/// Query a built flake attribute's output store path (cached; no rebuild).
fn query_out_path<R: CommandRunner>(runner: &R, attr: &str) -> Result<String> {
    let out = crate::nix::run_captured(
        runner,
        &["build", attr, "--no-link", "--print-out-paths"],
    )
    .context("failed to query output path from nix")?;
    if !out.success {
        bail!("could not determine output path:\n{}", out.stderr.trim());
    }
    // `--print-out-paths` may print multiple lines for multi-output derivations;
    // take the first non-empty line.
    let path = out
        .stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string();
    if path.is_empty() {
        bail!("nix did not report an output path");
    }
    Ok(path)
}

/// Print a summary of where each built package's outputs live.
fn print_output_locations(built: &[BuiltPackage]) {
    if built.is_empty() {
        return;
    }
    println!("\nOutputs:");
    for pkg in built {
        if pkg.labels.is_empty() {
            println!("  {} -> {}", pkg.name, pkg.out_path);
        } else {
            for label in &pkg.labels {
                println!("  {}:{} -> {}/{}", pkg.name, label, pkg.out_path, label);
            }
        }
    }
}

/// Convenience entry point used by `main` for `granit build`.
pub fn build_command<R: CommandRunner>(
    runner: &R,
    explicit_target: Option<&str>,
) -> Result<()> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let (ws, g) = load_workspace_and_graph(&cwd)?;
    let built = run_build(runner, &ws, &g, explicit_target, "build", &cwd)?;
    println!("granit: build complete ({} package(s))", built.len());
    print_output_locations(&built);
    Ok(())
}

/// Entry point for `granit graph`: print the dependency graph and build order.
pub fn graph_command() -> Result<()> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let (ws, g) = load_workspace_and_graph(&cwd)?;
    print!("{}", graph::render(&ws, &g));
    Ok(())
}

/// Entry point for `granit build --emit-only`: print the generated flake for
/// the current system without building.
pub fn emit_command<R: CommandRunner>(runner: &R) -> Result<()> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let (ws, g) = load_workspace_and_graph(&cwd)?;
    let lock = lock::ensure_lock(&ws, runner)?;
    let system = current_system(runner)?;
    let flake = nixgen::generate(&ws, &g, &lock, "build", &system)?;
    print!("{flake}");
    Ok(())
}

/// Entry point for `granit update`: re-resolve pinned inputs and rewrite the lock.
pub fn update_command<R: CommandRunner>(runner: &R) -> Result<()> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let ws = workspace::discover_and_load(&cwd)?;
    let updated = lock::update(&ws, runner)?;
    println!(
        "Updated {} — nixpkgs pinned to {}",
        lock::LOCK_FILE,
        updated.nixpkgs.locked_ref
    );
    for (overlay, locked) in ws.overlays.iter().zip(updated.overlays.iter()) {
        println!("  overlay {} -> {}", overlay.source, locked.locked_ref);
    }
    Ok(())
}

/// Verify every target package defines the named command; error clearly if not.
fn ensure_command_present(
    workspace: &Workspace,
    targets: &[String],
    command: &str,
) -> Result<()> {
    for target in targets {
        let pkg = workspace
            .package(target)
            .expect("target was resolved from the workspace");
        if !pkg.commands.contains_key(command) {
            let available: Vec<&str> = pkg.commands.keys().map(|s| s.as_str()).collect();
            let available = if available.is_empty() {
                "(none defined)".to_string()
            } else {
                available.join(", ")
            };
            bail!(
                "package `{target}` has no `{command}` command defined in [commands] \
                 (available: {available})"
            );
        }
    }
    Ok(())
}

/// Entry point for `granit test` and `granit run <cmd>`: run a named command in
/// the target package(s), erroring if the command is not defined.
pub fn run_named_command<R: CommandRunner>(
    runner: &R,
    explicit_target: Option<&str>,
    command: &str,
) -> Result<()> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let (ws, g) = load_workspace_and_graph(&cwd)?;

    // Validate the command exists for all resolved targets before doing work.
    let targets = resolve_targets(&ws, explicit_target, &cwd)?;
    ensure_command_present(&ws, &targets, command)?;

    let done = run_build(runner, &ws, &g, explicit_target, command, &cwd)?;
    println!(
        "granit: `{command}` complete ({} package(s))",
        done.len()
    );
    print_output_locations(&done);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{Overlay, Package, Workspace};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn pkg(name: &str, dir: PathBuf) -> Package {
        Package {
            name: name.to_string(),
            dir,
            tools: vec![],
            dependencies: vec![],
            exclude: vec![],
            outputs: BTreeMap::new(),
            commands: BTreeMap::new(),
        }
    }

    fn ws_with(root: PathBuf, pkgs: Vec<Package>) -> Workspace {
        Workspace {
            name: "test".into(),
            root,
            nixpkgs_ref: "github:NixOS/nixpkgs/nixpkgs-unstable".into(),
            overlays: Vec::<Overlay>::new(),
            packages: pkgs,
        }
    }

    #[test]
    fn explicit_target_takes_precedence() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join("packages/a")).unwrap();
        std::fs::create_dir_all(root.join("packages/b")).unwrap();
        let ws = ws_with(
            root.clone(),
            vec![
                pkg("a", root.join("packages/a")),
                pkg("b", root.join("packages/b")),
            ],
        );
        let targets = resolve_targets(&ws, Some("b"), &root).unwrap();
        assert_eq!(targets, vec!["b"]);
    }

    #[test]
    fn explicit_unknown_target_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let ws = ws_with(root.clone(), vec![pkg("a", root.join("packages/a"))]);
        let err = resolve_targets(&ws, Some("ghost"), &root).unwrap_err();
        assert!(err.to_string().contains("no package named `ghost`"));
    }

    #[test]
    fn cwd_inside_package_defaults_to_that_package() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let adir = root.join("packages/a");
        let bdir = root.join("packages/b");
        std::fs::create_dir_all(&adir).unwrap();
        std::fs::create_dir_all(&bdir).unwrap();
        let ws = ws_with(
            root.clone(),
            vec![pkg("a", adir.clone()), pkg("b", bdir.clone())],
        );
        // From inside packages/a -> default target a.
        let targets = resolve_targets(&ws, None, &adir).unwrap();
        assert_eq!(targets, vec!["a"]);
        // Even from a subdirectory of packages/b.
        let bsub = bdir.join("src");
        std::fs::create_dir_all(&bsub).unwrap();
        let targets = resolve_targets(&ws, None, &bsub).unwrap();
        assert_eq!(targets, vec!["b"]);
    }

    #[test]
    fn cwd_at_root_builds_all() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join("packages/a")).unwrap();
        std::fs::create_dir_all(root.join("packages/b")).unwrap();
        let ws = ws_with(
            root.clone(),
            vec![
                pkg("a", root.join("packages/a")),
                pkg("b", root.join("packages/b")),
            ],
        );
        let mut targets = resolve_targets(&ws, None, &root).unwrap();
        targets.sort();
        assert_eq!(targets, vec!["a", "b"]);
    }

    fn pkg_with_cmds(name: &str, dir: PathBuf, cmds: &[(&str, &str)]) -> Package {
        let mut p = pkg(name, dir);
        p.commands = cmds
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        p
    }

    #[test]
    fn ensure_command_present_ok_when_defined() {
        let root = PathBuf::from("/tmp/x");
        let ws = ws_with(
            root.clone(),
            vec![pkg_with_cmds("a", root.join("a"), &[("test", "true")])],
        );
        assert!(ensure_command_present(&ws, &["a".to_string()], "test").is_ok());
    }

    #[test]
    fn ensure_command_present_errors_when_missing() {
        let root = PathBuf::from("/tmp/x");
        let ws = ws_with(
            root.clone(),
            vec![pkg_with_cmds("a", root.join("a"), &[("build", "true")])],
        );
        let err = ensure_command_present(&ws, &["a".to_string()], "test").unwrap_err();
        assert!(err.to_string().contains("no `test` command"), "{err}");
        assert!(err.to_string().contains("build"), "should list available: {err}");
    }
}
