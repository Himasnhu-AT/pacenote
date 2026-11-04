# Festival update: canonical decisions (lead)

Lead programmer, 4 Nov 2026. A consistency check of the six design docs in this folder
(`GAME_SYSTEMS`, `ART_BIBLE`, `WORLD_PASS`, `AUDIO_PASS`, `UX_PASS`, `TECH_PASS`), run in
autonomous mode. There is no entity registry, so the six docs were cross-read as the GDDs and
every shared name, number and interface was compared.

**Precedence.** This file overrides every other doc in the folder. Below it:
1. `TECH_PASS` for file ownership, interfaces, engine rules, budgets and determinism.
2. `GAME_SYSTEMS` for rules, numbers and the economy (it is the gameplay design).
3. `WORLD_PASS` for terrain, the town, placement rules and measured numbers.
4. `ART_BIBLE`, `AUDIO_PASS` and `UX_PASS` for the look, the sound and the screens.

The code names in wave 0 (`src/career/*`, `src/daytime.rs`, `src/camera.rs`, ...) are the
contract. Where this file and a doc disagree about a name, the code and this file win.

## 1. Glossary (player-facing words)

| Concept | Canonical | Retired names | Code |
|---|---|---|---|
| The series of events + challenges | **Valley Trials** | festival (internal only: folder and branch names), Valley Gathering | — |
| Menu hub | **Service Park** (main menu) | — | `meta::hub` (E) |
| The in-world service area | Service Park marker (same name, it is where the hub "is") | — | — |
| Currency | **Notes**, abbreviated **N**: "12,400 Notes" or "12,400 N" | marks, Mk, ₦ (it is the naira sign, a real currency) | `career::Wallet::balance` |
| Points for driving with flair | **Style** | style points, skill points | `career::style` |
| One skill (slide, air, ...) | **skill** | trick | `career::style::StyleKind` |
| A chain of linked skills | **Rhythm** | streak, combo, chain (in copy) | `career::style::Rhythm` |
| Banking a Rhythm / losing it | **Banked ✓** / **Broken ✕** | LOST | `StyleEvent::Bank` / `StyleEvent::Lost` |
| Profile level | **Driver grade** (Grade 1-30, xp = banked Style 1:1) | — | `career::grade::Grade` |
| Drive-through roadside tests | **Challenges**: **Radar Post**, **Pace Section**, **Slide Section**, **Yump Board** | activities, speed trap, speed zone, drift zone, jump | `open::challenges` |
| A challenge rating | **stars** 1-3, drawn filled/hollow (★★☆) so shape carries it | pips, diamonds | `stars: u8` |
| AI opponents | **rivals** (rival drivers) | AI racers (in copy) | `career::racers` |
| Event modes | **Race** (vs 5 rivals) / **Time Trial** (solo, ghost, records) | — | `career::racers::EventMode` |
| Today's car | **Saker R5** (class B, PI 570): body mesh `rallycar.glb` | Talus | `career::garage::CARS[0]`, `Spec::default()` |
| Buy-a-car screen | **Dealership** | Showroom (GAME_SYSTEMS) | `meta::garage_ui` (E) |
| The 3D car diorama behind the menus | **showroom** (internal name only) | — | `meta::showroom` (E) |
| Records, stats, ledger | **Logbook** | Records (UX) | E |
| Upgrade levels | **Street / Gravel / Works** for L1 / L2 / L3 | — | `Upgrades` (0 = stock) |
| Car tiers | Class **D / C / B / A / S** from **PI** | — | `garage::{pi, Class}` |
| Two-player mode | **Play together** (existing label) | Drive together | — |

Number style: thousands with a comma ("48,250 N"), tabular figures, km/h or mph per `units`.

## 2. Decision table

### 2.1 Economy and garage (F; E shows it)

