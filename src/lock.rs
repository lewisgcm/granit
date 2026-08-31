//! `granit.lock` — Granit's owned lock file.
//!
//! Granit resolves the workspace's nixpkgs flake ref and each overlay source to
//! a concrete, immutable pin (rev + narHash) via `nix flake metadata --json`,
//! and records the result in `granit.lock` at the workspace root. Generated
//! `flake.nix` inputs are pinned from this file, so builds are reproducible.
//!
//! Resolution policy:
//! - On build, if `granit.lock` is missing, resolve and write it.
//! - If present, reuse it as-is.
//! - `granit update` always re-resolves and rewrites.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::runner::CommandRunner;
use crate::workspace::Workspace;

/// The lock filename at the workspace root.
pub const LOCK_FILE: &str = "granit.lock";

/// A single resolved input pin.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LockedInput {
    /// The original flake ref the user requested (may be mutable, e.g. a branch).
    pub original_ref: String,
    /// The locked (immutable) flake ref, suitable to paste into flake.nix
    /// inputs, e.g. `github:NixOS/nixpkgs/<rev>`.
    pub locked_ref: String,
    /// The resolved immutable revision (commit sha), if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// The narHash of the locked source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nar_hash: Option<String>,
    /// lastModified timestamp reported by nix, for informational purposes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<i64>,
}

/// The full lock file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Lock {
    /// Lock format version.
    pub version: u32,
    /// The resolved nixpkgs pin.
    pub nixpkgs: LockedInput,
    /// Resolved overlay pins, in the same order as declared in granit.toml.
    #[serde(default)]
    pub overlays: Vec<LockedInput>,
}

impl Lock {
    pub const CURRENT_VERSION: u32 = 1;

    /// Path to the lock file for a workspace root.
    pub fn path(root: &Path) -> PathBuf {
        root.join(LOCK_FILE)
    }

    /// Load and parse the lock file if it exists.
    pub fn load(root: &Path) -> Result<Option<Lock>> {
        let path = Self::path(root);
        if !path.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let lock: Lock = toml::from_str(&text)
            .with_context(|| format!("failed to parse {} (is it corrupt?)", path.display()))?;
        if lock.version != Self::CURRENT_VERSION {
            bail!(
                "{} has unsupported version {} (this granit supports version {}). \
                 Run `granit update` to regenerate it.",
                path.display(),
                lock.version,
                Self::CURRENT_VERSION
            );
        }
        Ok(Some(lock))
    }

    /// Serialize and write the lock file to the workspace root.
    pub fn save(&self, root: &Path) -> Result<()> {
        let path = Self::path(root);
        let text = toml::to_string_pretty(self)
            .context("failed to serialize granit.lock")?;
        let header = "# This file is generated and managed by granit.\n\
                      # It pins nixpkgs and overlay sources for reproducible builds.\n\
                      # Commit it to version control. Run `granit update` to refresh.\n\n";
        std::fs::write(&path, format!("{header}{text}"))
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }
}

/// Ensure a lock exists for `workspace`, resolving it if missing. Returns the
/// lock (loaded or freshly resolved). Used by build/test/run.
pub fn ensure_lock<R: CommandRunner>(workspace: &Workspace, runner: &R) -> Result<Lock> {
    if let Some(existing) = Lock::load(&workspace.root)? {
        return Ok(existing);
    }
    let lock = resolve(workspace, runner)?;
    lock.save(&workspace.root)?;
    Ok(lock)
}

/// Force re-resolution and rewrite the lock (`granit update`).
pub fn update<R: CommandRunner>(workspace: &Workspace, runner: &R) -> Result<Lock> {
    let lock = resolve(workspace, runner)?;
    lock.save(&workspace.root)?;
    Ok(lock)
}

/// Resolve all inputs (nixpkgs + overlays) to concrete pins.
pub fn resolve<R: CommandRunner>(workspace: &Workspace, runner: &R) -> Result<Lock> {
    let nixpkgs = resolve_ref(&workspace.nixpkgs_ref, runner)
        .with_context(|| format!("resolving nixpkgs ref `{}`", workspace.nixpkgs_ref))?;

    let mut overlays = Vec::new();
    for overlay in &workspace.overlays {
        let locked = resolve_ref(&overlay.source, runner)
            .with_context(|| format!("resolving overlay source `{}`", overlay.source))?;
        overlays.push(locked);
    }

    Ok(Lock {
        version: Lock::CURRENT_VERSION,
        nixpkgs,
        overlays,
    })
}

/// Resolve a single flake ref to a locked input via `nix flake metadata --json`.
fn resolve_ref<R: CommandRunner>(flake_ref: &str, runner: &R) -> Result<LockedInput> {
    let out = crate::nix::run_captured(runner, &["flake", "metadata", "--json", flake_ref])
        .with_context(|| "failed to invoke `nix flake metadata`")?;
    if !out.success {
        bail!(
            "`nix flake metadata {flake_ref}` failed:\n{}",
            out.stderr.trim()
        );
    }
    parse_metadata(flake_ref, &out.stdout)
}

