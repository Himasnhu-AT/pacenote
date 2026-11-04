# World pass: smooth mountains, a real town, roadside furniture, challenge spots

Level designer + world builder, 4 Nov 2026, autonomous run (no owner questions; each decision has
a one-line reason). Read with `docs/RALLY_MAP.md`, `docs/GAME_OVERVIEW.md` §4 and
`docs/festival/GAME_SYSTEMS.md` §3 (challenge rules). Spec only: no code changed here.

Owner asks: (1) houses are sparse, so make a real town with outskirts and rare mountain huts;
(2) mountains have sudden steeps or none at all, so make them smooth and structured; (3) road
conditions, signs, barriers on mountain curves; (4) spots for speed traps, speed zones, drift
zones and jumps.

All numbers marked *measured* come from the committed bake (`assets/levels/open/terrain.r16`,
2001² at 2 m, slope = central difference over 4 m) and the road splines in
`assets/levels/open.level.ron`, sampled every 2 m.

## 1. Diagnosis: why the land is cliff or flat

**Measured slope histogram (share of map area):**

| slope | <10 % | 10-20 | 20-30 | 30-45 | 45-70 | 70-100 | 100-150 | >150 % |
|---|---|---|---|---|---|---|---|---|
| now | 40.8 | 20.9 | 11.3 | 10.1 | 8.7 | 4.6 | 2.5 | 1.07 |

- Mean slope by height band: 80-100 m 5 %, 100-120 m 12 %, 120-180 m 27 %, then **40 % → 73 %
  between 180 and 320 m**, back down to 16-30 % on the plateau (400-480 m). Within 280-320 m, 38 %
  of the cells are steeper than 70 %.
- The land is bimodal. The valley floor plus the plateau make up about 52 % of the map. The
  180-340 m band in between is only about 14 %: the climb is squeezed into narrow walls.
- Cells steeper than 200 % (12.9k samples) peak at a final height of **300-350 m**. Neighbouring
  2 m samples that differ by more than 3 m number about 24.6k and peak at the same heights.
- Roads aren't the main cause. The share of slopes over 70 % is about the same at 4-12 m from a
  road (4.2 %) as at 60-90 m (5.6 %).

**Causes, from the code (most important first):**

1. **Amplitude jump at a macro height of 360 m** (`src/open/land.rs:190-194`). Below 360 m the
   ridge amplitude is `lerp(140, 50, tw)`: 140 m in the west, 50 m in the east. Above 360 m it is
   `lerp(70, 45, …)`, which starts at 70 m. The amplitude therefore jumps from 140 to 70 m in the
   west (and from 50 to 70 m in the east) across the macro contour. With `ridged` in −0.75…0.85
   (l.188) and `rolling·0.4` added, the result is a **step of up to ±60 m within one 4 m cell**
   along that contour. It is a scarp, or a fold where the land drops as the macro rises. This
   matches the measured peak of cliffs at final heights of 300-350 m.
2. **Knife-edge ridges** (`land.rs:180-181`). `1 - |noise|` has a slope discontinuity at every
   zero crossing. `r*r*weight` over 6 octaves (down to about 24 m wavelength) at 140 m amplitude
   makes creases with 30-60 % slopes flipping sign within a few metres.
3. **Zone-edge warp folds the ramps** (`land.rs:165`). A 420 m warp at a 1400 m scale has a
   Jacobian down to about 0.4, so the east wall (`l.170`: 245 m over `smoothstep(300, 1200)`, peak
   61 %) can shrink to about 360 m wide, which peaks near 100 %. The west escarpment (`l.168`) is
   gentler but still shrinks.
4. **Erosion artefacts** (`land.rs:219-318, 356-369`). There are only 600k droplets on a 1001²
   grid (0.6 per cell), so erosion barely softens causes 1-3. Deposits land on 4 corners with no
   brush (l.289-296), which leaves spikes and pits. The hard clamp `h.max(before - 30)` (l.364)
   leaves flat-floored trenches with creased walls. Nothing smooths the result afterwards; the
   only blur is on moisture (l.387).
