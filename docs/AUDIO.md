# Audio: car and stage (`src/audio/`)

Everything is synthesised in code: no sound files, nothing to download. One plugin,
`audio::RallyAudioPlugin`.

```text
DrivenCar + throttle action + StageRun ─┐  gather (PostUpdate, after SoundSet::Mix)
StageWorld.def (biome/weather/time)     ├─▶ Frame (Copy) ──try_lock per 256-frame block──▶ RallySynth
CameraMode (bonnet = inside)            │    + one-shot events                              (audio thread)
engine Mixer: "car", "ambience" gains ──┘                                                   car + ambience
                                                                                            → DC block, bus gain/low-pass
                                                                                            → compressor → limiter
                                                                                            → soft clip → rodio
```

The engine's sound voices play clips only, with no live pitch control, so the car runs in
its own rodio source (`RallyStream`) next to the engine mixer's. Each frame it copies the
engine mixer's bus gains and low-passes, so settings, `sound.bus` mute/solo, the pause
snapshot and voice ducking all apply to it.

## Buses

| Bus | Parent | What |
|---|---|---|
| `car` | `sfx` | engine, turbo, tyres, wind, impacts (added at startup) |
| `ambience` | master (built-in) | wind bed, birds, insects, rain |
| `voice` | master (built-in) | the partner's voice (stream "voice"), volume from `SoundSettings::voice_volume` |
| `ui` | master (built-in) | menus |

Ducking: voice chat plays in its own rodio source (branch `voice`). While the partner talks,
it turns on the mix snapshot `voice_chat` (`sfx` −4 dB, `ambience` −6 dB, fades 0.12 s in and
0.7 s out). `Mixer::effective_gain` includes snapshot dB and the parent chain, so `car` (under
`sfx`) and `ambience` duck through the gains this stream already mirrors, with no extra hook.
The engine's default duck rule (the same depths, keyed on engine voices on `voice`) also still
applies.
Volumes come from `CoreSettings` (master/sfx/ambience/ui) through the engine.

## The sounds

Files: `powertrain.rs` (engine, turbo), `tyres.rs`, `body.rs` (impacts, thumps, scrapes,
rattles), `car.rs` (whine, wind, inside/outside mix), `dsp.rs` (building blocks),
`stream.rs` (bus gains and master dynamics), `scenes.rs` (offline renders, tests only).

**Engine** (`powertrain.rs`). A 4-cylinder, 4-stroke engine, firing order 1-3-4-2: two
firings per revolution, so the firing frequency is rpm/30 Hz (32 Hz at idle, 253 Hz at the
7600 redline).
- Each cylinder's exhaust pulse is a shape over crank angle (~55–80° wide, `x(1−x)²` round
  on overrun blended to `x(1−x)⁵` sharp under load), so its harmonics stay on engine orders
  at any rpm. Every cylinder has its own gain, timing and width offset: the half-orders
  (rpm/60 multiples) that make it gruff, ~25–35 dB under the firing orders.
- A short "bark" at each firing adds bite under load. It is placed with sub-sample timing
  (split over two samples), so there is no timing jitter at high rpm.
- The pulses run through two pipe waveguides (a short header, a ~2.5 m tail pipe: fixed
  resonances the harmonics sweep through, the "vowel"), exhaust formants (110 Hz, 420 Hz,
  1.35 kHz rasp) and a muffler low-pass that opens with load and rpm. A 28 Hz high-pass
  removes the pulse train's DC.
- Intake roar (suction noise, an airbox resonance at 360 Hz) and valve-train ticks are a
  separate output, louder inside the car.
- Overrun: softer, rounder pulses. After a lift above 3200 rpm a crackle window opens
  (decays over ~1.4 s) and firings may pop: a noise crack plus a 60–95 Hz boom.
- Rev limiter: at the redline under throttle, ignition is cut about half of each 70 ms
  cycle (14 Hz bounce), rpm dips 3 %, and pops are likely.
