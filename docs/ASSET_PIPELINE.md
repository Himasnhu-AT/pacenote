# Asset pipeline and budgets (brief §1–2)

How assets get into Pacenote, how we prove they're licensed, and how we check the frame and
memory budgets. The rules are in `docs/ASSETS_BRIEF.md` (§0 licences and trademarks, §1 budgets,
§2 pipeline); this page is how they're applied in this repo.

## Where things live

| What | Where |
|---|---|
| Raw downloads (not shipped, gitignored) and their records | `assets_src/` and `assets_src/SOURCES.md` (tracked) |
| What ships | `assets/` (copied whole by `packaging/`) |
| The shipped-asset manifest | `assets/manifest.toml` |
| Generated attribution table | `ATTRIBUTION.md` (repo root) |
| In-game credits (must include every CC BY item) | `assets/credits.txt` |
| Builders | `tools/art/` (textures, meshes, tree cards, bushes, cars), `tools/audio/cook_car.py`, engine cook (`engine/tools/cook`, KTX2 BC7/BC5) |

## Adding or changing an asset

1. Download into `assets_src/<category>/...` and add a `### Title (`key`)` entry to
   `assets_src/SOURCES.md` with **Source page**, **Author**, **Licence** (CC0, CC BY or SIL OFL
   only; no NC, no game rips, no branded vehicles or logos: brief §0).
2. Build the shipped files with the tool for its kind (textures: `tools/art/prep_textures.sh`
   then the engine cook; meshes: `tools/art/build_meshes.py` in Blender; sounds:
   `tools/audio/cook_car.py`). Ship KTX2 for textures, ≤ 2048² (≤ 1024² for anything that
   isn't a hero asset), mips on.
3. Add or update its `[[asset]]` in `assets/manifest.toml`: `paths`, `kind`, `sources` (keys
   from SOURCES.md, or `"original"`), `collision` (how `vehicle.rs` sees it), `lod`, `built_by`.
   Sounds and anything without a SOURCES entry carry `credits` instead.
4. `python3 -I tools/assets/manifest.py check` must pass (every file under `assets/` covered,
   every source recorded with page/author/licence).
5. `python3 -I tools/assets/manifest.py attribution` regenerates `ATTRIBUTION.md`. If the asset
   is CC BY, add its credit line to `assets/credits.txt`.
6. Commit the shipped files, the manifest, `ATTRIBUTION.md` and the SOURCES entry together.

## LOD rule

Brief §2: every placed mesh ships an LOD chain drawn with `VisibilityRange` (dithered
crossfade). Today most meshes rely on the engine's detail tiers instead; the manifest's `lod`
field says what each has, so the gaps are visible (`grep lod assets/manifest.toml`). New placed
meshes must ship LOD0/1/2.

## Budget check

```sh
python3 -I tools/perf/budget.py --quality Low     # or Medium / High; --secs 60; --no-build
```

Runs the dev build offscreen through `engine/scripts/play.py`: loads the open world, puts the
car on a fixed tour (`open.tour`: mid-town along the main street, out the east country road,
onto the ring; traffic drawn but not solid), the dev autopilot drives it at 0.6 grip for 60 s,
then it reads `perf.report` and `render.stats` and compares with the brief's budgets. Exit 1 if
over.

| Measured | Budget (Low / High) | Source |
|---|---|---|
| Load to control | 8 / 5 s | `ready in` in the game log |
| Memory footprint (max) | 1,500 / 2,500 MB | `perf.report` `footprint_mb` |
| GPU per frame (p95) | 10 / 12 ms | `perf.report` `gpu_frame_ms` |
| Frame time (p95) | 16.6 ms | `perf.report` `frame_p95_ms` (offscreen, no vsync) |
| Draw calls after batching | 800 / 2,500 | `render.stats` `draws` (distinct visible mesh + material pairs) |

Not measured yet: resident triangles.
The reference machine for Low is an M1 8 GB; until one is available, results on the M5 Pro are
optimistic (brief §1). Run on an otherwise idle machine: other builds inflate load and frame
times.

History (M5 Pro, Low, open world):

| When | Load | Footprint | GPU p95 | Frame p95 | Visible meshes (draws) |
|---|---|---|---|---|---|
| 2 Nov, baseline (busy machine) | 5.7 s | 2,314 MB | 5.5 ms | 28.9 ms | 1,640 |
| + tree LODs/impostors, junctions, original car (2.8 MB GLB) | 8.5 s* | 1,367 MB | 5.2 ms | — | 1,327 |
| + dressing streamed at 600 m (engine PcgStreaming), engine texture cap and hi-DPI scale, frame p95 over the drive only (idle machine) | 4.6 s | 1,342 MB | 4.3 ms | 14.1 ms | 1,328 |

| + draws after batching measured (engine 92de8b1) | 4.6 s | 1,334 MB | 4.0 ms | 15.6 ms | 1,325 (666 draws) |

\* machine busy. **All five Low budgets pass on the M5 Pro** (2 Nov). Not yet measured on a base
M1, where GPU and frame time will be higher (brief §1: treat this as necessary, not sufficient).
Draw sets (distinct materials) are 474, high: many per-chunk materials that could share.
