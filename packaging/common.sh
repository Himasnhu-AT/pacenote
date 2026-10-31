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
#   PACENOTE_STORE_URL / PACENOTE_STEAM_URL
#                      the demo's store page (an itch.io URL: "Follow on itch.io" buttons)
#   PACENOTE_TRACE_ENDPOINT
#                      telemetry server, compiled into the `telemetry` build
#                      (default https://api.marlstudio.com)
#   PACENOTE_TRACE_KEY the app's write key (ingest only), compiled in. Default: the
#                      PACENOTE_WRITE_KEY line of $PACENOTE_KEYS_FILE
#                      (~/codes/synthai/backend/infra/secrets/keys.env). Never printed: logs
#                      say "set (N chars)" or "not set". Missing: the build works, sends nothing.
#   PACENOTE_BUILD_SHA set here to the git sha (compiled in as the build id; dev builds have none)
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
# The telemetry build id: packaged builds only (a dev `cargo run` has none = test data).
export PACENOTE_BUILD_SHA="$GIT_SHA"

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
  export PACENOTE_TRACE_ENDPOINT="${PACENOTE_TRACE_ENDPOINT:-https://api.marlstudio.com}"
  if [ -z "${PACENOTE_TRACE_KEY:-}" ]; then
    # Only the PACENOTE_WRITE_KEY line is read from the keys file; nothing else in it.
    keys_file="${PACENOTE_KEYS_FILE:-$HOME/codes/synthai/backend/infra/secrets/keys.env}"
    if [ -f "$keys_file" ]; then
      line="$(grep -E '^[[:space:]]*(export[[:space:]]+)?PACENOTE_WRITE_KEY=' "$keys_file" | tail -n 1 || true)"
      key="${line#*=}"
      key="${key%\"}"; key="${key#\"}"; key="${key%\'}"; key="${key#\'}"
      [ -n "$key" ] && export PACENOTE_TRACE_KEY="$key"
      unset line key
    fi
  fi
  echo "telemetry: endpoint $PACENOTE_TRACE_ENDPOINT, build $PACENOTE_BUILD_SHA"
  if [ -n "${PACENOTE_TRACE_KEY:-}" ]; then
    echo "telemetry: key set (${#PACENOTE_TRACE_KEY} chars)"
  else
    echo "telemetry: key not set"
    warn "no PACENOTE_TRACE_KEY (env) or PACENOTE_WRITE_KEY (in ${PACENOTE_KEYS_FILE:-$HOME/codes/synthai/backend/infra/secrets/keys.env}): this build works but sends no telemetry. Create the app on the server first (docs/TELEMETRY.md)."
  fi
fi

if echo ",$FEATURES," | grep -q ",demo,"; then
  # The demo's store buttons open this (the full game's store page), compiled in.
  # PACENOTE_STORE_URL wins (an itch.io URL makes them "Follow on itch.io"), else PACENOTE_STEAM_URL.
  [ -n "${PACENOTE_STORE_URL:-}${PACENOTE_STEAM_URL:-}" ] || warn "PACENOTE_STORE_URL / PACENOTE_STEAM_URL not set: the demo's store buttons have no store page to open"
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
