//! macOS CGEventTap input backend (§5.2) — stub for P0; full implementation in P1.2.

use std::sync::{atomic::AtomicBool, Arc};

use clack_core::types::{AccessError, DeviceInfo, KeyEvent};
use crossbeam_channel::Sender;

use crate::InputBackend;

pub struct MacosInput;

impl InputBackend for MacosInput {
    fn run(&mut self, _tx: Sender<KeyEvent>, _stop: Arc<AtomicBool>) -> anyhow::Result<()> {
        // TODO (P1.2): CGEventTapCreate + CFRunLoop + flagsChanged + keyDown/Up
        unimplemented!("macOS CGEventTap backend — implement in P1.2")
    }

    fn probe() -> Result<Vec<DeviceInfo>, AccessError> {
        // TODO (P1.2): CGPreflightListenEventAccess()
        unimplemented!("macOS probe — implement in P1.2")
    }
}
