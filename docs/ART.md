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
| `car.rs` | the rally car model on the simulated car: hubs follow suspension and steer, wheels spin; steering wheel, tacho and speedo follow the sim; lamp pod hidden from inside; cockpit-eye and bonnet camera positions |
| `look.rs` | `Look` per stage: sun angle/colour/lux per time of day, atmosphere or overcast gradient sky, fog distance per weather (also where trees and scenery stop), exposure, grading |
| `spray.rs` | dust, gravel chips, mud clods, snow powder, tarmac smoke per wheel from surface, speed and slip; rain around the camera on wet stages |
| `meshfile.rs` | reader for `assets/models/*.pnm` (small binary meshes, read synchronously at load) |

## Assets and how to rebuild them

Sources are the raw downloads in the main checkout's `assets_src/` (see `assets_src/SOURCES.md`).
Only cooked outputs are committed (`assets/textures/<set>/*.ktx2 + color.avg`,
`assets/models/*`); the intermediate JPG/PNG are gitignored.

```sh
tools/art/prep_textures.sh                       # ground/road/model sets → assets/textures/<set>/*.jpg (1K, normals 512)
memgate -- blender -b --factory-startup -P tools/art/render_tree_cards.py -- <assets_src> assets/textures [set ...]   # tree card atlases (+ fir snow)
memgate -- blender -b --factory-startup -P tools/art/build_bushes.py -- <assets_src> assets   # bushes: m_shrub atlas + shrub_a/b.pnm
memgate -- cargo run --manifest-path <engine>/tools/cook/Cargo.toml -- assets/textures       # → KTX2 (BC7/BC5), alpha kept
blender -b --factory-startup -P tools/art/build_meshes.py -- <assets_src> assets/models      # rocks/ferns/grass/stump → .pnm
memgate -- blender -b --factory-startup -P tools/art/build_rallycar.py -- assets/models/rallycar.glb [--preview DIR] [--no-bake]   # car (no sources needed)
```

- **Tree cards** (`render_tree_cards.py`) are orthographic renders of Poly Haven's full-size
  scans: `fir_tree_01` (spruce, `card_fir` + `card_fir_snow`), `pine_tree_01` (`card_pine`),
  `tree_small_02` (birch crown, trunk left out, `card_birch`) and `island_tree_01` (scrub tree,
  `card_olive`). Each atlas holds the whole tree from the side plus three small pictures: real
  branch slices seen from above (conifers) or leaf clumps seen from the side (broadleaf). Unlit
  albedo with a little baked ambient occlusion and a per-tree colour grade for the needles
  (the scans' pine needles are olive-brown), alpha from the separately downloaded `*_alpha` maps,
  camera-space normals softened towards the card normal, colour dilated into the transparent
  texels so mips don't halo; `_snow` whitens upward-facing needles. `frame.txt` lists the regions;
  `trees.rs` has them as constants and a test keeps the two in sync.
- **Bushes** (`build_bushes.py`): Poly Haven has no dense bush, so `shrub_a`/`shrub_b` are shells
  of leaf-clump cards cut from `tree_small_02` (`m_shrub`), 66 / 50 triangles, base at the origin.
- **Trees** (LOD chain, distances at High, scaled by the preset's `detail_scale`: Low 48 / 180 m):
  LOD0 to 80 m = trunk (tapered cylinder, `pine_bark`; white in code for birch) + crown (three
  crossed core cards from the crown up + drooping branch cards or leaf clumps), <= 400
  triangles, two entities per tree sharing meshes per species/variant (instanced), shadows.
  LOD1 to 300 m = the whole-tree picture on the three core cards + one whorl/clump in three
  (birch adds a 4-sided trunk), one entity per tree, shadows. Both crossfade with per-tree
  dithered `VisibilityRange`s and are spawned per PCG cell near the camera (3 cells a frame).
  Beyond: impostors = one camera-facing quad per tree (cylindrical billboard,
  `impostor.wesl`), merged per 256 m chunk and species, no shadows or prepass, fading in per
  tree with the same dither as Bevy's ranges; built only for chunks within the fog distance
  (at most 1.2 km) and dropped beyond. On Low every other tree has an impostor.
- **Car**: "Talus", an original 5-door gravel-rally hatch (design sheet `docs/cars/rally_hatch.md`)
  built entirely in `build_rallycar.py` (no downloaded parts): the body is a loft of parametric
  cross-sections (bonnet → windscreen → roof → steep tailgate, from per-station height/width
  curves), closed at the ends by horizontal strips so the bumpers split off below the flank
  crease; squared superellipse wheel arches cut with booleans, then flared box-section arch
  lips, arch liners, mud flaps, skid plate, roof scoop, roof-edge wing, mirrors, square lamps,
  grille, tow loops, crash structure behind the bumpers (seen when one comes off) and a 4-lamp
  bonnet pod (node `LightPod`). Livery (slate teal, chalk band and roof, ochre pinstripes, black
  sills and bumpers, number 27 on the doors and roof; no marks) is a generated 2048×1024 atlas
  (JPEG): sides map by (y, height) with the number drawn un-mirrored on each side, top by
  (y, x), ends by (x, height); `--paint RRGGBB` and `--number NN` change it. Paint has a light
  clear coat. Wheels: knobbly gravel tyres (R 0.325 m), anthracite six-spoke rims, vented
  discs, ochre callipers; rig `Hub*` (suspension) → `Steer*` (front: steering, calliper) →
  `Spin*` → `Wheel*`. Detachable `MirrorL/R`, `RearWing`, `FrontSplitter`, `Front/RearBumper`,
  `RearDiffuser` (origin at their middle; `props.rs` throws a copy off as debris). Levels of
  detail: the body and wheels are built three times (`Body_lod1/2`, `Wheel*_lod1/2`, coarser
  stations and arches, opaque dark glass), switched by `VisibilityRange` in `car.rs`
  (28–34 m, 85–95 m); parts and the pod draw at every distance. ~17.5 k exterior + ~28 k
  cockpit triangles at LOD0, 5.0 k at LOD1, 2.3 k at LOD2; 2.8 MB. Every material is
  single-sided unless noted: the body shell is invisible from inside, so the cockpit shows the
  interior's own skin (the shell's opaque cabin faces copied 3.5 cm in and flipped), dash,
  binnacle with live gauges (`Tacho`, `Speedo`), steering wheel (`SteeringWheel`, turns about its
  column), seats with harnesses (the empty passenger seat's is `SeatHarness`), shifter,
  hydraulic handbrake and a roll cage, AO baked into vertex colours; the seated driver
  (`driver.py`, `DriverBody`) shows from outside; the glass (alpha blended) shows only from
  outside. `--preview DIR` renders Workbench previews (exterior views, the driver's eye, LOD1/2).
- **Cameras** (`drive.rs` `CameraView`, `C` cycles): cockpit (default; driver's eye at the left
  seat, view direction lagged ~55 ms behind the body to filter suspension shake, pitched 3°
  down) → bonnet → chase. The lamp pod is hidden in cockpit and bonnet views.
  glTF images get CPU-built mips at load, then drop their CPU copy.

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
- Tree impostors are single-view (the side picture of the cards, turned to the camera): from
  high above they look like standing cards. An octahedral impostor (8x8 baked views) would fix it.
- Terrain layers are world-XZ projected (no triplanar): steep rock faces stretch.
- HDRIs in `assets_src/hdri` are unused: the engine's cubemap sky needs KTX2 cubemaps and there
  is no equirect → cubemap tool yet; the atmosphere / gradient skies are used instead.
- No tyre-track decals beyond the shaded wheel lines; no mud build-up on the car yet.
