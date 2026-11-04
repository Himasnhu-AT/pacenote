# Festival update: technical pass (TD + performance)

Scope: streams A World, B Roadside, C Look, D Car feel, E UI, F Gameplay, built in parallel
worktrees and merged by the lead. This doc covers the engine facts each stream depends on, file
ownership, the contracts between streams, budgets, determinism, build logistics and merge order.
It reconciles the stream docs `ART_BIBLE.md`, `AUDIO_PASS.md` and `UX_PASS.md`: where they
disagree, this doc wins. All decisions were made autonomously, with a one-line rationale each.

## 1. Engine capability check

| Need | Status | Where |
|---|---|---|
| Atmosphere sky | Yes. `SkyMode::Atmosphere` (default) spawns Bevy `Atmosphere::earth`, puts `AtmosphereSettings` on the camera, `SunDisk` | `engine/src/look/apply.rs:253,274,317-337`, `desc.rs:262` |
| Volumetric fog / shafts | Yes. `VolumetricLook` (fog box + `VolumetricLight` on the sun), tier `volumetric_fog` (High+) | `desc.rs:421`, `apply.rs:646` |
| Clouds | **None.** Bevy 0.20.1 has no cloud layer (`bevy_pbr/src/atmosphere` only mentions cloud shadows in the docs of `AtmosphereMode::Raymarched`), and neither does the engine. C builds it in the game (§3) | — |
| MotionBlur | Yes. `LensLook.motion_blur {enabled, shutter_angle, samples}`, tier `camera_fx` (High/Ultra only) | `desc.rs:644`, `apply.rs:695`, `gfx.rs:167` |
| Auto exposure | Yes. `exposure.auto` + compensation curve, tier `auto_exposure` (Medium+). Low uses manual `ev100` | `apply.rs:516,671`, `gfx.rs:166` |
| Bloom | Yes. Whether it runs is the tier's (Medium+); its tuning comes from `BloomLook` | `apply.rs:654`, `gfx.rs:148` |
| Look application | The `Look` resource drives an exclusive `apply_look` (`QualitySet::Look`, Update). It runs only when `Look`, the tier, the settings or the cameras change. Each apply costs 0.18–0.65 ms CPU (dev) | `apply.rs:1-4,149-168`, `docs/LOOK.md:230` |
| Changing the look every frame | Possible but not cheap: `LookTimeline` writes `Look` every frame, so `apply_look` runs every frame and the env light refilters at 10 Hz (measured at High: p95 17.5 ms vs 11.8 ms with the look static). **Rule for C:** write `Look` only when the sun has moved one quantum (0.02°, about once per real second at 5×) and set `LookConfig.env_refresh_hz = 0.05` (refilter at most every 20 s). Don't use `LookTimeline` | `timeline.rs:30-43`, `apply.rs:34-50`, `LOOK.md:254` |
| Env map / probe refresh | One hidden `SkyEnvFilter` entity filters the sky once and then freezes it. Each refresh is one filter pass over 2–3 frames (≈0.4–0.6 ms at 256²). Realtime probes cost +2.3 to +3.6 ms, so keep probes baked (`refresh: Some(0)`). Clouds don't appear in the env map: Bevy renders that map from the atmosphere alone. Getting them in needs an engine change (deferred) | `apply.rs:736-830`, `LOOK.md:225-256` |
| Pipeline toggles | Toggling SSR, SSAO, bloom, TAA, volumetric fog, the shadow filter or contact shadows during play rebuilds every pipeline (stalls up to 1 s). **Never switch `SkyMode` during play either:** it adds or removes the atmosphere on the view. Keep `Atmosphere` at night (the sky goes dark by itself) and draw the stars in the cloud shader. This overrides ART_BIBLE §2.3, which switches to `Gradient` at night | `GAME_GUIDE.md §3.3` |
| UI kit | `engine::ui`: theme, `nav_button`/`nav_list`/`action_button`/`tab_button`/`field_row`/`card_row`, focus and pad navigation, `ScrollList`, `Toast`, a screen stack (`register_screen`, `register_sticky_screen`, `ScreenRequest`, `RebuildScreen`), `MenuConfig` (main/pause items, side panel), and a settings screen generated from `FieldDef`s. Missing: a confirm modal, count-up text, a 3D showroom (E builds these) | `engine/src/ui/mod.rs:1-60`, `menus.rs:17-82`, `widgets.rs` |
| App states | `engine::flow::AppState { Boot, MainMenu, InGame }`. Menus are screen-stack ids under `MainMenu`, registered in `src/meta/screens.rs:89-127`. `MenuAction::Custom(&'static str)` sends a `MenuEvent`, which `meta::screens::on_menu` handles. No new `AppState` (as UX_PASS §7.4 says) | `engine/src/flow.rs:14` |
| Settings and prefs | One `settings.json` in the data dir. Each section is a serde struct added with `add_settings_section::<T>(app, "key")` and rows via `register_setting(FieldDef)`. `prefs::layout` puts the rows listed in `LAYOUT` first; **unlisted rows keep their order at the end of their tab, and an unknown tab is inserted before Controls**. Pacenote sections today: `prefs` (`Prefs`) and `profile` (`meta::Profile`) | `src/prefs.rs:113-122,326-351`, `src/meta/mod.rs:53-85,254` |
| Save location | `engine::paths::DataDir`, which is `~/Library/Application Support/pacenote/` on macOS. It holds `settings.json`, `leaderboard.json`, `ghosts/` and `logs/`. Career goes to a new `career.json` (§3) | `src/meta/mod.rs:42-47` |
| Car `Spec` | There is one `Spec` per game (`Spec::default()`, a ~1,250 kg R5) in a single resource, `drive::CarSpec(Spec)`. `prefs::apply_assists`/`sync_damage` then overwrite `abs`, `traction_assist` and `damage`. `drive::simulate` steps **every** `DrivenCar` with that one `CarSpec` and the player's input | `src/vehicle.rs:248-306`, `src/drive.rs:28,137-175`, `src/prefs.rs:376-433` |
| Sim cost | 2.2 µs per 60 Hz step (8 substeps) on flat ground in a dev build (`HANDLING.md:107`), 3.5–4.7 µs under Rosetta. Ground queries in the open world (heightfield, `roads::surface`, obstacle grid, forest, walls) plus the autopilot's `road.nearest` are the real cost: estimate 15–40 µs per car-step. 5 AI cars at 60 Hz ≈ 0.2 ms per frame, 0.4 ms when the fixed step catches up twice. **Cheap enough.** Measure with `handling_tests` "cost" (`src/handling_tests.rs:551-561`) and a new open-world variant (F) | — |
| Perf measurement | `tools/perf/budget.py --quality Low\|Medium\|High` runs the fixed `open.tour` with the autopilot for 60 s and checks `perf.report` and `render.stats` against budgets (`docs/ASSET_PIPELINE.md:43-80`) | — |

