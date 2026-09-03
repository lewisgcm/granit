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
    let flake = nixgen::generate(workspace, graph, &lock, command, &targets, &system)?;
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

        // Determine the output store path. For `build`, also materialize a
        // stable, GC-rooted link tree under `.granit/build/<pkg>/` so outputs
        // are discoverable and survive `nix-collect-garbage`. Only the packages
        // built this invocation are touched.
        let labels = workspace
            .package(target)
            .map(|p| p.outputs.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();

        let out_path = if command == "build" {
            link_build_outputs(runner, workspace, &attr, target)
                .with_context(|| format!("linking outputs for `{target}`"))?
        } else {
            // The build is now in the store; query its path without rebuilding.
            query_out_path(runner, &attr)
                .with_context(|| format!("determining output path for `{target}`"))?
        };

        println!("granit: built `{target}` -> {out_path}");
        built.push(BuiltPackage {
            name: target.clone(),
            out_path,
            labels,
        });
    }

    Ok(built)
}

/// The workspace-relative directory holding the stable, GC-rooted output link
/// tree: `.granit/build/<package>/<label>`.
const BUILD_LINK_DIR: &str = ".granit/build";

/// Create a stable, GC-rooted output link for a built package at
/// `.granit/build/<pkg>/result` (via `nix build --out-link`), pointing at
/// `$out`. Each declared label is then reachable at `result/<label>` (that's
/// how the install phase lays out `$out`), so no per-label links are needed.
/// The `result` link is a GC root, so the output survives
/// `nix-collect-garbage`. Only this package's directory is touched. Returns the
/// output store path.
fn link_build_outputs<R: CommandRunner>(
    runner: &R,
    workspace: &Workspace,
    attr: &str,
    package: &str,
) -> Result<String> {
    let pkg_dir = workspace.root.join(BUILD_LINK_DIR).join(package);
    std::fs::create_dir_all(&pkg_dir)
        .with_context(|| format!("failed to create {}", pkg_dir.display()))?;

    // `nix build --out-link <result>` creates a GC-rooted symlink to $out and,
    // with --print-out-paths, prints the store path. Cached (already built).
    let result_link = pkg_dir.join("result");
    let result_link_str = result_link
        .to_str()
        .context("result link path is not valid UTF-8")?;
    let out = crate::nix::run_captured(
        runner,
        &[
            "build",
            attr,
            "--out-link",
            result_link_str,
            "--print-out-paths",
        ],
    )
    .context("failed to create output link via nix")?;
    if !out.success {
        bail!("could not link output:\n{}", out.stderr.trim());
    }
    let out_path = out
        .stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string();
    if out_path.is_empty() {
        bail!("nix did not report an output path");
    }
    Ok(out_path)
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
                // Point at the stable, GC-rooted result link; the store path is
                // what it resolves to.
                println!(
                    "  {}:{} -> {}/{}/result/{} ({}/{})",
                    pkg.name,
                    label,
                    BUILD_LINK_DIR,
                    pkg.name,
                    label,
                    pkg.out_path,
                    label
                );
            }
        }
    }
}

/// Convenience entry point used by `main` for `granit build`.
pub fn build_command<R: CommandRunner>(
    runner: &R,
    explicit_target: Option<&str>,
) -> Result<()> {
    // `build` is a normal named command: this runs any `needs` hooks in-place
    // first, then the hermetic build derivation.
    run_named_command(runner, explicit_target, "build")
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
    // Emit the full build variant: with command "build", every package (target
    // or dependency) runs its build command, so the targets set is immaterial.
    let flake = nixgen::generate(&ws, &g, &lock, "build", &[], &system)?;
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

/// Entry point for `granit test` and `granit run <cmd>`.
///
/// `build` and `test` run as hermetic Nix derivations (sandboxed, outputs
/// collected). Any *other* command is a custom dev task: it runs in the
/// package's real source directory via `nix develop`, with the package's
/// `tools` on `PATH` and dependency outputs under `$GRANIT_DEPENDENCIES`, so
/// tasks like `go generate` can write back into the working tree. Custom
/// commands do not collect `[outputs]`.
pub fn run_named_command<R: CommandRunner>(
    runner: &R,
    explicit_target: Option<&str>,
    command: &str,
) -> Result<()> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let (ws, g) = load_workspace_and_graph(&cwd)?;

    // Validate the command exists for all resolved targets before doing work.
    // `build` and `test` are tolerant of absence (e.g. a source-only library
    // has no build command — its derivation just collects its source output);
    // custom `run` commands must be explicitly defined.
    let targets = resolve_targets(&ws, explicit_target, &cwd)?;
    if command != "build" && command != "test" {
        ensure_command_present(&ws, &targets, command)?;
    }

    // Run each target's `needs` hooks first, in-place (dev-style). Hooks run
    // in the real source tree so steps like `generate` write back into it
    // before the target command runs. This makes a `build` with `needs`
    // non-hermetic by design (see backlog: hermetic `--release` build).
    let system = current_system(runner)?;
    let lock = lock::ensure_lock(&ws, runner)?;
    let flake = nixgen::generate(&ws, &g, &lock, "build", &[], &system)?;
    let flake_dir = write_flake(&ws, &flake)?;
    let flake_dir_str = flake_dir
        .to_str()
        .context("generated flake directory path is not valid UTF-8")?
        .to_string();
    for target in &targets {
        let pkg = ws.package(target).expect("target resolved from workspace");
        let hooks = resolve_command_hooks(pkg, command)?;
        for hook in &hooks {
            run_command_in_place(runner, &ws, &flake_dir_str, &system, target, hook)?;
        }
    }

    // `build`/`test` are hermetic derivations; everything else runs in-place.
    if command == "build" || command == "test" {
        let done = run_build(runner, &ws, &g, explicit_target, command, &cwd)?;
        println!("granit: `{command}` complete ({} package(s))", done.len());
        print_output_locations(&done);
        return Ok(());
    }

    run_dev_command(runner, &ws, &g, &targets, command)
}

