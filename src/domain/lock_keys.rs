//! Lock-key domain model.
//!
//! Windows exposes `Keyboard.IsKeyToggled` and a low-level keyboard hook.
//! Linux has no equivalent "ask the desktop" API: the toggle state lives in
//! the kernel input subsystem, surfaced either as an LED class device or as
//! evdev events. Both are represented here behind a small trait so that the
//! GTK layer never talks to `/sys` or `/dev` directly.

use serde::{Deserialize, Serialize};

/// The keys the upstream application reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockKeyKind {
    Caps,
    Num,
    Scroll,
    /// `Insert` is not a toggle in Linux; upstream only reports its state.
    Insert,
}

impl LockKeyKind {
    pub const ALL: [Self; 4] = [Self::Caps, Self::Num, Self::Scroll, Self::Insert];

    /// Linux evdev key codes (`linux/input-event-codes.h`).
    pub fn evdev_code(self) -> u16 {
        match self {
            Self::Caps => 58,    // KEY_CAPSLOCK
            Self::Num => 69,     // KEY_NUMLOCK
            Self::Scroll => 70,  // KEY_SCROLLLOCK
            Self::Insert => 110, // KEY_INSERT
        }
    }

    /// Suffix used by LED class devices, e.g. `input3::capslock`.
    pub fn led_name(self) -> Option<&'static str> {
        match self {
            Self::Caps => Some("capslock"),
            Self::Num => Some("numlock"),
            Self::Scroll => Some("scrolllock"),
            // The kernel exposes no LED for Insert.
            Self::Insert => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Caps => "Caps Lock",
            Self::Num => "Num Lock",
            Self::Scroll => "Scroll Lock",
            Self::Insert => "Insert",
        }
    }
}

/// One decoded `input_event` from an evdev device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEvent {
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

impl InputEvent {
    /// Size of `struct input_event` on 64-bit Linux: two `timeval` fields
    /// (16 bytes) plus `__u16 type`, `__u16 code`, `__s32 value`.
    pub const SIZE: usize = 24;

    /// Decodes a little-endian `input_event`. Returns `None` for short slices.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        let kind = u16::from_le_bytes([bytes[16], bytes[17]]);
        let code = u16::from_le_bytes([bytes[18], bytes[19]]);
        let value = i32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        Some(Self { kind, code, value })
    }

    pub const EV_KEY: u16 = 0x01;
    /// `EV_LED`: the kernel reports the *new* LED state, which is exactly the
    /// toggle state of Caps/Num/Scroll Lock.
    pub const EV_LED: u16 = 0x11;

    /// `1` = key press, `0` = release, `2` = autorepeat.
    pub fn is_key_press(self) -> bool {
        self.kind == Self::EV_KEY && self.value == 1
    }

    /// Maps a key code to a tracked lock key, if any.
    pub fn lock_key(self) -> Option<LockKeyKind> {
        LockKeyKind::ALL
            .into_iter()
            .find(|kind| kind.evdev_code() == self.code)
    }

    /// Decodes an `EV_LED` event into `(key, on)`.
    ///
    /// LED codes come from `linux/input-event-codes.h` and are *not* the same
    /// as key codes: `LED_NUML` is 0, `LED_CAPSL` is 1, `LED_SCROLLL` is 2.
    pub fn led_state(self) -> Option<(LockKeyKind, bool)> {
        if self.kind != Self::EV_LED {
            return None;
        }
        let kind = match self.code {
            0 => LockKeyKind::Num,
            1 => LockKeyKind::Caps,
            2 => LockKeyKind::Scroll,
            _ => return None,
        };
        Some((kind, self.value != 0))
    }
}

/// Why lock-key support may be degraded, surfaced in diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockKeySupport {
    /// Both LED state and evdev events are available.
    Full,
    /// LED state is readable but key presses cannot be observed (the
    /// process lacks read access to `/dev/input/event*`).
    StateOnly,
    /// Neither mechanism works on this system.
    Unavailable { reason: String },
}

impl LockKeySupport {
    pub fn is_available(&self) -> bool {
        !matches!(self, Self::Unavailable { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evdev_codes_match_the_kernel_header() {
        assert_eq!(LockKeyKind::Caps.evdev_code(), 58);
        assert_eq!(LockKeyKind::Num.evdev_code(), 69);
        assert_eq!(LockKeyKind::Scroll.evdev_code(), 70);
        assert_eq!(LockKeyKind::Insert.evdev_code(), 110);
    }

    #[test]
    fn insert_has_no_led_device() {
        assert_eq!(LockKeyKind::Insert.led_name(), None);
        assert_eq!(LockKeyKind::Caps.led_name(), Some("capslock"));
    }

    #[test]
    fn input_event_decoding_reads_little_endian_fields() {
        let mut bytes = [0u8; InputEvent::SIZE];
        // type = EV_KEY, code = KEY_CAPSLOCK, value = 1
        bytes[16..18].copy_from_slice(&1u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&58u16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1i32.to_le_bytes());

        let event = InputEvent::from_bytes(&bytes).expect("full event");
        assert_eq!(
            event,
            InputEvent {
                kind: InputEvent::EV_KEY,
                code: 58,
                value: 1
            }
        );
        assert!(event.is_key_press());
        assert_eq!(event.lock_key(), Some(LockKeyKind::Caps));
    }

    #[test]
    fn short_slices_are_rejected_instead_of_panicking() {
        assert!(InputEvent::from_bytes(&[0u8; 10]).is_none());
        assert!(InputEvent::from_bytes(&[]).is_none());
    }

    #[test]
    fn non_key_events_are_not_treated_as_presses() {
        let event = InputEvent {
            kind: 0x02, // EV_REL
            code: 58,
            value: 1,
        };
        assert!(!event.is_key_press());
    }

    #[test]
    fn key_release_is_not_a_press() {
        let event = InputEvent {
            kind: InputEvent::EV_KEY,
            code: 58,
            value: 0,
        };
        assert!(!event.is_key_press());
    }

    #[test]
    fn unknown_codes_map_to_no_lock_key() {
        let event = InputEvent {
            kind: InputEvent::EV_KEY,
            code: 30, // KEY_A
            value: 1,
        };
        assert_eq!(event.lock_key(), None);
    }

    #[test]
    fn led_events_report_the_new_toggle_state() {
        // LED codes are not key codes: LED_NUML=0, LED_CAPSL=1, LED_SCROLLL=2.
        for (code, kind) in [
            (0u16, LockKeyKind::Num),
            (1u16, LockKeyKind::Caps),
            (2u16, LockKeyKind::Scroll),
        ] {
            assert_eq!(
                InputEvent {
                    kind: InputEvent::EV_LED,
                    code,
                    value: 1
                }
                .led_state(),
                Some((kind, true))
            );
            assert_eq!(
                InputEvent {
                    kind: InputEvent::EV_LED,
                    code,
                    value: 0
                }
                .led_state(),
                Some((kind, false))
            );
        }
    }

    #[test]
    fn key_events_are_not_led_events() {
        let event = InputEvent {
            kind: InputEvent::EV_KEY,
            code: 1,
            value: 1,
        };
        assert_eq!(event.led_state(), None);
    }

    #[test]
    fn unknown_led_codes_are_ignored() {
        let event = InputEvent {
            kind: InputEvent::EV_LED,
            code: 9,
            value: 1,
        };
        assert_eq!(event.led_state(), None);
    }
}
