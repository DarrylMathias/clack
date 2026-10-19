use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::{info, warn};

use crate::keymap;
use crate::pack::{Family, Manifest, SoundMap, SoundRef};

#[derive(Debug, Deserialize)]
struct MechvibesConfig {
    name: String,
    key_define_type: String,
    sound: Option<String>,
    defines: HashMap<String, serde_json::Value>,
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .replace("--", "-")
        .trim_matches('-')
        .to_string()
}

pub fn import(src: &Path, dest: &Path, license: &str) -> Result<Manifest> {
    let config_path = src.join("config.json");
    let config_str = fs::read_to_string(&config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;

    let config: MechvibesConfig = serde_json::from_str(&config_str)
        .with_context(|| format!("parsing config.json in {}", src.display()))?;

    // Create dest dir if not exists
    fs::create_dir_all(dest).context("creating destination directory")?;

    let id = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .map(|s| slugify(&s))
        .unwrap_or_else(|| slugify(&config.name));

    let mut manifest = Manifest {
        format: 1,
        id,
        name: config.name.clone(),
        family: Family::Other,
        license: license.to_string(),
        source: Some("Imported from Mechvibes".to_string()),
        credits: None,
        sprite: None,
        gain: None,
        press: SoundMap::default(),
        release: None,
    };

    if config.key_define_type == "single" {
        if let Some(sound) = &config.sound {
            manifest.sprite = Some(sound.clone());
            let src_audio = src.join(sound);
            let dest_audio = dest.join(sound);
            if src_audio.exists() {
                fs::copy(&src_audio, &dest_audio).with_context(|| format!("copying {}", sound))?;
            } else {
                warn!("Sprite audio {} missing in source", sound);
            }
        }
    }

    let mut unknown_codes = 0;

    for (vc_str, val) in &config.defines {
        let vc = match vc_str.parse::<u32>() {
            Ok(v) => v,
            Err(_) => {
                warn!("Invalid keycode format in config.json"); // privacy-ok: json format validation error, no key data
                continue;
            }
        };

        let evdev = match keymap::mechvibes_to_evdev(vc) {
            Some(code) => code,
            None => {
                unknown_codes += 1;
                continue;
            }
        };

        let sound_ref = match config.key_define_type.as_str() {
            "single" => {
                // val is [start, duration]
                if let Some(arr) = val.as_array() {
                    if arr.len() == 2 {
                        let start = arr[0].as_u64().unwrap_or(0) as u32;
                        let dur = arr[1].as_u64().unwrap_or(0) as u32;
                        SoundRef::Sprite([start, dur])
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }
            "multi" => {
                // val is filename string
                if let Some(file) = val.as_str() {
                    let file_str = file.to_string();
                    let src_audio = src.join(&file_str);
                    let dest_audio = dest.join(&file_str);
                    if src_audio.exists() {
                        if !dest_audio.exists() {
                            fs::copy(&src_audio, &dest_audio).ok();
                        }
                    } else {
                        warn!("Audio file {} missing in source", file_str);
                    }
                    SoundRef::File(file_str)
                } else {
                    continue;
                }
            }
            _ => continue,
        };

        manifest.press.keys.insert(evdev.to_string(), sound_ref);
    }

    if unknown_codes > 0 {
        warn!(
            "Skipped {} unknown Mechvibes keycodes during import.",
            unknown_codes
        );
    }

    // Default: take up to 5 clips from the alphanumeric keys
    let mut default_clips = Vec::new();
    let alphanumeric_keys = [
        2, 3, 4, 5, 6, 7, 8, 9, 10, 11, // numbers 1-0
        16, 17, 18, 19, 20, 21, 22, 23, 24, 25, // Q-P
        30, 31, 32, 33, 34, 35, 36, 37, 38, // A-L
        44, 45, 46, 47, 48, 49, 50, // Z-M
    ];

    for key in alphanumeric_keys {
        if let Some(clip) = manifest.press.keys.get(&key.to_string()) {
            default_clips.push(clip.clone());
            if default_clips.len() >= 5 {
                break;
            }
        }
    }

    if !default_clips.is_empty() {
        if default_clips.len() == 1 {
            manifest.press.default = Some(default_clips.remove(0));
        } else {
            manifest.press.default = Some(SoundRef::Multi(default_clips));
        }
    } else if let Some((_, first_clip)) = manifest.press.keys.iter().next() {
        // Fallback if no alphanumeric keys were found
        manifest.press.default = Some(first_clip.clone());
    }

    let manifest_path = dest.join("manifest.toml");
    let toml_str = toml::to_string_pretty(&manifest).context("serializing manifest to TOML")?;
    fs::write(&manifest_path, toml_str).context("writing manifest.toml")?;

    info!(
        "Imported Mechvibes pack '{}' into {}",
        manifest.name,
        dest.display()
    );

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    #[ignore]
    fn test_import_all_mechvibes_packs() {
        let mechvibes_audio = Path::new("/tmp/mechvibes/src/audio");
        if !mechvibes_audio.exists() {
            println!("Skipping test: {} not found. Clone https://github.com/hainguyents13/mechvibes to /tmp/mechvibes to run this test.", mechvibes_audio.display());
            return;
        }

        let temp_dest = tempdir().unwrap();
        let mut success = 0;
        let mut failed = 0;

        for entry in fs::read_dir(mechvibes_audio).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                let pack_name = path.file_name().unwrap().to_string_lossy();
                let dest_path = temp_dest.path().join(&*pack_name);
                println!("Importing {}", pack_name);
                match import(&path, &dest_path, "unknown") {
                    Ok(_) => success += 1,
                    Err(e) => {
                        println!("Failed to import {}: {}", pack_name, e);
                        failed += 1;
                    }
                }
            }
        }

        println!("Imported {} packs successfully, {} failed", success, failed);
        assert_eq!(failed, 0, "All packs should import successfully");
        assert!(success > 0, "No packs were found to import");
    }
}
