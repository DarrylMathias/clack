//! macOS CGEventTap input backend (§5.2).
//!
//! Uses a listen-only CGEventTap on keyDown, keyUp, and flagsChanged events,
//! run on its own CFRunLoop thread. The tap never intercepts or modifies events.
//!
//! Key mapping: macOS virtual keycodes (kVK_*) are translated to evdev KEY_*
//! codes via `clack_core::keymap::macos_vk_to_evdev`.
//!
//! Permission: CGPreflightListenEventAccess() / CGRequestListenEventAccess().
//! If permission is denied, `probe()` returns AccessError::PermissionDenied.
//!
//! Tap timeouts: if the system disables the tap (kCGEventTapDisabledByTimeout),
//! we re-enable it and keep running.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Instant,
};

use anyhow::Result;
use clack_core::{
    keymap::macos_vk_to_evdev,
    types::{AccessError, DeviceInfo, KeyEvent, KeyState},
};
use crossbeam_channel::Sender;
use tracing::{error, info, warn};

use crate::InputBackend;

// ── Core-Graphics / Core-Foundation bindings ─────────────────────────────────
// We call these via raw FFI so that the crate compiles on Linux CI.
// On macOS the symbols are provided by the CoreGraphics and CoreFoundation frameworks.

#![allow(non_upper_case_globals)]

#[allow(non_camel_case_types)]
mod ffi {
    use std::ffi::c_void;

    pub type Boolean = u8;
    pub type CGEventType = u32;
    pub type CGEventField = u32;
    pub type CGKeyCode = u16;
    pub type CFMachPortRef = *mut c_void;
    pub type CFRunLoopSourceRef = *mut c_void;
    pub type CFRunLoopRef = *mut c_void;
    pub type CGEventRef = *mut c_void;
    pub type CGEventTapProxy = *mut c_void;
    pub type CFStringRef = *const c_void;
    pub type CGEventFlags = u64;

    // CGEventType values we care about
    pub const kCGEventKeyDown: CGEventType = 10;
    pub const kCGEventKeyUp: CGEventType = 11;
    pub const kCGEventFlagsChanged: CGEventType = 12;
    pub const kCGEventTapDisabledByTimeout: CGEventType = 0xFFFFFFFE;
    // A mask that covers keyDown | keyUp | flagsChanged
    pub const KEY_EVENT_MASK: u64 =
        (1 << kCGEventKeyDown) | (1 << kCGEventKeyUp) | (1 << kCGEventFlagsChanged);

    // CGEventField values
    pub const kCGKeyboardEventKeycode: CGEventField = 9;
    pub const kCGKeyboardEventAutorepeat: CGEventField = 11;

    // CGEventTapLocation
    pub const kCGSessionEventTap: u32 = 1;
    // CGEventTapPlacement
    pub const kCGHeadInsertEventTap: u32 = 0;
    // CGEventTapOptions (listen-only)
    pub const kCGEventTapOptionListenOnly: u32 = 1;

    // CFRunLoopMode string (kCFRunLoopCommonModes)
    extern "C" {
        pub static kCFRunLoopCommonModes: CFStringRef;
    }

    pub type CGEventTapCallBack = unsafe extern "C" fn(
        proxy: CGEventTapProxy,
        event_type: CGEventType,
        event: CGEventRef,
        user_info: *mut std::ffi::c_void,
    ) -> CGEventRef;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGPreflightListenEventAccess() -> Boolean;
        pub fn CGRequestListenEventAccess() -> Boolean;
        pub fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: CGEventTapCallBack,
            user_info: *mut std::ffi::c_void,
        ) -> CFMachPortRef;
        pub fn CGEventTapEnable(tap: CFMachPortRef, enable: Boolean);
        pub fn CGEventGetIntegerValueField(event: CGEventRef, field: CGEventField) -> i64;
        pub fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFMachPortCreateRunLoopSource(
            allocator: *const std::ffi::c_void,
            port: CFMachPortRef,
            order: std::ffi::c_long,
        ) -> CFRunLoopSourceRef;
        pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        pub fn CFRunLoopRun();
        pub fn CFRunLoopStop(rl: CFRunLoopRef);
        pub fn CFRelease(obj: *const std::ffi::c_void);
    }
}