/// Compute the ordered list of `needs` hooks to run before `command` in
/// `pkg` — the transitive closure of `needs`, in dependency-first order, each
/// once, excluding `command` itself. Detects cycles among command hooks.
fn resolve_command_hooks(
    pkg: &crate::workspace::Package,
    command: &str,
) -> Result<Vec<String>> {
    use std::collections::BTreeSet;

    let mut order: Vec<String> = Vec::new();
    let mut done: BTreeSet<String> = BTreeSet::new();
    // `path` tracks the active DFS chain for cycle reporting.
    fn visit(
        pkg: &crate::workspace::Package,
        name: &str,
        order: &mut Vec<String>,
        done: &mut BTreeSet<String>,
        path: &mut Vec<String>,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        if path.iter().any(|p| p == name) {
            path.push(name.to_string());
            bail!("command hook cycle detected: {}", path.join(" -> "));
        }
        let Some(cmd) = pkg.commands.get(name) else {
            // Validated at load time, but guard defensively.
            bail!("package `{}` has no command `{name}`", pkg.name);
        };
        path.push(name.to_string());
        for needed in &cmd.needs {
            visit(pkg, needed, order, done, path)?;
        }
        path.pop();
        done.insert(name.to_string());
        order.push(name.to_string());
        Ok(())
    }

    // Visit the target's needs (but not the target itself); the target runs
    // afterwards via its own path. If the target command isn't defined for this
    // package (e.g. a source lib with no `build`), there are no hooks.
    let Some(cmd) = pkg.commands.get(command) else {
        return Ok(order);
    };
    let mut path: Vec<String> = vec![command.to_string()];
    for needed in &cmd.needs {
        visit(pkg, needed, &mut order, &mut done, &mut path)?;
    }
    Ok(order)
}

/// Run a single command in `target`'s real source directory via `nix develop`
/// against the generated per-package devShell.
fn run_command_in_place<R: CommandRunner>(
    runner: &R,
    workspace: &Workspace,
    flake_dir_str: &str,
    system: &str,
    target: &str,
    command: &str,
) -> Result<()> {
    let pkg = workspace.package(target).expect("target resolved from workspace");
    let script = pkg
        .commands
        .get(command)
        .map(|c| c.run.as_str())
        .expect("command presence validated");
    let shell_ref = format!("path:{flake_dir_str}#devShells.{system}.{target}");
    println!("granit: running `{command}` in `{target}` ({}) ...", pkg.dir.display());
    let out = crate::nix::run_streamed(
        runner,
        &["develop", &shell_ref, "--command", "sh", "-c", script],
        Some(&pkg.dir),
    )
    .with_context(|| format!("failed to run `{command}` in `{target}`"))?;
    if !out.success {
        bail!(
            "command `{command}` failed in package `{target}` (exit {:?})",
            out.code
        );
    }
    Ok(())
}

