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
    /// Labeled outputs: label -> what the build produces under that label.
    ///
    /// Each value is either a bare string (shorthand for an *artifact* at that
    /// path — the historical form) or a tagged table declaring the output
    /// *kind*: `{ artifact = "path" }` or `{ source = "path" }`. A `source`
    /// output makes the package's tree available to consumers as compile-time
    /// source (mounted repo-relative) rather than as a built artifact file.
    #[serde(default)]
    pub outputs: BTreeMap<String, OutputSpec>,
    /// Named commands (e.g. `build`, `test`, custom). Each value is either a
    /// bare string (the shell script) or a tagged table `{ run = "...",
    /// needs = ["cmd", ...] }` declaring commands to run first (see
    /// [`CommandSpec`]).
    #[serde(default)]
    pub commands: BTreeMap<String, CommandSpec>,
}

/// A command declaration: a shell script plus optional `needs` hooks.
///
/// Deserializes from either:
/// - a bare string `name = "script"` → the script with no hooks (back-compat), or
/// - a tagged table `name = { run = "script", needs = ["other", ...] }`.
///
/// `needs` lists other commands in the *same package* that must run first
/// (in-place, dev-style) before this command. It lets e.g. `build` run a
/// `generate` step first while keeping `generate` independently invocable.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CommandSpec {
    /// Bare string shorthand: the script, with no hooks.
    Script(String),
    /// Tagged table: script plus `needs`.
    Detailed(CommandDetail),
}

/// The explicit, tagged form of a command declaration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct CommandDetail {
    /// The shell script to run.
    pub run: String,
    /// Other command names (same package) to run first, in declared order.
    #[serde(default)]
    pub needs: Vec<String>,
}

impl CommandSpec {
    /// The shell script this command runs.
    pub fn run(&self) -> &str {
        match self {
            CommandSpec::Script(s) => s,
            CommandSpec::Detailed(d) => &d.run,
        }
    }

    /// The command names this command depends on (runs first).
    pub fn needs(&self) -> &[String] {
        match self {
            CommandSpec::Script(_) => &[],
            CommandSpec::Detailed(d) => &d.needs,
        }
    }
}

/// The declared kind and path of a single labeled output.
///
/// Deserializes from either:
/// - a bare string `label = "path"` → an artifact at `path` (back-compat), or
/// - a tagged table `label = { artifact = "path" }` / `{ source = "path" }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum OutputSpec {
    /// Bare string shorthand: an artifact produced at this path.
    Artifact(String),
    /// Tagged table selecting the output kind explicitly.
    Tagged(OutputKind),
}

/// The explicit, tagged form of an output declaration. Exactly one variant key
/// (`artifact` or `source`) is present in the TOML table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputKind {
    /// A built artifact (file or directory) at this path, collected into the
    /// dependency's materialized output and consumed at
    /// `$GRANIT_DEPENDENCIES/<pkg>/<label>`.
    Artifact(String),
    /// The package's source, made available to consumers as compile-time source
    /// (mounted into the consumer's sandbox at the package's repo-relative
    /// location). The path is relative to the package root (usually `"."`).
    Source(String),
}

impl OutputSpec {
    /// The output path (relative to the package root) regardless of kind.
    pub fn path(&self) -> &str {
        match self {
            OutputSpec::Artifact(p) => p,
            OutputSpec::Tagged(OutputKind::Artifact(p)) => p,
            OutputSpec::Tagged(OutputKind::Source(p)) => p,
        }
    }

    /// Whether this output is a source output (vs a built artifact).
    pub fn is_source(&self) -> bool {
        matches!(self, OutputSpec::Tagged(OutputKind::Source(_)))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageSection {
    pub name: String,
    /// Build-time tools by nixpkgs attribute name (resolved against the merged
    /// package set: nixpkgs + overlays + workspace overlay).
    #[serde(default)]
    pub tools: Vec<String>,
    /// Dependencies on other workspace packages, each in the form
    /// `package:label`. The referenced output's *kind* (artifact vs source)
    /// determines how granit mounts it into this package's build.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Gitignore-style patterns (rooted at the package directory) for files to
    /// exclude from the build source. `.git` and `.granit` are always excluded.
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml_str: &str) -> PackageFile {
        toml::from_str(toml_str).unwrap()
    }

    #[test]
    fn bare_string_output_is_artifact_backcompat() {
        let f = parse(
            r#"
[package]
name = "a"

[outputs]
hello = "hello.txt"
"#,
        );
        let out = f.outputs.get("hello").unwrap();
        assert_eq!(out, &OutputSpec::Artifact("hello.txt".into()));
        assert_eq!(out.path(), "hello.txt");
        assert!(!out.is_source());
    }

    #[test]
    fn tagged_artifact_output() {
        let f = parse(
            r#"
[package]
name = "a"

[outputs]
schema = { artifact = "tsp-output/schema/openapi.yaml" }
"#,
        );
        let out = f.outputs.get("schema").unwrap();
        assert!(!out.is_source());
        assert_eq!(out.path(), "tsp-output/schema/openapi.yaml");
    }

    #[test]
    fn tagged_source_output() {
        let f = parse(
            r#"
[package]
name = "common"

[outputs]
src = { source = "." }
"#,
        );
        let out = f.outputs.get("src").unwrap();
        assert!(out.is_source());
        assert_eq!(out.path(), ".");
    }

    #[test]
    fn bare_string_command_has_no_needs() {
        let f = parse(
            r#"
[package]
name = "a"

[commands]
test = "go test ./..."
"#,
        );
        let c = f.commands.get("test").unwrap();
        assert_eq!(c.run(), "go test ./...");
        assert!(c.needs().is_empty());
    }

    #[test]
    fn tagged_command_with_needs() {
        let f = parse(
            r#"
[package]
name = "a"

[commands]
generate = "codegen"
build = { run = "go build ./...", needs = ["generate"] }
"#,
        );
        let b = f.commands.get("build").unwrap();
        assert_eq!(b.run(), "go build ./...");
        assert_eq!(b.needs(), &["generate".to_string()]);
        // bare string still parses alongside.
        assert!(f.commands.get("generate").unwrap().needs().is_empty());
    }
}
