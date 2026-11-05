# Game systems: Style, roadside challenges, Notes, garage, rival drivers

Creative director + game designer + economy designer, 4 Nov 2026. Run without the owner present:
where a choice was needed it is made here, with a one-line reason. The UX team owns the look of
every screen; this doc says which systems, numbers, menu items and flows exist.
Code facts used: `vehicle::{Car, Spec, Wheel, Damage}`, `autopilot::Autopilot::input`,
`open::events`, `open::traffic::{Traffic::cars, CarView, TrafficSolids}`, `drive::{DrivenCar,
CarSpec, StageRun}`, `meta::Profile`, `art::traffic_car` (4 body styles, 7 paints).

## 1. Vision

1. The valley is a place you drive in on purpose: every road has something to beat on it.
2. Driving well pays. Slides, air, close shaves and clean lines build a **Rhythm** that banks
   **Style** and money.
3. Money is **Notes**. You spend it on cars and parts, and the parts change the real sim (no fake stats).
4. Events get **rival drivers**. Five AI cars run the same deterministic sim on the same narrow
   roads, and the purse depends on where you finish.
5. Everything is ours: names, cars, series. The genre's habits are fine; its brands are not.

**Pillar fit (driving that feels earned).** Every score comes from measured sim state: body slip
angle, wheel contact, speed and gaps. Nothing is triggered by a button. AI cars get no grip or
power cheats. Rubber banding is capped at ±3 % pace and is off near the finish. Upgrades multiply
`Spec` fields, so a faster car drives differently, not just faster on a stat card. Pillar 3
(roads worth learning) also holds: challenge thresholds come from a sim run on the actual road.

**Series name.** Events and challenges together are **the Valley Trials**. (The old "festival"
framing is out: it is genre-coded.)

## 2. Skill scoring: Style and the Rhythm

### 2.1 Inputs (all present in the sim today)

