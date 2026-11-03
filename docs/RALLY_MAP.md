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

## Status

- Phase 1: in progress.
- Phase 2: in progress.
- Phase 3: in progress.
