//! Package search (`granit search <query>`).
//!
//! Searches the workspace's *pinned* nixpkgs (from `granit.lock`) so results
//! match exactly the package set builds resolve against. Prints the nixpkgs
//! attribute name (what you put in a package's `tools`), version, and
//! description.

use anyhow::{bail, Context, Result};

use crate::build;
use crate::lock;
use crate::runner::CommandRunner;
use crate::workspace;

/// A single search hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    /// The nixpkgs attribute name to use in `tools` (e.g. `nodejs_20`).
    pub attr: String,
    pub version: String,
    pub description: String,
}

/// Entry point for `granit search <query>`.
pub fn search_command<R: CommandRunner>(runner: &R, query: &str) -> Result<()> {
    let cwd = std::env::current_dir().context("could not determine current directory")?;
    let ws = workspace::discover_and_load(&cwd)?;
    // Use the pinned nixpkgs so results reflect what builds actually see.
    let lock = lock::ensure_lock(&ws, runner)?;
    let system = build::current_system(runner)?;

    let results = search(runner, &lock.nixpkgs.locked_ref, &system, query)?;

    if results.is_empty() {
        println!("No packages matching `{query}` in {}.", lock.nixpkgs.locked_ref);
        return Ok(());
    }

    println!(
        "{} package(s) matching `{query}` in pinned nixpkgs:\n",
        results.len()
    );
    print_results(&results);
    println!(
        "\nAdd one to a package's tools, e.g.  tools = [\"{}\"]",
        results[0].attr
    );
    Ok(())
}

/// Run `nix search` against `flake_ref` for `query`, scoped to `system`.
fn search<R: CommandRunner>(
    runner: &R,
    flake_ref: &str,
    system: &str,
    query: &str,
) -> Result<Vec<SearchResult>> {
    let out = crate::nix::run_captured(runner, &["search", flake_ref, query, "--json"])
        .context("failed to invoke `nix search`")?;
    if !out.success {
        // `nix search` exits non-zero when there are no matches; treat an empty
        // result specially so we can show a friendly message instead of erroring.
        let stderr = out.stderr.trim();
        if stderr.contains("no results") || out.stdout.trim().is_empty() {
            return Ok(Vec::new());
        }
        bail!("`nix search` failed:\n{stderr}");
    }
    parse_search_results(&out.stdout, system)
}

/// Parse `nix search --json` output into results, keeping only entries for
/// `system` and stripping the `legacyPackages.<system>.` prefix to yield the
/// attribute name usable in `tools`.
pub fn parse_search_results(json: &str, system: &str) -> Result<Vec<SearchResult>> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value =
        serde_json::from_str(json).context("failed to parse `nix search` JSON output")?;
    let obj = match value.as_object() {
        Some(o) => o,
        None => return Ok(Vec::new()),
    };

    // Keys look like: legacyPackages.<system>.<attr...> or packages.<system>.<attr>
    let prefixes = [
        format!("legacyPackages.{system}."),
        format!("packages.{system}."),
    ];

    let mut results = Vec::new();
    for (key, meta) in obj {
        let Some(attr) = prefixes
            .iter()
            .find_map(|p| key.strip_prefix(p.as_str()))
        else {
            continue;
        };
        let version = meta
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let description = meta
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        results.push(SearchResult {
            attr: attr.to_string(),
            version,
            description,
        });
    }

    results.sort_by(|a, b| a.attr.cmp(&b.attr));
    Ok(results)
}

/// Print results as an aligned table: `attr  version  description`.
fn print_results(results: &[SearchResult]) {
    let attr_w = results.iter().map(|r| r.attr.len()).max().unwrap_or(0);
    let ver_w = results.iter().map(|r| r.version.len()).max().unwrap_or(0);
    for r in results {
        let desc = truncate(&r.description, 80);
        if r.description.is_empty() {
            println!("  {:attr_w$}  {:ver_w$}", r.attr, r.version);
        } else {
            println!("  {:attr_w$}  {:ver_w$}  {}", r.attr, r.version, desc);
        }
    }
}

/// Truncate a description to `max` chars, appending an ellipsis if cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "legacyPackages.aarch64-darwin.nodejs_20": {
        "description": "Event-driven I/O framework for the V8 JavaScript engine",
        "pname": "nodejs",
        "version": "20.18.1"
      },
      "legacyPackages.aarch64-darwin.nodejs_18": {
        "description": "Event-driven I/O framework",
        "pname": "nodejs",
        "version": "18.20.5"
      },
      "legacyPackages.x86_64-linux.nodejs_20": {
        "description": "should be filtered out (wrong system)",
        "pname": "nodejs",
        "version": "20.18.1"
      }
    }"#;

    #[test]
    fn parses_and_filters_by_system() {
        let results = parse_search_results(SAMPLE, "aarch64-darwin").unwrap();
        // Only the two aarch64-darwin entries, sorted by attr.
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].attr, "nodejs_18");
        assert_eq!(results[1].attr, "nodejs_20");
        assert_eq!(results[1].version, "20.18.1");
        assert!(results[1].description.contains("V8 JavaScript"));
    }

    #[test]
    fn strips_only_matching_system_prefix() {
        let results = parse_search_results(SAMPLE, "x86_64-linux").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].attr, "nodejs_20");
        assert!(results[0].description.contains("wrong system"));
    }

    #[test]
    fn handles_nested_attr_paths() {
        let json = r#"{
          "legacyPackages.aarch64-darwin.nodePackages.pnpm": {
            "description": "Fast, disk space efficient package manager",
            "pname": "pnpm",
            "version": "9.0.0"
          }
        }"#;
        let results = parse_search_results(json, "aarch64-darwin").unwrap();
        assert_eq!(results.len(), 1);
        // Nested attribute path is preserved as the tools name.
        assert_eq!(results[0].attr, "nodePackages.pnpm");
    }

    #[test]
    fn empty_input_yields_no_results() {
        assert!(parse_search_results("", "aarch64-darwin").unwrap().is_empty());
        assert!(parse_search_results("{}", "aarch64-darwin")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn truncate_adds_ellipsis() {
        assert_eq!(truncate("short", 80), "short");
        let long = "x".repeat(100);
        let t = truncate(&long, 10);
        assert_eq!(t.chars().count(), 10);
        assert!(t.ends_with('…'));
    }
}
