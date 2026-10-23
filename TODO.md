# TODO

Project: `fluentflyout-linux` — native Rust/GTK4 port of FluentFlyout.
Legend: `[H]` Human (DEV) · `[A]` Agent · `[S]` Shared.
Modes: `[PLAN]` `[MECHANICAL]` `[ARCH]` `[REVIEW]` `[LEARN]`.

## Human

- [ ] [H][ARCH] Decide whether the optional `layer-shell` feature should become the
      default on wlroots/KDE. It is implemented but not wired into the default build
      because GNOME ignores the protocol; enabling it by default would change placement
      behaviour on Plasma. Reproduce: `cargo build --features layer-shell`.
- [ ] [H][REVIEW] Approve the visual deviations listed in
      `docs/rea-report.md` → "Deliberate behaviour changes" (light surfaces follow the
      system theme instead of always-dark acrylic; Adwaita icons instead of Fluent
      glyphs).
- [ ] [H][REVIEW] Run the app on a real GNOME 48+ Wayland session and confirm the
      placement fallback (compositor-placed window) is acceptable, or approve adding a
      GNOME Shell extension for exact anchoring.
- [ ] [H][REVIEW] Run the lock-key flyout on hardware that exposes
      `/sys/class/leds/input*::capslock` and confirm state tracking end to end.
- [ ] [H][ARCH] Decide whether to publish/distribute binaries. Doing so triggers the
      GPL obligations in `docs/provenance.md`.

## Agent

- [ ] [A][MECHANICAL] Add an integration test that runs `examples/mock_player.rs` against
      a second player to exercise arbitration (playing beats paused) — the runtime script
      currently uses a single player.
- [ ] [A][MECHANICAL] Cover `SeekAbsolute` against a player that omits `mpris:trackid`
      (currently returns early; behaviour is intentional but untested).
- [ ] [A][MECHANICAL] Add `zh`, `ja`, `de` translations once a localisation decision is
      made (upstream ships 29).

## Shared

- [ ] [S][REVIEW] Re-run `scripts/verify-runtime.sh` on a host with a real PipeWire sink
      so the volume slider path is exercised end to end instead of only its parser.
- [ ] [S][REVIEW] Re-check contrast of the accent colour against artwork-derived
      accents with a palette of real album covers.

## Blocked

- [ ] [A][BLOCKED] Next-Up flyout — MPRIS exposes no queue look-ahead. Needs a
      per-player extension (Spotify/Mopidy) or an MPRIS metadata extension before a
      faithful implementation is possible.
- [ ] [A][BLOCKED] Taskbar widget and visualizer — requires Windows taskbar embedding; a
      Linux equivalent would be a separate GNOME Shell extension, outside this
      application.
- [ ] [A][BLOCKED] Client-side backdrop blur (acrylic/Mica) — Wayland offers no protocol
      for a client to blur what is behind its surface. Substituted with a blurred
      downscaled copy of the artwork.
- [ ] [A][BLOCKED] Global media-key hook — no supported Wayland mechanism. Substituted
      with the D-Bus interface plus `show_on_media_change`.

## Completed

- [x] [A][PLAN] REA of upstream `309e5ea`, Windows-API → Linux mapping, behavioural spec.
- [x] [A][MECHANICAL] Cargo workspace, GTK4/libadwaita app skeleton, diagnostics.
- [x] [A][MECHANICAL] MPRIS service: discovery, arbitration, metadata, artwork, position
      polling, commands, reconnect, "pause other players".
- [x] [A][MECHANICAL] Media flyout: exact geometry, cover overlay, controls, seekbar,
      repeat/shuffle, player chip, backdrop styles, accent from artwork.
- [x] [A][MECHANICAL] Lock-key flyout: LED-class state + evdev events, `EV_LED` decoding,
      capability reporting.
- [x] [A][MECHANICAL] Volume flyout via `wpctl`, parser, unavailable path.
- [x] [A][MECHANICAL] Settings window (six groups) and atomic debounced persistence.
- [x] [A][MECHANICAL] Animations matching the upstream duration/easing table; auto-hide
      with hover pause and "keep on screen".
- [x] [A][MECHANICAL] D-Bus control surface, single-instance forwarding, CLI flags,
      `--diagnose`, `--screenshot`.
- [x] [A][MECHANICAL] Runtime harness: mock MPRIS player + 23 end-to-end checks.
- [x] [A][REVIEW] `cargo fmt --check`, `clippy -D warnings`, 114 tests, runtime script —
      all green.
- [x] [A][MECHANICAL] Documentation: README, architecture, REA report, validation,
      provenance, decisions, spec.
