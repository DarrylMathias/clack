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
        use crate::ipc_client::{send, send_ok};
        use clack_core::ipc::Command;

        match self.command {
            None => {
                // No setup → run setup; for now just show status or prompt.
                match send(&Command::Status) {
                    Ok(resp) if resp.ok => {
                        if let Some(status) = resp.status {
                            print_status(&status);
                        }
                    }
                    _ => println!("clack is not running. Start it with: clack start"),
                }
                Ok(())
            }

            Some(Commands::Daemon) => {
                crate::daemon::install_signal_handler();
                crate::daemon::run()
            }

            Some(Commands::Status) => {
                let resp = send_ok(&Command::Status)?;
                if let Some(s) = resp.status {
                    print_status(&s);
                }
                Ok(())
            }

            Some(Commands::Toggle) => {
                send_ok(&Command::Toggle)?;
                println!("toggled");
                Ok(())
            }

            Some(Commands::On) => {
                send_ok(&Command::SetMuted { muted: false })?;
                println!("unmuted");
                Ok(())
            }

            Some(Commands::Off) => {
                send_ok(&Command::SetMuted { muted: true })?;
                println!("muted");
                Ok(())
            }

            Some(Commands::Volume { value: Some(v) }) => {
                send_ok(&Command::Volume { value: v })?;
                println!("volume set to {v}");
                Ok(())
            }

            Some(Commands::Volume { value: None }) => {
                let resp = send_ok(&Command::Status)?;
                if let Some(s) = resp.status {
                    println!("volume: {}", s.volume);
                }
                Ok(())
            }

            Some(Commands::Use { pack }) => {
                send_ok(&Command::Use { pack: pack.clone() })?;
                println!("switched to {pack}");
                Ok(())
            }

            Some(Commands::List) => {
                // TODO (P2.3): fetch catalog and render the full list.
                println!("(pack list not yet implemented — coming in P2.3)");
                Ok(())
            }

            Some(Commands::Start) => {
                // Spawn the daemon in the background.
                std::process::Command::new(std::env::current_exe()?)
                    .arg("daemon")
                    .spawn()
                    .map_err(|e| anyhow::anyhow!("failed to start daemon: {e}"))?;
                println!("clack started");
                Ok(())
            }

            Some(Commands::Stop) => {
                send_ok(&Command::Shutdown)?;
                println!("clack stopped");
                Ok(())
            }

            Some(Commands::Restart) => {
                let _ = send_ok(&Command::Shutdown);
                std::thread::sleep(std::time::Duration::from_millis(500));
                std::process::Command::new(std::env::current_exe()?)
                    .arg("daemon")
                    .spawn()
                    .map_err(|e| anyhow::anyhow!("failed to restart daemon: {e}"))?;
                println!("clack restarted");
                Ok(())
            }

            Some(Commands::Pack { action }) => {
                match action {
                    PackCommands::Add { source } => {
                        let path = std::fs::canonicalize(&source)
                            .map_err(|e| anyhow::anyhow!("File not found '{}': {}", source, e))?;
                        let resp = send_ok(&Command::AddPack { path: path.to_string_lossy().into_owned() })?;
                        if let Some(s) = resp.status {
                            if let Some(pack) = s.active_pack {
                                println!("Pack imported and active: {}", pack);
                            }
                        }
                        Ok(())
                    }
                    PackCommands::Remove { id: _ } => {
                        println!("TODO: pack remove — coming in P2.3");
                        Ok(())
                    }
                }
            }

            Some(Commands::Setup { reset }) => {
                println!("TODO: setup (reset={reset}) — coming in P3.3");
                Ok(())
            }

            Some(Commands::Doctor) => {
                println!("TODO: doctor — coming in P4.1");
                Ok(())
            }

            Some(Commands::Update) => {
                println!("TODO: update — coming in P4.2");
                Ok(())
            }

            Some(Commands::Uninstall) => {
                println!("TODO: uninstall — coming in P4.2");
                Ok(())
            }
        }
    }
}

fn print_status(s: &clack_core::ipc::DaemonStatus) {
    let mute_str = if s.muted { "muted" } else { "active" };
    let pack = s.active_pack.as_deref().unwrap_or("none");
    println!(
        "clack  {mute_str}  |  pack: {pack}  |  volume: {}  |  keyboards: {}",
        s.volume,
        s.keyboards_detected.len(),
    );
}
