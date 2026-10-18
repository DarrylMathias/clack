//! `clack doctor` — find and fix problems (§5.11, M3.6).
//!
//! Prints a ✓ / ✗ / ! line for each system health check with exact fix commands.

use std::path::Path;

use anyhow::Result;
use clack_core::{
    config::{config_path, Config},
    decoder::load_pack,
    ipc::Command,
    pack::pack_dir,
};
use clack_input::{DefaultBackend, InputBackend};
use cpal::traits::{DeviceTrait, HostTrait};

pub struct CheckResult {
    pub status: StatusLevel,
    pub title: String,
    pub detail: Option<String>,
    pub fix: Option<String>,
}

pub enum StatusLevel {
    Ok,
    Warn,
    Error,
}

pub fn run_doctor() -> Result<()> {
    println!("clack doctor — checking system health...\n");

    let mut checks = vec![
        // 1. Binary on PATH and version
        check_binary(),
        // 2. Config validity
        check_config(),
        // 3. Active pack validity
        check_active_pack(),
        // 4. Service status & IPC socket
        check_service_and_ipc(),
        // 5. Input permissions & keyboards
        check_input_access(),
        // 6. Audio output device
        check_audio(),
    ];

    // 7. Desktop environment autostart hint
    if let Some(check) = check_wm_autostart() {
        checks.push(check);
    }

    // Print all results
    let mut all_ok = true;
    for check in &checks {
        let symbol = match check.status {
            StatusLevel::Ok => "\x1b[32m✓\x1b[0m",
            StatusLevel::Warn => {
                all_ok = false;
                "\x1b[33m!\x1b[0m"
            }
            StatusLevel::Error => {
                all_ok = false;
                "\x1b[31m✗\x1b[0m"
            }
        };

        println!("  {symbol} {}", check.title);
        if let Some(ref detail) = check.detail {
            println!("    {detail}");
        }
        if let Some(ref fix) = check.fix {
            println!("    \x1b[1mFix:\x1b[0m {fix}");
        }
    }

    println!();
    if all_ok {
        println!("All checks passed! clack is ready to clack.");
    } else {
        println!("Some checks reported warnings or errors. Follow the Fix instructions above.");
    }

    Ok(())
}

fn check_binary() -> CheckResult {
    let current_exe = std::env::current_exe().unwrap_or_default();
    let version = env!("CARGO_PKG_VERSION");

    let path_var = std::env::var("PATH").unwrap_or_default();
    let in_path = std::env::split_paths(&path_var).any(|p| current_exe.starts_with(&p));

    if in_path {
        CheckResult {
            status: StatusLevel::Ok,
            title: format!("Binary: {} (v{})", current_exe.display(), version),
            detail: None,
            fix: None,
        }
    } else {
        CheckResult {
            status: StatusLevel::Warn,
            title: format!("Binary not in PATH: {}", current_exe.display()),
            detail: Some("Run clack from a standard location so service and shell can find it.".into()),
            fix: Some("mkdir -p ~/.local/bin && cp target/release/clack ~/.local/bin/ && export PATH=\"$HOME/.local/bin:$PATH\"".into()),
        }
    }
}

fn check_config() -> CheckResult {
    let path = config_path();
    if !path.exists() {
        return CheckResult {
            status: StatusLevel::Warn,
            title: "Config file does not exist yet (using defaults)".into(),
            detail: Some(format!("Expected at {}", path.display())),
            fix: Some("Run `clack setup` or `clack status` to generate default config.".into()),
        };
    }

    match Config::load() {
        Ok(cfg) => CheckResult {
            status: StatusLevel::Ok,
            title: format!(
                "Config: valid (pack: '{}', vol: {})",
                cfg.default_pack, cfg.volume
            ),
            detail: None,
            fix: None,
        },
        Err(e) => CheckResult {
            status: StatusLevel::Error,
            title: "Config file syntax error".into(),
            detail: Some(format!("Error: {e:#}")),
            fix: Some(format!(
                "Edit {} or remove it to restore defaults",
                path.display()
            )),
        },
    }
}

fn check_active_pack() -> CheckResult {
    let cfg = Config::load().unwrap_or_default();
    let pdir = pack_dir(&cfg.default_pack);

    if !pdir.exists() {
        return CheckResult {
            status: StatusLevel::Error,
            title: format!("Active pack '{}' is not installed", cfg.default_pack),
            detail: Some(format!("Missing directory: {}", pdir.display())),
            fix: Some(format!(
                "Run `clack pack add {}` or `clack use <pack>`",
                cfg.default_pack
            )),
        };
    }

    match load_pack(&pdir, 48_000) {
        Ok(bank) => {
            let press_count = bank.press.iter().flatten().count() + bank.press_default.len();
            CheckResult {
                status: StatusLevel::Ok,
                title: format!(
                    "Active pack '{}' loaded successfully ({} clips)",
                    cfg.default_pack, press_count
                ),
                detail: None,
                fix: None,
            }
        }
        Err(e) => CheckResult {
            status: StatusLevel::Error,
            title: format!("Active pack '{}' manifest/audio error", cfg.default_pack),
            detail: Some(format!("Error: {e:#}")),
            fix: Some(format!(
                "Check files in {} or switch with `clack use <pack>`",
                pdir.display()
            )),
        },
    }
}