## 2. File ownership

**Rule:** each stream works in its own new files. A shared file has one owner; anyone else gets
only the touches listed below. Everything that crosses streams lands **before branching**, in the
lead's "wave 0" contracts commit (§3.0), so no stream has to edit `lib.rs`, `art/mod.rs`,
`audio/mod.rs` or `prefs.rs`.

| Stream | Owns (may change freely) | Allowed tiny touches elsewhere |
|---|---|---|
| **A World** | `src/open/*` **except** `events.rs`, `furniture.rs` (B) and `activities.rs` (F); `src/world.rs`; `src/art/{terrain.rs,terrain.wesl,ground.rs,junctions.rs,water.rs,trees.rs,dressing.rs,props.rs}`; `src/map/*`; `assets/levels/open*`; `src/stage.rs` terrain code | golden hashes in `handling_tests.rs` (`stages_hash_the_same_everywhere`, obstacles) after the re-bake |
| **B Roadside** | new `src/open/furniture.rs` (pure placement: rails, signs, conditions; tested); `src/art/roadside.rs`; new `src/art/signs.rs`, `src/art/conditions.rs`; sign and decal textures | none; the hooks are in wave 0 |
| **C Look** | `src/art/look.rs`; new `src/daytime.rs`; new `src/art/sky.rs` + `src/art/clouds.wesl`; cloud textures | none. Cloud shadows in `terrain.wesl` come after the merge, as a follow-up |
| **D Car feel** | `src/drive.rs` (cameras; D may move them to a new `src/camera.rs`); `src/audio/{car.rs,powertrain.rs,stream.rs,body.rs}`; new `src/audio/pops.rs`; `src/art/{car.rs,spray.rs,hands.rs,mirror.rs}`; new `src/art/exhaust.rs`; headlights | none |
| **E UI** | `src/meta/screens.rs`, `src/meta/mod.rs`, `src/meta/coach.rs`, `src/loading.rs` (visuals and copy, **not** the phase logic), `src/hud.rs`; new `src/meta/{hub.rs,showroom.rs,garage_ui.rs,confirm.rs}`; new `src/audio/ui.rs` (UI sounds) | the `prefs::LAYOUT` row order, in a final commit only |
| **F Gameplay** | new `src/career/*`; new `src/pilot.rs` (non-dev driving core, moved out of `autopilot.rs`); `src/autopilot.rs` (becomes a shell over `pilot`); `src/open/events.rs`; new `src/open/activities.rs`; new `src/hud_style.rs`, `hud_activity.rs`, `hud_race.rs`; new `src/audio/festival.rs`; new `assets/levels/open/activities.ron` | `src/vehicle.rs`: **additive only** (`Spec` constructors such as `Spec::talus()` and `Spec::scaled(&Upgrades)`). No change to `step`, the tyres, `Ground` or `REVISION` |

