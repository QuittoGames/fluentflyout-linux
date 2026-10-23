//! Platform-independent domain model.
//!
//! Nothing in this module knows about GTK, D-Bus or the filesystem layout of
//! the host. That makes the rules directly unit-testable.

pub mod geometry;
pub mod lock_keys;
pub mod media;
pub mod settings;
pub mod volume;
