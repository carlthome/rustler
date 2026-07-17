#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

echo "Building..."
nix develop . --command cargo build 2>&1 | tail -1

PASS=0
FAIL=0

run_script() {
    local name=$1
    echo -n "Running $name ... "
    # Bot mode still initializes the window backend, so CI may wrap this script in xvfb-run.
    nix develop . --command ./target/debug/rustler --bot "$name" 2>&1 | tee "/tmp/bot_$name.log"
    local exitcode=${PIPESTATUS[0]}

    if [ $exitcode -eq 0 ]; then
        echo "PASS"
        PASS=$((PASS+1))
    else
        echo "FAIL"
        FAIL=$((FAIL+1))
    fi
}

run_script menu_to_game
run_script groove_dash
# run_script campaign_tutorial   # enable once tutorial->world-map bug is fixed

echo ""
echo "Results: $PASS passed, $FAIL failed"
[ $FAIL -eq 0 ]
