# Audio: car and stage (`src/audio/`)

The car plays recordings (engine, tyres, impacts, scrapes, pops: `assets/sounds/car/`, all
CC0) and the co-driver speaks from a phrase bank (`assets/sounds/voice/`, public domain); wind, stones, suspension thumps, rattles and the stage ambience are synthesised in
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

**Recordings** (`samples.rs`). CC0 recordings from Freesound, cooked by
`tools/audio/cook_car.py` into `assets/sounds/car/*.ogg` (63 files: 52 engine loops and 11
others; mono, 48 kHz, Ogg Vorbis, ~1 MB;
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
intake, valve train, block) and the exhaust. The session (Chippy569's "Dyno Sessions: 190E",
all CC0) has steady recordings at only three rpm per load (idle ~1000, ~2500, ~6300); its
sweeps are 2-10 s transient pulls with each mic from a different take, so they aren't used.
The game has 26 loops per mic, ~16 % apart:

| Set | rpm (recorded in **bold**, the rest re-synthesised) |
|---|---|
| both | **1006** (idle) |
| full load (`on_*`) | 1450, 1670, 1925, 2218, **2556**, 2963, 3434, 3980, 4613, 5347, **6208**, 7200 |
| no load (`off_*`) | 1166, 1351, 1566, 1815, 2103, **2431**, 2849, 3339, 3913, 4586, 5374, **6297**, 7300 |

- The points between recordings are made at cook time by pitch-synchronous overlap-add
  (PSOLA): each firing of the two recordings of that load that bracket the point (weighted in
  log rpm) is cut out, centred on its pulse, with a Hann window two firing periods long (of the
  shorter period), and laid down again at the new firing period; then the level is ridden flat
  (120 ms). Unlike resampling, the pulse shapes, and so the intake and exhaust resonances, stay
  where they were recorded. Derived loops are 1.3 s, recordings 1.5-2.6 s, all whole firings,
  each starting on a firing's pulse.
- One firing clock drives every loop (loop `i` plays firing `clock mod its firings`), so all
  loops sit on the car's rpm and their pulses line up: loops added together morph the firing's
  shape instead of beating. A loop is played at `rpm / its rpm`: the louder of the two heard
  is never more than ±8 % off its rpm (between 1.5k and the redline), the quieter at most
  ±16 %. Only loops with gain read samples (about four of the 26).
- By rpm: a crossfade in log rpm between the two loops of a set that bracket the car's rpm,
  with weights whose 1.5-norm is 1 (between equal gain, right for pulses in phase, and equal
  power, right for noise: within ~2 dB either way). Below idle and above the top loop (7200 /
  7300) the end loop is pitched (×1.06 at the 7600 redline).
- By load: an equal-power crossfade between the full-load and no-load sets, on the smoothed
  throttle (30 ms up, 60 ms down). Loudness grows with rpm (idle about 10 dB under the top)
  and load (the overrun about 6 dB under full load at the same rpm).
- Rev limiter: at the redline under throttle, ignition is cut about half of each 70 ms cycle
  (14 Hz bounce): the level drops to 0.35, load goes to zero, rpm dips 3 %.
- Gear change: the sim's `shifting` cuts load (overrun). A downshift blips the throttle for
  90 ms. A dog-box clunk plays on every change (synthesised, `body.rs`).
- Pops and bangs (`pops.rs`, festival AUDIO_PASS §2): decided on the **main thread** by
  `PopScheduler` (game time, seeded), which sends `ExhaustPop` (the flame, `art::exhaust`)
  and `CarEvent::Pop` (the synth) on the same frame. Triggers: a lift (peak throttle in the
  last 200 ms ≥ 0.6, now ≤ 0.2, ≥ 3200 rpm; any pedal speed, so a gamepad trigger let go over
  150 ms crackles) gives a burst of 2-6 clustered pops over 0.4-1.2 s (Rally profile +1, a
  Bang first above ~5600 rpm); an upshift at full throttle above 0.7 × redline bangs (90 %)
  plus 0-2 crackles; a downshift above 2800 rpm 1-3 crackles; 35 % of limiter cuts crackle;
  engine damage > 0.3 adds a Misfire (chance dmg²). Throttle back on ends a burst after one
  more pop; at most 10 pops a second.
