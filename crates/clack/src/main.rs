//! clack — mechanical keyboard sounds for your whole machine.
//!
//! Subcommands are thin IPC clients or local file operations.
//! The daemon itself is in `daemon.rs` (hidden subcommand `clack daemon`).

use clap::Parser;

mod cli;
mod daemon;

use cli::Cli;

fn main() {
    let cli = Cli::parse();
    if let Err(e) = cli.run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