Hot spots:
- `src/lib.rs`, `src/art/mod.rs`, `src/audio/mod.rs`: frozen after wave 0. Need another module? Ask the lead.
- `src/prefs.rs`: nobody adds `Prefs` fields. Each stream adds **its own section struct**
  (`DaySettings` "day" for C, `CameraFeel` "camera_feel" for D, `CareerSettings` "career" for F, `UiPrefs` "ui" for E if needed)
  with `add_settings_section` and `register_setting` in its own plugin. Tabs: rows with
  `tab: "World"` (C) and `"Camera"` (D) appear automatically. Wave 0 adds both names to `TABS`.
  Conflicts in `prefs.rs` then can't happen. This overrides UX_PASS §7.2.
- `src/meta/screens.rs` / `meta/mod.rs`: only E edits them. F never edits UI files: E reads F's resources (§3).
- `src/hud.rs`: only E edits it (marks chip, shift pulse, safe area). F's popups go in its own `hud_*.rs` files.
- `src/drive.rs`: only D edits it. F reads `DrivenCar` and doesn't write to it. AI cars **don't** use `DrivenCar` (§5).
- `src/world.rs`, `src/open/mod.rs`: A owns them. B's and F's hooks are already in wave 0.
- `Cargo.toml` / `Cargo.lock`: nobody touches them (no new crates). Need one? Ask the lead.

## 3. Shared interfaces

### 3.0 Wave 0: the lead's contracts commit (≈250 lines, compiles, tests green, no behaviour)

1. `lib.rs`: `pub mod career; pub mod daytime; pub mod pilot; pub mod hud_style; pub mod hud_activity; pub mod hud_race;`
   plus `add_plugins((career::CareerPlugin, daytime::DayPlugin))` with stub plugins. `art/mod.rs`: `pub mod sky; pub mod exhaust; pub mod signs; pub mod conditions;` with empty plugins registered.
   `audio/mod.rs`: `pub mod pops; pub mod festival; pub mod ui;`. `open/mod.rs`: `pub mod furniture; pub mod activities;`.
2. Every type and message below, with docs and `add_message`/`init_resource` calls, and stub
   bodies that are trivially correct (a catalogue of one car, the Talus; `spec_of` returns `Spec::default()`).
