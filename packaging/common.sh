# shellcheck shell=bash
# Shared setup for packaging/{macos,windows}/build.sh. Source it, don't run it.
#
# Reads:
#   EDITION            demo (default) | full
#   FEATURES           overrides the cargo features picked from EDITION
#   ALLOW_MISSING_FEATURES=1
#                      drop features Cargo.toml doesn't define yet (with a warning) instead of
#                      failing. Only for test builds before `demo` / `telemetry` exist.
#   CARGO_TARGET_DIR   kept when set; default ../pacenote-build/target-ship (never the shared
#                      Shroud target dir from .cargo/config.toml, so dev builds aren't clobbered)
#   DIST               output root; default <repo>/dist/steam
#   MEMGATE            memgate binary (default: the one on PATH); MEMGATE_PROFILE (default release)
#   PACENOTE_TRACE_ENDPOINT / PACENOTE_TRACE_KEY
#                      compiled into the `telemetry` build; never printed here
#
# Exports: ROOT VERSION BUILD_NO EDITION FEATURES CARGO_TARGET_DIR DIST RUSTFLAGS, and the helpers
# step / die / need / gate / features_args / copy_docs.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GAME_DIR="$(cd "$ROOT/.." && pwd)" # holds pacenote/ and engine/ (path dependency)
export ROOT

step() { printf '\n==> %s\n' "$*"; }
die() { echo "error: $*" >&2; exit 1; }
warn() { echo "warning: $*" >&2; }
need() { command -v "$1" >/dev/null || die "$1 not found ($2)"; }

# The version lives in Cargo.toml's [package] only.
VERSION="$(awk '/^\[/{p=($0=="[package]");next} p&&$1=="version"{gsub(/[" ]/,"",$3);print $3;exit}' "$ROOT/Cargo.toml")"
[ -n "$VERSION" ] || die "could not read [package] version from Cargo.toml"
BUILD_NO="$(git -C "$ROOT" rev-list --count HEAD 2>/dev/null || echo 0)"
GIT_SHA="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
export VERSION BUILD_NO GIT_SHA

EDITION="${EDITION:-demo}"
case "$EDITION" in
  # Ship builds: no dev tooling (control port, MCP, devkit), the engine tracer for telemetry,
  # and for the demo the stage limit and wishlist prompts.
  demo) WANT_FEATURES="${FEATURES:-demo,telemetry}" ;;
  full) WANT_FEATURES="${FEATURES:-telemetry}" ;;
  *) die "EDITION must be demo or full (got $EDITION)" ;;
esac

# Features Cargo.toml defines right now.
defined_features() {
  awk '/^\[/{p=($0=="[features]");next} p&&/^[A-Za-z0-9_-]+[ ]*=/{sub(/[ ]*=.*/,"");print}' "$ROOT/Cargo.toml"
}
FEATURES=""
missing=""
for f in ${WANT_FEATURES//,/ }; do
  if defined_features | grep -qx "$f"; then
    FEATURES="${FEATURES:+$FEATURES,}$f"
  else
    missing="${missing:+$missing,}$f"
  fi
done
if [ -n "$missing" ]; then
  if [ "${ALLOW_MISSING_FEATURES:-0}" = 1 ]; then
    warn "Cargo.toml has no feature(s) '$missing' yet: building without them (TEST BUILD, don't ship)"
  else
    die "Cargo.toml has no feature(s) '$missing'. Add them, or set ALLOW_MISSING_FEATURES=1 for a test build"
  fi
fi
export EDITION FEATURES
features_args() {
  # Always --no-default-features: the default `dev` feature ships the control port and MCP.
  if [ -n "$FEATURES" ]; then printf '%s\n' --no-default-features --features "$FEATURES"; else printf '%s\n' --no-default-features; fi
}

if echo ",$FEATURES," | grep -q ",telemetry,"; then
  if [ -z "${PACENOTE_TRACE_ENDPOINT:-}" ] || [ -z "${PACENOTE_TRACE_KEY:-}" ]; then
    warn "PACENOTE_TRACE_ENDPOINT / PACENOTE_TRACE_KEY not set: this build will send no telemetry"
  fi
fi

if echo ",$FEATURES," | grep -q ",demo,"; then
  # The demo's wishlist buttons open this (the full game's store page), compiled in.
  [ -n "${PACENOTE_STEAM_URL:-}" ] || warn "PACENOTE_STEAM_URL not set: the demo's wishlist buttons have no store page to open"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$GAME_DIR/pacenote-build/target-ship}"
DIST="${DIST:-$ROOT/dist/steam}"
mkdir -p "$DIST"
export DIST

# Rewrite build-machine paths (home folder, user name) out of panic messages and debug strings.
# RUSTFLAGS must not hold paths with spaces (it splits on whitespace).
for p in "$GAME_DIR" "${CARGO_HOME:-$HOME/.cargo}" "${RUSTUP_HOME:-$HOME/.rustup}" "$CARGO_TARGET_DIR"; do
  case "$p" in *" "*) die "path with a space can't go in RUSTFLAGS: $p" ;; esac
done
RUSTFLAGS="${RUSTFLAGS:-}"
RUSTFLAGS="$RUSTFLAGS --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
RUSTFLAGS="$RUSTFLAGS --remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup"
RUSTFLAGS="$RUSTFLAGS --remap-path-prefix=$GAME_DIR=/src"
RUSTFLAGS="$RUSTFLAGS --remap-path-prefix=$CARGO_TARGET_DIR=/target"
export RUSTFLAGS

# Heavy commands go through memgate (queued with every other build on this Mac).
MEMGATE="${MEMGATE:-$(command -v memgate || true)}"
gate() {
  if [ -n "$MEMGATE" ]; then "$MEMGATE" --profile "${MEMGATE_PROFILE:-release}" -- "$@"; else "$@"; fi
}

# Credits, licences and a readme into a shipped folder.
copy_docs() {
  local dest="$1"
  cp "$ROOT/assets/credits.txt" "$dest/CREDITS.txt"
  if [ -f "$ROOT/packaging/THIRD-PARTY-NOTICES.html" ]; then
    cp "$ROOT/packaging/THIRD-PARTY-NOTICES.html" "$dest/"
  else
    warn "packaging/THIRD-PARTY-NOTICES.html missing: run packaging/licenses/notices.sh (open-source licences must ship)"
  fi
}

# Copy the game's assets/ (cooked files only).
copy_assets() {
  local dest="$1"
  mkdir -p "$dest"
  rsync -a --delete --exclude '.DS_Store' --exclude '*.md' --exclude '*.blend' --exclude '*.blend1' \
    --exclude 'textures/*/*.jpg' --exclude 'textures/*/*.png' \
    "$ROOT/assets/" "$dest/"
}

echo "pacenote $VERSION (build $BUILD_NO, $GIT_SHA) edition=$EDITION features=${FEATURES:-<none>}"
echo "target dir: $CARGO_TARGET_DIR   dist: $DIST"
