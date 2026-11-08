# Realism asset import plan

7 Nov 2026. Goal: the look of the owner's three reference clips. Scrub = dry ochre/red gum-tree stage (clip 1), PineForest = fast forest gravel (clip 2), Moor = golden grassland (clip 3).
Inputs: `assets_src/fab_batch1/` (39 Fab downloads), `assets_src/realism_batch1/` (12 Poly Haven CC0), and the in-house builds in `asset-forge/assets_out/` (Eucalyptus, RallyTape, HayBale).
Research: `ASSET_RESEARCH_REALISM.md`. Pipeline facts: `docs/ASSET_PIPELINE.md`, `docs/ART.md`.

**Status (8 Nov 2026, branch `realism-assets`, owner approved):** Phases 1–5a are in. Phase 5b and 6 are not started.
- **1 Ground:**
  - `gravel` ← Gravel on Soil (not Gravel Road: flat grey), `gravel_red` ← Soil and Gravel, `mud` ← Mud Tire Tracks, `rock` ← Layered Rock.
  - New sets: `soil_dry`, `soil_red`, `soil_crack`, `dirt_road` (the Scrub verge).
  - Kept, because they looked better than the scans: `cliff` (orange layered rock suits Scrub) and `shoulder`. Grassy Meadow 04 is lush green, not golden, so it wasn't used.
  - Scrub and Moor layers re-tinted from screenshots.
- **2 Cover and rocks:** `boulder_rauk`, `dead_shrub_a/b`, `dry_branches`, `dead_branches`, `dry_grass_a/b`, `cocksfoot`, `graminea`, `kikuyu` (`dressing.rs`).
  - The scans load only on the biomes that use them.
  - Moor ground cover was raised to 0.85 keep.
  - The sandstone cliff, industrial and jersey barriers and the fence are cooked but parked in `assets_src/cooked_unused/`.
- **3 Trackside (rally stages):** `world::trackside` places three things:
  - spectator spots at the tightest corners and the start (tape + crowd);
  - hay bales on corner apexes (`PropKind::HayBale`, knocked away above ~20 km/h);
  - concrete barriers outside grade 0–2 corners on Scrub and tarmac stages (solid).
  - Drawn by `art::trackside`. `stage_hash` carries a `DRESSING_REV`, so old ghosts and records show as stale.
- **4 Gum trees:** `Species::Gum` (`card_gum` + `bark_gum`) is the main Scrub tree, with the olive.
- **5a Crowd:** `card_crowd`: 10 seated scans × 8 angles, camera-facing cards. **Licence to confirm:** see `fab_people_x10_01` in SOURCES.md.
- **Perf (Low, M5 Pro):** load 6.6 s, frame and GPU within budget, draws ≤ 609. Memory peaks at 1664 MB against 1608 MB on `main`; both are over the 1500 MB budget, which was already failing before this work.

The original plan follows.

## 0. Verification results (done)

All 39 Fab zips were unpacked into `assets_src/fab_batch1/<zip name>/` (3.2 GB, gitignored). Each one was opened in headless Blender, with triangle counts, sizes and texture links checked and a thumbnail rendered. Licences were checked against Fab's listing API.

- **Licences:** every item is free on the Professional tier. 34 are **Fab Standard**. 3 are **CC BY** and need credit lines: the *59 Crowd Idle animations* (`source/`), *Guideposts* and the *Decayed Rotten Eucalyptus Trunk*.
- **NoAI** tags on Grassy Meadow Ground 04, Graminea 01 and City People mean those files must not go into AI training or texture tools. Using them in the game is fine.
- **Normal maps:** all are **OpenGL**, measured against the height maps (correlation +0.26 to +0.95). No green flip is needed. The glTF-tier `_ORM.png` files already match the cook's layout (R = AO, G = roughness).

