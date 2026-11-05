# Pacenote handoff

Start here in a new session. **5 Nov 2026: the "Valley Trials" festival update is merged** (see
"Festival update" below; design docs in `docs/festival/`, start with `DECISIONS.md`). Older state as of 30 Oct 2026: everything below is merged on `main`, and
all tests pass (84 unit tests plus 3 integration binaries).

## What the game is

An original two-person rally game on the in-house engine (`/Users/himanshu/codes/game/engine`,
Bevy 0.19.1). There are 20 fixed stages, generated from seeds, and teams race each other's times.
- **Driver:** sees the 3D stage and drives, but has no map.
- **Guide:** sees the map and the upcoming corner calls, but can't control the car.
- **Voice:** the two talk over built-in voice chat.

Full design: `docs/DESIGN.md`.

## Festival update (5 Nov 2026)

Built by 7 parallel streams from `docs/festival/` (GAME_SYSTEMS, ART_BIBLE, WORLD_PASS, AUDIO_PASS,
UX_PASS, TECH_PASS; `DECISIONS.md` overrides them; `PERF_REPORT.md` has the perf breakdown).
All merged on `main`; 453 lib tests + integration tests pass.

| Path | What |
|---|---|
| `src/career/` | Notes economy (`wallet.rs`, `economy.rs`), 8-car roster + upgrades as `Spec` multipliers + PI/classes (`garage.rs`), grade/xp, `career.json` save (`save.rs`), Style skills + Rhythm (`style.rs`), rivals: pure race sim (`racers.rs`), race flow (`race_flow.rs`), liveries (`livery.rs`) |
| `src/pilot.rs` | The driving AI (rivals and the dev autopilot) |
| `src/daytime.rs` | `DayClock` at 5x real time, solar sun, moon; Settings › World (start time, time passes) |
| `src/camera.rs` | Cockpit / bonnet / spring-arm chase cameras, speed feel (FOV, blur, shake), `perf.draws` |
| `src/art/` | `sky.rs` + `clouds.wesl` (cloud dome, stars, cloud shadows), `exhaust.rs` (flames), `headlights.rs`, `signs.rs`, `conditions.rs` |
| `src/audio/` | `pops.rs` (pop/bang scheduler + synth), `festival.rs` (gameplay cues), `festival_recorded.rs` (Openverse sfx), `music.rs` (menu/loading music), `ui.rs` (menu sounds) |
| `src/open/` | `town.rs` (Fernhollow), `furniture.rs` (rails, signs, posts, road conditions), `challenges.rs` (13 challenges, `assets/levels/open/challenges.ron`) |
| `src/meta/` | `hub.rs` (Service Park menu), `showroom.rs` (3D car), `garage_ui.rs` (Garage/Upgrades/Dealership), `confirm.rs` |
| `src/hud_style.rs`, `hud_challenge.rs`, `hud_race.rs` | Rhythm panel, challenge cards, race positions/results |
| `tools/audio/` | `openverse_sounds.py` (CC0/CC BY downloader, `sfx_wishlist.json`), `cook_sfx.py` |
| `tools/perf/budget.py` | Per-scenario perf check (tour, town, mountain, pass, event), any checkout |

Dev commands: `career.give/request/state`, `events.start {id, race}`, `challenges.goto/state`,
`style.state`, `race.state`, `look.time`, `day.get`, `sky.clouds`, `perf.draws`, `autopilot.set`.

Records/ghosts: the re-baked level and `furniture::REV` 2 reset every open-world board.

Open items:
- Land is cached at bake (`assets/levels/open/land.bin`): no erosion at load. Re-bake after changing the land generator (`committed_land_cache_is_current` fails otherwise).
- Perf: see `docs/festival/PERF_REPORT.md` actions 2-5 (forest solids at load, music/texture memory,
  pipeline cache). Action 1 (256 m terrain chunks) is in.
- Repair untested in game (unit tests only); 3 AI stories untested (Rookie yield, gap accuracy,
  recover hold); warning/speed signs aren't knockable (draw budget); packs with no usable sound:
  service park, dog bark, car horn, gravel hits.
- Rust: rustc 1.97 can't build Bevy 0.20.1; use `cargo +stable` (or add a `rust-toolchain.toml`).

Engine gaps found (engine repo, not changed):
- No clouds in Bevy 0.20/engine (game-side dome); clouds aren't in reflections.
- `LookTimeline` re-applies the look every frame and refilters env lighting 10x/s (p95 11.8 → 17.5
  ms): needs a cheap "move the sun" path. Switching sky mode in play stalls pipelines.
- No streamed music: a song decodes to 75-100 MB of f32.
- Toast has a fixed 48 px top offset (covers the menu top bar).
- `VisibilityRange { use_aabb }` measures to the mesh centre: big merged tiles vanish when inside.
- Motion blur only on High+; no GPU memory census tool; offscreen runs render 1080p only and have
  no audio device (levels measured offline).

## Read first

