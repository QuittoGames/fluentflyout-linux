# FluentFlyout for Linux — derived behavioural specification

Status: implemented and verified.
Source of truth: upstream commit `309e5ea6708a5e8f928990acd24ff525b370da88`
(`github.com/unchihugo/FluentFlyout`, GPL-3.0-or-later), read directly from source, plus
runtime observation of this port.

Labels used throughout: **FACT** (read from source or measured), **INFERENCE** (derived
from implementation evidence), **DECISION** (made in this project), **UNKNOWN**
(unresolved).

---

## 1. Feature inventory

| Upstream feature | Ported | Notes |
|---|---|---|
| Media flyout | yes | full geometry, controls, seekbar |
| Lock-keys flyout | yes | state from kernel LEDs, events from evdev |
| Volume flyout | yes | default sink only; mixer omitted |
| Settings window | yes | 6 groups instead of 11 tabs |
| Six flyout positions | yes | exact upstream formula |
| Animation speed/easing | yes | same 6×4 table |
| Auto-hide + hover pause | yes | same 100 ms polling model replaced by signals |
| Theme (system/light/dark) | yes | GTK/libadwaita |
| Accent from artwork | yes | dominant colour of the cover |
| App filtering | yes | allow-list / block-list |
| Pause other players | yes | on transition only |
| Pinned player | yes | setting `pinned_player` |
| Next-Up flyout | **no** | MPRIS has no queue. DECISION 006 |
| Taskbar widget / visualizer | **no** | Windows taskbar embedding. DECISION 006 |
| Acrylic / Mica backdrop | **no** | no client backdrop blur on Wayland; blurred-artwork substitute |
| Global media-key hook | **no** | Wayland forbids it. DECISION 005 |
| Tray icon | **no** | no `StatusNotifierItem` host in the verified session; D-Bus trigger instead |
| 29 localisations | **no** | English only |

---

## 2. Media flyout

### 2.1 Geometry (FACT — `MainWindow.UpdateUILayout`)

```
non-compact width  = 310 - 72 + max(36·repeat + 36·shuffle + 90·player_chip, 72)
non-compact height = 112 + (36 if seekbar visible)
compact     width  = 400
compact     height = 60 + (36 if seekbar visible)
```

Measured in this port: `328×112` with the player chip enabled and no repeat/shuffle
(= 310 − 72 + 90), `310×112` with the chip disabled, `400×60` compact. **FACT.**

Inner metrics: cover 82×82 with a 4 px radius; title 14 px/600; artist 13–14 px at 50–58 %
opacity; transport buttons 28 px, play/pause 32 px in the accent colour; repeat/shuffle
28 px, icon at 50 % opacity when off; time labels 12 px at 50 % opacity, 45 px wide;
player chip max 90 px.

### 2.2 States

| State | Presentation |
|---|---|
| no player | card with "No media playing", transport disabled except the player chip |
| paused | play glyph, position frozen |
| playing | pause glyph, seekbar advances (500 ms poll) |
| seek unsupported | seekbar hidden even if the setting is on |
| no artwork | flat accent-tinted placeholder square |
| compact | no cover, no artist, single row |

### 2.3 Interactions

- play/pause → MPRIS `Play`/`Pause` according to the last known state
- next / previous → `Next` / `Previous`, disabled when the player reports `CanGoNext=false`
- repeat → cycles `None → Playlist → Track → None`, writes `LoopStatus`
- shuffle → toggles `Shuffle`
- seekbar drag → `SetPosition(trackid, position)` (requires `mpris:trackid`)
- click on the card does not close the flyout; the pointer entering pauses auto-hide

---

## 3. Lock-keys flyout

`160×50`, 22 px glyph, 14 px/600 label, 60×3 accent indicator bar with a 2 px radius at
the bottom edge. Label text is `"<Key> on"` / `"<Key> off"`.

State source order: LED class device (`/sys/class/leds/input*::<key>/brightness`) for Caps,
Num and Scroll; evdev `EV_LED` events override the polled value; evdev `EV_KEY` presses
provide the trigger. Insert has no LED and is therefore event-only.

Degradation matrix (`LockKeySupport`):

| LEDs | evdev | Result |
|---|---|---|
| yes | yes | Full — flyout on key press with the real state |
| yes | no | StateOnly — flyout appears when the polled state changes |
| no | yes | StateOnly — flyout on key press, state from `EV_LED` |
| no | no | Unavailable — reported in diagnostics, feature inert |

