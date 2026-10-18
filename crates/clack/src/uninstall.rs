//! `clack uninstall` — remove clack from this machine (§5.13, M3.8).

#[cfg(target_os = "linux")]
use std::path::Path;
use std::{fs, io::Write};

use anyhow::Result;

pub fn run_uninstall() -> Result<()> {
    println!("Uninstalling clack...\n");

    // 1. Stop service
    println!("Stopping background service...");
    let _ = crate::service::stop();

    // 2. Remove service unit
    #[cfg(target_os = "linux")]
    {
        let unit_path = dirs::config_dir().map(|p| p.join("systemd/user/clack.service"));
        if let Some(p) = unit_path {
            if p.exists() {
                let _ = fs::remove_file(&p);
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "daemon-reload"])
                    .output();
                println!("  ✓ Removed systemd user service unit");
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let plist = home.join("Library/LaunchAgents/dev.clack.plist");
            if plist.exists() {
                let _ = fs::remove_file(&plist);
                println!("  ✓ Removed launchd agent plist");
            }
        }
    }

    // 3. Linux udev rules check
    #[cfg(target_os = "linux")]
    {
        let udev_rule = Path::new("/etc/udev/rules.d/70-clack.rules");
        if udev_rule.exists() {
            println!("\nTo remove the udev permission rule, run:");
            println!("  sudo rm /etc/udev/rules.d/70-clack.rules && sudo udevadm control --reload");
        }
    }

    // 4. Prompt for config and data deletion
    print!("\nDelete sound packs and config too? [y/N]: ");
    std::io::stdout().flush()?;

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_ok() {
        let trimmed = answer.trim().to_lowercase();
        if trimmed == "y" || trimmed == "yes" {
            let cfg_dir = clack_core::config::config_path();
            if let Some(parent) = cfg_dir.parent() {
                if parent.exists() {
                    let _ = fs::remove_dir_all(parent);
                    println!("  ✓ Removed config at {}", parent.display());
                }
            }
            let data_dir = clack_core::config::packs_dir();
            if let Some(parent) = data_dir.parent() {
                if parent.exists() {
                    let _ = fs::remove_dir_all(parent);
                    println!("  ✓ Removed data and packs at {}", parent.display());
                }
            }
        } else {
            println!("Preserved sound packs and config.");
        }
    }

    // 5. Remove executable binary
    if let Ok(exe) = std::env::current_exe() {
        if exe.exists() {
            let _ = fs::remove_file(&exe);
            println!("  ✓ Removed binary at {}", exe.display());
        }
    }

    println!("\nclack has been uninstalled. Goodbye!");
    Ok(())
}