- Gear change: the sim's `shifting` cuts drive (overrun burble). An upshift under power above
  4200 rpm bangs (75 %) and chuffs the blow-off valve. A downshift blips the throttle for
  90 ms and opens the crackle window. A dog-box clunk plays on every change.
- Damage (`engine_damage`): misfires (up to ~18 % of firings under load at full damage),
  each leaving fuel that may bang on a later firing; rod knock (1.1–1.9 kHz rings) above
  0.25; a breathy manifold leak above 0.2; the idle hunts.
- Gear whine: input shaft × (17 + 2·gear) plus its octave, louder under load, much louder
  inside.

**Turbo** (`powertrain.rs`). Boost builds with load above about 2.4k rpm (lag 1.3/s) and
bleeds off on lift. The whistle (2.4–7.6 kHz, a blade-pass tone plus a weaker partial at
×1.52) follows the shaft speed, with air hiss. Lifting with boost above 0.3 gives a blow-off
(band-passed noise falling from 4.2 kHz) and compressor flutter (22 Hz AM).

**Tyres** (`tyres.rs`), by each wheel's surface, load share, rolling speed and slip speeds.
Sideways sliding (slip angle) counts about twice as much as wheelspin or a locked wheel at
the same slip speed, so a handbrake slide stands out from a straight-line launch.

| Class | Rolling | Sliding |
|---|---|---|
| gravel | crunch grains (resonant clicks 0.7–3.8 kHz, 320/s at roll 1), compression crunch, rumble | scrabble (1400 grains/s more), spray hiss, ploughing roar |
| tarmac | tread hum (~1 kHz band) + low road roar | squeal: PolyBLEP saw (~600 Hz) with stick-slip chatter and wander through two resonances; scrub |
| mud | low squelch with a wet resonance, rising "bubble" chirps | slurp, many more bubbles |
| snow | squeaky tonal grains (1.1–2.9 kHz) + soft crunch | denser squeak, hiss |
| off-road (verge, rough) | grass swish + bumpy rumble | more swish |

Stones ping into the arches (1.8–6.5 kHz) and thud on the floor (260–720 Hz with a floor-pan
resonance) on gravel, off-road and snow. The rate grows with roll and much more with slide.

**Wind.** Outside: low-passed noise whose cut-off rises with speed. Inside: low buffeting and
a door-seal whistle. Both go as speed², with slow gusts.

**Body and one-shots** (`body.rs`), from state changes between sim steps (`Tracker::detect`):
- Impacts: `car.impacts` rising by more than 0.4 m/s. A thud (lower and longer for bigger
  hits), a panel crunch, three inharmonic metal rings, then debris tinkling for ~0.5 s
  after hits over 6 m/s. All scaled by the hit speed.
- The body hitting the ground (`Knocks::count` rising with no obstacle hit, or a new body
  touch at speed): the same, darker, with less metal.
- Scrape: while `body_contact`, gritty noise with two panel modes ringing, scaled by speed.
- Suspension thumps: a wheel compressing faster than 1 m/s and at least 1.8× its previous
  rate, or landing after air. Bottoming out: a wheel reaching full travel while still
  compressing gives a metallic clonk on the bump stop.
- Rattles: suspension activity (mean |compression speed|) triggers small panel rattles,
  much more with suspension damage; a damaged car's heat shield buzzes.

**Inside vs outside** (bonnet/cockpit vs chase camera, crossfaded over 50 ms):
- Inside, the exhaust comes through the bulkhead (low-passed at 750 Hz, plus a 118 Hz cabin
  boom).
- Inside, tyres and outside wind are low-passed at 1.7 kHz.
- Inside, intake, valve train, knock, gear whine, scrapes and rattles are louder, and stones
  on the floor are twice as loud as outside.
- Outside, pops and the exhaust are direct and full-band.
- In rain there is a roof drum (ambience).
- Measured on the same run: above 2 kHz is −6.8 dB inside, below 300 Hz is +3.2 dB.

