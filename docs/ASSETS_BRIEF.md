# Pacenote — assets, sound and world-structure brief

> Received from the owner on 2 Nov 2026 (audit output). Kept verbatim below. Repo mapping:
> the brief's `assets-src/` is our `assets_src/` (raw downloads, gitignored, `SOURCES.md`
> tracked); `ATTRIBUTION.md` is added at the repo root and kept in step with
> `assets_src/SOURCES.md` and the in-game `assets/credits.txt`; the engine's own cook tool
> (`engine/tools/cook`, BC7/BC5 KTX2) is what we use today in place of `toktx`, which isn't
> installed. Blender is at `/Applications/Blender.app/Contents/MacOS/Blender`.

For coding agents working on `pacenote` (game) and `engine` (Hull). Written 2 Nov 2026 against
commit `1be89ed`. Read `GAME_OVERVIEW.md` first; this document covers only *how the world
looks, sounds and is assembled*, and which outside resources to use. The goal is a compact
open-world driving game in the Forza Horizon structural mould (open world as the menu, events
scattered in it) that stays **original** and ships on Steam on Mac and Windows.

## 0. Rules that override everything below

- **Licences:** only CC0, CC-BY (with credit), the **Fab Standard License** for listings that
  work in any engine (owner's ruling, 7 Nov 2026: Professional tier; ship only cooked KTX2 /
  .pnm, never the raw files; Unreal-only listings are out), or assets we made. Record every
  outside asset in `ATTRIBUTION.md` (name, author, source URL, licence, where used).
  CC-BY-NC, BBC RemArc, "free for personal use" and anything from a game's own files are
  **not allowed**.
- **Trademarks and likeness:** no real car makes, models, badges, liveries or recognisable
  silhouettes; no real road signs that carry logos; no fonts whose licence forbids embedding;
  no recognisable map, festival theme or UI from any existing racing game. A CC licence on a
  3D model does not clear the trademark on what it depicts (this is why the GT3 must go).
- **Crates:** no new heavy crates, do not change Bevy features. Prefer engine features and
  in-house code. Anything listed in §10 is a candidate to *evaluate*, not an instruction to add.
- **Determinism:** world generation stays pure and seeded (`libm`, no HashMap iteration order).
  New dressing must go through the same obstacle hashing so ghosts stay valid.
- **Budgets (M1 8 GB, Low, 60 fps):** see §1. Every new asset or system is measured against them
  in a scripted session (`engine/scripts/play.py`) before merge.
- **Process:** heavy commands through `memgate`, small commits, never push, screenshots for every
  visual change, no secrets in logs.

## 1. Budgets

| Thing | Low (M1 8 GB) | High (M-Pro 16 GB+) |
|---|---|---|
| Frame | 16.6 ms, GPU ≤ 10 ms | 16.6 ms, GPU ≤ 12 ms |
| Memory footprint | ≤ 1.5 GB | ≤ 2.5 GB |
| Resident triangles (view) | ≤ 1.5 M | ≤ 5 M |
| Draw calls after batching | ≤ 800 | ≤ 2,500 |
| Player car | LOD0 ≤ 60k exterior + ≤ 40k cockpit; LOD1 10k; LOD2 2k | same |
| Traffic car | ≤ 2k tris, one 1024² atlas shared | same |
| Tree LOD0 / LOD1 / impostor | 4k / 800 / 2 quads | 8k / 1.5k / 2 quads |
| Building block mesh | unchanged (~5k per block) | unchanged |
| Textures | 1024² max on Low (KTX2, mip-capped) | 2048² max |
| Audio voices | ≤ 48 simultaneous, ≤ 20 spatialised | ≤ 96 |
| Audio memory | ≤ 60 MB decoded | ≤ 120 MB |
| Loading (to control) | ≤ 8 s | ≤ 5 s |

Set a hard failing check: `play.py` perf report over a 60 s free-roam drive through town and
onto the ring must stay under the Low numbers on the reference machine. Until a base M1 is
available, use the M5 Pro with the governor pinned to Low and treat results as optimistic.

## 2. Asset pipeline (applies to every category)

