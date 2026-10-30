#!/usr/bin/env bash
# Upload a built edition to Steam with steamcmd (SteamPipe).
#
#   packaging/steam/upload.sh demo --preview          # build the manifests, upload nothing
#   packaging/steam/upload.sh demo                    # upload the demo; set live on a beta branch
#   packaging/steam/upload.sh full --setlive testing  # upload the full game to branch "testing"
#
# Before: run packaging/windows/build.sh and packaging/macos/build.sh with the same EDITION, so
# dist/steam/<edition>/windows and .../macos hold what ships.
#
# Ids (from Steamworks, App Admin; none are secret, but they live in your shell or in
# packaging/steam/ids.env, which is gitignored; see ids.env.example):
#   STEAM_DEMO_APPID  STEAM_DEMO_DEPOT_WIN  STEAM_DEMO_DEPOT_MAC
#   STEAM_FULL_APPID  STEAM_FULL_DEPOT_WIN  STEAM_FULL_DEPOT_MAC
#   STEAM_BUILDER     the Steam account that uploads (a separate builder account with only
#                     "Edit App Metadata" + "Publish App Changes To Steam" is best)
#
# Credentials are never stored here or passed on the command line: steamcmd asks for the
# password and the Steam Guard code the first time and caches a login token afterwards.
#
# Options: --preview, --setlive BRANCH (default: none; the build just appears under Builds),
#          --desc TEXT, --steamcmd PATH (default: steamcmd on PATH, or STEAMCMD).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"

EDITION="${1:-}"
case "$EDITION" in demo | full) shift ;; -h | --help | "") sed -n '2,25p' "$0"; exit 0 ;; *) echo "error: first argument: demo | full" >&2; exit 2 ;; esac
PREVIEW=0
SETLIVE=""
DESC=""
STEAMCMD="${STEAMCMD:-$(command -v steamcmd || true)}"
while [ $# -gt 0 ]; do
  case "$1" in
    --preview) PREVIEW=1 ;;
    --setlive) SETLIVE="$2"; shift ;;
    --desc) DESC="$2"; shift ;;
    --steamcmd) STEAMCMD="$2"; shift ;;
    *) echo "error: unknown argument $1" >&2; exit 2 ;;
  esac
  shift
done
[ "$SETLIVE" != default ] || { echo "error: set the default (public) branch live by hand in Steamworks, not from a script" >&2; exit 2; }

# shellcheck disable=SC1091
[ -f "$HERE/ids.env" ] && . "$HERE/ids.env"
UP="$(echo "$EDITION" | tr a-z A-Z)"
APPID_VAR="STEAM_${UP}_APPID"; WIN_VAR="STEAM_${UP}_DEPOT_WIN"; MAC_VAR="STEAM_${UP}_DEPOT_MAC"
APPID="${!APPID_VAR:-}"; DEPOT_WIN="${!WIN_VAR:-}"; DEPOT_MAC="${!MAC_VAR:-}"
for v in "$APPID_VAR=$APPID" "$WIN_VAR=$DEPOT_WIN" "$MAC_VAR=$DEPOT_MAC" "STEAM_BUILDER=${STEAM_BUILDER:-}"; do
  [ -n "${v#*=}" ] || { echo "error: ${v%%=*} is not set (see packaging/steam/ids.env.example)" >&2; exit 1; }
done
for v in "$APPID" "$DEPOT_WIN" "$DEPOT_MAC"; do
  [[ "$v" =~ ^[0-9]+$ ]] || { echo "error: Steam ids are numbers (got '$v')" >&2; exit 1; }
done

CONTENT="$ROOT/dist/steam/$EDITION"
[ -f "$CONTENT/windows/Pacenote.exe" ] || { echo "error: $CONTENT/windows/Pacenote.exe missing: run packaging/windows/build.sh (EDITION=$EDITION)" >&2; exit 1; }
ls -d "$CONTENT"/macos/*.app >/dev/null 2>&1 || { echo "error: no .app in $CONTENT/macos: run packaging/macos/build.sh (EDITION=$EDITION)" >&2; exit 1; }
[ -n "$STEAMCMD" ] && [ -x "$STEAMCMD" ] || { echo "error: steamcmd not found (docs/STEAM.md, 'steamcmd'), or pass --steamcmd PATH" >&2; exit 1; }

VERSION="$(awk '/^\[/{p=($0=="[package]");next} p&&$1=="version"{gsub(/[" ]/,"",$3);print $3;exit}' "$ROOT/Cargo.toml")"
SHA="$(git -C "$ROOT" rev-parse --short HEAD)"
DESC="${DESC:-pacenote $EDITION $VERSION ($SHA)}"
# steamcmd writes logs and chunk caches next to the scripts: keep them out of the repo.
WORK="$ROOT/dist/steam/.steampipe/$EDITION"
mkdir -p "$WORK/output"

fill() {
  sed -e "s|__APPID__|$APPID|g" -e "s|__DESC__|${DESC//|/ }|g" -e "s|__PREVIEW__|$PREVIEW|g" \
    -e "s|__SETLIVE__|$SETLIVE|g" -e "s|__CONTENT_ROOT__|$CONTENT|g" -e "s|__BUILD_OUTPUT__|$WORK/output|g" \
    -e "s|__DEPOT_WIN__|$DEPOT_WIN|g" -e "s|__DEPOT_MAC__|$DEPOT_MAC|g" "$@"
}
APP_VDF="$WORK/app_build_$APPID.vdf"
fill "$HERE/app_build.vdf.in" >"$APP_VDF"
fill -e "s|__DEPOT__|$DEPOT_WIN|g" -e "s|__SUBDIR__|windows|g" "$HERE/depot_build.vdf.in" >"$WORK/depot_build_$DEPOT_WIN.vdf"
fill -e "s|__DEPOT__|$DEPOT_MAC|g" -e "s|__SUBDIR__|macos|g" "$HERE/depot_build.vdf.in" >"$WORK/depot_build_$DEPOT_MAC.vdf"

echo "app $APPID ($EDITION): windows depot $DEPOT_WIN, macos depot $DEPOT_MAC"
echo "content: $CONTENT   preview: $PREVIEW   set live: ${SETLIVE:-<none>}"
echo "scripts: $WORK"
du -sh "$CONTENT/windows" "$CONTENT/macos"

# steamcmd prompts for the password / Steam Guard code itself when no cached login exists.
"$STEAMCMD" +login "$STEAM_BUILDER" +run_app_build "$APP_VDF" +quit
echo "Done. Steamworks > App Admin > SteamPipe > Builds shows the new build (BuildID in $WORK/output)."
