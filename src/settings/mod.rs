//! Settings persistence.
//!
//! Writes are debounced (upstream uses a 500 ms debounce) and atomic
//! (write to a temporary file in the same directory, then `rename`), so a
//! crash can never leave a half-written configuration behind.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::domain::settings::Settings;

/// Debounce window applied between a settings change and the disk write.
pub const SAVE_DEBOUNCE_MS: u64 = 500;

/// Owns the on-disk settings file and the in-memory model.
#[derive(Debug)]
pub struct SettingsStore {
    path: PathBuf,
    current: Settings,
}

impl SettingsStore {
    /// Resolves `$XDG_CONFIG_HOME/fluentflyout-linux/settings.json`, falling
    /// back to `~/.config` when the variable is unset.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(dirs::config_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("fluentflyout-linux").join("settings.json")
    }

    /// Loads settings, falling back to defaults when the file is missing or
    /// unreadable. A corrupt file is kept as `settings.json.corrupt` so the
    /// user does not silently lose it.
    pub fn load(path: PathBuf) -> Self {
        let current = match fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str::<Settings>(&contents) {
                Ok(mut settings) => {
                    settings.sanitize();
                    settings
                }
                Err(error) => {
                    log::warn!("settings file {} is invalid: {error}", path.display());
                    quarantine(&path);
                    Settings::default()
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(error) => {
                log::warn!("cannot read {}: {error}", path.display());
                Settings::default()
            }
        };

        Self { path, current }
    }

    pub fn with_default_path() -> Self {
        Self::load(Self::default_path())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn current(&self) -> &Settings {
        &self.current
    }

    /// Applies a mutation, sanitizes the result and persists it.
    ///
    /// Returns `true` when the value actually changed, so callers can avoid
    /// redundant UI refreshes.
    pub fn update<F>(&mut self, mutate: F) -> bool
    where
        F: FnOnce(&mut Settings),
    {
        let mut next = self.current.clone();
        mutate(&mut next);
        next.sanitize();
        if next == self.current {
            return false;
        }
        self.current = next;
        true
    }

    /// Persists the current settings atomically.
    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let serialized = serde_json::to_string_pretty(&self.current)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        // Same directory as the target: `rename` is only atomic within a
        // filesystem, and `/tmp` may be a different mount.
        let temp = self.path.with_extension("json.tmp");
        fs::write(&temp, serialized.as_bytes())?;
        fs::rename(&temp, &self.path)?;
        Ok(())
    }
}

/// Moves an unparseable settings file aside instead of deleting it.
fn quarantine(path: &Path) {
    let backup = path.with_extension("json.corrupt");
    if let Err(error) = fs::rename(path, &backup) {
        log::warn!("could not quarantine {}: {error}", path.display());
    } else {
        log::warn!("kept the previous settings at {}", backup.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::geometry::FlyoutAnchor;

    /// Unique scratch directory per test, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "ffl-test-{label}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = TempDir::new("missing");
        let store = SettingsStore::load(dir.file("settings.json"));
        assert_eq!(store.current(), &Settings::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new("roundtrip");
        let path = dir.file("settings.json");

        let mut store = SettingsStore::load(path.clone());
        assert!(store.update(|settings| {
            settings.position = FlyoutAnchor::TopCenter;
            settings.duration_ms = 1_200;
        }));
        store.save().expect("save");

        let reloaded = SettingsStore::load(path);
        assert_eq!(reloaded.current().position, FlyoutAnchor::TopCenter);
        assert_eq!(reloaded.current().duration_ms, 1_200);
    }

    #[test]
    fn update_reports_whether_anything_changed() {
        let dir = TempDir::new("noop");
        let mut store = SettingsStore::load(dir.file("settings.json"));
        assert!(!store.update(|_| {}));
        assert!(store.update(|settings| settings.duration_ms = 42));
    }

    #[test]
    fn update_sanitizes_before_storing() {
        let dir = TempDir::new("sanitize");
        let mut store = SettingsStore::load(dir.file("settings.json"));
        store.update(|settings| settings.duration_ms = 999_999);
        assert_eq!(
            store.current().duration_ms,
            crate::domain::settings::MAX_DURATION_MS
        );
    }

    #[test]
    fn corrupt_file_falls_back_and_is_quarantined() {
        let dir = TempDir::new("corrupt");
        let path = dir.file("settings.json");
        fs::write(&path, b"{ this is not json").expect("write");

        let store = SettingsStore::load(path.clone());
        assert_eq!(store.current(), &Settings::default());
        assert!(!path.exists(), "corrupt file should be moved aside");
        assert!(dir.file("settings.json.corrupt").exists());
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = TempDir::new("nested");
        let path = dir.file("a/b/c/settings.json");
        let mut store = SettingsStore::load(path.clone());
        store.update(|settings| settings.duration_ms = 900);
        store.save().expect("save should create parents");
        assert!(path.exists());
    }

    #[test]
    fn save_leaves_no_temporary_file_behind() {
        let dir = TempDir::new("tempfile");
        let path = dir.file("settings.json");
        let store = SettingsStore::load(path.clone());
        store.save().expect("save");
        assert!(!dir.file("settings.json.tmp").exists());
    }

    #[test]
    fn unknown_keys_are_tolerated_for_forward_compatibility() {
        let dir = TempDir::new("forward");
        let path = dir.file("settings.json");
        fs::write(&path, br#"{"duration_ms": 2500, "future_option": 7}"#).expect("write");
        let store = SettingsStore::load(path);
        assert_eq!(store.current().duration_ms, 2_500);
    }
}
