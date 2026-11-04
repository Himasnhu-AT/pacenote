# Rally map: terrain first, sim-rally roads

Owner's goal (3 Nov 2026): the open world should drive like a hardcore sim-rally venue, not
an arcade hub. The current map was built roads first (a flat 7x7 town grid in a circular
highway ring, forest roads from a steered random walk with a 38 m tightest radius, 91 m of
relief), so it has no hairpins, no topographic purpose and little vertical drama. This file is
the plan for the overhaul and records what each phase changed.

## Target

| | Before | Target |
|---|---|---|
| Layout | 7x7 town grid, highway ring, 4 dead-end forest spokes | river valley village + service park, B-road along the river, gravel basin, forestry escarpment, alpine plateau; every road ends at a road or a turnaround loop |
| Relief | 91 m fBm | 380-450 m: domain-warped ridges + droplet hydraulic erosion (gullies, spurs, fans) |
| Widths | 14 / 11 / 8 / 5.4-6.6 m | B-road 6-7 m, fast gravel 4.2-5 m, forest 3.0-3.8 m |
| Corners | tightest ~38 m | hairpins 7-12 m, squares 13-22 m, up to 200 m+ kinks |
| Road placement | random walk, terrain bent to the road | anisotropic A* on a 10 m grid: grade (4% target, 14% max), cross-slope and turning costs, so switchbacks appear where the land is steep |
| Cross-section | flat ribbon, 3 m verge, 34-52 m gentle blend | crown (+6-10 cm), banking mix (60% slight +, 25% flat, 15% off-camber), cut-side V ditch (0.25-0.45 m deep, 0.8-1.2 m wide), fill-side 0.5 m shoulder then a steep bank |
| Profile | 40 m-smoothed, laser-flat straights | micro-undulation (12-35 m, ±0.15-0.45 m) on gravel/forest, sharp crests kept (R_vert < 60 m where the land has a spur) |
| Grip across the road | road / verge / grass | wheel tracks (full grip, 2 cm ruts), crown and outside marbles (82-88%), ditch (drag), dusty tarmac 40 m after a gravel junction (-15-20%) |
| Walls | rows of 1.2 m cylinders | line-segment walls with height (buildings, parapets, barriers); cylinders stay for trees, posts, rocks |
| Ecology | uniform forest | by elevation, slope and moisture: valley farmland and hedges, plantation blocks with clear-cuts (stumps, timber piles), alpine rock, heather and snow poles |
| Events | one road each, 1-3 km | chains of road pieces across junctions, 4-15 km, mixed surfaces, unused junction branches taped off |
| Pace notes | grade by radius, crest flag | hairpin + grades 1-6 on the target radius table, tightens / opens, crest combinations, narrows, off-camber, don't cut |

Choice: the terrain stays at 2 m (2001², the engine terrain and memory on a base MacBook). The
ditch, crown and ruts are in the car's ground (an exact profile near roads) and in the road
mesh, not the heightfield.

## Phases

Each phase is playable, tested and merged to main on its own.

1. **Pace-note grades and analyzer**: the radius table, hairpins, tightens/opens, crest
   combinations, narrows, off-camber and don't-cut calls (`stage::corners`, `codriver`).
2. **Wall collision**: a line-segment obstacle with height in `vehicle`, used for building
   walls (then parapets and barriers).
3. **Terrain and road network**: eroded terrain with zones, POIs, A* roads with the new widths
   and kinds, valley village (80-120 buildings) and service park, no dead ends. Re-baked level.
4. **Road cross-section and grip**: crown, banking mix, ditches, embankments, undulation,
   lateral grip zones, dusty tarmac.
5. **Events as chains**: multi-road routes, 4-15 km, tape on junction branches.
6. **Ecology and road dressing**: biome rules from elevation/slope/moisture, clear-cuts,
   alpine zone, chevrons, marshal and finish boards, culverts and rocks in ditches.

## Checklist

Ticked as each step lands on main (commit in brackets).

### Phase 1: pace-note grades and analyzer
- [x] 1.1 Radius table: hairpin 7-12, 1 13-22, 2 23-35, 3 36-55, 4 56-90, 5 91-150, 6 151-220 m
- [x] 1.2 Tightens / opens within one turn
- [x] 1.3 Crest + corner ("over crest", "crest into")
- [x] 1.4 Narrows (> 1 m within 40 m)
- [x] 1.5 Off-camber from bank against the turn
- [x] 1.6 Don't cut (per-station hazard on `Road`, set by phase 6)
- [x] 1.7 Co-driver phrases; tests on synthetic roads