- Each pop is synthesised (`PopVoices`, 6 voices, oldest stolen): a 1-3 ms noise crack into a
  resonant low-pass (decorrelated L/R), one of the three recorded pops (±15 %, −9/−4 dB), a
  band-passed body, a falling sub thump and, on bangs, a pipe-ring comb, every layer drawn
  from the pop's seed. Its own peak over its first 25 ms is measured on a copy so it lands at
  **+8 dB (Crackle) / +13 dB (Bang) over the engine's 50 ms RMS** (± 2 dB with intensity,
  capped at −3 dBFS). Pops are mixed **after the compressor, before the limiter**
  (`Dynamics::run_with`), never ducked. Chase: panned from the tail pipe's bearing, plus a
  9 ms ground slap; inside: low-passed at 1.4 kHz, −5 dB, the sub +3 dB, and a bang knocks
  the shell.
- Damage (`engine_damage`): misfires (up to ~28 % of firings at full damage): the level
  drops 85 % for about a firing. The idle hunts (rpm wobble).
- A stopped engine (rpm below ~300) is silent.

**Tyres** (`tyres.rs`): drift only. Rolling road noise (gravel crunch, tarmac roar, snow) and
the synthesised stones were removed on 2 Nov 2026 (the gravel crunch was tiring): a car that
grips is quiet and the engine carries the sound. Sliding plays by each wheel's surface and slip
speeds; sideways sliding (slip angle) counts about twice as much as wheelspin or a locked wheel
at the same slip speed. Each loop is read twice, half a loop apart, for decorrelated left and
right channels; it fades out below ~6 m/s.

| Class | Sliding |
|---|---|
| gravel | `gravel_slide`: nine dirt skids joined |
| tarmac | `tarmac_squeal`: a tonal squeal at ~1 kHz, past the grip peak, pitch wandering and rising a little with slip |
| mud | `gravel_slide` low-passed at 900 Hz |
| snow | `gravel_slide` low-passed at 2.4 kHz, a little faster |
| off-road (verge, rough) | `gravel_slide` low-passed at 1.6 kHz |

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
  low-passed at 850 Hz plus a 118 Hz cabin boom; pops muffled (see above).
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

