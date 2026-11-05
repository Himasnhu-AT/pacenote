# Festival perf report: draws, memory, load at Low (open world)

Stream D (performance analyst), 5 Nov 2026, on `main` f64755c + this branch. M5 Pro, offscreen
1080p, dev build, quality Low unless noted. **The machine was shared with other streams' builds
(load average 6-18 during the runs): GPU and draw counts are reliable, CPU frame times and load
times are inflated, maybe ×1.5-2.5.** Re-measure on an idle machine before signing off.

How to reproduce:
- `python3 -I tools/perf/budget.py --quality Low` (scenarios tour, town, mountain, pass, event).
- `perf.draws` (new, dev builds, `src/camera.rs`): the main camera's draws grouped the way
  `render.stats` counts them (one per visible (mesh, material) bin; non-`StandardMaterial`
  materials one per entity), labelled by the nearest named entity, plus the sun's shadow
  cascades. Long reply: use a play.py copy without its 2000-character reply cut.
- Loading phases: the game log (`loading: X → Y at N s`), with `RUST_LOG=info,pacenote=debug`
  for the warm phase's once-a-second "waiting for" line.

## 1. Draw calls (budget 800 at Low)

Main view, Low, chase camera, same poses as `budget.py` (town by the square looking up the
street, the high pass, the mountain hairpin, event 103 as a Pro race 5 s after the start):

| What (owner) | town | pass | mountain | event |
|---|---|---|---|---|
| **Terrain chunks** (`PnTerrainExt`, 128 m chunks, drawn to the horizon) — A / engine | **426** | **815** | **360** | **552** |
| Roads: surface, verge, paint, patches, junctions, kerbs — A/B | 122 | 176 | 60 | 130 |
| Player car + driver (108 glTF primitives + hands) — D | 118 | 118 | 117 | 119 |
| Town tiles (merged per tile per material) — A | 28 | 51 | 5 | 45 |
| Traffic cars — A/B (`art/traffic_car.rs`) | 15 | 0 | 39 | 0 |
| Far trees (impostors) + far detail — A | 13 | 11 | 26 | 22 |
| Unnamed (cover, props) | 16 | 7 | 33 | 30 |
| PCG cells, river, furniture, rivals | 1 | 20 | 3 | 15 |
| **Total** (`render.stats draws`) | **739** | **1195** | **641** | **913** |
| Sun shadow cascade 0 (Low has one; not in `draws`, but real work) | 255 | 176 | 220 | 225 |

UI/HUD is not in these numbers (bevy_ui draws separately and is cheap). The `budget.py`
medians (town 930, pass 1227, event 1002) differ from this table only by the exact pose and
moment; the shares are the same.

**Why terrain dominates:** the open world uses the stage terrain settings: `chunk_quads: 64` at
2 m spacing (`src/world.rs`), so 128 m chunks over 4 km × 4 km = 1 024 chunks, and
`TerrainSettings.view_distance` is infinite at every tier (it must be: by day the fog sits at
8-9 km, so the far hills are meant to be seen). Every chunk is one draw whatever its LOD.

**Measured fix (not committed, A's file):** `chunk_quads: 128` (256 m chunks) for the open world.
Same poses:

| | town | pass | mountain | event |
|---|---|---|---|---|
| draws, 64 → 128 | 739 → **452** | 1195 → **588** | 641 → **372** | 913 → **533** |
| terrain draws | 426 → 132 | 815 → 209 | 360 → 96 | 552 → 158 |
| shadow cascade 0 | 255 → 228 | 176 → 148 | 220 → 193 | 225 → 187 |

Screenshots at the pass and in town are identical. GPU p95 over the session went 4.7 → 6.2 ms
(Low budget 10), because the full-detail radius is one chunk (now 256 m): set
`TerrainSettings.lod_distance = 128.0` with it to keep the old detail radius (not yet measured).
The saved splat map stays valid (2001 samples pad to 2048 for both 64 and 128).

**Ranked cuts:**
1. **Terrain chunks 128 quads** (+ `lod_distance` 128) — A, `src/world.rs` `TerrainDesc` (open
   world only). −290 to −610 draws; every scenario under 800. Biggest by far.
2. **Roads merged per tile per material**, like the town — A/B (`art/junctions.rs`, road meshes).
   Today 19 surface + 19 verge + 7 kerb + paint + patch meshes, all in view at once: −50 to −120.
3. **Player car**: done here, −17 (cockpit-only parts hidden in the driver's chase view: gauges,
   needles, LEDs, dial, LCD, shifter, handbrake). Next step (D): merge the car's static
   primitives per material at load (108 → ~50; detachable parts, wheels and the steering
   wheel stay separate): −50.
4. **Shadow cascade 0 at Low** — C, `art/look.rs` `shadow_distance` (160 m by day): 90 m at Low
   would roughly halve the 150-255 shadow draws. Not over any budget once (1) lands, so left as
   an option, not changed.
5. **Traffic cars** (up to 39 in view at the hairpin): each car is several parts; merge per car
   at spawn — owner of `art/traffic_car.rs`. −20 to −30 where traffic bunches.

## 2. Memory (budget 1500 MB at Low; measured max 1824 MB in `budget.py`, 1604 MB here)

Process footprint over a run (session log, 1 Hz):

| When | footprint | CPU RSS | GPU (process allocations) |
|---|---|---|---|
| boot, before the level | 537-652 MB | 446-561 MB | 88 MB |
| **end of Generate** (t ≈ 15 s) | **1502 MB** | **888 MB** | 606 MB |
| driving, first minute | 1420-1600 MB | 300-430 MB | 420-467 MB |
| `budget.py`, after the tour, pass and event | 1713 → 1818 MB | — | — |

The peak builds during loading, and play adds ~100 MB more as the map streams in. Itemised
(measured where stated, estimated otherwise):

| Item | Size | Evidence | Owner |
|---|---|---|---|
| **Music**: a decoded song is f32 stereo, 75-101 MB; the loading screen holds the loading song, plus the menu song while it crossfades | **~100-200 MB while loading**, 0 in a drive | ffprobe: 6 songs = 424 MB decoded; `music.rs` loads with any audio backend, so also in these offscreen runs | `audio/music.rs` + engine `SoundClip` |
| **CPU copies of textures** kept in the main world | **129 MB** (229 images) | `art.memory image_mb_cpu` | whoever loads each (art loaders; engine look) |
| Generation working set (heights, erosion, forest solids, PCG inputs) | ~400-500 MB transient (RSS 888 at the peak, 300 after) | session log | A (erosion work under way) |
| Streaming growth while driving (dressing cells, far detail, traffic) | ~+100 MB over 4 min | `budget.py` footprint per scenario | A / engine PCG |
| Terrain: heights 2001² f32 + padded copy 2049² + splat 2048² RGBA8 + map 2048² + canopy | ~65-80 MB (CPU and GPU together) | arithmetic | A / map |
| Car audio bank (decoded f32) | 21 MB | ffprobe | D |
| Other sounds (sfx, voice; 159 files) | up to 69 MB if all decoded | ffprobe; loaded on demand? not checked | F/E |
| Meshes (CPU copies) | 4 MB | `art.memory mesh_mb_cpu` | — |
| AI racers, career | < 10 MB | footprint at the event ≈ at the pass | F |

About 800 MB of the footprint isn't itemised (it isn't CPU heap or the GPU allocations the
engine reports): the Metal driver, shader and pipeline caches, and freed pages macOS still
counts. An engine-side census (GPU bytes per texture, mesh, render target) is the missing tool.

