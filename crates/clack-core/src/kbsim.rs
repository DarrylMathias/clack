//! kbsim soundpack importer (§6.3, M3.3).
//!
//! Imports Alps SKCM Blue and other kbsim packs from tplai/kbsim.

use std::{collections::HashMap, fs, path::Path};

use anyhow::{Context, Result};

use crate::pack::{Family, Manifest, SoundMap, SoundRef};

pub fn is_kbsim_pack(dir: &Path) -> bool {
    // If it's a folder containing audio files and matches kbsim layout or naming
    dir.join("config.yaml").exists()
        || dir.join("pack.json").exists()
        || (dir.is_dir()
            && fs::read_dir(dir)
                .map(|mut r| {
                    r.any(|e| {
                        e.ok().is_some_and(|entry| {
                            let n = entry.file_name().to_string_lossy().to_lowercase();
                            n.starts_with("bluealps") || n.starts_with("kbsim")
                        })
                    })
                })
                .unwrap_or(false))
}

pub fn import(src: &Path, dest: &Path, license: &str) -> Result<Manifest> {
    let sounds_dir = dest.join("sounds");
    fs::create_dir_all(&sounds_dir).context("creating dest sounds directory")?;

    let mut default_clips: Vec<SoundRef> = Vec::new();
    let mut press_keys: HashMap<String, SoundRef> = HashMap::new();

    let id = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "alps-skcm-blue".to_string());

    let entries = fs::read_dir(src).with_context(|| format!("reading {}", src.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path.file_name().unwrap().to_string_lossy().to_lowercase();
        let is_audio = file_name.ends_with(".wav")
            || file_name.ends_with(".ogg")
            || file_name.ends_with(".mp3")
            || file_name.ends_with(".flac");

        if !is_audio {
            continue;
        }

        let rel_dest = format!("sounds/{}", file_name);
        let dest_path = dest.join(&rel_dest);
        fs::copy(&path, &dest_path)?;

        let sound_ref = SoundRef::File(rel_dest);

        if file_name.contains("space") {
            press_keys.insert("57".to_string(), sound_ref);
        } else if file_name.contains("enter") || file_name.contains("return") {
            press_keys.insert("28".to_string(), sound_ref);
        } else if file_name.contains("backspace") {
            press_keys.insert("14".to_string(), sound_ref);
        } else {
            default_clips.push(sound_ref);
        }
    }

    let default_press = if !default_clips.is_empty() {
        if default_clips.len() == 1 {
            Some(default_clips.remove(0))
        } else {
            Some(SoundRef::Multi(default_clips))
        }
    } else {
        press_keys.values().next().cloned()
    };

    let manifest = Manifest {
        format: 1,
        id: id.clone(),
        name: if id.contains("bluealps") || id.contains("alps") {
            "Alps SKCM Blue".to_string()
        } else {
            id
        },
        family: Family::Clicky,
        license: if license.is_empty() || license == "unknown" {
            "MIT".to_string()
        } else {
            license.to_string()
        },
        source: Some("https://github.com/tplai/kbsim".to_string()),
        credits: Some("tplai / kbsim".to_string()),
        sprite: None,
        gain: None,
        press: SoundMap {
            default: default_press,
            keys: press_keys,
        },
        release: None,
    };

    let manifest_toml = toml::to_string_pretty(&manifest).context("serializing manifest.toml")?;
    fs::write(dest.join("manifest.toml"), manifest_toml).context("writing manifest.toml")?;

    Ok(manifest)
}
