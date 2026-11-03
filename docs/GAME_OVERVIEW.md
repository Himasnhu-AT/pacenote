# Pacenote: concept, current state, and how it is built

Written 2 Nov 2026 for an outside audit (human or AI). It describes what the game is meant to
be, what exists today, how it is built and how we work, and lists the known problems and open
questions. Statements are about the code at commit `1be89ed` on `main` unless dated otherwise;
section 4 (the map) and the rows and issues that touch it were updated on 3 Nov 2026 for
`095371a`.
Where a detail lives in another document, the path is given; those documents are more precise.

**How to use this document as an auditor:** read sections 1–3 for the picture, 4–7 for how it
works, 9 for what we already know is wrong, and 10 for the questions we most want answered. The
repo is `/Users/himanshu/codes/game/pacenote` (game) and `/Users/himanshu/codes/game/engine`
(the in-house engine, "Hull"). Section 12 says how to build, run and test.

---

## 1. Concept

### 1.1 Where it started (Oct 2026)

Pacenote began as a **two-person rally game**: a *driver* and a *guide* race one car through
point-to-point stages. The driver sees only the road; the guide sees the stage map and reads
pace notes ("left four into right two, don't cut, crest") over in-game voice. Teams compete on
per-stage leaderboards. Pillars (from `docs/DESIGN.md`):

1. **Trust and talk:** the fun is the conversation; neither player can win alone.
2. **Driving that feels earned:** weight transfer, slides on loose gravel, crests that go
   light; mistakes cost time, not restarts.
3. **Stages worth learning:** 20 fixed, seeded stages, so times are comparable.

A solo mode had an automatic co-driver reading the notes (an AI voice).

### 1.2 Where it is going (owner's decision, 2 Nov 2026)

The owner asked to turn it into an **open-world driving game in the open-world racing genre**:
no stage maps or stage select, one world to drive around freely, with NPC traffic, a town with
buildings, and events you drive to. Decisions (also in `docs/OPENWORLD.md`):

| Topic | Decision |
|---|---|
| Stage select | Removed. Play drops you into the world. |
| Activities | Free roam, plus **events you drive to**: markers in the world start a route (rally, sprint, lap) with clock, results and records. |
| Co-driver | **No co-driver in the car.** During an event an **AI voice** reads the pace notes for the route. Free roam has no notes. |
| Multiplayer | Friends racing together will talk over a **radio** (voice chat). Not built yet (phase 2). The old driver + guide mode still exists but only on the old stages. |
| NPC cars | **Traffic** (ambient AI cars). No racing opponents yet. |
| World size | **Compact first (4 x 4 km)** to measure the engine and procedural generation on Bevy 0.20 on a base MacBook; a larger region (~10 x 10 km) later. |
| Sound | Engine and drift (tyre slide) only; road noise removed. |
| Originality | An original game in the genre; it must not copy any existing game's map, cars, branding or festival. |

**Tension worth auditing:** the original identity ("one drives, one reads the road") and the
new one (open-world free roam with traffic) pull in different directions. Pace notes now only
matter inside events. Section 10 asks what the unifying hook should be.

### 1.3 Business context

- Target: **Steam**, first as a **free demo** to build a wishlist list, then a paid game.
  Packaging scripts, store copy drafts and assets exist (`docs/STEAM.md`, `packaging/`).
- **Anonymous telemetry** (opt-out) is collected so the owner can post aggregate stats on
  Reddit to promote the game (`docs/TELEMETRY.md`, `tools/telemetry/analyze.py`).
- A **marketing website** (Astro static site) exists in `web/` (26 pages, `docs` in `web/`).
- Platforms: macOS (Apple Silicon first) and Windows (cross-compiled). Linux builds but is
  untested as a product.
- The owner is a solo developer working with AI coding agents (section 8).

---

## 2. Current state at a glance