5. **Flat by design, with no relief** (`land.rs:193, 205-206`). The valley amplitude is 12 m and
   `h.max(floor)` flattens the floor, which gives the measured 5 % mean over 32 % of the map. The
   basin is a 185 m shelf (`l.170`). That is the "no steeps at all" half of the complaint.
6. **Bilinear sampling from 4 m into 2 m** (`land.rs:57-65`) leaves crease lines in the normals
   (faceted lighting on the slopes). It is cosmetic but visible.
7. **Road corridor shaping** (`src/open/profile.rs:219-235`, `src/open/roads.rs:583-595`):
   - The fill switches from 16 % to 90 % as the land's fall goes from 0.2 to 0.3. It is evaluated
     per point, so it flickers along a road.
   - The cut is held at ≤ 70 % (`CUT_GRADE`), then blends back to the land over a fixed 30-46 m
     (`REACH`). On a 100 % hillside that leaves about 22 m to lose in 16 m, which is a 140 % band.
   - Only the *nearest* road shapes a point. That makes a crease on the line halfway between
     stacked switchback legs.
   - §4.2-4.3 of the docs say "cut ≤ 45 %, 34-52 m", but the code has 70 % and 30-46 m. Fix the
     docs.
8. **Ruled out: the 16-bit heights.** The steps are 6.2 mm over 405 m (`terrain.ron`).

## 2. Terrain fix spec (`land.rs`, `profile.rs`, `roads.rs`)

All passes are fixed-order, `libm`, with a double buffer (Jacobi), so the bake stays bit-identical.
They run at bake time only: the game loads the level.

| # | Change | Parameters | Why |
|---|---|---|---|
| T1 | Continuous amplitude | `amp = lerp(amp_low, amp_high, smoothstep(320, 400, h))`, where `amp_low = lerp(lerp(110, 50, tw), 12, 1 - smoothstep(110, 190, h))` and `amp_high = lerp(70, 45, smoothstep(360, 450, h))` | Removes the ±60 m scarp (cause 1). West 140 → 110 m tames the walls. |
| T2 | Rounded ridges | `r = 1 - sqrt(n² + 0.015)` (rounding radius ε≈0.12), 6 → 5 octaves, `weight` clamp 0..0.9 | Crests rounded to R ≥ ~40 m (cause 2). |
| T3 | Crags only where intended | A **crag mask**: the plateau rim band (macro 370-420 m where the macro slope > 25 %, ≤ 15 % of the rim) plus the quarry face. Inside it keep `abs` ridges and talus 1.1. | Cliffs read as places, not noise. |
| T4 | Gentler zone warp | Warp 420 → 260 m at scale 1400 → 1800 m. East wall `smoothstep(300, 1200)` → `(150, 1350)` (peak 33 %). | Ramps keep their width (cause 3). |
| T5 | Macro pre-blur | Gaussian σ = 6 m (1.5 cells, radius 5) on the macro grid before erosion | Kills per-cell creases; erosion puts structure back. |
| T6 | Erosion | Droplets 600k → 1.2M. `LIFETIME` 70 → 90. Deposit through a radius-2 brush. Soft clamp `h = before - 35·(1 - e^(-cut/35))`. About +1.5 s at bake. | Real gullies and spurs, no pits or trenches (cause 4). |
| T7 | Thermal pass | 30 Jacobi iterations on the 4 m grid. 8 neighbours. Talus `T = 0.75` (37°), or 1.1 in the crag mask. Move `0.5·(Δ - T·d)` split by excess. | Hard cap on slope, outside the crag mask only. |
| T8 | Valley relief | Two river terraces (a 2.5-3.5 m scarp at 30 %, 180-220 m and 380-450 m from the river). Valley amplitude 12 → 20 m beyond 900 m from the town centre. The **town mask** (§3) blends 70 % toward a σ = 30 m blur (≤ 6 %). | Gentle structure on the flat, flat lots in town (cause 5). |
| T9 | Bicubic sampling | `Land::height`/`coarse` use Catmull-Rom on the 4 m grid | Smooth normals (cause 6). |
| T10 | Softer corridor | (a) Precompute `fall` per station in `profile::dress`, averaged over ±10 m, and widen the switch `/0.1` → `/0.25`. (b) Adaptive blend: `REACH.1 = 30 + clamp(|shaped - base| / 0.6, 16, 80)`, so the blend adds ≤ 60 %. (c) Two-road blend: if the second-nearest road is within REACH and `|lat2| - |lat1| < 16 m`, use `lerp(shaped2, shaped1, 0.5 + 0.5·smoothstep(0, 16, |lat2| - |lat1|))`. | No flicker, no 140 % band, no seam between switchbacks (cause 7). |
| T11 | Final 2 m smoothing | 8 thermal iterations, talus 1.0, on the shaped heightfield, frozen within `half width + 12 m` of every centreline | Removes leftover corridor steps; roads stay exact. |

