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
        self.keys
            .get(&code.to_string())
            .or(self.default.as_ref())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soundmap_lookup_falls_back_to_default() {
        let mut map = SoundMap::default();
        map.default = Some(SoundRef::Sprite([0, 90]));
        map.keys.insert("57".into(), SoundRef::Sprite([2000, 110]));

        // Space (57) has a specific entry
        assert!(map.get(57).is_some());
        // Arbitrary key falls back to default
        assert!(map.get(30).is_some());
        // Without a default, unmapped key returns None
        let empty = SoundMap::default();
        assert!(empty.get(30).is_none());
    }
}