| Asset (folder) | State | Fix needed before use |
|---|---|---|
| 13 ground surfaces (gravel road, gravel-dirt road, dirt road, excavated road, grassy dirt road, gravel on soil, soil & gravel, dry soil, arid red soil, cracked soil, mud/sand tyre tracks, grassy meadow 04) | OK, 4K | downscale to 1024 at prep |
| layered_rock 8K, rock_cliff 8K, dry_desert_grass | OK (texture only) | downscale |
| road_dust, road_gravel (decals, colour+alpha) | OK | needs a decal path (see Phase 6) |
| hay_bale | **texture only, no mesh** | mesh from image-to-3D or in-house (Phase 3) |
| gum_tree | bark/leaf atlas, not a tree | used as bark on the in-house eucalyptus |
| concrete_barrier (16k tris, 5.4 m), industrial barrier L04 (23k, 7.3 m) | OK, clean | FBX→glTF, decimate to ~2k |
| jersey_road_barrier no1 (1.0M tris) / no2 (68k) | include a cobblestone floor patch | cut the floor, decimate; no1 is optional |
| coast_rauk_boulder (20k), large_sandstone_cliff (26k) | OK | FBX→glTF, decimate |
| dead_shrubs, dead_branches, dry_branches, cocksfoot, kikuyu (170k), dry_grass (9k) | OK (LOD0 + billboards) | decimate kikuyu; use the shipped LODs |
| graminea01 | ASCII FBX (Blender can't read it) | use the bundled OBJ |
| modular_fence (19k, 3 m) | OK | FBX→glTF |
| guideposts (558k, ×100 scale) | carved text on boards; scale wrong | fix scale, decimate, check the text is generic |
| decayed eucalyptus trunk (50k, ×100 scale) | scan includes a ground disc | fix scale, trim |
| Models of people ×10 (`blend/`, ~86k tris each) | textures linked to `maps/` but stored in `textures/` | relink, then bake to impostor cards |
| City People Free (`people_freepack`, 1.6k each) | stylised low-poly | far crowd only, or skip |
| 59 Crowd Idle anims (`source/`) | on UE mannequin; textures missing (fine) | retarget later (Phase 5b) |

## 1. Ground and roads (no engine change)

Replace the sources of the existing texture sets and add new ones. The 4-layers-per-biome limit stays.

| Set | Today | New source | Used by |
|---|---|---|---|
| `gravel` (road) | PH forest_ground_04 | **Fab Gravel Road** | PineForest / Birch / Moor roads |
| `gravel_red` (road) | PH rocky_gravel | **Fab Dirt Road** (ochre packed dirt) | Scrub roads |
| `shoulder` (verge) | PH grass_path_2 | **Fab Grassy Dirt Road** | non-Scrub verges |
| `mud` | PH muddy_tracks | **Fab Mud Tire Tracks** | mud surface |
| new `soil_dry` | — | **Fab Dry Soil Surface** | Scrub base layer (replaces `heath` there) |
| new `soil_red` | — | **Fab Arid Soil Ground** | Scrub layer 2 + Scrub verge (replaces `dirt`) |
| new `soil_gravel` | — | **Fab Soil and Gravel** | Scrub layer 3 (replaces `gravel_red`) |
| `cliff` | PH cliff_side | **Fab Rock Cliff** (8K→1K) | Scrub rock |
| `rock` | PH rock_face_03 | **Fab Layered Rock** (8K→1K) | other biomes' rock |
| new `meadow_dry` | — | **Fab Grassy Meadow Ground 04** | Moor base layer |

Changes:
- `tools/art/prep_textures.sh`: add a `mq()` helper for Megascans names (`*_BaseColor`, `*_Normal`, `*_Roughness`, `*_AO`; glTF tier `T_*_B/_N/_ORM`). Point the sets above at `assets_src/fab_batch1/...`. Size 1024 as today.
- `src/art/ground.rs`: update the `layers()` Scrub and Moor arms and `surface_sets()` (Scrub → `("gravel_red", "soil_red")`). Re-tune tints so `color` × texture lands on the art-bible values.
- `assets/manifest.toml` `[[asset]]` entries, plus `assets_src/SOURCES.md` keys with **Licence: Fab Standard** (CC BY where noted).
- Docs: `docs/ASSETS_BRIEF.md` §0 and the `ATTRIBUTION.md` header (via `tools/assets/manifest.py`) still say CC0/CC BY only. Add the owner's 7 Nov ruling: Fab Standard, any engine, cooked only, no raw redistribution.

Not in this phase:
- The tyre-track and road-dust textures as **decals** (no decal path; Phase 6).
- The existing wheel-line vertex shading stays.

## 2. Ground cover and rocks (no engine change)

- **Converter (new):** `tools/art/fab_to_gltf.py` (Blender, headless). For FBX, OBJ or glTF in, it:
  - applies the scale,
  - drops cobble/ground patches by name or bounds,
  - attaches the Megascans textures,
  - writes `assets_src/models/<cat>/<stem>_2k.gltf`, the layout `build_meshes.py` already reads.
- **`tools/art/build_meshes.py` `ITEMS`:** add boulder_rauk (→1.5k tris), sandstone_cliff (→3k), dead_shrub ×2 variants (→400), dry_branches/dead_branches (→600), dry_grass and cocksfoot and graminea clumps (→800 each), and kikuyu (→1.5k).
- **`src/art/dressing.rs`:** `MESHES`, `catalog()`, and the biome weights in `rocks()`/`bushes()`:
  - Scrub gets dead shrubs, dry grass, branches and sandstone.
  - Moor gets dry grass, cocksfoot and graminea.
  - PineForest gets branches and boulders.
- **`src/world.rs` collision:** only rocks are solid, as today.
- Re-bless the obstacle hashes: `memgate -- cargo test print_obstacle_hashes -- --ignored`. Ghosts and records reset.

## 3. Roadside furniture (game code, no engine change)

- **Concrete barrier:**
  - Megascans *Concrete Barrier* is the hero piece (decimate to ~2k, 1024 textures); *Industrial L04* is the second variant.
  - New `BlockKind::Concrete` in `src/open/furniture.rs`, solid in `walls()`, bump `REV`.
  - Its own material/draw path beside `src/art/signs.rs` (the atlas can't hold a scan). Merged per 512 m tile: about +2 draws per tile.
  - Placed on Scrub hairpins and stage edges (art bible §4).
- **Spectator tape:** in-house `asset-forge/assets_out/RallyTape` (372 tris per 12 m run). Run along stage verges at spectator spots and merge per tile.
- **Hay bales:** mesh from the image-to-3D loop (`asset-forge/assets_out/HayBale`, from a CC0 photo) or an in-house cylinder with the Fab hay texture, whichever passes review. Placed at corner apexes on Scrub and Moor stages. Solid.
- **Fence and guideposts:** Fab Modular Fence (Moor field edges); guideposts at junctions (CC BY credit).

## 4. Eucalyptus trees (no engine change)

1. Export the procedural tree (`asset-forge/assets_out/Eucalyptus/build_eucalyptus.py`, seeds 7 / 11 / 23) to `assets_src/models/vegetation/eucalyptus_01/eucalyptus_01_2k.gltf`, with leaf alpha.
2. Add a `card_gum` row to `TREES` in `tools/art/render_tree_cards.py` (height 2048, `clusters`, `no_trunk`). The game draws the trunk with Fab *Gum Tree* bark as a new `bark_gum` set.
3. `src/art/trees.rs`: add `Species::Gum`, an `Atlas` const matching `frame.txt`, the material and bark mapping, the build/impostor loops, and add it to `mix(Biome::Scrub)`.

## 5. Spectators

- **5a (no engine change):** impostor cards.
  - Relink the *Models of people ×10* textures and render each person from 8 angles into a 2048² atlas (Blender, like the tree cards).
  - Draw crowds as instanced camera-facing quads behind the tape at spectator spots, with a per-instance tint for clothing variety.
  - Budget: 1 draw per tile.
- **5b (later):** a few animated people near the road. Retarget the CC BY crowd-idle animations onto decimated (~8k) people. Needs skinned-mesh glb support in the game, which exists for the car and arms. Measure the cost first.

## 6. Needs engine work (separate approval, engine repo)

- **HDRI skies / image-based lighting:** the Poly Haven HDRIs can't be used until the engine can cook an equirect→cubemap HDR (BC6H) and use it for environment lighting under the atmosphere sky. Until then they're look-dev references only.
- **Decals:** tyre tracks, road dust and gravel spray on the road.
- **More than 4 terrain layers, or several biomes in one open-world splat map:** the open world is all PineForest today.

## Commands

Run by me (Claude) for each phase. You don't need to run any of these yourself; they're listed so you can repeat them:
```bash
cd /Users/himanshu/codes/game/pacenote
bash tools/art/prep_textures.sh                                   # resize/pack sources -> assets/textures/<set>/*.png
memgate -- cargo run --manifest-path ../engine/tools/cook/Cargo.toml -- assets/textures --only gravel   # KTX2 (repeat per set, or omit --only)
memgate -- /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup -P tools/art/build_meshes.py -- assets_src assets/models
memgate -- /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup -P tools/art/render_tree_cards.py -- assets_src assets/textures card_gum
python3 -I tools/assets/manifest.py check && python3 -I tools/assets/manifest.py attribution
memgate -- cargo +stable test -q                                  # incl. shipped_meshes_load, atlas frames, obstacle hashes
python3 -I tools/perf/budget.py --quality Low                     # perf gate per phase (draws <= 800, memory <= 1.5 GB)
python3 -I ../engine/scripts/play.py shots.json --manifest Cargo.toml --bin pacenote --out shots/<phase> -- --stage 5   # before/after screenshots
```

What **you** do:
1. Approve the phases (one at a time or all).
2. Tell me when the GPU is free (renders, the image-to-3D runs and the perf gate all use it).
3. Pick from the before/after screenshots at the end of each phase.

Nothing else needs downloading for Phases 1–5a.

## Risks

- **Memory and draws are already near budget** (about 1.5 GB, 606–666 draws on Low). New 1024 sets replace old ones at equal size. Meshes and the tape/barrier/crowd paths add about +6 draws per tile. Each phase is gated on `budget.py`.
- **Ghosts and records reset** when the dressing or furniture changes (obstacle hashes, `REV`).
- **The Fab Standard licence forbids shipping raw files.** Only cooked KTX2/.pnm files go in `assets/`, which is what the pipeline already does.
