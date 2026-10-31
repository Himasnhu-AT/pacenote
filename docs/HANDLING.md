# Handling

How the car in `src/vehicle.rs` is set up, where the numbers come from, and how it's measured.
The harness and the tests are in `src/handling_tests.rs`.

## The car

A ~1,250 kg 4WD rally car, Group A / R5 class (Fabia R5, i20 R5, Polo GTI R5: 1,230 kg
minimum, ~2.55 m wheelbase, 1.55–1.6 m track, ~210 kW from a restricted 1.6 l turbo).

| | Value | Why |
|---|---|---|
| Mass | 1,250 kg | R5 minimum 1,230 kg plus fluids |
| Centre of mass | 0.48 m off the ground, 54 % front | Hatchbacks are 0.5–0.55 m; rally cars sit lower |
| Track / wheelbase | 1.56 m / 2.55 m | R5 homologation figures |
| Static stability factor | track / (2·h) ≈ 1.6 | More than any tyre's grip (≤ 1.2), so flat ground can't roll it |
| Inertia (pitch, yaw, roll) | 1,850 / 2,000 / 520 kg·m² | Yaw ≈ m·(0.4·L)²; published sedan yaw inertias are 1,800–2,600 |
| Springs | 42 kN/m, ride ≈ 1.8–1.9 Hz | Gravel cars run ~1.5–2 Hz, tarmac cars 2.5+ |
| Helper springs | 8 cm extra droop at 8 kN/m | A spring alone only droops its static deflection, g/ω² ≈ 7 cm |
| Dampers | 3.6 kN·s/m, 0.7× bump, 1.3× rebound | Damping ratio ~0.35 bump / 0.6 rebound |
| Anti-roll bars | 13 / 7 kN/m (≈ 58 % roll stiffness front) | Stable under power, loose only on a lift or the handbrake |
| Bump stop | Progressive past 24 cm of bump travel | Heavy landings don't bottom straight onto the body |
| Steering | 35° lock, 3 rad/s | ~11 m kerb-to-kerb circle (rally cars run extra lock for hairpins) |
| Gearing | 5 speeds, final drive 5.1 | Gravel ratios: ~55 km/h in 1st, ~195 km/h at the limiter |
| Diffs | 50 : 50, plated LSDs (coupling 60/s) | R5: mechanical front and rear LSDs, no active centre |
| Engine | 360 N·m peak at ~4,200 rpm, ~210 kW peak at ~6,700 rpm | R5: ~280–290 hp on a 32 mm restrictor |
| Engine braking | 16 % of peak torque at the redline | Lifting moves weight forward: that's what makes a lift rotate the car |

## Tyres

One grip curve over the *combined* slip: the slip ratio and the slip angle are each divided by
their peak slip, and the force points along the combined slip. A locked or spinning wheel
(big slip ratio) therefore gives up its side force, as a real tyre does. The old model computed
each force separately and clipped them to an ellipse, so a locked rear kept most of its side
grip and the handbrake did nothing.

The curve rises with slope 2 (cornering stiffness 2·μ·Fz / peak slip, ~16 Fz/rad on tarmac),
is flat at the peak, then eases down to `tail` by 3× the peak slip. Load sensitivity takes
~8 % of μ off per extra static load.

| Surface | μ | Peak slip angle | Peak slip ratio | Tail |
|---|---|---|---|---|
| Tarmac | 1.15 | 7° | 0.10 | 0.78 |
| Gravel | 0.85 | 11° | 0.15 | 0.88 |
| Snow (studs) | 0.48 | 10° | 0.14 | 0.85 |
| Mud | 0.58 | 12° | 0.17 | 0.85 |
| Verge | 0.62 | 12° | 0.17 | 0.82 |
| Rough | 0.55 | 13° | 0.18 | 0.85 |

Sources: Pacejka, *Tyre and Vehicle Dynamics* (dry tarmac peaks at 4–8° slip angle and ~0.1
slip ratio). Loose-surface tyre tests put gravel at ~0.8–0.9 g reached at 10–15° and staying
flat past it. Studded tyres on packed snow give ~0.4–0.5. `Surface::grip()` in `stage.rs` still
gives the stage-level μ used by the co-driver and the harness's driver; the car reads
`vehicle::tyre()`.

## Steering and input

