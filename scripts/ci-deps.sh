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
# Mirrors the Linux buildInputs in default.nix, plus xvfb + a software GL driver
# so bot mode can render offscreen.
$SUDO apt-get install -y -qq \
  pkg-config \
  libasound2-dev libudev-dev libdbus-1-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxext-dev \
  libxinerama-dev libxxf86vm-dev libxrender-dev libxcb1-dev libxau-dev libxdmcp-dev \
  libxkbcommon-dev libwayland-dev wayland-protocols libvulkan-dev \
  libfreetype-dev libfontconfig1-dev zlib1g-dev \
  libglib2.0-dev libgtk-3-dev libcairo2-dev libpango1.0-dev libgdk-pixbuf-2.0-dev \
  libgl1-mesa-dev libgl1-mesa-dri xvfb

echo "ci-deps: done."