- **Source of truth:** Blender files in `assets-src/` (not shipped). Export glTF 2.0 (`.glb`),
  +Y up, metres, apply transforms, one material per mesh unless a kit shares an atlas.
- **Naming:** `category_name_lodN` (`tree_pine_a_lod0`), collision meshes `*_col`, damage
  parts `car_<name>_part_<mirror_l>`. Empties named `socket_*` for attach points (wheel hubs,
  exhaust, headlights, seat/camera).
- **Textures:** author PNG/EXR, ship **KTX2** (UASTC for normals/data, ETC1S for albedo) via
  `toktx` (KTX-Software, Apache-2.0) — the engine already has a `ktx2` feature. Mips on. Keep a
  `scripts/textures.sh` that regenerates all KTX2 from `assets-src/`.
- **Mesh optimisation:** `gltfpack` / meshoptimizer (MIT) for vertex cache order, quantisation
  and LOD generation of non-hero meshes. Hero LODs (player car, main tree species) are
  hand-made in Blender.
- **LOD:** every placed mesh ships LOD0/1/2 and is drawn with Bevy `VisibilityRange` (dithered
  crossfade). No mesh without an LOD chain goes into the world.
- **Instancing:** Bevy batches identical mesh+material automatically; keep variants as the same
  mesh with per-instance colour (vertex colour or a small data texture), not as separate
  materials.
- **Manifest:** `assets/manifest.toml` lists every shipped asset with its LOD chain, collision
  type (`solid_cylinder`, `breakable`, `brush`, `none`) and attribution key, so the dressing
  graph and the obstacle list are driven by data, not code.
- **Tools (free):** Blender (GPL; output is ours), Material Maker (MIT, procedural PBR),
  Krita/GIMP, Audacity or Ardour (GPL), `toktx`, `gltfpack`, ImageMagick.

## 3. Roads

What is wrong today: junctions are overlapping ribbons, no kerbs, no markings, verges blend
by shader only, tiling is visible, surfaces are flat colour.

### 3.1 Geometry
- **Junction meshes.** For each junction node: offset each incoming road's edges by half-width,
  intersect adjacent edge polylines, fillet the corners (radius 6–12 m town, 15–25 m country),
  build the junction polygon and triangulate it (small ear-clipping routine in-house; `earcutr`
  is tiny if a crate is approved). Trim the road ribbons back to the junction boundary. The
  lane graph's junction paths already exist; they now get real tarmac and a height (fix the
  "junction paths ignore height" limit at the same time).
- **Kerbs** as a swept profile mesh along town street edges (0.12 m high, 0.25 m wide),
  dropped at driveways and pedestrian crossings. Collision: not needed (the car sim treats them
  as surface change; add a `kerb` surface with a small bump input to the suspension).
- **Camber and crown:** add a 2 % crown to the road profile; it sells wetness and lighting.
- **Verges:** a separate 1.5–3 m strip mesh with grass/gravel blend, mesh-based, not shader-only,
  so the road edge has a real silhouette. Ditch profile on country roads.
- **Shoulder props:** posts, delineators, guardrails on the ring (breakable/solid), marker stones
  on forest roads, cattle grids and gates at farm tracks.

### 3.2 Surface and markings
- **Textures:** ambientCG (CC0): `Asphalt0xx`, `Road0xx`, `Gravel0xx`, `Ground0xx` (mud),
  `Rock0xx`; Poly Haven (CC0) `aerial_asphalt`, `aerial_gravel`, `forest_ground`. Tile at 4–6 m,
  break tiling with a 64 m macro-variation mask and a 0.5 m detail normal in the WESL material.
- **Wear:** darker centre of lane, lighter tyre lines, oil in town; one "wear" mask channel
  sampled along the road's station so it follows curves.
- **Markings** as **decals** (Bevy `ForwardDecal` / clustered decals) or a thin offset mesh
  with UV along station: centre dashes, edge lines, stop lines, zebra crossings, arrows in
  town; none on gravel. Keep marking colour/ style original (not a copy of any country's exact
  standard).
