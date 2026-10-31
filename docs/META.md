# Meta: menus, leaderboards, ghosts

`src/meta/` is Pacenote's front end and competition layer: every menu screen, team names,
per-stage leaderboards with export/import, the results screen, ghost cars, the pause menu and
credits. One plugin, `meta::MetaPlugin`, added in `lib.rs`.

| File | What |
|---|---|
| `meta/mod.rs` | `MetaPlugin`, `Profile` (settings section `"profile"`), flow (`start_stage`, `restart_stage`, `recover`, `teardown`), finishing (`finish_run`), ghost car, export/import, dev commands |
| `meta/board.rs` | `Record`, `Board` (best per team per stage per road hash), `merge`, `BoardFile` exchange format, time/date formatting. Pure, tested |
| `meta/ghost.rs` | `Recorder` (per-fixed-step inputs, RLE), `Ghost` file, `Replay` / `replay` (bit-exact). Pure, tested |
| `meta/stages.rs` | `StageCatalog`: all 20 roads generated once in a background thread (0.12–0.15 s for all 20 in a debug build), `stage_hash`, CPU-drawn previews and the menu contour map |
| `meta/netlink.rs` | The only place the menus call networking: `Role`, `host`, `join`, `leave`, `status`, `peer_name`. **Stubs** until guide-net lands; swap by editing this file only |
| `meta/screens.rs` | All screens on the engine UI kit, theme, `on_menu` (every button's handler), settings rows ("Game" tab) |
| `meta/textfield.rs` | One-line text input (names, address) for the UI kit |

## Flow

Main menu (no stage shown: a neutral contour backdrop, the crew card) → **Play** (stage picker,
solo practice) / **Play together** (screen "together": **Host as driver** = the same stage picker,
then `netlink::host`, team run; **Join as guide** = address + name, `netlink::join`, then the net
module owns the guide's flow; and the crew's two names) / **Settings** / **Quit**. **Credits** is a
small link bottom right. The demo adds its store button under Play.

The stage picker ("stages"): the 20 stages, a details panel, and per stage a **weather** (Clear,
Overcast, Fog, Rain; no rain on the snow stages) and **time of day** (Morning, Midday, Afternoon,
Dusk), picked with the chips, A / D and T. Picks are remembered per stage (`Profile.conditions`)
and `meta::stage_def` applies them to the `StageDef` before loading, so the loading screen, the
look (sky, sun, fog, rain), wheel spray and ambience all follow. The driver replicates them to the
guide in `NetStage { weather, time }` (guide panel, guide ambience). Tab opens the stage's
leaderboard. Dev builds: `--weather rain --time dusk` override the pick.

**Settings** (`src/prefs.rs`): tabs Driving, Co-driver, HUD, Audio, Voice chat, Video,
Interface, Controls. `prefs::layout` (Startup) re-registers every row (same key, get, set) on its
rally tab with rally wording, and hides engine rows Pacenote doesn't use (mouse look, music,
sprint toggle, rumble, generic subtitles, the duplicate "Dialogue" volume). Pacenote's own
options are `Prefs` (section "prefs"): co-driver voice, volume, reading speed, call timing
(read-ahead ×1.3 / 1 / 0.75), call order ("left three" / "three left"), subtitles; speed units
and which HUD readouts show. Side panels preview the HUD and a sample call live.

**Camera view:** Cockpit / Bonnet / Chase, saved in `Profile.camera` (Settings > Game > Camera
view, the stage picker's View chips or C there). Runs start in it; C in game (or on the pause
screen, whose footer lists it) changes it and the change is kept (`meta::sync_camera`, which
also toasts "Chase camera"). A "C · Camera view" hint shows for 5 s at the start of each run.

**Weather and fairness:** each weather / time of day has its own board and ghosts. Records and
ghosts carry a board hash: the road hash at the stage's default conditions (unchanged, so
existing times stay where they are), `<road hash>@<weather>-<time>` otherwise
(`stages::board_hash`). A board lists only its own conditions; "Old road" (stale) means another
road in the same conditions. Ghost files get a `-<weather>-<time>` suffix off the defaults.

`start_stage(world, id, mode)` sets `StageChoice` and goes to `AppState::InGame`; the game's
`load_stage` builds it. From a stage straight into another (results → Next) the same state is
set again: Bevy runs `OnExit(InGame)` + `OnEnter(InGame)` for that identity transition, so
`teardown` removes the old stage and `load_stage` builds the new one (tested).

`teardown` (on `OnExit(InGame)`): `engine::world::despawn_world`, then every root entity with a
`Mesh3d`, light, `Camera3d`, `DrivenCar`, `PcgVolume` or `PcgCellRoot`, and the `StageWorld`
resource. **Other streams:** give in-game UI and non-mesh entities `DespawnOnExit(AppState::InGame)`
(the HUD does) or they survive into the menu.

Finish (`StageRun.finished` set, watched in `Update`): `finish_run` builds a `Record`, inserts
it into the board, saves `leaderboard.json`, saves the ghost when it's the team's best, stores
`LastResult` and opens the sticky `"results"` screen (Retry / Next stage / Menu). The guide's
machine (`Role::Guide`) never writes records.

