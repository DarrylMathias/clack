use std::{
    io::{stdout, Write},
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, terminal,
};

use clack_core::{
    config::{config_path, packs_dir, Config},
    decoder::load_pack,
    mixer::{Engine, SampleBank},
    pack::pack_dir,
};
use clack_input::{DefaultBackend, InputBackend};

struct PackItem {
    id: String,
    name: String,
    family: String,
    status: String,
}

pub fn run_setup(reset: bool) -> Result<()> {
    if reset {
        println!("Resetting clack configuration...");
        let cfg_file = config_path();
        if cfg_file.exists() {
            let _ = std::fs::remove_file(cfg_file);
        }
    }

    println!("  clack · mechanical keyboard sounds for your whole machine\n");

    // ── Step 1: Keyboard access ──────────────────────────────────────────────
    println!("  [1/4] Keyboard access");
    println!("        clack needs read access to your keyboard to hear keypresses.");
    println!("        It never records, stores, or sends what you type.");

    let access_ok = DefaultBackend::probe()
        .map(|devs| !devs.is_empty())
        .unwrap_or(false);
    if access_ok {
        println!("        → Keyboard access is already configured. ✓\n");
    } else {
        #[cfg(target_os = "linux")]
        {
            print!("        → Installing udev rule (sudo password may be needed once)… ");
            stdout().flush()?;
            match crate::perms::setup_permissions() {
                Ok(_) => println!("✓\n"),
                Err(e) => {
                    println!("\x1b[33m!\x1b[0m ({e})");
                    println!("        You can run without sudo if already in the input group.\n");
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            println!("        → Please grant Input Monitoring permission in System Settings.\n");
        }
    }

    // ── Step 2: Pick keyboard (Interactive TUI) ──────────────────────────────
    let chosen_pack = run_picker()?;

    // Save chosen pack to config
    let mut config = Config::load().unwrap_or_default();
    config.default_pack = chosen_pack.clone();
    config.save().context("saving config with default pack")?;

    // ── Step 3: Start at login ───────────────────────────────────────────────
    println!("  [3/4] Start at login");
    match crate::service::install_and_start() {
        Ok(_) => {
            println!("        → Background service installed and started. ✓\n");
        }
        Err(e) => {
            println!("        → Service install notice: {e:#}\n");
        }
    }

    // ── Step 4: Done ─────────────────────────────────────────────────────────
    println!("  [4/4] Done. clack is running with '{}'.", chosen_pack);
    println!("\n        Double-tap Ctrl to mute/unmute.   Run `clack --help` for commands.\n");

    Ok(())
}

fn run_picker() -> Result<String> {
    println!("  [2/4] Pick your keyboard   (↑/↓ to browse, type in the box to hear it, Enter to choose)\n");

    // Discover installed packs or defaults
    let mut packs: Vec<PackItem> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(packs_dir()) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if let Ok(manifest) = clack_core::pack::Manifest::load(&p) {
                    packs.push(PackItem {
                        id: manifest.id,
                        name: manifest.name,
                        family: manifest.family.to_string(),
                        status: "ready".into(),
                    });
                }
            }
        }
    }

    // If no packs installed yet, provide the standard starter pack list
    if packs.is_empty() {
        packs = vec![
            PackItem {
                id: "cherrymx-blue-pbt".into(),
                name: "Cherry MX Blue (PBT)".into(),
                family: "clicky".into(),
                status: "ready".into(),
            },
            PackItem {
                id: "cherrymx-brown-pbt".into(),
                name: "Cherry MX Brown (PBT)".into(),
                family: "tactile".into(),
                status: "ready".into(),
            },
            PackItem {
                id: "ibm-model-m".into(),
                name: "IBM Model M (1985)".into(),
                family: "buckling".into(),
                status: "available".into(),
            },
            PackItem {
                id: "cream-travel".into(),
                name: "NK Cream (full travel)".into(),
                family: "linear".into(),
                status: "ready".into(),
            },
        ];
    }

    // Start local audio engine for live interactive preview
    let mut engine = Engine::start().ok();
    let mut current_bank: Option<SampleBank> = None;
    let mut loaded_pack_id = String::new();
    let mut rng = fastrand::Rng::new();

    let mut selected_idx = 0;
    let mut typed_text = String::from("the quick brown fox");

    terminal::enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, cursor::Hide)?;

    let res = (|| -> Result<String> {
        loop {
            // Load sample bank for currently selected pack if changed
            let sel = &packs[selected_idx];
            if sel.id != loaded_pack_id {
                let pdir = pack_dir(&sel.id);
                if pdir.exists() {
                    if let Ok(bank) = load_pack(&pdir, 48_000) {
                        current_bank = Some(bank);
                        loaded_pack_id = sel.id.clone();
                    }
                }
            }

            // Render TUI
            render_tui(&mut out, &packs, selected_idx, &typed_text)?;

            // Event poll
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Up => {
                                selected_idx = selected_idx.saturating_sub(1);
                            }
                            KeyCode::Down if selected_idx + 1 < packs.len() => {
                                selected_idx += 1;
                            }
                            KeyCode::Enter => {
                                return Ok(packs[selected_idx].id.clone());
                            }
                            KeyCode::Esc => {
                                return Ok(packs[selected_idx].id.clone());
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                return Ok(packs[selected_idx].id.clone());
                            }
                            KeyCode::Char(c) => {
                                typed_text.push(c);
                                if typed_text.len() > 36 {
                                    typed_text.remove(0);
                                }
                                // Play preview sound!
                                if let (Some(ref mut eng), Some(ref bank)) =
                                    (&mut engine, &current_bank)
                                {
                                    // Use ASCII code or standard keycode 30
                                    let code = match c {
                                        ' ' => 57,
                                        _ => 30,
                                    };
                                    if let Some(clip) = bank.pick_clip(code, false, &mut rng) {
                                        eng.play(clip.samples.clone(), clip.channels, 1.0, 1.0);
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                typed_text.pop();
                                if let (Some(ref mut eng), Some(ref bank)) =
                                    (&mut engine, &current_bank)
                                {
                                    if let Some(clip) = bank.pick_clip(14, false, &mut rng) {
                                        eng.play(clip.samples.clone(), clip.channels, 1.0, 1.0);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    })();

    // Restore terminal
    execute!(out, cursor::Show)?;
    terminal::disable_raw_mode()?;
    println!();

    res
}

fn render_tui<W: Write>(
    out: &mut W,
    packs: &[PackItem],
    selected: usize,
    typed_text: &str,
) -> Result<()> {
    // Clear lines and redraw picker
    execute!(out, cursor::SavePosition)?;

    for (idx, pack) in packs.iter().enumerate() {
        let is_sel = idx == selected;
        let prefix = if is_sel {
            "      \x1b[36m▸\x1b[0m "
        } else {
            "        "
        };
        let name_style = if is_sel { "\x1b[1m" } else { "" };
        let reset = "\x1b[0m";

        let status_color = if pack.status == "ready" {
            "\x1b[32m████\x1b[0m"
        } else {
            "\x1b[33m██░░\x1b[0m"
        };

        let line = format!(
            "{prefix}{name_style}{:<26}{reset} {:<10} {status_color} {:<10}\r\n",
            pack.name, pack.family, pack.status
        );
        out.write_all(line.as_bytes())?;
    }

    // Try it box
    out.write_all("      \x1b[90m┌ try it ──────────────────────────────┐\x1b[0m\r\n".as_bytes())?;
    let box_line = format!(
        "      \x1b[90m│\x1b[0m {:<36}\x1b[90m│\x1b[0m\r\n",
        format!("{typed_text}_")
    );
    out.write_all(box_line.as_bytes())?;
    out.write_all("      \x1b[90m└──────────────────────────────────────┘\x1b[0m\r\n".as_bytes())?;

    out.flush()?;
    execute!(out, cursor::RestorePosition)?;
    Ok(())
}
