#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

# Prefer the Nix dev shell when it's available (CI and local development).
# Otherwise fall back to plain cargo so the bot playtests also run in
# cargo-only environments — e.g. Claude's remote routine sessions, which have
# no Nix. Run scripts/ci-deps.sh first there to install the system libraries
# and configure headless audio.
if command -v nix >/dev/null 2>&1; then
    BUILD=(nix develop . --command cargo build)
    RUN_PREFIX=(nix develop . --command)
else
    BUILD=(cargo build)
    RUN_PREFIX=()
    export WGPU_BACKEND="${WGPU_BACKEND:-gl,gles}"
    export LIBGL_ALWAYS_INDIRECT="${LIBGL_ALWAYS_INDIRECT:-1}"
    # Bot mode still initializes the window backend; give it a virtual display
    # when we're headless and xvfb is installed.
    if [ -z "${DISPLAY:-}" ] && command -v xvfb-run >/dev/null 2>&1; then
        RUN_PREFIX=(xvfb-run -a)
    fi
fi

echo "Building..."
"${BUILD[@]}" 2>&1 | tail -1

PASS=0
FAIL=0

run_script() {
    local name=$1
    echo -n "Running $name ... "
    # Bot mode still initializes the window backend, so CI may wrap this script in xvfb-run.
    "${RUN_PREFIX[@]}" ./target/debug/rustler --bot "$name" 2>&1 | tee "/tmp/bot_$name.log"
    local exitcode=${PIPESTATUS[0]}

    if [ $exitcode -eq 0 ]; then
        echo "PASS"
        PASS=$((PASS+1))
    else
        echo "FAIL"
        FAIL=$((FAIL+1))
    fi
}

run_script groove_dash
run_script menu_to_game
run_script campaign_tutorial
run_script npc_steal

echo ""
echo "Results: $PASS passed, $FAIL failed"
[ $FAIL -eq 0 ]