**Keeping roads valid.**
- The A* replans on the new land. The `grades_and_hairpins` test keeps grade ≤ 11/13/14 % (tarmac,
  gravel, forest) and hairpins ≥ 7 m.
- *Measured*: today's splines already reach 16-22 % over 20 m windows, near junction pins. Note
  this, don't hide it.
- The events need roads 0-4 to keep their kinds and order (`events.rs:167-172`). Every new road
  (the town streets) is appended at index 9 and up.
- Retune `land::tests::zones_span_the_target_heights`. Relief stays 360-480 m. Lower the valley
  point bound to 65 m if the terraces cut it.

## 3. Town spec: Fernhollow (canon level: Provisional)

Lore (Provisional, visible): Fernhollow is a mill town on the river's north bank. The escarpment
and its forestry road take their name from it. Its timber comes down the forestry road (road 4),
which is the town's north-south spine. *Consistency check:* the "Fernhollow escarpment" event
already exists, so nothing contradicts this.

**Site (measured):** x −680…+40, z −1175…−860, 91-110 m high, median slope 4 % (90th percentile 9 %).
The river is only 60-80 m south of the B-road here (z ≈ −1210…−1230), so the town grows north.
Above z −850 the slope is 11 % median: that is outskirts.

```
 z −860  ┌─ Upper Road ───────────────────┬────────────┐   outskirts climb north
         │  terraces / semis  │ road 4 │   apartments  │
 z −1000 West ─ Mill Street ──┼─(spine)┼─────────── East Lane
 Lane    │  terraces          │ church │   terraces    │
 z −1080 ├─ Church Street ────┼────────┼──── square ───┤
         │  shops 2-4 fl.     │ market │   shops       │
 z −1150 ══════ Main Street = B-road (road 0, 50 km/h) ══════
         riverside row (one deep, backs on the river) ~~~ river ~~~
        x −680                x −285                  x +40
```

**New streets** (kind `Street`, 5.0 m, appended as roads 9+, all T-junctions):
- Built from contour-following polylines through a new `Builder::street(pts, from, to)`: resample,
  `road_heights` at 8 %, Chaikin ×2. No A*.
- Reason for T-junctions only: `find_junctions`/junction meshes are proven on T and Y joins. A
  4-arm crossing risks the "no unwanted crossings" rule.

| Street | Course | Length |
|---|---|---|
| Church Street | z ≈ −1080, West Lane → East Lane | ~700 m |
| Mill Street | z ≈ −1000, West Lane → East Lane | ~680 m |
| Upper Road | z ≈ −900, along the contour (±15 m), West Lane → East Lane | ~620 m |
| West Lane / East Lane | x ≈ −680 / +40, Main St → Upper Road | ~300 m each |
| 6 staggered cross lanes | x ≈ −560, −420, −160, −60 between Main/Church/Mill, offset ≥ 25 m from the next row's (no X) | 70-90 m each |
| Existing village lane (road 8) | Kept; folds into the grid between Church St and Main St | — |

Total: about 3.3 km of new street and about 24 new junctions. Every street ends on another, so the
no-dead-ends test holds.

**Density targets** (test: count building centres per zone polygon):

