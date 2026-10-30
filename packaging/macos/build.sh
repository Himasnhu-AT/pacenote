#!/usr/bin/env bash
# Build Pacenote.app for the Steam macOS depot:
#
#   dist/steam/<edition>/macos/Pacenote.app      (Pacenote Demo.app for the demo)
#   dist/steam/<edition>/pacenote-<edition>-<version>-macos-<arch>.zip (+ .sha256), for testers
#
#   packaging/macos/build.sh                         # demo, universal (arm64 + x86_64)
#   EDITION=full packaging/macos/build.sh            # the full game
#   packaging/macos/build.sh --arch arm64            # Apple Silicon only (one build, faster)
#   packaging/macos/build.sh --skip-build            # re-bundle what the target dir holds
#   SIGN_ID="Developer ID Application: …" packaging/macos/build.sh --notarize
#
# Signing: without SIGN_ID the app is signed ad hoc (runs on this Mac and through Steam, but
# Gatekeeper warns if it is downloaded outside Steam). With SIGN_ID (a "Developer ID Application"
# identity in the keychain) it is signed with the hardened runtime and the entitlements in
# entitlements.plist (microphone). --notarize also sends it to Apple's notary service with the
# notarytool keychain profile NOTARY_PROFILE (default xcnotarize) and staples the ticket.
# See docs/STEAM.md, "macOS signing".
#
# Env: EDITION, FEATURES, ALLOW_MISSING_FEATURES, CARGO_TARGET_DIR, DIST (see ../common.sh),
#      BUNDLE_ID (default com.himanshuat.pacenote[.demo]), SIGN_ID, NOTARY_PROFILE.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"

ARCH=universal
SKIP_BUILD=0
NOTARIZE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --arch) ARCH="$2"; shift ;;
    --skip-build) SKIP_BUILD=1 ;;
    --notarize) NOTARIZE=1 ;;
    -h | --help) sed -n '2,22p' "$0"; exit 0 ;;
    *) echo "error: unknown argument $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done
case "$ARCH" in
  universal) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  arm64) TARGETS=(aarch64-apple-darwin) ;;
  x86_64) TARGETS=(x86_64-apple-darwin) ;;
  *) echo "error: --arch universal|arm64|x86_64" >&2; exit 2 ;;
esac

# shellcheck source=../common.sh
. "$HERE/../common.sh"

if [ "$EDITION" = demo ]; then
  NAME="Pacenote Demo"; BUNDLE_ID="${BUNDLE_ID:-com.himanshuat.pacenote.demo}"
else
  NAME="Pacenote"; BUNDLE_ID="${BUNDLE_ID:-com.himanshuat.pacenote}"
fi
OUT="$DIST/$EDITION/macos"
APP="$OUT/$NAME.app"
ZIP="$DIST/$EDITION/pacenote-$EDITION-$VERSION-macos-$ARCH.zip"
ICNS="$HERE/AppIcon.icns"
export MACOSX_DEPLOYMENT_TARGET=12.0

need lipo "Xcode command line tools"
need codesign "Xcode command line tools"
[ "$NOTARIZE" = 0 ] || [ -n "${SIGN_ID:-}" ] || die "--notarize needs SIGN_ID (a Developer ID Application identity)"

FEAT_ARGS=()
while IFS= read -r a; do FEAT_ARGS+=("$a"); done < <(features_args)

if [ "$SKIP_BUILD" = 0 ]; then
  for t in "${TARGETS[@]}"; do
    rustup target list --installed | grep -qx "$t" || die "rustup target $t missing (rustup target add $t)"
    step "cargo build --release --target $t ${FEAT_ARGS[*]}"
    # No live mail credentials reach a build script.
    SMTP_USER= SMTP_PASS= gate cargo build --release --locked --manifest-path "$ROOT/Cargo.toml" \
      --bin pacenote --target "$t" "${FEAT_ARGS[@]}"
  done
fi

step "Check: no dev tooling (control port / MCP) in the ship build"
if cargo tree --manifest-path "$ROOT/Cargo.toml" -e normal "${FEAT_ARGS[@]}" | grep -E "ashlar-(control|mcp|command)"; then
  die "dev tooling is in the ship build's dependency tree"
fi

step "Assemble $NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
BINS=()
for t in "${TARGETS[@]}"; do
  b="$CARGO_TARGET_DIR/$t/release/pacenote"
  [ -f "$b" ] || die "$b missing (build first, or drop --skip-build)"
  BINS+=("$b")
done
if [ "${#BINS[@]}" -gt 1 ]; then
  lipo -create -output "$APP/Contents/MacOS/pacenote" "${BINS[@]}"
else
  cp "${BINS[0]}" "$APP/Contents/MacOS/pacenote"
fi
lipo -info "$APP/Contents/MacOS/pacenote"
# The engine looks for Contents/Resources/assets first when it runs inside a bundle.
copy_assets "$APP/Contents/Resources/assets"
copy_docs "$APP/Contents/Resources"
sed -e "s/__NAME__/$NAME/g" -e "s/__BUNDLE_ID__/$BUNDLE_ID/" -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD_NO/" \
  "$HERE/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null
if [ -f "$ICNS" ]; then cp "$ICNS" "$APP/Contents/Resources/AppIcon.icns"; else warn "no $ICNS: the app has the default icon"; fi

# A dev path compiled in (GameInfo::assets_fallback) is harmless but leaks the build folder.
if strings "$APP/Contents/MacOS/pacenote" | grep "$GAME_DIR" >/dev/null; then
  warn "the binary contains the build machine's path ($GAME_DIR); see docs/STEAM.md, 'Shipped asset paths'"
fi

if [ -n "${SIGN_ID:-}" ]; then
  step "Sign with $SIGN_ID (hardened runtime, secure timestamp)"
  codesign --force --timestamp --options runtime --entitlements "$HERE/entitlements.plist" --sign "$SIGN_ID" "$APP/Contents/MacOS/pacenote"
  codesign --force --timestamp --options runtime --entitlements "$HERE/entitlements.plist" --sign "$SIGN_ID" "$APP"
else
  step "Sign ad hoc (set SIGN_ID for a Developer ID signature)"
  codesign --force --options runtime --entitlements "$HERE/entitlements.plist" --sign - "$APP"
fi
codesign --verify --strict --deep --verbose=2 "$APP"

if [ "$NOTARIZE" = 1 ]; then
  step "Notarize (a few minutes; needs the keychain unlocked)"
  NZ="$(mktemp -d)/notarize.zip"
  ditto -c -k --keepParent "$APP" "$NZ"
  xcrun notarytool submit "$NZ" --keychain-profile "${NOTARY_PROFILE:-xcnotarize}" --wait
  xcrun stapler staple "$APP"
  spctl -a -t exec -vv "$APP"
fi

step "Zip for testers"
rm -f "$ZIP" "$ZIP.sha256"
( cd "$OUT" && ditto -c -k --keepParent "$NAME.app" "$ZIP" )
( cd "$(dirname "$ZIP")" && shasum -a 256 "$(basename "$ZIP")" >"$(basename "$ZIP").sha256" )

step "Done"
du -sh "$APP" "$ZIP"
echo "Steam depot content root: $OUT"