`vehicle::steer_frame` decides where input puts the road wheels:
- **Lock at speed:** full input asks for the front tyres' peak slip angle (`steer_slip` × peak,
  for the surface under the front wheels), measured from the direction the front axle is
  travelling. When parking it's full lock, with a kinematic term fading out by ~20 km/h.
  - Drifting nose-in, a held key can't overwork the fronts. Fixed to the body, it put them at
    twice their peak slip at 100 km/h on gravel, and the car yawed 33 → 1 → 19°/s.
  - Sliding nose-out, full opposite input still points the wheels past the direction of
    travel.
- **Centred input** follows the front axle's travel halfway (`ALIGN` = 0.5) above ~55 km/h,
  fading to straight below ~20 km/h. A real car's caster and self-aligning torque turn the
  wheels into a slide when the driver's hands relax. With the wheels held straight, keyboard
  play never caught a slide; with full alignment the car wobbled after letting go.
- **Keyboard** (`drive::keyboard_steer`): keys ramp the input, taking 0.2 s to full at hairpin
  speed and ~0.6 s at 110 km/h. Letting go, or switching sides, unwinds faster. A gamepad stick
  passes straight through. The ghost records the ramped value.
- **Drivetrain:**
  - the clutch slips at launch (up to ~4,500 rpm), so the engine doesn't bog at idle;
  - gravel gearing (final drive 5.1, ~195 km/h at the limiter);
  - plated limited-slip diffs (coupling 60/s);
  - no upshift on a lift unless at the limiter.

`PACENOTE_CARLOG=<file>` makes the game write every sim step (input, attitude, rates, gear,
per-wheel load / slip / compression) to a CSV file, for reviewing real play.

## Collisions

- **Ground:** the body box (offset up from the centre of mass, bumpers raised for approach and
  departure angles) is tested at a 3×2×3 grid of points.
- **Trees and boulders:** each is a circle tested against the box's whole footprint, so a trunk
  can't slip between sample points.

## Measured (before → after)

Same harness and same path-following driver for both.

| | Before | After |
|---|---|---|
| Turning circle, kerb to kerb | 12.1 m | 11.1 m (tarmac), 11.2 m (gravel) |
| Max lateral g, tarmac | rolled over first (≥ 0.8 g) | 1.0 g, 2.4° of roll |
| Max lateral g, gravel / mud / snow | 0.68 / 0.49 / 0.35 | 0.77 / 0.51 / 0.43 |
| Gravel, 90 km/h, full input | 0.67 g, 208 m radius | 0.74 g, 99 m radius |
| Ramp steer to full lock, flat tarmac, 50–110 km/h | rolls every time | slides, never above 2.4° of roll |
| Handbrake turn, 50 km/h gravel (with vs without) | 45° vs 43° | 86° vs 50° |
| Handbrake turn, tarmac / snow | 50° vs 47° / 28° vs 29° | 99° vs 53° / 53° vs 30° |
| Lift-off at 70 km/h, gravel yaw rate | 24.7 → 24.7°/s | 20.3 → 26.3°/s |
| Stages at pace 0.55 (finished / rollovers / wrecked) | 19 / 1 / 1 | 20 / 0 / 0 |
| Stages at pace 0.7 | 9 / 14 / 10 | 18 / 0 / 2 |
| Stages at pace 0.85 | 4 / 15 / 16 | 9 / 0 / 11 |
| 0–100 km/h, tarmac / gravel / snow | — | 4.1 / 4.6 / 7.2 s |
| One 60 Hz step (8 substeps, dev build) | — | 2.2 µs |

Notes on the table:
- **Pace:** the driver corners at that share of the surface's grip.
- **Before, on stages:** the rolls happened on the road, before the car left it.
- **After, on stages:** every wreck follows a tree hit at 18+ m/s after running wide, a trip.
- **What caused the old rollovers:**
  - The centre of mass sat 0.87 m off the ground, because the body box was centred on it.
    That gave a stability factor of 0.88, below tarmac grip.
  - Only 7 cm of droop meant wheels left the road over every dip.

## Keyboard review (revision 4)

Measured with a keyboard driver: on/off keys through the game's ramp, speed held with the
pedals.

