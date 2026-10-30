#!/usr/bin/env bash
# Regenerate packaging/THIRD-PARTY-NOTICES.html: the licences of every Rust crate in the ship
# builds (MIT/Apache and friends require their text to ship with the binaries). The build
# scripts copy it next to the game. Re-run after dependency changes. Needs cargo-about
# (`cargo install cargo-about`).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
cargo about generate --manifest-path "$ROOT/Cargo.toml" --config "$HERE/about.toml" \
  --no-default-features --features demo,telemetry --fail \
  -o "$ROOT/packaging/THIRD-PARTY-NOTICES.html" "$HERE/about.hbs"
ls -l "$ROOT/packaging/THIRD-PARTY-NOTICES.html"
