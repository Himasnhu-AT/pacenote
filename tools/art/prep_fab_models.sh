#!/usr/bin/env bash
# Builds the texture sets for the Fab (Megascans / photoscan) model meshes from the raw
# downloads in assets_src/fab_batch1/ (see its SOURCES.md) into
# assets/textures/m_<name>/{color,normal,rough,ao,alpha}.jpg, ready for the engine cook:
#   memgate -- cargo run --release --manifest-path <engine>/tools/cook/Cargo.toml -- assets/textures --only m_<name>
# The meshes come from tools/art/fab_to_gltf.py then tools/art/build_meshes.py.
# Normal maps are OpenGL convention (measured): no green flip. Fab licence: these jpgs are
# gitignored; only the cooked .ktx2 + color.avg ship.
# Usage: tools/art/prep_fab_models.sh [path to assets_src/fab_batch1]
set -euo pipefail
F=${1:-/Users/himanshu/codes/game/pacenote/assets_src/fab_batch1}
OUT=$(cd "$(dirname "$0")/../.." && pwd)/assets/textures

rs() { magick "$1" -resize "${2}x${2}" "${@:4}" -quality 95 "$3"; }

# Fills the transparent texels of an alpha set's colour/rough/ao maps with the nearby
# opaque values (alpha-weighted averages at 128², 32² and the whole image), so the cook's
# plain mip averaging doesn't pull black backgrounds into leaf edges (dark, shiny fringes).
# Texels with any alpha keep their own value.
dilate() {
  local set=$OUT/$1 m
  for m in color rough ao; do
    [ -f "$set/$m.jpg" ] || continue
    local sz; sz=$(magick identify -format %w "$set/$m.jpg")
    magick "$set/$m.jpg" -colorspace sRGB \( "$set/alpha.jpg" -resize "${sz}x${sz}!" -colorspace Gray \) \
      -alpha off -compose CopyOpacity -composite "$set/_rgba.png"
    magick "$set/_rgba.png" \
      \( -clone 0 -resize 1x1! -alpha off -scale "${sz}x${sz}!" \) \
      \( -clone 0 -resize 32x32! -channel A -threshold 0 +channel -filter point -resize "${sz}x${sz}!" \) \
      \( -clone 0 -resize 128x128! -channel A -threshold 0 +channel -filter point -resize "${sz}x${sz}!" \) \
      \( -clone 0 \) -delete 0 -background none -compose Over -flatten -alpha off \
      $([ "$m" = color ] || echo -colorspace Gray) -quality 95 "$set/$m.jpg"
    rm "$set/_rgba.png"
  done
}

# Megascans FBX tier: <dir>/<prefix>_{BaseColor,Normal,Roughness,AO}.jpg (+ _Opacity.jpg).
# Grey maps: some foliage ships them red-channel-only (G = B = 0), so always take R.
# mq <set> <dir> <prefix> <colour size> <normal size> [alpha]
mq() {
  local name=$1 p=$F/$2/$3 cs=$4 ns=$5 alpha=${6:-}
  mkdir -p "$OUT/$name"
  rs "${p}_BaseColor.jpg" "$cs" "$OUT/$name/color.jpg"
  rs "${p}_Normal.jpg" "$ns" "$OUT/$name/normal.jpg"
  rs "${p}_Roughness.jpg" "$ns" "$OUT/$name/rough.jpg" -channel R -separate
  # Some foliage (Dead Shrubs) ships no AO map; the cook then uses AO = 1.
  if [ -f "${p}_AO.jpg" ]; then
    rs "${p}_AO.jpg" "$ns" "$OUT/$name/ao.jpg" -channel R -separate
  fi
  if [ -n "$alpha" ]; then
    rs "${p}_Opacity.jpg" "$cs" "$OUT/$name/alpha.jpg" -channel R -separate
  fi
}

