//! Dependency graph over workspace packages.
//!
//! Edges are derived from each package's `dependencies` (`package:label`),
//! covering both artifact and source dependencies (both require the producer
//! to be available before the consumer builds). Provides reference validation
//! (package + label must exist), cycle detection, and a topological build
//! order.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Result};

use crate::workspace::Workspace;

/// A validated dependency graph with a resolved build order.
#[derive(Debug, Clone)]
pub struct Graph {
    /// Package names in dependency order (dependencies before dependents).
    pub order: Vec<String>,
    /// Adjacency: package -> the packages it directly depends on (by name),
    /// sorted and deduplicated. Retained for graph queries and future
    /// per-package orchestration (see backlog.md).
    #[allow(dead_code)]
    pub edges: BTreeMap<String, Vec<String>>,
}

impl Graph {
    /// The set of package names that `package` transitively depends on, plus
    /// `package` itself, in build order. Retained for future per-package
    /// orchestration (see backlog.md); the current single-flake build lets Nix
    /// resolve transitive ordering.
    #[allow(dead_code)]
    pub fn build_order_for(&self, package: &str) -> Vec<String> {
        let mut needed: BTreeSet<String> = BTreeSet::new();
        let mut stack = vec![package.to_string()];
        while let Some(p) = stack.pop() {
            if needed.insert(p.clone()) {
                if let Some(deps) = self.edges.get(&p) {
                    for d in deps {
                        stack.push(d.clone());
                    }
                }
            }
        }
        // Preserve the global topological order, filtered to the needed set.
        self.order
            .iter()
            .filter(|name| needed.contains(*name))
            .cloned()
            .collect()
    }
}

/// Build and validate the dependency graph for a workspace.
pub fn build(workspace: &Workspace) -> Result<Graph> {
    // Index packages by name for validation.
    let by_name: BTreeMap<&str, &crate::workspace::Package> = workspace
        .packages
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();

    // Build adjacency and validate references.
    let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pkg in &workspace.packages {
        let mut deps: BTreeSet<String> = BTreeSet::new();
        for dep in &pkg.dependencies {
            // Referenced package must exist.
            let Some(target) = by_name.get(dep.package.as_str()) else {
                bail!(
                    "package `{}` depends on `{}:{}`, but no package named `{}` exists in the workspace",
                    pkg.name,
                    dep.package,
                    dep.label,
                    dep.package
                );
            };
            // Referenced label must be a declared output of the target.
            if !target.outputs.contains_key(&dep.label) {
                let available: Vec<&str> = target.outputs.keys().map(|s| s.as_str()).collect();
                let available = if available.is_empty() {
                    "(none declared)".to_string()
                } else {
                    available.join(", ")
                };
                bail!(
                    "package `{}` depends on `{}:{}`, but package `{}` declares no output labeled `{}` (available: {})",
                    pkg.name,
                    dep.package,
                    dep.label,
                    dep.package,
                    dep.label,
                    available
                );
            }
            // Self-dependency is a trivial cycle.
            if dep.package == pkg.name {
                bail!(
                    "package `{}` depends on itself (`{}:{}`)",
                    pkg.name,
                    dep.package,
                    dep.label
                );
            }
            deps.insert(dep.package.clone());
        }
        edges.insert(pkg.name.clone(), deps.into_iter().collect());
    }

    let order = topo_sort(&edges)?;
    Ok(Graph { order, edges })
}

/// Topologically sort the graph (dependencies before dependents), detecting
/// cycles. `edges[p]` are the nodes `p` depends on.
fn topo_sort(edges: &BTreeMap<String, Vec<String>>) -> Result<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Unvisited,
        InProgress,
        Done,
    }

    let mut state: BTreeMap<&str, Mark> =
        edges.keys().map(|k| (k.as_str(), Mark::Unvisited)).collect();
    let mut order: Vec<String> = Vec::new();

    // Iterative DFS that tracks the active path so we can report the cycle.
    for start in edges.keys() {
        if state[start.as_str()] == Mark::Done {
            continue;
        }
        // (node, index-into-deps)
        let mut stack: Vec<(&str, usize)> = vec![(start.as_str(), 0)];
        let mut path: Vec<&str> = vec![start.as_str()];
        *state.get_mut(start.as_str()).unwrap() = Mark::InProgress;

        while let Some(&(node, idx)) = stack.last() {
            let deps = &edges[node];
            if idx < deps.len() {
                // advance this frame's index
                stack.last_mut().unwrap().1 += 1;
                let next = deps[idx].as_str();
                match state.get(next).copied().unwrap_or(Mark::Done) {
                    Mark::Done => { /* already processed */ }
                    Mark::InProgress => {
                        // Found a cycle: from `next` around the path back to `node`.
                        let cycle = extract_cycle(&path, next);
                        bail!("dependency cycle detected: {}", cycle.join(" -> "));
                    }
                    Mark::Unvisited => {
                        *state.get_mut(next).unwrap() = Mark::InProgress;
                        stack.push((next, 0));
                        path.push(next);
                    }
                }
            } else {
                // done with this node
                *state.get_mut(node).unwrap() = Mark::Done;
                order.push(node.to_string());
                stack.pop();
                path.pop();
            }
        }
    }

    Ok(order)
}

