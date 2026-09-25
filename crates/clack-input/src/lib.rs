//! clack-input — platform-specific keyboard input backends.
//!
//! The public trait `InputBackend` is re-exported from `clack-core` and
//! implemented here for Linux (evdev) and macOS (CGEventTap).

use std::sync::{atomic::AtomicBool, Arc};

use clack_core::types::{AccessError, DeviceInfo, KeyEvent};
use crossbeam_channel::Sender;

/// Common interface for all input backends.
/// Implementors must be Send (the trait is called from a spawned thread).
pub trait InputBackend: Send {
    /// Block, pushing events into `tx`, until `stop` is set.
    /// Handles hotplug internally.
    fn run(&mut self, tx: Sender<KeyEvent>, stop: Arc<AtomicBool>) -> anyhow::Result<()>;

    /// Probe available devices without opening a persistent listener.
    /// Returns the list of usable keyboards, or a typed error.
    fn probe() -> Result<Vec<DeviceInfo>, AccessError>
    where
        Self: Sized;
}

// Platform-specific implementations
#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

/// Return the platform default backend.
#[cfg(target_os = "linux")]
pub use linux::LinuxInput as DefaultBackend;

#[cfg(target_os = "macos")]
pub use macos::MacosInput as DefaultBackend;