| Area | State | Notes |
|---|---|---|
| Open world (4 x 4 km) | **Playable** | 23 roads (~31 km), terrain, town and farms with 617 buildings, ~238k trees. Stored as a level file the game loads. |
| World editor (dev) | **Working** | The engine's editor on the game binary: roads, buildings, event starts, trees, terrain; edits rebuild the world live. Needs the engine's `editor` branch. |
| Free roam | **Playable** | Spawn in town, R recovers onto nearest road, no clock. |
| Events (7) | **Playable** | 4 forest rallies, 2 gravel sprints, 1 ring lap; AI pace notes; records, ghosts, results. |
| Traffic | **Playable** | 40 AI cars around the player; lanes, junction right-of-way; solid. Off during events. |
| Car handling | **Mature** | Custom deterministic sim, tuned and measured (`docs/HANDLING.md`). |
| Damage | **Working** | Engine/steering/suspension damage, body parts come off, retire on a fully broken part. |
| Cockpit / views | **Working** | Cockpit (default), bonnet, chase. Detailed cabin with animated hands. |
| Audio | **Working** | Recorded CC0 engine + drift; synthesised wind, impacts, ambience. |
| AI co-driver voice | **Working** | OS text-to-speech; events only in the open world. |
| Menus, settings, HUD | **Working** | Engine UI kit; recently ported to Bevy 0.20 picking. |
| Records & ghosts | **Working** | Local leaderboard file, ghost replay (bit-exact). Per event id in the open world. |
| Driver + guide (2 players) | **Legacy** | Works on the old 20 stages only; not in the open world. |
| Voice chat | **Legacy** | Works in the driver + guide session; radio for the open world not built. |
| The 20 old stages | **Hidden** | Still in code; reachable with `--stage N` and from "Play together". |
| Map / minimap | **Missing** | No way to find events except driving around. |
| Online play, matchmaking | **Missing** | LAN / forwarded port only (engine has no relay). |
| Steam demo build | **Outdated** | Built for the 3-stage demo before the open-world pivot. |
| Website / store copy | **Outdated** | Describes the stage-based game. |
| Telemetry | **Working** | Events keyed by stage id; open-world events have ids 100+. |
| Metrics viewer (dev) | **Working** | Separate process. A live-update bug (core keys frozen after 1 s) was fixed in engine `8ab2de8` on 2 Nov. |
| Performance on a base MacBook | **Unverified** | First-launch quality picks Low on 8 GB; never measured on such a machine. |

Size: ~31,500 lines of Rust in `src/` (plus tests), 212 library tests + integration tests, all
passing at `1be89ed`. Assets ~110 MB (`assets/`).

---

## 3. What the player does today

1. **Main menu → Play** loads the open world. The world itself is ready in under a second (read
   from the level file); the loading screen then warms render pipelines and trees, ~10 s in
   all on an M5 Pro (measured 2 Nov).
2. The car starts on a town street. Drive anywhere: town grid, country roads, a highway ring,
   forest gravel roads. Traffic drives around you. Buildings, trees and boulders are solid.
3. **Events:** glowing columns mark event starts (orange = rally, blue = sprint, purple = lap).
   Driving into one shows a toast with its name, kind and length; **stopping inside** starts it.
   The car is placed on the start line, a 3-second countdown runs, traffic disappears, and the
   AI voice reads pace notes ahead of each corner. The HUD shows the event's clock and progress.
4. At the finish: the **results screen** (time, splits, rank against your own and the best
   records, ghost saved if it's your best). **Retry**, **Free roam** (continue roaming from
   where you are) or **Menu**.
5. Esc pauses (Resume, Restart, Recover +5 s, Settings, Quit to menu). R recovers. C changes
   camera. N toggles the co-driver voice.

Controls: keyboard (W/S/A/D or arrows, Space handbrake, R recover, C camera) and gamepad
(analogue). Keyboard steering ramps (0.35 s to full lock at 20 km/h, 0.85 s at 110 km/h).

---

## 4. The map (open world, `src/open/`, `docs/OPENWORLD.md`, `docs/RALLY_MAP.md`)

One 4 x 4 km region (−2,000 … +2,000 m in x and z), rebuilt on 3 Nov 2026 **terrain first** for
a sim-rally feel (`docs/RALLY_MAP.md` has the plan and checklist). It is generated from one seed
(`open::SEED`) by pure, deterministic Rust (`libm`, section 6.2) and **baked into a level
file**, which is the source of truth: the game loads it and the editor (4.7) changes it.

### 4.1 Layout at a glance

```
                           N (+z)
┌─────────────────────────────────────────────────────────┐ z = +2,000 m
│ ALPINE PLATEAU (380-490 m)   radio tower                │
│  hillclimb top ●──── mountain pass (forest) ────● basin │
│  │                    │                         │  road │
│  │ tarmac      FORESTRY ESCARPMENT      FAST GRAVEL     │
│  │ hillclimb   (180-380 m)   │   gully  BASIN (140-240) │
│  │ (switch-    camp ● logging│   stream   quarry ●      │
│  │  backs)     track─┘       │     │         │          │
│  │                           │     gully road│          │
│  ●══ valley B-road ═ VILLAGE ═╧═ SERVICE PARK ═════════●  │
│      (tarmac)    ~~~~~~~~~ river ~~~~~~~~~~~~~          │
│        ╰──── southern gravel loop (2 crossings) ────╯   │
└─────────────────────────────────────────────────────────┘ z = −2,000 m
x = −2,000 m                                             x = +2,000 m
```

