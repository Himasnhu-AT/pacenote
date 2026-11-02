# Audio: car and stage (`src/audio/`)

The car plays recordings (engine, tyres, impacts, scrapes, pops: `assets/sounds/car/`, all
CC0); wind, stones, suspension thumps, rattles and the stage ambience are synthesised in
code. One plugin, `audio::RallyAudioPlugin`.

```text
DrivenCar + throttle action + StageRun ─┐  gather (PostUpdate, after SoundSet::Mix)
StageWorld.def (biome/weather/time)     ├─▶ Frame (Copy) ──try_lock per 256-frame block──▶ RallySynth
CameraMode (bonnet = inside)            │    + one-shot events                              (audio thread)
engine Mixer: "car", "ambience" gains ──┘                                                   car + ambience
                                                                                            → DC block, bus gain/low-pass
                                                                                            → compressor → limiter
                                                                                            → soft clip → rodio
```

The engine's sound voices play clips only, with no live pitch control, so the car (its
recordings played at a live rate, mixed with the synthesised parts) runs in its own rodio
source (`RallyStream`) next to the engine mixer's. Each frame it copies the
engine mixer's bus gains and low-passes, so settings, `sound.bus` mute/solo, the pause
snapshot and voice ducking all apply to it.

## Buses

| Bus | Parent | What |
|---|---|---|
| `car` | `sfx` | engine, tyres, wind, impacts (added at startup) |
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

Files: `samples.rs` (the recordings: loading, loop and one-shot players), `powertrain.rs`
(engine), `tyres.rs`, `body.rs` (impacts, thumps, scrapes, rattles), `car.rs` (wind,
inside/outside mix), `dsp.rs` (building blocks), `stream.rs` (bus gains and master dynamics),
`scenes.rs` (offline renders, tests only).

**Recordings** (`samples.rs`). 22 CC0 recordings from Freesound, cooked by
`tools/audio/cook_car.py` into `assets/sounds/car/*.ogg` (mono, 48 kHz, Ogg Vorbis, 868 KB;
sources, authors and the parts used are in `assets_src/SOURCES.md`). At startup, when the
output is a device or `--car-audio-wav`, a worker thread reads and decodes them
(`samples::start_loading`, from the engine's `AssetsDir`, so dev runs and shipped builds both
find them); until then the recorded layers are silent. A missing file logs a warning and
plays as silence. Loops are made seamless when cooked (the tail crossfaded into the head;
engine loops cut to whole firing periods). `Looper` plays a loop at any rate with 4-point
Hermite interpolation; `Shots<N>` plays up to N one-shots. Unit tests load the bank from this
crate's `assets/` on first use. To change a sound: edit the tables at the top of
`cook_car.py` and run it with a Python that has numpy (`/opt/homebrew/bin/python3 -I
tools/audio/cook_car.py --report`; needs ffmpeg and oggenc).

**Engine** (`powertrain.rs`). A Mercedes-Benz 190E 2.3-16V (2.3 l 16-valve 4-cylinder)
recorded on a dyno at steady rpm, with two mics at once: the engine bay (above the headlight:
intake, valve train, block) and the exhaust. Five loops, measured from their firing frequency
(rpm/30 Hz):

| Loop | rpm | Load |
|---|---|---|
| `idle` | 1006 | none (bottom of both sets) |
| `off_lo`, `off_hi` | 2431, 6297 | none (neutral, free revving) |
| `on_lo`, `on_hi` | 2556, 6208 | full |

- Every loop plays all the time at `rpm / its rpm`, so all of them sit on the car's rpm; a
  loop's two mics share one play position (they were recorded together).
- By rpm: an equal-power crossfade in log rpm between the two loops of a set that bracket the
  car's rpm (the furthest a loop is pitched is ×1.55 up or ×0.65 down, mid-way between 2.5k
  and 6.3k). Above 6.3k the top loop is pitched up (×1.21 at the 7600 redline).
- By load: an equal-power crossfade between the full-load and no-load sets, on the smoothed
  throttle (30 ms up, 60 ms down). Loudness grows with rpm (idle about 10 dB under the top)
  and load (the overrun about 6 dB under full load at the same rpm).
