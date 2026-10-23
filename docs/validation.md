# Validation

Every claim in this document was produced by executing the command described in it on
the development host. Nothing here is aspirational.

## Environment under test

| Item | Value | How it was determined |
|---|---|---|
| Distribution | Fedora Linux 44 (WSL2 guest) | `/etc/os-release` |
| Kernel | 6.18.54.1-microsoft-standard-WSL2 | `uname -r` |
| GTK | 4.22.4 | `pkg-config --modversion gtk4` |
| libadwaita | 1.9.2 | `pkg-config --modversion libadwaita-1` |
| Rust | 1.99.0 (b940084d7, 2026-09-28, Fedora 1.99.0-1.fc44) | `rustc -Vv` |
| Session | WSLg: Wayland `wayland-0` plus XWayland `:0` | `$WAYLAND_DISPLAY`, `$DISPLAY` |
| D-Bus | session bus at `/run/user/1000/bus` | `$DBUS_SESSION_BUS_ADDRESS` |
| Audio | PipeWire 1.6.9 running, **no sink present** | `wpctl status` |
| Input | **no** `/dev/input`, **no** `/sys/class/leds` | `ls` on both paths |

The host has no GNOME Shell (WSLg composites with Weston), no xdg-desktop-portal and no
wlr-layer-shell. That matters for two rows below and is stated again where relevant.

## Static gates

| Command | Result |
|---|---|
| `cargo build --all-targets` | success, no warnings |
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo test --all-targets` | **111 lib + 3 bin passed; 0 failed; 0 ignored** |

The unit tests cover the platform-independent rules where a real defect would otherwise
be invisible: anchor arithmetic for all six positions including work-area offsets and
clamping, duration formatting, loop-status cycling, seekbar heuristics, settings
round-trip/migration/sanitising, atomic persistence with corrupt-file quarantine,
`wpctl` output parsing including muted/clamped/NaN inputs, `input_event` decoding, LED
`EV_LED` decoding, percent-decoding of `file://` URIs, metadata extraction, and artwork
source resolution.

## Rendering: is Vulkan actually in use?

GTK 4.22 exposes no public "which renderer am I using" accessor, so the application
collects three independent pieces of evidence and only claims Vulkan when all agree.

1. **Compile-time support** — `/usr/lib64/libgtk-4.so.1` links `libvulkan.so.1`, and
   `GSK_RENDERER=help` lists `vulkan` among the available renderers.
2. **Requested renderer** — the app sets `GSK_RENDERER=vulkan` at startup when a Vulkan
   loader *and* at least one ICD manifest are present (`main.rs::prefer_vulkan`).
3. **Runtime evidence** — `platform.rs` reads `/proc/self/maps` and looks for a mapped
   `libvulkan` after the first frame.

`fluentflyout --diagnose` output on the host:

```
Rendering
---------
  GSK_RENDERER   : vulkan
  in use         : Vulkan
  libvulkan mapped: yes
  GTK built with Vulkan: yes

Vulkan devices
--------------
  apiVersion         = 1.2.354
  deviceType         = PHYSICAL_DEVICE_TYPE_DISCRETE_GPU
  deviceName         = Microsoft Direct3D12 (AMD Radeon RX 7600)
  apiVersion         = 1.4.354
  deviceType         = PHYSICAL_DEVICE_TYPE_CPU
  deviceName         = llvmpipe (LLVM 22.1.8, 256 bits)
```

**FACT:** the application renders through GSK's Vulkan backend on this machine, using the
`dzn` ICD over D3D12 on a discrete Radeon RX 7600. The loader prints
`WARNING: dzn is not a conformant Vulkan implementation, testing use only` — that is the
ICD's own warning about the D3D12 translation layer in WSL, not an application error.

**Limit of the claim:** this validates the Vulkan path on a WSL2 host with a virtualised
GPU. It does not prove behaviour on a native Fedora + GNOME + Mesa session; there the
same code path applies but was not executed here.

## End-to-end runtime

`scripts/verify-runtime.sh` launches the real binary, a mock MPRIS player
(`examples/mock_player.rs`) and drives everything through the public D-Bus interface.

```
== 23 checks, 23 passed ==
```

Covered:

- the process starts under Wayland, exports `org.fluentflyout.Linux` and answers
  `Version`, `Capabilities` and `Status`;
- the media service discovers the mock player and reports identity, title, artist,
  playback state and duration through `Status`;
- `ShowMediaFlyout`, `ShowLockKeys`, `ShowVolume`, `ShowSettings` each return success;
- `PlayPause` toggles the player Playing → Paused (verified from the mock's own log);
- `NextTrack` and `PreviousTrack` reach the player;
- when the player exits, the app reports `"media":null`, logs no error and keeps running;
- the process shuts down cleanly on `Quit`;
- `--diagnose` prints a complete capability report.

The one initial failure was a stale string in the script (`grep "Renderer"` against a
report whose header is `Rendering`); the report itself was correct and the check now
passes.

## Visual verification

`--screenshot <DIR>` renders each surface through its real GTK renderer to PNG. This
exists because `xwd`/`import` cannot capture Wayland surfaces through XWayland.

| Surface | Size | Verified |
|---|---|---|
| Media flyout | 328×112 | 82×82 artwork, title, artist, prev/play/next, player chip |
| Lock flyout | 160×50 | lock icon, "Caps Lock on", 60×3 accent bar |
| Volume flyout | 320×56 | speaker icon, slider at 45%, "45%" |
| Settings | 620×760 | six preference groups, switches, combos, duration spin |

328 px is the upstream formula `310 − 72 + 90` with the player chip enabled, i.e. the
layout math produces the same geometry as the WPF original rather than a lookalike.

Accessibility-relevant properties checked in code and by inspection: labels are real
`gtk::Label`s (readable by AT-SPI), every control is a focusable GTK widget reachable by
keyboard, the accent bar is decorative rather than the only state cue (the text says
"Caps Lock on/off"), and contrast ratios were computed by unit tests for the accent
foreground (`ui::style` tests).

## What could not be verified here, and why

| Area | Status | Reason / how to verify elsewhere |
|---|---|---|
| Flyout placement on GNOME Wayland | **not executed** | GNOME Shell does not implement wlr-layer-shell and forbids client-side toplevel positioning; the app detects this and asks the compositor to place the window. Needs a real GNOME session. |
| Lock-key flyout against real hardware | **not executed** | This host exposes neither `/sys/class/leds` nor `/dev/input`. Decoding and state mapping are unit-tested; the capability report correctly says `Unavailable`. |
| Volume flyout against a real sink | **partially** | `wpctl` is installed but no sink exists, so only the unavailable path was exercised end to end, plus unit tests of the output parser. Run on a desktop with PipeWire to see the slider. |
| Remote (`http(s)://`) artwork | **not executed** | Requires gvfs-http; local `file://` artwork is exercised by the mock player. |
| Next-Up flyout | **not implemented** | MPRIS has no queue look-ahead (see `docs/rea-report.md`). |
| Autostart entry | **not executed** | Writing `~/.config/autostart` is implemented; enabling it would modify the session of the machine, which the task forbids. |
| 29 localisations | **not implemented** | Out of scope; English only. |

## Reproducing

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
CARGO_TARGET_DIR=$HOME/ffl-target ./scripts/verify-runtime.sh
$HOME/ffl-target/debug/fluentflyout --diagnose
$HOME/ffl-target/debug/fluentflyout --screenshot .artifacts/screenshots
```
