//! Keyboardsounds profile importer (§6.3, M3.3).
//!
//! Imports keyboardsounds profiles (e.g., Electric Typewriter, Zeal Tealios V2).

use std::{collections::HashMap, fs, path::Path};

use anyhow::{Context, Result};

use crate::pack::{Family, Manifest, SoundMap, SoundRef};

pub fn is_keyboardsounds_pack(dir: &Path) -> bool {
    dir.join("profile.yaml").exists()
        || dir.join("profile.yml").exists()
        || dir.join("profile.json").exists()
}

pub fn import(src: &Path, dest: &Path, license: &str) -> Result<Manifest> {
    let sounds_dir = dest.join("sounds");
    fs::create_dir_all(&sounds_dir).context("creating dest sounds directory")?;

    // Copy all audio files (.wav, .ogg, .mp3, .flac)
    let mut default_clips: Vec<SoundRef> = Vec::new();
    let mut press_keys: HashMap<String, SoundRef> = HashMap::new();

    let id = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "keyboardsounds-pack".to_string());

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
            press_keys.insert("57".to_string(), sound_ref); // KEY_SPACE
        } else if file_name.contains("enter")
            || file_name.contains("return")
            || file_name.starts_with("ent")
        {
            press_keys.insert("28".to_string(), sound_ref); // KEY_ENTER
        } else if file_name.contains("backspace") || file_name.starts_with("back") {
            press_keys.insert("14".to_string(), sound_ref); // KEY_BACKSPACE
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

    let family = if id.contains("typewriter") {
        Family::Typewriter
    } else if id.contains("tealios") || id.contains("linear") {
        Family::Linear
    } else if id.contains("blue") || id.contains("click") {
        Family::Clicky
    } else {
        Family::Tactile
    };

    let name = id
        .replace(['-', '_'], " ")
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let manifest = Manifest {
        format: 1,
        id,
        name,
        family,
        license: if license.is_empty() || license == "unknown" {
            "GPL-3.0".to_string()
        } else {
            license.to_string()
        },
        source: Some("https://github.com/nathan-fiscaletti/keyboardsounds".to_string()),
        credits: Some("Nathan Fiscaletti / keyboardsounds".to_string()),
        sprite: None,
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