- **Surface data for the sim** stays per 2 m station (already done); add `kerb`, `rumble`,
  `puddle` as surface kinds with their own grip and sound.
- **Wet variant:** roughness map + puddle mask driven by the weather setting; no new textures.

### 3.3 Signs and street furniture
- Kenney (CC0): City Kit (Roads, Suburban, Commercial, Industrial), Car Kit, Racing Kit,
  Road Textures; Quaternius (CC0): Ultimate Modular Buildings, Cars, Nature, Props. These are
  low-poly and stylised; use them as **prop bases and references** and restyle to our look
  (retexture with ambientCG materials, add bevels), not as-is.
- Design an original sign set (speed limit, junction warning, place names) as one atlas.
  Place names come from a seeded word list; add **landmarks** (water tower, radio mast, petrol
  station, grain silo, bridge over the ring, a church/town hall) so the AI navigator and the
  player can orient without a minimap.

## 4. Terrain

- Keep the heightfield and road shaping. Improve the material: four-layer splat (grass, dirt,
  rock, gravel) chosen by slope, height, distance-to-road and a noise mask; **triplanar** on
  slopes > 35°; macro colour variation from a 1 km noise; detail normal at 0.5 m; per-layer
  roughness. ambientCG/Poly Haven CC0 ground textures as above.
- **Grass cards** (two crossed quads, alpha-to-coverage) within 60 m of the player on grass
  surfaces, spawned per 32 m cell from the dressing graph, wind in the vertex shader. Off at Low.
- **Tyre tracks and dust:** keep the existing spray VFX; a render-to-texture track mask is a
  phase-2 nicety, skip for now.
- **Far terrain:** lower-resolution chunks past 800 m (the engine `world` HLOD path); make sure
  skirts hide seams.

## 5. Trees and vegetation

Target feel: dense pine/spruce forest on forest roads, birch and oak at field edges, hedgerows
along country roads, almost none in town except parks and street trees.

- **Authoring:** Blender's bundled *Sapling Tree Gen* add-on or the free *Modular Tree*
  add-on → bake each species to LOD0 (4–8k tris: trunk + 6–10 foliage cards), LOD1 (800–1.5k),
  LOD2 **octahedral impostor** (bake 8×8 views to a 1024² atlas; shader picks the view). 3–4
  variants per species, plus per-instance scale (0.85–1.2) and yaw jitter from the seed.
- **Textures:** Poly Haven / ambientCG bark (CC0); foliage cards painted or baked from
  Blender particle leaves; alpha-to-coverage, two-sided, subsurface tint for leaves.
- **Wind:** vertex animation in WESL (trunk sway ∝ height², leaf flutter from a global wind
  time), strength from the weather setting.
- **Streaming:** trees live in cells; only cells within 1.2 km resident, impostors beyond 300 m,
  LOD1 within 300 m, LOD0 within 80 m (`VisibilityRange` crossfade). This replaces the
  whole-map 145k resident trees. Far-card density on Low = half.
- **Collision:** trunks within 30 m of a road solid (exists). Hedgerows as brush.
- **Understory:** shrubs, ferns, fallen logs, boulders from the dressing graph with the same LOD
  rule; forest floor decals (needles, leaves) near roads.

## 6. Buildings and town structure

The procedural block mesh and facade atlas are a strength; keep them and make them richer.

- **Facades:** replace the single 1024² atlas with a **trim sheet** (one 2048² on High, 1024² on
  Low) holding window types, doors, shopfront, brick/plaster/concrete bands, cornice, parapet.
  Generate façades storey-by-storey from the trim sheet with per-building colour via vertex
  colour. Add ground-floor shops with awnings and signs (original names) downtown, balconies
  on apartment slabs, garage doors on houses.
- **Night:** emissive window mask with a seeded "lit" pattern that changes by time of day;
  shop signs emissive; street lights with Bevy light textures (cookies) for the pool.
- **Roofs:** flat roofs get AC units, vents, water tanks; gable roofs get chimneys. All as
  instanced props from the manifest.
- **Streets:** sidewalks with real kerb meshes, driveways, bins, benches, bus stops, lamp posts,
  bollards, hydrants, bike racks, trees in grates; traffic lights as static props (working
  lights are phase 2; traffic already has junction right-of-way).
