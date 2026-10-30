#!/usr/bin/env bash
# Cross-compile Pacenote for Windows x64 on macOS (or Linux) into a plain folder for the Steam
# Windows depot (Steam installs folders; no installer):
#
#   dist/steam/<edition>/windows/Pacenote.exe
#   dist/steam/<edition>/windows/assets/…
#   dist/steam/<edition>/windows/CREDITS.txt, THIRD-PARTY-NOTICES.html
#   dist/steam/<edition>/pacenote-<edition>-<version>-windows-x64.zip (+ .sha256), for testers
#
#   packaging/windows/build.sh                  # demo
#   EDITION=full packaging/windows/build.sh     # the full game
#   packaging/windows/build.sh --skip-build     # re-stage what the target dir holds
#
# How: cargo-xwin (MSVC target; it downloads and caches the Windows SDK / CRT on first use,
# ~1 GB in ~/Library/Caches/cargo-xwin), static C runtime (no Visual C++ redistributable needed),
# the GUI subsystem (no console window behind the game), and the icon + version info linked in
# from a .res made by llvm-rc. All three come in as linker/rustc flags here, so src/ needs no
# build.rs. The exe is not code-signed (Steam doesn't require it; see docs/STEAM.md).
#
# Needs: rustup target x86_64-pc-windows-msvc, cargo-xwin (`cargo install cargo-xwin`),
# llvm-rc (`brew install llvm`, or RC_PATH), python3 (PE header check), magick (icon; optional).
# Env: EDITION, FEATURES, ALLOW_MISSING_FEATURES, CARGO_TARGET_DIR, DIST (see ../common.sh).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"

SKIP_BUILD=0
for a in "$@"; do
  case "$a" in
    --skip-build) SKIP_BUILD=1 ;;
    -h | --help) sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "error: unknown argument $a (see --help)" >&2; exit 2 ;;
  esac
done

# Static CRT: the exe runs on a clean Windows 10/11 without the VC++ redistributable.
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static"
# shellcheck source=../common.sh
. "$HERE/../common.sh"

TARGET=x86_64-pc-windows-msvc
EXE=Pacenote.exe
OUT="$DIST/$EDITION/windows"
ZIP="$DIST/$EDITION/pacenote-$EDITION-$VERSION-windows-x64.zip"
BIN="$CARGO_TARGET_DIR/$TARGET/release/pacenote.exe"
RES_DIR="$CARGO_TARGET_DIR/winres-$EDITION"
ICO="$HERE/pacenote.ico"

need python3 "used to check the PE headers"
FEAT_ARGS=()
while IFS= read -r a; do FEAT_ARGS+=("$a"); done < <(features_args)