| | Revision 3 | Revision 4 |
|---|---|---|
| Key held, gravel 100 km/h: yaw | 33 → 1 → 19°/s swinging, fronts at 21° slip | turns at 0.75 g (15.8°/s path), fronts at peak |
| Key held, gravel 70 km/h: path turn rate | 13.4°/s | 21.7°/s (0.75 g) |
| Key held, tarmac 100 km/h: path turn rate | 22°/s, 53 % yaw overshoot | 20°/s (1.0 g), 47 % |
| Key held at 25 km/h: radius (gravel / tarmac) | — | 7.5 / 6.1 m |
| Letting go after a held key, gravel 100 km/h | wheels straight to the body, slide not caught | wheels half follow the travel, settles in 1.3 s |
| Launch: rpm 0.5 s in | 1,600 (engine bogging at idle) | 4,500 (clutch slips) |
| 0–100 km/h, gravel / tarmac | 4.6 / 4.1 s | 4.4 / 3.9 s |
| Top speed | 223 km/h | 193 km/h |
| Straight braking from 100 km/h, gravel / tarmac | 0.85 / 1.02 g, dead straight | 0.85 / 1.07 g, dead straight |
| Keyboard driver at a steady pace: stages finished | 14 / 20 | 18 / 20 (no untripped rolls) |
| Gearbox: shifts reversed within 0.7 s | — | 2 in 547 |

## Same bits on every machine (revision 5)

Ghosts replay inputs through the sim, and boards only rank runs on the same road hash, so the
car and the stage generator must give bit-identical results on every CPU. They now do:
- Every transcendental in `vehicle.rs` and `stage.rs` goes through the `libm` crate. std's
  sin / cos / atan2 / exp / powf differ between Apple arm64, x86_64 and Windows.
- The quaternion step is spelled out, because glam's NEON and SSE2 `normalize` round
  differently.

Before the change, a car diverged from step 12 between arm64 and x86_64, and 10 of the 20
stage hashes differed. Checked with a standalone build of `vehicle.rs` and the stage generator
on arm64 and x86_64 (Rosetta): a 600 s scripted drive and all 20 stage hashes are identical.
The tests `scripted_drive_is_bit_identical` and `stages_hash_the_same_everywhere` pin the
values. Run `cargo test` on another CPU and they must still pass.

**Cross-CPU check on the real crate:** run the golden tests as an x86_64 build under Rosetta,
then natively. The x86 target dir is already compiled, so a rerun takes about a minute.

```sh
SMTP_USER= SMTP_PASS= CARGO_TARGET_DIR=/Users/himanshu/codes/game/pacenote-build/target-x86 \
  memgate -- cargo test -q --target x86_64-apple-darwin --lib -- scripted_drive_is_bit_identical stages_hash_the_same_everywhere obstacles_the_same_everywhere
SMTP_USER= SMTP_PASS= memgate -- cargo test -q --lib -- scripted_drive_is_bit_identical stages_hash_the_same_everywhere obstacles_the_same_everywhere
```

Both pass with the pinned values (scripted drive `0x41d0f4247faaf7b8`). If the sim changes on
purpose, re-pin `SCRIPTED_HASH` in `handling_tests.rs`, bump `vehicle::REVISION` and rerun both.

**Quick check without building the game:** `tools/simcheck/run.sh`. It builds `tools/simcheck`, which compiles the
game's own `src/vehicle.rs` and `src/stage.rs` (no copies, no Bevy: a glam shim) for arm64 and
for x86_64. It runs both, x86_64 under Rosetta, and compares the scripted-drive hash
(`vehicle::check::scripted_hash`) and all 20 stage hashes. Exit 0 means identical. It takes
about 30 s, and needs `rustup target add x86_64-apple-darwin` once. (A step costs ~3.5–4.7 µs,
about twice what it did before the determinism and audit fixes; still negligible.) Last result: identical,
vehicle `41d0f4247faaf7b8`.

**Not covered:**
- **Windows** is unverified. It should match: it takes the same SSE2 code and pure-Rust libm
  paths as x86_64 macOS, but run `cargo test` there once before shipping.
- **Obstacle placement** (trees and rocks the car can hit) goes through pcg-core. Its
  rotations (`Rect` clearing, yaw transforms) use libm, as does `world.rs` for the road
  rectangles' yaw. `obstacles_the_same_everywhere` pins a hash of every stage's obstacle list.
  Identical on arm64 and x86_64 both before and after the libm change, so the change is
  insurance.