**Co-driver** (`voice.rs`, `codriver/speech.rs`). The co-driver speaks from a phrase bank, not
a text-to-speech engine, so it sounds the same on macOS and Windows:
- 136 clips in `assets/sounds/voice/` (mono 24 kHz Ogg Vorbis, ~1 MB; `bank.txt` lists what
  each says): numbers 0-59 (times, splits; 1-6 double as the grades in "tightens one"), distances 60-190 by 10
  and 200-1000 by 50 ("one fifty", "two hundred"), the note words (left, right, hairpin, square, tightens,
  opens, long, over crest, crest into, don't cut, narrows, off camber, careful, into, flying finish, flat out, ready, …), times (split, minute,
  point, seconds, stop, stage time), damage ("Big hit!", "the engine", "is badly damaged",
  "nurse it home", "it's pulling left", …) and recovery ("We're on the roof.", "Press R to get
  back on the road.", …).
- Rendered once by `tools/audio/cook_voice.py` with Piper (neural TTS, build time only) and the
  `en_GB-cori-high` voice (public domain: trained from scratch on LibriVox recordings; see
  `assets_src/SOURCES.md`). Each clip is trimmed (15 ms before the speech, 40 ms faded tail)
  and levelled (speech RMS −17 dBFS, peaks ≤ −1.5 dBFS).
- Main thread (`VoiceBank::plan`): the line's text → clip ids and pauses. Longest phrase
  first ("over crest" before "over"); commas pause 110 ms, full stops 230 ms; a number the bank
  lacks is read digit by digit; an unknown word is skipped (logged once at debug), never an
  error. The pace setting (195/230/265 wpm) scales pauses and overlaps. The line's length is
  known from the clips, so the queue moves on without asking the audio thread.
- Audio thread (`VoicePlayer`, inside the rally stream): a new serial in the `Frame` starts a
  line (cutting the last off with a 12 ms fade); each word starts while the last one's faded
  tail plays (35 ms overlap: a short crossfade); mixed in the centre after the car's
  compressor, before the soft clip. Gain: co-driver volume × the engine mixer's `voice` bus.
- Ducking: while it talks the car (engine, tyres, wind, impacts) goes down 5 dB and the
  ambience 3 dB (30 ms down, 250 ms back). Voice chat ducks through the mixer snapshot as before.
- `--os-tts` (dev): the old system voice (`say`, `espeak`, PowerShell) instead; the car ducks
  the same while it speaks.
- Not loaded yet, or no audio output: lines are timed by word count and show as subtitles only.

**Traffic** (`traffic.rs`, open world). The four nearest traffic cars within 140 m (`traffic_heard`, from the camera and the player's velocity) each get a voice: the recorded on-load 1925 rpm exhaust loop pitched by a road-car gearbox from the car's speed (1.7-4.4k rpm), tyre roar (band-passed noise rising with speed), inverse-distance gain (1 at 3 m), air absorption (low-pass 11 kHz / (1 + d/28 m)), Doppler from the closing speed and equal-power pan by bearing; muffled and −5 dB inside. A voice stays with its car (by id) and is only handed to another once silent. A car passing at 3 m at 80 km/h peaks ~5 dB over the player's idle (`render_places`).

**Ambience** (`ambience.rs`, `Recipe::of`):

| Biome | Bed | Life |
|---|---|---|
| pine forest | light wind, leaf rustle | songbirds 0.45/s |
| birch hills | wind, more rustle | songbirds 0.6/s, crickets at dusk |
| scrub | light wind | cicadas (62 Hz pulsed 5.6 kHz buzz), few birds |
| moor | strong gusty wind | curlew-like rising calls, rare birds |
| snow forest | quiet wind | rare birds |

Open world places (`places_near`, quantised to 1/32): near the river (1 on the bank, 0 from ~120 m) a babble of two band-noise streams with an uneven swell and popping bubbles; in the village (houses within 90 m) a low hum, a dog barking now and then, and half the forest birds and leaves.

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
- `PACENOTE_AUDIO_OUT=<dir> cargo test --lib audio::scenes::render_previews -- --ignored
  --nocapture` writes rpm sweeps (full load and no load, chase; full load, cockpit) and
  co-driver lines over a driving engine (and the voice alone).
- `PACENOTE_AUDIO_OUT=<dir> cargo test --lib audio::scenes::render_places -- --ignored
  --nocapture` writes traffic passing an idling car (chase and cockpit) and the river, village
  and forest ambience (20 s each).
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
| Recordings in memory | car: 63 sounds, ~126 s of mono 48 kHz f32: ~24 MB (engine 79 s = 15 MB, was 7 MB); co-driver: 130 clips, 92 s at 24 kHz: 8.9 MB. ~33 MB in all (budget 60 MB on Low), decoded once on a worker thread |
| Preview renders (`render_previews`) | rpm sweeps 1000-7500-1000 in 16 s: full load −20.9 LUFS, true peak −8.4 dBFS; no load −26.8 LUFS; cockpit −21.1 LUFS; largest step between 150 ms windows 2.4 dB (full load), 3.9 dB once (no load near 1700 rpm, noise-dominated); co-driver over the engine −16.3 LUFS, true peak −2.5 dBFS; no clipped samples, no clicks (5 ms high-band spikes over 6× their surround) |
| Allocation | None per frame. `Frame` is `Copy` and exchanged with `try_lock` once per 256-frame block. Players and voice pools are fixed arrays inside the synth, which is boxed once at start; the recordings are read from a `&'static` bank |
| Levels over the twelve scenes | RMS −20.6 to −23 dBFS (idle −36.5), peaks ≤ 0.73 (−2.7 dBFS), no clipped samples, DC < 0.0001, no dropouts (silencedetect), no clicks: no 20 ms window has a high-band spike over 12× its own level except at backfire pops |

## Tests (`cargo test --lib audio`, 42 tests + 3 ignored renders/reports; `codriver` adds the bank coverage test)

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
  - pops (`pops.rs`): 2-6 per lift at 6000 rpm for keys and a 150 ms trigger lift in ≥ 90 %
    of 50 seeds, 2-3 at 3500, none below 3200; upshift bangs ≥ 85 %; downshift crackles;
    cancel, rate cap, misfires; levels +6..+10 / +11..+15 dB over the engine; no two pops of a
    burst alike; cockpit darker with more sub; a bang adds ≤ 1 dB of gain reduction
    (`stream.rs`); a downshift blips;
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
- Engine crossfade weights are normalised, never pitch a loop past its neighbour, and keep the
  louder loop within ±8.5 %; every engine loop loads, both mics the same length, and wraps
  seamlessly.
- Co-driver voice: plans longest phrases, pauses and skips unknown words; plays as long as
  planned with silent pauses; a new line cuts the last off within 13 ms; the real bank loads;
  the car ducks 5 dB under it and it is heard; the queue sends lines to the audio thread and
  urgent lines cut in; **every line the co-driver can produce** (pace notes over six stages
  with every option, the start, splits, times, all damage combinations, recovery) is covered
  by the bank.
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

Festival pass (stream D, 4 Nov): `render_scenes` mean 42.5 µs per block, worst block with
pops 192 µs; scene 03 peak −4.5 dBFS, no clipped samples; lift and upshift pops reach 15-17 dB
peak (10 ms) over the 50 ms RMS where the engine alone sits at 9.8 (median). Wind gains a
6 kHz "air" band at `0.4 × SpeedFeel²` and keeps growing to ~220 km/h; the seal whistle
follows `2.2 + 1.5 × SpeedFeel` kHz (`camera::speed_feel`, the value the FOV and blur use).
`PopStats` (`scheduled`, `played`, `bangs`, `misfires`) is on the control port.

## Known limits

- **Guide machine:** the guide has no `DrivenCar` and no `StageWorld`. `net_car_sound` copies
  `guide::sync::NetCar` from the `Remote` entity (rpm, gear, the driver's throttle, speed)
  into `audio::RemoteCarSound`, and the synth plays that: engine, pops and wind. There
  are no tyres, thumps or impacts, because the replicated state has no surface, slip or
  impacts. Ambience comes from `GuideStage.def`.
- The car is 2D, centred on the listener. That is fine for the chase and bonnet cameras.
  Traffic has distance, pan and Doppler but no front/back cue or occlusion; ghosts are silent.
- The rolling road noise and gravel stones stay out (removed on 2 Nov as tiring); turbo and
  gear whine too (no CC0 recordings).
- Not in engine bus meters: the engine's `sound.stats` peak meters don't include the rally
  stream. `RallyAudioStats` is the place to look.
- The rally stream doesn't go through the engine reverb (no tunnels exist yet).
- Impacts don't know what was hit (tree or rock): the sim only sums impact speeds. Scrapes
  against obstacles aren't heard: the sim only reports the body touching the ground.
- The synth runs at a fixed 48 kHz; rodio resamples to the device rate.
- The sound is tuned by numbers and spectrograms, not yet by ear on many speakers.
- Only five of the 26 engine loops are recordings; the rest are PSOLA blends of the two
  recordings around them, so the mid-range timbre (3-5k rpm) is an interpolation between
  ~2.5k and ~6.2k, not a recording at that rpm. Steady recordings at 500 rpm steps (on and
  off load, same car, two mics) would replace them one for one. Partial throttle is a blend
  of the full-load and no-load sets. Granular playback (grains picked live from the nearest
  loop) is the next step.
- The co-driver is a synthetic voice reading single words: the joins are clean but the
  intonation is flat (each clip is said on its own). Recording a human co-driver for the same
  130 entries (`bank.txt`) would drop in without code changes.
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

## Recorded sound effects from Openverse (`tools/audio/`)

Real-world one-shots and ambience beds (crowds, air horn, radar shutter, money, town, church bell,
birds, crickets, wind in pines, guard-rail scrapes) come from [Openverse](https://openverse.org)
(mostly Freesound), CC0 / CC BY only, like every other asset.

1. `python3 -I tools/audio/openverse_sounds.py --wishlist` downloads what `tools/audio/sfx_wishlist.json`
   asks for (each pack: query, count, max length, where the game uses it) into
   `assets_src/audio/openverse/<pack>/` (gitignored) with `credits.json` / `CREDITS.md` per pack.
   One-off searches: `openverse_sounds.py "camera shutter" --pack radar_shutter -n 5 --dry-run`.
   Defaults to licences allowing commercial use and modification, limited to CC0 / PDM / CC BY.
   Standard library only; it falls back to the system certificate bundle when Python has none.
2. `python3 -I tools/audio/cook_sfx.py` makes `assets/sounds/sfx/<pack>_<n>.ogg`: mono 48 kHz Vorbis,
   silence trimmed, one-shots cut to the pack's `max_s` and normalised to -20 LUFS, ambience beds to
   -26 LUFS and crossfaded (2 s, equal power) into seamless 30 s loops. It writes
   `assets/sounds/sfx/index.json` (file → author, licence, source, use), the SFX block in
   `assets/credits.txt` (CC BY lines are required) and the `audio/openverse` section of
   `assets_src/SOURCES.md`. Needs ffmpeg and oggenc.

Notes: the Openverse front blocks some words (any query with "outdoor" gets a 403 bot check);
Freesound results carry no `category`, so don't filter on it. Listen to every new file before
wiring it in: search results vary in quality. Packs with nothing usable today: service park,
dog bark, car horn, gravel hits (search with other words, or synthesise).

### Where they play (`src/audio/festival_recorded.rs`)

| Sound | When | Bus, level |
|---|---|---|
| `start_crowd` | an event's countdown reaches 3 (races: `career::race_flow`; time trials and classic stages: `trial_cues`) | sfx −14 |
| `air_horn` | GO, and at the finish; a win or a best adds a second horn | sfx −8 / −6 |
| `finish_crowd` | the finish: applause (`_3`, −15), or a long cheer on a win / best (`_1`, `_2`, −8) | sfx |
| `radar_shutter_2` | a Radar Post flash, 3D at the camera, under the synthesised readout chime | sfx −4 |
| `money_1` | `MoneyEarned`, and a purchase (the wallet's `spent` grew) | hud −14 |
| `church_bell_1` | on the hour, 07:00-21:00, 3D from the town square (heard to ~1.4 km) | sfx −2 |
| `guardrail_scrape_1/2` | the car's side touching one of B's rails above 2 m/s, 3D at the contact | sfx −8 |
| `car_horn_1-3` | traffic: at the car just shaved past (60 % of Close Shaves), or behind you when you sit blocking it for 2 s | sfx −6, 3D |
| `dog_bark_1/2` | now and then (≈ 1 in 25 s) at a farm or outskirts house within 120 m | sfx −10, 3D |
| `gravel_hit_1/2` | stones on the underbody on gravel / washouts above 30 km/h, more the faster | sfx −18 |
| `service_park_1` | the Service Park hub (main menu, under the music), and in the world within ~120-300 m of home | ambience −16 / −8 |
| beds | looping on `ambience`, weights crossfaded (τ 1.5 s): `town_ambience_2` in and around the Fernhollow core, `birds_morning_1` 05:00-11:00, `night_insects_1` by night, `wind_pines_1` above ~250 m (the pass, the plateau) | ambience −5 to −9 |

The beds and driving one-shots load (AssetServer, decoded off-thread) on entering the game and are
released on leaving it; money loads at startup (the Dealership). Not used after a check of level,
spectrogram and title: `birds_morning_2` (rain), `night_insects_2` (a band), `town_ambience_1`
(a snowstorm), `radar_shutter_1` (beeps), `radar_shutter_3` (a phone), `guardrail_scrape_3`
(a tick), `wind_pines_2` (a step at the loop seam). Dev: `festival.beds`, `festival.voices`.

## Music (menus and loading screen, `src/audio/music.rs`)

Recorded music, picked by ear from Openverse and pinned by id in `tools/audio/sfx_wishlist.json`
(`menu_music`: "Circuit Synthwave" by Doctor_Dreamchip CC0, "Free (rock instrumental)" by Starfish
Stories CC BY 3.0, "Drive Clip" by BaDoink CC0; `loading_music`: "Drops" and "At The Edge Of Space (II)"
by Andrewkn CC0, "Lofi Fusion Background Music" by Seth_Makes_Sounds CC0). `cook_sfx.py` cooks
`"music": true` packs stereo and full length (-18 LUFS, 1 s fade in, 3 s fade out) into
`assets/sounds/music/`.

Original songs made with ACE-Step 1.5 (MIT code and weights; `../ACE-Step-1.5`, MLX on the Mac)
by `tools/audio/gen_music_ace.py` (fixed captions and seeds: re-runs give the same songs, ~2 min
per 150 s song on the M5 Pro) and cooked by `tools/audio/cook_ace.py` (the same `cook_music`):
`menu_music_4..6` (festival rock, electronic rally theme, outback folk-rock), `night_music_1..3`
(synthwave, downtempo, lo-fi) and `loading_music_4` (ambient). Raw renders: `assets_src/music_ace/`.

- Menus: a shuffled playlist, crossfading (2.5 s out / 2 s in) into the next song 4 s before the end.
  After dark (clock 20:30-05:30) the next song comes from the night list (the Late Run).
- Loading screen: a calmer song crossfades in. Driving: the music fades out over 2.5 s; the car and
  the world's ambience take over. There's no music while driving, on purpose.
- Played by the engine's `Music` on the `music` bus: Settings › Audio › Music (was hidden), ducked
  by the pause snapshot. Only the playing song and the one fading in are decoded (~75-100 MB each,
  stereo f32), and both are released after the fade-out into a drive.
- `sound.stats` (dev control) shows the playing track and its position.