**Ranked cuts:**
1. **Music: don't hold decoded f32 songs while loading.** Start the loading song only once
   Generate is done, or decode to i16 (half), or stream Vorbis (engine). −100 to −200 MB at
   the peak. Owner: `audio/music.rs` (+ engine for i16/streaming).
2. **Drop CPU copies of textures** (`RenderAssetUsages::RENDER_WORLD`, as `art/car.rs` and the
   terrain material already do) for every image that's never read back. −50 to −120 MB. Owner:
   per loader; find them with an `art.memory` listing by path.
3. **Lower the generation peak** (erosion buffers, building forest solids for every road
   tile at once): A is on erosion; also see load item 2 below. −200 to −400 MB of the peak.
4. **Unload far dressing cells** (cells that streamed in stay; +100 MB over a drive): engine PCG
   streaming radius at Low. Owner: A / engine.
5. Smaller: car bank as i16 (−10 MB), map image 1024² at Low (−12 MB).

## 3. Load time (budget 8 s at Low; 17.7 s in `budget.py`, 26.6-40.5 s on the busiest runs)

Phases from the game log (three runs, Low, busy machine):

| Phase | Time | What |
|---|---|---|
| Generate: level + terrain (`open world: … in N s`) | 6.6-8.8 s | 1.4 MB RON level, terrain files, erosion at load (**A is on this**) |
| Generate: the rest of `world::generate_open` | **6.5-11.7 s** | dressing graph + `ForestSolids::fill` for **every** tile along every road and its neighbours, on all cores |
| Build | 0.9-1.2 s | world spawn 59 ms, roads/junctions 0.7 s, signs 0.15 s, conditions 6 ms |
| Warm | **4.7-18.2 s** | pipeline warm-up of 149 materials (twice); textures, car, near trees were ready after 1 s in the debug run; PCG reported 4 403 of 4 405 cells pending, which doesn't hold the phase. The spread is shader compiling on a cold or busy machine |
| Settle | 0.25 s | 30 calm frames |

**What else is slow (besides erosion):**
1. **Forest solids for the whole road network at load** (`src/world.rs` `generate_open` →
   `forest.fill(&tiles)`): build only the tiles around the spawn and the first few hundred
   metres now, and the rest from the streaming path that `open::forest` already has for
   off-road tiles. Expected −4 to −9 s. Owner: A.
2. **Pipeline warm-up: 149 materials** (Warm 4.7-18 s): fewer distinct materials (the signs,
   furniture and town share atlases already; the car has 48, the terrain extension and
   impostors each compile their own) and a persistent pipeline cache (engine). Expected −2 to
   −10 s on a cold start. Owner: engine + the material-heavy streams.
3. Level RON parse: 1.4 MB of text through the Bevy reflection deserialiser is part of the
   6.6-8.8 s; a binary or cached level would shave ~1-2 s (measure first). Owner: A / engine.

## 4. Changed in this branch

- `src/camera.rs`: dev command `perf.draws` (the breakdowns above).
- `src/art/car.rs`: the driver's chase view hides the cockpit-only parts (−17 draws, every
  scenario); cockpit view and the guide's seat unchanged (checked in a screenshot).