| Zone | Buildings | Kinds and mix | Lot / footprint | Setback | Floors |
|---|---|---|---|---|---|
| Core (town mask) | **380-520** | Shops 30 % (Main St, square), **terraces** 50 % (new `Use::Terrace`, joined rows), apartments 12 % (East Lane end), civic 2-3 (church with steeple, town hall: new `Use::Civic`), mill 1 | Terrace frontage 5.5-7.5 m × depth 9-12 m. Shop 7-12 × 12-16 m. Apartment 18-30 × 12-14 m. Alley gap 3-4 m every 40-60 m. | 0-1.5 m behind a 2 m pavement; apartments 4-6 m | 2-4 (apartments 3-4) |
| Square + car park | 0 | Market square 60 × 40 m (`Plaza`) at Main St × road 4; car park `Parking` 50 × 30 m by East Lane | — | — | — |
| Outskirts | **80-130** | Detached houses with gardens and the odd semi: B-road ribbon x −1300…−700 (~35) and +40…+200 (~12), above Upper Road on road 4 and a 400 m "Hill Lane" (~25), 3 hamlets of 8-14 (Westbrook at the hillclimb foot −1700,−1180; Southford on the loop near 0,−1640; Quarryside near 1300,−900) | Plot frontage 18-26 m, footprint 8-12 × 8-10 m, side gap ≥ 6 m | 5-9 m | 2 |
| Farms (low roads) | 25-35 steadings (55-75 buildings) | Farmhouse + barn (as now), also on the B-road outside the town | as now | 14-22 m | 1-2 |
| Service park | 7-10 | Sheds (as now) + 2 team awnings | as now | 14 m | 1 |
| Mountain (> 250 m) | **8-12** | `Use::Hut` (1 floor, 4-6 × 5-7 m, timber, gable): logging camp ×3, radio tower ×1, hillclimb summit café ×1, pass shelter ×2, quarry office ×1, 2 forest cabins on road 4 | ≤ 1 per 400 m of road, only at places of interest | 8-20 m | 1 |

**Totals:** about 560-780 buildings (now 110).

**Placement rules:**
- Shape the rows by the block edges. Every street gets a building line; the block interiors stay
  back yards (dressing places sheds, washing lines and allotments, no trees).
- Block depth between Main St and Church St is about 70 m: two rows 12-14 m deep with 15-20 m
  yards.
- Keep the `frontage` flatness test (`buildings.rs:306`, under 2.5 m across corners). In the town
  mask, terraces step their `base` per unit on slopes over 4 %.

**Code touch points:**
- `buildings.rs:311-363` (`village` becomes `town` + `outskirts`), `VILLAGE_R` (l.366), and the
  tests at l.1385 (60-140 becomes 380-520 core), l.1386 (≤ 200 becomes ≤ 900) and `mesh_budget`
  l.1577.
- `world.rs:369` replaces the 210 m square tree exclusion with the town-mask polygon and the
  outskirts plots.
- Traffic: town streets carry traffic at a lower spawn weight (0.3). The B-road is 50 km/h over
  the town span, through per-station `limit_at` (`traffic.rs:161`).

**Performance:** today each building is its own mesh entity with one shared material, so draw
calls equal the building count (×4 shadow cascades).
- At 700 buildings that is about 2,800 draw calls. **Merge per cluster:** one mesh per street block
  or hamlet, at most 120 m across and at most 40k triangles.
- That gives about 45-60 meshes in total, under 250 draw calls with shadows.
- `Building` entities stay in the level as data (editor Details/Move). `open::live` rebuilds only
  the cluster that holds an edited building.
- Keep the total triangle count under 600k (about 600 per terrace).
- `Layout::occupied` (`buildings.rs:193`) and the `overlaps` scan in `frontage` are O(n). Bucket
  buildings into the 10 m `WallGrid`-style grid before tree placement asks per candidate.

## 4. Roadside furniture and road conditions

Generated in `art::roadside::items` (`roadside.rs:62-139`), so it is deterministic and in station
order. Solid pieces become `vehicle::Wall` segments in the shared list (`open/mod.rs:142-149`;
`walls::WallGrid`, 10 m cells). Signs sit on the **right** verge (traffic keeps right) at
`edge + 0.5 + 1.0 m`, at most one sign per 50 m. Priority: junction > hairpin > crest > bend.

