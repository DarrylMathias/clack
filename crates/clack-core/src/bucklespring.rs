//! Bucklespring soundpack importer (§6.3, M3.3).
//!
//! Imports IBM Model M sounds from zevv/bucklespring.
//! Bucklespring wav files are named `<hex>-0.wav` (key press) and `<hex>-1.wav` (key release),
//! where <hex> is the evdev keycode in hexadecimal.

use std::{collections::HashMap, fs, path::Path};

use anyhow::{Context, Result};

use crate::pack::{Family, Manifest, SoundMap, SoundRef};

pub fn is_bucklespring_pack(dir: &Path) -> bool {
    // Check if there are any files matching [0-9a-fA-F]+-[01]\.wav
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if is_bucklespring_filename(&name_str) {
                return true;
            }
        }
    }
    // Check subfolder wav/
    let wav_dir = dir.join("wav");
    if wav_dir.is_dir() {
        return is_bucklespring_pack(&wav_dir);
    }
    false
}

fn is_bucklespring_filename(name: &str) -> bool {
    if !name.ends_with(".wav") {
        return false;
    }
    let stem = &name[..name.len() - 4];
    if let Some((hex_part, state_part)) = stem.split_once('-') {
        (state_part == "0" || state_part == "1") && u16::from_str_radix(hex_part, 16).is_ok()
    } else {
        false
    }
}

pub fn import(src: &Path, dest: &Path, license: &str) -> Result<Manifest> {
    let wav_dir = if src.join("wav").is_dir() {
        src.join("wav")
    } else {
        src.to_path_buf()
    };

    let sounds_dir = dest.join("sounds");
    fs::create_dir_all(&sounds_dir).context("creating dest sounds directory")?;

    let mut press_keys: HashMap<String, SoundRef> = HashMap::new();
    let mut release_keys: HashMap<String, SoundRef> = HashMap::new();
    let mut alpha_press_clips: Vec<SoundRef> = Vec::new();
    let mut alpha_release_clips: Vec<SoundRef> = Vec::new();

    let entries =
        fs::read_dir(&wav_dir).with_context(|| format!("reading {}", wav_dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path.file_name().unwrap().to_string_lossy();
        if !is_bucklespring_filename(&file_name) {
            continue;
        }

        let stem = &file_name[..file_name.len() - 4];
        let (hex_part, state_part) = stem.split_once('-').unwrap();
        let evdev_code = u16::from_str_radix(hex_part, 16).unwrap();

        let rel_dest_sound = format!("sounds/{}", file_name);
        let dest_sound_path = dest.join(&rel_dest_sound);
        fs::copy(&path, &dest_sound_path).with_context(|| {
            format!(
                "copying {} to {}",
                path.display(),
                dest_sound_path.display()
            )
        })?;

        let sound_ref = SoundRef::File(rel_dest_sound);

        if state_part == "0" {
            // Press
            press_keys.insert(evdev_code.to_string(), sound_ref.clone());
            // Collect default alphanumeric key clips (e.g. A-Z, 0x10..=0x32)
            if (0x10..=0x32).contains(&evdev_code) && alpha_press_clips.len() < 10 {
                alpha_press_clips.push(sound_ref);
            }
        } else {
            // Release
            release_keys.insert(evdev_code.to_string(), sound_ref.clone());
            if (0x10..=0x32).contains(&evdev_code) && alpha_release_clips.len() < 10 {
                alpha_release_clips.push(sound_ref);
            }
        }
    }

    if press_keys.is_empty() {
        anyhow::bail!(
            "no valid Bucklespring audio files found in {}",
            src.display()
        );
    }

    let default_press = if !alpha_press_clips.is_empty() {
        Some(SoundRef::Multi(alpha_press_clips))
    } else {
        press_keys.values().next().cloned()
    };

    let default_release = if !alpha_release_clips.is_empty() {
        Some(SoundRef::Multi(alpha_release_clips))
    } else {
        release_keys.values().next().cloned()
    };

    let press_map = SoundMap {
        default: default_press,
        keys: press_keys,
    };

    let release_map = if !release_keys.is_empty() {
        Some(SoundMap {
            default: default_release,
            keys: release_keys,
        })
    } else {
        None
    };

    let manifest = Manifest {
        format: 1,
        id: "ibm-model-m".to_string(),
        name: "IBM Model M (1985)".to_string(),
        family: Family::Buckling,
        license: if license.is_empty() || license == "unknown" {
            "GPL-2.0".to_string()
        } else {
            license.to_string()
        },
        source: Some("https://github.com/zevv/bucklespring".to_string()),
        credits: Some("Ico Doornekamp (zevv) / Bucklespring".to_string()),
        sprite: None,
        gain: None,
        press: press_map,
        release: release_map,
    };

    let manifest_toml = toml::to_string_pretty(&manifest).context("serializing manifest.toml")?;
    fs::write(dest.join("manifest.toml"), manifest_toml).context("writing manifest.toml")?;

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucklespring_filename_check() {
        assert!(is_bucklespring_filename("1e-0.wav"));
        assert!(is_bucklespring_filename("1e-1.wav"));
        assert!(is_bucklespring_filename("39-0.wav"));
        assert!(!is_bucklespring_filename("sound.wav"));
        assert!(!is_bucklespring_filename("1e-2.wav"));
        assert!(!is_bucklespring_filename("zz-0.wav"));
    }
}