- **Land** (`land.rs`): zones from a warped macro shape (valley floor 80-120 m, basin, an
  escarpment, a plateau up to ~490 m: **405 m of relief**, was 91), domain-warped ridged noise,
  then **600k droplets of hydraulic erosion** on a 4 m grid (gullies, spurs, fans) and a
  moisture map from where the water ran (mud on forest roads; ecology next).
- **Roads solve the land** (`plan.rs`): an anisotropic A* on a 10 m grid in 16 headings; each
  step costs its length times grade above a target (4-5 %, soft limit 11-14 %), cross-slope and
  turning. On steep ground the cheapest path traverses and **switches back**; the turn limit per
  step gives **hairpins of ~8-10 m radius** (tightest on the map: 8.1 m). Paths are smoothed
  (Chaikin), untangled (no road crosses itself), and approach every junction straight.
- **9 roads, ~29.5 km, 15 junctions, no dead ends** (every road ends on another):
  - valley **B-road** (6.5 m tarmac, 3.7 km) through the village and service park;
  - **tarmac hillclimb** (6.5 m, 5.2 km) up the west side to the plateau;
  - **mountain pass** (forest, 3.4 m, 3.7 km) over the plateau by the radio tower;
  - **basin gravel** (4.6 m, 5.6 km) past the quarry and up the east wall;
  - **forestry road** (3.4 m, 5.9 km) from the village up the escarpment past the logging camp
    to the pass, and a **logging track** (0.7 km) from the camp to the hillclimb;
  - **gully road** (gravel, 1.9 km) from the service park to the basin road;
  - **southern loop** (gravel, 2.2 km) over the river twice;
  - a **village lane** (5 m, 0.5 km).
- Widths are rally widths: B-road 6.5 m, fast gravel 4.2-5.0 m, forest 3.0-3.8 m (were 14 / 11 /
  8 / 5.4-6.6 m).

### 4.2 Roads (`roads.rs`, `plan.rs`, `data.rs`, `junctions.rs`)

- In the level, each road is an engine **`Spline`** (Catmull-Rom; 4,186 control points for the 9
  roads, about one per 7 m since the hill roads bend and climb all the time) plus a
  `RoadSpline { kind, speed limit }`. Kinds: `Street`, `BRoad`, `Gravel`, `Forest` (old files'
  `Highway` / `Country` still read).
- Loading samples each spline every 2 m into a `stage::Road` (direction, curvature, width,
  banking, surface, lookup grid), so road meshes, pace notes and records reuse stage code.
  Junctions are found again from where roads meet.
- Road heights follow the land, held to the grade (14 % forest, 13 % gravel, 11 % tarmac; up to
  ~17 % for a few metres where a road meets another), crests kept.
- **Junctions** get their own meshes (`open/junctions.rs`, `art/junctions.rs`), ribbons trimmed
  back to the mouths. No town kerbs or zebras any more.
- **Cross-section** (`profile.rs`): a crown (5-8 cm), worn wheel tracks (2-4 cm) on gravel,
  corners banked in (60 %), flat (25 %) or off-camber (15 %, called "off camber"), small
  undulations on gravel and forest roads; beyond the edge a 0.5 m shoulder, then on the side cut
  into the hill a V ditch 0.25-0.45 m deep and on the other side the ground, dropping off steeply
  where the land falls away. The car feels this exactly near a road; the terrain is shaped to
  match.
- **Grip across the road:** gravel wheel tracks are firm; the crown, the outside of bends and the
  edges are loose gravel (`Loose`, 85 % grip); the ditch drags (`Ditch`); tarmac within 40 m of a
  gravel road's junction is dusty (`Dusty`, 82 %); mud where the moisture map is wet.

### 4.3 Terrain

- A 2001² heightfield at 2 m, stored with the level as `assets/levels/open/terrain.r16` (16-bit,
  ~6 mm steps over the 405 m range) + `terrain.ron`.
- It is the eroded land **shaped by the nearest road** with the stage rules (road sunk in, a
  shoulder, fill ≤ 16 %, cut ≤ 45 %, blending back 34-52 m out). In the editor it can be
  sculpted and painted; moving a road re-shapes the ground around the part that moved (from the
  land, rebuilt from the seed).
- Render: the engine's chunked, LOD'd terrain with a 4-layer splat material (`art/terrain.rs`).

### 4.4 Buildings (`buildings.rs`, `level.rs`)

- **110 buildings** (were 617): a **valley village** of houses and, near its middle, shops on
  both sides of the B-road and the lane (about 90), big sheds at the **service park**, and
  farmhouses with barns along the low gravel roads. Each faces its road, sits on its own ground
  (no town plateau any more) and is kept clear of every road.
