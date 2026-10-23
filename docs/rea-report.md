# REA report — FluentFlyout

Reverse-engineering analysis of the reference implementation, and the mapping from each
Windows mechanism to the Linux interface used by this port.

- Reference: `https://github.com/unchihugo/FluentFlyout`
- Revision analysed: `309e5ea6708a5e8f928990acd24ff525b370da88` (2026-10-20)
- Licence: GPL-3.0-or-later
- Size: WPF/.NET, 10 803 lines of C#; `FluentFlyoutWPF/MainWindow.xaml.cs` is 1 795 lines.

The upstream repository was cloned read-only and never modified.

---

## 1. Repository structure (FACT)

```
FluentFlyout/
├── FluentFlyoutWPF/
│   ├── MainWindow.xaml(.cs)        # media flyout + taskbar widget + audio/mica logic
│   ├── LockWindow.xaml(.cs)        # lock-key flyout
│   ├── NextUpWindow.xaml(.cs)      # "up next" flyout
│   ├── VolumeWindow.xaml(.cs)      # volume flyout
│   ├── SettingsWindow/             # 11 settings pages
│   ├── Classes/
│   │   ├── UserSettings.cs         # persisted model
│   │   ├── SettingsManager.cs      # XML load/save
│   │   ├── Utils/                  # helpers (accent, colour, DWM)
│   │   └── Windows/                # Win32 interop, taskbar, hooks
│   ├── Localization/               # 29 .resx languages
│   └── Assets/
├── FluentFlyout.sln
└── LICENSE                          # GPL-3.0-or-later
```

## 2. Lifecycle (FACT)

`App.xaml.cs` reads `UserSettings`, creates `MainWindow` hidden, installs a low-level
keyboard hook, and reacts to `WM_HOTKEY`/media-key messages by showing a flyout. The app
lives in the tray and never exits on window close. Settings are written by
`SettingsManager` with a 500 ms debounce to `%APPDATA%\FluentFlyout\settings.xml`.

## 3. Feature behaviour extracted from source

### 3.1 Media flyout

- Visibility is driven by media-key presses with a 500 ms repeat debounce, plus
  `MediaFlyoutAlwaysDisplay`.
- Auto-hide: a loop polls pointer-over every 100 ms, then waits `Duration`
  (default 3000 ms, clamped 0–10 000) and closes unless the pointer re-enters.
- Suppressed while a fullscreen application is in the foreground.
- Layout constants and the width/height formulas are reproduced verbatim in
  `src/domain/settings.rs::media_flyout_size` and documented in the spec.
- Session selection prefers the current SMTC session; a pinned session overrides it.

### 3.2 Lock flyout

`160×50`; 22 px lock glyph; 14 px/600 label; 60×3 accent bar with a 2 px radius; the
shackle animates between open and closed. Driven by `Keyboard.IsKeyToggled` through
`GetKeyState`.

### 3.3 Volume flyout

Default render device volume with mute toggle. Upstream also exposes a full mixer; the
port keeps the default sink only.

### 3.4 Taskbar widget / visualizer

Draws inside the Windows taskbar via `Shell_TrayWnd` subclassing and WASAPI loopback peaks.

### 3.5 Next-Up

Reads the SMTC queue through the WinRT session model.

---

## 4. Windows API → Linux mapping

| Upstream API | Purpose | Linux replacement | Fidelity |
|---|---|---|---|
| `GlobalSystemMediaTransportControlsSessionManager` | media session, metadata, transport | MPRIS (`org.mpris.MediaPlayer2.Player`) over D-Bus | high, minus queue |
| `Windows.Media.Control` timeline | position/duration | `Position` + `mpris:length` (500 ms poll) | high |
| `NAudio` / Core Audio | device volume, mute | WirePlumber `wpctl` on `@DEFAULT_AUDIO_SINK@` | high for default sink |
| `GetKeyState(VK_CAPITAL)` | lock state | `/sys/class/leds/*::capslock/brightness` | medium: polled |
| `WH_KEYBOARD_LL` hook | key trigger | evdev `EV_KEY` (needs `input` group) or D-Bus trigger | medium |
| `SetWindowPos` + WPF `Top` animation | placement, animation | `compute_origin` + `AdwAnimation`; compositor may override | medium on GNOME |
| DWM Mica/Acrylic | backdrop | translucent surface + blurred artwork | visual approximation |
| `Shell_TrayWnd` subclassing | taskbar widget | — | none |
| Windows notification area | tray icon | — (D-Bus trigger instead) | none |
| MSIX startup task | autostart | XDG autostart `.desktop` entry (opt-in) | high |

## 5. Deliberate behaviour changes

1. `show_on_media_change` (default on): the flyout also appears when the active player
   changes track or playback state, because a Wayland client cannot observe key presses.
2. `autostart` defaults to off; enabling it writes an XDG autostart entry.
3. Settings are JSON, not XML. Migration from the Windows file is not provided: the
   formats and defaults differ enough that a silent conversion would misrepresent intent.

## 6. Non-portable elements (FACT + DECISION)

| Element | Why it cannot be ported | Nearest supported behaviour |
|---|---|---|
| Next-Up flyout | MPRIS exposes the current track only | none; feature omitted |
| Taskbar widget/visualizer | Windows taskbar embedding | none; would require a GNOME Shell extension |
| Acrylic/Mica blur | Wayland offers no client backdrop blur | blurred artwork behind a translucent card |
| Global media keys | no global input capture for clients | D-Bus call from a desktop shortcut |
| Tray icon | no StatusNotifierItem host in the verified session | D-Bus + settings window |

## 7. Licensing observations (FACT)

The upstream `LICENSE` is GPL-3.0-or-later. This port is distributed under the same
licence. No upstream source file was copied: layout constants and behavioural rules were
re-derived and re-expressed in Rust. See `docs/provenance.md`.