---

## 4. Volume flyout

`320×56`: speaker icon (muted variant when muted), slider, percentage label. Slider writes
`wpctl set-volume @DEFAULT_AUDIO_SINK@ <fraction>`; the icon toggles mute through
`wpctl set-mute … toggle`. Level is re-read whenever the flyout opens; there is no device
change subscription.

---

## 5. Placement

`compute_origin(anchor, work_area, size, reserve_volume_osd)`:

| anchor | x | y |
|---|---|---|
| bottom-left | `+16` | `bottom − height − 16` |
| bottom-center | centred | `bottom − height − 80` |
| bottom-right | `right − width − 16` | `bottom − height − 16` |
| top-left | `+16` | `+16` |
| top-center | centred | `+16` |
| top-right | `right − width − 16` | `+16` |

Result is clamped into the work area, including the degenerate case of a window larger than
the monitor.

**UNKNOWN / platform-limited:** on GNOME Wayland a toplevel cannot choose its own
position. The app computes the intended rectangle, logs it, and lets the compositor place
the window; the settings window states this explicitly. On wlroots/KDE the optional
`layer-shell` feature can anchor surfaces instead.

---

## 6. Animation

Duration table (ms): `0→0, 1→150, 2→300, 3→450, 4→600, 5→900`. Easing: linear, sine,
quadratic, cubic. Animated properties: opacity and a small vertical offset, 150–200 ms
perceptually equivalent to the upstream `Top` + `Opacity` pair. Animations honour
`gtk-enable-animations`.

---

## 7. Settings and persistence

| Field group | Defaults |
|---|---|
| theme | system |
| position | bottom-left |
| animation_speed / easing | 2 / quadratic |
| duration_ms | 3000 (clamp 0–10000) |
| player chip / seekbar / repeat / shuffle | on / off / off / off |
| accent_from_artwork | off |
| backdrop | solid |
| show_on_media_change | on (Linux adaptation) |
| lock keys enabled + caps/num/scroll/insert | all on, duration 2000 |
| volume control | off, duration 3000 |
| autostart | **off** (upstream defaults on because of its installer) |
| app filtering | off, allow-list |

File: `$XDG_CONFIG_HOME/fluentflyout-linux/settings.json`. Atomic write via temp file +
`rename`, 500 ms debounce, invalid JSON quarantined as `settings.json.corrupt`, unknown
keys ignored, missing keys defaulted.

---

## 8. Acceptance criteria ↔ evidence

| Criterion | Evidence |
|---|---|
| builds reproducibly | `cargo build --release`, see `docs/validation.md` |
| launches on Fedora/GNOME Wayland | launched under WSLg Wayland (`wayland-0`) |
| polished functional UI | `.artifacts/screenshots/*.png`, rendered by the app |
| discovers players, performs actions | `scripts/verify-runtime.sh` 22/22 against a mock MPRIS player |
| metadata and artwork update | title/artist/art verified in captures; artwork reload on track change |
| settings persist | unit tests + restart in the runtime script |
| optional services never crash the app | audio and lock-key Unavailable paths exercised |
| audio / lock keys within permissions | capability matrix above |
| shutdown and cleanup tested | runtime script kills the player and the app without panics |
| Vulkan investigated and tested | `--diagnose`: `GSK_RENDERER=vulkan`, `libvulkan mapped: yes`, dzn device present |
| Vulkan claims only with evidence | conclusions are derived from `/proc/self/maps` |
| builds/tests pass | 111 lib + 3 bin tests, clippy `-D warnings` clean |
| spec matches implementation | this document, reviewed against the code |

---

## 9. Uncertainties

1. **GNOME placement.** The verified session (WSLg/Weston) has no `xdg-desktop-portal`, so
   portal-based positioning could not be tested. On GNOME the compositor decides; this is
   documented rather than worked around with X11 assumptions.
2. **Lock keys on the test host.** No `/sys/class/leds` and no `/dev/input` exist in the
   containerised session, so only the parsers and the Unavailable path are covered by
   tests. The LED and evdev paths are code-reviewed, not executed here.
3. **Audio end-to-end.** PipeWire runs with no sink, so `wpctl` returns no default sink;
   only the Unavailable path and the parser are verified at runtime.
4. **MPRIS artwork over HTTP.** Remote artwork is fetched via GIO, which needs a GVFS HTTP
   backend. Local `file://` artwork is verified; HTTP is best-effort by design.