- **Heightfield sampling** (engine `Heightfield::height_at` / `smooth_normal_at`) is plain
  arithmetic plus `Vec3::normalize`, which matched bit for bit in the audit's per-op tests. Keep glam's
transcendental helpers (`from_rotation_*`, `from_scaled_axis`, `to_angle`, `from_angle`,
`Quat::normalize`) out of sim code.

Other fixes from the physics audit:
- engine braking works rolling backwards too;
- the bump stop has damping (a 7 m/s landing no longer bounces the car into the air);
- hill hold, the handbrake and the foot brake pin a stopped car (it crept 1–5 m a minute on
  slopes);
- implicit wheel-spin update (spin flipped sign every substep below ~7 m/s, feeding slip noise
  to ABS, traction assist and audio);
- the suspension ray is measured to the ground plane on slopes;
- inputs are sanitised, with a guard against a non-finite state;
- caps on penetration force and spin rate;
- no stale slip on airborne wheels;
- the recover key is latched once per frame (it was lost or doubled depending on the frame
  rate).
- the anti-roll bar sees an airborne wheel at full droop, and the brakes stop a wheel in the
  air.

Skipped from the audit:
- **L1:** an implicit gyroscopic step. Spin is capped at 25 rad/s instead, which stops the
  blow-up (normal crashes stay under 15 rad/s). The few-percent energy gain in long free
  flight was left.
- **L4:** engine and drivetrain inertia (7–10 kg·m² per wheel in 1st). It would soften
  wheelspin and revs, but it changes launch and shift feel, which was just tuned. Peak power
  was fixed (360 N·m → ~210 kW).
- **Grip-limited braking:** braking stays torque-limited (1.10 g on tarmac, about the tyre
  limit anyway).

## Dev autopilot

In dev builds, `src/autopilot.rs` drives the car for recording videos. It uses the same input
path as the player (`drive::AutoInput`), so the ghost records it and the cockpit animates it.
Control port commands:
- `autopilot.set {on, pace: 0..1.2, style: "clean" | "drift", crash_at_m}`
- `autopilot.state`

Drift style flicks the handbrake into grade 1–2 hairpins on loose ground and powers out. For a
clean slide, use stage 15 (gravel hairpins from ~6 s in) at pace 0.55–0.6.

## Running the harness

```sh
memgate -- cargo test -q --lib handling_tests            # the locked-in tests
memgate -- cargo test -q --lib -- --ignored --nocapture --exact vehicle::handling_tests::report
PACE=0.7 memgate -- cargo test -q --lib -- --ignored --nocapture --exact vehicle::handling_tests::stage_report
KEYS=1 PACE=0.55 memgate -- cargo test ... vehicle::handling_tests::stage_report       # keyboard driver
memgate -- cargo test -q --lib -- --ignored --nocapture --exact vehicle::handling_tests::player_report
STAGE=8 TRACE=700,800 memgate -- cargo test ... vehicle::handling_tests::stage_report   # per-step trace
STAGE=8 TRACE=700,800 memgate -- cargo test ... vehicle::handling_tests::road_profile   # road cross-section
```

The tests cover:
- the turning circle;
- lateral g per surface;
- full input being useful at speed;
- no rolls on flat ground (ramp steer, constant radius, slalom);
- handbrake and lift-off rotation;
- crests landing on the wheels;
- a sideways slide into a bank tripping the car (while the same slide on open ground doesn't);
- every stage clean at pace 0.55, and a small budget at 0.7 where any roll must follow a hit.

## Ghosts and boards

- **Ghosts:** a ghost stores `meta::ghost::sim_hash`, which hashes the `Spec` and
  `vehicle::REVISION` (now 5). `meta::pick_ghost` only races ghosts with the current hash,
  because an old ghost would replay its inputs through the new car and drive somewhere else.
- **Boards:** each board `Record` stores `handling` (`vehicle::REVISION`; files from before
  load with 0). Only current-handling runs rank, count as a team's best or lead a stage. Older
  runs list in the stale section of `Board::listing`.
- **Changing the handling:** bump `vehicle::REVISION`. That invalidates ghosts and moves old
  times to stale in one place.
