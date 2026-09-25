//! Keycode translation tables (§5.3).
//!
//! Canonical keycode = Linux evdev KEY_* value (D15).
//! All input backends normalize to these codes before events leave the backend.

use crate::types::KeyCode;

// ── macOS virtual keycode → evdev (§5.2 / §5.3) ─────────────────────────────
//
// Sources:
//   - <HIToolbox/Events.h> kVK_* constants
//   - Linux <linux/input-event-codes.h> KEY_* constants

/// Map a macOS virtual keycode (kVK_*) to an evdev KEY_* code.
/// Returns None for keycodes that have no evdev equivalent (e.g. Fn, brightness).
pub fn macos_vk_to_evdev(vk: u16) -> Option<KeyCode> {
    // Common ANSI letter keys (macOS kVK == USB HID usage - differs from evdev)
    let evdev = match vk {
        0x00 => 30, // kVK_ANSI_A → KEY_A
        0x0B => 48, // kVK_ANSI_B → KEY_B
        0x08 => 46, // kVK_ANSI_C → KEY_C
        0x02 => 32, // kVK_ANSI_D → KEY_D
        0x0E => 18, // kVK_ANSI_E → KEY_E
        0x03 => 33, // kVK_ANSI_F → KEY_F
        0x05 => 34, // kVK_ANSI_G → KEY_G
        0x04 => 35, // kVK_ANSI_H → KEY_H
        0x22 => 23, // kVK_ANSI_I → KEY_I
        0x26 => 36, // kVK_ANSI_J → KEY_J
        0x28 => 37, // kVK_ANSI_K → KEY_K
        0x25 => 38, // kVK_ANSI_L → KEY_L
        0x2E => 50, // kVK_ANSI_M → KEY_M
        0x2D => 49, // kVK_ANSI_N → KEY_N
        0x1F => 24, // kVK_ANSI_O → KEY_O
        0x23 => 25, // kVK_ANSI_P → KEY_P
        0x0C => 16, // kVK_ANSI_Q → KEY_Q
        0x0F => 19, // kVK_ANSI_R → KEY_R
        0x01 => 31, // kVK_ANSI_S → KEY_S
        0x11 => 20, // kVK_ANSI_T → KEY_T
        0x20 => 22, // kVK_ANSI_U → KEY_U
        0x09 => 47, // kVK_ANSI_V → KEY_V
        0x0D => 17, // kVK_ANSI_W → KEY_W
        0x07 => 45, // kVK_ANSI_X → KEY_X
        0x10 => 21, // kVK_ANSI_Y → KEY_Y
        0x06 => 44, // kVK_ANSI_Z → KEY_Z

        // Number row
        0x12 => 2,  // 1
        0x13 => 3,  // 2
        0x14 => 4,  // 3
        0x15 => 5,  // 4
        0x17 => 6,  // 5
        0x16 => 7,  // 6
        0x1A => 8,  // 7
        0x1C => 9,  // 8
        0x19 => 10, // 9
        0x1D => 11, // 0

        // Punctuation / symbols
        0x29 => 39, // kVK_ANSI_Semicolon → KEY_SEMICOLON
        0x27 => 40, // kVK_ANSI_Quote → KEY_APOSTROPHE
        0x2A => 43, // kVK_ANSI_Backslash → KEY_BACKSLASH
        0x2B => 51, // kVK_ANSI_Comma → KEY_COMMA
        0x2F => 52, // kVK_ANSI_Period → KEY_DOT
        0x2C => 53, // kVK_ANSI_Slash → KEY_SLASH
        0x1B => 12, // kVK_ANSI_Minus → KEY_MINUS
        0x18 => 13, // kVK_ANSI_Equal → KEY_EQUAL
        0x21 => 26, // kVK_ANSI_LeftBracket → KEY_LEFTBRACE
        0x1E => 27, // kVK_ANSI_RightBracket → KEY_RIGHTBRACE
        0x32 => 41, // kVK_ANSI_Grave → KEY_GRAVE

        // Control keys
        0x31 => 57,  // kVK_Space → KEY_SPACE
        0x33 => 14,  // kVK_Delete (Backspace) → KEY_BACKSPACE
        0x24 => 28,  // kVK_Return → KEY_ENTER
        0x30 => 15,  // kVK_Tab → KEY_TAB
        0x35 => 1,   // kVK_Escape → KEY_ESC
        0x75 => 111, // kVK_ForwardDelete → KEY_DELETE
        0x73 => 102, // kVK_Home → KEY_HOME
        0x77 => 107, // kVK_End → KEY_END
        0x74 => 104, // kVK_PageUp → KEY_PAGEUP
        0x79 => 109, // kVK_PageDown → KEY_PAGEDOWN

        // Arrow keys
        0x7B => 105, // kVK_LeftArrow → KEY_LEFT
        0x7C => 106, // kVK_RightArrow → KEY_RIGHT
        0x7E => 103, // kVK_UpArrow → KEY_UP
        0x7D => 108, // kVK_DownArrow → KEY_DOWN

        // Modifier keys
        0x38 => 42,  // kVK_Shift (left) → KEY_LEFTSHIFT
        0x3C => 54,  // kVK_RightShift → KEY_RIGHTSHIFT
        0x3B => 29,  // kVK_Control (left) → KEY_LEFTCTRL
        0x3E => 97,  // kVK_RightControl → KEY_RIGHTCTRL
        0x3A => 56,  // kVK_Option/Alt (left) → KEY_LEFTALT
        0x3D => 100, // kVK_RightOption → KEY_RIGHTALT
        0x37 => 125, // kVK_Command (left) → KEY_LEFTMETA
        0x36 => 126, // kVK_RightCommand → KEY_RIGHTMETA
        0x39 => 58,  // kVK_CapsLock → KEY_CAPSLOCK

        // Function keys
        0x7A => 59,  // kVK_F1
        0x78 => 60,  // kVK_F2
        0x63 => 61,  // kVK_F3
        0x76 => 62,  // kVK_F4
        0x60 => 63,  // kVK_F5
        0x61 => 64,  // kVK_F6
        0x62 => 65,  // kVK_F7
        0x64 => 66,  // kVK_F8
        0x65 => 67,  // kVK_F9
        0x6D => 68,  // kVK_F10
        0x67 => 87,  // kVK_F11
        0x6F => 88,  // kVK_F12

        _ => return None,
    };
    Some(evdev)
}

