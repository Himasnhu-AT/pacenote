#!/usr/bin/env bash
# Builds the texture sets the game uses from the raw downloads in assets_src/ (see
# assets_src/SOURCES.md), at 1024² (tiling ground) or as listed, into
# assets/textures/<set>/{color,normal,rough,ao}.jpg, ready for the engine cook:
#   memgate -- cargo run --manifest-path <engine>/tools/cook/Cargo.toml -- assets/textures
# Sources (jpg) are not committed; the cooked .ktx2 + color.avg are.
# Usage: tools/art/prep_textures.sh [path to assets_src]  (default: the main checkout's)
set -euo pipefail
SRC=${1:-/Users/himanshu/codes/game/pacenote/assets_src}
OUT=$(cd "$(dirname "$0")/../.." && pwd)/assets/textures
G=$SRC/textures/ground

# set <name> <size> <color> <normal> <rough> [ao]
set_() {
  local name=$1 size=$2; shift 2
  mkdir -p "$OUT/$name"
  local maps=(color normal rough ao)
  local i=0
  for f in "$@"; do
    local m=${maps[$i]}
    if [ -n "$f" ]; then
      # Normals at half size: plenty at 4-9 m per repeat, a quarter of the bytes.
      local sz=$size
      [ "$m" = normal ] && sz=$((size / 2))
      magick "$f" -resize "${sz}x${sz}" -quality 95 "$OUT/$name/$m.jpg"
    fi
    i=$((i + 1))
  done
}
ph() { echo "$G/$1/$1_diff_2k.jpg" "$G/$1/$1_nor_gl_2k.jpg" "$G/$1/$1_rough_2k.jpg" "$G/$1/$1_ao_2k.jpg"; }
acg() { echo "$G/$1/$1_2K-JPG_Color.jpg" "$G/$1/$1_2K-JPG_NormalGL.jpg" "$G/$1/$1_2K-JPG_Roughness.jpg" "$G/$1/$1_2K-JPG_AmbientOcclusion.jpg"; }

# Terrain layers.
set_ meadow 1024 $(ph leafy_grass)
set_ moss 1024 $(acg Ground037)
set_ heath 1024 $(ph sparse_grass)
set_ needles 1024 $(acg Ground077)
set_ leaves 1024 "$G/forest_leaves_02/forest_leaves_02_diffuse_2k.jpg" "$G/forest_leaves_02/forest_leaves_02_nor_gl_2k.jpg" "$G/forest_leaves_02/forest_leaves_02_rough_2k.jpg" "$G/forest_leaves_02/forest_leaves_02_ao_2k.jpg"
set_ dirt 1024 $(ph gravel_road)
set_ rock 1024 $(ph rock_face_03)
set_ cliff 1024 $(ph cliff_side)
set_ snow 1024 $(ph snow_02)
# Road surfaces and the verge.
set_ gravel 1024 $(ph forest_ground_04)
set_ gravel_red 1024 $(ph rocky_gravel)
set_ shoulder 1024 $(ph grass_path_2)
set_ tarmac 1024 $(ph tarred_gravel)
set_ mud 1024 $(ph muddy_tracks)

# Model sets: <folder> <stem> <set name> <size> [alpha map] (ARM = AO, roughness, metal).
M=$SRC/models
model() {
  local dir=$M/$1/textures stem=$2 name=$3 size=$4 alpha=${5:-}
  mkdir -p "$OUT/$name"
  local tex=${6:-$stem}
  magick "$dir/${tex}_diff_2k.jpg" -resize "${size}x${size}" -quality 95 "$OUT/$name/color.jpg"
  magick "$dir/${tex}_nor_gl_2k.jpg" -resize "${size}x${size}" -quality 95 "$OUT/$name/normal.jpg"
  if [ -f "$dir/${tex}_arm_2k.jpg" ]; then
    magick "$dir/${tex}_arm_2k.jpg" -resize "${size}x${size}" -channel G -separate -quality 95 "$OUT/$name/rough.jpg"
    magick "$dir/${tex}_arm_2k.jpg" -resize "${size}x${size}" -channel R -separate -quality 95 "$OUT/$name/ao.jpg"
  else
    magick "$dir/${tex}_rough_2k.jpg" -resize "${size}x${size}" -quality 95 "$OUT/$name/rough.jpg"
  fi
  if [ -n "$alpha" ]; then
    magick "$dir/$alpha" -resize "${size}x${size}" -colorspace Gray -quality 95 "$OUT/$name/alpha.jpg"
  fi
}
model rocks/boulder_01 boulder_01 m_boulder 1024
model rocks/rock_moss_set_01 rock_moss_set_01 m_rock_moss1 1024
model rocks/rock_moss_set_02 rock_moss_set_02 m_rock_moss2 1024
model rocks/namaqualand_boulder_05 namaqualand_boulder_05 m_rock_dry 1024
model vegetation/shrub_02 shrub_02 m_shrub 1024 shrub_02_alpha_2k.jpg
model vegetation/fern_02 fern_02 m_fern 512 fern_02_alpha_2k.jpg
model vegetation/grass_medium_01 grass_medium_01 m_grass1 1024 grass_medium_01_alpha_2k.jpg
model vegetation/grass_medium_02 grass_medium_02 m_grass2 512 grass_medium_02_alpha_2k.jpg
model vegetation/tree_stump_01 tree_stump_01 m_stump 512
# Conifer trunk bark: Poly Haven's `pine_bark` scan (2 m per repeat; ARM = AO, rough, metal;
# colour at 1024, the rest at 512).
B=$SRC/textures/bark/pine_bark
mkdir -p "$OUT/bark"
magick "$B/pine_bark_diff_2k.jpg" -resize 1024x1024 -quality 95 "$OUT/bark/color.jpg"
magick "$B/pine_bark_nor_gl_2k.jpg" -resize 512x512 -quality 95 "$OUT/bark/normal.jpg"
magick "$B/pine_bark_arm_2k.jpg" -resize 512x512 -channel G -separate -quality 95 "$OUT/bark/rough.jpg"
magick "$B/pine_bark_arm_2k.jpg" -resize 512x512 -channel R -separate -quality 95 "$OUT/bark/ao.jpg"
echo "prepared: $(ls "$OUT" | tr '\n' ' ')"
