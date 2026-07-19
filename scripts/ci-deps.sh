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
# Loosely mirrors the Linux buildInputs in default.nix, plus xvfb + a software
# GL driver so bot mode can render offscreen. default.nix also lists
# glib/gtk3/cairo/pango/gdk-pixbuf, but `ldd target/debug/rustler` shows the
# actual binary never links against any of them (nothing in Cargo.lock pulls
# in a GTK/Cairo/Pango binding — no `rfd`-style native dialog crate), and a
# full `cargo build` + `bash scripts/playtest.sh` (all 5 scenarios) passes
# with that whole GTK toolchain absent. Skipping it here avoids apt pulling in
# its large, unused transitive chain (icon themes, at-spi, etc.) on every
# CI/Playtest job.
$SUDO apt-get install -y -qq \
  pkg-config \
  libasound2-dev libudev-dev libdbus-1-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxext-dev \
  libxinerama-dev libxxf86vm-dev libxrender-dev libxcb1-dev libxau-dev libxdmcp-dev \
  libxkbcommon-dev libwayland-dev wayland-protocols libvulkan-dev \
  libfreetype-dev libfontconfig1-dev zlib1g-dev \
  libgl1-mesa-dev libgl1-mesa-dri xvfb

echo "ci-deps: done."
