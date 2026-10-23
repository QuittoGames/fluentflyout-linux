# Architecture

## 1. Goals and non-goals

**Goals.** Reproduce FluentFlyout's media, lock-key and volume flyouts with native GTK4
widgets, keep the platform integrations replaceable, keep the pure rules testable without
a display server, and never claim a capability the host does not provide.

**Non-goals.** A general agent platform, a plugin system, a custom compositor, or feature
parity with Windows-only integrations (see `docs/rea-report.md`).

## 2. Layers

```
┌────────────────────────────────────────────────────────────┐
│ ui/        GTK4 + libadwaita widgets, CSS, animations      │
├────────────────────────────────────────────────────────────┤
│ app.rs     event pump, auto-hide timers, window orchestration│
│ app/control.rs  D-Bus interface, single-instance handshake  │
├────────────────────────────────────────────────────────────┤
│ media/  audio/  lock_keys/  settings/  (own threads each)   │
├────────────────────────────────────────────────────────────┤
│ domain/    pure rules: media, geometry, settings, volume,   │
│            lock keys — no GTK, no D-Bus, no filesystem      │
├────────────────────────────────────────────────────────────┤
│ platform.rs  diagnostics.rs  (capability detection)         │
└────────────────────────────────────────────────────────────┘
```

Dependencies point downwards only. `domain` has no I/O; the service modules own their
threads and translate host interfaces into domain values; `app` is the only place that
knows about every subsystem; `ui` never talks to D-Bus or `/sys`.

## 3. Data flow

### Media

```
MPRIS signals ──► media/mpris.rs ──► AppEvent::Media(snapshot) ──► app.rs
                       ▲                                              │
                       │                                              ▼
              MediaCommand ◄────────── user intent ◄────────── ui/flyout_media.rs
```

`media/mpris.rs` runs `zbus::block_on` on its own thread. It multiplexes three sources in
one `select!`: a `MessageStream` for `PropertiesChanged` (path
`/org/mpris/MediaPlayer2`), a second for `NameOwnerChanged`
(`arg0namespace=org.mpris.MediaPlayer2`), a command channel, and a 500 ms tick channel
used for the position poll. Every event re-reads the active player's full snapshot; a
discovery reconcile runs every six ticks so a missed signal cannot leave a stale player.

### Audio

`wpctl` is invoked synchronously from the audio thread. There is no push notification, so
the level is refreshed when the flyout opens and after every write.

### Lock keys

Two producers: a 150 ms poll of LED class devices, and evdev readers that decode 24-byte
`input_event` records. `EV_KEY` presses trigger the flyout; `EV_LED` records confirm the
resulting state. Both feed the same `AppEvent` channel, and the service reports
`LockKeySupport` so the UI can explain a degraded mode.

## 4. Threading model

| Thread | Owns |
|---|---|
| GTK main | all widgets, animations, CSS, timers, event pump |
| `mpris-service` | D-Bus connection, proxies, player arbitration |
| `audio-service` | `wpctl` invocations |
| `lock-keys` (+ `lock-keys-leds`, `lock-keys-forward`, one per evdev device) | LED polling and event decoding |
| `mpris-position-tick` | 500 ms heartbeat that wakes the media loop |
| `cover-art-fetch` (per request) | HTTP artwork download, capped at 8 MiB |

Cross-thread communication is limited to `async_channel` senders. `gdk::Texture` is not
`Send`, so remote artwork travels as `glib::Bytes` and becomes a texture on the main
context.

## 5. State

`App` owns the authoritative in-memory state: the current `MediaSnapshot`, volume level,
lock states, capability values and one `glib::SourceId` per flyout for auto-hide.
`SettingsStore` owns the persisted configuration and is shared via `Rc<RefCell<_>>` inside
the GTK thread.

Auto-hide replaces upstream's 100 ms pointer polling with `EventControllerMotion`
enter/leave signals on each flyout window, which produces the same user-visible behaviour
without a polling loop.

## 6. Error handling

- Services never panic on a missing host capability: they publish `Unavailable(reason)`.
- MPRIS property reads degrade per field (`unwrap_or_default`), so a player that omits an
  optional property still works.
- A player that disappears between calls is dropped from the registry, not retried.
- Settings that fail to parse are quarantined; writes are atomic.
- GTK failures (no display, no renderer) are reported through `--diagnose`.

## 7. Rendering

GTK picks the renderer from `GSK_RENDERER`. When `--` is not set, `main.rs` requests
`vulkan` only if a loader and an ICD JSON are present; otherwise it leaves the choice to
GSK. Verification is evidence-based: `platform.rs` reads `/proc/self/maps` for
`libvulkan`, so `--diagnose` can distinguish "requested" from "actually in use".

## 8. Extensibility points

- New flyout: implement `present`/`animate_in`/`animate_out` + a `FlyoutKind` arm.
- New media backend: replace `media/mpris.rs` behind `MediaCommand`/`AppEvent`.
- New audio backend: replace `audio/mod.rs`; the domain model does not change.
- New placement strategy: `domain::geometry` is pure and independently tested.
