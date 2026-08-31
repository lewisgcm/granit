//! Serde data model for `granit.toml` and `package.toml`.
//!
//! These are the raw, on-disk shapes. Higher-level validation and the
//! processed workspace live in [`crate::workspace`].

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The top-level `granit.toml` file at the workspace root.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceFile {
    pub workspace: WorkspaceSection,
    pub nixpkgs: NixpkgsSection,
    /// Additional overlays composed over nixpkgs, in declared order.
    #[serde(default)]
    pub overlays: Vec<OverlaySection>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceSection {
    pub name: String,
    /// Member package directories. Entries may be glob patterns
    /// (e.g. `packages/*`) or explicit paths.
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NixpkgsSection {
    /// A flake reference, e.g. `github:NixOS/nixpkgs/nixpkgs-unstable` or
    /// `github:NixOS/nixpkgs/<commit-sha>` to pin exactly.
    ///
    /// The field is named `ref_` in Rust (since `ref` is a keyword) but maps to
    /// the `ref` key in TOML.
    #[serde(rename = "ref")]
    pub ref_: String,
}

impl NixpkgsSection {
    pub fn reference(&self) -> &str {
        &self.ref_
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OverlaySection {
    /// A flake reference to a source providing an overlay (e.g. a proprietary
    /// package set): `git+https://...`, `github:org/repo`, `path:./local`.
    pub source: String,
}

/// A `package.toml` file inside a member directory.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageFile {
    pub package: PackageSection,
    /// Labeled outputs: label -> filename produced by the build.
    #[serde(default)]
    pub outputs: BTreeMap<String, String>,
    /// Named commands (e.g. `build`, `test`, custom).
    #[serde(default)]
    pub commands: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageSection {
    pub name: String,
    /// Build-time tools by nixpkgs attribute name (resolved against the merged
    /// package set: nixpkgs + overlays + workspace overlay).
    #[serde(default)]
    pub tools: Vec<String>,
    /// Artifact dependencies on other workspace packages, each in the form
    /// `package:label`.
    #[serde(default)]
    pub dependencies: Vec<String>,
}
