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
mod runner;
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
        Command::Graph => {
            let ws = workspace::discover_and_load(std::path::Path::new("."))?;
            let g = graph::build(&ws)?;
            print!("{}", graph::render(&ws, &g));
            Ok(())
        }
        Command::Init => init::init_command(),
        Command::Build { package, emit_only } => {
            if emit_only {
                let ws = workspace::discover_and_load(std::path::Path::new("."))?;
                let g = graph::build(&ws)?;
                let lock = lock::ensure_lock(&ws, &runner)?;
                let nix = nixgen::generate(&ws, &g, &lock, "build")?;
                print!("{nix}");
                return Ok(());
            }
            build::build_command(&runner, package.as_deref())
        }
        Command::Test { package } => {
            build::run_named_command(&runner, package.as_deref(), "test")
        }
        Command::Run { command, package } => {
            build::run_named_command(&runner, package.as_deref(), &command)
        }
        Command::Update => {
            let ws = workspace::discover_and_load(std::path::Path::new("."))?;
            let lock = lock::update(&ws, &runner)?;
            println!(
                "Updated {} — nixpkgs pinned to {}",
                lock::LOCK_FILE,
                lock.nixpkgs.locked_ref
            );
            for (overlay, locked) in ws.overlays.iter().zip(lock.overlays.iter()) {
                println!("  overlay {} -> {}", overlay.source, locked.locked_ref);
            }
            Ok(())
        }
    }
}
