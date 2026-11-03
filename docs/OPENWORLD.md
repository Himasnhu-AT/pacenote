# Open world

Pacenote turns from 20 separate rally stages into one open world you drive around freely, with
traffic, a town with buildings, and events you drive into. Original design in the open-world
racing genre; it borrows no map, cars, branding or festival from any existing game.

## Decisions (owner, 2 Nov 2026)

- **No stage select.** The game starts in the world. The 20 stage maps go away as a menu.
- **Events you drive to**: markers in the world start a race route along the roads (point to
  point with checkpoints, time trial). Results and records like today's stages.
- **No co-driver in the car.** During an event an AI voice reads the pace notes for the route
  (today's co-driver voice and planner). Free roam has no notes. Friends racing together will
  talk over a radio (voice chat; phase 2), replacing today's driver + guide mode.
- **Traffic**: AI cars drive the roads (lanes, giving way, slowing behind each other and you).
  No racing AI yet.
- **Compact first**: about 4 x 4 km, to measure the engine and `pcg-core` on Bevy 0.20 on a
  base MacBook. A larger region (~10 x 10 km) comes after the numbers are in.
- **Sound**: engine and drift only (no road noise), done.

## How it fits the code

The car sim only sees the world through `vehicle::Ground` (height, normal, surface,
obstacles), so the open world is a new `Ground`; the car, damage, physics and HUD stay.

- `src/open/` (new): the world model, pure and deterministic like `stage.rs`.
  - `RoadNet`: every road as a `stage::Road` (points every 2 m, width, surface per point) plus
    the junctions between them, and a segment grid for "nearest road" queries.
  - Kinds: town streets (grid, tarmac, 11 m), a highway ring (tarmac, 14 m), country roads
    from town to the ring (tarmac/gravel, 8 m), forest roads off the ring (gravel, 7 m, rally
    style: the event roads).
  - Terrain: one heightfield (2 m) over the region, shaped so every road sits in the land
    (today's cut/fill rules, applied for the nearest road), flat in town.
  - `OpenWorld: Ground`: height/normal from the heightfield, surface from the nearest road
    (else grass/forest floor), obstacles from a grid (trunks, boulders, buildings, traffic).
- Rendering reuses today's art: road ribbons per road, the engine terrain with the splat
  layers, PCG dressing and trees kept off roads and lots, streamed by distance.
- Buildings: procedural, per town lot (houses, shops, blocks); boxes with facade materials and
  a few roof shapes; collision as box obstacles.
- Traffic: kinematic cars following lanes on the road graph (no full sim each), visible and
  solid near the player, despawned far away.
- Events: a route = a list of road pieces; its own `Road` gives corners and crests, so the
  pace-note planner and the results/records flow carry over.

## The level file and the editor

Needs the engine folder on the engine's `editor` branch (level, spline and editor features)
until that branch is merged to engine main.

The world is a level: `assets/levels/open.level.ron` plus its terrain in `assets/levels/open/`
(`terrain.ron` + `terrain.r16`, 2001² at 2 m). The game reads it at start (`open::level::load`,
0.04 s; the open world builds in under a second), or the file `--level` names (an open-world
level starts the open world, no `--open` needed); a missing or bad level logs an error and the
world is generated from the seed instead.

What the level holds (`src/open/level.rs`):

| Entity | Components | Is |
|---|---|---|
| `road N (kind)` | `Spline`, `RoadSpline { kind, speed_kmh }` (kinds `Street`, `BRoad`, `Gravel`, `Forest`) | a road. Control points where needed (every station within 10 cm sideways, 3 cm in height of the curve); `tags` = surface + kind (`data::tags`), `bank` = roll in radians. Loading samples it every 2 m. Roads are numbered in stable-id order (events and grades use the number). |
| `open world` | `WorldDoc { data, terrain }` | seed, town height, time of day, events (road, start/finish station), town lots; the terrain folder |
| `home`, `event start: …`, grade notes | `HomePoint`, `EventStart`, `GradeNote` | the placeables (`placeable.rs`): moved in the editor, snapped to the road on load |
| `house 12`, `shop 40`, … | `Building` | one building each (110): kind, size, floors, storey height, roof, colours; its Transform places it. The game builds its mesh as a child |
| `Trees`, `Dressing` | `Foliage`, `PcgOverrides`, `PcgFollowTerrain` | hand edits of the trees and of the rocks / bushes; the game puts the `PcgVolume` on them |

Edit it with the game's own binary: `cargo run -- --editor --level assets/levels/open.level.ron`
(the `dev` feature includes the engine's `editor`). Save writes the level and, if sculpted or
painted, the terrain files (only what changed). Play runs the game on a copy of the level
(`--level-origin` tells it where the terrain is).

What reacts to an edit (`open::live`, 0.3 s after the last change, ~0.7 s):
- **Roads** (Spline tool): the road, its junctions and verges, the ground around the part
  that moved (re-shaped from the generator's hills; sculpts elsewhere stay), trees kept off
  it, traffic lanes, event finishes on it. Undo goes back the same way.
- **Event starts, home, grade notes** (Move / Place): the event's route and marker.
- **Buildings** (Details, Move / Rotate, Delete, Place): the building's mesh at once; the car's
  walls and where trees grow.
- **Trees, rocks, bushes** (Foliage tool) and **terrain** (Sculpt / Paint): at once; trees
  follow the ground. The car's tree colliders apply the edits when the world loads.

Records and ghosts of an event are keyed by its route and the level's content (file +
terrain), so an edited level starts fresh boards. Dev command `open.check` lists roads more
than 5 cm off the live ground.

Re-bake (throws away every edit): `cargo run -- --bake-world assets/levels/open.level.ron`.

## Since 3 Nov 2026: terrain first

The world described above (town grid, ring, spokes) was replaced by a terrain-first sim-rally
map: eroded land with a river valley, escarpment, basin and plateau, roads planned across it by
an anisotropic A* (switchbacks, 8-10 m hairpins, rally widths), a valley village and service
park, traffic on the tarmac only. See `docs/RALLY_MAP.md` (plan, checklist) and section 4 of
`docs/GAME_OVERVIEW.md` (the map as it is).

## Phases

Phase 1 (compact world, in this order, each step playable and committed):
1. `RoadNet` + terrain + `Ground`, tested (deterministic, connected, roads flat across,
   no unwanted crossings).
2. Free roam: the menu's Play loads the world; spawn in town; recover puts you on the nearest
   road; HUD without stage clock. Stage select removed.
3. Scenery: road meshes, terrain splat, trees and dressing kept off roads and town.
4. Buildings in town.
5. Traffic.
6. Events with AI pace notes; results and records.
7. Performance pass on Low/Medium/High with the metrics viewer.

Phase 2: radio voice chat and racing with friends in one world, drift zones, the larger region.
