//! `clack update` — self-update to latest release (§5.12, M3.7).

use std::{fs, io::Read};

use anyhow::{Context, Result};

const REPO: &str = "DarrylMathias/clack";

#[derive(serde::Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(serde::Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub fn run_update() -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    println!("Checking for updates (current version: v{current_version})...");

    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = ureq::get(&url).set("User-Agent", "clack-updater").call();

    let release: GithubRelease = match resp {
        Ok(r) => r.into_json().context("parsing github release JSON")?,
        Err(e) => {
            println!("Could not reach GitHub Releases ({e}). You are on v{current_version}.");
            return Ok(());
        }
    };

    let latest_tag = release.tag_name.trim_start_matches('v');
    if latest_tag == current_version {
        println!("clack is already up to date (v{current_version})!");
        // Refresh catalog as part of update
        let _ = clack_core::catalog::fetch_catalog();
        return Ok(());
    }

    println!("Found new version: v{latest_tag}! Updating...");

    // Determine target asset name
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let target_name = "clack-linux-x86_64";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    let target_name = "clack-linux-aarch64";
    #[cfg(target_os = "macos")]
    let target_name = "clack-macos";

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == target_name || a.name.starts_with(target_name))
        .ok_or_else(|| {
            anyhow::anyhow!("no prebuilt binary asset found for target '{target_name}'")
        })?;

    println!("Downloading {}...", asset.name);
    let mut download_resp = ureq::get(&asset.browser_download_url)
        .set("User-Agent", "clack-updater")
        .call()
        .context("downloading binary asset")?
        .into_reader();

    let mut binary_bytes = Vec::new();
    download_resp
        .read_to_end(&mut binary_bytes)
        .context("reading downloaded binary")?;

    let current_exe = std::env::current_exe().context("locating current executable")?;
    let temp_exe = current_exe.with_extension("tmp_update");

    fs::write(&temp_exe, &binary_bytes).context("writing updated binary to temporary file")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&temp_exe)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&temp_exe, perms)?;
    }

    // Atomic replace
    fs::rename(&temp_exe, &current_exe).context("replacing executable binary")?;

    println!("Updated clack to v{latest_tag} successfully!");

    // Refresh catalog
    println!("Refreshing sound pack catalog...");
    let _ = clack_core::catalog::fetch_catalog();

    // Restart service if running
    println!("Restarting background service with updated binary...");
    if let Err(e) = crate::service::restart() {
        println!("Notice: could not restart background service: {e:#}");
        println!("You can restart it manually with: clack restart");
    }

    #[cfg(target_os = "macos")]
    {
        println!(
            "\nNOTE: macOS may reset Input Monitoring permissions for updated unsigned binaries."
        );
        println!("If keyboard sounds stop, re-grant permission in System Settings -> Privacy -> Input Monitoring.");
    }

    Ok(())
}
