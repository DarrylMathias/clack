//! Top-level CLI definition (§3.3–3.4) — stub for P0.

use clap::{Parser, Subcommand};

const HELP_TEMPLATE: &str = "\
clack {version} — mechanical keyboard sounds for your whole machine

USAGE
  clack [COMMAND]

{subcommands}

EXAMPLES
  clack use ibm-model-m
  clack volume 60
  clack pack add ~/Downloads/my-mechvibes-pack.zip

FILES
  config   ~/.config/clack/config.toml
  packs    ~/.local/share/clack/packs/
  logs     ~/.local/state/clack/clack.log

PRIVACY
  clack reads key codes only to pick a sound. Nothing is logged, stored, or sent.

Docs & issues: https://github.com/DarrylMathias/clack
";

#[derive(Debug, Parser)]
#[command(
    name = "clack",
    version,
    about = "clack — mechanical keyboard sounds for your whole machine",
    long_about = None,
    help_template = HELP_TEMPLATE,
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
    #[command(next_help_heading = "EVERYDAY")]
    /// Mute/unmute (or double-tap Ctrl anywhere)
    Toggle,
    /// Switch keyboard sound; downloads it if needed (interactive studio if no name given)
    Use {
        /// Pack name (omit to pick interactively in TUI)
        pack: Option<String>,
    },
    /// Show all keyboard sounds in interactive studio
    List {
        /// Output plain text instead of interactive studio
        #[arg(long)]
        raw: bool,
    },
    /// Get or set the volume (0–100)
    Volume {
        /// New volume (omit to print current)
        value: Option<u8>,
    },
    /// Show what clack is doing
    Status,

    // SERVICE
    #[command(next_help_heading = "SERVICE")]
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
    #[command(next_help_heading = "PACKS")]
    /// Install or remove packs
    Pack {
        #[command(subcommand)]
        action: PackCommands,
    },

    // MAINTENANCE
    #[command(next_help_heading = "MAINTENANCE")]
    /// Run onboarding again
    Setup {
        #[arg(long, help = "Redo everything from scratch")]
        reset: bool,
        #[arg(long)]
        permissions_only: bool,
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
    Daemon {
        #[arg(long, visible_alias = "output-sink")]
        sink: Option<String>,
    },
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
                if !clack_core::config::config_path().exists() {
                    return crate::setup::run_setup(false);
                }
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

            Some(Commands::Daemon { sink }) => {
                crate::daemon::install_signal_handler();
                crate::daemon::run(sink)
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

            Some(Commands::Use { pack: Some(p) }) => {
                apply_use_pack(&p)?;
                Ok(())
            }

            Some(Commands::Use { pack: None }) => {
                if let Some(pack) = crate::setup::run_picker()? {
                    apply_use_pack(&pack)?;
                }
                Ok(())
            }

            Some(Commands::List { raw }) => {
                use std::io::IsTerminal;
                if !raw && std::io::stdout().is_terminal() {
                    if let Some(pack) = crate::setup::run_picker()? {
                        apply_use_pack(&pack)?;
                    }
                    return Ok(());
                }

                // Determine active pack
                let active_pack = match crate::ipc_client::send(&clack_core::ipc::Command::Status) {
                    Ok(resp) => resp.status.and_then(|s| s.active_pack).unwrap_or_else(|| {
                        clack_core::config::Config::load()
                            .unwrap_or_default()
                            .default_pack
                    }),
                    Err(_) => {
                        clack_core::config::Config::load()
                            .unwrap_or_default()
                            .default_pack
                    }
                };

                // Find local packs
                let mut local_packs = std::collections::HashMap::new();
                if let Ok(entries) = std::fs::read_dir(clack_core::config::packs_dir()) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            if let Ok(manifest) = clack_core::pack::Manifest::load(&path) {
                                local_packs.insert(manifest.id.clone(), manifest);
                            }
                        }
                    }
                }

                // Fetch catalog
                let catalog_entries = clack_core::catalog::fetch_catalog()
                    .map(|c| c.packs)
                    .unwrap_or_default();

                // Build a combined list
                let mut all_ids = std::collections::HashSet::new();
                for id in local_packs.keys() {
                    all_ids.insert(id.clone());
                }
                for entry in &catalog_entries {
                    all_ids.insert(entry.id.clone());
                }

                let mut sorted_ids: Vec<_> = all_ids.into_iter().collect();
                sorted_ids.sort();

                println!("{:-<50}", "");
                println!("{:<2} {:<25} {:<15} LICENSE", "", "ID", "FAMILY");
                println!("{:-<50}", "");

                for id in sorted_ids {
                    let is_active = id == active_pack;
                    let is_installed = local_packs.contains_key(&id);

                    let marker = if is_active {
                        "●"
                    } else if is_installed {
                        "○"
                    } else {
                        " "
                    };

                    let (_name, family, license, is_local_only) =
                        if let Some(cat) = catalog_entries.iter().find(|c| c.id == id) {
                            (
                                cat.name.to_string(),
                                cat.family.to_string(),
                                cat.license.to_string(),
                                false,
                            )
                        } else if let Some(loc) = local_packs.get(&id) {
                            (
                                loc.name.to_string(),
                                loc.family.to_string(),
                                loc.license.to_string(),
                                true,
                            )
                        } else {
                            continue;
                        };

                    let lic_str = if is_local_only {
                        "local"
                    } else if license == "MIT" || license == "CC0-1.0" {
                        ""
                    } else {
                        &license
                    };

                    println!("{marker}  {:<25} {:<15} {}", id, family, lic_str);
                }
                Ok(())
            }

            Some(Commands::Start) => {
                crate::service::install_and_start()?;
                Ok(())
            }

            Some(Commands::Stop) => {
                crate::service::stop()?;
                Ok(())
            }

            Some(Commands::Restart) => {
                crate::service::restart()?;
                Ok(())
            }

            Some(Commands::Pack { action }) => {
                match action {
                    PackCommands::Add { source } => {
                        let is_url_or_id = source.starts_with("http")
                            || !source.contains(std::path::MAIN_SEPARATOR);

                        let (path_to_add, catalog_entry) = if is_url_or_id {
                            // Find in catalog
                            let cat = clack_core::catalog::fetch_catalog()?;
                            let entry = cat
                                .packs
                                .iter()
                                .find(|p| p.id == source || p.url == source)
                                .cloned();
                            if let Some(ref entry) = entry {
                                println!("downloading {}...", entry.id);
                                clack_core::catalog::install_pack(entry)?;
                                (
                                    clack_core::config::packs_dir().join(&entry.id),
                                    Some(entry.clone()),
                                )
                            } else {
                                (std::path::PathBuf::from(&source), None)
                            }
                        } else {
                            (
                                std::fs::canonicalize(&source).map_err(|e| {
                                    anyhow::anyhow!("File not found '{}': {}", source, e)
                                })?,
                                None,
                            )
                        };

                        if let Some(entry) = catalog_entry {
                            let manifest = clack_core::pack::Manifest::load(&path_to_add)?;
                            let resp = send_ok(&Command::Use {
                                pack: entry.id.clone(),
                            });
                            match resp {
                                Ok(_) => {
                                    println!(
                                        "Pack installed and active: {} ({})",
                                        manifest.name, entry.id
                                    );
                                }
                                Err(_) => {
                                    if let Ok(mut cfg) = clack_core::config::Config::load() {
                                        cfg.default_pack = entry.id.clone();
                                        let _ = cfg.save();
                                    }
                                    println!(
                                        "Pack installed: {} ({})",
                                        manifest.name,
                                        path_to_add.display()
                                    );
                                }
                            }
                        } else {
                            let resp = send_ok(&Command::AddPack {
                                path: path_to_add.to_string_lossy().into_owned(),
                            });
                            match resp {
                                Ok(r) => {
                                    if let Some(s) = r.status {
                                        if let Some(pack) = s.active_pack {
                                            println!("Pack imported and active: {}", pack);
                                        }
                                    }
                                }
                                Err(_) => {
                                    let filename = path_to_add
                                        .file_name()
                                        .map(|s| s.to_string_lossy().to_string())
                                        .unwrap_or_else(|| "imported-pack".to_string());
                                    let slug = filename
                                        .trim_end_matches(".tar.gz")
                                        .trim_end_matches(".tgz")
                                        .trim_end_matches(".zip")
                                        .to_lowercase()
                                        .chars()
                                        .map(|c| if c.is_alphanumeric() { c } else { '-' })
                                        .collect::<String>()
                                        .replace("--", "-")
                                        .trim_matches('-')
                                        .to_string();
                                    let dest = clack_core::pack::pack_dir(&slug);
                                    let manifest =
                                        clack_core::pack::import_pack(&path_to_add, &dest)?;
                                    if let Ok(mut cfg) = clack_core::config::Config::load() {
                                        cfg.default_pack = manifest.id.clone();
                                        let _ = cfg.save();
                                    }
                                    println!(
                                        "Pack imported: {} ({})",
                                        manifest.name,
                                        dest.display()
                                    );
                                }
                            }
                        }
                        Ok(())
                    }
                    PackCommands::Remove { id } => {
                        let active_pack =
                            match crate::ipc_client::send(&clack_core::ipc::Command::Status) {
                                Ok(resp) => {
                                    resp.status.and_then(|s| s.active_pack).unwrap_or_default()
                                }
                                Err(_) => String::new(),
                            };
                        if active_pack == *id {
                            anyhow::bail!(
                                "Cannot remove active pack. Switch to another pack first."
                            );
                        }
                        let p = clack_core::config::packs_dir().join(&id);
                        if !p.exists() {
                            anyhow::bail!("Pack '{}' is not installed locally", id);
                        }
                        std::fs::remove_dir_all(&p)?;
                        println!("Removed pack '{}'", id);
                        Ok(())
                    }
                }
            }

            Some(Commands::Setup {
                reset,
                permissions_only,
            }) => {
                if permissions_only {
                    crate::perms::setup_permissions()?;
                } else {
                    crate::setup::run_setup(reset)?;
                }
                Ok(())
            }

            Some(Commands::Doctor) => {
                crate::doctor::run_doctor()?;
                Ok(())
            }

            Some(Commands::Update) => {
                crate::update::run_update()?;
                Ok(())
            }

            Some(Commands::Uninstall) => {
                crate::uninstall::run_uninstall()?;
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

fn apply_use_pack(pack: &str) -> anyhow::Result<()> {
    let p = clack_core::config::packs_dir().join(pack);
    if !p.exists() || clack_core::pack::Manifest::load(&p).is_err() {
        if let Ok(cat) = clack_core::catalog::fetch_catalog() {
            if let Some(entry) = cat.packs.iter().find(|e| e.id == pack) {
                println!("downloading {pack}...");
                clack_core::catalog::install_pack(entry)?;
            }
        }
    }
    if let Ok(mut cfg) = clack_core::config::Config::load() {
        cfg.default_pack = pack.to_string();
        let _ = cfg.save();
    }
    let _ = crate::ipc_client::send(&clack_core::ipc::Command::Use {
        pack: pack.to_string(),
    });
    println!("switched to {pack}");
    Ok(())
}