/// Given the active DFS `path` and the node `back` we looped back to, produce a
/// readable cycle description.
fn extract_cycle(path: &[&str], back: &str) -> Vec<String> {
    let start = path.iter().position(|&n| n == back).unwrap_or(0);
    let mut cycle: Vec<String> = path[start..].iter().map(|s| s.to_string()).collect();
    cycle.push(back.to_string());
    cycle
}

/// Render the graph and build order for the `graph` command.
pub fn render(workspace: &Workspace, graph: &Graph) -> String {
    let mut out = format!("Workspace `{}` dependency graph\n", workspace.name);
    out.push_str("================================\n");

    // Show each package and its dependency edges (with labels).
    for pkg in &workspace.packages {
        if pkg.dependencies.is_empty() {
            out.push_str(&format!("{} (no dependencies)\n", pkg.name));
        } else {
            let refs: Vec<String> = pkg
                .dependencies
                .iter()
                .map(|d| format!("{}:{}", d.package, d.label))
                .collect();
            out.push_str(&format!("{} -> {}\n", pkg.name, refs.join(", ")));
        }
    }

    out.push_str("\nBuild order:\n");
    for (i, name) in graph.order.iter().enumerate() {
        out.push_str(&format!("  {}. {}\n", i + 1, name));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{DependencyRef, Package, Workspace};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn pkg(name: &str, deps: &[(&str, &str)], outputs: &[&str]) -> Package {
        Package {
            name: name.to_string(),
            dir: PathBuf::from(name),
            tools: vec![],
            dependencies: deps
                .iter()
                .map(|(p, l)| DependencyRef {
                    package: p.to_string(),
                    label: l.to_string(),
                })
                .collect(),
            exclude: vec![],
            outputs: outputs
                .iter()
                .map(|o| {
                    (
                        o.to_string(),
                        crate::workspace::Output {
                            path: format!("{o}.txt"),
                            is_source: false,
                        },
                    )
                })
                .collect(),
            commands: BTreeMap::new(),
        }
    }

    fn ws(packages: Vec<Package>) -> Workspace {
        Workspace {
            name: "test".into(),
            root: PathBuf::from("/tmp/test"),
            nixpkgs_ref: "github:NixOS/nixpkgs/nixpkgs-unstable".into(),
            overlays: vec![],
            packages,
        }
    }

    fn pos(order: &[String], name: &str) -> usize {
        order.iter().position(|n| n == name).unwrap()
    }

    #[test]
    fn linear_chain_orders_dependencies_first() {
        // c -> b -> a
        let w = ws(vec![
            pkg("a", &[], &["out"]),
            pkg("b", &[("a", "out")], &["out"]),
            pkg("c", &[("b", "out")], &["out"]),
        ]);
        let g = build(&w).unwrap();
        assert!(pos(&g.order, "a") < pos(&g.order, "b"));
        assert!(pos(&g.order, "b") < pos(&g.order, "c"));
    }

    #[test]
    fn diamond_orders_correctly() {
        // d depends on b and c; b and c depend on a.
        let w = ws(vec![
            pkg("a", &[], &["out"]),
            pkg("b", &[("a", "out")], &["out"]),
            pkg("c", &[("a", "out")], &["out"]),
            pkg("d", &[("b", "out"), ("c", "out")], &["out"]),
        ]);
        let g = build(&w).unwrap();
        assert!(pos(&g.order, "a") < pos(&g.order, "b"));
        assert!(pos(&g.order, "a") < pos(&g.order, "c"));
        assert!(pos(&g.order, "b") < pos(&g.order, "d"));
        assert!(pos(&g.order, "c") < pos(&g.order, "d"));
    }

    #[test]
    fn self_cycle_is_rejected() {
        let w = ws(vec![pkg("a", &[("a", "out")], &["out"])]);
        let err = build(&w).unwrap_err();
        assert!(err.to_string().contains("depends on itself"));
    }

    #[test]
    fn multi_node_cycle_is_rejected() {
        // a -> b -> c -> a
        let w = ws(vec![
            pkg("a", &[("b", "out")], &["out"]),
            pkg("b", &[("c", "out")], &["out"]),
            pkg("c", &[("a", "out")], &["out"]),
        ]);
        let err = build(&w).unwrap_err();
        assert!(err.to_string().contains("cycle detected"), "{err}");
    }

    #[test]
    fn unknown_package_reference_is_rejected() {
        let w = ws(vec![pkg("a", &[("ghost", "out")], &["out"])]);
        let err = build(&w).unwrap_err();
        assert!(err.to_string().contains("no package named `ghost`"), "{err}");
    }

    #[test]
    fn undeclared_label_reference_is_rejected() {
        // b depends on a:missing but a only declares `out`.
        let w = ws(vec![
            pkg("a", &[], &["out"]),
            pkg("b", &[("a", "missing")], &["out"]),
        ]);
        let err = build(&w).unwrap_err();
        assert!(
            err.to_string().contains("no output labeled `missing`"),
            "{err}"
        );
    }

    #[test]
    fn build_order_for_subset() {
        // c -> b -> a ; d independent
        let w = ws(vec![
            pkg("a", &[], &["out"]),
            pkg("b", &[("a", "out")], &["out"]),
            pkg("c", &[("b", "out")], &["out"]),
            pkg("d", &[], &["out"]),
        ]);
        let g = build(&w).unwrap();
        let order = g.build_order_for("c");
        assert_eq!(order, vec!["a", "b", "c"]);
        assert!(!order.contains(&"d".to_string()));
    }
}
