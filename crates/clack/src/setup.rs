use std::{
    io::{stdout, Write},
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    terminal::{self, ClearType},
};

use clack_core::{
    catalog::{fetch_catalog, install_pack, CatalogEntry},
    config::{config_path, packs_dir, Config},
    decoder::load_pack,
    mixer::{perceptual_gain, Engine, SampleBank},
    pack::pack_dir,
};
use clack_input::{DefaultBackend, InputBackend};

struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(stdout(), terminal::EnterAlternateScreen, cursor::Hide)?;
        Ok(Self { active: true })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = execute!(stdout(), cursor::Show, terminal::LeaveAlternateScreen);
            let _ = terminal::disable_raw_mode();
        }
    }
}

struct PackItem {
    id: String,
    name: String,
    family: String,
    size_str: String,
    catalog_entry: Option<CatalogEntry>,
    is_installed: bool,
    is_downloading: bool,
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
    println!("  [2/4] Pick your keyboard sound (opening sound studio…)\n");
    let chosen_pack = run_picker()?.unwrap_or_else(|| {
        Config::load()
            .map(|c| c.default_pack)
            .unwrap_or_else(|_| "cherrymx-blue-pbt".to_string())
    });

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

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1}M", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{}K", bytes / 1_000)
    } else if bytes > 0 {
        format!("{}B", bytes)
    } else {
        String::new()
    }
}

fn family_badge(family: &str) -> String {
    format!("\x1b[38;5;244m[{}]\x1b[0m", family.to_uppercase())
}

fn char_width(c: char) -> usize {
    if c == '\u{fe0f}' || ('\u{0300}'..='\u{036f}').contains(&c) {
        0
    } else if ('\u{1f300}'..='\u{1faff}').contains(&c) || c == '⌨' || c == '🎹' {
        2
    } else {
        1
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn visual_width(s: &str) -> usize {
    strip_ansi(s).chars().map(char_width).sum()
}

fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut current_vw = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if current_vw + cw > max_width {
            if current_vw < max_width {
                out.push('…');
            }
            return out;
        }
        out.push(c);
        current_vw += cw;
    }
    out
}

fn pad_to(s: &str, target_width: usize) -> String {
    let vw = visual_width(s);
    if vw >= target_width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(target_width - vw))
    }
}

fn box_top(title: &str, width: usize) -> String {
    let border = "\x1b[38;5;239m";
    let reset = "\x1b[0m";
    if title.is_empty() {
        format!(
            "{border}╭{}╮{reset}\r\n",
            "─".repeat(width.saturating_sub(2))
        )
    } else {
        let title_fmt = format!(" {title} ");
        let v_len = visual_width(&title_fmt);
        let rem = width.saturating_sub(2 + 2 + v_len);
        format!(
            "{border}╭─{reset}{title_fmt}{border}{}╮{reset}\r\n",
            "─".repeat(rem)
        )
    }
}

fn box_row(content: &str, width: usize) -> String {
    let border = "\x1b[38;5;239m";
    let reset = "\x1b[0m";
    let inner_width = width.saturating_sub(4);
    let padded = pad_to(content, inner_width);
    format!("{border}│{reset} {padded} {border}│{reset}\r\n")
}

fn box_bottom(width: usize) -> String {
    let border = "\x1b[38;5;239m";
    let reset = "\x1b[0m";
    format!(
        "{border}╰{}╯{reset}\r\n",
        "─".repeat(width.saturating_sub(2))
    )
}

