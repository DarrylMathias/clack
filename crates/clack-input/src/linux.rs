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
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
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
            Ok(dev) => {
                devices.insert(path, dev);
            }
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
    
    // Set inotify to non-blocking
    use std::os::fd::AsRawFd;
    unsafe {
        let fd = inotify.as_raw_fd();
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }

    inotify
        .watches()
        .add(
            INPUT_DIR,
            WatchMask::CREATE | WatchMask::DELETE | WatchMask::ATTRIB,
        )
        .context("adding inotify watch on /dev/input")?;

    let mut inotify_buf = [0u8; 4096];

    let mut dedupe = DedupeFilter::new();

    while !stop.load(Ordering::Relaxed) {
        // Poll each device with a short timeout so we can also check inotify and stop.
        let mut to_remove: Vec<PathBuf> = Vec::new();
        for (path, dev) in &mut devices {
            let dev_phys = dev.physical_path().unwrap_or("unknown").to_string();
            // fetch_events is non-blocking when there are events; returns immediately if none.
            match dev.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        if let InputEventKind::Key(key) = ev.kind() {
                            let value = ev.value();
                            let state = match value {
                                1 => KeyState::Down,
                                0 => KeyState::Up,
                                _ => continue, // drop auto-repeat
                            };
                            let code = key_to_evdev_code(key);
                            let event = KeyEvent {
                                code,
                                state,
                                at: Instant::now(),
                            };
                            
                            if !dedupe.filter(&dev_phys, &event) {
                                continue;
                            }
                            
                            info!("Input thread parsed key event: {:?}", event);
                            if tx.send(event).is_err() {
                                return Ok(());
                            }
                        }
                    }
                }
                Err(e) if e.raw_os_error() == Some(LIBC_ENODEV) => {
                    info!("device {} disconnected", path.display());
                    to_remove.push(path.clone());
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // non-blocking
                }
                Err(e) => {
                    info!("read error on {}: {e}", path.display());
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

// ── Dedupe Filter ─────────────────────────────────────────────────────────────

struct DedupeFilter {
    recent_downs: HashMap<(String, KeyCode), Instant>,
}

impl DedupeFilter {
    fn new() -> Self {
        DedupeFilter {
            recent_downs: HashMap::new(),
        }
    }

    /// Returns true if this event should be passed through.
    fn filter(&mut self, dev_phys: &str, ev: &KeyEvent) -> bool {
        if ev.state == KeyState::Up {
            return true;
        }

        // Extract base phys path, e.g. "usb-0000:00:14.0-2.4/input0" -> "usb-0000:00:14.0-2.4"
        let base_phys = if let Some(idx) = dev_phys.rfind("/input") {
            &dev_phys[..idx]
        } else {
            dev_phys
        };

        let key = (base_phys.to_string(), ev.code);
        let now = Instant::now();
        
        if let Some(last_at) = self.recent_downs.get(&key) {
            if now.duration_since(*last_at).as_millis() < 5 {
                return false; // dedupe
            }
        }
        
        self.recent_downs.insert(key, now);
        
        // Clean up old entries periodically to prevent unbounded growth.
        // Doing it probabilistically or lazily is fine.
        if self.recent_downs.len() > 100 {
            self.recent_downs.retain(|_, at| now.duration_since(*at).as_millis() < 100);
        }
        
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedupe_filter() {
        let mut filter = DedupeFilter::new();
        let ev = KeyEvent {
            code: 30,
            state: KeyState::Down,
            at: Instant::now(),
        };

        // First down from keyboard 1 should pass.
        assert!(filter.filter("usb-0000:00:14.0-2.4/input0", &ev));
        
        // Second down from keyboard 2 (same phys root) within 5ms should be filtered.
        assert!(!filter.filter("usb-0000:00:14.0-2.4/input1", &ev));
        
        // Down from entirely different keyboard should pass.
        assert!(filter.filter("usb-0000:00:14.0-3/input0", &ev));
        
        // Up events should always pass.
        let up_ev = KeyEvent {
            code: 30,
            state: KeyState::Up,
            at: Instant::now(),
        };
        assert!(filter.filter("usb-0000:00:14.0-2.4/input1", &up_ev));
    }
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
    let Ok(entries) = fs::read_dir(INPUT_DIR) else {
        return vec![];
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_event_node(p) && is_keyboard(p))
        .collect()
}

/// Heuristic: the device reports EV_KEY and has KEY_A through KEY_Z and KEY_SPACE.
fn is_keyboard(path: &Path) -> bool {
    let Ok(dev) = Device::open(path) else {
        return false;
    };
    let Some(supported) = dev.supported_keys() else {
        return false;
    };
    supported.contains(Key::KEY_A)
        && supported.contains(Key::KEY_Z)
        && supported.contains(Key::KEY_SPACE)
}

fn open_device(path: &Path) -> Result<Device> {
    let dev = Device::open(path).with_context(|| format!("opening {}", path.display()))?;
    use std::os::fd::AsRawFd;
    unsafe {
        let fd = dev.as_raw_fd();
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    Ok(dev)
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
