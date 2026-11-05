# Audio pass: pops and bangs, festival gameplay sounds, speed

Audio director and sound designer, autonomous mode: every decision below has a one-line rationale.
Base: `docs/AUDIO.md`, `src/audio/` (`powertrain.rs`, `car.rs`, `stream.rs`, `traffic.rs`, `car.rs` wind),
`tools/audio/cook_car.py`, the engine `sound` crate (`SoundCue`, buses, `synth::*`).
Rule for sources: CC0 recordings (listed in `assets_src/SOURCES.md`) or synthesis in code. No music.

**Sonic identity:** diegetic first. The car and the place carry the game. Festival layers (skills, money,
menus) are short, dry and tonal: one family of bell or pluck sounds in a shared key (E), never loops,
never louder than the engine at speed.

## 1. Diagnosis: why the current pops are weak

The current pops are in `powertrain.rs` (`Engine::set`, `shift`, `pop`, and the firing loop in `sample`),
mixed in `car.rs:252`. The source is three slices (0.17 to 0.20 s) of one Freesound anti-lag recording
(`cook_car.py` SHOTS `pop1..3`).

| # | Problem | Where | Effect |
|---|---|---|---|
| 1 | The lift is detected per frame: the throttle must drop from ≥ 0.5 to < 0.25 between two published frames (about 16 ms; `set` only sees new values per game frame) | `Engine::set` | A gamepad trigger released over 50 to 150 ms **never** opens the crackle window. Only keyboard lifts and downshifts crackle |
| 2 | Each firing has an independent chance: `0.02 × overrun × crackle`. The window decays exponentially with τ = 1.4 s | firing loop | You get evenly spread random single pops (mean gap 0.2 to 0.5 s), not the clustered burst a listener reads as crackle. At a 3.5k rpm lift: about 2 pops in 1.4 s |
| 3 | Pop gain is `rand(0.5..1) × strength`, multiplied only by `level`. It isn't scaled to the engine's loudness or rpm | `pop()` | A 7k rpm lift sounds the same as a 3.5k one, so pops feel random rather than earned |
| 4 | Pops go through the master compressor (above −7 dBFS, 3:1, 4 ms attack) and the limiter | `stream.rs` | The transient is flattened and the engine pumps under each pop. Pops read as "pff", not "crack" |
| 5 | Only 3 variants, all from one take, ±12 % pitch, high-passed at 40 Hz with no low end | cook table | Repetition is obvious within one burst. There is no sub thump and no body |
| 6 | Inside the car pops are just at half level (`0.5 × inside`), with no muffling | `car.rs` | In the cockpit they sound pasted on, brighter than the engine through the bulkhead |
| 7 | The pan is random (±0.2) and has nothing to do with where the exhaust is | `pop()` | No sense of the exhaust being behind the car in chase view |
| 8 | Pops are decided on the audio thread with its own RNG, and nothing is reported back | — | The visual flame can't be synchronised (and no flame exists yet) |

## 2. Fix: exhaust pops and bangs

**Decision: move the decision to the main thread and make the sound hybrid.** A main-thread
`PopScheduler` decides when each pop happens, sends `ExhaustPop` (visuals listen) and pushes
`CarEvent::Pop` to the synth. The synth plays a recorded crack layer plus a synthesised body and sub.
*Rationale: one source of truth keeps the flame and the sound in sync. Synthesis gives unlimited
variation and the low end the CC0 recording lacks.*

### 2.1 Trigger rules (scheduler, per car, game time)