3. `stage::Surface::Wet` (grip `(0.72, 0.10)`) with its arms in the 6 exhaustive matches
   (`vehicle.rs`, `stage.rs`, `art/spray.rs`, `meta/screens.rs`, `audio/mod.rs`, `open/roads.rs`).
   No surface uses it yet, so no golden hash changes.
4. Hooks in A's files, each returning "nothing" until B or F fills it in:
   - `OpenWorld::ground_height`: `+ furniture::dip(&self.furniture, x, z)` (potholes; 0.0 for now).
   - `OpenWorld::surface`: `furniture::surface_override(&self.furniture, x, z).unwrap_or(roads::surface(..))`.
   - `OpenWorld::from_data_with`: `furniture::walls(&net, &heights, &mut walls)` (guard rails as `Wall`s).
   - `OpenWorld` field `pub furniture: Arc<furniture::Furniture>` (`Default` = empty).
   - `StageWorld` field `pub racers: Option<Arc<career::racers::RacerSolids>>`, appended in `obstacles()` after traffic.
5. `prefs::TABS` gains "World" and "Camera".

Everyone branches from that commit. **Decision:** F owns the career data types. The lead lands
them as stubs in wave 0, so E builds the garage, upgrades and dealership against the real API
from day one. *Rationale: the data is gameplay (it feeds `Spec`, so it affects determinism), and
the stubs remove the blocking.*

### 3.1 Types (exact names)