- Rev limiter: at the redline under throttle, ignition is cut about half of each 70 ms cycle
  (14 Hz bounce): the level drops to 0.35, load goes to zero, rpm dips 3 %, and a pop is
  likely at each cut.
- Gear change: the sim's `shifting` cuts load (overrun). An upshift under power above 4200
  rpm bangs (75 %). A downshift blips the throttle for 90 ms and opens the crackle window. A
  dog-box clunk plays on every change (synthesised, `body.rs`).
- Pops: three recorded anti-lag bangs, picked at random with ±12 % pitch. After a lift above
  3200 rpm a crackle window opens (decays over ~1.4 s) and each firing may pop on the
  overrun (2 % × overrun × crackle).
- Damage (`engine_damage`): misfires (up to ~28 % of firings at full damage): the level
  drops 85 % for about a firing, and the unburnt fuel may bang on a later firing. The idle
  hunts (rpm wobble).
- A stopped engine (rpm below ~300) is silent.

**Tyres** (`tyres.rs`), by each wheel's surface, load share, rolling speed and slip speeds.
Sideways sliding (slip angle) counts about twice as much as wheelspin or a locked wheel at
the same slip speed, so a handbrake slide stands out from a straight-line launch. Each loop is
read twice, half a loop apart, for decorrelated left and right channels. Rolling loops were
recorded at low speed and play faster with the car (gravel ×0.8 at rest to ×1.5 at 40 m/s).

| Class | Rolling | Sliding |
|---|---|---|
| gravel | `gravel_roll`: tyres on a gravel road, mic at the wheel | `gravel_slide`: nine dirt skids joined |
| tarmac | `tarmac_roll`: highway tyre roar from inside a car | `tarmac_squeal`: a tonal squeal at ~1 kHz, past the grip peak, pitch wandering and rising a little with slip |
| mud | `gravel_roll` slowed and low-passed at 650 Hz | `gravel_slide` low-passed at 900 Hz |
| snow | `snow_roll`: tyres creeping over snow and ice (faster when sliding) | plus `gravel_slide` low-passed at 2.4 kHz |
| off-road (verge, rough) | `gravel_roll` low-passed at 1.8 kHz, plus a synthesised bumpy rumble | `gravel_slide` low-passed at 1.6 kHz |

Stones ping into the arches (1.8–6.5 kHz) and thud on the floor (260–720 Hz with a floor-pan
resonance) on gravel, off-road and snow, synthesised. The rate grows with roll and much more
with slide.

**Wind** (synthesised). Outside: low-passed noise whose cut-off rises with speed. Inside: low
buffeting and a door-seal whistle. Both go as speed², with slow gusts.

**Body and one-shots** (`body.rs`), from state changes between sim steps (`Tracker::detect`):
- Impacts: `car.impacts` rising by more than 0.4 m/s. Hits under ~4 m/s are the light
  `impact_light` clank; bigger ones are the `impact_heavy` metal crunch (lower and louder for
  bigger hits) with the clank under it, then debris tinkling (synthesised) for ~0.5 s after
  hits over 6 m/s.
- The body hitting the ground (`Damage::hits` rising with no obstacle hit, or a new body
  touch at speed): the same recordings at 0.72× pitch (darker, slower).
- Snapping a sapling, post or sign: a recorded branch crack, and a few chips.
- Scrape: while `body_contact`, the `scrape` loop (metal on a hard surface), faster and louder
  with speed.
- Suspension thumps, bottoming out, rattles, a damaged car's buzzing heat shield, brush
  swishing along the sides and the gear clunk stay synthesised (short rings and filtered
  noise).

**Inside vs outside** (bonnet/cockpit vs chase camera, crossfaded over 50 ms):
- Inside: the engine bay mic through the bulkhead (low-passed at 2.6 kHz), the exhaust
  low-passed at 850 Hz plus a 118 Hz cabin boom; pops at half level.
- Outside: mostly the exhaust mic, with the engine bay under it (0.4).
- Inside, tyres and outside wind are low-passed at 1.7 kHz; scrapes and rattles are louder,
  and stones on the floor are twice as loud as outside.
- In rain there is a roof drum (ambience).