- Each is a **level entity** with a `Building` component placed by its Transform, drawn as its
  own mesh with one shared material (a procedural facade atlas + vertex colours).
- **Collision:** each facade is a **line-segment wall** (`vehicle::Wall`, `open::walls`), so a car
  scrapes along it instead of snagging (a 4° brush at 110 km/h keeps 97 % of its speed; the old
  rows of 1.2 m cylinders spun it in).

### 4.5 Trees, rocks and dressing

- Trees (pine / spruce / birch, near and far versions), rocks, shrubs and ground cover come from a
  `pcg-core` graph, kept off every road, building and the village. **Ecology** (`open/ecology.rs`):
  none above the **treeline** (~410 m: stunted trees and scree on the plateau), on the flat
  **valley fields** (single trees), or in **7 clear-cuts** beside the gravel and forest roads
  (stumps, a timber pile; long views open up). Two engine PCG volumes: trees (whole map) and
  dressing (streamed near the camera).
- **Solid:** every trunk, boulder and **stump** (0.3 m), anywhere on the map (128 m tiles built
  in parallel along the roads at load and around the car as it drives), building walls, **bridge
  parapets**, **culvert headwalls** and timber piles (line-segment walls).
- **Road dressing** (`art/roadside.rs`): stone bridges where the southern loop crosses the river
  (narrow deck, parapets), culverts in the inside ditch of tight corners ("don't cut"), snow
  poles along the plateau roads, chevron boards on deceptive corners, event boards (100 m to the
  finish, the finish, a marshal post), brake marks before tight corners.
- Hand edits (removed, moved or added trees, cleared or thinned areas) live in the level as
  `PcgOverrides` on two entities, `Trees` and `Dressing`; the volumes and the car's colliders
  apply the same edits. Trees follow sculpted ground (`PcgFollowTerrain`).

### 4.6 Events, home and traffic on the map

- **Events (5)**, each a **chain of road pieces across junctions** (`EventData::then`):
  **Fernhollow escarpment** (rally, 6.9 km of forest track from the village to the plateau),
  **Blackpine marathon** (rally, 12.8 km: tarmac hillclimb, gravel over the plateau, down the
  basin road), **Quarry loop** (5.2 km, tarmac and gravel round the basin), **River crossings**
  (2.8 km, over the river twice) and the **Hillclimb** (5.2 km tarmac). Each finishes onto a
  road that leads on. A translucent coloured marker at each start; stop within 9 m for 0.6 s to
  start. While one runs, **red and white tape** closes every junction branch it doesn't take.
- Starting sets the world's active route (`StageWorld::road`) and computes its corners and
  crests, so the stage machinery runs it: countdown, clock, pace notes (hairpin / square / 2-6,
  tightens, opens, over crest, crest into, narrows, off camber), records and ghosts keyed by
  event id and the level's content, results screen.
- **Home** is on the B-road by the service park.
- **Traffic** (`traffic.rs`) drives **only the tarmac** (B-road and village lane: the gravel and
  forest tracks are single-car rally roads); cars turn round where the tarmac ends. Intelligent-
  driver model, speed limits, junction reservation. Solid; free roam only.
- There is **no map or minimap** in the game: events are found by driving around (9.4).

### 4.7 The level file and the editor

