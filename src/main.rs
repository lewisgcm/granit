//! Granit CLI entry point.

mod build;
mod cli;
mod doctor;
mod graph;
mod init;
mod lock;
mod model;
mod nix;
mod nixgen;
mod nixwriter;
mod runner;
mod search;
mod source;
mod workspace;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};
use runner::SystemRunner;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let runner = SystemRunner::new();

    match cli.command {
        Command::Doctor => {
            let report = doctor::diagnose(&runner);
            print!("{}", doctor::render(&report));
            if !report.is_healthy() {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Init => init::init_command(),
        Command::Graph => build::graph_command(),
        Command::Search { query } => search::search_command(&runner, &query),
        Command::Build { package, emit_only } => {
            if emit_only {
                build::emit_command(&runner)
            } else {
                build::build_command(&runner, package.as_deref())
            }
        }
        Command::Test { package } => build::run_named_command(&runner, package.as_deref(), "test"),
        Command::Run { command, package } => {
            build::run_named_command(&runner, package.as_deref(), &command)
        }
        Command::Update => build::update_command(&runner),
    }
}
