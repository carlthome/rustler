#!/usr/bin/env bash
#
# Provision a cargo-only build/playtest environment (no Nix).
#
# The Nix dev shell normally supplies the game's C libraries and a headless
# audio device. In environments without Nix — notably Claude's remote routine
# sessions, which get a fresh ephemeral container each run — this script
# installs the same system dependencies with apt and points ALSA at a null
# device so the game can build and the bot playtests can launch offscreen.
#
# Idempotent: safe to run at the top of every session. It no-ops quickly once
# the alsa dev headers are already present.
set -euo pipefail

# Nix-first: when the dev shell is available it already supplies every
# dependency, so this script does nothing. It only provisions in cargo-only
# environments without Nix (e.g. Claude's remote routine sandboxes). This keeps
# it safe to run unconditionally from a SessionStart hook on a Nix machine.
if command -v nix >/dev/null 2>&1; then
  echo "ci-deps: Nix detected — dev shell provides dependencies, nothing to do."
  exit 0
fi

# Headless ALSA: route the default PCM to null so audio init succeeds when the
# container has no sound card (otherwise ggez aborts with an AudioError).
if [ ! -f "$HOME/.asoundrc" ]; then
  printf '%s\n' 'pcm.!default {' '  type null' '}' > "$HOME/.asoundrc"
fi

# System libraries. Skip the (slow) apt work when the headers are already there.
if pkg-config --exists alsa 2>/dev/null; then
  echo "ci-deps: system libraries already present, skipping apt."
  exit 0
fi

SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  else
    echo "ci-deps: need root or sudo to install packages" >&2
    exit 1
  fi
fi

export DEBIAN_FRONTEND=noninteractive
$SUDO apt-get update -qq
# Trimmed to what Cargo.lock actually links against, plus xvfb + software GPU
# drivers so bot mode can render offscreen. default.nix's buildInputs are wider
# (glib/gtk3/cairo/pango/gdk-pixbuf/dbus/freetype/fontconfig) but none of those
# have a corresponding -sys crate in Cargo.lock: text rendering goes through the
# pure-Rust ab_glyph/glyph_brush stack and X11/Wayland access is dlopen'd via
# x11-dl/wayland-client.
#
# ggez 0.10 (wgpu 29 / winit 0.30) changed two runtime requirements vs 0.9:
#   • ggez picks wgpu Backends::PRIMARY first (Vulkan on Linux), ignoring the
#     WGPU_BACKEND env var, and only falls back to GL as SECONDARY. The software
#     GL/EGL path no longer initializes headless under wgpu 29, so we install the
#     Mesa software Vulkan driver (lavapipe: libvulkan1 + mesa-vulkan-drivers) to
#     give that PRIMARY backend a working ICD. This is why Vulkan is no longer
#     "never touched" — it is now the backend the game actually renders through.
#   • winit 0.30 dlopens libxkbcommon-x11 (separate from libxkbcommon) for X11
#     keyboard input, so bot mode panics in xkbcommon-dl without it.
$SUDO apt-get install -y -qq \
  pkg-config \
  libasound2-dev libudev-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxext-dev \
  libxinerama-dev libxxf86vm-dev libxrender-dev libxcb1-dev libxau-dev libxdmcp-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev wayland-protocols \
  libgl1-mesa-dev libgl1-mesa-dri libvulkan1 mesa-vulkan-drivers xvfb

echo "ci-deps: done."