if [ "$SKIP_BUILD" = 0 ]; then
  need cargo-xwin "cargo install cargo-xwin"
  rustup target list --installed | grep -qx "$TARGET" || die "rustup target $TARGET missing (rustup target add $TARGET)"
  if [ -z "${RC_PATH:-}" ]; then
    for rc in "$(command -v llvm-rc || true)" /opt/homebrew/opt/llvm/bin/llvm-rc /usr/local/opt/llvm/bin/llvm-rc; do
      if [ -n "$rc" ] && [ -x "$rc" ]; then RC_PATH="$rc"; break; fi
    done
  fi
  [ -n "${RC_PATH:-}" ] || die "llvm-rc not found (brew install llvm, or set RC_PATH)"

  step "Resources: icon + VERSIONINFO ($RES_DIR/pacenote.res)"
  mkdir -p "$RES_DIR"
  if [ "$EDITION" = demo ]; then PRODUCT="Pacenote Demo"; else PRODUCT="Pacenote"; fi
  IFS=. read -r V1 V2 V3 <<<"${VERSION%%-*}"
  {
    if [ -f "$ICO" ]; then printf '1 ICON "%s"\n' "$ICO"; else warn "no $ICO: the exe has no icon"; fi
    cat <<EOF
1 VERSIONINFO
FILEVERSION $V1,$V2,$V3,$BUILD_NO
PRODUCTVERSION $V1,$V2,$V3,$BUILD_NO
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Pacenote"
      VALUE "FileDescription", "$PRODUCT"
      VALUE "FileVersion", "$VERSION"
      VALUE "InternalName", "pacenote"
      VALUE "OriginalFilename", "$EXE"
      VALUE "ProductName", "$PRODUCT"
      VALUE "ProductVersion", "$VERSION ($GIT_SHA)"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
EOF
  } >"$RES_DIR/pacenote.rc"
  # Relative names: llvm-rc on macOS reads an argument starting with / as an option.
  ( cd "$RES_DIR" && "$RC_PATH" -nologo -fo pacenote.res pacenote.rc )

  step "cargo xwin rustc --release --target $TARGET ${FEAT_ARGS[*]}"
  # `cargo rustc` passes the flags after `--` to the pacenote binary only (not its dependencies):
  # the .res, and the GUI subsystem with the normal entry point (what
  # #![windows_subsystem = "windows"] does).
  SMTP_USER= SMTP_PASS= gate cargo xwin rustc --release --locked --manifest-path "$ROOT/Cargo.toml" \
    --bin pacenote --target "$TARGET" "${FEAT_ARGS[@]}" -- \
    -C "link-arg=$RES_DIR/pacenote.res" -C link-arg=/SUBSYSTEM:WINDOWS -C link-arg=/ENTRY:mainCRTStartup
fi

[ -f "$BIN" ] || die "$BIN missing (build first, or drop --skip-build)"

step "Check the exe: x64, GUI subsystem, version info, no MSVC runtime DLLs"
python3 -I - "$BIN" <<'EOF'
import struct, sys
path = sys.argv[1]
b = open(path, "rb").read()
pe = struct.unpack_from("<I", b, 0x3C)[0]
machine = struct.unpack_from("<H", b, pe + 4)[0]
sub = struct.unpack_from("<H", b, pe + 0x5C)[0]
errors = []
if machine != 0x8664: errors.append(f"machine 0x{machine:X}, expected 0x8664 (x64)")
if sub != 2: errors.append(f"PE subsystem {sub}, expected 2 (GUI: no console window)")
if "VS_VERSION_INFO".encode("utf-16-le") not in b: errors.append("no VERSIONINFO resource")
low = b.lower()
for dll in (b"vcruntime140.dll", b"msvcp140.dll"):
    if dll in low: errors.append(f"imports {dll.decode()} (the static CRT didn't apply)")
if errors: sys.exit(f"{path}: " + "; ".join(errors))
print(f"ok: x64, GUI subsystem, VERSIONINFO, static CRT ({len(b) / 1e6:.1f} MB)")
EOF

step "Stage $OUT"
rm -rf "$OUT"
mkdir -p "$OUT"
cp "$BIN" "$OUT/$EXE"
# Next to the exe: the engine finds ./assets when Steam starts the game in its install folder
# (the default working directory). See docs/STEAM.md, "Shipped asset paths".
copy_assets "$OUT/assets"
copy_docs "$OUT"
# Text files with Windows line endings, so Notepad shows them properly.
for f in "$OUT/CREDITS.txt"; do perl -pi -e 's/\r?\n/\r\n/' "$f"; done
if strings "$OUT/$EXE" | grep "$GAME_DIR" >/dev/null; then
  warn "the exe contains the build machine's path ($GAME_DIR); see docs/STEAM.md, 'Shipped asset paths'"
fi

step "Zip for testers"
rm -f "$ZIP" "$ZIP.sha256"
# Unzips into a Pacenote/ folder (zip follows the temporary link).
( cd "$DIST/$EDITION" && rm -f Pacenote && ln -s windows Pacenote && zip -qr -X "$ZIP" Pacenote && rm Pacenote )
( cd "$(dirname "$ZIP")" && shasum -a 256 "$(basename "$ZIP")" >"$(basename "$ZIP").sha256" )

step "Done"
du -sh "$OUT" "$ZIP"
echo "Steam depot content root: $OUT"