| Symbol | Source |
|---|---|
| `v` km/h | `car.speed() * 3.6` (forward speed) |
| `β` deg | body slip `atan2(vel·left, vel·fwd)` (the autopilot's `beta`), signed |
| contact | `car.wheels[i].contact` |
| surface | `wheels[i].surface` (tarmac vs loose: Gravel/Loose/Mud/Snow/Dusty) |
| hit | `car.damage.hits` increments (a damaging hit, sev ≥ `DAMAGE_FROM`) |
| knock | `car.impacts` rises without `hits` rising (a scrape or brush) |
| broken | `car.broken.len()` grows (saplings, posts, signs snapped) |
| upright | `car.up().y` |
| others | traffic `CarView`s + rival cars (`OtherCars`, §8): pos, fwd, speed, id |
| road | nearest road station: `curv`, half width, lateral offset |

Detection runs in `FixedUpdate` after `SimulateSet`, once per 60 Hz step. It is pure
(`skills::detect`). Nothing scores during the countdown, a pause, or while `StageRun.down` is set.

### 2.2 Skills

| Skill | Starts / scores when | Points |
|---|---|---|
| **Slide** (drift) | \|β\| ≥ 15°, v ≥ 35 and ≥ 2 wheels in contact, held 0.25 s. Continues while \|β\| ≥ 10° and v ≥ 25. Ends 0.3 s after dropping below. | per s: `4 × (min(|β|,55) − 10) × min(v,140)/50`. A 30° slide at 60 km/h scores 96/s. |
| ↳ **Switch** | β changes sign inside one Slide within 1.0 s (a flick) | +150 flat |
| ↳ spin | \|β\| > 100° | that Slide's points are void. The Rhythm is kept. |
| **Flat Out** (speed) | v ≥ T, where T = 150 on tarmac and 115 on loose. Ends 0.5 s below T − 5. | per s: `3 × (v − T + 10)`. 160 km/h on tarmac scores 60/s. |
| **Air** (jump) | All 4 wheels off the ground for ≥ 0.30 s. Scored at landing. | `250 × t_air + 6 × horizontal metres`. 0.8 s and 20 m scores 320. Label "Big Air" at ≥ 25 m. |
| ↳ landing | Clean landing: within 0.6 s of first contact, no `hits` increase, no body contact, `up.y` ≥ 0.85 | A bad landing voids the Air. A hit also breaks the Rhythm (see breaks). |
| **Close Shave** (near miss) | When another car's centre crosses the player's lateral plane: body gap `g = |lateral| − 1.8 m` ≤ 1.2 m, closing speed ≥ 25 km/h, no contact with it for ±1 s | 120, or 250 when g ≤ 0.6 m ("Paint Scraper"). ×1.5 for oncoming cars (fwd·fwd < 0). Once per car per 5 s. |
| **Slipstream** (draft) | A car ahead 4-20 m, lateral offset ≤ 1.2 m, both cars ≥ 70 km/h, held 1.0 s. Ends 0.5 s after leaving that zone. | 25/s |
| **Splinter** (wreckage) | Each new entry in `car.broken` | 40, or 80 at v ≥ 80. Counts the first 12 per Rhythm only (no sapling farming). |
| **Switchback** (hairpin) | Entering and leaving a stretch of the nearest road with radius ≤ 12 m (\|curv\| ≥ 0.083), staying on the road (\|lateral\| ≤ half width + 0.5), no hit, mean speed ≥ 30 km/h | `200 + 10 × (v_mean − 30)` |
| **Clean Line** | Every 1,000 m driven with ≥ 80 % on the road and no hit, car contact or recover. In an event: each quarter split instead. | 300 |
| **Clean Pass** (overtake) | Races only: your route progress goes from behind a rival to ahead of it and stays ahead 3 s, with no contact with that rival | 300 |
| **Challenge** | A roadside challenge finishes (§3) | 500 × stars earned on that run |

### 2.3 The Rhythm (chain)

- The first scored skill opens a Rhythm with pending `P = 0` and multiplier `M = 1.0`.
- Each completed skill, and each 2 s of a continuous skill (Slide, Flat Out, Slipstream), adds
  0.2 to M. A skill different from the last completed one adds another 0.1 (rewards variety).
  M caps at ×6.0.
- **Timeout:** 3.5 s with no skill active, then it **banks**. A continuous skill keeps the Rhythm
  alive. The HUD ring shows the time left.
- **Bank:** `Style = round(P × M)`. It also pays `floor(Style / 25)` Notes, capped at 2,000
  Notes per Rhythm. Driving into an event marker, starting a challenge gate or opening the
  map/pause also banks at once. Pause only freezes the clock; it does not bank.
- **Break** (P is lost, "Rhythm broken"): a damaging hit; contact with a traffic or rival car at
  a closing speed ≥ 8 km/h; Recover (R); upside down or on the side (`up.y` < 0.3); a car swap;
  an event's start or restart; quitting.
- **Scuff:** a knock below damage (a wall brush, a shrub) halves M (minimum ×1.0) and keeps P.
  A Splinter is not a scuff.
- Events and races score the same way. On a timed run the Rhythm banks at the finish.
- Typical rates, used for the economy: a tidy minute on gravel ≈ 4-6k Style ≈ 200 Notes; a
  great minute ≈ 15-20k ≈ 700 Notes.

### 2.4 HUD messages (`SkillEvent`, §8)

`Started{skill}`, `Live{skill, points}` (each step while a continuous skill runs), `Scored{skill,
label, points}`, `Mult{m}`, `Scuffed{m}`, `Banked{style, mult, notes}`, `Broken{lost, why}`.
HUD: a Rhythm panel at the top centre (P, ×M, the timeout ring) and up to 3 stacked skill lines
(newest on top, each 2 s). Banked shows a Style total plus "+N Notes". Broken shows the lost
points struck through, plus the reason ("Hit a tree"). Setting: Interface > Style HUD: Full /
Minimal (bank and break only) / Off. Scoring still runs when the HUD is off.

## 3. Roadside challenges

These are drive-through challenges: no stopping and no menu, which is what makes them different
from events, where you stop in the column. The first gate starts the attempt. Every result is
stored, and every star pays once. Placement is decided by a pure, seeded function over the
`RoadNet` and baked into the level as placeables (`ChallengeStart`), so the editor can move them.

| Kind | What it measures | Validity | Count | Placement |
|---|---|---|---|---|
| **Radar Post** (speed trap) | Speed at the step the car's centre crosses the gate line, either direction | On the road (\|lateral\| ≤ half width + 1 m) | 7 | At the end of a ≥ 250 m straight (\|curv\| < 1/300), ideally before a grade ≤ 4 corner (the braking is the risk). B-road ×3, hillclimb ×1, basin gravel ×2, gully or southern loop ×1. |
| **Pace Section** (speed zone) | Mean speed = length / time, start gate to end gate | Pass 4 hidden mid-gates (within 15 m of centre), no Recover, no reversing | 5 | One stretch of 1.2-2.5 km each on the hillclimb, basin gravel, mountain pass, forestry road and southern loop |
| **Slide Section** (drift zone) | Raw Slide + Switch points scored between the gates (before M) | No damaging hit inside | 6 | 300-800 m with ≥ 4 corners of grade ≤ 3 or ≥ 2 hairpins. Gravel or forest ×5, tarmac switchbacks ×1. |
| **Yump Board** (danger-sign jump) | Horizontal metres from take-off (last wheel leaves) to the first wheel down | Take-off within ±8 m of the crest station and on the road; clean landing (§2.2) | 6 (min 3) | Crest with vertical radius < 60 m and a grade change ≥ 5 %, a straight ≥ 100 m before it and ≥ 60 m after it, and no grade ≤ 3 corner within 80 m after the crest |

24 challenges in total. Spacing rules: ≥ 500 m between challenges of the same kind, ≥ 120 m from
a junction, ≥ 50 m from an event marker, none inside the village bounds.

**Stars.** At bake time (`--bake-challenges`), the dev autopilot in a stock Saker R5 (Class B)
drives each challenge at pace 0.85 and records reference value `R`. Then 3★ = 1.00 R,
2★ = 0.88 R, 1★ = 0.72 R (Slide Section uses the drift style). Thresholds are stored with the
placeable and rounded: km/h to 5, metres to 1, points to 100. Expected ranges: Radar 115/140/160
km/h; Pace 70/85/97 km/h; Slide 3,000/6,000/10,000; Yump 14/22/30 m. Thresholds are the same for
every car. Faster cars earn stars more easily, which is the reason to buy them.

**Finding them.** Each kind has its own roadside board: a coloured plate with a pictogram and the
current best. Gates show a painted ground stripe and two flag posts. Every challenge is on the
full map from the start (grey, then bronze/silver/gold by stars), with no discovery grind. The
minimap shows those within 400 m. Inside 150 m a pill shows "Radar Post · best 132 · ★★☆ 140".
The result pill shows the value, the stars and "+N Notes" when a star is earned.

**Rewards.** First 1★ pays 400 Notes, first 2★ pays +600 more, first 3★ pays +1,000 more (2,000
per challenge, 48,000 for all 24). A new personal best with no new star pays 50. Every run also
feeds the Rhythm (the Challenge skill).

## 4. Economy: Notes

Currency **Notes**, shown as `₦` (the HUD and menus write "12,400 Notes"; the symbol is a UX
option). The name ties paper money to pace notes.

| Faucet | Amount |
|---|---|
| Starting balance | 12,000, plus the **Wren 1.4** (owned, free) |
| Rhythm bank | Style / 25, max 2,000 per Rhythm |
| Challenges | 400 / 600 / 1,000 per new star; 50 for a new best |
| Race purse `P` | `round100(300 + 350 × km)`: River crossings 1,300, Hillclimb / Quarry 2,100, Fernhollow 2,700, Blackpine 4,800 |
| Finish share | P1 100 %, P2 55 %, P3 35 %, P4 20 %, P5 10 %, P6 5 %, DNF 0 |
| Difficulty × | Rookie ×0.75, Pro ×1.0, Ace ×1.5, Legend ×2.5 |
| First win | +P × difficulty, once per event per difficulty |
| Clean race | +10 % (no contact with a rival) |
| Time trial (solo, existing) | 15 % of P per finish; +10 % of P for a new personal best |

| Sink | Amount |
|---|---|
| Cars | 9k-140k (table below) |
| Upgrades | 1.5k-14k per level × class factor |
| Entry fee | Ace 10 % of P, Legend 25 % of P (the high stakes: a DNF loses it) |
| Service (repair) | `round100(1,500 × Damage::total() × class factor)`. Free at an event start (fair start). Otherwise at the Service Park marker or in the Garage. |

Pacing check: all first wins at Pro ≈ 26k; all 3★ ≈ 48k; skilled free roam ≈ 12-40k/h. The first
car purchase (Saker, 45k) comes after ~1-1.5 h; the top car plus a full build comes after ~8 h.
Selling returns 60 % of the price plus 50 % of what was spent on upgrades.

### 4.1 Roster: one sim, eight `Spec`s

Base = `Spec::default()` (the R5: 360 N·m ≈ 210 kW, 1,250 kg). `torque×` scales `torque`;
`grip×` is a **new `Spec.grip`** multiplier on tyre μ (default 1.0); `split` is `front_split`;
other fields stay default unless listed. Mass also scales `inertia`.

| Car | Body | Class / PI | Drive | kW ≈ | Mass | grip× | Price | Colour | Notes |
|---|---|---|---|---|---|---|---|---|---|
| **Wren 1.4** | traffic hatch | D 370 | FWD (1.0) | 115 (×0.55) | 1,050 | 0.92 | starter (9k) | yellow | `traction_assist` on, `lsd` 20 |
| **Marten Tourer** | estate | D 362 | RWD (0.0) | 136 (×0.65) | 1,320 | 0.90 | 14k | green | soft springs ×0.85, `drag` 0.80 |
| **Bruin Trail** | SUV | C 428 | AWD (0.45) | 189 (×0.90) | 1,550 | 0.90 | 24k | dark green (metallic) | `rest` +0.06, `travel` +0.04: a yump monster |
| **Vale GT** | saloon | C 456 | RWD (0.0) | 178 (×0.85) | 1,300 | 0.95 | 30k | red | the drift car: `lsd` 90 |
| **Saker R5** | rally hatch | B 570 | AWD (0.5) | 210 (×1.0) | 1,250 | 1.00 | 45k | white / blue | today's car, unchanged |
| **Harrier S** | saloon | B 585 | AWD (0.35) | 241 (×1.15) | 1,380 | 1.00 | 62k | silver (metallic) | road-going AWD, `drag` 0.68 |
| **Fennec Kit** | rally hatch | A 659 | RWD (0.0) | 220 (×1.05) | 1,020 | 1.02 | 85k | orange | light and tail-happy |
| **Ossian Evo** | rally hatch | S 728 | AWD (0.45) | 284 (×1.35) | 1,230 | 1.05 | 140k | black / gold | `drag` 0.78, brakes ×1.15 |

Paint: the player picks any of the 7 traffic paints in solid or metallic, free. Each car's listed
colour is its showroom default. The rally-hatch body is shared by three cars; the UX/art teams
should tell them apart by livery, not by a new mesh.

### 4.2 Upgrades (`Upgrades`, level 0 = stock)

Prices are × the class factor: D 0.6, C 0.8, B 1.0, A 1.3, S 1.6.

| Part | L1 | L2 | L3 | Spec effect (L1 / L2 / L3) |
|---|---|---|---|---|
| Engine | 3,000 | 7,000 | 14,000 | `torque` ×1.06 / 1.12 / 1.20 |
| Turbo | 6,000 | 12,000 | n/a | `torque` ×1.08 / 1.16, `engine_brake` ×0.9. Engine × turbo is capped at ×1.35. |
| Tyres | 2,000 | 5,000 | 9,000 | `grip` ×1.03 / 1.06 / 1.09 |
| Brakes | 1,500 | 3,500 | 6,000 | `brake_torque` and `handbrake_torque` ×1.10 / 1.20 / 1.30 |
| Suspension | 2,500 | 5,000 | 8,000 | `damper` ×1.10 / 1.15 / 1.20, `travel` +0.01 / +0.02 / +0.03, `arb` ×1.05 / 1.10 / 1.15 |
| Weight | 3,000 | 7,000 | 12,000 | `mass` and `inertia` ×0.97 / 0.94 / 0.90 |
| Gearbox | 2,500 | 6,000 | n/a | **new `Spec.shift_time`** ×0.8 / 0.6 (today a constant), `final_drive` ×1.04 / 1.08 |

Fitting is instant in the Garage. A build is `(car, Upgrades)`, and `build_spec()` is pure, so
ghosts and records carry `sim_hash` exactly as they do today.

### 4.3 Performance index

```
kW     = 0.583 × torque            (N·m; the R5 curve's peak)
pw     = kW / (mass / 1000)
PI     = clamp(100 + 2.2·pw + 400·(grip − 0.85) + drive + 100·(brake_torque/4200 − 1)
               + 50·(1 − shift_time/base_shift), 100, 999)
drive  = AWD 40 · RWD 15 · FWD 0        (AWD = 0.2 < split < 0.8)
Class  = D <400 · C 400-499 · B 500-599 · A 600-699 · S ≥700
```

The record board stays one per event. A record stores car id + PI, and the board shows a class
chip next to each record. (One board keeps the bragging simple; class filters can come later.)

### 4.4 Save: `career.json` (data dir, separate from settings)

```json
{ "version": 1,
  "wallet": { "balance": 12000, "earned": 0, "spent": 0 },
  "garage": [ { "car": "wren", "upgrades": {"engine":0,"turbo":0,"tyres":0,"brakes":0,
                "suspension":0,"weight":0,"gearbox":0}, "paint": [0, false], "damage": 0.0 } ],
  "selected": "wren",
  "world": { "pos": [x,y,z], "heading": 0.0 },
  "challenges": { "<id>": { "best": 0.0, "stars": 0 } },
  "races": { "<event id>": { "rookie": 0, "pro": 0, "ace": 0, "legend": 0 } },
  "stats": { "style_banked": 0, "best_rhythm": 0, "km": 0.0, "top_kmh": 0.0,
             "longest_air_m": 0.0, "close_shaves": 0, "wins": 0 } }
```

Save happens on every wallet change (debounced 2 s), on garage changes, and on quit. Writes are
atomic (tmp + rename). A missing file starts a fresh career; a corrupt file is kept as
`career.bad.json` and the game starts fresh with a notice. Offline only, so there is no
anti-tamper.

## 5. Rival drivers (AI racers)

- **Field:** 5 rivals plus the player (6 cars) in every Rally and Sprint event, when started
  with "Race". **Time Trial** stays as today (solo, ghost, records). At the marker, the
  stop-to-start pill offers Race (Enter) or Time Trial (T). The default is the last one used.
- **Driving:** each rival is a full `vehicle::Car` stepped by `vehicle::step` with its own
  `Spec` (its roster car plus upgrades, matched to the player's PI ±20, class first), on the same
  `OpenWorld` ground. Its input comes from the autopilot's path follower, refactored into a pure
  `ai::Driver` that works in release builds (not just `dev`), reading the route `Road` + corners.
  Additions: a lateral line offset, gap keeping (IDM-style on the car ahead), overtaking, yielding
  and mistakes. Everything is seeded by `hash(event id, difficulty, attempt)`, so a Retry with
  the same inputs gives the same race.
- **Step order:** player first, then rivals in id order. Each car sees the others' poses from
  the start of the step, as 3 solid circles each (r 0.95 at −1.3 / 0 / +1.3 m, top 1.4: the
  traffic solids). Deterministic.

| Difficulty | Pace (share of grip) | Brake share | Mistake / corner ≤ grade 2 | Yields to player | Player start slot |
|---|---|---|---|---|---|
| Rookie | 0.55 | 0.50 | 15 % | always | 3rd |
| Pro | 0.68 | 0.55 | 8 % | always | 4th |
| Ace | 0.80 | 0.60 | 4 % | 50 % (seeded) | 5th |
| Legend | 0.92 | 0.65 | 1 % | never: defends the centre | 6th (last) |

Each rival also gets a seeded pace spread of ±3 %. A mistake is a lift plus a 0.4 s late
turn-in, or on loose ground an over-rotation; it costs 0.5-2 s and never a crash.

- **Rubber banding: light.** A rival > 6 s behind the player gets +2 % pace; a rival > 6 s ahead
  gets −2 %. Total effect is capped at ±3 %, and it is **off in the last 15 % of the route**.
  (It keeps the pack in sight without the finish feeling handed out.)
- **Starts, decided by road width.** If the route's minimum width over its first 400 m is
  ≥ 6.0 m (B-road, hillclimb): a **Grid Start**, 2 wide (±1.5 m off the centre), 3 rows 8 m apart,
  one launch. If it is narrower (gravel and forest, 3-5 m): a **Pursuit Start**, single file 10 m
  apart, released 1.5 s apart from the front. One race clock; the first car across the finish
  wins. (Real time-gap rally starts give no wheel-to-wheel racing, and side-by-side on 3.4 m is
  a wreck. A pursuit start makes passing the goal.)
- **Overtaking:** a rival passes when the road ahead is ≥ 4.5 m wide for ≥ 60 m and the car
  ahead is ≥ 3 % slower; it uses the line offset `±(half width − 1.1)`. Otherwise it follows at a
  0.6 s headway. **Yielding** (the rally "let past"): a rival caught within 1.2 s for 2 s, on a
  straight ≥ 3.8 m wide, moves to the edge and caps itself at 85 % speed for up to 3 s.
- **Positions:** ordered by route progress `s` (`StageRun.progress` for the player). The HUD
  shows "P3 / 6", the gaps to the cars ahead and behind (from a 0.25 s progress→time history),
  and the rivals as dots on the progress bar. Rivals have invented names, e.g. "A. Varenne",
  "T. Okkonen", "R. Castell", "M. Duvane", "I. Strand", "K. Belloc", "J. Marr", "S. Ilves". UX
  must check none is a real driver before shipping.
- **Finish:** the player's finish ends the race. Rivals still running get an estimated time:
  their elapsed time + remaining metres ÷ their mean speed so far (deterministic). A rival whose
  part breaks retires (DNF, last). A retired player is DNF: no purse, entry fee lost.
- **Recover in a race:** puts the car back on the route at the same `s` and **holds it for
  5 s** (the clock is shared, so a time penalty would mean nothing).
- **Contact:** solid and fully simmed, so damage follows the player's DamageMode for both cars.
  Contact at ≥ 15 km/h closing speed is logged. Any logged contact voids the Clean race bonus
  and that rival's Clean Pass, and breaks the Rhythm (≥ 8 km/h).
- **Performance budget:** 5 extra car steps (8 substeps each) per 60 Hz step must stay
  ≤ 1.5 ms on an M-series base chip. Fallback, documented but not built: rivals > 600 m from the
  player step at 30 Hz.

## 6. Menu structure (items and flows; UX designs the look)

```
Title (press any key) ─▶ Main menu
  Continue        → loads the world at the saved pose in the selected car (first launch: "Start")
  Events          → list of 5: kind, km, purse at each difficulty, best finish, 1st-win badge
                      ▸ pick event ▸ Race (pick difficulty, shows fee/purse) | Time Trial
                      ▸ Go now (loads straight to the start line) | Set route (loads, map route on)
  Challenges      → 24 by kind, stars, best, "Set route"
  Garage          → owned cars ▸ Select · Upgrades (7 parts, PI before → after, price) ·
                      Paint · Service (repair cost) · Sell
  Showroom        → 8 cars: stats bars (power, mass, grip, drive), PI/class, price ▸ Buy (confirm)
  Logbook         → stats, records boards (per event; class chips), ghosts, Notes ledger (last 50)
  Play together   → legacy driver + guide (unchanged)
  Settings        → existing tabs, + World: time of day, weather; + Interface: Style HUD
  Credits · Quit
```

- The wallet balance and the selected car (name, class chip, PI) show on every menu screen,
  top right.
- Pause menu in free roam gains **Garage** (only at the Service Park or home, so you can't
  re-tune mid-run) and **Logbook**. Pause during a race: Resume, Restart race (the fee is not
  charged again), Recover (5 s hold), Retire, Settings.
- Loading screens (UX owns the look) get content hooks: the car you're in (name, PI), one
  unearned star nearby, and a tip drawn from §2-§5.
- Race results: the finish table (pos, driver, car, time / gap), then the purse breakdown
  (share, difficulty ×, first win, clean bonus, fee), then the new balance. Buttons: Retry /
  Free roam / Menu.

## 7. Stories

The shape follows `create-stories`: an id, the behaviour, and testable acceptance criteria.
Streams run in parallel against §8. Inside a stream, order = dependency order.

### SKILLS (Style + HUD)

- **SK-1 Skill detector core.** Pure `skills::detect(&mut DetectState, &SkillFrame) -> Vec<SkillEvent>`, no Bevy queries.
  AC: unit tests build `SkillFrame`s from synthetic `Car`s · same frames always give the same events · nothing is emitted while countdown, pause or `down` is set.
- **SK-2 Slide + Switch.** As in §2.2.
  AC: 30° at 60 km/h held 2 s gives 192 ± 5 points · β 14° never starts · a sign flip within 1 s adds 150 · β > 100° voids the slide · the dev autopilot `drift` style on a hairpin produces ≥ 1 Slide.
- **SK-3 Air + landing.**
  AC: 0.29 s off the ground scores nothing · 0.8 s and 20 m scores 320 · a landing with a `hits` increase emits no Air and emits Broken · a roof landing voids it.
- **SK-4 Close Shave + Slipstream** using `OtherCars`.
  AC: passing a traffic car at a 0.5 m gap and 40 km/h closing gives 250; oncoming gives 375 · a 1.3 m gap gives nothing · the same car within 5 s scores once · 10 m behind at 80 km/h for 3 s gives 50 Slipstream (scoring starts after the 1 s hold).
- **SK-5 Splinter, Switchback, Clean Line, Clean Pass, Flat Out.**
  AC: each has a pass and a fail test from §2.2 (e.g. the 13th Splinter in one Rhythm scores 0; 165 km/h on tarmac for 1 s scores 75).
- **SK-6 Rhythm.** Multiplier, variety bonus, cap, 3.5 s timeout, bank, scuff, breaks.
  AC: tests per rule · banking pays `Payout{Rhythm}` = Style/25, capped at 2,000 · Recover breaks it · a wall brush halves M · pause freezes the timeout.
- **SK-7 Style HUD.** Rhythm panel, timeout ring, 3 skill lines, Banked and Broken states, the setting Full / Minimal / Off.
  AC: screenshot via `play.py` with the autopilot in drift style shows the panel · Off hides it and scoring still pays · the HUD despawns on leaving InGame.
- **SK-8 Stats + dev commands.** `skills.state` and `skills.inject` control commands; stats feed `Career.stats`.
  AC: `skills.state` returns P, M, active skills · best_rhythm persists across restarts.

### CHALLENGES

- **CH-1 Placement.** Pure `challenges::place(&RoadNet, seed) -> Vec<ChallengeDef>` with the §3 rules; baked as `ChallengeStart` placeables; snapped to the road on load; editor-movable.
  AC: deterministic · counts 7/5/6/≥3 · every spacing rule tested · the level loads them back unchanged.
- **CH-2 Radar Post.**
  AC: crossing the gate at 120 km/h records 120 ± 1 · off the road records nothing · both directions count · the best is kept.
- **CH-3 Pace Section.**
  AC: mean = length/time within 0.5 % · skipping a mid-gate voids the run · Recover voids it · re-entering the start gate restarts it.
- **CH-4 Slide Section.** Reads `SkillEvent` (raw Slide + Switch points between the gates).
  AC: M is not applied · a damaging hit voids it · the autopilot in drift style reaches ≥ 1★ on a gravel section.
- **CH-5 Yump Board.**
  AC: measured distance matches the take-off→touchdown horizontal distance ± 0.2 m · a take-off > 8 m from the crest is void · a bad landing is void.
- **CH-6 Star bake.** `--bake-challenges` runs the autopilot (Saker, pace 0.85) through each challenge and writes `R` and the thresholds into the level.
  AC: deterministic between runs · thresholds round as §3 · every challenge has 1★ < 2★ < 3★.
- **CH-7 Markers, map, pills, rewards.** Boards, gate stripes, map/minimap icons by star colour, approach and result pills, `Payout{Challenge}`.
  AC: each star pays once (400/600/1,000) · a new best without a new star pays 50 · the map shows all 24 from a fresh career.

### ECONOMY + GARAGE

- **EC-1 Career + Wallet.** `career.json` as in §4.4; `Wallet` resource; one `Payout`/`Charge` path writing a ledger.
  AC: round-trip test · atomic write · a corrupt file → `career.bad.json` + fresh career · the balance never goes negative (a charge is refused).
- **EC-2 Roster + Spec.** `garage::CARS` (8 `CarDef`s), new `Spec.grip` and `Spec.shift_time` (defaults keep today's sim bit-identical), `build_spec(def, &Upgrades)`.
  AC: the Saker stock spec == `Spec::default()` · handling tests still pass · `sim_hash` differs per car.
- **EC-3 PI + class.** `pi(&Spec) -> u16`, `Class`.
  AC: the stock cars hit the §4.1 PIs ±5 and classes · monotonic with each upgrade.
- **EC-4 Upgrades.** 7 parts as in §4.2, class factor, the torque cap.
  AC: each level's Spec delta matches the table · price × factor · engine L3 + turbo L2 is clamped at ×1.35.
- **EC-5 Showroom.** List, stats bars, buy with confirm, sell (60 % + 50 % of upgrades).
  AC: buy debits and adds to the garage · buying an owned car or with too little money is refused · you can't sell your last car.
- **EC-6 Garage + car swap.** Select / Upgrades / Paint / Service; sets `CarSpec`, the body model (rally hatch or a traffic shape) and the paint on spawn.
  AC: the selected car persists · in-world swap only at the Service Park / home · repair price per §4 · event start repairs for free.
- **EC-7 Payout rules.** Purse, shares, difficulty, first win, clean bonus, fee, time-trial pay.
  AC: table-driven tests for every §4 row (e.g. Blackpine, Legend, P2, clean = 4,800×2.5×0.55×1.1 = 7,260) · the fee is charged once per Retry chain.
- **EC-8 Main menu restructure.** Items and flows as in §6 (Continue, Events, Challenges, Garage, Showroom, Logbook); balance + car chip on every screen.
  AC: `tests/meta.rs` reaches each screen headless · Continue restores the saved pose · Go now loads at the event's start line.

### RIVAL DRIVERS (AI racers)

- **AI-1 `ai::Driver` from the autopilot.** A pure input function over (`Road`, corners, progress, `DriverParams`), compiled in release; the dev autopilot becomes a thin wrapper.
  AC: the autopilot tests pass unchanged · the driver compiles without `dev` · same inputs → same output.
- **AI-2 RaceField.** Spawn 5 rival cars (body + paint), step them in `FixedUpdate` after the player, publish `OtherCars` + solids.
  AC: the headless race is deterministic (two runs with the same player inputs → identical rival poses at step N) · the player hits a rival and both take impulses · perf ≤ 1.5 ms per step (logged by `perf.stats`).
- **AI-3 Start formats.** Grid vs Pursuit by the width rule; slots by difficulty; staggered release.
  AC: the Hillclimb picks Grid and Fernhollow picks Pursuit · no car overlaps another or the road edge at spawn · release gaps are exactly 1.5 s (90 steps).
- **AI-4 Racecraft.** Gap keeping, overtake on width ≥ 4.5 m, yielding, seeded mistakes, difficulty table.
  AC: in a 6-car Pro race with the player parked, all 5 finish without retiring · on a 3.4 m road no rival passes on a hairpin · on Rookie a rival yields within 3 s of being caught.
- **AI-5 Light rubber band.** As in §5.
  AC: the pace modifier is never beyond ±3 % · it is 0 in the last 15 % · tested on synthetic gaps.
- **AI-6 Standings HUD.** P x/6, the gaps ahead and behind, rival dots on the progress bar.
  AC: positions follow progress `s` · gaps are within 0.1 s of a ground-truth replay.
- **AI-7 Race flow + results.** Race / Time Trial choice at the marker, difficulty pick, fee charged, finish estimate for rivals still running, results table, `RaceResult` → `Payout`.
  AC: finish order is correct when the player crosses first and when last · a DNF player pays nothing back and the fee is kept · Restart doesn't charge the fee again.
- **AI-8 Recover hold + contact log.** A 5 s hold on Recover in a race; contacts ≥ 15 km/h are logged for the clean bonus.
  AC: Recover places the car at the same `s` ± 2 m and holds it for 300 steps · one bump voids the clean bonus.

## 8. Shared interfaces (agree these first; each stream owns its module)

| Item | Kind | Owner | Shape (suggested) | Read by |
|---|---|---|---|---|
| `OtherCars` | Resource, rebuilt each fixed step | AI (rivals) + Traffic (adds its cars) | `Vec<OtherCar { id: u32, kind: Traffic \| Rival(u8), pos: Vec3, fwd: Vec3, speed: f32 }>` | Skills, HUD, minimap |
| `SkillFrame` | struct | Skills | car snapshot, `β`, surface, nearest road (`curv`, half width, lateral), `&[OtherCar]`, `in_race`, `rival_progress` | Skills |
| `SkillEvent` | Message | Skills | §2.4 enum, `Skill` enum (Slide, Switch, FlatOut, Air, CloseShave, Slipstream, Splinter, Switchback, CleanLine, CleanPass, Challenge) | HUD, Challenges (Slide Section), Career stats |
| `Payout` / `Charge` | Message | Economy | `{ amount: u32, reason: PayReason }`, where `PayReason` = Rhythm \| ChallengeStar{id, stars} \| ChallengeBest{id} \| Race{event, pos, diff} \| TimeTrial{event} \| Sale{car} / Purchase{car} \| Upgrade{car, part, lvl} \| EntryFee{event} \| Service | Any stream emits; only Economy mutates `Wallet` |
| `Wallet` | Resource | Economy | `{ balance: u64 }` + ledger | menus, HUD |
| `Career` | Resource + `career.json` | Economy | §4.4; `Career::save()` debounced | all |
| `CarDef`, `CARS`, `Upgrades`, `build_spec`, `pi`, `Class` | pure module `garage` | Economy | `CarDef { id: &'static str, name, body: Body::{Rally, Hatch, Saloon, Estate, Suv}, price, base: fn() -> Spec, paint }` | AI (rival cars), Challenges (bake car), menus |
| `CarSpec` | Resource (exists) | Economy sets it | `build_spec(...)` of the selected car | sim, ghosts, AI |
| `Spec.grip`, `Spec.shift_time` | new fields | Economy (EC-2), first PR | defaults = today's behaviour | all |
| `ChallengeDef` / `ChallengeStart` | data + placeable | Challenges | `{ id, kind, road, gates: Vec<station>, thresholds: [f32; 3], reference: f32 }` | map, menus |
| `ChallengeResult` | Message | Challenges | `{ id, value, stars, new_best }` | Economy, Skills (Challenge skill), HUD |
| `ai::Driver`, `DriverParams` | pure module | AI | `input(&mut self, &Car, &Spec, &Road, &[Corner], s, others) -> Input` | dev autopilot, challenge bake |
| `RaceField` | Resource | AI | `{ racers: Vec<Racer { car, spec, driver, name, start_step, finished, dnf }>, difficulty, format }` | HUD, results |
| `RaceResult` | Message | AI | `{ event, difficulty, order: Vec<(Entrant, f32 \| DNF)>, player_pos, clean }` | Economy, results screen, Career |
| `Difficulty` | enum | AI | Rookie / Pro / Ace / Legend + the §5 table as consts | menus, Economy |

**Merge order.** EC-2 (the Spec fields), then `OtherCars` + `SkillEvent` + `Payout` stubs in
one small "interfaces" PR. After that the four streams run independently. Every stream keeps
the house rules: a pure core with unit tests, `DespawnOnExit(InGame)` on in-game UI, nothing
visual in headless runs, and no frame time or randomness in `vehicle.rs`.

## 9. As built: Style and challenges (stream F1, Nov 2026)

**Style** (`src/career/style.rs`, tests in `style_tests.rs`). `Detector::step(&StyleFrame, &mut
Vec<StyleEvent>)` is pure; all eleven skills use the §2.2 numbers. How the rules were read:
- A skill adds to P when it *completes*: a Slide when it ends (0.3 s after dropping below),
  an Air 0.6 s after a clean landing, a Close Shave 1 s after the pass (no contact around it).
  Running Slide / Flat Out / Slipstream points live in `Rhythm::live`; they keep the Rhythm
  alive, add +0.2 every 2 s, and are lost on a break.
- A Slide's 0.25 s hold counts towards its points (SK-2: 30° at 60 km/h for 2 s = 192).
  Slipstream's 1 s hold doesn't (SK-4: 3 s behind = 50).
- Scuffs (a knock with no damaging hit, not a Splinter) halve M at most once per 0.5 s.
- Contact with another car = a knock with that car within 3.6 m at ≥ 8 km/h relative speed.
- A recover is seen as a jump in position (the same rule catches respawns and restarts).
- Banks: timeout, pause/map opening, driving into an event marker, a Pace/Slide Section start
  gate. Breaks: hit, car contact, recover, rolled (`up.y` < 0.3), an event start or restart, a
  car swap. Quitting drops the Rhythm silently.
- `StyleEvent::Tick` carries a `StyleTag` (Big Air, Paint Scraper) for the HUD.
- `Payout{Rhythm}` on every bank that pays; grade xp comes from `Bank.points` (F2).
- Dev: `style.state`, `style.inject {skill, points}` (also `bank` / `lose`).

**Challenges** (`src/open/challenges.rs`, tests in `challenges_tests.rs`). The 13 are in
`assets/levels/open/challenges.ron`: road index + stations, with the world x,z of the first and
last gate as a check. A's re-bake moved every road, so the WORLD_PASS §5 anchors were re-picked
on the new roads with the §3 rules (tested: ≥ 500 m between same-kind challenges, gates ≥ 120 m
from junctions and ≥ 50 m from event markers, section lengths). Radars sit at the ends of
≥ 250 m straights (road 0 ×2, 7, 2), sections on the long stretches and hairpin runs (Pace:
roads 6, 3, 7; Slide: 1 tarmac, 2, 4), and the Yump Boards on A's three kickers
(`RoadNet::yumps`). Thresholds are the §3 placeholders until the star bake (C4), except the
Yump Boards: the dev autopilot brakes for crests and never leaves the ground, so each kicker's
R is its clean distance at 120 km/h (Gully: 105, its fastest clean entry) from the straight
sweep `kicker_speeds` (Hillclimb 23 m, Pass 20, Gully 18; stars at 0.72 / 0.88 / 1.0 R).
The clean-landing rule allows 0.3 s of body-on-ground (the springs bottom out on most
landings); longer, a hit, or `up.y` < 0.85 is a bad landing (Air and Yump). After another
re-bake: `PACENOTE_CHALLENGES=reanchor cargo test --features wt_style --lib write_challenges --
--ignored` moves them by their x,z; `the_13_challenges_sit_on_their_roads` fails while a gate is
more than 3 m off.
- Sections run towards increasing stations. A Pace Section has 4 hidden mid-gates (within
  15 m of the centreline), spread evenly. A run voids on a missed gate, a recover, 1 s
  reversing, 3 s off its road, or averaging < 15 km/h. Re-crossing the start restarts it.
- Slide Section = the raw Slide + Switch points between the gates (`Rhythm::slide_meter`); a
  damaging hit voids it.
- Yump Board: take-off within ±8 m of the crest on the road, ≥ 0.15 s in the air, measured
  take-off → first touchdown; a bad landing voids it.
- Each star tier pays once (400 / 600 / 1,000, summed in one `Payout{ChallengeStar}`); a new
  best with no new star pays 50 (`ChallengeBest`). Bests are kept in the career save
  (`CareerProgress::challenges` in `career.json`).
- Markers: gate stripes on the road surface, flag posts, a kind-coloured board, and a radar
  camera whose lamp flashes once (0.25 s) on `RadarFlash`.
- Dev: `challenges.list`, `challenges.state`, `challenges.goto {id, lead, kmh}`.

**HUD.** `hud_style.rs`: the Rhythm panel above the speedo (×M, P, 3 lines, timeout bar,
Banked ✓ / Broken ✕ with the total struck through). It is compact in timed events.
`hud_challenge.rs`: a top-centre card for the approach, live readout (bar with star notches),
result (stars filling 150 ms apart) and void. ★ ✓ ✕ are drawn as images because the fonts
don't have them. Reduce motion turns off the shake, bump, slide-in and star fill. Settings
rows: HUD › Style HUD (Full / Compact / Off), HUD › Challenge cards.

**Sound** (`src/audio/festival.rs`, bus `hud` under `ui`). All AUDIO_PASS §3 festival cues are
synthesised at startup: tick ≤ 15/s, bank −14 dBFS above lost −18, a 3D radar shutter, chimes,
countdown/go, checkpoint, finish, fail and the money roll.

**Not yet.** The star bake (`--bake-challenges`, after A's re-bake). Map and minimap icons
(`src/map` is A's; the data is `open::challenges::Challenges` plus `Bests`). The board's
pictogram and best value. The seeded placer (C2: later).