| Level entity | What |
|---|---|
| `road 0 (BRoad)` … `road 8 (Street)` | `Spline` + `RoadSpline` |
| `open world` | `WorldDoc`: seed, village height, time of day, events, terrain folder |
| `home`, `event start: …`, grade notes | `HomePoint`, `EventStart`, `GradeNote` (placeables) |
| `house 0` … (110) | `Building` |
| `Trees`, `Dressing` | `PcgOverrides` (+ the game's PCG volume at run time) |

- Files: `assets/levels/open.level.ron` (1.0 MB) + the terrain folder (8 MB). A missing or bad
  level logs an error and the world is generated from the seed (the land takes ~3 s to erode,
  the roads ~2 s); a test loads every committed level.
  `pacenote --bake-world <file>` re-bakes from the seed (throws away every edit).
- **Editor:** `cargo run -- --editor --level assets/levels/open.level.ron` opens the game in the
  engine's editor (Outliner, Details, tools for splines, sculpt, paint, foliage, place). Edits
  rebuild the world ~0.7 s after the last change (`open/live.rs`): road meshes and junctions,
  the ground around a moved road, buildings, event routes and markers, trees, traffic lanes,
  colliders. Undo works the same way. **Play** runs the game on the edited level.
- Parity: the baked world was compared with the generated one from 8 camera spots and by counts
  (heights within 1 mm, trees within 31 of 238k, same events within 4 m).

---

## 5. Driving, damage, and feel

### 5.1 The car sim (`src/vehicle.rs`, `docs/HANDLING.md`)

- **Custom, pure, fixed-step (60 Hz, with substeps)**: no physics engine. Rigid-body chassis,
  per-wheel spring/damper/anti-roll bar/bump stop, combined-slip tyre model per surface,
  torque curve, engine braking, 5-speed auto gearbox, 4WD with limited-slip diffs, handbrake.
- Tuned to a ~1,250 kg 4WD R5-class rally car (sources and measurements in `HANDLING.md`).
  The visual car is "Talus", an original 5-door rally hatch made for it
  (`docs/cars/rally_hatch.md`, `tools/art/build_rallycar.py`).
- The world is accessed only through `vehicle::Ground` (height, normal, surface, obstacles),
  which is why the open world could be plugged in without touching the sim.
- Collisions: chassis corners vs ground, and vertical cylinders (`Obstacle`): solid (trunks,
  boulders, walls, traffic), breakable (saplings, posts, signs), brush (shrubs).
- **Deterministic**: ghosts replay recorded inputs through the sim bit-exactly; a golden hash
  test pins a scripted drive (`REVISION` 8 after the Bevy 0.20 / glam 0.33 bump).

### 5.2 Damage

Hits above walking pace against solid things damage the engine (power), steering (lock, speed,
pull) and suspension (grip, wallow) by location and severity, and knock off body parts (mirrors,
wing, splitter, bumpers, diffuser). Settings > Driving > Damage picks the mode, part of the car
spec (so ghosts replay it): **None** (nothing breaks), **Cosmetic** (parts come off, the car
drives as new) or **Simulation** (default). In Simulation a fully broken part leaves the car
**not drivable** until the player recovers it: back on the road where it is, fully repaired
(+5 s in an event). One crash that takes half the car or more (worst part +50 % in one step)
is **fatal**: "You're dead" for 2.5 s, then home (free roam) or the event restarts from its
line. Recovers and repairs are recorded in ghosts so replays match. Damage shows on the HUD (Simulation only); spoken by the co-driver (in events).

### 5.3 Pace notes and the AI voice (`src/codriver/`)

- Corners are extracted from road curvature and graded by the radius at the tightest point:
  hairpin (< 13 m), 1 "square" (13–22) … 6 (151–220, a flat kink beyond), with "tightens" /
  "opens", "crest into" / "over crest", "long", "don't cut", "narrows", "off camber", "careful"
  (`stage::corners`, docs/RALLY_MAP.md phase 1). A planner reads notes ~4.5 s ahead (70–280 m),
  chaining close corners with "into". Damage lines and recovery hints.
- Spoken with the **OS text-to-speech** (`say` on macOS, PowerShell on Windows, espeak or
  spd-say on Linux), not through the game's audio mixer. Silent in free roam.

### 5.4 Audio (`src/audio/`, `docs/AUDIO.md`)

Engine: five steady-RPM CC0 recordings of a 4-cylinder (engine-bay and exhaust mics),
crossfaded by RPM and load, pitched to the live RPM, filtered for the cockpit. Tyres: **drift
only** (gravel skid, tarmac squeal, per surface), no rolling noise (owner's request). Pops on
lift-off and shifts, impacts and scrapes, synthesised wind and stage ambience. The car mix runs
in its own rodio stream because the engine mixer can't pitch clips live.

### 5.5 Cameras and HUD

Cockpit (default; detailed cabin, animated arms, working steering wheel and gauges), bonnet,
chase. HUD: speed and gear, rev bar, clock and progress (events/stages only), damage panel,
co-driver subtitles. Settings for each HUD group, camera shake, steering speed, stick
deadzone/linearity, assists.

---

## 6. Technical architecture

### 6.1 Engine ("Hull", `/Users/himanshu/codes/game/engine`)

A code-first engine around **Bevy 0.20.1** (Rust, ECS) and avian3d 0.8 (physics, not used by
the car). Pacenote uses its features: `look` (sky, fog, grading, quality tiers), `vfx`, `pcg`
(+ the `pcg-core` crate), `world` (terrain, streaming cells, HLOD), `net` (UDP listen server),
`sound`, `actions` (input mapping), `props`, `ktx2`, and in dev builds `devtools` (control
port, metrics viewer) and `devkit`. Also: a UI kit (menus, settings, toasts, picking-based
buttons), `render_budget` (quality governor, render scale, frame pacing, detail tiers,
pipeline warm-up). Engine docs: `engine/docs/`, `engine/CLAUDE.md`, `GAME_GUIDE.md`.

Shared by three games: Pacenote, Shroud (an FPS) and Courier (hover-bike delivery). One shared
Cargo target dir (`shroud/target`); toolchain stable 1.99 via rustup overrides.

### 6.2 Determinism

Ghosts and leaderboards require identical results on every machine. Rules: all trig/exp in the
sim and generators through `libm`; quaternion steps spelled out; no `HashMap` iteration order
in outputs; sorted obstacle lists. Golden tests: `scripted_drive_is_bit_identical`,
`stages_hash_the_same_everywhere`, `obstacles_the_same_everywhere`. Cross-CPU checks were
done for the old stages (arm64 vs x86_64 under Rosetta, revision 5). **Not yet re-checked**
after the Bevy 0.20 bump (revision 8) or for the open world (section 9.3).

### 6.3 Pacenote modules (`src/`)

| Module | What |
|---|---|
| `lib.rs`, `main.rs` | App setup, plugins, CLI (`--open`, `--stage N`, `--metrics-window`...) |
| `open/` | Open world: `roads`, `junctions`, `buildings`, `traffic`, `events`, `forest` (tree colliders), `data` (world as data), `level` (bake, load, level entities), `live` (editor rebuild), `placeable`, `OpenWorld` (Ground) |
| `world.rs` | `StageWorld` (the active world: road/route, heights, corners, obstacles, `open`, `event`), generation, terrain spawn, dressing graph |
| `stage.rs` | Old stages, `Road` type, terrain shaping, corners/crests |
| `vehicle.rs`, `handling_tests.rs` | Car sim and its test harness |
| `drive.rs` | Input → sim each fixed step, run state (`StageRun`), camera, recover |
| `loading.rs` | Loading screen and phases (generate on a worker, build, warm pipelines, settle) |
| `art/` | Terrain material (WESL shader), road meshes, trees, dressing, props, car model, spray, look |
| `audio/` | Car and ambience audio |
| `codriver/` | Pace-note planner and TTS |
| `meta/` | Menus, profile, leaderboards, ghosts, results, pause, settings |
| `guide/`, `net_session.rs`, `voice/` | Two-player driver + guide mode and voice chat (legacy stages) |
| `hud.rs`, `prefs.rs` | HUD and player preferences |
| `hardware.rs` | First-launch quality from RAM/chip; render-scale cap on Retina at Low/Medium |
| `telemetry.rs`, `demo.rs` | Anonymous stats; Steam demo gating |
| `autopilot.rs`, `capture.rs` | Dev-only autopilot and video capture |

### 6.4 Frame and data flow

`FixedUpdate` (60 Hz): read actions → keyboard steer ramp → `vehicle::step` against
`StageWorld` (Ground) → run state (clock, progress, finish, recover) → traffic step (after the
sim). `Update`: camera follow, HUD, co-driver, audio gather, events approach, meta (stage
registration, finish → results), art streaming (trees near/far, PCG cells). Render: Bevy 0.20
with the engine's render budget (governor may lower render scale or shadow resolution when the
GPU is over budget).

### 6.5 Loading

The open world is read from its level file on a worker thread (0.04 s for the file and terrain;
~0.8 s with the road network, buildings layout and the tree colliders along the roads), then
built (terrain chunks, roads, buildings, PCG volumes, trees), then render pipelines are warmed
behind the loading screen, then it settles (stable frame times) before the player gets
control. ~10 s total on an M5 Pro (measured 2 Nov), almost all of it warming and settling.

### 6.6 Dev tooling

- **Control port** (dev builds, `--control`): JSON commands over TCP; `engine/scripts/play.py`
  runs scripted sessions (keys, screenshots, ECS queries, perf reports). Many game commands
  (`art.stats`, `art.memory`, `art.textures`, `ui.click_at`, `events.*`, `meta.finish`...).
- **Metrics viewer:** a separate process (`pacenote --metrics-viewer`) reading the data dir's
  logs: live and historical runs, charts, events.
- **Session logs** (`<data>/logs/session-*.jsonl`): perf samples each second, state changes,
  governor steps, hitches. Run summaries in `<data>/metrics/sessions.jsonl`.
- Data dir on macOS: `~/Library/Application Support/pacenote/`.

### 6.7 Distribution and services

- `packaging/macos/build.sh` (universal .app, needs signing and notarisation by the owner),
  `packaging/windows/build.sh` (cargo-xwin), `packaging/steam/` (VDF, upload script, store
  assets). `docs/STEAM.md` has the launch checklist.
- Telemetry: the engine's tracer (`ashlar-trace`) to the owner's backend (api.marlstudio.com),
  consent categories, opt-out in Settings, first-launch notice. Events: `run.start`,
  `run.end`, `run.crash`, `run.recover`, `demo.wishlist`.
- Website: `web/` (Astro + GSAP + Lenis), pages generated from `web/src/data/` (stages,
  stats). Deploy target Vercel. Not updated for the open world.

---

## 7. Performance

Measured on an Apple M5 Pro (24 GB) unless noted:

| Scene | Memory (footprint) | GPU per frame | Notes |
|---|---|---|---|
| Old stage 1, High, before fixes | ~2.4 GB | 6.6 ms | 152k trees across the map, 913k far-card triangles |
| Old stage 1, Low, after fixes | ~1.9 GB | ~3–6 ms | far cards halved, Retina render scale 70 % |
| Open world, High (with buildings, traffic) | ~2.4 GB | ~6.4 ms | 40k entities, 37k meshes |
| Open world, Low, 2 Nov evening (budget tour) | ~1.34 GB | 4.3 ms p95 | streamed dressing, tree LODs, original car; 4 of 5 Low budgets pass (`docs/ASSET_PIPELINE.md`) |

Measures in place: first-launch quality from hardware (8 GB → Low; base Apple chip or 16 GB →
Medium; else High), draw distance by quality, far tree cards only near roads, render-scale cap
on Retina at Low/Medium, the engine governor (render scale, shadow size) and frame pacing.

A playtester's base MacBook (likely M1/M2 8 GB) froze the whole machine on the first build
(High, fullscreen Retina). The fixes above target that, but **no measurement on such a machine
exists**. The open world keeps ~145k trees resident and ~2.4 GB at High.

---

## 8. How we work

- **Solo owner + AI agents.** The owner gives direction and tests builds; Claude Code sessions
  implement. A lead session per game integrates on `main`; helper agents work in git worktrees
  on their own branches (with their own Cargo target dirs) and hand back for merging. One
  session owns the engine (`engine-c3` on 2 Nov); games ask it for engine changes. Sessions
  message each other; see `docs/TEAM.md`.
- **Rules:** every heavy command through `memgate` (a memory-aware job queue on the Mac); commit
  small and often; never push without the owner; no new heavy crates; don't change Bevy
  features; secrets never printed.
- **Testing:** unit tests for pure modules (sim, generators, traffic, buildings, boards, ghosts,
  audio DSP); golden hashes for determinism; integration tests (meta flow, guide net, voice, UI
  clicks with Bevy picking); scripted play sessions with screenshots and perf reports; GPU
  tests are `#[ignore]`d and run explicitly. Visual checks are done by looking at screenshots.
- **Code navigation:** the `graft` index (`graft/`) for symbol and call-graph queries.

---

## 9. Known issues and risks (most important first)

### 9.1 Legal / IP (blocker for a commercial release)

- ~~The player car was a branded Sketchfab GT3 model~~: replaced (2 Nov 2026) by the
  original Talus rally hatch (`docs/cars/rally_hatch.md`). Still to do: the Steam store
  screenshots in `packaging/steam/store-assets/screenshots/` show the old car and must be
  retaken.
- The genre pivot must stay original (no recognisable map, festival theme, UI or names from
  any existing open-world racing game).

### 9.2 Product and identity

- The game's hook (two people, one reading pace notes) is now confined to events, and the
  two-player mode doesn't work in the open world yet. Free roam has nothing to do but drive.
- No map or minimap: events can only be found by driving around. No progression, unlocks,
  cars to choose, or reason to return besides records.
- The 20 old stages are hidden but still in the code, docs, website, demo config
  (`demo::STAGES = [1, 6, 9]`) and store copy.

### 9.3 Correctness and determinism

- Cross-CPU determinism (arm64 vs x86_64) **not re-verified** after the Bevy 0.20 / glam 0.33
  bump (sim revision 8), nor for the open world generator, buildings or events. Leaderboards
  and ghosts shared between Mac and Windows depend on it.
- Event records and ghosts are keyed by the route and the level file's content (level +
  terrain), so any edit to the level starts fresh boards, even one far from the route.