#[allow(clippy::needless_range_loop)]
fn render_frame(
    packs: &[PackItem],
    selected: usize,
    scroll_offset: usize,
    visible_count: usize,
    typed_text: &str,
    width: usize,
    volume: u8,
) -> String {
    let mut frame = String::with_capacity(4096);

    // 1. Simple header — just the name of clack and volume
    let header = format!(
        "  \x1b[1;38;5;141mclack\x1b[0m \x1b[38;5;244m— pick your keyboard sound\x1b[0m   \x1b[38;5;244m[vol: \x1b[1;38;5;255m{}%\x1b[0;38;5;244m  - / + to adjust]\x1b[0m\r\n\r\n",
        volume
    );
    frame.push_str(&header);

    // 2. Sound Packs Box
    frame.push_str(&box_top("\x1b[1;38;5;141mSound Packs\x1b[0m", width));

    if scroll_offset > 0 {
        let up_ind = format!("\x1b[38;5;244m    ▲ ({} more above)\x1b[0m", scroll_offset);
        frame.push_str(&box_row(&up_ind, width));
    }

    let end_idx = (scroll_offset + visible_count).min(packs.len());
    let inner_width = width.saturating_sub(4);

    for idx in scroll_offset..end_idx {
        let pack = &packs[idx];
        let is_sel = idx == selected;
        let pointer = if is_sel {
            "\x1b[1;38;5;141m❯\x1b[0m "
        } else {
            "  "
        };

        let badge = family_badge(&pack.family);
        let badge_vw = visual_width(&badge);

        let status = if pack.is_downloading {
            "\x1b[38;5;141m⠋ downloading…\x1b[0m".to_string()
        } else if pack.is_installed {
            "\x1b[38;5;78m● ready\x1b[0m".to_string()
        } else if is_sel {
            "\x1b[38;5;141m[Space to download]\x1b[0m".to_string()
        } else if !pack.size_str.is_empty() {
            format!("\x1b[38;5;244m↓ {}\x1b[0m", pack.size_str)
        } else {
            String::new()
        };

        let status_vw = visual_width(&status);
        // Calculate max name width available inside the row
        let reserved = 2 + badge_vw + 2 + status_vw + 2;
        let max_name_len = inner_width.saturating_sub(reserved).max(8);

        let truncated_name = truncate_to_width(&pack.name, max_name_len);
        let name_vw = visual_width(&truncated_name);
        let padded_name = if is_sel {
            format!(
                "\x1b[1;38;5;255m{}\x1b[0m{}",
                truncated_name,
                " ".repeat(max_name_len.saturating_sub(name_vw))
            )
        } else {
            format!(
                "\x1b[38;5;248m{}\x1b[0m{}",
                truncated_name,
                " ".repeat(max_name_len.saturating_sub(name_vw))
            )
        };

        let row_str = format!("{pointer}{padded_name}  {badge}  {status}");
        frame.push_str(&box_row(&row_str, width));
    }

    if end_idx < packs.len() {
        let down_ind = format!(
            "\x1b[38;5;244m    ▼ ({} more below)\x1b[0m",
            packs.len() - end_idx
        );
        frame.push_str(&box_row(&down_ind, width));
    }

    frame.push_str(&box_bottom(width));
    frame.push_str("\r\n");

    // 3. Plain no-name text box for typed characters
    frame.push_str(&box_top("", width));
    let display_text = if typed_text.is_empty() {
        "\x1b[38;5;240m(type anywhere on your keyboard to test sound...)\x1b[0m".to_string()
    } else {
        format!("\x1b[1;38;5;255m{typed_text}\x1b[38;5;78m█\x1b[0m")
    };
    frame.push_str(&box_row(&display_text, width));
    frame.push_str(&box_bottom(width));
    frame.push_str("\r\n");

    // 4. Clean minimal navigation footer
    let footer = "  \x1b[38;5;244m[↑/↓]\x1b[0m \x1b[38;5;250mNavigate\x1b[0m   \x1b[38;5;244m•\x1b[0m   \x1b[38;5;141m[Space]\x1b[0m \x1b[38;5;250mDownload\x1b[0m   \x1b[38;5;244m•\x1b[0m   \x1b[38;5;78m[Enter]\x1b[0m \x1b[38;5;255mSelect\x1b[0m   \x1b[38;5;244m•\x1b[0m   \x1b[38;5;141m[-/+]\x1b[0m \x1b[38;5;250mVol\x1b[0m   \x1b[38;5;244m•\x1b[0m   \x1b[38;5;244m[Esc]\x1b[0m \x1b[38;5;250mExit\x1b[0m\r\n";
    frame.push_str(footer);

    frame
}

