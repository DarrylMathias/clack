//! Linux evdev input backend (§5.1) — stub for P0; full implementation in P1.1.

use std::sync::{atomic::AtomicBool, Arc};

use clack_core::types::{AccessError, DeviceInfo, KeyEvent};
use crossbeam_channel::Sender;

use crate::InputBackend;

pub struct LinuxInput;

impl InputBackend for LinuxInput {
    fn run(&mut self, _tx: Sender<KeyEvent>, _stop: Arc<AtomicBool>) -> anyhow::Result<()> {
        // TODO (P1.1): enumerate /dev/input/event*, filter keyboards, epoll, hotplug via inotify
        unimplemented!("Linux evdev backend — implement in P1.1")
    }

    fn probe() -> Result<Vec<DeviceInfo>, AccessError> {
        // TODO (P1.1): scan /dev/input/event* and check capabilities + permissions
        unimplemented!("Linux evdev probe — implement in P1.1")
    }
}