- The terrain is shaped by the nearest road only, so seams are possible where roads meet at
  angles; where roads join the ring they sit up to 18 cm off the ground (`open.check`).
- The generated ring had a 0.6 m step in height where its ends meet; the baked curve smooths
  it, so the baked ring differs from the generated one there.
- Moving a road in the editor re-shapes the ground around it from the generator's hills,
  which also undoes any sculpting there. Buildings don't move with a moved street (one left
  overlapping a road only logs a warning).
- Pacenote `main` builds against the engine's `editor` branch (level, spline, editor
  features) until that branch reaches engine main; on engine main it doesn't build.
- Building collision is approximated with cylinders (good to ~0.15 m; corners are rounded).

### 9.4 Gameplay limits

- Traffic: no overtaking (queues behind a slow player), one lane each way on the highway,
  tight U-turns at dead ends, junction paths ignore height, no pedestrians.
- Event start only by stopping in a marker; no restart-from-world prompt, no abandon option in
  the pause menu for events (Restart restarts the event; Quit to menu leaves the world).
- The co-driver voice uses OS TTS: different voices per OS, ~0.3–1 s latency on Windows, not
  in the game's audio mix or volume settings.
- Recover always places on the nearest road, which can be a different road at junctions.
- No map or minimap: the only way to find events is to drive around. The level already holds
  everything one needs (roads, buildings, event starts, home).