# Megascans glTF tier: <dir>/Textures/T_<id>_4K_{B,B-O,N,ORT}.png. ORT = AO, roughness,
# translucency; the opacity is the alpha of B-O.
# mt <set> <dir> <id> <colour size> <normal size>
mt() {
  local name=$1 p=$F/$2/Textures/T_$3_4K cs=$4 ns=$5
  mkdir -p "$OUT/$name"
  rs "${p}_B.png" "$cs" "$OUT/$name/color.jpg"
  rs "${p}_B-O.png" "$cs" "$OUT/$name/alpha.jpg" -alpha extract
  rs "${p}_N.png" "$ns" "$OUT/$name/normal.jpg"
  rs "${p}_ORT.png" "$ns" "$OUT/$name/rough.jpg" -channel G -separate
  rs "${p}_ORT.png" "$ns" "$OUT/$name/ao.jpg" -channel R -separate
}

# Rocks and cliffs.
mq m_rock_rauk coast_rauk_boulder_l_03_wfepegb_high Coast_Rauk_Boulder_L_03_wfepegb_High_4K 1024 1024
mq m_sandstone large_sandstone_cliff_vminadn_high Large_Sandstone_Cliff_vminadn_High_4K 1024 1024
# Ground cover (alpha cards except the branches, which are solid geometry).
mq m_dead_shrub dead_shrubs_vdknafgha_high Dead_Shrubs_vdknafgha_High_4K 1024 512 alpha
mq m_dry_branches dry_branches_pdye3_high Dry_Branches_pdye3_High_4K 512 512
mq m_dead_branches dead_branches_qjuwd_high Dead_Branches_qjuwd_High_4K 512 512
mq m_cocksfoot cocksfoot_grass_tcfoejcr_high Cocksfoot_Grass_tcfoejcr_High_4K 1024 512 alpha
mt m_dry_grass dry_grass_tbbqejqr_ue_high tbbqejqr 1024 512
mt m_kikuyu kikuyu_grass_wiljearqx_ue_high wiljearqx 1024 512
# Graminea 01 (Dekogon-style names): C colour, N normal, ORM = AO, rough, metal; O opacity.
G=$F/graminea01_additional_files/TX_Graminea01_2K
mkdir -p "$OUT/m_graminea"
rs "${G}_C.jpg" 1024 "$OUT/m_graminea/color.jpg"
rs "${G}_O.jpg" 1024 "$OUT/m_graminea/alpha.jpg" -channel R -separate
rs "${G}_N.jpg" 512 "$OUT/m_graminea/normal.jpg"
rs "${G}_ORM.jpg" 512 "$OUT/m_graminea/rough.jpg" -channel G -separate
rs "${G}_ORM.jpg" 512 "$OUT/m_graminea/ao.jpg" -channel R -separate
# Roadside furniture.
mq m_barrier_concrete concrete_barrier_ubitfhtfa_high Concrete_Barrier_ubitfhtfa_High_4K 1024 1024
mq m_barrier_industrial industrial_construction_barrier_concrete_l_04_xiwcfczsc_high Industrial_Construction_Barrier_Concrete_L_04_xiwcfczsc_High_4K 1024 1024
mq m_fence modular_fence_ufflaitdw_high Modular_Fence_ufflaitdw_High_4K 1024 512
# Jersey barrier no2 (Sketchfab-style glTF, 8K): metallicRoughness G = roughness (its B
# "metal" channel is wrong for concrete and unused); no AO map.
J=$F/jersey_road_barrier_no2_free_photoscan_gltf/textures/Material_0
mkdir -p "$OUT/m_jersey"
rs "${J}_baseColor.jpeg" 1024 "$OUT/m_jersey/color.jpg"
rs "${J}_normal.jpeg" 1024 "$OUT/m_jersey/normal.jpg"
rs "${J}_metallicRoughness.jpeg" 1024 "$OUT/m_jersey/rough.jpg" -channel G -separate
for s in m_dead_shrub m_cocksfoot m_dry_grass m_kikuyu m_graminea; do dilate $s; done
echo "prepared: m_rock_rauk m_sandstone m_dead_shrub m_dry_branches m_dead_branches m_cocksfoot m_dry_grass m_kikuyu m_graminea m_barrier_concrete m_barrier_industrial m_fence m_jersey"