// ── Mechvibes (libuiohook VC_*) → evdev (§5.3) ───────────────────────────────
//
// Basic keys are identical (PC set-1 scancodes match evdev for most keys).
// Extended (0xE0xx) keys need an explicit table.

/// Map a Mechvibes / libuiohook VC_* code to an evdev KEY_* code.
/// Returns None for unsupported or unmapped codes.
pub fn mechvibes_to_evdev(vc: u32) -> Option<KeyCode> {
    // Most basic keys are identical — pass through if in the normal range
    if vc < 0x80 {
        return Some(vc as KeyCode);
    }

    // Extended keys (libuiohook stores them as 0xE0xx where xx is the extended scancode)
    let evdev = match vc {
        0xE048 => 103, // VC_UP → KEY_UP
        0xE050 => 108, // VC_DOWN → KEY_DOWN
        0xE04B => 105, // VC_LEFT → KEY_LEFT
        0xE04D => 106, // VC_RIGHT → KEY_RIGHT
        0xE047 => 102, // VC_HOME → KEY_HOME
        0xE04F => 107, // VC_END → KEY_END
        0xE049 => 104, // VC_PAGE_UP → KEY_PAGEUP
        0xE051 => 109, // VC_PAGE_DOWN → KEY_PAGEDOWN
        0xE052 => 110, // VC_INSERT → KEY_INSERT
        0xE053 => 111, // VC_DELETE → KEY_DELETE
        0xE035 => 98,  // VC_KP_DIVIDE → KEY_KPSLASH
        0xE01C => 96,  // VC_KP_ENTER → KEY_KPENTER
        0xE05B => 125, // VC_META_L → KEY_LEFTMETA
        0xE05C => 126, // VC_META_R → KEY_RIGHTMETA
        0xE038 => 100, // VC_ALT_R → KEY_RIGHTALT
        0xE01D => 97,  // VC_CONTROL_R → KEY_RIGHTCTRL
        _ => return None,
    };
    Some(evdev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_ansi_letters_map() {
        // A through Z should all map to something
        let letter_vks: &[u16] = &[
            0x00, 0x0B, 0x08, 0x02, 0x0E, 0x03, 0x05, 0x04, 0x22, 0x26,
            0x28, 0x25, 0x2E, 0x2D, 0x1F, 0x23, 0x0C, 0x0F, 0x01, 0x11,
            0x20, 0x09, 0x0D, 0x07, 0x10, 0x06,
        ];
        for &vk in letter_vks {
            assert!(
                macos_vk_to_evdev(vk).is_some(),
                "macOS vk 0x{vk:02X} should map to an evdev code"
            );
        }
    }

    #[test]
    fn mechvibes_basic_passthrough() {
        // KEY_A = 30 in both worlds
        assert_eq!(mechvibes_to_evdev(30), Some(30));
        // KEY_SPACE = 57
        assert_eq!(mechvibes_to_evdev(57), Some(57));
    }

    #[test]
    fn mechvibes_extended_keys() {
        // VC_UP (0xE048) should map to KEY_UP (103)
        assert_eq!(mechvibes_to_evdev(0xE048), Some(103));
        // VC_DOWN (0xE050) → KEY_DOWN (108)
        assert_eq!(mechvibes_to_evdev(0xE050), Some(108));
    }
}