### 9.5 Performance and platforms

- Base MacBook (8 GB) performance unmeasured; Windows build untested on real hardware since the
  pivot; the shipped demo build predates the open world.
- Whole-map trees resident (~238k) and ~2.4 GB at High; no world streaming of scenery beyond
  the existing PCG cells and tree near/far LOD; the engine's world streaming mode isn't used.
- Car model is 124k triangles with 2048² textures; the GLB is 21 MB.

### 9.6 Tooling

- Editor (engine `editor` branch, 3 Nov): between engine `3fa9765` and `53614ee` the viewport
  froze on one view when the editor adopted the game's camera; fixed in `53614ee`.
- Metrics viewer: until engine `8ab2de8` (2 Nov) the per-second metrics lines carried the core
  frame/CPU/GPU/memory keys only once, so those panels froze; fixed, with a regression test.
- Several Bevy 0.20 regressions were found only by running the game (blank window from a camera
  race, unclickable menus, render-scale freeze, MSAA validation crash). They're fixed, but most
  were not caught by tests; the GPU/UI tests are `#[ignore]`d and run manually.

---

## 10. Questions for auditors

1. **Concept:** what should unify "open-world driving" with Pacenote's pace-note / two-person
   identity so the game has a clear hook and isn't a generic free-roam driver? (e.g. events
   that need a reader, pace-note challenges, co-op routes, a radio-guided navigator mode)
