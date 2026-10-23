# FluentFlyout for Linux

A native Linux reimplementation of [FluentFlyout](https://github.com/unchihugo/FluentFlyout):
media, lock-key and volume flyouts built with **Rust + GTK4 + libadwaita**, rendering
through **GTK's GSK Vulkan renderer** where the installed GTK supports it.

The original application is a WPF/.NET Windows app. This project reimplements its
behaviour using Linux-native interfaces instead of translating Windows APIs:

| Windows mechanism | Linux equivalent used here |
|---|---|
| `GlobalSystemMediaTransportControlsSessionManager` | MPRIS over the session D-Bus |
| `NAudio` / Core Audio | WirePlumber (`wpctl`) for the default sink |
| `Keyboard.IsKeyToggled` + low-level hook | `/sys/class/leds/*::capslock` + evdev |
| WPF window animation | GTK4 `AdwAnimation` with the same duration/easing table |
| Acrylic / Mica backdrop | Translucent surface + blurred artwork (no client-side blur on Wayland) |
| Global media-key hook | D-Bus trigger + tray-free custom shortcut (Wayland forbids global hooks) |

![Media flyout](.artifacts/screenshots/media-flyout.png)
![Lock flyout](.artifacts/screenshots/lock-flyout.png)
![Volume flyout](.artifacts/screenshots/volume-flyout.png)

## Implemented

- **Media flyout** — cover art, title, artist, player chip, previous / play-pause / next,
  optional repeat and shuffle, optional seekbar with elapsed/total time.
  Exact upstream geometry: `310×112` base, `310-72 + max(36·repeat + 36·shuffle + 90·chip, 72)`
  wide, `+36` when the seekbar is visible; compact layout is `400×60`.
- **Lock-key flyout** — `160×50` card with the accent indicator bar, driven by Latch/Caps
  state read from the kernel (see limitations below).
- **Volume flyout** — `320×56` slider bound to the default PipeWire sink.
- **Settings window** — theme, position, animation speed/easing, durations, media
  features, app filtering, lock keys, volume, autostart.
- **Six flyout positions** — bottom/top × left/center/right, 16 px edge margin,
  bottom-center reserving 80 px for the system OSD (upstream rule).
- **Animation** — 0/150/300/450/600/900 ms × linear/sine/quadratic/cubic,
  animated height and opacity, matching `MainWindow.getDuration()`.
- **Auto-hide** — configurable duration (0–10 000 ms), paused while the pointer is
  over the flyout, "keep on screen" mode.
- **MPRIS** — discovery, player switching, metadata, artwork (local and remote),
  playback state, position polling, seek, repeat/shuffle, optional "pause other players".
- **App filtering** — allow-list / block-list by desktop entry or bus name.
- **D-Bus control interface** — see below.

## Not implemented (and why)

These upstream features have no faithful Linux equivalent; they are documented rather
than faked:

- **Next-Up flyout** — MPRIS exposes no queue look-ahead. Windows' SMTC does.
- **Taskbar widget / visualizer** — requires embedding into the Windows taskbar. A GNOME
  equivalent would have to be a separate Shell extension, deliberately out of scope here.
- **Acrylic/Mica blur** — Wayland gives clients no backdrop blur. The substitute is a
  translucent surface with a blurred, downscaled copy of the artwork.
- **Global media-key hook** — no supported Wayland mechanism. The flyout is triggered by
  D-Bus, a desktop shortcut, or automatically when the active player changes.
- **29 localisations** — English only.

## Requirements

Fedora 44 was used for development:

```bash
sudo dnf install gtk4-devel libadwaita-devel glib2-devel pkgconf-pkg-config \
  dbus-devel pipewire-utils rust cargo
```

Minimum versions: GTK 4.20+, libadwaita 1.6+, Rust 1.85 (edition 2024).

## Build

```bash
cargo build --release
```

The `layer-shell` feature is optional and only useful on wlroots/KDE:

```bash
cargo build --release --features layer-shell
```

## Run

```bash
fluentflyout              # start hidden, waiting for triggers
fluentflyout --settings   # open the settings window
fluentflyout --show-media # show the media flyout immediately
```

### CLI

| Flag | Meaning |
|---|---|
| `--show-media`, `--show-lock`, `--show-volume`, `--settings` | show a surface on startup |
| `--hidden` | never present a window until asked |
| `--diagnose` | print a capability report (session, renderer, Vulkan devices, MPRIS) and exit |
| `--screenshot <DIR>` | render every surface to PNG and exit (used for the images above) |
| `--config <PATH>` | use an alternative settings file |
| `--verbose` | enable debug logging |
| `--version`, `--help` | the usual |

### Custom shortcuts (the replacement for the global hook)

Wayland does not let an application grab media keys globally. Bind a key in
*Settings → Keyboard → Custom Shortcuts* to:

```bash
gdbus call --session --dest org.fluentflyout.Linux \
  --object-path /org/fluentflyout/Linux \
  --method org.fluentflyout.Linux.ShowMediaFlyout
```

`ShowLockKeys`, `ShowVolume`, `ShowSettings`, `ToggleMediaFlyout`, `PlayPause`,
`NextTrack`, `PreviousTrack` and `Quit` work the same way.

### D-Bus interface

```
bus name   org.fluentflyout.Linux
path       /org/fluentflyout/Linux
interface  org.fluentflyout.Linux
```

Methods: `ShowMediaFlyout`, `ShowLockKeys`, `ShowVolume`, `ShowSettings`,
`ToggleMediaFlyout`, `PlayPause`, `NextTrack`, `PreviousTrack`, `Quit`.
Read-only properties: `Version`, `Capabilities` (JSON), `Status` (JSON).

## Settings

Stored at `$XDG_CONFIG_HOME/fluentflyout-linux/settings.json`
(usually `~/.config/fluentflyout-linux/settings.json`), written atomically with a
500 ms debounce. Missing keys fall back to upstream defaults; a corrupt file is moved to
`settings.json.corrupt` rather than discarded.

## Testing

```bash
cargo test --all-targets          # unit tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
./scripts/verify-runtime.sh       # end-to-end checks against a mock MPRIS player
```

`examples/mock_player.rs` is a self-contained MPRIS player used by the runtime script:

```bash
cargo run --example mock_player -- --art .artifacts/test-cover.png --identity "Mock Player"
```

## Architecture

```
src/
├── domain/      # pure rules: media, geometry, settings, lock keys, volume
├── media/       # MPRIS client (zbus, own thread)
├── audio/       # WirePlumber client (wpctl)
├── lock_keys/   # LED class + evdev watcher
├── settings/    # atomic JSON store
├── ui/          # GTK4 widgets, CSS, animations, capture
├── app.rs       # event pump, auto-hide timers, window orchestration
├── platform.rs  # session/renderer/Vulkan capability detection
└── diagnostics.rs
```

Subsystems run on their own threads and publish events; only `app.rs` and `ui/` touch
GTK. See [docs/architecture.md](docs/architecture.md).

## License

GPL-3.0-or-later, matching the upstream project. See [LICENSE](LICENSE) and
[docs/provenance.md](docs/provenance.md) for what was adapted versus reimplemented.