**Master** (`stream.rs`): a 22 Hz DC blocker on the car, the bus gains, then a gentle
compressor (above ≈ −7 dBFS, 3:1, 4 ms / 180 ms) and a 1 ms look-ahead peak limiter
(ceiling ≈ −1 dBFS), then a soft clip as the last guard. Normal driving stays under the
compressor; impacts are caught.

**Co-driver.** While the co-driver's system voice speaks (`CoDriver::speaking`), the car
ducks to 0.63 (≈ −4 dB). Voice chat ducks through the mixer snapshot as before.

**Ambience** (`ambience.rs`, `Recipe::of`):

| Biome | Bed | Life |
|---|---|---|
| pine forest | light wind, leaf rustle | songbirds 0.45/s |
| birch hills | wind, more rustle | songbirds 0.6/s, crickets at dusk |
| scrub | light wind | cicadas (62 Hz pulsed 5.6 kHz buzz), few birds |
| moor | strong gusty wind | curlew-like rising calls, rare birds |
| snow forest | quiet wind | rare birds |

Birds sing more in the morning and less at dusk. Overcast, fog and rain thin out the life;
rain adds hiss and drops. Every combination stays quiet (RMS 0.0004–0.12 before the bus
gain; tested).

## Control and debugging

- `ecs.res.get {"name": "RallyAudioStats"}` returns:
  - `output` (`device` / `capture` / `off`);
  - `dsp_load` (share of real time spent in the rally synth);
  - rpm, throttle, gear and speed as the synth sees them;
  - bus gains;
  - counters for impacts, thumps and shifts;
  - roll and slide per surface class.
- `sound.bus {"name": "car", "mute": true}` and the other engine `sound.*` commands work on
  the `car` and `ambience` buses.
- `--car-audio-wav <path> [--car-audio-secs N]` (dev builds only) renders the same synth on
  the main thread in step with game time and writes a 16-bit WAV. This works in
  muted/offscreen runs:

  ```sh
  python3 -I ../engine/scripts/play.py steps.json --manifest Cargo.toml --bin pacenote --id pacenote \
      --out /tmp/run -- --car-audio-wav /tmp/run/car.wav --car-audio-secs 20
  ```
- To hear it on the device from a scripted run: `play.py ... --sound -- --sound`.
  `--mute-output` runs the full DSP at zero gain.
- `PACENOTE_AUDIO_WAV=/tmp/sim.wav cargo test --lib sim_drives_the_synth` drives the real
  car sim and the synth together and writes `sim.wav` plus `sim.wav.csv` (frame, rpm,
  throttle).
- `cargo test --lib report_levels -- --ignored --nocapture` prints the levels of each part.
- `PACENOTE_AUDIO_OUT=<dir> cargo test --lib audio::scenes::render_scenes -- --ignored
  --nocapture` renders twelve scripted scenes through the real car sim (`scenes.rs`):
  idle, full throttle through the gears, lift-off pops, a handbrake slide on gravel, a
  tarmac skid, hitting a post, a damaged engine on rough gravel, a crest jump, cockpit vs
  chase, snow then mud, and a roll. It writes a WAV and a CSV per scene and prints the cost
  per block.

## Silence in automated runs

The rally stream starts only when the engine sound backend is `device`. That backend is
`none` with `--mute`, headless, and in test-driven runs (`--control`, `--offscreen`, `--mcp`)
without `--sound`.

## Measurements

Apple M-series, dev profile (game crate at opt-level 1):

| What | Result |
|---|---|
| Car synth + bus + dynamics, per 256-frame block (5.3 ms of audio), scenes renderer | **~23 µs mean, ≤ 49 µs worst: 0.43 % of real time.** Before the 31 Oct rework: ~11 µs (0.2 %) |
| Same synth with ambience on the main thread (`--car-audio-wav`, stage 3) | `dsp_load` 0.64 % |
| Allocation | None per frame. `Frame` is `Copy` and exchanged with `try_lock` once per 256-frame block. Delay lines and voice pools are fixed arrays inside the synth, which is boxed once at start |
| Levels over the twelve scenes | RMS −18.5 to −21 dBFS (idle −34), peaks ≤ 0.73, no clipped samples, DC < 0.001 |