```rust
// src/daytime.rs — C. Look-only: the sim never reads it.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct DayClock { pub hours: f32 /*0..24*/, pub rate: f32 /*game s per real s, 5.0*/, pub running: bool }
impl DayClock { pub fn sun(&self) -> (f32 /*az°*/, f32 /*el°*/); pub fn daylight(&self) -> f32 /*0 night..1 day*/; pub fn is_night(&self) -> bool }
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)] #[serde(default)]
pub struct DaySettings { pub start: u32 /*0 Dawn..5 Night, 6 Real clock*/, pub time_passes: bool }   // section "day", tab "World"
pub const DAY_RATE: f32 = 5.0;   // advanced on Time<Virtual> in Update (pause freezes it)

// src/audio/pops.rs — D. PopScheduler (main thread) is the single source; flame + synth read it.
#[derive(Message, Clone, Copy, Debug)] pub struct ExhaustPop { pub car: Entity, pub intensity: f32, pub kind: PopKind, pub seed: u32 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PopKind { Crackle, Bang, Misfire }   // Misfire: smoke, no flame
// (ART_BIBLE's `Backfire` is this message; the name `ExhaustPop` wins.)

// src/camera.rs (or drive.rs) — D.
#[derive(Resource, Clone, Copy, Default)] pub struct SpeedFeel(pub f32);   // 0..1, smoothstep 40→180 km/h; FOV, blur, wind
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)] #[serde(default)]
pub struct CameraFeel { pub motion_blur: u32 /*0 off,1 low,2 high*/, pub chase_stiffness: f32, pub landing_kick: bool }   // "camera_feel", tab "Camera"

// src/career/wallet.rs, garage.rs, grade.rs — F (stubs in wave 0). One save: <DataDir>/career.json.
pub const CAREER_FILE: &str = "career.json";
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)] #[serde(default)]
pub struct Wallet { pub marks: u32 }
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)] #[serde(default)]
pub struct Garage { pub owned: Vec<OwnedCar>, pub active: usize }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OwnedCar { pub model: String /*CarModel::id*/, pub upgrades: Upgrades, pub paint: u8 }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Upgrades { pub engine: u8, pub drivetrain: u8, pub tyres: u8, pub suspension: u8, pub brakes: u8, pub weight: u8 } // 0..=3
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum UpgradeKind { Engine, Drivetrain, Tyres, Suspension, Brakes, Weight }
pub struct CarModel { pub id: &'static str, pub name: &'static str, pub class: &'static str, pub price: u32, pub base: fn() -> Spec, pub model: &'static str /*glb*/ }
pub fn catalog() -> &'static [CarModel];
pub fn spec_of(car: &OwnedCar) -> Spec;            // pure: base × upgrade multipliers (tested)
pub fn car_key(car: &OwnedCar) -> String;          // "talus.e2d0t1s0b0w0": boards/ghost key (§5)
pub fn stat_bars(spec: &Spec) -> [f32; 5];         // power, grip, handling, braking, weight 0..1 for E's bars
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Refusal { Marks { need: u32 }, Owned, MaxLevel, Locked { grade: u32 } }
impl Garage { pub fn can_buy(&self, w: &Wallet, g: &Grade, model: &str) -> Result<u32, Refusal>;
              pub fn can_upgrade(&self, w: &Wallet, kind: UpgradeKind) -> Result<u32, Refusal>; pub fn active_car(&self) -> &OwnedCar }
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)] pub struct Grade { pub xp: u32 }  // level() 1..=30
// UI → career requests (E sends after its confirm modal; F applies, saves, toasts on refusal):
#[derive(Message, Clone, Debug)] pub enum CareerRequest { BuyCar(String), BuyPart(UpgradeKind), SetActiveCar(usize), Paint(u8) }
#[derive(Message, Clone, Copy, Debug)] pub struct MoneyEarned { pub amount: u32 }         // audio + HUD chip
#[derive(Resource, Clone, Debug, Default)] pub struct LastPayout(pub Vec<(String, u32)>)  // lines for E's results card

// src/career/style.rs — F (UX_PASS "style"; AUDIO_PASS's SkillEvent folded in).
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum StyleKind { Drift, Speed, Air, NearMiss, Clean }
#[derive(Message, Clone, Copy, Debug)]
pub enum StyleEvent { Tick { kind: StyleKind, points: u32, chain: u8 }, Bank { points: u32, chain: u8 }, Lost { points: u32 } }
#[derive(Resource, Clone, Debug, Default)] pub struct Streak { pub points: u32, pub chain: u8, pub timer: f32 }

// src/open/activities.rs — F. Placed from assets/levels/open/activities.ron (NOT the level file A re-bakes).
#[derive(Message, Clone, Copy, Debug)] pub struct SpeedTrap { pub pos: Vec3, pub kmh: f32, pub target_beaten: bool, pub personal_best: bool }
#[derive(Message, Clone, Copy, Debug)] pub enum ActivityCue { Countdown(u8), Go, Checkpoint, Finish { place: u8, best: bool }, Fail }

// src/career/racers.rs — F.
#[derive(Component)] pub struct Racer { pub slot: u8, pub car: Car, pub spec: Spec, pub pilot: pilot::Pilot, pub prev: (Vec3, Quat) }
#[derive(Default)] pub struct RacerSolids(pub std::sync::RwLock<Vec<(Vec3, f32 /*heading*/)>>);  // published after each step

// src/open/furniture.rs — B (pure, tested; built with OpenWorld, from the road net + heights, seeded).
#[derive(Default)] pub struct Furniture { pub rails: Vec<RailRun>, pub signs: Vec<Sign>, pub patches: Vec<Patch> /*sorted, grid-indexed*/ }
pub enum PatchKind { Pothole { depth: f32 }, Wet, Washout }      // Wet→Surface::Wet, Washout→Surface::Loose
pub fn dip(f: &Furniture, x: f32, z: f32) -> f32; pub fn surface_override(f: &Furniture, x: f32, z: f32) -> Option<Surface>;
pub fn walls(net: &RoadNet, h: &Heightfield, out: &mut WallGrid);
```

Menu wiring (E ↔ F): E owns `MenuConfig` and every screen. Dynamic lists (cars, parts) use
fixed id arrays such as `"dealer:0".."dealer:15"` and `"part:0".."part:5"` (the `ROW_IDS` pattern,
because `MenuAction::Custom` takes `&'static str`). `on_menu` turns those ids into a
`CareerRequest`. Screens read `Wallet`/`Garage`/`Grade`/`LastPayout` and call `can_*` to grey
out buttons and give the reason. The **active car lives in `Garage.active`, not in `meta::Profile`**;
E adds only `last_pose` to `Profile`, and this overrides UX_PASS §7.2. F applies the car between runs:
`CarSpec(spec_of(active).with_assists(&prefs))`, set before `loading::begin`, so the assist and
damage overrides in `prefs` still apply.

