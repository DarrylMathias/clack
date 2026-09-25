//! Core types shared across all crates (§4.2).

use std::time::Instant;

/// Linux evdev KEY_* keycode (D15 — canonical keycode for all platforms).
pub type KeyCode = u16;

/// Direction of a key event.
/// Auto-repeat events are filtered out by the input backend before they reach this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Down,
    Up,
}

/// A single keyboard event, normalized to evdev keycodes.
#[derive(Debug, Clone)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub state: KeyState,
    /// Wall-clock timestamp captured as close to the hardware event as possible.
    pub at: Instant,
}

/// Human-readable information about a discovered keyboard device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// e.g. "/dev/input/event5"
    pub path: String,
    /// e.g. "AT Translated Set 2 keyboard"
    pub name: String,
}

/// Typed error returned by `InputBackend::probe()`.
#[derive(Debug, thiserror::Error)]
pub enum AccessError {
    #[error("permission denied for device {device:?}; run `clack setup` to fix")]
    PermissionDenied { device: String },
    #[error("no keyboard devices found")]
    NoDevicesFound,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
