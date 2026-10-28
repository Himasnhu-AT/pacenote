# Audio: car and stage (`src/audio/`)

Everything is synthesised in code: no sound files, nothing to download. One plugin,
`audio::RallyAudioPlugin`.

```text
DrivenCar + throttle action + StageRun ─┐  gather (PostUpdate, after SoundSet::Mix)
StageWorld.def (biome/weather/time)     ├─▶ Frame (Copy) ──try_lock per 256-frame block──▶ RallySynth
CameraMode (bonnet = inside)            │    + one-shot events                              (audio thread)
engine Mixer: "car", "ambience" gains ──┘                                                   car + ambience
                                                                                            → bus gain/low-pass
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

**Engine** (`car.rs`). A 4-cylinder, 4-stroke engine: two firings per revolution, so the
firing frequency is rpm/30 Hz (32 Hz at idle, 253 Hz at the 7600 redline). Each firing is
an impulse shaped by two one-poles into a pulse, plus a burst of combustion noise. The pulse
train goes through exhaust formants (115 Hz body, 430 Hz, 1.3 kHz rasp), an rpm/load
low-pass (1.1 → 6.5 kHz) and intake noise. As rpm sweeps, the harmonics move through these
fixed formants, which is what makes the "vowel" change.
- Load (throttle) gives sharper, louder pulses and more rasp. Overrun gives soft, darker
  pulses.
- At idle, cylinder-to-cylinder differences and jitter give a lumpy burble; they even out
  higher up.
- Rev limiter: at the redline under throttle, ignition is cut about half of each 70 ms
  cycle (14 Hz bounce), rpm dips 3 %, and pops are likely.
- Backfire pops: after a lift above 3200 rpm a crackle window opens (decays over ~1.4 s).
  Each firing may then pop: a noise crack, plus a 65–95 Hz boom.
- Gear change: the sim's `shifting` cuts drive and the engine drops to an overrun burble.
  A gearbox clunk plays on each change.
- Gear whine: a sine at input shaft × (17 + 2·gear), louder under load and inside.

**Turbo.** Boost builds with load above about 2.4k rpm (lag 1.3/s) and bleeds off on lift.
The whistle (2.4–7.6 kHz) follows the shaft speed. Lifting with boost above 0.3 gives a
blow-off (band-passed noise falling from 4.2 kHz) and compressor flutter (22 Hz AM).

**Tyres**, by each wheel's surface, load share, rolling speed and slip speed
(`|slip_ratio|·v + |sin slip_angle|·v`):

| Class | Rolling | Sliding |
|---|---|---|
| gravel | crunch grains (70/s per unit) + low rumble | more grains, broadband hiss |
| tarmac | low road roar | tonal squeal 820–1340 Hz with wander, past the grip peak |
| mud | low squelch with a wandering wet resonance | stronger slurp |
| snow | dense high squeaky grains | denser |
| off-road (verge, rough) | grass swish + bumpy rumble | more swish |

Stones ping into the arches (damped sines at 1.8–6.5 kHz, panned) on gravel and off-road.
The rate grows with roll and slide.

**Wind** is low-passed noise whose cut-off rises with speed. The level goes as speed², with
slow gusts.

**One-shots** come from state changes between sim steps (`Tracker::detect`):
- Impacts: `car.impacts` rising by more than 0.4 m/s gives a thud, three inharmonic metal
  rings and a noise burst, scaled by the hit speed.
- Suspension thumps: a wheel compressing faster than 1 m/s and at least 1.8× its previous
  rate, or landing after air.
- Gear clunks.

**Inside the car** (bonnet camera): tyres, wind and ambience are low-passed (down to about
2.5 kHz), the engine is 30 % louder, pops are quieter, and in rain there is a roof drum.

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

## Silence in automated runs

The rally stream starts only when the engine sound backend is `device`. That backend is
`none` with `--mute`, headless, and in test-driven runs (`--control`, `--offscreen`, `--mcp`)
without `--sound`.

## Measurements

Apple M-series, dev profile (game crate at opt-level 1):

| What | Result |
|---|---|
| Rally synth on the audio thread (device, full throttle on stage 1, ambience on) | `dsp_load` **0.58–0.76 %** of one core |
| Same synth on the main thread (`--car-audio-wav`) | 0.27 % |
| Allocation | none per frame. `Frame` is `Copy` and exchanged with `try_lock` once per 256-frame block. The synth is boxed once at start |
| Game frame | gather is a few hundred float ops per frame. The run stayed at 120 fps offscreen (`perf.stats` p50 6.8–8.4 ms) |

## Tests (`cargo test --lib audio`, 22 tests)

- DSP blocks: noise determinism and range, sine approximation, biquad pass/reject, ring
  frequency and decay, envelopes and smoothing, wander bounds, clip and pan, NaN guard.
- Car:
  - deterministic output;
  - silent when inactive, then fades out;
  - levels bounded over rpm × throttle × surfaces, with stacked one-shots and garbage
    input;
  - the spectrum follows rpm (firing harmonics beat in-between and wrong-rpm frequencies);
  - load is louder and brighter;
  - lift gives blow-off and pops;
  - the limiter bounces;
  - every surface rolls, and sliding is louder;
  - tarmac squeal is tonal;
  - wind grows with speed.
- Ambience: every biome × weather × inside/outside is finite and quiet, recipes differ as
  designed, rain is louder than clear, it fades out.
- Stream: the source follows published frames and bus gain (a muted bus gives silence),
  the event queue keeps events the audio thread hasn't taken yet.
- Sim-driven (`sim_drives_the_synth`): 16 s of the real `vehicle::step`, covering full
  throttle through the gears, a lift, and a handbrake slide on gravel. Output is finite and
  ≤ 1, upshifts are detected, and the pitch tracker finds rpm/30 within 8 % in at least 80 %
  of the windows under load.
- Offscreen game run with `input.key` throttle and steer and a camera switch: no panic,
  events counted, WAV written. Its spectrogram shows the firing harmonics following rpm.

## Known limits

- **Guide machine:** the throttle comes from the local `throttle` action. On the guide's
  machine the replicated car would sound off-load. It needs throttle in the replicated car
  state; the networking stream can pass it in, and `car_params` takes it as an argument.
- The car is 2D, centred on the listener. That is fine for the chase and bonnet cameras,
  but there is no doppler or distance for a ghost or other cars (use engine 3D cues for
  those).
- Not in engine bus meters: the engine's `sound.stats` peak meters don't include the rally
  stream. `RallyAudioStats` is the place to look.
- The rally stream doesn't go through the engine reverb (no tunnels exist yet).
- Impacts don't know what was hit (tree or rock): the sim only sums impact speeds.
- The sound is tuned by numbers and spectrograms, not yet by ear on many speakers.

## Engine change that would fold this into the mixer

A "procedural voice" in `engine::sound` would let the rally synth run inside the engine
`Renderer` on the `car` bus, so it would get meters, reverb and limits:
`SoundCtl::play_synth(bus, Box<dyn FnMut(&mut [f32]) + Send>)` plus a parameter channel.
A smaller step would be `SoundCtl::set_pitch(voice, ratio)` (like `set_volume`), which would
allow clip-based layers crossfaded by rpm.
