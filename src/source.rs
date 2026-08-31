//! Computes the set of a package's source files to include in the build,
//! applying gitignore-style `exclude` patterns (rooted at the package dir) plus
//! implicit excludes for `.git` and `.granit`.
//!
//! Matching is done here in Rust (via the `ignore` crate's gitignore engine)
//! rather than in the generated Nix, so the semantics are the familiar
//! gitignore ones and are unit-testable. The resulting file list is emitted
//! into the flake as an explicit `src` filter (see `nixgen`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ignore::gitignore::GitignoreBuilder;

/// Patterns always excluded from build source, regardless of package config.
pub const IMPLICIT_EXCLUDES: &[&str] = &[".git", ".granit"];

/// Compute the package-relative paths of files to include as build source.
///
/// - `dir` is the package directory.
/// - `exclude` are gitignore-style patterns rooted at `dir`.
///
/// Returns a sorted list of relative paths (files only; directories are
/// implied by their contents). Excluded directories are pruned so their
/// contents are not walked.
pub fn included_files(dir: &Path, exclude: &[String]) -> Result<Vec<PathBuf>> {
    // Build a gitignore matcher rooted at the package dir from the implicit
    // excludes plus the user's patterns.
    let mut builder = GitignoreBuilder::new(dir);
    for pat in IMPLICIT_EXCLUDES {
        builder
            .add_line(None, pat)
            .with_context(|| format!("invalid implicit exclude `{pat}`"))?;
    }
    for pat in exclude {
        builder
            .add_line(None, pat)
            .with_context(|| format!("invalid exclude pattern `{pat}`"))?;
    }
    let matcher = builder
        .build()
        .context("failed to build exclude matcher")?;

    let mut included: Vec<PathBuf> = Vec::new();
    walk(dir, dir, &matcher, &mut included)?;
    included.sort();
    Ok(included)
}

/// Recursively walk `current`, collecting included files relative to `root`,
/// pruning excluded directories.
fn walk(
    root: &Path,
    current: &Path,
    matcher: &ignore::gitignore::Gitignore,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = std::fs::read_dir(current)
        .with_context(|| format!("failed to read directory {}", current.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("error reading entry in {}", current.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to stat {}", path.display()))?;
        let is_dir = file_type.is_dir();

        // Match against the gitignore matcher using the path relative to root.
        let matched = matcher.matched_path_or_any_parents(&path, is_dir);
        if matched.is_ignore() {
            continue; // excluded; prune directories entirely.
        }

        if is_dir {
            walk(root, &path, matcher, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_path_buf();
            out.push(rel);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(path: &Path) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, "x").unwrap();
    }

    fn rels(paths: &[PathBuf]) -> Vec<String> {
        let mut v: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn includes_everything_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("package.toml"));
        touch(&tmp.path().join("main.tsp"));
        touch(&tmp.path().join("src/a.ts"));
        let files = included_files(tmp.path(), &[]).unwrap();
        assert_eq!(rels(&files), vec!["main.tsp", "package.toml", "src/a.ts"]);
    }

    #[test]
    fn bare_dir_name_excludes_dir_and_contents() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("package.json"));
        touch(&tmp.path().join("node_modules/dep/index.js"));
        touch(&tmp.path().join("tsp-output/schema/openapi.yaml"));
        let files = included_files(
            tmp.path(),
            &["node_modules".to_string(), "tsp-output".to_string()],
        )
        .unwrap();
        assert_eq!(rels(&files), vec!["package.json"]);
    }

    #[test]
    fn gitignore_style_patterns() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("keep.ts"));
        touch(&tmp.path().join("debug.log"));
        touch(&tmp.path().join("nested/trace.log"));
        touch(&tmp.path().join("nested/keep.ts"));
        // `*.log` matches at any depth (gitignore semantics).
        let files = included_files(tmp.path(), &["*.log".to_string()]).unwrap();
        assert_eq!(rels(&files), vec!["keep.ts", "nested/keep.ts"]);
    }

    #[test]
    fn anchored_pattern_only_matches_root() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("dist/out.js"));
        touch(&tmp.path().join("sub/dist/out.js"));
        // Leading slash anchors to the package root.
        let files = included_files(tmp.path(), &["/dist".to_string()]).unwrap();
        assert_eq!(rels(&files), vec!["sub/dist/out.js"]);
    }

    #[test]
    fn implicit_excludes_git_and_granit() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("package.toml"));
        touch(&tmp.path().join(".git/HEAD"));
        touch(&tmp.path().join(".granit/flake.nix"));
        let files = included_files(tmp.path(), &[]).unwrap();
        assert_eq!(rels(&files), vec!["package.toml"]);
    }
}