- **Parks:** paths, benches, a bandstand, lamp posts, hedges, flower beds. **Car parks:** parked
  traffic cars (same meshes, random paint, solid).
- **Landmarks** (see §3.3) at the ends of the main streets and at the ring bridges.
- **Outside town:** farm yards (barns exist; add fences, silos, hay, tractors as props), a
  petrol station on each country road, a forest ranger hut at each forest road start, a quarry
  on one quadrant, a lake with a jetty in another. These double as event markers and navigator
  waypoints.
- **Collision:** keep cylinders for walls (16k is fine); use boxes if the sim gains them later.
- **Reference kits:** Kenney City Kits, Quaternius Modular Buildings (both CC0) for prop
  shapes; textures from ambientCG (`Bricks`, `Plaster`, `Concrete`, `Metal`, `RoofingTiles`).

## 7. Cars

- **Player cars: original designs, modelled in Blender.** Write a one-page design sheet per
  car (silhouette, proportions, era, drivetrain, personality) before modelling; the sheet must
  not reference a real car. Start with the rally hatch that matches the tuned sim
  (~1,250 kg 4WD, 5-door, boxy, wide arches, roof vents, mud flaps). Then a rear-drive coupe, a
  pickup, a fast GT. Distinct handling is sim parameters (`vehicle.rs`), not new physics.
- **Topology:** separate meshes for body, doors, bonnet, boot, bumpers, wings, mirrors,
  splitter, diffuser, wheels (4), brake discs, glass, interior, dashboard, wheel, seats, hands.
  Damage parts match the existing part list so damage code keeps working.
- **Materials:** Bevy `StandardMaterial` clearcoat for paint, anisotropy for brushed metal,
  transmission for glass, emissive for lights. Livery as a decal/overlay texture on a UV2 so
  paint and livery are independent (player liveries later).
- **Cockpit:** keep the detailed cabin, cut to ≤ 40k tris, dash textures 2048² only on High.
  Gauges stay driven by the sim.
- **Traffic cars:** Quaternius/Kenney CC0 vehicles restyled (generic shapes, no badges),
  ≤ 2k tris, one shared atlas, 8–10 paints by vertex colour; 4–5 body types (hatch, saloon,
  van, pickup, lorry) so the road reads as a real town.
- **Wheels:** separate mesh, spin and steer from the sim; tyre with tread normal map; dirt
  mask driven by surface history (cheap: a float per car that rises on gravel, falls on tarmac).

## 8. Sound

Current: 5 steady-RPM CC0 engine recordings crossfaded by RPM/load, drift-only tyres,
synthesised wind/impacts/ambience, OS TTS co-driver, own rodio stream outside the engine mixer.

### 8.1 Engine (the core of the feel)
- **More RPM points:** 5 steady recordings force pitch stretches beyond ±15 %, which is where
  engine audio starts sounding synthetic. Target **8–12 RPM points × on-throttle and
  off-throttle** per car (16–24 loops of 2–4 s each). Crossfade by RPM; pitch-shift never
  more than ±8 %. Loop points zero-crossing aligned, seamless.
- **Layers:** intake (throttle-driven), exhaust (RPM-driven, louder in chase cam), mechanical
  (valvetrain, constant), transmission whine (speed- and gear-driven), turbo spool/flutter
  if the car has one (synthesised: filtered noise + sine), rev limiter stutter (gate on the
  exhaust layer), overrun pops (exists), starter and idle lump.
- **Per-camera filtering:** cockpit = low-pass ~3 kHz + body resonance, bonnet = exhaust-heavy,
  chase = full. Exists partly; make it a per-bus EQ.
- **Granular synthesis** is the next step up (pick grains from the recording nearest the
  current RPM, play them back-to-back): same recordings, far smoother than crossfades. Build
  as an in-house DSP module (you already have DSP tests); evaluate after the sample set grows.
