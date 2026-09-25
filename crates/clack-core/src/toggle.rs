//! Double-tap toggle detector (§3.5).
//!
//! Accepts a stream of `KeyEvent`s and fires whenever the configured
//! toggle binding is detected.

use std::time::{Duration, Instant};

use crate::{
    config::ToggleBinding,
    types::{KeyCode, KeyEvent, KeyState},
};

/// Timing constants (§3.5).
const MAX_TAP_DURATION: Duration = Duration::from_millis(250);
const MAX_INTER_TAP_GAP: Duration = Duration::from_millis(350);

/// Modifier keycodes (evdev) we care about for toggle detection.
const LEFT_CTRL: KeyCode = 29;
const RIGHT_CTRL: KeyCode = 97;
const LEFT_SHIFT: KeyCode = 42;
const RIGHT_SHIFT: KeyCode = 54;
const LEFT_ALT: KeyCode = 56;
const RIGHT_ALT: KeyCode = 100;

fn is_ctrl(code: KeyCode) -> bool { matches!(code, LEFT_CTRL | RIGHT_CTRL) }
fn is_shift(code: KeyCode) -> bool { matches!(code, LEFT_SHIFT | RIGHT_SHIFT) }
fn is_alt(code: KeyCode) -> bool { matches!(code, LEFT_ALT | RIGHT_ALT) }

/// State machine for a single double-tap gesture.
#[derive(Debug, Default)]
enum TapState {
    #[default]
    Idle,
    /// Saw the first Down; waiting for the first Up.
    FirstDown { at: Instant },
    /// Saw the first tap (Down + Up); waiting for the second Down.
    BetweenTaps { released_at: Instant },
    /// Saw the second Down; waiting for the second Up.
    SecondDown { at: Instant },
}

/// Stateful double-tap detector.
#[derive(Debug, Default)]
pub struct ToggleDetector {
    state: TapState,
    /// Set to true when any non-trigger key is pressed during the gesture.
    dirty: bool,
}

impl ToggleDetector {
    /// Feed a key event. Returns `true` if the configured toggle gesture just completed.
    pub fn feed(&mut self, event: &KeyEvent, binding: &ToggleBinding) -> bool {
        let is_trigger = |code: KeyCode| -> bool {
            match binding {
                ToggleBinding::DoubleCtrl => is_ctrl(code),
                ToggleBinding::DoubleShift => is_shift(code),
                ToggleBinding::DoubleAlt => is_alt(code),
                ToggleBinding::None => false,
            }
        };

        if *binding == ToggleBinding::None {
            return false;
        }

        let now = event.at;

        match event.state {
            KeyState::Down => {
                if !is_trigger(event.code) {
                    // A non-trigger key → abort the gesture
                    self.dirty = true;
                    self.state = TapState::Idle;
                    return false;
                }
                match self.state {
                    TapState::Idle => {
                        self.dirty = false;
                        self.state = TapState::FirstDown { at: now };
                    }
                    TapState::BetweenTaps { released_at } => {
                        if now.duration_since(released_at) <= MAX_INTER_TAP_GAP && !self.dirty {
                            self.state = TapState::SecondDown { at: now };
                        } else {
                            // Too slow → restart
                            self.dirty = false;
                            self.state = TapState::FirstDown { at: now };
                        }
                    }
                    _ => {
                        // Unexpected Down (e.g. two Down without Up) → restart
                        self.dirty = false;
                        self.state = TapState::FirstDown { at: now };
                    }
                }
                false
            }
            KeyState::Up => {
                if !is_trigger(event.code) {
                    return false;
                }
                match self.state {
                    TapState::FirstDown { at } => {
                        if now.duration_since(at) <= MAX_TAP_DURATION {
                            self.state = TapState::BetweenTaps { released_at: now };
                        } else {
                            self.state = TapState::Idle;
                        }
                        false
                    }
                    TapState::SecondDown { at } => {
                        let fired = now.duration_since(at) <= MAX_TAP_DURATION && !self.dirty;
                        self.state = TapState::Idle;
                        self.dirty = false;
                        fired
                    }
                    _ => false,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(code: KeyCode, state: KeyState, at: Instant) -> KeyEvent {
        KeyEvent { code, state, at }
    }

    /// Helper: feed a clean double-tap of left Ctrl with realistic timing.
    fn double_tap(detector: &mut ToggleDetector, start: Instant) -> bool {
        let t0 = start;
        let t1 = t0 + Duration::from_millis(60);  // first up (tap)
        let t2 = t1 + Duration::from_millis(150); // second down (inter-tap)
        let t3 = t2 + Duration::from_millis(60);  // second up

        let binding = ToggleBinding::DoubleCtrl;
        detector.feed(&make_event(LEFT_CTRL, KeyState::Down, t0), &binding);
        detector.feed(&make_event(LEFT_CTRL, KeyState::Up,   t1), &binding);
        detector.feed(&make_event(LEFT_CTRL, KeyState::Down, t2), &binding);
        detector.feed(&make_event(LEFT_CTRL, KeyState::Up,   t3), &binding)
    }

    #[test]
    fn clean_double_tap_fires() {
        let mut d = ToggleDetector::default();
        assert!(double_tap(&mut d, Instant::now()));
    }

    #[test]
    fn slow_inter_tap_does_not_fire() {
        let mut d = ToggleDetector::default();
        let now = Instant::now();
        let binding = ToggleBinding::DoubleCtrl;
        d.feed(&make_event(LEFT_CTRL, KeyState::Down, now), &binding);
        d.feed(&make_event(LEFT_CTRL, KeyState::Up,   now + Duration::from_millis(60)), &binding);
        // Gap > 350 ms
        d.feed(&make_event(LEFT_CTRL, KeyState::Down, now + Duration::from_millis(500)), &binding);
        let fired = d.feed(&make_event(LEFT_CTRL, KeyState::Up, now + Duration::from_millis(560)), &binding);
        // Should NOT fire (gap too long) — the second press restarts the sequence
        assert!(!fired);
    }

    #[test]
    fn key_in_between_aborts() {
        let mut d = ToggleDetector::default();
        let now = Instant::now();
        let binding = ToggleBinding::DoubleCtrl;
        d.feed(&make_event(LEFT_CTRL, KeyState::Down, now), &binding);
        d.feed(&make_event(LEFT_CTRL, KeyState::Up,   now + Duration::from_millis(50)), &binding);
        // A non-trigger key in the gap
        d.feed(&make_event(30 /* KEY_A */, KeyState::Down, now + Duration::from_millis(100)), &binding);
        d.feed(&make_event(LEFT_CTRL, KeyState::Down, now + Duration::from_millis(150)), &binding);
        let fired = d.feed(&make_event(LEFT_CTRL, KeyState::Up, now + Duration::from_millis(200)), &binding);
        assert!(!fired);
    }

    #[test]
    fn binding_none_never_fires() {
        let mut d = ToggleDetector::default();
        let now = Instant::now();
        let binding = ToggleBinding::None;
        d.feed(&make_event(LEFT_CTRL, KeyState::Down, now), &binding);
        let fired = d.feed(&make_event(LEFT_CTRL, KeyState::Up, now + Duration::from_millis(50)), &binding);
        assert!(!fired);
    }

    #[test]
    fn consecutive_double_taps_both_fire() {
        let mut d = ToggleDetector::default();
        let now = Instant::now();
        let first = double_tap(&mut d, now);
        let second = double_tap(&mut d, now + Duration::from_millis(800));
        assert!(first);
        assert!(second);
    }
}