2. **Scope:** is a compact 4 x 4 km world with 7 events enough for a demo that builds
   wishlists? What's the minimum content and progression for a paid release?
3. **Priorities:** given section 9, what order would you tackle: IP-safe car, map/minimap,
   multiplayer in the world, performance on base hardware, cross-CPU determinism, store/site
   update?
4. **Architecture:** is `StageWorld` (one active route plus an optional open world) the right
   abstraction, or should the world and the active event be separated now, before multiplayer?
5. **Traffic and AI:** is the lane-graph + IDM design a good base for racing opponents later?
6. **Performance:** what should the target hardware and budget be (e.g. M1 8 GB at 60 fps
   Low), and which systems should stream?
7. **Process:** where are tests missing (we found several Bevy 0.20 regressions only by playing)?
8. **Telemetry and marketing:** are the collected events the right ones to support Reddit posts
   and to tune the open world?

---

## 11. Roadmap (current intent)

Phase 1 (compact world), done: road network and terrain, free roam, buildings, traffic, events,
the world as a level file with a working editor (roads, buildings, events, trees, terrain).
Remaining phase 1: a map/minimap (from the level's roads, buildings and event starts),
performance pass on Low/Medium on a base Mac, polish of town dressing (parks, street props,
markings).

Phase 2: radio voice chat and racing with friends in one world (multiplayer in the open world),
drift zones, the larger ~10 x 10 km region, racing opponents (maybe), an original car.

Release track: replace the car (IP), rebuild the Steam demo around the open world, update
website and store copy, verify Windows and base-Mac performance, sign and notarise.

---

## 12. Build, run, test

```sh
cd /Users/himanshu/codes/game/pacenote
memgate -- cargo run                       # dev build, opens the menu (+ metrics viewer)
memgate -- cargo run -- --open             # straight into the open world
memgate -- cargo run -- --stage 3          # an old stage
memgate -- cargo test -q                   # all tests
memgate -- cargo test -q --test ui_click -- --ignored   # GPU UI click tests
PACENOTE_MAP_SVG=/tmp/net.svg memgate -- cargo test -q --lib open::roads::map -- --ignored   # road map
# Scripted session (from the engine dir): keys, screenshots, ECS queries, perf
python3 -I /Users/himanshu/codes/game/engine/scripts/play.py steps.json \
  --manifest /Users/himanshu/codes/game/pacenote/Cargo.toml --bin pacenote --out /tmp/run -- --open
```

Other documents: `HANDOFF.md` (state and conventions), `docs/DESIGN.md` (original design),
`docs/OPENWORLD.md` (open-world plan), `docs/HANDLING.md`, `docs/AUDIO.md`, `docs/META.md`,
`docs/NET.md`, `docs/VOICE.md`, `docs/ART.md`, `docs/TELEMETRY.md`, `docs/STEAM.md`,
`docs/TEAM.md`.
