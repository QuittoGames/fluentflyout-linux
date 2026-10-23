# Provenance and licensing

## Upstream

- Project: **FluentFlyout** — <https://github.com/unchihugo/FluentFlyout>
- Revision analysed: `309e5ea6708a5e8f928990acd24ff525b370da88` (2026-10-20)
- License: **GPL-3.0-or-later**
- Local clone used for the reverse-engineering pass: `%TEMP%\fluentflyout` (read-only;
  the upstream repository was never modified, committed to or pushed).

## What was reused

**Measured values and behaviour**, extracted by reading the upstream source:

| Item | Upstream location |
|---|---|
| Flyout geometry (`310×112`, width formula, compact `400×60`) | `MainWindow.xaml.cs` → `UpdateUILayout` |
| Position arithmetic, 16 px margin, 80 px OSD reservation | `MainWindow.xaml.cs` → `GetFinalPosition` |
| Animation duration table and easing styles | `MainWindow.xaml.cs` → `getDuration`, `UserSettings` |
| Visibility timing, hover-pause, debounce | `MainWindow.xaml.cs` → `ShowMediaFlyout`, `StartHideAnimation` |
| Settings keys and defaults | `UserSettings.cs`, `SettingsManager.cs` |
| Lock flyout layout (`160×50`, 60×3 bar) | `LockWindow.xaml` |
| Seekbar heuristic (`length ≥ 1 s`) | `MainWindow.xaml.cs` |
| Feature list and interaction order | application source and `README.md` |

These are functional specifications, not expressive code. Reproducing the dimensions,
timings and rules of an interface is what makes this a port rather than a different app.

**No upstream source code was copied.** All Rust in `src/` was written for this project.
The C# and XAML were read to derive behaviour; no file was translated line by line, and
no upstream file exists verbatim in this repository.

**No upstream assets were reused.** There are no icons, images, fonts, sounds or
localisation strings from FluentFlyout in this repository:

- icons come from the system **Adwaita** icon theme via named icon lookups
  (`media-skip-backward-symbolic`, `media-playback-start-symbolic`, …);
- the cover image used in screenshots is a gradient generated for testing
  (`.artifacts/test-cover.png`);
- every screenshot in `.artifacts/screenshots/` was rendered by this application itself
  through `--screenshot`.

## License of this project

**GPL-3.0-or-later**, the same license as the upstream work, which keeps the obligations
simple and unambiguous: this is an independent implementation of the same interface, and
choosing the matching license avoids any question about derivative-work status.

Full text: [`LICENSE`](../LICENSE).

Obligations if you distribute binaries:

- ship the complete corresponding source;
- keep the copyright and license notices;
- state significant changes made to the original (this project is a reimplementation for
  a different platform, not a modified copy — say so);
- do not impose additional restrictions.

## Third-party Rust crates

| Crate | License |
|---|---|
| `gtk4`, `glib`, `gio`, `gdk4` (gtk-rs) | MIT |
| `libadwaita` (gtk-rs) | MIT |
| `zbus`, `zvariant` | MIT |
| `async-channel`, `futures-util` | MIT OR Apache-2.0 |
| `serde`, `serde_json` | MIT OR Apache-2.0 |
| `anyhow`, `thiserror` | MIT OR Apache-2.0 |
| `log` | MIT OR Apache-2.0 |
| `dirs` | MIT OR Apache-2.0 |
| `sha2` | MIT OR Apache-2.0 |
| `gtk4-layer-shell` (optional feature) | MIT |

The full dependency graph and exact versions are pinned in `Cargo.lock`.

## Attribution in the UI

The About group of the settings window states that this is a Linux reimplementation of
FluentFlyout by *unchihugo*, links the upstream repository and names the license, so the
attribution travels with the application rather than living only in this file.
