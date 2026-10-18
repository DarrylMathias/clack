#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;
#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "linux")]
pub fn setup_permissions() -> Result<()> {
    // Check if running NixOS.
    if std::path::Path::new("/etc/NIXOS").exists() || std::path::Path::new("/etc/nixos").exists() {
        println!("You are running NixOS. Please add the following to your configuration.nix:");
        println!("  services.udev.extraRules = \"KERNEL==\\\"event*\\\", SUBSYSTEM==\\\"input\\\", TAG+=\\\"uaccess\\\"\";"); // privacy-ok: udev device glob, no key data
        println!("Then run `nixos-rebuild switch` and reboot.");
        return Ok(());
    }

    // Check if systemd-logind is running.
    let has_logind = Command::new("systemctl")
        .args(["is-active", "systemd-logind"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let udev_rule = if has_logind {
        "KERNEL==\"event*\", SUBSYSTEM==\"input\", TAG+=\"uaccess\""
    } else {
        "KERNEL==\"event*\", SUBSYSTEM==\"input\", GROUP=\"input\", MODE=\"0660\""
    };

    println!("We need to install a udev rule to allow reading keyboard events."); // privacy-ok: prose message, no key data
    println!("This requires sudo.");

    let rule_path = "/etc/udev/rules.d/70-clack.rules";
    let sh_cmd = format!("echo '{}' > {}", udev_rule, rule_path);

    let status = Command::new("sudo")
        .arg("sh")
        .arg("-c")
        .arg(&sh_cmd)
        .status()?;

    if !status.success() {
        anyhow::bail!("Failed to write udev rule.");
    }

    Command::new("sudo")
        .args(["udevadm", "control", "--reload-rules"])
        .status()?;
    Command::new("sudo").args(["udevadm", "trigger"]).status()?;

    if !has_logind {
        let user = std::env::var("USER").context("USER not set")?;
        println!("Systemd-logind not detected. Adding you to the 'input' group.");
        Command::new("sudo")
            .args(["usermod", "-aG", "input", &user])
            .status()?;
        println!("IMPORTANT: You must log out and log back in for group changes to take effect.");
    } else {
        println!("Permissions set up successfully.");
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn setup_permissions() -> Result<()> {
    println!("Permission setup is handled natively on macOS via the accessibility prompt.");
    Ok(())
}
