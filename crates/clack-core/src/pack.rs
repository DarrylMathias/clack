//! Pack format — manifest parsing and sample bank (§5.4).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

use crate::types::KeyCode;

// ── Manifest ──────────────────────────────────────────────────────────────────

/// A sound value in the manifest: either a sprite range [start_ms, dur_ms]
/// or a relative file path, both wrapped in an array to allow random picks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SoundRef {
    Sprite([u32; 2]),
    File(String),
    Multi(Vec<SoundRef>),
}

/// `[press]` / `[release]` section of the manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SoundMap {
    /// Fallback for unmapped keycodes.
    pub default: Option<SoundRef>,
    /// Per-keycode overrides. Key is a stringified evdev code.
    #[serde(flatten)]
    pub keys: HashMap<String, SoundRef>,
}

impl SoundMap {
    /// Look up a keycode, falling back to the default.
    pub fn get(&self, code: KeyCode) -> Option<&SoundRef> {
        self.keys.get(&code.to_string()).or(self.default.as_ref())
    }
}

/// Pack sound family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Clicky,
    Tactile,
    Linear,
    Topre,
    Buckling,
    Typewriter,
    Other,
}

impl std::fmt::Display for Family {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Family::Clicky => "clicky",
            Family::Tactile => "tactile",
            Family::Linear => "linear",
            Family::Topre => "topre",
            Family::Buckling => "buckling",
            Family::Typewriter => "typewriter",
            Family::Other => "other",
        };
        write!(f, "{s}")
    }
}

/// Parsed `manifest.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub id: String,
    pub name: String,
    pub family: Family,
    pub license: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub credits: Option<String>,

    /// Sprite file path (relative to pack dir). Omit for multi-file mode.
    #[serde(default)]
    pub sprite: Option<String>,

    #[serde(default)]
    pub press: SoundMap,
    #[serde(default)]
    pub release: Option<SoundMap>,
}

impl Manifest {
    pub fn load(pack_dir: &std::path::Path) -> Result<Self> {
        let path = pack_dir.join("manifest.toml");
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading manifest {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("parsing manifest {}", path.display()))
    }
}

// ── Pack location helpers ─────────────────────────────────────────────────────

/// Returns the directory for a given pack id.
pub fn pack_dir(id: &str) -> PathBuf {
    crate::config::packs_dir().join(id)
}

/// Recursively copy a directory and its contents.
pub fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Import any pack (zip archive, tar.gz, native pack, Mechvibes, Bucklespring, Keyboardsounds, kbsim)
/// into `dest` directory and return the parsed `Manifest`.
pub fn import_pack(src: &std::path::Path, dest: &std::path::Path) -> Result<Manifest> {
    // If src is a zip or tar.gz archive, extract to temp dir and recurse.
    if src.is_file() {
        let name = src
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if name.ends_with(".zip") {
            let temp_dir = tempfile::tempdir().context("creating temp dir for zip extraction")?;
            let file = std::fs::File::open(src).context("opening zip file")?;
            let mut archive = zip::ZipArchive::new(file).context("reading zip archive")?;
            archive
                .extract(temp_dir.path())
                .context("extracting zip archive")?;
            return import_pack(temp_dir.path(), dest);
        } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            let temp_dir =
                tempfile::tempdir().context("creating temp dir for tar.gz extraction")?;
            let file = std::fs::File::open(src).context("opening tar.gz file")?;
            let gz = flate2::read::GzDecoder::new(file);
            let mut archive = tar::Archive::new(gz);
            archive
                .unpack(temp_dir.path())
                .context("extracting tar.gz archive")?;
            return import_pack(temp_dir.path(), dest);
        }
    }

    // If directory contains a single subfolder containing the actual pack, unwrap it.
    if src.is_dir() {
        if let Ok(entries) = std::fs::read_dir(src) {
            let entries: Vec<_> = entries.flatten().collect();
            if entries.len() == 1 && entries[0].path().is_dir() {
                let child = &entries[0].path();
                if child.join("manifest.toml").exists()
                    || child.join("config.json").exists()
                    || child.join("profile.yaml").exists()
                    || child.join("profile.yml").exists()
                    || crate::bucklespring::is_bucklespring_pack(child)
                {
                    return import_pack(child, dest);
                }
            }
        }
    }

    // 1. Native clack pack
    if src.join("manifest.toml").exists() {
        copy_dir_recursive(src, dest)?;
        return Manifest::load(dest);
    }

    // 2. Bucklespring
    if crate::bucklespring::is_bucklespring_pack(src) {
        return crate::bucklespring::import(src, dest, "GPL-2.0");
    }

    // 3. Mechvibes
    if src.join("config.json").exists() {
        return crate::mechvibes::import(src, dest, "unknown");
    }

    // 4. Keyboardsounds
    if crate::keyboardsounds::is_keyboardsounds_pack(src) {
        return crate::keyboardsounds::import(src, dest, "GPL-3.0");
    }

    // 5. kbsim
    if crate::kbsim::is_kbsim_pack(src) {
        return crate::kbsim::import(src, dest, "MIT");
    }

    anyhow::bail!("unrecognized pack format in {}", src.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soundmap_lookup_falls_back_to_default() {
        let mut map = SoundMap {
            default: Some(SoundRef::Sprite([0, 90])),
            ..Default::default()
        };
        map.keys.insert("57".into(), SoundRef::Sprite([2000, 110]));

        // Space (57) has a specific entry
        assert!(map.get(57).is_some());
        // Arbitrary key falls back to default
        assert!(map.get(30).is_some());
        // Without a default, unmapped key returns None
        let empty = SoundMap::default();
        assert!(empty.get(30).is_none());
    }

    #[test]
    fn test_manifest_parse_sprite_mode() {
        let toml_str = r#"
format = 1
id = "cherry-mx-blue-pbt"
name = "Cherry MX Blue (PBT)"
family = "clicky"
license = "MIT"
sprite = "sound.ogg"

[press]
default = [[0, 90], [120, 90]]
57 = [2000, 110]
"#;
        let manifest: Manifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.id, "cherry-mx-blue-pbt");
        assert_eq!(manifest.family, Family::Clicky);
        assert_eq!(manifest.sprite.as_deref(), Some("sound.ogg"));
        assert!(manifest.release.is_none());
        assert!(manifest.press.default.is_some());
    }

    #[test]
    fn test_manifest_parse_multi_file_mode() {
        let toml_str = r#"
format = 1
id = "ibm-model-m"
name = "IBM Model M (1985)"
family = "buckling"
license = "GPL-2.0"

[press]
default = "sounds/key.wav"
57 = "sounds/space.wav"

[release]
default = "sounds/rel.wav"
"#;
        let manifest: Manifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.id, "ibm-model-m");
        assert_eq!(manifest.family, Family::Buckling);
        assert!(manifest.sprite.is_none());
        assert!(manifest.release.is_some());
    }
}
