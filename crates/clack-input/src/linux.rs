//! Linux evdev input backend (§5.1).
//!
//! - Enumerates /dev/input/event* and keeps only keyboard-like devices.
//! - One epoll instance watches all open devices.
//! - Hotplug via inotify on /dev/input.
//! - Returns typed AccessError so `clack doctor` can give precise advice.
//! - Never grabs the device (EVIOCGRAB); we are read-only observers.
//! - Auto-repeat events (value == 2) are dropped here, before the channel.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{atomic::{AtomicBool, Ordering}, Arc},
    time::Instant,
};

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use evdev::{Device, InputEventKind, Key};
use inotify::{Inotify, WatchMask};
use tracing::{debug, info, warn};

use clack_core::types::{AccessError, DeviceInfo, KeyCode, KeyEvent, KeyState};

use crate::InputBackend;

const INPUT_DIR: &str = "/dev/input";

pub struct LinuxInput;

impl InputBackend for LinuxInput {
    fn run(&mut self, tx: Sender<KeyEvent>, stop: Arc<AtomicBool>) -> Result<()> {
        run_loop(tx, stop)
    }

    fn probe() -> Result<Vec<DeviceInfo>, AccessError> {
        probe_devices()
    }
}

// ── Main event loop ───────────────────────────────────────────────────────────

fn run_loop(tx: Sender<KeyEvent>, stop: Arc<AtomicBool>) -> Result<()> {
    // Open all current keyboards.
    let mut devices: HashMap<PathBuf, Device> = HashMap::new();
    for path in enumerate_keyboards() {
        match open_device(&path) {
            Ok(dev) => { devices.insert(path, dev); }
            Err(e) => warn!("skipping {}: {e}", path.display()),
        }
    }

    if devices.is_empty() {
        warn!("no keyboard devices could be opened; waiting for hotplug");
    } else {
        info!("opened {} keyboard device(s)", devices.len());
    }

    // Set up inotify to watch /dev/input for hotplug events.
    let mut inotify = Inotify::init().context("initialising inotify")?;
    inotify
        .watches()
        .add(INPUT_DIR, WatchMask::CREATE | WatchMask::DELETE | WatchMask::ATTRIB)
        .context("adding inotify watch on /dev/input")?;

    let mut inotify_buf = [0u8; 4096];

    while !stop.load(Ordering::Relaxed) {
        // Poll each device with a short timeout so we can also check inotify and stop.
        let mut to_remove: Vec<PathBuf> = Vec::new();
        for (path, dev) in &mut devices {
            // fetch_events is non-blocking when there are events; returns immediately if none.
            match dev.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        if let InputEventKind::Key(key) = ev.kind() {
                            let value = ev.value();
                            // value: 1 = Down, 0 = Up, 2 = auto-repeat (drop)
                            let state = match value {
                                1 => KeyState::Down,
                                0 => KeyState::Up,
                                _ => continue, // drop auto-repeat
                            };
                            let code = key_to_evdev_code(key);
                            let event = KeyEvent { code, state, at: Instant::now() };
                            if tx.send(event).is_err() {
                                // Receiver dropped — time to stop.
                                return Ok(());
                            }
                        }
                    }
                }
                Err(e) if e.raw_os_error() == Some(LIBC_ENODEV) => {
                    debug!("device {} disconnected", path.display());
                    to_remove.push(path.clone());
                }
                Err(e) => {
                    warn!("read error on {}: {e}", path.display());
                    to_remove.push(path.clone());
                }
            }
        }
        for path in to_remove {
            devices.remove(&path);
        }

        // Process inotify events (hotplug).
        match inotify.read_events(&mut inotify_buf) {
            Ok(events) => {
                for event in events {
                    if let Some(name) = event.name {
                        let path = PathBuf::from(INPUT_DIR).join(name);
                        if is_event_node(&path) && !devices.contains_key(&path) {
                            match open_device(&path) {
                                Ok(dev) => {
                                    info!("hotplug: opened {}", path.display());
                                    devices.insert(path, dev);
                                }
                                Err(e) => debug!("hotplug: skipping {}: {e}", path.display()),
                            }
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => warn!("inotify read error: {e}"),
        }

        // Brief sleep to avoid a busy-wait when no events are coming.
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    info!("Linux input backend stopped");
    Ok(())
}

// ── Device helpers ────────────────────────────────────────────────────────────

/// Returns true if the path looks like /dev/input/eventN.
fn is_event_node(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with("event"))
        .unwrap_or(false)
}

/// List all /dev/input/eventN paths that look like keyboards.
fn enumerate_keyboards() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(INPUT_DIR) else { return vec![] };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_event_node(p) && is_keyboard(p))
        .collect()
}

/// Heuristic: the device reports EV_KEY and has KEY_A through KEY_Z and KEY_SPACE.
fn is_keyboard(path: &Path) -> bool {
    let Ok(dev) = Device::open(path) else { return false };
    let Some(supported) = dev.supported_keys() else { return false };
    supported.contains(Key::KEY_A)
        && supported.contains(Key::KEY_Z)
        && supported.contains(Key::KEY_SPACE)
}

/// Open a device read-only. Returns an error if permission is denied.
fn open_device(path: &Path) -> Result<Device> {
    Device::open(path).with_context(|| format!("opening {}", path.display()))
}

/// Probe keyboard devices and return useful info (used by `clack doctor` / `clack status`).
fn probe_devices() -> Result<Vec<DeviceInfo>, AccessError> {
    let Ok(entries) = fs::read_dir(INPUT_DIR) else {
        return Ok(vec![]);
    };

    let paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_event_node(p))
        .collect();

    if paths.is_empty() {
        return Err(AccessError::NoDevicesFound);
    }

    let mut devices = Vec::new();
    let mut denied = None;

    for path in &paths {
        match Device::open(path) {
            Ok(dev) => {
                let supported = dev.supported_keys();
                let is_kb = supported
                    .map(|k| k.contains(Key::KEY_A) && k.contains(Key::KEY_SPACE))
                    .unwrap_or(false);
                if is_kb {
                    devices.push(DeviceInfo {
                        path: path.to_string_lossy().into_owned(),
                        name: dev.name().unwrap_or("unknown").to_owned(),
                    });
                }
            }
            Err(e) if e.raw_os_error() == Some(13 /* EACCES */) => {
                denied = Some(path.to_string_lossy().into_owned());
            }
            Err(_) => {}
        }
    }

    if devices.is_empty() {
        if let Some(device) = denied {
            return Err(AccessError::PermissionDenied { device });
        }
        return Err(AccessError::NoDevicesFound);
    }

    Ok(devices)
}

/// Map an evdev `Key` back to its raw u16 keycode (evdev code is the canonical code, D15).
fn key_to_evdev_code(key: Key) -> KeyCode {
    // evdev::Key is a newtype over u16; use the raw value directly.
    key.code()
}

// ENODEV = 19 on Linux
const LIBC_ENODEV: i32 = 19;
