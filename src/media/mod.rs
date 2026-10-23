//! Media integration: MPRIS over the session D-Bus.
//!
//! Windows uses `GlobalSystemMediaTransportControlsSessionManager`, which
//! hands the app a single "current" session plus rich metadata. On Linux the
//! equivalent is MPRIS: every player exports the same object path
//! (`/org/mpris/MediaPlayer2`) and the same interface
//! (`org.mpris.MediaPlayer2.Player`), so the app must do its own discovery
//! and arbitration between players.
//!
//! The service runs on a dedicated thread with a single-threaded async
//! executor. It never touches GTK; it publishes [`AppEvent`]s through a
//! channel and accepts [`MediaCommand`]s back.

pub mod mpris;

pub use mpris::{MediaCommand, MediaService, ServiceStatus};