### Phase 2: wall collision
- [x] 2.1 Line-segment wall with top height in the car sim
- [x] 2.2 Normal impulse + tangential friction (scrape, no snag), damage, segment ends, passing over
- [x] 2.3 Building walls become segments (generated and level buildings, live edits)
- [x] 2.4 Tests: 110 km/h 4° brush keeps speed, 90° hit stops, end = post, above = passes

### Phase 3: terrain and road network
- [x] 3.1 Terrain: zones (valley 80-120 m, basin 140-240, escarpment 180-380, plateau 380-480), river valley, domain-warped ridges
- [x] 3.2 Droplet hydraulic erosion (bake time, deterministic) + moisture map
- [x] 3.3 Points of interest: village, service park, logging camp, quarry, radio tower, river crossings
- [x] 3.4 Anisotropic A* (10 m grid, headings; grade 4% target / 14% max, cross-slope, turning) with switchbacks
- [x] 3.5 Path to centreline: smoothing that keeps 7-12 m hairpins; road heights from the land with grade limit, crests kept
- [x] 3.6 Road kinds and widths: village lane, B-road 6-7 m, fast gravel 4.2-5 m, forest 3.0-3.8 m; no dead ends (turnaround loops)
- [x] 3.7 Network: B-road along the river, tarmac hillclimb, gravel basin loop, forestry escarpment, mountain pass, logging loops
- [x] 3.8 Valley village (80-120 buildings along the roads) and service park yard
- [x] 3.9 Traffic on the new kinds (B-road and village only), spawn in the service park
- [x] 3.10 Re-bake the level, editor live-edit still works (base terrain kept for reshaping), docs

### Phase 4: cross-section and grip
- [x] 4.1 Crown (+6-10 cm) in the road profile, mesh and car ground
- [x] 4.2 Banking mix: 60% +2-4°, 25% flat, 15% off-camber -2-5° (flagged)
- [x] 4.3 Cut-side V ditch (0.25-0.45 m deep, 0.8-1.2 m wide), exact in the car's ground, drawn in the road mesh
- [x] 4.4 Fill side: 0.5 m shoulder, then a 1.5-4 m steep bank where the land falls > 20%
- [x] 4.5 Micro-undulation (12-35 m, ±0.15-0.45 m) on gravel and forest
- [x] 4.6 Lateral grip: wheel tracks (2 cm ruts), crown/outside marbles 82-88%, ditch drag
- [x] 4.7 Dusty tarmac for 40 m after a gravel junction

### Phase 5: events as chains
- [x] 5.1 Event = ordered road pieces across junctions (data, level file, editor)
- [x] 5.2 4-15 km stages with mixed surfaces, flying finish onto a liaison road
- [x] 5.3 Pace notes, ghosts and records over the chained route
- [x] 5.4 Tape and marshal barriers across unused junction branches during an event

### Phase 6: ecology and road dressing
- [x] 6.1 Biomes by elevation, slope and moisture (valley farmland/hedges/stone walls, plantations, alpine rock/heather)
- [x] 6.2 Clear-cuts 150-300 m: stumps (0.3 m solids), timber piles, sightlines
- [x] 6.3 Snow poles on the alpine edge
- [x] 6.4 Chevrons on deceptive corners, marshal posts, 100 m finish boards
- [x] 6.5 Culverts / rocks in inside ditches, setting don't-cut
- [x] 6.6 Brake and scuff marks before grade 1-3 corners
- [x] 6.7 Bridges with stone parapets (wall segments) at river crossings

## Status

- **Festival world pass (5 Nov 2026,** `docs/festival/WORLD_PASS.md`**):** the land smoothed
  (T1-T11: no scarps, rounded ridges, a 75 % talus outside a few crags, corridor shaping with
  cut ≤ 60 %, an adaptive blend and two-road blending), the valley village became the town of
  Fernhollow (13 streets as roads 8-20, 654 buildings in all, drawn merged per tile), three
  jumps, the level re-baked (21 roads, 5 events 4.4-10.7 km). The phase notes below describe
  the 3 Nov state.

