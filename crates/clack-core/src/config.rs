//! User configuration (`~/.config/clack/config.toml`) and runtime state (§5.10).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Toggle hotkey binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ToggleBinding {
    #[default]
    DoubleCtrl,
    DoubleShift,
    DoubleAlt,
    None,
}

impl std::fmt::Display for ToggleBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToggleBinding::DoubleCtrl => write!(f, "double-ctrl"),
            ToggleBinding::DoubleShift => write!(f, "double-shift"),
            ToggleBinding::DoubleAlt => write!(f, "double-alt"),
            ToggleBinding::None => write!(f, "none"),
        }
    }
}

/// Persisted user config (`~/.config/clack/config.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_pack: String,
    /// Master volume, 0–100.
    pub volume: u8,
    pub toggle: ToggleBinding,
    pub release_sounds: bool,
    pub humanize: bool,
    /// PipeWire node.name of the built-in speaker sink.
    /// Clack will ONLY output here; it never follows the default sink.
    /// If empty, the PIPEWIRE_NODE env var (or service Environment=) is used.
    /// If the sink does not exist, clack logs a warning and stays idle.
    pub speaker_sink: String,
    /// Seconds of silence after which the audio stream is closed.
    /// The stream reopens automatically on the next keypress.
    /// Set to 0 to keep the stream permanently open.
    pub idle_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_pack: "cherry-mx-blue-pbt".into(),
            volume: 70,
            toggle: ToggleBinding::DoubleCtrl,
            release_sounds: true,
            humanize: true,
            speaker_sink:
                "alsa_output.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Speaker__sink"
                    .into(),
            idle_timeout_secs: 10,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        config_dir().join("config.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Config::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("parsing config {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(&path, raw).with_context(|| format!("writing config {}", path.display()))
    }
}

/// Runtime state (`~/.local/state/clack/state.toml`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    pub muted: bool,
}

impl State {
    pub fn path() -> PathBuf {
        state_dir().join("state.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(State::default());
        }
        let raw = std::fs::read_to_string(&path)?;
        toml::from_str(&raw).with_context(|| format!("parsing state {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("writing state {}", path.display()))
    }
}

// ── XDG / standard paths (D16) ───────────────────────────────────────────────

/// `~/.config/clack/` (respects `$XDG_CONFIG_HOME`).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("clack")
}

/// `~/.local/share/clack/` (respects `$XDG_DATA_HOME`).
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("clack")
}

/// `~/.local/state/clack/` (respects `$XDG_STATE_HOME`).
pub fn state_dir() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/state"))
        .join("clack")
}

/// Path to the packs directory.
pub fn packs_dir() -> PathBuf {
    data_dir().join("packs")
}

/// Path to the IPC socket.
pub fn socket_path() -> PathBuf {
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime).join("clack.sock")
    } else {
        state_dir().join("clack.sock")
    }
}

/// Path to the main log file.
pub fn log_path() -> PathBuf {
    state_dir().join("clack.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrips() {
        let cfg = Config::default();
        let raw = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&raw).unwrap();
        assert_eq!(back.volume, cfg.volume);
        assert_eq!(back.toggle, cfg.toggle);
    }
}
