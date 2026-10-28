# Art: how a stage looks (`src/art/`)

Everything visual about a stage that isn't gameplay: materials, terrain shading, the road
surface, trees and dressing, the car model, sky and weather, wheel spray. One plugin
(`art::ArtPlugin`); the stage loader calls `art::spawn_stage` after the terrain, and
`drive::spawn_car` calls `art::car::spawn`.

| File | What |
|---|---|
| `mod.rs` | plugin, `pbr(set)` (cooked set → `StandardMaterial`), `spawn_stage`, dev commands `art.stats`, `art.place {s, lateral?}` |
| `ground.rs` | terrain splat layers per biome (textured), road ribbon (UVs along the road, darker compacted wheel lines, loose stones in the middle and at the edges), surface patches as blended overlays that fade in over 10 m, soft verge strips each side |
| `terrain.rs`, `terrain.wgsl` | full-PBR terrain material (colour + normal + ORM per layer, two-scale anti-tiling, AO-weighted layer blend) put on the engine's terrain chunks; same splat map and layer rules as the engine's |
| `trees.rs` | trees built from foliage cards: pine, spruce, snowy spruce, birch, olive-like scrub tree (3 variants each), placed on the dressing graph's `trunks` points (the ones the car collides with) |
| `dressing.rs` | PCG catalog: decimated Poly Haven rocks, shrubs, ferns, grass, stump; item mixes per biome; ground cover fades out at ~95 m instead of the budget's flat far version |
| `car.rs` | Car Concept model on the simulated car: hubs follow suspension and steer, wheels spin; bonnet camera position |
| `look.rs` | `Look` per stage: sun angle/colour/lux per time of day, atmosphere or overcast gradient sky, fog distance per weather (also where trees and scenery stop), exposure, grading |
| `spray.rs` | dust, gravel chips, mud clods, snow powder, tarmac smoke per wheel from surface, speed and slip; rain around the camera on wet stages |
| `meshfile.rs` | reader for `assets/models/*.pnm` (small binary meshes, read synchronously at load) |

## Assets and how to rebuild them

Sources are the raw downloads in the main checkout's `assets_src/` (see `assets_src/SOURCES.md`).
Only cooked outputs are committed (`assets/textures/<set>/*.ktx2 + color.avg`,
`assets/models/*`); the intermediate JPG/PNG are gitignored.

```sh
tools/art/prep_textures.sh                       # ground/road/model sets → assets/textures/<set>/*.jpg (1K, normals 512)
blender -b --factory-startup -P tools/art/render_cards.py -- <assets_src> assets/textures   # foliage cards (+ snow variant)
memgate -- cargo run --manifest-path <engine>/tools/cook/Cargo.toml -- assets/textures       # → KTX2 (BC7/BC5), alpha kept
blender -b --factory-startup -P tools/art/build_meshes.py -- <assets_src> assets/models      # rocks/plants → .pnm
blender -b --factory-startup -P tools/art/build_car.py -- <CarConcept.glb> assets/models/car.glb
```

- **Foliage cards** are orthographic side renders of the Poly Haven saplings (unlit albedo,
  alpha from the separately downloaded `*_alpha` maps, camera-space normals softened halfway to
  the card normal, colour dilated into the transparent texels so mips don't halo). `*_dense`
  composites four rotated copies for a fuller silhouette; `_snow` whitens upward-facing needles.
- **Trees**: near = trunk (tapered cylinder, bark) + crown (three crossed core cards + drooping
  branch cards), ≤ 400 triangles, two entities sharing meshes per species/variant (batched), cast
  shadows; spawned for PCG cells within 220 m of the camera (3 cells per frame), removed beyond
  270 m, fade out at 150 m. Far = the core cards merged per 64 m PCG cell, GPU-only mesh, no
  shadows, from 104 m (to the cell centre) to the fog distance.
- **Car**: Car Concept (CC BY 4.0, credit in `assets/credits.txt`), rescaled to the sim's
  2.47 m wheelbase, nose to −Z, body joined per material, wheels re-rigged as `Hub*` (steer +
  suspension, caliper) → `Spin*` (tyre, rim, disc); the steering-wheel emblem deleted, the plate
  texture and every emissive "logo" texture removed, material variants dropped, deep blue
  livery. Bevy 0.19 issues found: the clearcoat lobe washes the paint out to white, and
  `KHR_texture_transform` on normal/emissive maps is ignored (the paint's flake normal tiles
  30× and sparkled), so both are off. glTF images get CPU-built mips at load, then drop
  their CPU copy.

## Measurements

Apple M5 Pro, dev build, offscreen 1920×1080, High quality, governor off, chase camera at the
start and at s = 700 / 1800 m, then 3 s of driving; `perf.stats` session values (the machine
was shared with other agents' builds: ±1 ms).

| stage | before (placeholders) p50 / GPU / footprint / entities | after p50 / GPU / footprint / entities | trees (near entities) |
|---|---|---|---|
| 1 Pinewater (pine forest, morning) | 7.1 ms / 4.3 ms / 1171 MB / 18.0 k | 8.4 ms / 8.4 ms / 2001 MB / 30.1 k | 152 k (5.4 k) |
| 9 Frostline (snow forest, overcast) | 4.8 ms / 4.5 ms / 1066 MB / 15.7 k | 8.0 ms / 7.7 ms / 1925 MB / 23.1 k | 127 k (4.5 k) |
| 5 Sunscar (scrub, noon) | — | 6.6 ms / 6.6 ms / 2716 MB / 31.8 k | 19 k |
| 7 High Cairn (moor, dusk overcast) | — | 6.4 ms / 5.7 ms / 2035 MB / 38.8 k | 3.4 k |
| 12 Saw Mill (pine, tarmac, dusk) | — | 6.8 ms / 6.9 ms / 2088 MB / 24.1 k | 127 k |

Breakdown on stage 1 (A/B, same binary): trees +2.7 ms GPU, +240 MB; PCG rocks/plants
+110 MB; PBR terrain material +100 MB, ≈ +0.5 ms GPU; car, sky/look, spray ≈ +400 MB. Far trees:
0.91 M triangles in 2 500 merged cells. Ground cover is limited to a 90 m band along the road
(it was everywhere: 76 k entities → 30 k).

## Known limits

- Trees are cards: from directly below or very close the planes show; no wind.
- The far tree tier is drawn per 64 m cell, so a tree can be drawn by both tiers for a few
  metres around 100–150 m (never by neither).
- Terrain layers are world-XZ projected (no triplanar): steep rock faces stretch.
- HDRIs in `assets_src/hdri` are unused: the engine's cubemap sky needs KTX2 cubemaps and there
  is no equirect → cubemap tool yet; the atmosphere / gradient skies are used instead.
- No tyre-track decals beyond the shaded wheel lines; no mud build-up on the car yet.