## Tests (`cargo test --lib audio`, 33 tests + 2 ignored reports)

- DSP blocks:
  - noise, sine approximation, biquads, ring decay, envelopes and smoothing, wander;
  - PolyBLEP saw (no alias fold-back), delay line;
  - dynamics: quiet signals pass untouched, a 4× hit stays under the ceiling;
  - clip and pan, NaN guard.
- Car:
  - deterministic; silent when inactive, then fades out;
  - levels bounded over rpm × throttle × every surface × damage × scrape, with stacked
    one-shots and garbage input;
  - the spectrum follows rpm: firing harmonics beat in-between and wrong-rpm frequencies;
    half-orders are present but well below;
  - low rpm is pulses with the pipe ringing, not a click train (crest factor, busy share);
  - load is louder and brighter;
  - lift gives blow-off and pops; an upshift under power bangs; a downshift blips;
  - the limiter bounces;
  - every surface rolls and sliding is louder; tarmac squeal is harmonic; mud is the
    darkest surface, snow brighter than tarmac;
  - a damaged engine misfires (per-firing spread ×2) and knocks (knock band ×3);
  - scrape, rattles with suspension damage, impacts scaled by speed, bottoming out are heard;
  - inside is darker and the gear whine louder;
  - an rpm sweep with throttle stabs and slides has no sample-to-sample jumps, and the
    tail decays to exact zeros (no denormals);
  - wind grows with speed.
- Ambience: every biome × weather × inside/outside is finite and quiet, recipes differ as
  designed, rain is louder than clear, it fades out.
- Stream:
  - the source follows published frames and bus gain (a muted bus gives silence);
  - the co-driver duck lowers the car by its factor;
  - the event queue keeps events the audio thread hasn't taken yet.
- Sim-driven (`sim_drives_the_synth`): 16 s of the real `vehicle::step`. Output is finite and
  ≤ 1, upshifts are detected, and the pitch tracker finds rpm/30 within 8 % in at least 80 %
  of the windows under load.

## Known limits

- **Guide machine:** the guide has no `DrivenCar` and no `StageWorld`. `net_car_sound` copies
  `guide::sync::NetCar` from the `Remote` entity (rpm, gear, the driver's throttle, speed)
  into `audio::RemoteCarSound`, and the synth plays that: engine, turbo, pops and wind. There
  are no tyres, thumps or impacts, because the replicated state has no surface, slip or
  impacts. Ambience comes from `GuideStage.def`.
- The car is 2D, centred on the listener. That is fine for the chase and bonnet cameras,
  but there is no doppler or distance for a ghost or other cars (use engine 3D cues for
  those).
- Not in engine bus meters: the engine's `sound.stats` peak meters don't include the rally
  stream. `RallyAudioStats` is the place to look.
- The rally stream doesn't go through the engine reverb (no tunnels exist yet).
- Impacts don't know what was hit (tree or rock): the sim only sums impact speeds. Scrapes
  against obstacles aren't heard: the sim only reports the body touching the ground.
- The synth runs at a fixed 48 kHz; rodio resamples to the device rate.
- The sound is tuned by numbers and spectrograms, not yet by ear on many speakers.

## Engine change that would fold this into the mixer

A "procedural voice" in `engine::sound` would let the rally synth run inside the engine
`Renderer` on the `car` bus, so it would get meters, reverb and limits:
`SoundCtl::play_synth(bus, Box<dyn FnMut(&mut [f32]) + Send>)` plus a parameter channel.
A smaller step would be `SoundCtl::set_pitch(voice, ratio)` (like `set_volume`), which would
allow clip-based layers crossfaded by rpm.