1. `docs/DESIGN.md`: pillars, roles, stages, driving model.
2. `/Users/himanshu/codes/game/engine/CLAUDE.md` and `engine/docs/GAME_GUIDE.md`: how to build
   games on the engine, perf rules, pitfalls.
3. The doc for the area you touch: `docs/ART.md`, `AUDIO.md`, `META.md`, `NET.md`, `VOICE.md`.
   `docs/TEAM.md` covers the worktree-per-agent workflow, if you split work up again.

## Code map

| Path | What |
|---|---|
| `src/stage.rs` | 20 stage definitions, road growth, terrain shaping, corner grades 1–6, crests (pure, tested) |
| `src/vehicle.rs` | Car sim (pure, deterministic): raycast suspension, Pacejka tyres per surface, ABS, traction assist, viscous LSD, hill hold, auto gearbox, impulse obstacle hits, `Damage` (engine / steering / suspension; a broken part retires the car; body parts come off, `Damage::detached`) from solid hits only; saplings, posts and signs snap (`Car::broken`) and shrubs drag without damage |
| `src/world.rs` | `StageWorld` (road, heights, obstacles; implements `Ground`), terrain + PCG dressing spawn |
| `src/drive.rs` | Input actions, 60 Hz fixed-step sim, cameras (cockpit / bonnet / chase), `StageRun` (clock, splits, progress, retired) |
| `src/loading.rs` | Loading screen: stage generated on a worker, built, pipelines warmed, near scenery and trees in, frames settled; then the countdown |
| `src/codriver/` | AI co-driver: pace notes read ahead by time (`calls.rs`, pure), damage/splits/finish/off-road/roof lines, system TTS (`speech.rs`: `say`, espeak, PowerShell), subtitles; `N` toggles; quiet when a human guide is connected |
| `src/hud.rs` | Driver HUD (speed, gear, clock, damage readout, retired message) |
| `src/map/` | Open-world map: painted region image (`paint.rs`), road routes (`path.rs`), free-roam minimap (`mini.rs`), full map screen on M (`screen.rs`) |
| `src/art/` | Textured terrain material (`terrain.wgsl`), road ribbon, card trees, rocks/plants, car model, per-stage look (sky/fog/rain), wheel spray |
| `src/audio/` | Car from CC0 recordings (`assets/sounds/car/`, cooked by `tools/audio/cook_car.py`): engine rpm/load loops crossfaded and pitched, tyres per surface, impacts, pops; synthesised wind, stones, thumps and ambience; all on a custom stream; guide hears the replicated car (`RemoteCarSound`) |
| `src/meta/` | Menus, stage select, leaderboards (`board.rs`), ghosts (`ghost.rs`), results, credits; `netlink.rs` adapts to `net_session` |
| `src/net_session.rs` | `Role` (Solo / Driver / Guide), `Session`, `host` / `join` / `host_with` / `join_with` |
| `src/guide/` | Replication (`sync.rs`: `NetCar`, `NetRun`, `NetStage`), guide map, panel, driver's "guide connected" notice |
| `src/voice/` | Mic capture (cpal), IMA ADPCM, jitter buffer, playback, push-to-talk / open mic, indicator |
| `tests/` | `guide_net.rs` (two apps on loopback), `meta.rs` (scenario flow), `voice.rs` (two-app voice) |
| `tools/art/` | Blender / Python scripts that build meshes, foliage cards, and prep textures |
| `assets/` | Cooked KTX2 textures + `color.avg`, `.pnm` meshes, `rallycar.glb`, `credits.txt` (sources are gitignored) |
| `assets_src/` | Raw CC0 / CC-BY downloads, local only; `SOURCES.md` is tracked |

## Build, test, run

Every cargo command goes through `memgate`. The target dir is shared with Shroud via
`.cargo/config.toml`, so Bevy builds once. Never run `cargo clean`.

```sh
memgate -- cargo check -q
memgate -- cargo test -q                  # everything
memgate -- cargo test -q --lib vehicle    # one module
memgate -- cargo test -q --test guide_net # one integration binary
```

Run offscreen, drive it over the control port and look at the screenshots. The steps file runs
from the engine dir:

```sh
cd /Users/himanshu/codes/game/engine
SMTP_USER= SMTP_PASS= python3 -I scripts/play.py steps.json \
  --manifest /Users/himanshu/codes/game/pacenote/Cargo.toml --bin pacenote --out <dir> -- --stage 5
```

Steps: `{"sleep": 3}, {"cmd": "engine.start"}, {"cmd": "input.key", "params": {"key": "KeyW",
"action": "down"}}, {"shot": "name"}, {"cmd": "perf.stats"}, {"cmd": "voice.status"}`. The game
log is `<dir>/game.log`; grep it for `ERROR`.

Metrics: dev runs on screen open the engine's metrics viewer as its own process
(`pacenote --metrics-viewer`; F4 reopens it). It reads the data dir's files, so it keeps
working after the game closes and shows every past run: Left/Right pick a run, L the latest.

Driver + guide on one machine: start the driver's play.py in the background, wait ~4 s, then
start the guide's:

```sh
... -- --role driver --host 7791 --stage 3
... -- --role guide --connect 127.0.0.1:7791
```

Real play: `pacenote --role driver --host 7777` and `pacenote --role guide --connect <ip>:7777`.
You can also use the menus: Host as driver / Join as guide.

## Conventions

- **Determinism:** the sim is pure and runs at a 60 Hz fixed step. Ghosts replay inputs
  bit-exactly, so keep `vehicle.rs` free of frame-time or random state.
- **Left:** left = up × direction everywhere (heading, lateral, banking, ribbon).
- **Visuals:** in headless runs (`art::visual(world)` false) nothing visual spawns. Tests rely
  on that.
- **In-game entities:** give UI and non-mesh entities `DespawnOnExit(AppState::InGame)`, or they
  survive into the menu.
- **Assets:** `GameInfo::assets_fallback` points at this crate's `assets/`. Don't remove it, or
  runs started from the engine dir lose every texture and mesh.
- **Commits:** end messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Engine:** don't change Bevy features. Engine changes go in the engine repo
  (`github.com/Himasnhu-AT/hull`), committed separately.

## Known issues and gaps (candidates to fix)

Loading / damage / co-driver (added 30 Oct):
- The freeze on entering a stage (up to 13 s, window not responding) was the stage built in one
  frame, then every pipeline compiling synchronously on first draw (macOS) while 30k scenery
  entities streamed in. It all happens behind `loading.rs` now (~4 s on an M5 Pro). Scenery past
  450 m of the start still streams in during play, nearest first.
- Damage isn't replicated to the guide (`NetCar` lacks it), and a retired run isn't on the
  results screen or the board: the HUD says RETIRED and Esc offers restart / leave.
- The co-driver uses the OS voice (`say`): its sound skips the engine mixer and volume. Voice,
  rate and timing (`calls::LEAD_S`) may want tuning by ear. Corner calls don't include
  "don't cut" / "caution" from terrain (only grade, long, crest, narrows).
- Worktrees: cargo gives every checkout of this package the same artifact hashes, so two
  worktrees on the shared target dir overwrite each other's binary. Give a second worktree its
  own `CARGO_TARGET_DIR`.

Driving (revision 10, `docs/HANDLING.md` "Crashes, rolls and starts"):
- 0–100 km/h: 4.1 s on tarmac, 4.6 s on gravel, 7.5 s on snow. That matches an R5.
- Damage: an 80 km/h tree costs ~0.4 of the engine and the car drives on. A part breaks at
  ~120 km/h head-on. Death takes 0.6 of the whole car in one crash (~125 km/h nose-on).
- Every stage starts on a 60 m straight.
- Side hits on trunks spin the car instead of rolling it. It still rolls sliding sideways off
  the road down a steep bank. The HUD says "Car on its roof · R Recover" after 1 s upside
  down and still (`meta/coach.rs`).
- Recover is `R` (+5 s in an event).
- Gravel at 100 km/h with a held key overshoots the yaw (~170 %) and swings once; it needs a
  tyre-lag or yaw-damping pass.
- Chase camera: it keeps out of traffic, walls and banks. FOV widens with speed, and the
  cockpit and bonnet views get road buzz.
- Traffic stops for the player's whole footprint along its own path.

Art (`docs/ART.md`):
- Trees are flat cards: the planes show up close, and there's no wind. Around 100–150 m a tree
  can be drawn by both its near and far version for a moment.
- Steep rock faces stretch their texture.
- There's a hard road edge just behind the car at the start line.
- No tyre-track decals, no mud on the car, HDRI skies unused (the engine has no
  equirect→cubemap tool).
- Cost: on stage 1 the GPU goes from 4.3 to 8.4 ms per frame and memory from 1.2 to 2.0 GB
  (debug, 1080p). Trees are ~2.7 ms GPU and 240 MB.

Guide / net (`docs/NET.md`):
- Sessions started from the CLI send empty player names. Only the menus fill them in. A
  `--name` flag would fix it.
- The guide map has no terrain shading or hazard marks.
- Mouse-wheel zoom and drag-pan are untested (the control port has no wheel command).
- No test for the driver changing stage mid-session.
- One guide per driver. A crashed guide gets back in after the engine's 5 s timeout.
- LAN or a forwarded port only. No relay or NAT traversal (an engine limit).

Audio / voice (`docs/AUDIO.md`, `docs/VOICE.md`):
- The guide doesn't hear tyre, thump or impact sounds; the replicated state lacks them.
- No echo cancellation (headphones needed with open mic), no mic picker. A mic unplugged
  mid-session isn't reopened.
- Voice bypasses the engine mixer's low-pass and reverb. A packaged `.app` would need
  `NSMicrophoneUsageDescription`.

Repo:
- No git remote yet. The 30+ commits exist only on this machine.
- Old worktrees under `/Users/himanshu/codes/game/pacenote-wt/` (art, audio, guide-net, meta,
  voice) are fully merged. Remove them with `git worktree remove <path>` when no longer needed.
