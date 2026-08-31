//! Centralized `nix` invocation helpers.
//!
//! All of Granit's own `nix` calls go through here so that:
//! - the required experimental features (`nix-command flakes`) are always
//!   enabled for Granit's invocations (doctor separately *reports* on the
//!   user's global config, but Granit itself does not depend on it), and
//! - the `nix` binary is located even when the Nix profile is not on `PATH`.

use crate::runner::{CommandOutput, CommandRunner};

/// The experimental features Granit's flake-based invocations require.
pub const EXPERIMENTAL_FEATURES: &str = "nix-command flakes";

/// Build the full argument vector for a `nix` subcommand, prepending the
/// `--extra-experimental-features` flag.
pub fn nix_args<'a>(subcommand_args: &[&'a str]) -> Vec<String> {
    let mut args = vec![
        "--extra-experimental-features".to_string(),
        EXPERIMENTAL_FEATURES.to_string(),
    ];
    for a in subcommand_args {
        args.push((*a).to_string());
    }
    args
}

/// Run `nix` with the given subcommand args, capturing output. Experimental
/// features are always enabled for the invocation.
pub fn run_captured<R: CommandRunner>(
    runner: &R,
    subcommand_args: &[&str],
) -> std::io::Result<CommandOutput> {
    let owned = nix_args(subcommand_args);
    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    runner.run_captured("nix", &refs)
}

/// Run `nix` with the given subcommand args, streaming output to the terminal.
pub fn run_streamed<R: CommandRunner>(
    runner: &R,
    subcommand_args: &[&str],
    cwd: Option<&std::path::Path>,
) -> std::io::Result<CommandOutput> {
    let owned = nix_args(subcommand_args);
    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    runner.run_streamed("nix", &refs, cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_prepends_experimental_features() {
        let args = nix_args(&["flake", "metadata", "--json", "github:x/y"]);
        assert_eq!(args[0], "--extra-experimental-features");
        assert_eq!(args[1], "nix-command flakes");
        assert_eq!(&args[2..], &["flake", "metadata", "--json", "github:x/y"]);
    }
}