| Trigger | Condition | Output |
|---|---|---|
| **Lift-off crackle** | Peak throttle in the last 200 ms ≥ 0.6, current throttle ≤ 0.2, rpm ≥ 3200, engine not damaged-dead. Works for any pedal speed (fixes #1) | A burst (see 2.2), intensity `I = smoothstep(3200, redline, rpm_at_lift)` |
| **Upshift bang** | Rising edge of `shifting`, throttle ≥ 0.7, rpm ≥ 0.7 × redline | 1 `Bang` (90 % chance) at `I = 0.7 + 0.3 × rpm_n`, then 0 to 2 crackles within 150 ms |
| **Downshift blip** | Downshift with rpm ≥ 2800 (blip stays as is) | A short burst: 1 to 3 crackles over 0.4 s, `I × 0.7` |
| **Rev limiter** | Each ignition cut starts (`lim_start`) | 35 % chance of one `Crackle` at 0.6 |
| **Damage backfire** | `engine_damage > 0.3` on any lift or shift | Adds 1 `Misfire` bang (chance `dmg²`). Misfire level dips stay on the audio thread |
| **Cancel** | Throttle > 0.35 during a burst | At most one more pop, then the burst ends (fuel burns in the cylinder again) |

Anti-lag flavour: **no continuous ALS.** *Rationale: the engine is a naturally aspirated 190E with no turbo sound, so
constant ALS bangs would contradict what you hear.* The "rally" feel comes from the upshift bang and denser bursts
(car spec `pop_profile: Road | Rally`; Rally doubles the burst count and makes `Bang` likelier).

### 2.2 Burst timing

- Count `N = 2 + round(4 × I)` (2 to 6). Rally profile: `min(6, N + 1)`.
- Window `0.4 + 0.8 × I` s (0.4 to 1.2 s). Pop times are drawn as clusters: gaps uniform 40 to 220 ms, with
  a 30 % chance of a doublet 25 to 45 ms after the previous pop. Minimum spacing 25 ms.
- Per-pop amplitude `a = I × rand(0.6..1.0) × (1 − 0.5 × t/window)` (the burst fades).
- Kinds: the first pop of a burst at `I > 0.7` is a `Bang`; the rest are `Crackle`.
- Global cap per car: 10 pops/s; a burst that would exceed it drops pops.

### 2.3 Sound recipe (audio thread, `PopVoice` pool of 6 per player car, steal the oldest)

Every pop is the sum of four layers, randomised per pop from `seed`. All filters are `dsp::Biquad`
and all envelopes are exponential.

| Layer | Recipe | Crackle | Bang |
|---|---|---|---|
| Crack | White noise burst 1 to 3 ms → resonant low-pass, fc rand(1.8 to 4.5 kHz), Q rand(2 to 4). Decay τ | 8 to 15 ms, 0 dB | 15 to 25 ms, 0 dB |
| Recorded | One of `pop1..3` (kept) at ±15 % rate, start offset rand(0 to 10 ms) | −9 dB | −4 dB |
| Body | Noise → band-pass rand(180 to 450 Hz), Q 1.5, τ | 30 ms, −6 dB | 60 to 90 ms, −3 dB |
| Sub thump | Sine rand(55 to 80 Hz), pitch ×0.7 over 30 ms, τ 50 ms | −15 dB | −2 dB |
| Pipe ring | Comb on the body, delay rand(6 to 9 ms), feedback 0.45 | off | τ 80 ms, −10 dB |

The crack layer uses a different noise seed for each channel (L/R decorrelated, a slightly wide image).

### 2.4 Level, routing, perspective

- Level relative to the engine: the scheduler doesn't set absolute gain. The synth scales by
  `eng_rms` (the 50 ms RMS of the engine's own output). Crackle peaks sit **+8 dB** over `eng_rms` and Bang
  peaks **+13 dB**, with a hard cap at −3 dBFS.
- **Routing: pops are summed after the compressor and before the limiter** (like the co-driver).
  This fixes #4: the transient survives and the engine no longer pumps.
- **Chase:** the pan comes from the exhaust's bearing to the camera, `pan = 0.6 × sin(rel_yaw)`.
  A ground-slap tap is added at 9 ms, −10 dB, low-passed at 3 kHz (an outdoor reflection).
- **Cockpit/bonnet:** the whole pop is low-passed at 1.4 kHz, −5 dB, the sub thump +3 dB (felt through the
  shell), and the body `shell` rattle (`body.rs`) is triggered at `a × 0.5` for Bangs. Crossfade on the existing
  `interior` value.
- **Guide machine / remote cars:** the same scheduler runs on `RemoteCarSound` (rpm, gear, throttle). The pop
  pool is shared with tier-A AI cars (section 4).
- **Visual sync:** the scheduler emits `ExhaustPop` on the frame the pop is due and pushes
  `CarEvent::Pop` the same frame. Audio latency is one block plus the device buffer (≈ 10 to 25 ms); the flame
  shows on the next rendered frame (16 to 33 ms). Target offset ≤ ±40 ms. The flame uses `intensity` and
  `kind` for size, and `seed` for shape, so a big bang gets a big flame.

## 3. Festival gameplay sounds

Unless marked *diegetic*, these play through the engine mixer as `SoundCue`s, with clips synthesised at startup
in a new `audio/festival.rs` (no assets). Tonal sounds are in E (E5 = 659 Hz). New bus `hud`, with `ui` as its parent: the UI
volume slider covers it, and it can be ducked under the co-driver separately from menus.

| Sound | Trigger (message) | Recipe | Peak | Bus, limit |
|---|---|---|---|---|
| Skill tick | `SkillEvent::Tick` | Pluck: sine + 0.3 × 2nd harmonic, 1 ms attack, τ 60 ms, 659 Hz × 2^(min(chain−1,12)/12): it climbs a semitone per chain level | −22 dBFS | hud. Max 15/s: ticks past the cap merge into the next one at +1 dB |
| Chain bank | `SkillEvent::Bank` | Bell arpeggio (FM ratio 3.5, index 2→0 over 300 ms) at the chain pitch: 3, 4 or 5 notes 50 ms apart for small, medium or large banks (< 1k, < 10k, ≥ 10k), plus a 70 Hz thump of 80 ms | −14 | hud, 1 instance, stops pending ticks |
| Chain lost | `SkillEvent::Lost` | Saw glide 600→150 Hz over 350 ms through a 1.2 kHz low-pass, plus a low-passed noise puff (120 ms). Deflating, not a crash | −18 | hud, cancels ticks |
| Speed trap flash | `SpeedTrap` | *Diegetic*, `play_at(pos)`: a shutter clack (two 4 ms noise bursts band-passed at 3 kHz, 25 ms apart), then a flash-capacitor whine (chirp 3.2→8 kHz, 250 ms, −30 dB under the clack) | −10 at 5 m | sfx, spatial, 1 |
| Speed trap readout | `SpeedTrap`, +150 ms | Two-note chime: up a fifth if the target is beaten, up an octave plus a sparkle if it is a personal best, down a minor third if neither | −16 | hud |
| Countdown | `ActivityCue::Countdown(n)`, `Go` | 880 Hz sine-square blend (LP 3 kHz) for 120 ms per count, then 1760 Hz for 350 ms on Go. The stage countdown is silent today, so this fixes it for rally stages too | −14 | hud |
| Checkpoint | `ActivityCue::Checkpoint` | 2-note pluck up a fourth, 90 ms | −18 | hud |
| Finish | `ActivityCue::Finish` | 4-note rising arpeggio, saw through a low-pass sweeping 800 Hz→4 kHz (brassy), 1.2 s. Place 1 or a best time adds a fifth note | −12 | hud, ducks car (section 5) |
| Fail / DNF | `ActivityCue::Fail` | 2 notes falling a minor third, dull (LP 1.5 kHz), 500 ms | −16 | hud |
| Money | `MoneyEarned` | A tick roll: ticks of 6 ms (inharmonic partials 3.1 and 4.7 kHz), at most 30/s, lasting `min(1.2, 0.3 + 0.15·log10(amount))` s, pitch rising +5 semitones across the roll. It ends with a bell at 1.3 kHz plus 2.6 kHz (τ 400 ms) | −18 | hud, 1 (a new amount extends the roll) |

**Main menu UI** (bus `ui`, never ducked by the car, all synthesised):

| Sound | Recipe | Peak | Limits |
|---|---|---|---|
| `Hover` | 12 ms tick: 2.2 kHz plus a 3.3 kHz partial, τ 18 ms, ±0.5 semitone jitter | −26 | 40 ms cooldown, 2 instances |
| `Select` | 110 Hz thump (40 ms), a 1.6 kHz click (8 ms) and two notes E5→B5 (triangle, LP 4 kHz, 45 ms each) | −16 | 1 |
| `Back` | Select reversed (B5→E5), LP 2.5 kHz | −18 | 1 |
| `Transition{forward}` | Pink noise for 380 ms, band-pass 400 Hz→2.8 kHz (Q 0.8; downward when going back), fade-in 220 ms and fade-out 160 ms, pan sweeping from −0.4 to +0.4 | −20 | 1, steal |
| `Error` | 2 × 90 Hz square, 60 ms, LP 800 Hz | −20 | 150 ms cooldown |

**Loading screen: no music pad.** It plays the destination's ambience bed (`ambience::Recipe::of` the stage
def, already known at load start) at −8 dB with a 1.5 s fade-in. When loading ends it rises to full level over
1 s, so it carries straight into the stage. *Rationale: a cheap synthesised pad reads as a placeholder; the
place is the game's identity and costs nothing.* The main menu uses the same idea: the backdrop's ambience at
−10 dB, car bus muted.

## 4. AI racers and traffic: voice budget and LOD

The budget is 8 external car voices in total (traffic plus AI). In races, traffic is off and AI cars get all 8.
They are ranked each frame by `loudness = speed_term / max(d, 3 m)`. Hysteresis: a car changes tier
only if its rank changes by 2 or more, or it has held its tier for 0.5 s. Promotion and demotion crossfade over 150 ms.

| Tier | Count | Within | Engine | Extras |
|---|---|---|---|---|
| A | 2 | 80 m | "powertrain-lite": the car's own rpm and throttle from its sim, the 2 nearest loops of the on/off sets (4 loops read at most), and a load crossfade | Pops through the shared pool (half the burst count), tyre slide when slipping, Doppler, pan, air absorption (the existing `traffic.rs` code) |
| B | up to 6 | 200 m | `traffic.rs` voice: one loop, pitched by the car's **rpm** (not a gearbox guess), gain by throttle | Doppler, pan, absorption. No pops |
| C | the rest | 400 m | One shared "pack" voice: the 2.5k on-load loop through a low-pass at 600 Hz, gain √Σ(1/d²) of the C cars | Gives a sense of the field, costs one voice |

- Variety without new assets: each AI car gets a fixed per-car tilt EQ (±3 dB shelf at 1.5 kHz), a loop-set
  offset (±1 loop, about 16 % in timbre), and the exhaust-facing cue: −4 dB on a 2 kHz shelf when its tail points away
  from the listener. This gives the missing front/back cue.
- Pops on AI cars come from the same `PopScheduler` (main thread), so their flames work too.
- Inside the player's car, external cars are muffled as traffic is today (−5 dB, LP).

## 5. Wind, speed and the mix

**Current state:** the wind exists (`car.rs`). Outside it is low-passed noise with fc `180 + 28·v` Hz, plus hiss
band-passed at `1500 + 40·v`, with level `(v/40)²` capped at 1.5 (saturates at about 176 km/h) and slow gusts. Inside it is
buffeting plus a seal whistle. It is neither tied to the FOV kick (`drive.rs speed_fov`) nor to the motion blur, and
there are no pass-by cues.

**Decision: a shared `SpeedFeel` (0..1) resource** = the smoothstep already in `speed_fov` (40→180 km/h).
FOV, motion blur and audio all read it, so the senses ramp together. *Rationale: if sight and sound ramp at
different speeds, the speed feels fake.*
1. Wind: add a high "air" band (band-pass at 6 kHz, Q 0.7) at `0.4 × SpeedFeel²`. Extend the level cap to 2.0 (so it keeps growing to
   220 km/h). Inside, the seal whistle's band follows `2.2 + 1.5 × SpeedFeel` kHz.
2. **Pass-by whooshes** (new, synthesised, the strongest speed cue): a roadside object (tree, pole, post, wall end,
   parked car) that passes within 5 m laterally at > 70 km/h fires a noise burst. Its band-pass sweeps down 2.5 kHz→900 Hz over
   `clamp(4 m / v, 60..150 ms)`, panned to the object's side. Level ∝ `speed² / lateral`, at most −14 dBFS. Limit 8/s,
   pool of 4. Inside: −8 dB, LP 1.5 kHz. Source: roadside instance positions near the car (main thread),
   pushed as `CarEvent::PassBy{side, strength}`.
3. Speed comes from brightness and event density, not volume: total car loudness may rise by at most 3 dB from 100 to
   200 km/h at constant rpm.
4. No rolling road or stones noise: it stays out (removed as tiring, 2 Nov).

**Priority and ducking** (highest first): co-driver › impacts › player engine and pops › finish/activity stingers ›
other `hud` sounds › AI and traffic › wind, tyres and pass-bys › ambience. UI is on its own.

| When | Car | Ambience | hud | Fade down / back |
|---|---|---|---|---|
| Co-driver talks (existing) | −5 dB | −3 dB | **−6 dB (new)** | 30 ms / 250 ms |
| Finish or Fail stinger | −4 dB | −3 dB | — | 80 ms / 1.2 s |
| Chain bank, speed trap, money | none (they are short and sit above the engine's band) | — | — | — |
| Pause / menu over a drive | existing pause snapshot (LP) | — | — | existing |
| Main menu, loading | muted | −10 / −8 dB bed | — | 300 ms |

Loudness targets: driving −20 to −23 LUFS integrated (unchanged); `hud` peaks ≤ −12 dBFS; UI ≤ −16 dBFS;
no clipped samples; pops are never ducked and never duck anything.

## 6. Event interface (gameplay → audio, Bevy `Message`s)

```rust
// audio crate owns these; gameplay writes, audio (and art for ExhaustPop) reads.
#[derive(Message, Clone, Copy)] pub struct ExhaustPop { pub car: Entity, pub intensity: f32, pub kind: PopKind, pub seed: u32 }
#[derive(Clone, Copy)] pub enum PopKind { Crackle, Bang, Misfire }        // sent BY audio::PopScheduler
#[derive(Message, Clone, Copy)] pub enum SkillEvent { Tick { points: u32, chain: u8 }, Bank { points: u32, chain: u8 }, Lost { points: u32 } }
#[derive(Message, Clone, Copy)] pub struct SpeedTrap { pub pos: Vec3, pub kmh: f32, pub target_beaten: bool, pub personal_best: bool }
#[derive(Message, Clone, Copy)] pub enum ActivityCue { Countdown(u8), Go, Checkpoint, Finish { place: u8, best: bool }, Fail }
#[derive(Message, Clone, Copy)] pub struct MoneyEarned { pub amount: u32 }
#[derive(Message, Clone, Copy)] pub enum UiSound { Hover, Select, Back, Transition { forward: bool }, Error }
// synth side (stream.rs CarEvent, MAX_EVENTS 16 is enough: ≤ 3 per frame):
CarEvent::Pop { car_slot: u8, intensity: f32, kind: PopKind, seed: u32, pan: f32 }
CarEvent::PassBy { side: f32, strength: f32 }
```

- Remove from `powertrain.rs`: the lift detection in `set`, the per-firing chance `0.02 × overrun × crackle`, the
  upshift bang in `shift` and the limiter pop. Keep the blip, misfire dips and limiter level bounce. `crackle`
  and `popping()` are replaced by the scheduler (update the tests `lift_off_pops` and
  `upshift_under_power_bangs...` to drive the scheduler).
- Gameplay never picks sound parameters. It sends facts; the audio crate maps them. Dropped messages are harmless.
- Test hook: `RallyAudioStats` gains `pops_scheduled`, `pops_played`, `passbys`, `ext_voices[A,B,C]`, `hud_cues`.

## 7. CPU and memory budget

| Part | Mean per 256-frame block | Notes |
|---|---|---|
| Today (car, bus, dynamics) | ~35 µs | from `docs/AUDIO.md` |
| Pop pool (6 voices × about 6 biquads, active only) | ≤ 3 µs | idle at 0 |
| Pass-by pool (4) and air band | ≤ 2 µs | |
| AI tier A (2) | ≤ 8 µs | 4 loop reads + filters each |
| AI tier B (6) and C (1) | ≤ 9 µs | about 1.2 µs per `traffic.rs` voice |
| **Rally stream total** | **≤ 60 µs mean, ≤ 0.35 ms worst (≈ 1.1 % RT)** | Dev profile, M-series, `render_scenes` |
| `hud`/UI cues | engine renderer | ≤ 8 simultaneous, within the engine's 48-voice cap |

Memory: no new recordings. Festival clips synthesised at startup: < 1 MB. No allocation on the audio thread
(pools are fixed arrays, as today).

## 8. Acceptance criteria

1. Lift at 6000 rpm (keyboard **and** an analog lift ramped over 150 ms): 2 to 6 pops within 1.2 s in ≥ 90 % of 50
   seeded runs. A lift at 3500 rpm gives 2 or 3 pops; no pops below 3200 rpm (scene `lift_off_pops`, a new pad variant).
2. Upshift at ≥ 0.7 × redline under full throttle: a Bang in ≥ 85 % of shifts. Downshift from 4k: ≥ 1 crackle.
3. Peak level in 10 ms windows: Crackle +6 to +10 dB and Bang +11 to +15 dB over the engine's 50 ms RMS (chase).
   Compressor gain reduction caused by a pop ≤ 1 dB. No clipped samples, true peak ≤ −1 dBFS.
4. In one burst no two pops are identical (the spectral centroid of each pair differs by > 5 %, or the length by > 10 %).
5. Cockpit pops: centroid ≤ 0.6 × chase, and sub energy (< 100 Hz) ≥ chase.
6. `ExhaustPop` count equals `pops_played` (±0). In a capture (`--car-audio-wav` plus frame log), the audio onset minus
   the flame frame is within ±40 ms.
7. Skill ticks never exceed 15/s; hover never re-fires within 40 ms; Bank stops pending ticks; Lost is never louder than Bank.
8. `SpeedFeel` drives FOV, blur and wind from the same value (unit test). Pass-bys: ≤ 8/s, none below 70 km/h; at 150 km/h
   on a tree-lined road, 2 to 6 per second.
9. 12 AI cars on the grid: at most 8 external voices plus 1 pack voice; no tier change clicks (no 20 ms high-band
   spikes); rally stream mean ≤ 60 µs per block.
10. Co-driver line during a chain bank: `hud` is −6 dB and the voice stays intelligible (speech ≥ 8 dB over the rest,
    measured in the 300 Hz to 3 kHz band).
11. Loading → stage: the ambience is continuous (no gap > 50 ms, no step > 3 dB). The menu has no car sound.
12. Every new sound is synthesised in code or CC0 and listed in `assets_src/SOURCES.md`. A listening pass on headphones,
    laptop speakers and a TV is written up in `docs/AUDIO.md` (measurements table) before merge.