/// Parse the JSON emitted by `nix flake metadata --json` into a [`LockedInput`].
fn parse_metadata(original_ref: &str, json: &str) -> Result<LockedInput> {
    let meta: serde_json::Value =
        serde_json::from_str(json).context("failed to parse `nix flake metadata` JSON output")?;

    // The immutable, resolved locked ref string (preferred).
    let locked_url = meta
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let locked = meta.get("locked");
    let rev = locked
        .and_then(|l| l.get("rev"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let nar_hash = locked
        .and_then(|l| l.get("narHash"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let last_modified = locked
        .and_then(|l| l.get("lastModified"))
        .and_then(|v| v.as_i64());

    // Prefer the resolved `url`; otherwise synthesize a pinned ref from the
    // original ref + rev when we can (github-style refs).
    let locked_ref = locked_url
        .or_else(|| rev.as_ref().map(|r| pin_ref_with_rev(original_ref, r)))
        .unwrap_or_else(|| original_ref.to_string());

    Ok(LockedInput {
        original_ref: original_ref.to_string(),
        locked_ref,
        rev,
        nar_hash,
        last_modified,
    })
}

/// Replace the mutable component of a flake ref with an explicit rev.
/// For `github:owner/repo` or `github:owner/repo/branch`, produce
/// `github:owner/repo/<rev>`. For other schemes, fall back to appending.
fn pin_ref_with_rev(original_ref: &str, rev: &str) -> String {
    if let Some(rest) = original_ref.strip_prefix("github:") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 2 {
            return format!("github:{}/{}/{}", parts[0], parts[1], rev);
        }
    }
    // Generic fallback: attach the rev as a query parameter.
    if original_ref.contains('?') {
        format!("{original_ref}&rev={rev}")
    } else {
        format!("{original_ref}?rev={rev}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::mock::{fail, ok, MockRunner};
    use crate::workspace::{Overlay, Workspace};
    use std::path::PathBuf;

    fn sample_metadata_json(rev: &str) -> String {
        format!(
            r#"{{
              "description": "nixpkgs",
              "lastModified": 1700000000,
              "locked": {{
                "lastModified": 1700000000,
                "narHash": "sha256-AAAABBBBCCCCDDDD=",
                "owner": "NixOS",
                "repo": "nixpkgs",
                "rev": "{rev}",
                "type": "github"
              }},
              "url": "github:NixOS/nixpkgs/{rev}"
            }}"#
        )
    }

    fn ws(root: PathBuf, overlays: Vec<Overlay>) -> Workspace {
        Workspace {
            name: "test".into(),
            root,
            nixpkgs_ref: "github:NixOS/nixpkgs/nixpkgs-unstable".into(),
            overlays,
            packages: vec![],
        }
    }

    #[test]
    fn parses_metadata_into_locked_input() {
        let rev = "abc123def456abc123def456abc123def456abcd";
        let li = parse_metadata("github:NixOS/nixpkgs/nixpkgs-unstable", &sample_metadata_json(rev))
            .unwrap();
        assert_eq!(li.rev.as_deref(), Some(rev));
        assert_eq!(li.nar_hash.as_deref(), Some("sha256-AAAABBBBCCCCDDDD="));
        assert_eq!(li.locked_ref, format!("github:NixOS/nixpkgs/{rev}"));
        assert_eq!(li.original_ref, "github:NixOS/nixpkgs/nixpkgs-unstable");
        assert_eq!(li.last_modified, Some(1700000000));
    }

    #[test]
    fn pins_github_ref_with_rev_when_url_missing() {
        // JSON with a locked rev but no top-level url.
        let json = r#"{ "locked": { "rev": "deadbeef", "narHash": "sha256-x=" } }"#;
        let li = parse_metadata("github:NixOS/nixpkgs/nixpkgs-unstable", json).unwrap();
        assert_eq!(li.locked_ref, "github:NixOS/nixpkgs/deadbeef");
    }

    #[test]
    fn resolve_uses_nix_flake_metadata() {
        let rev = "1111111111111111111111111111111111111111";
        let runner = MockRunner::new().with_response(
            "nix --extra-experimental-features nix-command flakes flake metadata --json github:NixOS/nixpkgs/nixpkgs-unstable",
            ok(&sample_metadata_json(rev)),
        );
        let w = ws(PathBuf::from("/tmp/does-not-matter"), vec![]);
        let lock = resolve(&w, &runner).unwrap();
        assert_eq!(lock.version, Lock::CURRENT_VERSION);
        assert_eq!(lock.nixpkgs.rev.as_deref(), Some(rev));
        assert!(lock.overlays.is_empty());
    }

    #[test]
    fn resolve_includes_overlays_in_order() {
        let runner = MockRunner::new()
            .with_response(
                "nix --extra-experimental-features nix-command flakes flake metadata --json github:NixOS/nixpkgs/nixpkgs-unstable",
                ok(&sample_metadata_json("aaaa")),
            )
            .with_response(
                "nix --extra-experimental-features nix-command flakes flake metadata --json github:acme/overlay",
                ok(&sample_metadata_json("bbbb")),
            );
        let w = ws(
            PathBuf::from("/tmp/does-not-matter"),
            vec![Overlay {
                source: "github:acme/overlay".into(),
            }],
        );
        let lock = resolve(&w, &runner).unwrap();
        assert_eq!(lock.overlays.len(), 1);
        assert_eq!(lock.overlays[0].original_ref, "github:acme/overlay");
        assert_eq!(lock.overlays[0].rev.as_deref(), Some("bbbb"));
    }

    #[test]
    fn resolve_fails_when_nix_fails() {
        let runner = MockRunner::new().with_response(
            "nix --extra-experimental-features nix-command flakes flake metadata --json github:NixOS/nixpkgs/nixpkgs-unstable",
            fail(1, "could not fetch"),
        );
        let w = ws(PathBuf::from("/tmp/does-not-matter"), vec![]);
        let err = resolve(&w, &runner).unwrap_err();
        assert!(err.to_string().contains("resolving nixpkgs ref"), "{err}");
    }

    #[test]
    fn load_save_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = Lock {
            version: Lock::CURRENT_VERSION,
            nixpkgs: LockedInput {
                original_ref: "github:NixOS/nixpkgs/nixpkgs-unstable".into(),
                locked_ref: "github:NixOS/nixpkgs/abc".into(),
                rev: Some("abc".into()),
                nar_hash: Some("sha256-x=".into()),
                last_modified: Some(1),
            },
            overlays: vec![],
        };
        lock.save(tmp.path()).unwrap();
        let loaded = Lock::load(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded, lock);
    }

    #[test]
    fn load_returns_none_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Lock::load(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn ensure_lock_resolves_when_missing_then_reuses() {
        let rev = "2222222222222222222222222222222222222222";
        let runner = MockRunner::new().with_response(
            "nix --extra-experimental-features nix-command flakes flake metadata --json github:NixOS/nixpkgs/nixpkgs-unstable",
            ok(&sample_metadata_json(rev)),
        );
        let tmp = tempfile::tempdir().unwrap();
        let w = ws(tmp.path().to_path_buf(), vec![]);

        // First call: no lock -> resolve + write.
        let first = ensure_lock(&w, &runner).unwrap();
        assert_eq!(first.nixpkgs.rev.as_deref(), Some(rev));
        assert!(Lock::path(tmp.path()).is_file());
        let calls_after_first = runner.calls.borrow().len();

        // Second call: lock present -> no further nix invocation.
        let second = ensure_lock(&w, &runner).unwrap();
        assert_eq!(second, first);
        assert_eq!(
            runner.calls.borrow().len(),
            calls_after_first,
            "ensure_lock should not re-run nix when a lock exists"
        );
    }

    #[test]
    fn update_overwrites_existing_lock() {
        let runner = MockRunner::new()
            .with_response(
                "nix --extra-experimental-features nix-command flakes flake metadata --json github:NixOS/nixpkgs/nixpkgs-unstable",
                ok(&sample_metadata_json("newrev")),
            );
        let tmp = tempfile::tempdir().unwrap();
        // Pre-existing lock with a different rev.
        let old = Lock {
            version: Lock::CURRENT_VERSION,
            nixpkgs: LockedInput {
                original_ref: "github:NixOS/nixpkgs/nixpkgs-unstable".into(),
                locked_ref: "github:NixOS/nixpkgs/oldrev".into(),
                rev: Some("oldrev".into()),
                nar_hash: Some("sha256-old=".into()),
                last_modified: Some(1),
            },
            overlays: vec![],
        };
        old.save(tmp.path()).unwrap();

        let w = ws(tmp.path().to_path_buf(), vec![]);
        let updated = update(&w, &runner).unwrap();
        assert_eq!(updated.nixpkgs.rev.as_deref(), Some("newrev"));
        let reloaded = Lock::load(tmp.path()).unwrap().unwrap();
        assert_eq!(reloaded.nixpkgs.rev.as_deref(), Some("newrev"));
    }

    #[test]
    fn load_rejects_unsupported_version() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            Lock::path(tmp.path()),
            r#"version = 999
[nixpkgs]
original_ref = "x"
locked_ref = "x"
"#,
        )
        .unwrap();
        let err = Lock::load(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("unsupported version"), "{err}");
    }

    #[test]
    fn load_rejects_corrupt_lock() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(Lock::path(tmp.path()), "not = = valid toml").unwrap();
        let err = Lock::load(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("failed to parse"), "{err}");
    }
}
