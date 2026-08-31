//! Command-line interface definition.

use clap::{Parser, Subcommand};

/// Granit: a simple monorepo build tool that wraps Nix flakes.
#[derive(Debug, Parser)]
#[command(name = "granit", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scaffold a new granit workspace in the current directory.
    Init,

    /// Build a package (and its dependencies) via Nix.
    Build {
        /// Package to build. Defaults to the package containing the current
        /// directory, or all members if run from the workspace root.
        package: Option<String>,

        /// Generate and print the flake.nix without invoking nix build.
        #[arg(long)]
        emit_only: bool,
    },

    /// Run a package's `test` command.
    Test {
        /// Package to test. Defaults to the package containing the current dir.
        package: Option<String>,
    },

    /// Run a named custom command defined in a package's [commands] table.
    Run {
        /// Name of the command to run (e.g. `build`, `lint`).
        command: String,

        /// Package to run in. Defaults to the package containing the current dir.
        package: Option<String>,
    },

    /// Print the dependency graph and resolved build order.
    Graph,

    /// Search the workspace's pinned nixpkgs for build-time tools.
    ///
    /// Prints matching nixpkgs attribute names (usable in a package's `tools`),
    /// their versions, and descriptions.
    Search {
        /// Search term, e.g. `node`, `python`, `gcc`.
        query: String,
    },

    /// Verify the environment (nix installed, flakes enabled, ...).
    Doctor,

    /// Re-resolve pinned inputs and rewrite granit.lock.
    Update,
}