| # | Decision |
|---|---|
| E1 | **Start state:** the Saker R5 (stock, `Spec::default()`) is owned and active, plus **12,000 Notes** (`STARTING_BALANCE`). The Wren 1.4 is no longer free: it is a 9,000 N car in the Dealership like the others. |
| E2 | **Roster:** GAME_SYSTEMS §4.1, all 8 cars available from the start (UX's "Coming" silhouettes are dropped). Names `CarDef`/`CARS`/`build_spec`/`pi`/`Class` from GAME_SYSTEMS; TECH_PASS's `CarModel`/`catalog()` are dropped. `spec_of(&OwnedCar)` is kept as the convenience wrapper over `build_spec`. |
| E3 | **Pacing check (no price change needed).** 12,000 N buys about half of the Saker's Street (L1) parts (all seven cost 20,500 N at class factor 1.0), so the first Garage visit is a real choice. A newcomer at Pro earns about 8-12k N in the first 25 min (the WORLD_PASS loop: ~4 first stars = 1,600, one first win = 2 × 2,100, Rhythm banks ≈ 200 N/min). So by 20-30 min they can afford the next step: an Engine Gravel (L2, 7,000), a full Street set, or a second car (Marten 14k, Bruin 24k). The Vale GT (30k) comes at about 1 h, the Harrier S (62k) at about 3 h, the top car with a full build after about 8 h (as GAME_SYSTEMS). |
| E4 | **Upgrades: 7 parts** from GAME_SYSTEMS §4.2 (Engine, Turbo, Tyres, Brakes, Suspension, Weight, Gearbox). Turbo and Gearbox stop at L2. TECH_PASS's 6-part list (with "Drivetrain") and UX's six rows are replaced. UX's "Dampers" row = Suspension; E's fixed ids are `part:0..part:6`. |
| E5 | **Buying levels:** fitting a level you don't own buys every missing level below it (price = their sum). Owned levels can be refitted for free, downgrades included (UX). So `OwnedCar` keeps both `fitted` and `bought`. |
| E6 | **New `Spec` fields:** `Spec.grip` (tyre μ multiplier, default 1.0) and `Spec.shift_time` (default = today's constant) are approved for F, read by `vehicle::step`. They are an exception to TECH_PASS §2 "no change to `step`", with conditions: no `REVISION` bump, `scripted_drive_is_bit_identical` and the handling tests pass unchanged, and **`ghost::sim_hash` of a stock Saker stays equal to today's** (hash the new fields only when they differ from default), so classic-stage ghosts survive. They land in F's first commit, before any other F work. |
| E7 | **Wallet:** `Wallet { balance, earned, spent }` (u32, Notes). TECH_PASS's `marks` and GAME_SYSTEMS's `u64` are dropped. Only the economy mutates it, through `Payout` / `Charge` messages with a `PayReason` (GAME_SYSTEMS §8). `MoneyEarned` is the after-the-fact notification for audio and the HUD chip (TECH_PASS/AUDIO_PASS). Both exist. |
| E8 | **Active car lives in `Garage.active`** (index into `Garage.owned`), not in `meta::Profile` (TECH_PASS wins over UX §7.2). E adds only `last_pose` to `Profile`; the career save's `world.pos` (GAME_SYSTEMS §4.4) is the same data: **F owns it in `career.json`**, E reads it. |
| E9 | **Save:** `<DataDir>/career.json`, the GAME_SYSTEMS §4.4 shape (versioned, atomic, `career.bad.json` on corruption). Grade and the Rhythm stats are in it too. Settings stay in `settings.json`. |
| E10 | **Paint:** GAME_SYSTEMS: any of the 7 traffic paints, solid or metallic, free (`Paint { colour, metallic }`). UX's race number 1-99 is kept (`OwnedCar.number`). |
| E11 | **No grade locks** on cars in this update (GAME_SYSTEMS has none). TECH_PASS's `Refusal::Locked` is dropped. |
| E12 | **Records:** one board per event (GAME_SYSTEMS §4.3). `board::Record` gains `car: String` (`car_key`, `serde(default = "saker")`) and `pi: u16`; the board shows a class chip. TECH_PASS's per-class ranking and UX's "upgraded cars go on the class's own leaderboard" note are dropped (a class filter can come later). Ghost file names include the car key (TECH_PASS §5). |

### 2.2 Style and the Rhythm (F)

| # | Decision |
|---|---|
| S1 | **Rules and numbers: GAME_SYSTEMS §2** (skills, thresholds, multiplier +0.2 per skill, +0.1 for variety, cap ×6.0, timeout 3.5 s, Notes = Style / 25 capped at 2,000 per Rhythm). UX §3.1's own rules (×1-×5 by distinct tricks, 2.5 s timeout, marks = style ÷ 10, "3 s standing still loses it", near miss at > 80 km/h) are replaced. |
| S2 | **Message:** `StyleEvent { Tick, Bank, Lost }` (TECH_PASS). GAME_SYSTEMS's `SkillEvent` and its seven variants are folded in: `Tick` carries kind + points + chain; `Bank` carries points, multiplier and Notes; `Lost` carries the points and a `LostReason`. Live state (pending P, ×M, timer, scuffs, live skills) is read from the `Rhythm` resource, not sent as messages. |
| S3 | **Skill kinds:** GAME_SYSTEMS's eleven (Slide, Switch, FlatOut, Air, CloseShave, Slipstream, Splinter, Switchback, CleanLine, CleanPass, Challenge). TECH_PASS's 5-kind `StyleKind` is extended to these. |
| S4 | **HUD:** UX §3.1's layout (bottom right above the speedo, lines update in place, compact in timed events) with **at most 3 lines** (GAME_SYSTEMS + ART_BIBLE; UX said 4). The timeout bar shows the 3.5 s. Setting **HUD › Style HUD: Full / Compact / Off** (UX's tab, GAME_SYSTEMS's "Minimal" is "Compact"). It lives in `CareerSettings`. |
| S5 | **Grade xp** = banked Style 1:1 (UX). The level curve is F's. |

### 2.3 Challenges (F, stream CHALLENGES)

| # | Decision |
|---|---|
| C1 | **Names** from GAME_SYSTEMS: Radar Post, Pace Section, Slide Section, Yump Board. Module `open::challenges` (TECH_PASS's `open/activities.rs` is renamed), HUD `hud_challenge.rs` (was `hud_activity.rs`). Messages `RadarFlash` (was `SpeedTrap`) and `RunCue` (was `ActivityCue`; it also carries event/race countdowns). |
| C2 | **Count: the 13 hand-anchored challenges of WORLD_PASS §5 now** (Radar 4, Pace 3, Slide 3, Yump 3). The seeded placer that fills to GAME_SYSTEMS's 24 (7/5/6/6) is **later**, not this update. Totals in copy: "13 challenges", 26,000 N for all stars. CH-1's acceptance becomes "the 13 anchors load, in order, on the road"; CH-7's "the map shows all 24" becomes "all 13". |
| C3 | **Storage:** `assets/levels/open/challenges.ron` (F-owned; TECH_PASS's `activities.ron`), not `ChallengeStart` placeables in the level file, because A's re-bake rewrites the level. Each entry stores **road index + station `s`** (plus the world x,z as a check). A test loads it against the committed level and fails if a gate is off its road. |
| C4 | **Star thresholds** are baked (`--bake-challenges`, GAME_SYSTEMS §3) into `challenges.ron`, after A's re-bake (WORLD_PASS §6.5). Until then, F uses GAME_SYSTEMS's expected ranges as placeholders. Bake car: stock Saker R5, pace 0.85. |
| C5 | **Yump crests:** A sculpts the 3 crests in the re-bake (WORLD_PASS §5 "Yump sculpt"); B puts the jump danger sign at them. |

### 2.4 Rivals and races (F)

| # | Decision |
|---|---|
| R1 | **Race is a mode, not an event kind.** GAME_SYSTEMS's flow wins over TECH_PASS's `EventKind::Race`: at a Rally or Sprint marker the pill offers **Race** (Enter, with a difficulty) or **Time Trial** (T). TECH_PASS's determinism rule still holds, keyed on the mode: only Race runs have rivals, Race runs record no ghost and no board time; Time Trial stays solo and bit-exact. Lap events stay Time Trial only. |
| R2 | **5 rivals** (6 cars). AUDIO_PASS's 12-car acceptance item becomes 5; 8 needs a measurement first (TECH_PASS §4). |
| R3 | **Names:** TECH_PASS's `pilot::Pilot` (pure, non-dev driving core moved out of `autopilot.rs`) replaces GAME_SYSTEMS's `ai::Driver`; `PilotParams` replaces `DriverParams`. `Racer` component and `RacerSolids` per TECH_PASS, `RaceField`, `RaceResult`, `Difficulty`, `OtherCars` per GAME_SYSTEMS §8. All in `career::racers` (and `pilot`). |
| R4 | **Solids:** `RacerSolids` holds `(pos, forward)` per car, the same shape as `traffic::TrafficSolids`, so the 3-circle traffic solid shape is reused (GAME_SYSTEMS §5). TECH_PASS's `(Vec3, heading)` is replaced. Racers never get `DrivenCar`. |
| R5 | **`OtherCars`** is rebuilt each fixed step by F from the traffic (read-only) and the rivals. It is the one list style, the HUD and the minimap read. |
| R6 | **Race HUD:** UX §3.3's top-left block (P2/6, ▲/▼, ahead/behind with gaps at splits, LAP) **plus** GAME_SYSTEMS's rival dots on the progress bar. |
| R7 | **Purses, shares, fees:** GAME_SYSTEMS §4. UX's "Reward Mk 2 500 · +600 xp" briefing line becomes "2,500 N" (xp only comes from Style). |

### 2.5 Time of day, sky and night (C; D for lights)

| # | Decision |
|---|---|
| L1 | **Clock:** `daytime::DayClock`, 5× real time (`DAY_RATE`), on `Time<Virtual>` (pause freezes it). **No `LookTimeline`** (ART_BIBLE §2.3's `LookTimeline::speed` is replaced). `Look` is written only when the sun moved one 0.02° quantum (≈ 1 Hz), `LookConfig.env_refresh_hz = 0.05` (TECH_PASS §1; replaces ART's 0.75°/20 s rule). |
| L2 | **Never switch `SkyMode` during play.** Night keeps `Atmosphere`; the stars are drawn by the cloud shader (overrides ART_BIBLE §2.3's switch to `Gradient` below −8°). |
| L3 | **Clouds = a game-side camera-following shell mesh** with an unlit transparent material (`art/clouds.wesl`) that ray-marches the 1.8-3.2 km shell, 4 steps (Medium) / 8 (High), Low a static layer. Not a full-screen ½-res render-graph pass (ART_BIBLE). Clouds in the env map and cloud shadows on the ground are **post-merge follow-ups** (engine request / `terrain.wesl` is A's file). |
| L4 | **Moon** = the sun's `DirectionalLight` re-aimed and recoloured below −6°, not a second shadowed light (TECH_PASS). ART's moon colour and 0.6 lux stand. |
| L5 | **Start-time presets** (setting **World › Start time**): Dawn 06:00 · Morning 09:00 · Noon 12:30 · Afternoon 16:00 · Sunset 19:30 · Night 23:00 · Real clock · Last (resume the saved clock). Default **Morning**. UX's shorter list and ART's "Video" tab are replaced. **World › Time passes** On/Off (default On). Free roam only: classic stages freeze at their authored `TimeOfDay`; open-world events run at whatever the clock says (look-only, not in the condition key). |
| L6 | **Night gate:** the **Night** preset (and "Real clock"/"Last" resolving to a night hour) is offered **only if the `art::headlights::Headlights` resource exists**. D's `HeadlightsPlugin` inserts it once headlights work; wave 0 defines the type but does not insert it. C also needs its moon before it shows Night. Until both land, a night start resolves to Sunset. |
| L7 | **Lights at night are D's** (CAR FEEL): 2 low-beam spots + the LightPod, plus brake/tail lamps on the player car, per ART_BIBLE §2.3 and TECH_PASS §4 (the shadowed spot is spawned at load with intensity 0 by day; never toggle `shadows_enabled`). Street-lamp `PointLight`s are C's (spawned at load from B's street-light posts). Lit windows are a post-merge follow-up. |
| L8 | **Volumetric shafts** are on or off per load (High+), never toggled by sun elevation; their density varies (TECH_PASS over ART). |

### 2.6 Car feel (D)

| # | Decision |
|---|---|
| D1 | **`ExhaustPop { car, intensity, kind, seed }`** + `PopKind { Crackle, Bang, Misfire }`, in `audio::pops` (ART_BIBLE's `Backfire { strength, kind }` is this). Trigger rules: **AUDIO_PASS §2.1-2.2** (it owns the scheduler); ART_BIBLE §3.1's slightly different thresholds are replaced. Flame look and tip position: ART_BIBLE §3.1. |
| D2 | **Turbo upgrade ↔ sound:** the engine recording is naturally aspirated, so no continuous anti-lag. A fitted Turbo (or a rally-hatch body) selects `pop_profile = Rally` (denser bursts, likelier Bang; AUDIO_PASS §2.1). `CarDef` carries the profile. |
| D3 | **Motion blur:** setting **Camera › Motion blur Off / Low / High, default Low** (ART said High, UX said Off: Low keeps the speed cue without the full smear). Only where the tier allows it (High+); never insert or remove `MotionBlur` in play, write `shutter_angle` after `QualitySet::Look`. Reduce motion forces Off. |
| D4 | **`SpeedFeel`** (0..1, smoothstep 40→180 km/h) in `camera.rs` drives FOV, blur and wind (AUDIO_PASS §5). |
| D5 | **Spring arm:** ART_BIBLE §3.2 numbers. UX's "Chase distance Near/Normal/Far" and "Chase stiffness" are rows of `CameraFeel`. |
| D6 | **Settings tab Camera** holds D's rows (`CameraFeel`, section `camera_feel`). Moving the existing `camera_view`, `fov_deg` and `camera_shake` rows there is a `prefs::LAYOUT` change: **E does it in its final commit**. |

### 2.7 World and roadside (A, B)

| # | Decision |
|---|---|
| W1 | **Road numbering stays stable through the re-bake.** Roads 0-4 keep their kinds and order (`events.rs` roles); every new road (town streets, Hill Lane) is appended at index 9 and up (WORLD_PASS §2-3). Event ids stay the same. Challenges (C3) are stored by road index + station and must be re-measured after the bake. |
| W2 | **The re-bake resets every open-world board and ghost** (level content hash). Expected; it goes in the release notes. Only A updates the stage/obstacle goldens, in the re-bake commit with a reason. |
| W3 | **Furniture lives in `open::furniture`** (B, pure, seeded, tested), reached through the wave-0 hooks (`dip`, `surface_override`, `walls`), **not** in `art::roadside::items` or per-station data in `profile::dress` (WORLD_PASS §4). Existing roadside kinds stay where they are. |
| W4 | **Placement rules: WORLD_PASS §4** (measured on the map: rail drop D > 3 m at edge + 8 m, extend 20 m, merge < 30 m, min 24 m; signs 120 m / 70 m before, ≤ 1 per 50 m, right verge; speed values that match `traffic::limit`; km posts on roads 0 and 1). **Look: ART_BIBLE §4** (materials, sign atlas, kinds). ART's own placement thresholds are replaced. |
| W5 | **Road conditions** (WORLD_PASS §4 table wins on grip): potholes = a height dip only (grip unchanged; ART's ×0.85 dropped); wet patches → `Surface::Wet`; mud → `Mud`; washouts → `Loose` + corrugation in `dip`; shaded ice → `Snow`. ART's patched tarmac, cracks and mud tongues are visual only. Drawn as merged mesh patches 2 mm over the road (TECH_PASS), not `ForwardDecal`s. The furniture revision `furniture::REV` goes into the event `stage_hash`. |
| W6 | **`Surface::Wet` grip:** `Surface::grip() = (0.72, 0.10)` (TECH_PASS) and tyre μ **0.85** in `vehicle::tyre` (WORLD_PASS's "peak μ 0.85"). These agree: both are ≈ 70-74 % of dry tarmac (1.05 / 1.15). Peak at 7.5°, tail 0.72 (wet falls away past the peak). No surface uses it until B's patches land, so no golden changes. |
| W7 | **Street lights:** B places the poles (mesh + a wall stub, WORLD_PASS); C lights the nearest 24 at night. |
| W8 | **Town:** WORLD_PASS §3 (Fernhollow) for layout and counts, ART_BIBLE §5 for roofs, walls and gardens; per-tile/per-block merged meshes are mandatory (TECH_PASS §4 ≤ +60 draws). |

### 2.8 UI (E)

| # | Decision |
|---|---|
| U1 | **Hub = Service Park**, items: DRIVE: Continue, Events, Challenges, Play together · CAR: Garage, Upgrades, Dealership · YOU: Logbook, Settings, Quit · Credits in the footer. (UX's sections + GAME_SYSTEMS's Challenges item; GAME_SYSTEMS's Showroom = Dealership, Records = Logbook.) Top bar: "PACENOTE · VALLEY TRIALS", grade bar, balance in N, plus the active car chip (name, class, PI) from GAME_SYSTEMS §6. |
| U2 | **Events screen:** "Go now" (loads at the start line) and "Set route" (GAME_SYSTEMS) replace UX's single "Drive there"; Race / Time Trial and difficulty are picked there or at the marker. |
| U3 | **No new `AppState`**; screens are screen-stack ids (UX §7.4 = TECH_PASS). |
| U4 | **E never mutates career state.** It sends `CareerRequest`s (after its confirm modal) and reads `Wallet` / `Garage` / `Grade` / `LastPayout`, calling `can_buy` / `can_fit` to grey out rows and show the reason. "Couldn't save, purchase undone" is F's rollback, shown by E as a toast. |
| U5 | **HUD settings rows:** Style HUD, Challenge cards, Race positions → F's `CareerSettings` (F's HUD files read them). Notes chip, HUD safe area → E's own `UiPrefs` section ("ui") if E needs it. All on the HUD tab. |
| U6 | **The 3D showroom** uses the active car's body (rally hatch = `rallycar.glb`, other cars = the traffic body shapes with the chosen paint). |
| U7 | **UI sounds:** `audio::ui::UiSound` (E), recipes from AUDIO_PASS §3. |

### 2.9 Engine and process (all)

TECH_PASS §1, §4-§7 stand as written: one shared target dir, every cargo command through
`memgate`, `play.py --manifest` for runs, no new crates, merge order D → C → A → B → F → E, the
bit-exact tests as merge gates, per-stream budgets measured on the M5 Pro (base Mac = ×3; this
replaces ART_BIBLE §7's base-Mac framing, whose ≤ 3.5 ms High total still holds).

## 3. Conflicts found and their resolution

Each line: what disagreed → the ruling (decision id).

**Names**
1. Currency: Notes (GAME_SYSTEMS) vs marks/Mk (UX, TECH `Wallet.marks`) vs ₦ symbol → Notes / N, no ₦ (glossary, E7).
2. Chain: Rhythm (GAME_SYSTEMS) vs streak (UX, TECH `Streak`) → Rhythm (S2).
3. Series: Valley Trials (GAME_SYSTEMS) vs Valley Gathering (UX) vs "festival" (ART) → Valley Trials.
4. Today's car: Saker R5 (GAME_SYSTEMS) vs Talus (ART, UX, TECH `car_key "talus..."`, `Spec::talus()`) → Saker R5; Talus retired (glossary).
5. Buy screen: Showroom (GAME_SYSTEMS) vs Dealership (UX), while UX uses "showroom" for the diorama → Dealership; showroom = diorama (glossary).
6. Records (UX) vs Logbook (GAME_SYSTEMS) → Logbook (U1).
7. Stars (GAME_SYSTEMS ★) vs pips ◆ (UX) → stars, filled/hollow (glossary).
8. Challenge names: Radar Post/Pace/Slide/Yump (GAME_SYSTEMS) vs speed trap/zone/drift zone/jump and "activities" (UX, AUDIO, TECH) → GAME_SYSTEMS names, module `open::challenges` (C1).
9. Messages: `SkillEvent` (GAME_SYSTEMS, AUDIO) vs `StyleEvent` (TECH) → `StyleEvent { Tick, Bank, Lost }` (S2); `Backfire` (ART) vs `ExhaustPop` (AUDIO, TECH) → `ExhaustPop` (D1); `SpeedTrap`/`ActivityCue` → `RadarFlash`/`RunCue` (C1).
10. AI driver: `ai::Driver`/`DriverParams` (GAME_SYSTEMS) vs `pilot::Pilot` (TECH) → `pilot` (R3).
11. Roster API: `CarDef`/`CARS`/`build_spec` (GAME_SYSTEMS) vs `CarModel`/`catalog()`/`spec_of` (TECH) → GAME_SYSTEMS names + `spec_of` wrapper (E2).
12. "Drive together" (UX) vs the existing "Play together" → Play together (glossary).

**Numbers and rules**
13. Starting car: Wren free + 12,000 (GAME_SYSTEMS) vs Talus only (UX) → Saker owned + 12,000, Wren buyable at 9k (E1, E3).
14. Upgrade parts: 7 (GAME_SYSTEMS) vs 6 with Drivetrain (TECH) vs 6 with Gearbox/Dampers (UX) → 7 (E4).
15. Wallet shape: `{marks: u32}` (TECH) vs `{balance: u64}` + ledger (GAME_SYSTEMS) → `{balance, earned, spent}: u32` + ledger in the save (E7).
16. Payout path: `MoneyEarned` only (TECH, AUDIO) vs `Payout`/`Charge` (GAME_SYSTEMS) → both, different roles (E7).
17. Active car in `Profile.active_car` (UX) vs `Garage.active` (TECH) vs `"selected": id` (GAME_SYSTEMS save) → `Garage.active` (E8).
18. Grade-locked cars (`Refusal::Locked`, TECH) vs none (GAME_SYSTEMS) → none (E11).
19. Boards: per class (TECH, UX) vs one board with class chips (GAME_SYSTEMS) → one board (E12).
20. Spec changes: `Spec.grip`/`shift_time` (GAME_SYSTEMS EC-2) vs "no change to `step`" (TECH) → allowed with the bit-identical and sim_hash conditions (E6).
21. Rhythm rules: GAME_SYSTEMS (×6 cap, 3.5 s, Style/25) vs UX (×5 by distinct tricks, 2.5 s, ÷10, still-stand loss) → GAME_SYSTEMS (S1).
22. Style HUD position: top centre (GAME_SYSTEMS) vs bottom right (UX) vs bottom-centre-right (ART); lines 3 vs 4 → UX position, 3 lines (S4).
23. Style HUD setting: Interface tab, Minimal (GAME_SYSTEMS) vs HUD tab, Compact (UX) → HUD › Style HUD Full/Compact/Off (S4).
24. Challenge count: 24 seeded (GAME_SYSTEMS) vs 13 anchors (WORLD_PASS) → 13 now, placer later (C2).
25. Challenge storage: level placeables (GAME_SYSTEMS) vs `activities.ron` (TECH) → `challenges.ron` by road + station (C3).
26. Racers: in every Rally/Sprint via a Race option (GAME_SYSTEMS) vs only a new `EventKind::Race` (TECH) → a mode (R1).
27. AI count: 5 (GAME_SYSTEMS, TECH) vs 12 (AUDIO §8.9) → 5 (R2).
28. Racer solids `(Vec3, heading)` (TECH) vs traffic-style circles (GAME_SYSTEMS) → `(pos, forward)` reusing the traffic shape (R4).
29. Briefing reward includes xp (UX) vs xp only from Style → Notes only (R7).
30. Night sky: switch to `Gradient` (ART) vs never switch (TECH) → never (L2).
31. Clouds: full-screen ½-res pass (ART) vs shell mesh material (TECH) → shell mesh (L3).
32. Moon: second light (ART) vs the sun re-aimed (TECH) → re-aimed (L4).
33. Clock driver: `LookTimeline` (ART) vs `DayClock` + quantised writes (TECH) → `DayClock` (L1).
34. Env refresh: 0.75° / 20 s (ART) vs 0.02° quantum + 0.05 Hz (TECH) → TECH (L1).
35. Start presets: 7 + Real clock in Video (ART) vs Morning/Midday/Afternoon/Dusk/Last in World (UX) → union in World, default Morning (L5).
36. Night gate: "Night hidden until night kit" (ART, TECH) had no mechanism → `Headlights` resource (L6).
37. Volumetric shafts by sun elevation (ART) vs per load (TECH) → per load (L8).
38. Pop triggers: ART §3.1 vs AUDIO §2.1 thresholds (50 % vs 0.6 throttle, 4,200 rpm vs 0.7 × redline, 75 % vs 90 %) → AUDIO (D1).
39. Turbo part (GAME_SYSTEMS) vs "naturally aspirated, no turbo sound" (AUDIO) → Turbo selects the Rally pop profile, no ALS (D2).
40. Motion blur default: High (ART) vs Off (UX); tab Video (ART, UX) vs Camera (TECH) → Low, Camera tab (D3).
41. Furniture code home: `art::roadside::items` + `profile::dress` (WORLD_PASS) vs `open::furniture` + hooks (TECH) → `open::furniture` (W3).
42. Guard-rail, sign, speed-sign and km-post rules: WORLD_PASS §4 vs ART_BIBLE §4 → WORLD_PASS (W4).
43. Pothole grip: unchanged (WORLD_PASS) vs ×0.85 (ART) → unchanged (W5).
44. Condition rendering: decals (ART) vs mesh patches (TECH) → mesh patches (W5).
45. Wet grip: (0.72, 0.10) (TECH) vs μ 0.85 / 0.10 (WORLD_PASS) vs 0.6-0.75 (ART) → both TECH and WORLD_PASS, in different functions (W6).
46. Budgets measured on a base M2 (ART) vs M5 Pro × 3 (TECH) → TECH (§2.9).
47. "Exhaustive Surface matches" (TECH §3.0 lists 6): the exhaustive ones are `stage.rs`, `vehicle.rs`, `meta/screens.rs`, `audio/mod.rs` and `drive.rs` (`surface_buzz`, not in TECH's list); `art/spray.rs` and `open/roads.rs` have wildcards. Wave 0 added Wet to the 5; spray's wet splash is D's.
48. `StageWorld::surface` calls `roads::surface` directly, bypassing `OpenWorld::surface` (TECH's hook would miss the player car) → wave 0 routes both through `OpenWorld::surface_at`.
49. WORLD_PASS's corridor docs vs code (cut 70 % / 30-46 m in code, 45 % / 34-52 m in `GAME_OVERVIEW.md`) → A fixes the docs (WORLD_PASS §1.7 already says so).

**Verdict: CONFLICTS FOUND, all resolved above.** No conflict is left open. Not checked: art
and audio numbers that only one doc states (they have nothing to conflict with).