Pause (Esc, engine pause screen): Resume, Restart stage, Recover car (+5 s), Settings, Quit to
menu. The header shows the stage clock, penalties and progress.

## Leaderboard records

```json
{"id": "9f1c…", "stage": 1, "driver": "Mia Kova", "guide": "Leo Brandt", "time": 191.7,
 "penalty": 0.0, "splits": [47.9, 95.9, 143.8, 191.7], "date": 1793190000,
 "version": "0.1.0", "stage_hash": "d3df5080b137ef9a"}
```

- `guide: ""` = solo practice. Practice times rank separately and never take the stage record.
- Team identity is case- and whitespace-insensitive ("mia kova|leo brandt").
- **Best per team per stage per road hash**: a slower run never replaces a faster one; a run on
  a regenerated road (different `stage_hash`) gets its own slot and the old one shows as
  "Old road", ranked after current times.
- `id` is a content hash (stage, team, time, penalty, date, road hash) that is the same on
  every machine: an online board can upsert by id and apply the same keep-the-best rule
  (`Board::merge` is commutative and idempotent, tested).
- `stage_hash` (`stages::stage_hash`) hashes the stage definition and every road station
  (position, width, banking, surface): any generator change shows.

Files in the data dir (`<DataDir>`, `PACENOTE_DATA_DIR` overrides):

| Path | What |
|---|---|
| `leaderboard.json` | the `Board` |
| `ghosts/sNN-<team hash>.json` | best ghost per team per stage |
| `boards/pacenote-board-YYYY-MM-DD.json` | "Export board": every record + every ghost (`BoardFile`) |
| `boards/inbox/*.json` | "Import boards" merges each, then moves it to `inbox/merged/` |

`--import-board <file>` merges a file at startup.

## Ghosts

The car sim (`vehicle::step`) is deterministic, so a run is its start pose (from the road),
the `vehicle::Input` of every fixed step since the car spawned (countdown included) and the
recoveries. `drive::simulate` feeds `meta::ghost::Recorder` (two small hooks); inputs are stored
as raw f32 bits, run-length encoded: one entry per input change (a 7–8 s keyboard test run is 4 entries, ~0.5 KB). The ghost car
steps in `Update` until it has applied as many steps as the recorder (lockstep with the driven
car, interpolated with the same overstep fraction) and is drawn as an unlit translucent body
(no shadow, no collision). Each ghost stores the stage hash and a `sim_hash` of the car `Spec`;
a mismatch marks it stale (it may drift).

Which ghost races (Settings → Game → Ghost car): Off, Our best, Fastest (any ghost you have,
current road first), Rival (picked on the leaderboard: Enter on a time with a ghost tag).

## Control commands (dev builds)

| Command | Params | Result |
|---|---|---|
| `meta.state` | | screens, role, mode, names, stage + hash, record count, recorded steps, ghost, last result |
| `meta.profile` | `{driver?, guide?, address?, ghost?}` | names |
| `meta.start` | `{stage, mode: "practice" \| "team"}` | loads a stage |
| `meta.finish` | `{time?}` | finishes the run now (results screen) |
| `meta.restart`, `meta.recover` | | as the pause menu |
| `board.export` | `{path?}` | `{path}` |
| `board.import` | `{path}` | `{changed}` |
| `ghost.verify` | | replays this run's recording, `{same: bool, steps, runs}` |

## Tests

- `board.rs`: insert keeps each team's best, ranking order, stale records, merge (best kept,
  commutative, idempotent, junk skipped), file round trip and rejection, names, formatting, ids.
- `ghost.rs`: a recorded run (with a recovery) replays through JSON to exactly the same car;
  recorder restarts on a new car; spec changes change the sim hash.
- `stages.rs`: hash stable and sensitive; info and pictures.
- `tests/meta.rs` (headless scenario): the menu reaches stage 3, "Next" loads stage 4 with one
  car and one terrain, "Menu" leaves no stage meshes; a driven run (keys + a recovery) replays
  bit-exact, finishing writes the board and the ghost, and on Retry the ghost driven with the
  same inputs is exactly on the car at the same step.

## Known limits

- Networking is stubbed (`netlink.rs`): Host works locally (team runs use the profile's guide
  name), Join reports that networking isn't in the build yet.
- Import reads a folder (no file dialog in the engine). Export/import carry every ghost.
- Ghost size grows with input changes: keyboard driving changes input a few times a second
  (tens of KB for a 4 min stage); an analogue gamepad changes it nearly every step (up to
  ~1 MB pretty-printed for 4 min). Quantising analogue input in `drive::simulate` (e.g. to
  1/128) before it reaches the car would make gamepad ghosts as small as keyboard ones.
- The results screen dims the HUD but the HUD's centre "FINISH" text still shows faintly
  behind it (the HUD could hide while a screen is open).
- Ghosts replay against the current `StageWorld` obstacles: a dressing change (trees moved)
  can make an old ghost drift even when the road hash matches.