- Phase 1: done. `stage::grade_for` holds the radius table; the hairpin is grade 0
  (`stage::HAIRPIN`), so world data went to version 2 and a version 1 grade override of 1 is
  migrated to 0. `Corner` gained `crest_into`, `tightens_from`, `opens_to`, `off_camber` and
  `dont_cut`. Calls read like "hairpin left", "square right", "three left tightens one",
  "crest into two right" and "four right over crest, don't cut". The six new words were
  rendered with `cook_voice.py --only`. `Road::dont_cut` (per station: a hazard on the inside
  edge) is all false until phase 6 sets it from culverts and rocks; event routes carry it over.
  Fixed on the way: `Road::from_centreline` had its curvature sign backwards (+ was right), so
  open-world calls were mirrored and its default banking leaned out of corners.
- Phase 2: done for buildings. `vehicle::Wall` segments (`Ground::walls`, `open::walls::WallGrid`); facades are one wall per side, rows of shops joined. A 4° brush at 110 km/h keeps 97 % of its speed. Parapets and barriers still to add.
- Phase 3: done. `open::land` (zones, river, gully, ridged noise, 600k-droplet erosion, moisture),
  `open::plan` (places, anisotropic A* with soft grade limit, arrival heading, untangling),
  `open::roads` rewritten (kinds `Street`/`BRoad`/`Gravel`/`Forest`, 9 roads, ~29.5 km, no dead
  ends, tightest hairpin 8.1 m), valley village + service park (110 buildings), traffic on the
  tarmac only, level re-baked. The logging *loop* became a camp-to-hillclimb track (a loop on
  the escarpment kept crossing its own road).
- Phase 4: done. `stage::Road` has a crown and worn wheel tracks per station (in `height_at`,
  so the mesh and the car agree); `open::profile` dresses each road (crown by kind, less in
  bends; a V ditch 0.25-0.45 m deep on the side cut into the hill, none near junctions; dusty
  tarmac 40 m round gravel junctions) and gives the car the exact cross-section within a few
  metres of a road (shoulder, ditch, blend into the terrain); the 2 m terrain is shaped to
  match, with a steep drop-off where the land falls away > 20 %. Corners bank 60 / 25 / 15 %
  (in / flat / off-camber, measured 16 % off-camber, 30 % flat); gravel and forest roads
  undulate (25-35 m, up to 0.2 m). New surfaces `Loose` (marbles: crown, outside of bends,
  beyond the tracks; 85 % of gravel's grip), `Ditch` (drag: a car coasting from 40 km/h with two
  wheels in keeps 22 km/h after 2 s vs 26 on the road) and `Dusty` (82 % of tarmac).
- Phase 5: done. An event's route is a chain of pieces (`EventData` keeps its first piece in
  `road`/`from`/`to` and the rest in `then: Vec<Leg>`; a piece runs backwards when `from > to`;
  old level files read as one piece). Pieces must meet within 3 m; the route rounds the turn at
  each joint, turns banking round on a piece driven backwards, and `route_s` finds a station on
  any piece (grade notes). Five stages: Fernhollow escarpment (rally, 6.9 km, forest), Blackpine
  marathon (rally, 12.8 km: tarmac hillclimb, plateau gravel, basin descent), Quarry loop
  (5.2 km, tarmac and gravel), River crossings (2.8 km, two river crossings), Hillclimb
  (5.2 km tarmac). While one runs, red and white tape closes every junction branch the route
  doesn't take (`events::closed_arms`); it goes when you leave. Visual only (no collision).
- Phase 6: done. `open::ecology` masks the land for the dressing graph: no trees above the
  treeline (~410 m, or high and steep: 3 km², stunted trees and scree instead), on the flat
  valley fields (2.5 km², single trees) or in 7 clear-cuts (110-200 x 150-300 m, beside gravel
  and forest roads, full of solid 0.3 m stumps, a timber pile at the road edge). Bridges where
  the southern loop crosses the river: level deck, 3.5 m wide (the notes say "narrows"), stone
  parapets (walls). Culverts in the inside ditch of tight corners (47; their notes say "don't
  cut"; solid). Snow poles every 25 m on the plateau roads (228), chevron boards on the outside
  of tight corners after a fast run or over a crest (78), 100 m and finish boards and a marshal
  post at every event's finish, brake marks in the wheel tracks before hairpin-to-3 corners
  (`art::roadside`, `art::ground::brake_wear`).
  Not done: species by zone (birch in the valley, plantation rows), hedgerows and stone walls,
  breakable snow poles (they're drawn, not solid).
