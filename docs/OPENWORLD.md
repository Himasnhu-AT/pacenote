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