pub fn run_picker() -> Result<Option<String>> {
    // 1. Discover packs from catalog and local disk
    let mut packs: Vec<PackItem> = Vec::new();

    if let Ok(cat) = fetch_catalog() {
        for entry in cat.packs {
            let pdir = pack_dir(&entry.id);
            let is_installed = pdir.exists() && clack_core::pack::Manifest::load(&pdir).is_ok();
            let size_str = format_size(entry.size);
            packs.push(PackItem {
                id: entry.id.clone(),
                name: entry.name.clone(),
                family: entry.family.clone(),
                size_str,
                catalog_entry: Some(entry),
                is_installed,
                is_downloading: false,
            });
        }
    }

    // Also look for any local custom packs
    if let Ok(entries) = std::fs::read_dir(packs_dir()) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if let Ok(manifest) = clack_core::pack::Manifest::load(&p) {
                    if !packs.iter().any(|item| item.id == manifest.id) {
                        packs.push(PackItem {
                            id: manifest.id,
                            name: manifest.name,
                            family: manifest.family.to_string(),
                            size_str: String::new(),
                            catalog_entry: None,
                            is_installed: true,
                            is_downloading: false,
                        });
                    }
                }
            }
        }
    }

    if packs.is_empty() {
        packs.push(PackItem {
            id: "ibm-model-m".into(),
            name: "IBM Model M (1985)".into(),
            family: "buckling".into(),
            size_str: "3.8M".into(),
            catalog_entry: None,
            is_installed: false,
            is_downloading: false,
        });
    }

    // Audio preview engine
    let cfg = Config::load().unwrap_or_default();
    let mut current_vol = cfg.volume;
    let mut engine = Engine::start().ok();
    if let Some(ref mut eng) = engine {
        eng.set_master_gain(perceptual_gain(current_vol));
    }
    let mut current_bank: Option<SampleBank> = None;
    let mut loaded_pack_id = String::new();
    let mut rng = fastrand::Rng::new();

    // Default selection to current active pack if found
    let active_id = cfg.default_pack.clone();
    let mut selected_idx = packs.iter().position(|p| p.id == active_id).unwrap_or(0);
    let mut scroll_offset = if selected_idx >= 5 {
        selected_idx.saturating_sub(2)
    } else {
        0
    };

    // If initially selected pack is already installed, load it immediately
    let initial_pdir = pack_dir(&packs[selected_idx].id);
    if initial_pdir.exists() {
        if let Ok(bank) = load_pack(&initial_pdir, 48_000) {
            current_bank = Some(bank);
            loaded_pack_id = packs[selected_idx].id.clone();
        }
    }

    let mut typed_text = String::new();

    let _guard = TerminalGuard::enter()?;
    let mut out = stdout();

    loop {
        let (cols, rows) = terminal::size().unwrap_or((80, 24));
        // Responsive width: never overflow small terminals!
        let width = (cols as usize).saturating_sub(4).clamp(32, 74);
        let visible_count = (rows as usize).saturating_sub(9).clamp(3, packs.len());

        if selected_idx >= scroll_offset + visible_count {
            scroll_offset = selected_idx + 1 - visible_count;
        } else if selected_idx < scroll_offset {
            scroll_offset = selected_idx;
        }

        let frame = render_frame(
            &packs,
            selected_idx,
            scroll_offset,
            visible_count,
            &typed_text,
            width,
            current_vol,
        );
        queue!(
            out,
            cursor::MoveTo(0, 0),
            terminal::Clear(ClearType::FromCursorDown)
        )?;
        out.write_all(frame.as_bytes())?;
        out.flush()?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Up if selected_idx > 0 => {
                            selected_idx -= 1;
                            let pdir = pack_dir(&packs[selected_idx].id);
                            if pdir.exists() && packs[selected_idx].id != loaded_pack_id {
                                if let Ok(bank) = load_pack(&pdir, 48_000) {
                                    current_bank = Some(bank);
                                    loaded_pack_id = packs[selected_idx].id.clone();
                                }
                            } else if !pdir.exists() {
                                current_bank = None;
                                loaded_pack_id.clear();
                            }
                        }
                        KeyCode::Down if selected_idx + 1 < packs.len() => {
                            selected_idx += 1;
                            let pdir = pack_dir(&packs[selected_idx].id);
                            if pdir.exists() && packs[selected_idx].id != loaded_pack_id {
                                if let Ok(bank) = load_pack(&pdir, 48_000) {
                                    current_bank = Some(bank);
                                    loaded_pack_id = packs[selected_idx].id.clone();
                                }
                            } else if !pdir.exists() {
                                current_bank = None;
                                loaded_pack_id.clear();
                            }
                        }
                        // SPACE: Download uninstalled pack, OR type space if already installed!
                        KeyCode::Char(' ') => {
                            if !packs[selected_idx].is_installed {
                                if let Some(entry) = packs[selected_idx].catalog_entry.clone() {
                                    packs[selected_idx].is_downloading = true;
                                    let frame = render_frame(
                                        &packs,
                                        selected_idx,
                                        scroll_offset,
                                        visible_count,
                                        &typed_text,
                                        width,
                                        current_vol,
                                    );
                                    queue!(
                                        out,
                                        cursor::MoveTo(0, 0),
                                        terminal::Clear(ClearType::FromCursorDown)
                                    )?;
                                    out.write_all(frame.as_bytes())?;
                                    out.flush()?;

                                    if install_pack(&entry).is_ok() {
                                        packs[selected_idx].is_installed = true;
                                        packs[selected_idx].is_downloading = false;
                                        let pdir = pack_dir(&packs[selected_idx].id);
                                        if let Ok(bank) = load_pack(&pdir, 48_000) {
                                            current_bank = Some(bank);
                                            loaded_pack_id = packs[selected_idx].id.clone();
                                        }
                                    } else {
                                        packs[selected_idx].is_downloading = false;
                                    }
                                }
                            } else {
                                typed_text.push(' ');
                                if typed_text.len() > width.saturating_sub(8) {
                                    typed_text.remove(0);
                                }
                                if let (Some(ref mut eng), Some(ref bank)) =
                                    (&mut engine, &current_bank)
                                {
                                    if let Some(clip) = bank.pick_clip(57, false, &mut rng) {
                                        eng.play(clip.samples.clone(), clip.channels, 1.0, 1.0);
                                    }
                                }
                            }
                        }
                        // ENTER: Select and finish (downloads first if needed)
                        KeyCode::Enter => {
                            if !packs[selected_idx].is_installed {
                                if let Some(entry) = packs[selected_idx].catalog_entry.clone() {
                                    packs[selected_idx].is_downloading = true;
                                    let frame = render_frame(
                                        &packs,
                                        selected_idx,
                                        scroll_offset,
                                        visible_count,
                                        &typed_text,
                                        width,
                                        current_vol,
                                    );
                                    queue!(
                                        out,
                                        cursor::MoveTo(0, 0),
                                        terminal::Clear(ClearType::FromCursorDown)
                                    )?;
                                    out.write_all(frame.as_bytes())?;
                                    out.flush()?;

                                    let _ = install_pack(&entry);
                                }
                            }
                            return Ok(Some(packs[selected_idx].id.clone()));
                        }
                        KeyCode::Esc => {
                            return Ok(None);
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            return Ok(None);
                        }
                        // VOLUME: - / _ lowers volume, + / = raises volume
                        KeyCode::Char('-') | KeyCode::Char('_') => {
                            current_vol = current_vol.saturating_sub(5);
                            if let Some(ref mut eng) = engine {
                                eng.set_master_gain(perceptual_gain(current_vol));
                                if let Some(ref bank) = current_bank {
                                    if let Some(clip) = bank.pick_clip(12, false, &mut rng) {
                                        eng.play(clip.samples.clone(), clip.channels, 1.0, 1.0);
                                    }
                                }
                            }
                            if let Ok(mut c) = Config::load() {
                                c.volume = current_vol;
                                let _ = c.save();
                            }
                            let _ = crate::ipc_client::send(&clack_core::ipc::Command::Volume {
                                value: current_vol,
                            });
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            current_vol = (current_vol + 5).min(100);
                            if let Some(ref mut eng) = engine {
                                eng.set_master_gain(perceptual_gain(current_vol));
                                if let Some(ref bank) = current_bank {
                                    if let Some(clip) = bank.pick_clip(13, false, &mut rng) {
                                        eng.play(clip.samples.clone(), clip.channels, 1.0, 1.0);
                                    }
                                }
                            }
                            if let Ok(mut c) = Config::load() {
                                c.volume = current_vol;
                                let _ = c.save();
                            }
                            let _ = crate::ipc_client::send(&clack_core::ipc::Command::Volume {
                                value: current_vol,
                            });
                        }
                        KeyCode::Char(c) => {
                            typed_text.push(c);
                            if typed_text.len() > width.saturating_sub(8) {
                                typed_text.remove(0);
                            }
                            if let (Some(ref mut eng), Some(ref bank)) =
                                (&mut engine, &current_bank)
                            {
                                let code = match c {
                                    '\n' | '\r' => 28,
                                    '\t' => 15,
                                    '0'..='9' => match c {
                                        '1' => 2,
                                        '2' => 3,
                                        '3' => 4,
                                        '4' => 5,
                                        '5' => 6,
                                        '6' => 7,
                                        '7' => 8,
                                        '8' => 9,
                                        '9' => 10,
                                        '0' => 11,
                                        _ => 30,
                                    },
                                    'a'..='z' | 'A'..='Z' => match c.to_ascii_lowercase() {
                                        'q' => 16,
                                        'w' => 17,
                                        'e' => 18,
                                        'r' => 19,
                                        't' => 20,
                                        'y' => 21,
                                        'u' => 22,
                                        'i' => 23,
                                        'o' => 24,
                                        'p' => 25,
                                        'a' => 30,
                                        's' => 31,
                                        'd' => 32,
                                        'f' => 33,
                                        'g' => 34,
                                        'h' => 35,
                                        'j' => 36,
                                        'k' => 37,
                                        'l' => 38,
                                        'z' => 44,
                                        'x' => 45,
                                        'c' => 46,
                                        'v' => 47,
                                        'b' => 48,
                                        'n' => 49,
                                        'm' => 50,
                                        _ => 30,
                                    },
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
}