// ── Shared callback state ─────────────────────────────────────────────────────

/// Data shared between the CGEventTap callback and the driver thread.
/// Must be heap-allocated and stable (we pass a raw pointer to the C callback).
struct TapState {
    tx: Sender<KeyEvent>,
    stop: Arc<AtomicBool>,
    tap: std::cell::UnsafeCell<ffi::CFMachPortRef>,
    /// Last seen CGEventFlags, for deriving modifier Up/Down from flagsChanged.
    last_flags: AtomicU32,
}

// Safety: TapState is accessed only from the CFRunLoop thread (single-threaded CFRunLoop).
unsafe impl Send for TapState {}
unsafe impl Sync for TapState {}

// CGEventFlags bits for the modifiers we care about (from HIToolbox/Events.h)
const FLAG_SHIFT: u64 = 1 << 17;
const FLAG_CTRL: u64 = 1 << 18;
const FLAG_ALT: u64 = 1 << 19;
const FLAG_CMD: u64 = 1 << 20;
const FLAG_CAPS: u64 = 1 << 16;
const FLAG_FN: u64 = 1 << 23;

/// Map a CGEventFlags bit to the evdev keycode of the corresponding modifier.
/// We only care about the modifier keys that clack-core's toggle detector
/// and pack key maps use.
fn flags_bit_to_vk(bit: u64) -> Option<u16> {
    match bit {
        FLAG_SHIFT => Some(0x38), // kVK_Shift
        FLAG_CTRL => Some(0x3B),  // kVK_Control
        FLAG_ALT => Some(0x3A),   // kVK_Option
        FLAG_CMD => Some(0x37),   // kVK_Command
        FLAG_CAPS => Some(0x39),  // kVK_CapsLock
        _ => None,
    }
}

const MODIFIER_BITS: &[u64] = &[
    FLAG_SHIFT, FLAG_CTRL, FLAG_ALT, FLAG_CMD, FLAG_CAPS, FLAG_FN,
];

// ── CGEventTap callback ───────────────────────────────────────────────────────

unsafe extern "C" fn tap_callback(
    proxy: ffi::CGEventTapProxy,
    event_type: ffi::CGEventType,
    event: ffi::CGEventRef,
    user_info: *mut std::ffi::c_void,
) -> ffi::CGEventRef {
    let state = &*(user_info as *const TapState);

    if state.stop.load(Ordering::Relaxed) {
        // Signal the run loop to stop.
        unsafe { ffi::CFRunLoopStop(ffi::CFRunLoopGetCurrent()) };
        return event;
    }

    if event_type == ffi::kCGEventTapDisabledByTimeout {
        // Re-enable the tap so we keep receiving events.
        let tap = *state.tap.get();
        if !tap.is_null() {
            unsafe { ffi::CGEventTapEnable(tap, 1) };
            warn!("CGEventTap re-enabled after timeout");
        }
        return event;
    }

    match event_type {
        ffi::kCGEventKeyDown | ffi::kCGEventKeyUp => {
            // Drop auto-repeat events.
            let is_repeat =
                unsafe { ffi::CGEventGetIntegerValueField(event, ffi::kCGKeyboardEventAutorepeat) };
            if is_repeat != 0 {
                return event;
            }

            let vk = unsafe {
                ffi::CGEventGetIntegerValueField(event, ffi::kCGKeyboardEventKeycode) as u16
            };
            if let Some(code) = macos_vk_to_evdev(vk) {
                let key_state = if event_type == ffi::kCGEventKeyDown {
                    KeyState::Down
                } else {
                    KeyState::Up
                };
                let ev = KeyEvent {
                    code,
                    state: key_state,
                    at: Instant::now(),
                };
                // Best-effort send; if the channel is closed, we just drop.
                let _ = state.tx.send(ev);
            }
        }
        ffi::kCGEventFlagsChanged => {
            // Derive modifier Up/Down by diffing flags.
            let new_flags = unsafe { ffi::CGEventGetFlags(event) };
            let old_flags = state.last_flags.load(Ordering::Relaxed) as u64;
            state.last_flags.store(new_flags as u32, Ordering::Relaxed);

            for &bit in MODIFIER_BITS {
                let was_set = old_flags & bit != 0;
                let is_set = new_flags & bit != 0;
                if was_set == is_set {
                    continue;
                }
                let vk = match flags_bit_to_vk(bit) {
                    Some(v) => v,
                    None => continue,
                };
                if let Some(code) = macos_vk_to_evdev(vk) {
                    let key_state = if is_set { KeyState::Down } else { KeyState::Up };
                    let ev = KeyEvent {
                        code,
                        state: key_state,
                        at: Instant::now(),
                    };
                    let _ = state.tx.send(ev);
                }
            }
        }
        _ => {}
    }

    event
}

