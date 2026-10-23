//! FluentFlyout for Linux.
//!
//! A native GTK4 / libadwaita reimplementation of the FluentFlyout Windows
//! application. The crate is split so that the platform-independent rules
//! (domain) can be tested without a display server, while the GTK layer only
//! renders state and forwards user intent.
//!
//! Layering (dependencies point downwards only):
//!
//! ```text
//! ui ─────────────► app ─────────────► domain
//!  │                 │
//!  │                 ├── media   (MPRIS over D-Bus)
//!  │                 ├── audio   (WirePlumber / wpctl)
//!  │                 ├── lock_keys (sysfs LEDs + evdev)
//!  │                 └── settings (JSON store)
//!  └────────────────► platform / diagnostics
//! ```

pub mod app;
pub mod audio;
pub mod autostart;
pub mod diagnostics;
pub mod domain;
pub mod lock_keys;
pub mod media;
pub mod platform;
pub mod settings;
pub mod ui;
