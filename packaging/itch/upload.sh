#!/usr/bin/env bash
# Push the Pacenote demo to itch.io (https://himanshuat.itch.io/pacenote) with butler.
#
#   packaging/itch/upload.sh            # push both platforms
#   packaging/itch/upload.sh --dry-run  # show what would be pushed
#
# Build first (the demo's store buttons then say "Follow on itch.io"):
#   export PACENOTE_STORE_URL=https://himanshuat.itch.io/pacenote DIST=$PWD/dist/itch
#   packaging/windows/build.sh && SIGN_ID="Developer ID Application: …" packaging/macos/build.sh --notarize
#
# butler must be logged in once (`butler login`). Channels: windows, osx (itch tags the
# platform from the channel name).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TARGET="${ITCH_TARGET:-himanshuat/pacenote}"
DIST="${DIST:-$ROOT/dist/itch}/demo"
VERSION="$(awk '/^\[/{p=($0=="[package]");next} p&&$1=="version"{gsub(/[" ]/,"",$3);print $3;exit}' "$ROOT/Cargo.toml")"
BUTLER="${BUTLER:-$(command -v butler || echo "$HOME/.local/bin/butler")}"
DRY=""; [ "${1:-}" = --dry-run ] && DRY="--dry-run"

[ -d "$DIST/windows" ] || { echo "error: $DIST/windows missing (build first)" >&2; exit 1; }
[ -d "$DIST/macos/Pacenote Demo.app" ] || { echo "error: $DIST/macos/Pacenote Demo.app missing (build first)" >&2; exit 1; }

# The .itch.toml tells the itch app what to launch.
cat >"$DIST/windows/.itch.toml" <<'T'
[[actions]]
name = "play"
path = "Pacenote.exe"
T
"$BUTLER" push $DRY "$DIST/windows" "$TARGET:windows" --userversion "$VERSION"
# The macOS folder holds only the signed, notarized .app.
"$BUTLER" push $DRY "$DIST/macos" "$TARGET:osx" --userversion "$VERSION"
"$BUTLER" status "$TARGET"
