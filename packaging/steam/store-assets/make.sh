#!/usr/bin/env bash
# DRAFT Steam store and library art from in-game screenshots (for a designer to replace).
#
#   packaging/steam/store-assets/make.sh
#
# In:  src/*.jpg (1920x1080 gameplay frames from the engine's play script, see docs/STEAM.md),
#      fonts/Archivo-*.ttf (OFL, static cuts of the website's Archivo; see fonts/README.txt).
# Out: full/ and demo/ (capsules, library art, icons), screenshots/, contact-sheet.jpg.
# Sizes are Valve's (partner.steamgames.com/doc/store/assets). Needs ImageMagick 7 (`magick`).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"
command -v magick >/dev/null || { echo "error: magick not found (brew install imagemagick)" >&2; exit 1; }

FONT=fonts/Archivo-WideBlack.ttf
LABEL_FONT=fonts/Archivo-SemiWide.ttf
INK='#f3f4f1'; ORANGE='#ff6b14'; NAVY='11,21,48'
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT

# Which frame goes where (see src/).
# Capsules get a HUD-free crop (the HUD sits in the bottom 105 px, call subtitles in the top 75).
clean() { magick "$1" -crop 1560x878+180+78 +repage "$2"; }
clean src/01-dust-devil-dusk-chase.jpg "$TMP/key.png";   KEY="$TMP/key.png"   # header, main, small, background
clean src/03-frostline-snow-chase.jpg "$TMP/tall.png";   TALL="$TMP/tall.png" # vertical and library capsules
clean src/06-saw-mill-dusk-chase.jpg "$TMP/hero.png";    HERO="$TMP/hero.png" # library hero (no text)

# --- Logo: PACENOTE in Archivo (wdth 125, wght 900) over the livery's orange slash ------------
magick -background none -fill "$INK" -font "$FONT" -pointsize 300 label:'PACENOTE' -trim +repage "$TMP/word.png"
W=$(magick identify -format %w "$TMP/word.png"); H=$(magick identify -format %h "$TMP/word.png")
magick -size $((W + 80))x$((H + 90)) xc:none "$TMP/word.png" -gravity north -composite \
  -fill "$ORANGE" -draw "polygon 30,$((H + 40)) $((W + 70)),$((H + 40)) $((W + 50)),$((H + 80)) 10,$((H + 80))" \
  -trim +repage "$TMP/logo.png"
# Demo: the same with DEMO on an orange tag under the right end.
magick -background none -fill '#0b1530' -font "$FONT" -pointsize 130 label:'DEMO' -trim +repage "$TMP/demo-word.png"
DW=$(magick identify -format %w "$TMP/demo-word.png"); DH=$(magick identify -format %h "$TMP/demo-word.png")
magick -size $((DW + 120))x$((DH + 60)) xc:none -fill "$ORANGE" \
  -draw "polygon 20,0 $((DW + 120)),0 $((DW + 100)),$((DH + 60)) 0,$((DH + 60))" \
  "$TMP/demo-word.png" -gravity center -composite "$TMP/demo-tag.png"
LW=$(magick identify -format %w "$TMP/logo.png"); LH=$(magick identify -format %h "$TMP/logo.png")
TW=$(magick identify -format %w "$TMP/demo-tag.png"); TH=$(magick identify -format %h "$TMP/demo-tag.png")
magick -size "${LW}x$((LH + 30 + TH))" xc:none "$TMP/logo.png" -gravity northwest -composite \
  "$TMP/demo-tag.png" -geometry +$((LW - TW))+$((LH + 30)) -composite "$TMP/logo-demo.png"

# A logo with a soft shadow, for laying over a photo.
shadowed() { # in width out
  magick "$1" -resize "$2"x \( +clone -background 'rgba(0,0,0,0.75)' -shadow 60x8+0+4 \) +swap \
    -background none -layers merge +repage "$3"
}

# cover SRC W H GRAVITY OUT: fill WxH from SRC, cropping around GRAVITY.
cover() { magick "$1" -resize "$2x$3^" -gravity "$4" -extent "$2x$3" "$5"; }

# shade IN OUT DIR STRENGTH: navy gradient for legibility (DIR: south = dark at the bottom).
shade() {
  local w h; w=$(magick identify -format %w "$1"); h=$(magick identify -format %h "$1")
  local g="rgba($NAVY,0)-rgba($NAVY,$4)"
  if [ "$3" = north ]; then g="rgba($NAVY,$4)-rgba($NAVY,0)"; fi
  magick "$1" \( -size "${w}x${h}" gradient:"$g" \) -compose over -composite "$2"
}

