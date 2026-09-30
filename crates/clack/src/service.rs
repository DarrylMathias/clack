use anyhow::Result;
use dirs::config_dir;
use std::path::PathBuf;
use std::process::Command;

#[cfg(target_os = "linux")]
fn is_systemd_available() -> bool {
    Command::new("systemctl").arg("--version").output().is_ok()
}

#[cfg(target_os = "linux")]
pub fn install_and_start() -> Result<()> {
    if is_systemd_available() {
        let sysd_dir = config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("systemd/user");
        std::fs::create_dir_all(&sysd_dir)?;
        let svc_path = sysd_dir.join("clack.service");
        let exe = std::env::current_exe()?;
        // Read the configured speaker sink so the unit matches the binary.
        let speaker_sink = clack_core::config::Config::load()
            .unwrap_or_default()
            .speaker_sink;
        let content = format!(
            "[Unit]\n\
            Description=Clack Keyboard Sounds\n\
            After=graphical-session.target pipewire.service wireplumber.service\n\
            \n\
            [Service]\n\
            ExecStart=\"{}\" daemon\n\
            Restart=on-failure\n\
            RestartSec=3\n\
            Environment=PIPEWIRE_NODE={}\n\
            Environment=PIPEWIRE_LATENCY=256/48000\n\
            \n\
            [Install]\n\
            WantedBy=default.target\n",
            exe.display(),
            speaker_sink,
        );
        std::fs::write(&svc_path, content)?;
        // Always reload so the new unit takes effect.
        Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()?;
        Command::new("systemctl")
            .args(["--user", "enable", "--now", "clack.service"])
            .status()?;
        println!("clack started (systemd)");
    } else {
        // XDG autostart
        let autostart_dir = config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("autostart");
        std::fs::create_dir_all(&autostart_dir)?;
        let desktop_path = autostart_dir.join("clack.desktop");
        let exe = std::env::current_exe()?;
        let content = format!(
            "[Desktop Entry]\n\
            Type=Application\n\
            Name=Clack\n\
            Exec=\"{}\" daemon\n\
            Hidden=false\n\
            NoDisplay=false\n\
            X-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        std::fs::write(&desktop_path, content)?;
        println!("clack installed to XDG autostart.");

        let wm = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase();
        if wm.contains("hyprland") || wm.contains("sway") || wm.contains("i3") {
            println!("Hint: Your window manager may ignore XDG autostart.");
            println!(
                "You may need to add this to your config: exec-once = {}",
                exe.display()
            );
        }

        spawn_detached()?;
        println!("clack started (detached)");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn stop() -> Result<()> {
    if is_systemd_available() {
        Command::new("systemctl")
            .args(["--user", "stop", "clack.service"])
            .status()?;
        println!("clack stopped (systemd)");
    } else {
        let _ = crate::ipc_client::send(&clack_core::ipc::Command::Shutdown);
        println!("clack stopped");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn restart() -> Result<()> {
    if is_systemd_available() {
        Command::new("systemctl")
            .args(["--user", "restart", "clack.service"])
            .status()?;
        println!("clack restarted (systemd)");
    } else {
        stop()?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        spawn_detached()?;
        println!("clack restarted (detached)");
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn install_and_start() -> Result<()> {
    spawn_detached()?;
    println!("clack started (detached)");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn stop() -> Result<()> {
    let _ = crate::ipc_client::send(&clack_core::ipc::Command::Shutdown);
    println!("clack stopped");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn restart() -> Result<()> {
    stop()?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    spawn_detached()?;
    println!("clack restarted (detached)");
    Ok(())
}

fn spawn_detached() -> Result<()> {
    // We just spawn in background. `setsid` could be done via `std::os::unix::process::CommandExt`.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Command::new(std::env::current_exe()?)
            .arg("daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0) // setsid eqv if not session leader
            .spawn()?;
    }
    #[cfg(not(unix))]
    {
        Command::new(std::env::current_exe()?)
            .arg("daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
    }
    Ok(())
}