The turbo (whistle, blow-off, flutter) and the gear whine were synthesised tones and were
removed with the move to recordings: the recorded engine is naturally aspirated, and no CC0
recording of a blow-off valve or a straight-cut gearbox was found.

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
| Car (recordings + synthesised parts) + bus + dynamics, per 256-frame block (5.3 ms of audio), scenes renderer | **~35 µs mean, ≤ 0.23 ms worst: 0.65 % of real time.** The all-synthesised car before 2 Nov: ~23 µs (0.43 %) |
| Same with ambience on the main thread (`--car-audio-wav`, stage 3), synthesised car | `dsp_load` 0.64 % |
| Recordings in memory | 22 sounds, ~95 s of mono 48 kHz f32: ~18 MB, decoded once on a worker thread |
| Allocation | None per frame. `Frame` is `Copy` and exchanged with `try_lock` once per 256-frame block. Players and voice pools are fixed arrays inside the synth, which is boxed once at start; the recordings are read from a `&'static` bank |
| Levels over the twelve scenes | RMS −20.6 to −23 dBFS (idle −36.5), peaks ≤ 0.73 (−2.7 dBFS), no clipped samples, DC < 0.0001, no dropouts (silencedetect), no clicks: no 20 ms window has a high-band spike over 12× its own level except at backfire pops |

## Tests (`cargo test --lib audio`, 37 tests + 2 ignored reports)

- DSP blocks:
  - noise, sine approximation, biquads, ring decay, envelopes and smoothing, wander;
  - PolyBLEP saw (no alias fold-back), delay line;
  - dynamics: quiet signals pass untouched, a 4× hit stays under the ceiling;
  - clip and pan, NaN guard.
- Car:
  - deterministic; silent when inactive, then fades out;
  - levels bounded over rpm × throttle × every surface × damage × scrape, with stacked
    one-shots and garbage input;
  - the pitch follows rpm, on and off load, at the recorded rpms and between them: firing
    harmonics (rpm/30 × 1..6) beat those of an rpm 18 % off either way;
  - load and rpm make it louder;
  - a lift pops; an upshift under power bangs; a downshift blips;
  - the limiter bounces;
  - every surface rolls and sliding is louder; mud is the darkest surface;
  - a damaged engine misfires (spread of the level per two firings ×1.5);
  - scrape, rattles with suspension damage, impacts scaled by speed, bottoming out are heard;
  - inside is darker;
  - an rpm sweep through every crossfade with throttle stabs and slides has no
    sample-to-sample jumps, and the tail decays to exact zeros (no denormals);
  - the level holds through the crossfades (no step over 3 dB between 150 ms windows on a
    slow 1000–7400 rpm sweep, on and off load);
  - wind grows with speed.
- Engine crossfade weights are equal power and never pitch a loop past its neighbour.
- Recordings: every sound loads, is longer than 0.1 s and within ±1; a loop wraps smoothly;
  a one-shot plays once.
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
  into `audio::RemoteCarSound`, and the synth plays that: engine, pops and wind. There
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
- The engine has only three rpm points per load. Between 2.5k and 6.3k the two loops are
  pitched up to ×1.55 / down to ×0.65, so mid-range timbre is a blend of a sped-up low loop
  and a slowed-down high one. A recording with steady loops at ~4000 rpm (on and off load,
  same car) would make the middle more natural. Partial throttle is a blend of the full-load
  and no-load sets.
- The engine is a naturally aspirated 190E: there is no turbo sound. The dyno mics are the
  engine bay and the exhaust; the cockpit is built from them with filters (there is no
  recording from inside this car).
- Tyre recordings were made at low speed (gravel, snow) or in a road car (tarmac roar) and
  are sped up and levelled in code; the gravel slide is a dirt skid.
- Stones, suspension thumps, rattles, the gear clunk and wind are still synthesised.

## Engine change that would fold this into the mixer

A "procedural voice" in `engine::sound` would let the rally synth run inside the engine
`Renderer` on the `car` bus, so it would get meters, reverb and limits:
`SoundCtl::play_synth(bus, Box<dyn FnMut(&mut [f32]) + Send>)` plus a parameter channel.
A smaller step would be `SoundCtl::set_pitch(voice, ratio)` (like `set_volume`): the
recorded layers could then be engine voices on the `car` bus. Until then they are played by
`samples.rs` inside the rally stream.