// ── InputBackend impl ─────────────────────────────────────────────────────────

pub struct MacosInput;

impl InputBackend for MacosInput {
    fn run(&mut self, tx: Sender<KeyEvent>, stop: Arc<AtomicBool>) -> Result<()> {
        // Check permission first.
        let has_access = unsafe { ffi::CGPreflightListenEventAccess() != 0 };
        if !has_access {
            // Trigger the system prompt (will not block; user must go to System Settings).
            unsafe { ffi::CGRequestListenEventAccess() };
            // Give the user 30 s to grant access, polling every 500 ms.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                if unsafe { ffi::CGPreflightListenEventAccess() != 0 } {
                    break;
                }
                if std::time::Instant::now() > deadline || stop.load(Ordering::Relaxed) {
                    return Err(anyhow::anyhow!(
                        "Input Monitoring permission not granted. \
                         Open System Settings → Privacy & Security → Input Monitoring \
                         and enable clack."
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }

        // Allocate TapState on the heap and keep a stable pointer.
        let state = Box::new(TapState {
            tx,
            stop,
            tap: std::cell::UnsafeCell::new(std::ptr::null_mut()),
            last_flags: AtomicU32::new(0),
        });
        let state_ptr = Box::into_raw(state);

        // Create the event tap.
        let tap = unsafe {
            ffi::CGEventTapCreate(
                ffi::kCGSessionEventTap,
                ffi::kCGHeadInsertEventTap,
                ffi::kCGEventTapOptionListenOnly,
                ffi::KEY_EVENT_MASK,
                tap_callback,
                state_ptr as *mut _,
            )
        };

        if tap.is_null() {
            // Safety: we own state_ptr; reconstitute the Box so it gets dropped.
            unsafe { drop(Box::from_raw(state_ptr)) };
            return Err(anyhow::anyhow!(
                "CGEventTapCreate returned null. \
                 Make sure clack has Input Monitoring permission."
            ));
        }

        // Store the tap handle back into TapState so the callback can re-enable it.
        unsafe { *(*state_ptr).tap.get() = tap };

        info!("macOS CGEventTap created");

        // Wire the tap into a CFRunLoopSource and run the loop on this thread.
        unsafe {
            let source = ffi::CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            let rl = ffi::CFRunLoopGetCurrent();
            ffi::CFRunLoopAddSource(rl, source, ffi::kCFRunLoopCommonModes);
            ffi::CGEventTapEnable(tap, 1);
            ffi::CFRunLoopRun(); // blocks until CFRunLoopStop is called
            ffi::CFRelease(source as _);
            ffi::CFRelease(tap as _);
        }

        // Reconstitute and drop TapState.
        unsafe { drop(Box::from_raw(state_ptr)) };
        info!("macOS CGEventTap stopped");
        Ok(())
    }

    fn probe() -> Result<Vec<DeviceInfo>, AccessError> {
        let has_access = unsafe { ffi::CGPreflightListenEventAccess() != 0 };
        if !has_access {
            return Err(AccessError::PermissionDenied {
                device: "CGEventTap (Input Monitoring)".into(),
            });
        }
        // On macOS we don't enumerate individual keyboard nodes — there's just one tap.
        Ok(vec![DeviceInfo {
            path: "CGEventTap".into(),
            name: "System keyboard (CGEventTap)".into(),
        }])
    }
}
