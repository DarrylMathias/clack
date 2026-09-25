//! Top-level CLI definition (§3.3–3.4) — stub for P0.

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "clack",
    version,
    about = "clack — mechanical keyboard sounds for your whole machine",
    long_about = None,
    help_template = "\
clack {version} — mechanical keyboard sounds for your whole machine

{usage-heading} {usage}

{all-args}

PRIVACY
  clack reads key codes only to pick a sound. Nothing is logged, stored, or sent.

Docs & issues: https://github.com/DarrylMathias/clack
",
    disable_help_subcommand = true,
    propagate_version = true,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    // EVERYDAY
    /// Mute/unmute (or double-tap Ctrl anywhere)
    Toggle,
    /// Switch keyboard sound; downloads it if needed
    Use {
        /// Pack name (e.g. `ibm-model-m`)
        pack: String,
    },
    /// Show all keyboard sounds
    List,
    /// Get or set the volume (0–100)
    Volume {
        /// New volume (omit to print current)
        value: Option<u8>,
    },
    /// Show what clack is doing
    Status,

    // SERVICE
    /// Start the background service
    Start,
    /// Stop the background service
    Stop,
    /// Restart the background service
    Restart,
    /// Unmute
    On,
    /// Mute
    Off,

    // PACKS
    /// Install or remove packs
    Pack {
        #[command(subcommand)]
        action: PackCommands,
    },

    // MAINTENANCE
    /// Run onboarding again
    Setup {
        #[arg(long, help = "Redo everything from scratch")]
        reset: bool,
    },
    /// Find and fix problems
    Doctor,
    /// Update clack to the latest version
    Update,
    /// Remove clack from this machine
    Uninstall,

    // HIDDEN
    /// Run the daemon in the foreground (used by the service manager)
    #[command(hide = true)]
    Daemon,
}

#[derive(Debug, Subcommand)]
pub enum PackCommands {
    /// Install a pack (catalog id, URL, folder, or Mechvibes zip)
    Add {
        /// Catalog id, URL, local path, or Mechvibes zip
        source: String,
    },
    /// Uninstall a pack
    Remove {
        /// Pack id to remove
        id: String,
    },
}

impl Cli {
    pub fn run(self) -> anyhow::Result<()> {
        match self.command {
            None => {
                println!("clack is not set up yet. Run `clack setup` to get started.");
                Ok(())
            }
            Some(Commands::Daemon) => {
                crate::daemon::install_signal_handler();
                crate::daemon::run()
            }
            Some(cmd) => {
                println!("TODO: implement command {cmd:?}");
                Ok(())
            }
        }
    }
}