## 4. Performance budgets

Reference: `tools/perf/budget.py`, offscreen 1080p, governor off, idle machine, same binary
for A/B (`look.feature` toggles). We only have an M5 Pro. A base M1/M2 is roughly **3× slower on
the GPU**, so every GPU delta below is the M5 Pro number, and the base-Mac figure is ×3. The base
Mac launches at Medium (`hardware.rs`). Baseline (2 Nov, Low, M5 Pro): GPU p95 4.0 ms, frame p95
15.6 ms (dev build: **CPU-bound, about 1 ms of headroom**), 666 draws (budget 800), 1.33 GB.

| Stream | GPU Δ (M5 Pro, Medium) | CPU Δ per frame (dev) | Draws Δ / entities Δ | Strategy |
|---|---|---|---|---|
| A town (hundreds of buildings) | ≤ +0.4 ms | ≤ +0.2 ms | ≤ **+60** draws; ≤ +1.5 k entities | Today each building is its own mesh (`buildings.rs:1264-1276`, 110 draws, plus shadow passes). **Merge per 128 m tile per material** at load, keeping a `VisibilityRange` per tile, and drop details past 300 m (ART_BIBLE §5). The per-building `Building` level entities stay for the editor (A keeps per-building meshes in editor mode, for `live.rs`) and are not drawn in play. Collision stays as facade `Wall`s. Terrain smoothing: no cost change |
| B rails, signs, conditions | ≤ +0.2 ms | ≤ +0.05 ms | ≤ **+20** draws (1 merged mesh per kind per tile); 0 entities per post | One shared mesh and material (or atlas) per kind, merged per tile, `Scenery`, no shadows on posts or signs, range 250 m. Breakable reflector/km posts reuse the existing props path. Conditions: mesh patches 2 mm over the road, merged per tile, **not** `ForwardDecal` (Low has no depth prepass) |
| C sky, clouds, day | clouds ≤ +0.5 ms (Medium, 4 steps), ≤ +0.7 ms High; Low static ≤ 0.05 | ≤ +0.1 ms average; no spike > 2 ms | +1 draw | **Cloud layer = one camera-following shell mesh**, unlit transparent `Material` (`clouds.wesl`) that ray-marches the 1.8–3.2 km shell in 4/8 steps. It is depth-tested so only sky pixels shade. No render-graph node, no engine change (half-res comes later from the engine). `Look` writes about 1 Hz, `env_refresh_hz` 0.05. Moon = the same sun light, re-aimed (no second shadowed light). Headlight and lamp lights are spawned at load with a fixed shadow config (keeps the shadow layer count steady) |
| D blur, flames, camera | blur ≤ +0.5 ms High (engine-measured +0.04); flames ≤ 0.05 | ≤ +0.05 ms | ≤ +4 draws; ≤ 32 sprites | `MotionBlur` only where the tier allows it (High+), which already has motion vectors from TAA. **Never insert or remove it in play**: D writes `shutter_angle` on the existing component in `Update.after(QualitySet::Look)` (otherwise `apply_look` resets it each time the clock moves the look). Flames: 2 additive cones, shared mesh, pooled `Actor`, `NotShadowCaster` |
| E UI | 0 in play | HUD ≤ 0.1 ms | ≤ 60 UI nodes in play | Rebuild text only when it changes. Showroom only under `MainMenu` (`DespawnOnExit`). Loading must stay ≤ +0.5 s over today (~10 s) |
| F AI and HUD | ≤ +0.3 ms (5 cars) | AI ≤ **0.5 ms**; style ≤ 0.05; popups ≤ 0.1 | ≤ +15 draws; ≤ 20 popup nodes | AI cars reuse the traffic car meshes and LODs (`art/traffic_car.rs`), `Actor`, a shadow only within 60 m. **5 AI cars approved; 8 needs a measurement; 12 (AUDIO_PASS §4) is not approved.** Traffic is off in races, which pays for the AI |