| Item | Rule | Collision |
|---|---|---|
| **Guard rail** (steel W-beam on tarmac; timber-faced steel on gravel/forest) | Station `i`, a side where the drop `D = edge height - terrain(edge + 8 m)` is more than 3 m **and** (outside of a corner of grade ≤ 4, or `D` over 6 m anywhere, or grade 5/6 corners above 250 m with `D` over 3 m). Extend 20 m both ways, merge gaps under 30 m, minimum 24 m, ends buried and flared 4 m. Offset `edge + 0.5 + 0.4 m`. *Measured* edge with D > 3 m (6 m out): hillclimb 430 m, basin 610, forestry 792, logging 96, gully 56, pass 52. Expect 1.2-1.6 km of rail. | `Wall` every 2 stations, radius 0.12, top ground + 0.75. Tangential friction as for facades (scrape, don't snag); damage like a facade brush. |
| **Stone blocks / boulders** (gravel/forest, instead of rail where D is 3-6 m and the corner is grade ≥ 3) | One per 6-8 m on the outside edge | Existing cylinder obstacles (r 0.4-0.6) |
| **Chevrons** | Keep today's rule (grade ≤ 2 after a fast run or over a crest, `roadside.rs:104-129`). Also: every hairpin on tarmac, and tarmac grade 3-4 when the corner speed is 30 km/h or more below the speed 150 m before. 3 boards (grade 3-4) or 5 (hairpin, 6 m apart). | Knockable prop (`props.rs` knock), not solid |
| **Warning signs** (triangles) | 120 m before on tarmac, 70 m on gravel. Hairpin (grade 0); double bend (two grade ≤ 3 corners of opposite hand within 150 m); crest (R_v < 150 m with the crest blind); junction (every side road on BRoad/Street); steep hill "12 %" (> 10 % for > 200 m); falling rocks (cut bank > 6 m for > 60 m); deer (forest roads, every 1.5 km); narrow bridge (bridge stations). Yump challenge crests get the "jump" danger sign (`GAME_SYSTEMS.md` §3). | Knockable |
| **Speed / place signs** | Town name board + "50" where the B-road enters the town mask, "end 50" + "80" on the way out; "30" at each town street mouth; "60" before the hillclimb hairpin ladder. Values must match `traffic::limit` and `limit_at`. | Knockable |
| **Km posts** | Every 1 km on roads 0 and 1 (and 100 m stubs on the hillclimb), right side | Knockable |
| **Reflector posts** (delineators) | Tarmac outside the town: every 50 m both sides, every 25 m in corners with R < 150 m. Gravel: every 100 m on the fast gravel roads (2, 6, 7). Forest: none (snow poles above 405 m stay as now). | Knockable |
| **Street lights** | Town streets every 30 m, alternating sides | Wall stub r 0.15 |

**Road conditions.** These are per-station data written by `profile::dress`. The exact surface is
in `roads::surface` and the exact height in `profile::ground`, so the car and the mesh agree.

| Condition | Where | Effect |
|---|---|---|
| Potholes | Roads 0 and 1 outside the town where `noise(s/150) > 0.55` ("old tarmac"), 1 per 120-250 m, Ø 0.6-1.4 m, 4-8 cm deep, plus darker patch repairs | A height dip in `ground` (a jolt), small suspension damage over 70 km/h; grip unchanged |
| Wet tarmac / puddles | Tarmac where `wet > 0.75` at a local low point of the profile, 8-25 m | New `Surface::Wet`: peak μ 0.85, loose 0.10; spray FX |
| Mud patches | Gravel where `wet > 0.70` (forest already uses `> 0.62`, `roads.rs:351`), 15-40 m | `Mud` (0.55 / 0.8) |
| Washouts | Gravel/forest where grade > 9 % and `wet > 0.5`, 10-30 m | Full-width `Loose` (0.66) + corrugation ±2 cm at 0.9 m wavelength in `ground` (analytic in s) |
| Shaded ice | Forest above 440 m, north-facing cut side, in hairpins only | `Snow` (0.42) for the inside 1.5 m |

Every condition shows in the road mesh. The pace notes add "caution" on washouts and ice; that is
`codriver` work for the audio pass.

## 5. Challenge placement (first wave, hand-picked anchors)

These follow the `GAME_SYSTEMS.md` §3 rules: ≥ 500 m between challenges of the same kind, ≥ 120 m
from a junction, none inside the town mask. The seeded placer fills up to 7/5/6/6; these are the
anchors it must keep. Station `s` is in metres along the road (re-measure after the re-bake: the
roads move).

| Kind | Where (road, s, world x,z) | Why it fits (*measured*) |
|---|---|---|
| Radar Post 1 | B-road east straight, gate s ≈ 3330 (1590, −1250), eastbound | 620 m at −0.3 %, ends at the bend into the basin road |
| Radar Post 2 | B-road west straight, gate s ≈ 600 (−1180, −1150), westbound, outside the town | 776 m straight at −0.3 % |
| Radar Post 3 | Southern loop (7), gate s ≈ 1030 (60, −1636), eastbound | 592 m flat gravel straight |
| Radar Post 4 | Hillclimb (1), gate s ≈ 400 (−1535, −905), uphill | 312 m at 3.2 % before the first bends |
| Pace Section 1 | Gully road (6), s 100-1900 (1.8 km) | Flowing gravel: 300 m of grade 3-5 radii per 500 m, almost no tight corners |
| Pace Section 2 | Mountain pass (3), s 1500-2900 (1.4 km) | Alpine, no tight corners in s 2050-2550, finishes at the crest |
| Pace Section 3 | Southern loop (7), s 250-1850 (1.6 km) | Fast gravel, two narrow bridges |
| Slide Section 1 | Hillclimb tarmac switchbacks, s 1250-1900 (−1918, 81 → −1868, 400) | 4 hairpins in 650 m (tarmac ×1 per the rules) |
| Slide Section 2 | Basin gravel (2), s 1800-2600 | 3 hairpins (s 1840, 2170, 2584) |
| Slide Section 3 | Forestry road (4), s 2450-3000 | 2 hairpins (s 2528, 2924) and a gravel mid-section |
| Yump Board 1 | Hillclimb (1) crest s ≈ 932 (−1561, −319), tarmac | Now R_v 155 m, 5.7 → −8.4 %, 92 m straight before |
| Yump Board 2 | Pass (3) crest s ≈ 2858 (1461, 1735) | Now R_v 109 m, 10.4 → −11.6 %, 96 m straight before; hairpin 188 m after |
| Yump Board 3 | Gully road (6) crest s ≈ 1294 (1120, −175) | Now R_v 175 m, 5.5 → −6.2 %, 70 m straight before. Fallback: forestry road (4) crest s ≈ 2096 (52, 302), R_v 99 m |

**Yump sculpt.** No crest on the map meets R_v < 60 m today. Fix each Yump crest in `roads.rs`
after `undulate`:
- Within ±40 m, replace the profile with the two tangent grades measured at ±40 m, joined by a
  vertical arc of R_v **45 m (gravel) / 55 m (tarmac)**.
- Height change ≤ 0.4 m (mid-ordinate `R·Δg²/8`). Grades stay as they are, so the grade limits
  hold.
- Take-off at `v > √(g·R)`: 75 km/h (gravel), 83 km/h (tarmac). Then re-shape the terrain corridor.
- Straighten the run-up to ≥ 100 m where the bake measures less (Yump 1 and 3 are borderline).
  Otherwise use the fallback.
- The pace notes already call crests with R_v < 60 m.

**Free-roam loop** (pacing): town (calm, 50 km/h) → Radar 2 (burst) → hillclimb Slide 1 + Yump 1
(peak) → pass Pace 2 + Yump 2 (high, open views) → basin Slide 2 (peak) → gully Pace 1 → service
park (rest) → Radar 1. That is a 20-25 minute circuit with a rest point every 5-7 minutes.

## 6. Re-bake (`--bake-world`)

1. Commit the code first. **The bake throws away every hand edit** (moved buildings, `Trees` and
   `Dressing` `PcgOverrides`). Check `git diff assets/levels/` and keep `open.level.ron.bak`.
2. `memgate -- cargo test --lib open::` covers land, roads, buildings, events and level round
   trips. Picture check:
   `PACENOTE_LAND_PPM=/tmp/land.ppm memgate -- cargo test --lib open::land::picture -- --ignored`.
3. `memgate --profile release -- cargo run --release -- --bake-world assets/levels/open.level.ron`.
   It prints the roads, control points, events, buildings and terrain (`level.rs:606-633`). It
   writes `open.level.ron` + `open/terrain.{ron,r16}`.
4. Expect: 9 + ~12 roads, 5 events (lengths 4-15 km), 560-780 buildings.
5. The challenges bake after this (`--bake-challenges`, `GAME_SYSTEMS.md` §3) because their
   stations depend on the new roads.
6. Run `committed_levels_load` and `open.check` (roads more than 5 cm off the live ground: expect
   0).
7. Run the game detached (nohup, output to a file). Take screenshots at the 8 parity camera spots,
   plus the town, the east wall and the escarpment at 300 m.
8. Records and ghosts are keyed by the level's content, so the boards reset. That is expected; say
   so in the commit.

## 7. Acceptance criteria (measured on the new bake, same method as §1)

| Metric | Now | Target |
|---|---|---|
| Area > 100 % slope | 3.6 % | ≤ 0.8 %, ≥ 70 % of it inside the crag mask |
| Area > 150 % slope outside the crag mask | ~1.0 % | ≤ 0.05 % |
| Neighbouring 2 m samples more than 3 m apart, outside crags | ~24,600 | < 1,000 |
| Worst mean slope of any 20 m height band | 73 % (300 m) | ≤ 50 % |
| Area at 180-340 m (hypsometry) | ~14 % | ≥ 20 % |
| Area < 10 % slope | 40.8 % | 30-38 % (the town stays flat) |
| Town mask slope, 90th percentile | 9 % | ≤ 6 % |
| Slope > 70 % at 30-46 m from a road vs 60-90 m | 5.5 vs 5.6 % | ≤ the 60-90 m figure; no 2 m step > 1.5 m within 50 m of a road |
| Road grade (`grades_and_hairpins`) | passes | passes; hairpins ≥ 7 m; events 4-15 km |
| Buildings | 110 | core 380-520 · outskirts + hamlets 80-130 · farm steadings 25-35 · huts 8-12 |
| Building meshes / triangles | 110 / — | ≤ 60 cluster meshes / < 600k triangles |
| Furniture | — | rail 1.2-1.6 km; every hairpin has a warning sign + chevrons; a delineator every 50 m (±5) on rural tarmac |
| Yump crests | 0 with R_v < 60 m | 3 at 45/55 m; the dev autopilot at pace 0.85 gets ≥ 0.3 s airborne at each |
| Determinism | `land::tests::deterministic` | two bakes give the same bytes (`terrain.r16`, `.level.ron`) |
| Bake / load time | ~5 s / 0.04 s | ≤ 10 s / ≤ 0.1 s |

Add an ignored dev test `open::land::tests::slope_report`. It prints the §1 histogram, the band
means and the 2 m step count, and gives these numbers one command.

## 8. Risks

- **Every road moves.** New land means a new A*. The event roles and indices (`events.rs:167`)
  survive, but the hand-placed grade notes, home and challenge stations need re-measuring. The
  §5 stations are approximate.
- **The bake wipes hand edits.** Diff before and after. If the owner has edited in the editor, port
  the edits by hand.
- **Junction count about ×2.5** (15 → ~39): more traffic reservation and junction meshes. Keep the
  town streets T-only and give them a low traffic weight. Watch `traffic.rs` cost in the metrics
  viewer.
- **Draw calls** if per-cluster merging slips: 700 single meshes × shadow cascades will hurt a base
  MacBook. Merging is part of this pass, not a follow-up.
- **Thermal pass over-flattens the plateau**: talus 0.75 is a cap, not a target. If the band means
  drop below 15 % on the plateau, lift T to 0.85.
- **Grade limits**: the corridor softening (T10) only changes ground beside roads, never road
  heights. The Yump sculpt keeps the grades but adds crest calls the voice must read ("over jump"
  may need a new line: flag to audio).
- **Docs drift**: update `GAME_OVERVIEW.md` §4.3/4.4 and `RALLY_MAP.md` status after the merge (cut
  70 %, blend adaptive, building counts).
