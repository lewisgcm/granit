//! End-to-end integration test over the example workspace.
//!
//! The parse/graph/emit portions run unconditionally. The actual `nix build`
//! portion is gated on nix being available (on PATH or in the standard Nix
//! profile dir) and is skipped with a message otherwise, so the suite passes in
//! environments without Nix.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Path to the compiled `granit` binary under test.
fn granit_bin() -> &'static str {
    env!("CARGO_BIN_EXE_granit")
}

/// The committed example workspace.
fn example_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("basic")
}

/// Locate a `nix` binary, checking PATH and the standard Nix profile dir.
/// Returns the directory to prepend to PATH so the binary is invocable.
fn nix_bin_dir() -> Option<PathBuf> {
    // On PATH?
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if dir.join("nix").is_file() {
                return Some(dir);
            }
        }
    }
    // Standard multi-user profile location.
    for dir in [
        "/nix/var/nix/profiles/default/bin",
        "/run/current-system/sw/bin",
    ] {
        let p = Path::new(dir);
        if p.join("nix").is_file() {
            return Some(p.to_path_buf());
        }
    }
    None
}

/// Run the granit binary in `cwd` with `args`, returning (success, stdout, stderr).
/// If `nix_dir` is provided, it is prepended to PATH.
fn run_granit(cwd: &Path, args: &[&str], nix_dir: Option<&Path>) -> (bool, String, String) {
    let mut cmd = Command::new(granit_bin());
    cmd.args(args).current_dir(cwd);
    if let Some(dir) = nix_dir {
        let existing = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.display(), existing);
        cmd.env("PATH", new_path);
    }
    let out = cmd.output().expect("failed to spawn granit");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Copy the example workspace into a fresh temp dir so the test never mutates
/// the committed files (granit writes flake.nix / granit.lock).
fn stage_example() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let src = example_root();
    copy_dir(&src, tmp.path());
    tmp
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

#[test]
fn graph_shows_dependency_and_order() {
    // Runs unconditionally — no nix required.
    let staged = stage_example();
    let (ok, stdout, stderr) = run_granit(staged.path(), &["graph"], None);
    assert!(ok, "granit graph failed: {stderr}");
    assert!(stdout.contains("b -> a:hello"), "graph output:\n{stdout}");
    // Build order must place a before b.
    let a_pos = stdout.find("1. a").expect("a not first in order");
    let b_pos = stdout.find("2. b").expect("b not second in order");
    assert!(a_pos < b_pos);
}

#[test]
fn graph_works_from_subdirectory() {
    let staged = stage_example();
    let subdir = staged.path().join("packages/b");
    let (ok, stdout, stderr) = run_granit(&subdir, &["graph"], None);
    assert!(ok, "granit graph from subdir failed: {stderr}");
    assert!(stdout.contains("b -> a:hello"));
}

#[test]
fn end_to_end_build_with_nix() {
    let Some(nix_dir) = nix_bin_dir() else {
        eprintln!("SKIP: nix not found on PATH or in standard profile; skipping end-to-end build test");
        return;
    };

    let staged = stage_example();

    // 1. Resolve the lock (granit update writes granit.lock).
    let (ok, _out, err) = run_granit(staged.path(), &["update"], Some(&nix_dir));
    assert!(ok, "granit update failed:\n{err}");
    assert!(
        staged.path().join("granit.lock").is_file(),
        "granit.lock was not created"
    );

    // 2. Build b (which requires a). Use --print-out-paths via the CLI's build.
    let (ok, out, err) = run_granit(staged.path(), &["build", "b"], Some(&nix_dir));
    assert!(ok, "granit build b failed:\nSTDOUT:\n{out}\nSTDERR:\n{err}");

    // 3. Verify b's output content by building directly and inspecting the store path.
    // Ask nix for b's out path and read result/from-a content via the output label.
    let sys_out = Command::new(nix_dir.join("nix"))
        .args([
            "--extra-experimental-features",
            "nix-command flakes",
            "eval",
            "--impure",
            "--raw",
            "--expr",
            "builtins.currentSystem",
        ])
        .current_dir(staged.path())
        .output()
        .expect("nix eval currentSystem failed");
    let system = String::from_utf8_lossy(&sys_out.stdout).trim().to_string();
    assert!(!system.is_empty(), "empty system double");

    let flake_dir = staged.path().to_path_buf();
    let build_out = Command::new(nix_dir.join("nix"))
        .args([
            "--extra-experimental-features",
            "nix-command flakes",
            "build",
            &format!("path:{}#packages.{system}.b", flake_dir.display()),
            "--no-link",
            "--print-out-paths",
        ])
        .current_dir(staged.path())
        .output()
        .expect("nix build b failed to spawn");
    assert!(
        build_out.status.success(),
        "nix build b failed:\n{}",
        String::from_utf8_lossy(&build_out.stderr)
    );
    let store_path = String::from_utf8_lossy(&build_out.stdout)
        .trim()
        .to_string();
    assert!(store_path.starts_with("/nix/store/"), "unexpected out path: {store_path}");

    // b's declared output is labeled `result`.
    let result_file = Path::new(&store_path).join("result");
    let content = std::fs::read_to_string(&result_file)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", result_file.display()));
    assert!(
        content.contains("hello world"),
        "b's output should contain a's content, got:\n{content}"
    );
    assert!(
        content.contains("built by b"),
        "b's output should contain its own content, got:\n{content}"
    );
}