/// Run a custom command in each target package's real source directory using a
/// generated per-package devShell (tools on PATH, `$GRANIT_DEPENDENCIES` set).
fn run_dev_command<R: CommandRunner>(
    runner: &R,
    workspace: &Workspace,
    graph: &Graph,
    targets: &[String],
    command: &str,
) -> Result<()> {
    let system = current_system(runner)?;
    let lock = lock::ensure_lock(workspace, runner)?;

    // Generate the flake. Command/targets here only affect the `packages`
    // (build-variant) derivations that devShells depend on for materializing
    // dependency outputs, so build everything with `build`.
    let flake = nixgen::generate(workspace, graph, &lock, "build", &[], &system)?;
    let flake_dir = write_flake(workspace, &flake)?;
    let flake_dir_str = flake_dir
        .to_str()
        .context("generated flake directory path is not valid UTF-8")?
        .to_string();

    for target in targets {
        run_command_in_place(runner, workspace, &flake_dir_str, &system, target, command)?;
    }

    println!("granit: `{command}` complete ({} package(s))", targets.len());
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
            .map(|(k, v)| {
                (
                    k.to_string(),
                    crate::workspace::Command {
                        run: v.to_string(),
                        needs: vec![],
                    },
                )
            })
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

    fn pkg_with_needs(name: &str, cmds: &[(&str, &[&str])]) -> Package {
        let mut p = pkg(name, PathBuf::from(name));
        p.commands = cmds
            .iter()
            .map(|(k, needs)| {
                (
                    k.to_string(),
                    crate::workspace::Command {
                        run: format!("run-{k}"),
                        needs: needs.iter().map(|s| s.to_string()).collect(),
                    },
                )
            })
            .collect();
        p
    }

    #[test]
    fn hooks_none_when_no_needs() {
        let p = pkg_with_needs("a", &[("build", &[])]);
        assert!(resolve_command_hooks(&p, "build").unwrap().is_empty());
    }

    #[test]
    fn hooks_single_need() {
        let p = pkg_with_needs("a", &[("generate", &[]), ("build", &["generate"])]);
        assert_eq!(resolve_command_hooks(&p, "build").unwrap(), vec!["generate"]);
    }

    #[test]
    fn hooks_transitive_ordered_and_deduped() {
        // build -> [gen, compile]; gen -> [proto]; compile -> [proto]
        let p = pkg_with_needs(
            "a",
            &[
                ("proto", &[]),
                ("gen", &["proto"]),
                ("compile", &["proto"]),
                ("build", &["gen", "compile"]),
            ],
        );
        let order = resolve_command_hooks(&p, "build").unwrap();
        // proto appears once and before gen and compile.
        assert_eq!(order.iter().filter(|c| *c == "proto").count(), 1);
        let pos = |n: &str| order.iter().position(|c| c == n).unwrap();
        assert!(pos("proto") < pos("gen"));
        assert!(pos("proto") < pos("compile"));
        assert!(!order.contains(&"build".to_string()));
    }

    #[test]
    fn hooks_cycle_detected() {
        // a -> b -> a among hooks.
        let p = pkg_with_needs(
            "a",
            &[("x", &["y"]), ("y", &["x"]), ("build", &["x"])],
        );
        let err = resolve_command_hooks(&p, "build").unwrap_err();
        assert!(err.to_string().contains("cycle detected"), "{err}");
    }

    #[test]
    fn hooks_absent_target_command_yields_no_hooks() {
        // A source-lib style package with no `build` command.
        let p = pkg_with_needs("a", &[("test", &[])]);
        assert!(resolve_command_hooks(&p, "build").unwrap().is_empty());
    }

    #[test]
    fn link_build_outputs_creates_gc_rooted_result_link() {
        use crate::runner::mock::{ok, MockRunner};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let ws = ws_with(root.clone(), vec![pkg("a", root.join("packages/a"))]);

        // The mock returns a store path for the --out-link build call.
        let store = "/nix/store/deadbeef-a-0.0.0";
        let runner = MockRunner::new().with_default(ok(&format!("{store}\n")));

        let out = link_build_outputs(&runner, &ws, "attr", "a").unwrap();
        assert_eq!(out, store);

        // A `result` entry exists under .granit/build/a. (The mock doesn't
        // create the symlink Nix would, but the dir is prepared and the call
        // targets result via --out-link.)
        let pkg_dir = root.join(BUILD_LINK_DIR).join("a");
        assert!(pkg_dir.is_dir(), "package link dir should be created");
        // The nix invocation requested --out-link at .granit/build/a/result.
        let calls = runner.calls.borrow();
        let joined = calls.join("\n");
        assert!(
            joined.contains("--out-link") && joined.contains("/.granit/build/a/result"),
            "should build with --out-link at the result path:\n{joined}"
        );
        assert!(joined.contains("--print-out-paths"));
    }
}