Totals: GPU ≤ +1.6 ms on Medium on the M5 Pro (≈ +5 ms on a base Mac, which then needs Low;
ART_BIBLE §7's ≤ 3.5 ms High budget still holds). Draws stay ≤ 800 at Low. Memory ≤ +60 MB
(cloud noise 0.5 MB, sign atlas 4 MB, merged town < 30 MB). CPU is the scarce resource: each
stream reports its own `span` timing.

How to measure (each stream, before handing back): `python3 -I tools/perf/budget.py --quality Low`
and `--quality Medium`. Write the table row into `docs/ASSET_PIPELINE.md` "History" in your own
hand-back, not in the file (the lead appends it). For an A/B, use `look.feature`
(`motion_blur`, `auto_exposure`, ...), `render.stats` (draws, cameras ≤ 32) and `perf.stats`
at fixed poses (`art.teleport`), plus screenshots at 06:30 / 12:00 / 19:00 / 23:00 (`look.time`, C).

## 5. Determinism rules

- `vehicle.rs` stays pure. No frame time, RNG, `HashMap` order or ECS in `step`. Trig goes through `libm`.
  Any change to how the same inputs drive needs a `REVISION` bump, and the lead approves it.
  **No stream in this update changes `step`.**
- Upgrades produce a different `Spec`. `ghost::sim_hash` already hashes `format!("{spec:?}")`
  (`meta/ghost.rs:79`), so ghosts filter by car and upgrades automatically. **But board records
  don't**: `board::Record` has no car key, and the ghost file name only keys by team. F adds
  `Record.car: String` (`car_key`, `#[serde(default = "talus")]`) to the content id and the ghost file name, and boards
  rank per car class. Older records count as `talus` stock.
- `DayClock`, weather visuals, `SpeedFeel`, pops, camera and HUD never feed the sim. Time of day
  is **not** part of the open-world event condition key (it is continuous and look-only). Weather
  stays in the key, until rain makes surfaces wet.
- Road conditions and rails come from the road net and heights through seeded, sorted generation
  (`furniture.rs`, tested for the same output twice). They change the sim, so the furniture
  revision `furniture::REV` goes into the event `stage_hash`. A's re-bake already changes every
  level hash, which **resets all open-world boards and ghosts** (expected; say so in the release notes).
- AI racers: own `Racer` component, own `FixedUpdate` system **after** `SimulateSet`, stepped in
  `slot` order, `pilot::Pilot` pure (no RNG, or a seeded one per slot). The player sees the
  racers as solid obstacles through `RacerSolids` (positions from the previous step). Racers
  see the player and each other the same way. **Only the new `EventKind::Race` has racers. Races
  record no ghost and no time-trial board** (results are positions). Rally, Sprint and Lap stay
  solo and bit-exact, like traffic today (`world.rs:174-180`). Racers never get `DrivenCar` (`drive::simulate`
  would drive them with the player's input and `CarSpec`).
- Style scoring and activities are read-only observers of `DrivenCar`, after `SimulateSet`.
- Gates before merging: `scripted_drive_is_bit_identical`, `stages_hash_the_same_everywhere`,
  `obstacles_the_same_everywhere` and `ghost.verify` on a recorded event run. Only A may update
  the stage and obstacle goldens, in the re-bake commit with a reason.

## 6. Build and test logistics (parallel worktrees, 45 GB free)

**Decision: one shared target dir** (`.cargo/config.toml` → `shroud/target`, tracked, so
worktrees inherit it). A separate debug target costs 10–15 GB (`engine/target-bench` is 15 GB),
so six would need 60–90 GB, more than the 45 GB free. Rules:
- Worktrees: `/Users/himanshu/codes/game/pacenote-wt/<stream>` on branch `fest-<stream>` (TEAM.md).
- Every cargo command is `memgate -- cargo …`. Waiting in the queue is normal, and cargo's
  build-dir lock also serialises builds. No `--release`, no `cargo clean`, no new crates.
- Inner loop: `memgate -- cargo check -q --lib` and module tests (`cargo test -q --lib career`).
  Run the full suite once before handing back.
- Expect the **pacenote crate (not Bevy) to recompile** after another worktree has built: all
  worktrees share its artifact hashes. That's the price of sharing; keep builds batched.
- Run the game **only through `engine/scripts/play.py --manifest <wt>/Cargo.toml`** or
  `tools/perf/budget.py`. play.py builds and then runs a **private copy** of the binary, so
  another worktree overwriting `target/debug/pacenote` can't affect a run in progress. Never launch
  `target/debug/pacenote` directly. If you must, `cp` it right after your build to
  `/Users/himanshu/codes/game/pacenote-wt/bin/<stream>/`.
- `cargo test` runs `deps/pacenote-<hash>`, which another worktree can replace between build and
  run. If results look foreign (tests you don't have), re-run.
- Blank the SMTP keys for runs (`SMTP_USER= SMTP_PASS=`). If `df -h` shows < 20 GB, stop
  and tell the lead. Remove worktrees once merged.
- The level re-bake (A) is the only job that writes `assets/levels/open*`. Other streams don't
  commit level files: F's activities live in `activities.ron`.

## 7. Merge order and risks

Order (the lead rebases each branch on `main` and runs the full suite plus `budget.py` Low after each merge):
0. **Wave 0 contracts** (lead) → everyone branches.
1. **D Car feel**: self-contained (drive/audio/car art); gives `SpeedFeel` and `ExhaustPop` to the rest.
2. **C Look**: `art/look.rs` and new files; sets the lighting baseline every later screenshot depends on.
3. **A World**: big re-bake, golden hash updates, new terrain. Merging it early keeps B and F on final ground.
4. **B Roadside**: rebase onto A, then re-run placement on the final roads and terrain. Rails become solid here.
5. **F Gameplay**: rebase onto A (activity coordinates in `activities.ron` checked against the
   new level by a test), then races, economy and boards.
6. **E UI**: last. It shows everything; it only touches UI files, so the rebase is mechanical. E
   also lands the final `prefs::LAYOUT` order.

| Risk | Mitigation |
|---|---|
| A's re-bake resets every event board and ghost; golden hashes change | Expected. One commit, goldens updated with a reason, release note |
| Night is unplayable without lights (5× clock reaches night in ~1.6 real hours) | `DaySettings` hides "Night" until D's headlights and C's moon land (ART_BIBLE §7.7). Start defaults to Morning |
| Pipeline stalls from switching features mid-play (sky mode, blur insert, volumetrics at dusk) | §1 rules. Volumetric shafts are on or off **per load** (High+), never toggled by the sun's elevation; their density varies instead |
| Spot-shadow texture rebuilds when headlights switch on | Spawn the shadowed headlight at load with intensity 0 by day; never toggle `shadows_enabled` |
| `apply_look` every frame from the clock | Quantised sun updates about 1 Hz, `env_refresh_hz` 0.05; C proves it with `look.get` (apply count) |
| Town density blows the Low draw budget (666/800 today) | Per-tile merge is mandatory, not optional. A's hand-back reports `render.stats` draws |
| AI cars break ghost exactness | Racers only in `Race`; races have no ghosts or time boards; the bit-exact tests are merge gates |
| `Surface::Wet` added in 6 exhaustive matches | Done once in wave 0, so later conflicts are impossible |
| CPU headroom ~1 ms (dev, frame p95 15.6 ms) | Per-stream CPU caps (§4). F's AI at 0.5 ms is the biggest; F reports `span` timing |
| Shared target churn and lock contention slow everyone | `check` in the inner loop, batched builds, play.py private copies |
| Clouds missing from reflections | Accepted for this pass. Engine request: render the cloud material into the atmosphere env cubemap |
| Base-Mac numbers are extrapolated (×3) | Flag it in the release notes; the owner measures on a real M1/M2 before any Medium default changes |
