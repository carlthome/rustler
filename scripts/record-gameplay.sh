#!/usr/bin/env bash
# Record a short, heavily-compressed gameplay GIF for the Developer Diary Slack post.
#
# It reuses the e2e playtest bot to DRIVE real gameplay (so the clip shows the actual
# catching / train / steal loop, not a staged demo), renders it to a headless virtual
# display, and screen-records that display with ffmpeg into a small looping GIF.
#
# The bot normally skips all rendering to run headless-fast; setting RUSTLER_RECORD makes
# it render the real scene at 1x speed (see src/main.rs). That env var is the only thing
# that changes game behaviour — with it unset the playtests are byte-identical.
#
# Usage: scripts/record-gameplay.sh [scenario] [output.gif] [seconds] [start-delay]
#   scenario     bot script to drive (default: npc_steal — catching + a growing train)
#   output.gif   where to write (default: screenshots/latest.gif)
#   seconds      clip length in real seconds (default: 6)
#   start-delay  seconds to wait before recording, to skip the menu (default: 4)
#
# Quality: 480px wide, 8fps, 128 colours, floyd_steinberg dithering — looks good in Slack
# without bloating the repo past ~1MB per clip. Versioned filenames build a history.
set -euo pipefail
cd "$(dirname "$0")/.."

SCENARIO="${1:-npc_steal}"
OUT="${2:-screenshots/latest.gif}"
SECS="${3:-6}"
START_DELAY="${4:-4}"
W=800; H=600            # ggez WindowMode::default()
GIF_W=480; FPS=8; COLORS=128

# --- provisioning: xvfb comes from ci-deps.sh; ffmpeg is the only extra the diary needs ---
if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "record-gameplay: installing ffmpeg..."
    SUDO=""; [ "$(id -u)" -ne 0 ] && SUDO="sudo"
    $SUDO apt-get update -qq && $SUDO apt-get install -y -qq ffmpeg
fi
if ! command -v Xvfb >/dev/null 2>&1; then
    echo "record-gameplay: Xvfb missing (run scripts/ci-deps.sh first)" >&2
    exit 1
fi

# --- build (prefer Nix when present, like playtest.sh) ---
if command -v nix >/dev/null 2>&1; then
    RUN_PREFIX=(nix develop . --command)
    "${RUN_PREFIX[@]}" cargo build 2>&1 | tail -1
else
    RUN_PREFIX=()
    export WGPU_BACKEND="${WGPU_BACKEND:-gl,gles}"
    cargo build 2>&1 | tail -1
fi

# --- pick a free X display, start Xvfb, always clean up ---
DISP=""
for n in 99 98 97 96 95; do
    if [ ! -e "/tmp/.X11-unix/X$n" ]; then DISP=":$n"; break; fi
done
[ -z "$DISP" ] && { echo "record-gameplay: no free X display" >&2; exit 1; }

XVFB_PID=""; GAME_PID=""
cleanup() {
    [ -n "$GAME_PID" ] && kill "$GAME_PID" 2>/dev/null || true
    [ -n "$XVFB_PID" ] && kill "$XVFB_PID" 2>/dev/null || true
}
trap cleanup EXIT

Xvfb "$DISP" -screen 0 "${W}x${H}x24" -nolisten tcp >/tmp/xvfb-record.log 2>&1 &
XVFB_PID=$!
sleep 2

# --- drive real gameplay on the virtual display (RUSTLER_RECORD => render at 1x) ---
DISPLAY="$DISP" RUSTLER_RECORD=1 "${RUN_PREFIX[@]}" ./target/debug/rustler --bot "$SCENARIO" \
    >/tmp/record-game.log 2>&1 &
GAME_PID=$!
sleep "$START_DELAY"     # let the menu clear and the player enter the game

# --- screen-record the display, then palette-compress to a small looping GIF ---
RAW=/tmp/record-clip.mp4
PAL=/tmp/record-pal.png
DISPLAY="$DISP" ffmpeg -hide_banner -loglevel error -y \
    -f x11grab -framerate 15 -video_size "${W}x${H}" -i "$DISP" \
    -t "$SECS" -c:v libx264 -qp 0 "$RAW" </dev/null

ffmpeg -hide_banner -loglevel error -y -i "$RAW" \
    -vf "fps=$FPS,scale=$GIF_W:-1:flags=lanczos,palettegen=max_colors=$COLORS:stats_mode=full" "$PAL"
ffmpeg -hide_banner -loglevel error -y -i "$RAW" -i "$PAL" \
    -lavfi "fps=$FPS,scale=$GIF_W:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=floyd_steinberg" \
    "$OUT"

# --- sanity: a real capture is tens-of-KB+; a black/static grab is a couple KB ---
BYTES=$(stat -c%s "$OUT" 2>/dev/null || echo 0)
if [ "$BYTES" -lt 20000 ]; then
    echo "record-gameplay: output looks empty ($BYTES bytes) — did the game render?" >&2
    exit 1
fi
echo "record-gameplay: wrote $OUT ($((BYTES/1024)) KB, ${SECS}s @ ${FPS}fps, ${GIF_W}px, ${COLORS} colors)"
