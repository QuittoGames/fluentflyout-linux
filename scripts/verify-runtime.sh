#!/usr/bin/env bash
# End-to-end runtime verification for FluentFlyout Linux.
#
# Runs the real application inside the current graphical session, drives it
# through its D-Bus control interface, attaches a mock MPRIS player and checks
# that discovery, metadata, playback state and control calls all behave.
#
# Usage:  scripts/verify-runtime.sh
# Exit:   0 when every check passed, 1 otherwise.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
APP="$TARGET/debug/fluentflyout"
MOCK="$TARGET/debug/examples/mock_player"
WORK="${TMPDIR:-/tmp}/ffl-verify"

BUS="org.fluentflyout.Linux"
OBJ="/org/fluentflyout/Linux"
IFACE="org.fluentflyout.Linux"

APP_PID=""
MOCK_PID=""
PASS=0
FAIL=0

mkdir -p "$WORK"
rm -f "$WORK"/*.log "$WORK"/*.png

cleanup() {
    [[ -n "$MOCK_PID" ]] && kill "$MOCK_PID" 2>/dev/null
    [[ -n "$APP_PID" ]] && kill "$APP_PID" 2>/dev/null
    wait 2>/dev/null
}
trap cleanup EXIT

ok()   { printf '  \033[32mPASS\033[0m %s\n' "$1"; PASS=$((PASS + 1)); }
bad()  { printf '  \033[31mFAIL\033[0m %s\n' "$1"; FAIL=$((FAIL + 1)); }
info() { printf '\n\033[1m%s\033[0m\n' "$1"; }

call() {
    gdbus call --session --dest "$BUS" --object-path "$OBJ" \
        --method "$IFACE.$1" 2>&1
}

prop() {
    gdbus call --session --dest "$BUS" --object-path "$OBJ" \
        --method org.freedesktop.DBus.Properties.Get "$IFACE" "$1" 2>&1
}

wait_for_bus() {
    for _ in $(seq 1 60); do
        if gdbus call --session --dest org.freedesktop.DBus \
            --object-path /org/freedesktop/DBus \
            --method org.freedesktop.DBus.ListNames 2>/dev/null | grep -q "$BUS"; then
            return 0
        fi
        sleep 0.25
    done
    return 1
}

# --------------------------------------------------------------------------
info "0. Artefacts"
if [[ ! -x "$APP" ]]; then
    echo "missing $APP — run: cargo build --all-targets" >&2
    exit 1
fi
ok "application binary present"

# A deterministic cover so artwork loading and accent extraction are exercised.
if command -v magick >/dev/null; then
    magick -size 320x320 gradient:'#1b3a5c'-'#e0613a' "$WORK/cover.png" 2>/dev/null \
        && ok "test artwork generated with magick"
else
    printf '\x89PNG\r\n\x1a\n' > "$WORK/cover.png"
    bad "ImageMagick not found; artwork test uses a placeholder file"
fi

# --------------------------------------------------------------------------
info "1. Application launch"
"$APP" --hidden --verbose > "$WORK/app.log" 2>&1 &
APP_PID=$!

if wait_for_bus; then
    ok "app registered $BUS on the session bus"
else
    bad "app did not register $BUS within 15s"
    tail -20 "$WORK/app.log"
    exit 1
fi

VER="$(prop Version)"
[[ "$VER" == *"0.1.0"* ]] && ok "Version property = $(echo "$VER" | tr -d "\n")" \
    || bad "unexpected Version: $VER"

CAPS="$(prop Capabilities)"
[[ "$CAPS" == *media* ]] && ok "Capabilities advertise media (${CAPS})" \
    || bad "Capabilities missing media: $CAPS"

# --------------------------------------------------------------------------
info "2. Mock MPRIS player"
"$MOCK" --art "$WORK/cover.png" --identity "Mock Player" > "$WORK/mock.log" 2>&1 &
MOCK_PID=$!
sleep 2.5

STATUS="$(prop Status)"
if [[ "$STATUS" == *"Northern Lights"* ]]; then
    ok "MPRIS discovery found the mock player (track metadata visible)"
else
    bad "MPRIS discovery did not surface the mock track: $STATUS"
fi
[[ "$STATUS" == *"Mock Player"* ]] && ok "player identity resolved from org.mpris.MediaPlayer2" \
    || bad "player identity missing: $STATUS"
[[ "$STATUS" == *"Playing"* ]] && ok "playback state read as Playing" \
    || bad "playback state not Playing: $STATUS"

# --------------------------------------------------------------------------
info "3. Flyout request path"
OUT="$(call ShowMediaFlyout)"
[[ "$OUT" == *true* ]] && ok "ShowMediaFlyout returned true" \
    || bad "ShowMediaFlyout failed: $OUT"
sleep 0.6

OUT="$(call ShowLockKeys)"
[[ "$OUT" == *true* ]] && ok "ShowLockKeys returned true" \
    || bad "ShowLockKeys failed: $OUT"

OUT="$(call ShowVolume)"
[[ "$OUT" == *true* ]] && ok "ShowVolume returned true (degrades without a sink)" \
    || bad "ShowVolume failed: $OUT"

OUT="$(call ShowSettings)"
[[ "$OUT" == *true* ]] && ok "ShowSettings returned true" \
    || bad "ShowSettings failed: $OUT"
sleep 0.4

# --------------------------------------------------------------------------
info "4. Playback control through D-Bus"
call PlayPause > /dev/null
sleep 1.0
STATUS="$(prop Status)"
[[ "$STATUS" == *"Paused"* ]] && ok "PlayPause paused the player" \
    || bad "PlayPause did not pause: $STATUS"

call PlayPause > /dev/null
sleep 1.0
STATUS="$(prop Status)"
[[ "$STATUS" == *"Playing"* ]] && ok "PlayPause resumed the player" \
    || bad "PlayPause did not resume: $STATUS"

call NextTrack > /dev/null
call PreviousTrack > /dev/null
sleep 0.6
grep -q "mock: Next" "$WORK/mock.log" && ok "NextTrack reached the player" \
    || bad "NextTrack never arrived"
grep -q "mock: Previous" "$WORK/mock.log" && ok "PreviousTrack reached the player" \
    || bad "PreviousTrack never arrived"

# --------------------------------------------------------------------------
info "5. Resilience"
kill -9 "$MOCK_PID" 2>/dev/null
MOCK_PID=""
sleep 2.0
STATUS="$(prop Status)"
if [[ "$STATUS" == *'"media":null'* ]]; then
    ok "player disappearance handled without crashing the app"
else
    bad "stale player after kill: $STATUS"
fi
if kill -0 "$APP_PID" 2>/dev/null; then
    ok "application still alive after the player vanished"
else
    bad "application exited when the player vanished"
fi

# --------------------------------------------------------------------------
info "6. Log hygiene"
if grep -qiE "panic|thread .* panicked" "$WORK/app.log"; then
    bad "panic found in application log"
    grep -iE "panic" -A3 "$WORK/app.log" | head -20
else
    ok "no panics in the application log"
fi
ERRORS="$(grep -c "^.*ERROR" "$WORK/app.log" 2>/dev/null || true)"
if [[ "${ERRORS:-0}" -eq 0 ]]; then
    ok "no ERROR lines logged"
else
    bad "$ERRORS ERROR line(s) in the log"
    grep "ERROR" "$WORK/app.log" | head -10
fi

# --------------------------------------------------------------------------
info "7. Diagnostics / renderer evidence"
# Diagnostics describe *this* process, so no other instance may be running:
# a second launch would otherwise be forwarded to the live one.
kill "$APP_PID" 2>/dev/null
wait "$APP_PID" 2>/dev/null
APP_PID=""
sleep 0.5
timeout 40 "$APP" --diagnose > "$WORK/diagnose.log" 2>&1
DIAG_STATUS=$?
if [[ $DIAG_STATUS -eq 0 ]]; then
    ok "--diagnose exited cleanly"
else
    bad "--diagnose exited with $DIAG_STATUS"
fi
if grep -q "Rendering" "$WORK/diagnose.log"; then
    ok "capability report rendered"
    sed 's/^/       /' "$WORK/diagnose.log"
else
    bad "capability report missing from --diagnose output"
    sed 's/^/       /' "$WORK/diagnose.log"
fi
if grep -qi "vulkan" "$WORK/diagnose.log"; then
    ok "diagnostics report Vulkan state"
    grep -i "vulkan" "$WORK/diagnose.log" | head -6 | sed 's/^/       /'
else
    bad "diagnostics never mention Vulkan"
fi

# --------------------------------------------------------------------------
info "Summary"
printf '  %d passed, %d failed\n' "$PASS" "$FAIL"
printf '  logs: %s\n' "$WORK"
[[ $FAIL -eq 0 ]] || exit 1