- **Sources (CC0):** freesound.org with the licence filter set to **CC0** (search "engine
  steady rpm", "onboard", "exhaust"); Sonniss *GameAudioGDC* bundles (royalty-free,
  commercial OK, no attribution); OpenGameArt (check per asset). Record your own if a
  4-cylinder is available: steady-state recordings at 1,000–7,000 rpm in 500 rpm steps, two
  mics, are an afternoon's work and remove all licence doubt. BBC Sound Effects: **not
  allowed** (non-commercial).

### 8.2 Tyres and surfaces
- Keep drift-only rolling noise per the owner, but add the sounds that make rally roads feel
  physical: **gravel spray on the underbody** (speed- and surface-driven, random stone hits),
  mud splash, puddle splash, kerb/rumble-strip thump, grass swish, loose-gravel crackle on
  sharp steering at low speed.
- Slip sounds in three layers by slip angle and speed (light scrub, full slide, lock-up), per
  surface (tarmac squeal, gravel roar, mud). Stereo pan to the sliding axle.

### 8.3 Car body and suspension
- Bump thuds and bottoming-out hits from suspension travel velocity; body creaks on big
  weight transfer; rattles after damage; brake squeal; handbrake ratchet; gear-shift clunk
  (exists); door/body parts falling off.
- Wind: speed-driven filtered noise (exists) plus whooshes when passing trees/signs within 3 m
  at > 80 km/h (cheap, sells speed).

### 8.4 Traffic and environment
- Spatialised engine loops for traffic within 120 m, **Doppler** on pass-by, horns on near
  misses (IDM "emergency brake" event), tyre hiss on wet. Cap at 12 traffic voices by distance.
- Ambience by biome and time: forest birds/wind, field insects, town hum and distant traffic,
  church bell on the hour, ring road hiss. Interior attenuation in cockpit.
- Impacts: scale by relative speed and material (tree, rock, wall, car, sign).

### 8.5 Co-driver voice
- Stop using OS TTS at runtime. Build a **phrase bank** once at build time (per note word and
  number, plus damage and recovery lines; ~200 clips) and play them through the game mixer
  with short crossfades. Generate with **Piper** (open-source neural TTS; check each voice's
  licence) or record a human voice actor (one session, best result). The planner already
  produces word sequences, so this is a renderer swap.
- Voice bus ducks engine and tyres by 4–6 dB while a note plays; radio-style band-pass for the
  phase-2 friend radio.

### 8.6 Mixing and engineering
- Buses: engine, tyres, body, environment, traffic, voice, UI, music (none yet). Per-bus
  settings sliders, sidechain duck under voice, soft limiter on master, target loudness
  around −18 LUFS integrated, true peak −1 dB. Sound test scene via a dev command.
- **Engine audio stack:** bevy_audio (rodio) cannot pitch live, which is why the car mix runs
  in its own rodio stream. Options, in order of preference under the no-heavy-crates rule:
  (a) keep the separate stream but move all car and traffic sound into one in-house graph
  (voices, buses, filters, spatialisation) with the engine `sound` plugin for UI/ambience;
  (b) evaluate **bevy_seedling** (Firewheel) at the *engine* level as the single mixer — it is
  the current community direction for Bevy audio, supports graphs, effects and spatial audio,
  but requires disabling `bevy_audio` across all three games, so it is an engine-owner decision.
- Clip format: OGG Vorbis for ambience/music (streamed), WAV 44.1 kHz 16-bit for short SFX,
  loops with embedded loop points in the manifest.

## 9. Lighting and look (Forza-feel on a budget)

- **Time of day:** drive a sun `DirectionalLight` and Bevy's procedural `Atmosphere` from a
  clock; keep the existing `look` grading per hour. Dawn/dusk and night are the cheapest
  "wow" the game can add.
- **Shadows:** 4 cascades, first cascade tight (0–25 m) so the car and cockpit shadows are
  sharp; shadow-casting trees only in LOD0/1.
- **Post:** tonemapping (TonyMcMapface or AgX), auto exposure, small bloom on emissive/
  highlights, SSAO on High, volumetric fog for low sun through trees on High, motion blur off
  by default (option).
- **Night:** headlights as two spotlights with a cookie, traffic uses emissive only (clustered
  light count is the limit); street lights with cookies in town; emissive windows (§6).