# place BG LOGO WIDTH GRAVITY OFFSET OUT
place() {
  shadowed "$2" "$3" "$TMP/l.png"
  magick "$1" "$TMP/l.png" -gravity "$4" -geometry "$5" -compose over -composite -quality 92 "$6"
}

make_set() { # edition logo
  local ed=$1 logo=$2 out="$HERE/$1"
  rm -rf "$out"; mkdir -p "$out"
  # Header capsule 920x430 (also the library header).
  cover "$KEY" 920 430 center "$TMP/b.png"; shade "$TMP/b.png" "$TMP/b.png" north 0.8
  place "$TMP/b.png" "$logo" 560 north +0+30 "$out/header_capsule_920x430.jpg"
  cp "$out/header_capsule_920x430.jpg" "$out/library_header_920x430.jpg"
  # Small capsule 462x174: the logo nearly fills it.
  cover "$KEY" 462 174 center "$TMP/b.png"
  magick "$TMP/b.png" -fill "rgba($NAVY,0.55)" -draw "rectangle 0,0 462,174" "$TMP/b.png"
  place "$TMP/b.png" "$logo" 420 center +0+0 "$out/small_capsule_462x174.jpg"
  # Main capsule 1232x706.
  cover "$KEY" 1232 706 center "$TMP/b.png"; shade "$TMP/b.png" "$TMP/b.png" south 0.8
  place "$TMP/b.png" "$logo" 700 southwest +56+48 "$out/main_capsule_1232x706.jpg"
  # Vertical capsule 748x896 and library capsule 600x900: car centred, logo at the top.
  cover "$TALL" 748 896 center "$TMP/b.png"; shade "$TMP/b.png" "$TMP/b.png" north 0.85
  place "$TMP/b.png" "$logo" 640 north +0+70 "$out/vertical_capsule_748x896.jpg"
  cover "$TALL" 600 900 center "$TMP/b.png"; shade "$TMP/b.png" "$TMP/b.png" north 0.85
  place "$TMP/b.png" "$logo" 520 north +0+80 "$out/library_capsule_600x900.jpg"
  # Library hero 3840x1240: no text. Upscaled from 1080p: recapture at 4K for the real one.
  cover "$HERO" 3840 1240 center "$TMP/b.png"
  magick "$TMP/b.png" -quality 90 "$out/library_hero_3840x1240.jpg"
  # Library logo: transparent PNG, 1280 wide.
  magick "$logo" -resize 1280x720 "$out/library_logo_1280w.png"
  # Page background 1438x810: ambient, blurred and dark.
  cover "$KEY" 1438 810 center "$TMP/b.png"
  magick "$TMP/b.png" -blur 0x10 -fill "rgba($NAVY,0.6)" -draw "rectangle 0,0 1438,810" -quality 90 "$out/page_background_1438x810.jpg"
  # Icons: app icon 184x184 JPG, shortcut icon 512x512 PNG (the Windows .ico and Mac .icns
  # come from ../../icon-1024.png).
  magick ../../icon-1024.png -background '#0b1530' -flatten -resize 184x184 -quality 92 "$out/app_icon_184x184.jpg"
  magick ../../icon-1024.png -resize 512x512 "$out/shortcut_icon_512x512.png"
  echo "$ed: $(ls "$out" | wc -l | tr -d ' ') files"
}

make_set full "$TMP/logo.png"
make_set demo "$TMP/logo-demo.png"

# Screenshots: the frames in src/ (numbered in upload order) as they are (gameplay, HUD included), 1920x1080.
rm -rf screenshots; mkdir -p screenshots
for f in src/*.jpg; do magick "$f" -quality 95 "screenshots/$(basename "$f")"; done

# Contact sheet: every file, labelled with its name and size.
magick montage -font "$LABEL_FONT" -pointsize 14 -background '#1a1f2e' -fill '#f3f4f1' \
  -label '%d/%f\n%wx%h' full/* demo/* screenshots/* -tile 6x -geometry 300x300+8+8 \
  -title 'Pacenote: DRAFT Steam assets (for a designer)' contact-sheet.jpg
echo "contact sheet: $HERE/contact-sheet.jpg"
