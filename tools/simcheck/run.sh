#!/bin/bash
# Cross-CPU determinism check: builds tools/simcheck (the game's own vehicle.rs and stage.rs,
# no Bevy) for this Mac's arm64 and for x86_64 (run under Rosetta), runs both and compares the
# scripted-drive hash and all 20 stage hashes. Exit 0 = identical.
#
#   tools/simcheck/run.sh
#
# Needs the x86_64 target once: rustup target add x86_64-apple-darwin.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
export CARGO_TARGET_DIR="$here/target"
cd "$here"
memgate -- cargo build -q --release
memgate -- cargo build -q --release --target x86_64-apple-darwin
arm="$("$CARGO_TARGET_DIR/release/simcheck")"
x86="$(arch -x86_64 "$CARGO_TARGET_DIR/x86_64-apple-darwin/release/simcheck")"
if [ "$arm" = "$x86" ]; then
    echo "$arm"
    echo "simcheck: arm64 and x86_64 identical"
else
    echo "simcheck: arm64 and x86_64 differ" >&2
    diff <(echo "$arm") <(echo "$x86") >&2 || true
    exit 1
fi
