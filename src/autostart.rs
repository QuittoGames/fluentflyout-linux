//! Autostart integration.
//!
//! Upstream ships a Windows startup task. The Linux equivalent is an XDG
//! autostart entry, created only when the user enables it — a desktop app
//! must never add itself to the session silently.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// File name used inside `$XDG_CONFIG_HOME/autostart`.
const FILE_NAME: &str = "fluentflyout-linux.desktop";

/// Resolves the autostart entry path.
pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(dirs::config_dir)?;
    Some(base.join("autostart").join(FILE_NAME))
}

pub fn is_enabled() -> bool {
    path().is_some_and(|path| path.exists())
}

/// Creates or removes the autostart entry.
///
/// When enabling, the entry points at the binary that is currently running,
/// resolved through `/proc/self/exe`, so a relocated build keeps working.
pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let Some(path) = path() else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no XDG config directory is available",
        ));
    };

    if enabled {
        let executable = std::env::current_exe()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, desktop_entry(&executable))
    } else if path.exists() {
        fs::remove_file(&path)
    } else {
        Ok(())
    }
}

/// Renders the `.desktop` entry.
///
/// `--hidden` keeps the process in the background at login instead of
/// opening the settings window.
pub fn desktop_entry(executable: &Path) -> String {
    let exec = shell_quote(&executable.display().to_string());
    format!(
        "[Desktop Entry]
Type=Application
Name=FluentFlyout
Comment=Media, lock-key and volume flyouts for GNOME
Exec={exec} --hidden
Terminal=false
X-GNOME-Autostart-enabled=true
Categories=Utility;
"
    )
}

/// Quotes a path for a `.desktop` `Exec=` key.
///
/// The specification requires double quotes around arguments containing
/// spaces, with backslash-escaping inside them.
fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".into();
    }
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_contains_the_executable_and_background_flag() {
        let entry = desktop_entry(Path::new("/usr/bin/fluentflyout"));
        assert!(entry.contains("Exec=\"/usr/bin/fluentflyout\" --hidden"));
        assert!(entry.contains("Type=Application"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
    }

    #[test]
    fn paths_with_spaces_are_quoted() {
        let entry = desktop_entry(Path::new("/home/user/My Apps/fluentflyout"));
        assert!(entry.contains("Exec=\"/home/user/My Apps/fluentflyout\" --hidden"));
    }

    #[test]
    fn quotes_and_backslashes_are_escaped() {
        let entry = desktop_entry(Path::new("/tmp/a\"b\\c"));
        assert!(entry.contains(r#"Exec="/tmp/a\"b\\c" --hidden"#));
    }

    #[test]
    fn empty_path_still_produces_a_valid_line() {
        assert_eq!(shell_quote(""), "\"\"");
    }
}