- **Weather:** rain streaks on glass (screen-space in cockpit), wet roads via roughness,
  spray VFX stronger; fog by weather setting.
- **Solari / DLSS** (Bevy 0.20): not for this game's budget; do not enable.

## 10. Bevy 0.20 features and crates to use or evaluate

Use (built-in): `VisibilityRange` LOD crossfade, automatic batching/instancing, `ForwardDecal`
and clustered decals, `StandardMaterial` clearcoat/anisotropy/transmission, procedural
`Atmosphere`, volumetric fog, SSAO, auto exposure, light textures (cookies), CSM, occlusion
culling (experimental; test on Metal before relying on it), WESL materials, KTX2 loading,
`bevy_audio` spatial sources for simple emitters.

Evaluate only with engine-owner approval (check each crate has a 0.20 release first):
`bevy_seedling` (audio graph), `bevy_hanabi` (GPU particles, for spray/dust/leaves — heavy),
`earcutr` (triangulation, tiny), `kira` via `bevy_kira_audio` (if seedling is rejected and the
rodio stream is dropped). Do not add terrain, physics or navigation crates; those systems are
in-house by design.

Learning resources: Bevy 0.20 release notes and migration guide (bevy.org/news), the Bevy
examples repo (shadows, decals, atmosphere, visibility_range, ssao, volumetric fog, spatial
audio), Tainted Coders Bevy guides (updated for 0.20), Bevy Discord `#rendering` and
`#audio`. For road/junction construction, study how open-source city builders build
intersections from offset lane edges; for impostors, the octahedral impostor technique
(Ryan Brucks' write-up); for engine audio, any granular-synthesis primer and the
*Engine Simulator* project for how RPM-indexed sound behaves (reference only, check licence
before reusing code).

## 11. Order of work

1. Pipeline (§2): manifest, KTX2 script, LOD rule, attribution file. Small, unblocks all else.
2. Player car replacement (§7): rally hatch, LODs, damage parts, cockpit cut. Legal blocker.
3. Tree streaming and impostors (§5): the memory and GPU win that makes Low viable.
4. Junction geometry, kerbs, markings (§3): the thing every screenshot currently gives away.
5. Engine sound set and co-driver phrase bank (§8.1, §8.5).
6. Town dressing, landmarks, night lighting (§6, §9).
7. Terrain material and grass (§4), traffic car variety (§7), traffic/environment audio (§8.4).
8. Measure on the base M1 after 3 and again after 6; fix before moving on.

## 12. Definition of done (per area)

- Every asset in the manifest with licence and LOD chain; `ATTRIBUTION.md` complete.
- Scripted session screenshots at town centre, ring, forest road, farm, at noon and at night.
- Perf report under §1 Low budgets; memory report from `art.memory`.
- Determinism tests still pass (`stages_hash_the_same_everywhere`,
  `obstacles_the_same_everywhere`, scripted drive hash) — if dressing changed, bump the
  obstacle revision and say so in the commit.
- Sound: a 30 s capture of a forest-road drive in cockpit and chase, loudness within target,
  no clicks at loop points (test with the existing DSP tests).

## 13. Licence quick table

| Source | Licence | Use |
|---|---|---|
| Kenney (kenney.nl) | CC0 | props, kits, UI |
| Quaternius | CC0 | buildings, cars, nature bases |
| Poly Haven | CC0 | textures, HDRIs, some models |
| ambientCG | CC0 | PBR textures (roads, ground, walls) |
| 3dtextures.me | CC0 | textures |
| freesound.org | mixed — **filter CC0** | SFX, engine recordings |
| Sonniss GameAudioGDC | royalty-free, commercial OK | SFX |
| OpenGameArt | mixed — check each | SFX, textures |
| Sketchfab | filter CC0/CC-BY; **check trademarks** | props only, never vehicles with brands |
| BBC Sound Effects | non-commercial | **do not use** |
| Piper TTS voices | per voice | co-driver bank (check voice licence) |
| Blender, Material Maker, toktx, gltfpack | GPL/MIT/Apache | tools; output is ours |