fn check_service_and_ipc() -> CheckResult {
    let ipc_res = crate::ipc_client::send(&Command::Status);

    match ipc_res {
        Ok(resp) => {
            if let Some(s) = resp.status {
                let state_str = if s.muted { "muted" } else { "active" };
                CheckResult {
                    status: StatusLevel::Ok,
                    title: format!("Daemon running and responding via IPC ({state_str})"),
                    detail: None,
                    fix: None,
                }
            } else {
                CheckResult {
                    status: StatusLevel::Ok,
                    title: "Daemon connected via IPC".into(),
                    detail: None,
                    fix: None,
                }
            }
        }
        Err(_) => {
            // Check if service unit is installed
            #[cfg(target_os = "linux")]
            let svc_check = std::process::Command::new("systemctl")
                .args(["--user", "is-active", "clack"])
                .output();

            #[cfg(target_os = "linux")]
            let svc_active = svc_check.map(|o| o.status.success()).unwrap_or(false);
            #[cfg(not(target_os = "linux"))]
            let svc_active = false;

            if svc_active {
                CheckResult {
                    status: StatusLevel::Warn,
                    title: "systemd service is active but daemon is not responding to IPC".into(),
                    detail: Some("The socket may be starting up or stuck.".into()),
                    fix: Some("systemctl --user restart clack".into()),
                }
            } else {
                CheckResult {
                    status: StatusLevel::Warn,
                    title: "clack service is not currently running".into(),
                    detail: Some("No running daemon detected.".into()),
                    fix: Some("Run `clack start` to start the background service.".into()),
                }
            }
        }
    }
}

fn check_input_access() -> CheckResult {
    match DefaultBackend::probe() {
        Ok(devs) if !devs.is_empty() => {
            let names: Vec<String> = devs.iter().map(|d| d.name.clone()).collect();
            CheckResult {
                status: StatusLevel::Ok,
                title: format!(
                    "Keyboard input access: OK ({} keyboards readable)",
                    devs.len()
                ),
                detail: Some(format!("Devices: {}", names.join(", "))),
                fix: None,
            }
        }
        Ok(_) => CheckResult {
            status: StatusLevel::Warn,
            title: "No keyboard devices detected".into(),
            detail: Some("No input devices currently matched keyboard heuristics.".into()),
            fix: Some("Plug in a keyboard or check /dev/input permissions.".into()),
        },
        Err(e) => {
            #[cfg(target_os = "linux")]
            {
                let udev_rule = Path::new("/etc/udev/rules.d/70-clack.rules");
                let rule_exists = udev_rule.exists();

                let mut fix = "Run `clack setup --permissions-only` (or install /etc/udev/rules.d/70-clack.rules)".to_string();
                if !rule_exists {
                    fix = "sudo cp assets/70-clack.rules /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger".into();
                }

                CheckResult {
                    status: StatusLevel::Error,
                    title: "Permission denied reading /dev/input/event*".into(),
                    detail: Some(format!("{e}")),
                    fix: Some(fix),
                }
            }

            #[cfg(target_os = "macos")]
            {
                CheckResult {
                    status: StatusLevel::Error,
                    title: "macOS Input Monitoring permission required".into(),
                    detail: Some("clack needs Input Monitoring permission to observe keyboard taps.".into()),
                    fix: Some("Open System Settings -> Privacy & Security -> Input Monitoring -> Enable clack".into()),
                }
            }
        }
    }
}

fn check_audio() -> CheckResult {
    #[cfg(target_os = "linux")]
    {
        let sink = std::env::var("PIPEWIRE_NODE")
            .unwrap_or_else(|_| clack_core::config::DEFAULT_SPEAKER_SINK.to_string());
        if !clack_core::mixer::check_pipewire_node_exists(&sink) {
            return CheckResult {
                status: StatusLevel::Warn,
                title: format!("Target audio sink '{sink}' not found in PipeWire graph"),
                detail: Some("The default laptop speaker sink was not found. PipeWire may be idle or using a different sink name.".into()),
                fix: Some("Check sinks with `wpctl status` and set PIPEWIRE_NODE=<sink_name>".into()),
            };
        }
    }

    let host = cpal::default_host();
    match host.default_output_device() {
        Some(dev) => {
            let name = dev.name().unwrap_or_else(|_| "default device".into());
            CheckResult {
                status: StatusLevel::Ok,
                title: format!("Audio output device: OK ({name})"),
                detail: Some("48000 Hz target with ~5.3 ms buffer quantum (256/48000)".into()),
                fix: None,
            }
        }
        None => CheckResult {
            status: StatusLevel::Error,
            title: "No default audio output device found".into(),
            detail: Some(
                "Audio server (PipeWire/PulseAudio/CoreAudio) has no active output.".into(),
            ),
            fix: Some("Ensure sound server is running and an audio device is connected.".into()),
        },
    }
}

fn check_wm_autostart() -> Option<CheckResult> {
    #[cfg(target_os = "linux")]
    {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase();
        let _wayland_display = std::env::var("WAYLAND_DISPLAY").ok();

        if desktop.contains("hyprland") || std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            return Some(CheckResult {
                status: StatusLevel::Ok,
                title: "Window Manager: Hyprland detected".into(),
                detail: Some("clack systemd user unit works on Hyprland, or add `exec-once = clack start` to hyprland.conf.".into()),
                fix: None,
            });
        } else if desktop.contains("sway") || std::env::var("SWAYSOCK").is_ok() {
            return Some(CheckResult {
                status: StatusLevel::Ok,
                title: "Window Manager: Sway detected".into(),
                detail: Some("Add `exec clack start` to ~/.config/sway/config if systemd user services are disabled.".into()),
                fix: None,
            });
        }
    }
    None
}
