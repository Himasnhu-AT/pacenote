# Festival UX pass: hub, loading, driving HUD, garage, settings

Status: Draft for implementation · Author: ux-designer + ui-programmer (autonomous `/team-ui`, UX
authoring phase) · Platform target: PC (keyboard + mouse, gamepad), 1080p reference, 720p minimum
(Steam Deck 800p) · Builds on the engine UI kit (`engine/src/ui`) and `docs/META.md`, `docs/ART.md`.

**Run record (team-ui Phase 0/1a).** Active set (team.size individual): ux-designer + ui-programmer.
Not spawned: art-director, accessibility-specialist, engine UI specialist; their concerns go
through these two. Inputs: game concept → `docs/GAME_OVERVIEW.md` (stand-in); `design/player-journey.md`
ABSENT; GDD UI requirements ABSENT (owner's brief used instead); `design/ux/interaction-patterns.md`
ABSENT (no pattern library: every pattern below is new and listed in §7.5); `design/accessibility-requirements.md`
ABSENT, so **no committed tier**. Assumed tier: Standard (focus order, contrast, no colour-only signals).

> **Superseded names:** `DECISIONS.md` in this folder overrides this doc. Names below were
> updated to the canonical ones (Notes, Rhythm, Valley Trials, Saker R5, stars); its rulings on
> Rhythm scoring (S1), settings and file ownership win over §3.1, §5 and §7 here.

**Names (ours, original).** The festival is the **Valley Trials**. The hub is the **Service
Park**. Money is **Notes** (`12,400 N`). Points for driving with flair are **style**. A run of
linked tricks is a **Rhythm**. Profile level is the **driver grade** (Grade 1–30, earned with
style). Activities score 1–3 **stars** (★★☆): in rally-plate yellow, filled
or hollow, so the shape tells you the result as well as the colour.

**Look rules (all screens).** Keep the existing theme (`meta::screens::theme`): night-forest
green surfaces, map-paper text, rally-plate yellow accent, Barlow Condensed. Contour-map lines and
stage-card framing are the motif; no chrome, no neon, no tilted tiles. Event kind colours stay
(orange rally, blue sprint, purple lap). Each kind also gets a glyph: rally ◆, sprint ▶, lap ○,
and activities get trap ⊢, zone ═, drift ∿, jump ⌒.

## 1. Main menu: the Service Park hub
**Player need:** "I want to get driving in one press, and see what to do next and what I can
afford." They arrive from boot (cold), from an event's results, or from "Quit to menu".

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ PACENOTE · VALLEY TRIALS           GRADE 7 ▰▰▰▰▰▱▱  2 340/4 000   48,250 N  │ top bar
│───────────────────────────────────────────────────────────────────────────────│
│  DRIVE ───────────                                                            │
│  ▌CONTINUE              Free roam · Mill Lane                                 │
│   EVENTS                12 / 31 ★                [ live 3D: Saker R5 on a gravel │
│   DRIVE TOGETHER                                   plinth, floodlit service   │
│  CAR ─────────────                                 tent, slow 40 s orbit ]    │
│   GARAGE                Saker R5 #27                                             │
│   UPGRADES              2 parts affordable                                    │
│   DEALERSHIP            1 new car                                             │
│  YOU ─────────────                       ┌ ROUTE CARD (focused item) ───────┐ │
│   RECORDS                                │ CONTINUE                          │ │
│   SETTINGS                               │ Back where you parked: Mill Lane, │ │
│   QUIT                                   │ dusk, clock running 5×.           │ │
│                                          │ Nearest: Sawmill Sprint 1.2 km ▶  │ │
│                                          └───────────────────────────────────┘ │
│ ⏎/Ⓐ Select   Esc/Ⓑ Back   Q E/LB RB Section                       CREDITS     │
└───────────────────────────────────────────────────────────────────────────────┘
```

- **Items** (sections are labels, not focusable): DRIVE: Continue (free roam at the last parked
  spot; first launch says "Start: free roam" and drops you in the village), Events (list, §1.1),
  Drive together (the existing "together" screen). CAR: Garage, Upgrades, Dealership. YOU:
  Records (grade, totals, the existing leaderboards), Settings, Quit. Credits stays a footer link.
  The demo's store button goes under Continue (as now).
- **Right-hand meta** per item (muted, SIZE_BODY): the item's most useful number. It is never
  blank: when the source is absent it shows nothing rather than "0".
- **Route card** (bottom right; replaces `crew_card`): what the focused item does and the next step
  ("Upgrades: Gravel dampers 6,000 N, you have 48,250 N"). Rebuilt on focus change.
- **Top bar**: festival name left; grade with a 7-segment bar (filled vs hollow) and the xp as
  numbers; Notes right. When Notes change since the last visit, the number counts up over 0.6 s
  (instant with reduce motion).
- **Background: live 3D showroom, not a world flyover.** Decision: the open world takes a full
  loading screen to build (§2), so a flyover behind the menu would put that load on every boot. A
  small diorama costs almost nothing: `assets/models/rallycar.glb` as a `SceneRoot` (no sim) on a
  gravel disc, one spot light and a fill light, a canvas tent backdrop, contour-line ground decal,
  camera orbiting at 9°/s, 6 m out, 1.1 m high. The same scene serves Garage, Upgrades and
  Dealership with different camera framings. The menu first draws with the current 2D contour
  backdrop, and the diorama fades in over 0.5 s once the scene is ready and two frames are drawn
  (starelines compile behind the 2D backdrop, so the macOS first-draw stall is never shown).
  Reduce motion: the orbit stops at a 3/4 front view.
- **Navigation.** Keyboard: ↑/↓ (W/S) move focus and wrap; Enter selects; Esc does nothing on the
  hub (Quit is an item). Gamepad: d-pad or stick, Ⓐ, Ⓑ. Q/E or LB/RB jump to the next or previous
  section's first item. Mouse: hover focuses, click selects. Focus order: Continue → … → Quit →
  Credits. On return from a sub-screen, focus goes back to the item that opened it.
- **Quit** opens a confirm ("Quit to desktop?", focus on **Stay**). Nothing is unsaved: Notes and
  progress are saved when they change.
- **Transitions.** Hub ↔ sub-screen: the list slides 24 px and fades over 160 ms, and the showroom
  camera eases to the screen's framing over 0.6 s (cubic in-out). Reduce motion: the list cuts
  and the camera moves at once. Hub → drive: the loading screen fades in over 200 ms.

### 1.1 Events screen
A list grouped by kind (Rallies, Sprints, Laps, Races vs AI, Activities). Each row: glyph, name,
length, best time or score, stars ★★☆, reward. A filter by kind is Q/E. A details panel (right)
shows the route map preview (`StageCatalog::preview`), conditions and the leaderboard top 3.
Enter = **Drive there**: loads free roam with the car at the event start and the route set.
Tab = leaderboard (existing "boards"). Empty state (no events known yet): "Events appear as you
discover them in the valley. Continue to start driving."

## 2. Loading screen
Keep the current structure (map bleeding off the right edge, text column left) and make it an
**event briefing**: it is up for 3–20 s, so it should tell the player something useful.

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ ◆ RALLY · STAGE 07 · DUSK, OVERCAST                     ╭─contour map──────── │
│ HIGH CAIRN                                              │   route drawn in    │
│ Moor · gravel 81%, tarmac 19% · 11.4 km                 │   accent, start ●   │
│                                                         │   finish ▣, fades   │
│ ┌ BRIEFING ──────────────────────┐                      │   in when ready     │
│ │ Your best  6:42.31   ★★☆        │                      │                     │
│ │ Leader     6:30.05  (Moss&Kit)  │                      │                     │
│ │ Reward     2,500 N  · +600 xp   │                      │                     │
│ │ Opponents  5 AI · you start P6  │ (races only)         │                     │
│ └─────────────────────────────────┘                      │                     │
│ TIP 2/6  The handbrake helps the car rotate into hairpins.    ‹ ›            │
│ BUILDING TERRAIN AND ROAD ··· 2/4                                              │
│ ████████████████████░░░░░░░░░░░░░░░░░░░░░░                                     │
└───────────────────────────────────────────────────────────────────────────────┘
```

- **Content.** Kicker with kind glyph and conditions; event name (display, existing sizes);
  facts line (exists); briefing card (best, leader, reward, opponents, stars); the route map (the
  worker-drawn contour map exists; draw the event route on it in accent). Free roam: the card
  shows grade, Notes, "Parked at Mill Lane", and 3 nearest unfinished activities.
- **Progress uses the real phases** from `loading.rs`: Generate "Surveying the roads", Build
  "Building terrain and road", Warm "Preparing scenery and shaders", Settle "Final checks", shown
  as `n/4`. The bar keeps its rule that it only rises; Warm's share follows `pcg_progress` and
  the near-cell count. **Build is one long frame**: the screen freezes for it, so nothing on the
  screen may suggest smooth motion through it (no spinner). The bar jumps after it, which reads as
  honest progress.
- **Tips rotate** every 7 s (crossfade 300 ms, from a shuffled list of the pool per mode, not
  just one picked by seed). ←/→ or LB/RB move to the previous or next tip by hand. Tips name the
  keys for the device used last (as `coach::controls` already does).
- **Timing.** No artificial minimum. When loading is done the screen holds 0.4 s at 100% and then
  fades out over 250 ms; for timed events the countdown starts after the fade (as now). Error
  state: after 20 s the phase line adds "· taking longer than usual"; at `TIMEOUT_S` (60 s) the
  existing give-up lets you drive and shows the notice "Some scenery is still loading".
- **Animation.** Map fades in (exists). Route line draws start → finish over 1.2 s when the map
  arrives (shown complete with reduce motion). Nothing else moves.

## 3. Driving HUD
**Philosophy:** the road is the content. Fixed readouts stay at the corners; flair and activity
feedback appear only while relevant and never cross the central 50% × 50% of the screen. Timed
events are quieter than free roam.

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ P2/6  ▲1        ┌ SPEED ZONE · Kiln Straight ─┐               48,250 N  (+750) │
│ ▲ Rook  +1.4    │ AVG 128 km/h  ★140 ★155 ★170│               [damage panel]   │
│ ▼ Vale  −0.8    │ ██████████░░░░░░  0.6 km    │                                │
│ LAP 2/3         └─────────────────────────────┘                                │
│                         (central 50% × 50%: no HUD)                             │
│                                                          ┌ RHYTHM ×3 ──────┐   │
│                                                          │ 2 480           │   │
│                                                          │ DRIFT    +1 120 │   │
│                                                          │ AIR 1.4s   +640 │   │
│                                                          │ NEAR MISS  +300 │   │
│                                                          │ ▬▬▬▬▬▬░░░ bank  │   │
│ [minimap / stage plate]          [notice pill]           └─[revs speed gear]┘  │
└───────────────────────────────────────────────────────────────────────────────┘
```

### 3.1 Style Rhythm (bottom right, above the speedo plate)
- **Tricks and base points:** Drift (angle > 15° at > 40 km/h, points per second ∝ angle × speed),
  Air (airtime > 0.4 s; bonus for a level landing), Speed (> 150 km/h held, per second), Near miss
  (traffic or AI within 1.2 m at > 80 km/h), Clean landing, Overtake (races), Clearing (snapping a
  marker post or sign; small, capped at 5 per Rhythm). The sim emits them; the HUD only shows them.
- **Stacking:** up to 4 lines, newest on top; a repeat of the same trick updates its line in place
  (`DRIFT +1 120` keeps counting) instead of adding one. Older lines past 4 collapse into the
  total. Each new line slides in from the right 12 px over 120 ms.
- **Multiplier:** ×1 at the start; +1 for each *different* trick in the Rhythm, up to ×5. Shown in
  the header (`RHYTHM ×3`), 1.0 → 1.15 → 1.0 scale bump on change (none with reduce motion).
- **Bank timer:** the bar under the lines empties over 2.5 s; any trick refills it. When it runs
  out the Rhythm **banks**: the header becomes `BANKED ✓ 7 440` in `good` for 0.8 s, then the
  panel fades and the number flies to the top-right Notes/xp chip (0.4 s; just fades with reduce
  motion). A hit over the damage threshold, a recover, or 3 s standing still **loses** it:
  `LOST ✕` in `bad`, the total struck through, shake 4 px for 0.2 s (none with reduce motion).
  Glyph and word always go with colour.
- **Conversion:** banked style → xp 1:1; Notes = style ÷ 10 (rounded to 10). In timed events
  style still banks, but the panel is **compact**: only the header line, no trick lines.

### 3.2 Activities (top centre)
- **Approach card** (within 200 m, ahead ±45°): `⊢ SPEED TRAP · Mill Lane · best 142 ★★☆ ·
  next ★ 155` for 4 s or until entered. One at a time; the nearest wins. Activities start by
  driving through; there is no stop-to-start (that stays for events, `coach` prompt).
- **Live readout:** trap = speed at the gate; zone = average speed with star ticks on the bar; drift
  zone = zone score with ticks; jump = distance shown on landing. Ticks are notches with the star
  value printed, so position as well as colour shows the target.
- **Result card** (same slot, 3.5 s): the value large, stars filling left to right 150 ms apart,
  `NEW BEST` tag (text, accent outline), reward `+400 N`. Not modal; driving continues.

### 3.3 Races vs AI (top left)
`P2/6` in display 64, then ▲/▼ with the change since the last checkpoint, then the car ahead and
behind with name and gap in seconds (updated at each split, not every frame), `LAP 2/3` for laps.
The event clock stays in the bottom-left plate. The controls card (top left) is never shown in a
race. Finish: the results screen gets a positions table (exists for times; add AI rows).

### 3.4 Notes, damage, toasts (top right)
Notes chip shows in free roam only: for 4 s after any change, then hides. Damage panel moves
below it when both show. Grade up, car bought and part fitted use the engine `Toast` (top right,
3 s). Big moments are never centre-screen pop-ups.

### 3.5 Speedo and gear
Keep the current plate (it is legible and on-style). One change: the gear box border pulses
accent → white at the shift point (2 Hz, off with reduce motion) as well as the red rev segments.

### 3.6 HUD states and priority
| Context | Shown | Hidden |
|---|---|---|
| Free roam | speedo, minimap, Rhythm, activity cards, Notes chip (on change), damage | stage plate |
| Timed event | speedo, stage plate, compact Rhythm, damage | minimap, activity cards, Notes chip |
| Race | as timed event + position block | controls card |
| Paused / map / menus | nothing (`hide_under_screens`) | all |

Priority: MUST KEEP speedo, clock, position; SHOULD KEEP damage, minimap; CAN HIDE Rhythm,
activity cards, Notes (each has a HUD setting, §5). Budget: at most 6 groups on screen, ≤ 18% of
the screen area.

### 3.7 Colour, size, contrast, safe area
- Every HUD element sits on a plate (`hud::plate`) so text holds ≥ 4.5:1 over snow and sky.
- Accent yellow = your style/selection; `good` green = banked/gained (✓, ▲); `bad` red =
  lost/damage (✕, ▼, strike-through); kind colours only alongside their glyph.
- UI scales as viewport height / 1080 × UI scale. New HUD text is never below 22 design px
  (≈ 15 px at 720p); menu text never below 18 (12 px at 720p). SIZE_TINY (14) is not used for
  anything the player must read while driving.
- HUD inset: 40 design px now; new setting HUD safe area 0–5% adds to it (TVs, the Deck's bezel).

## 4. Garage, Upgrades, Dealership
All three: list left (focusable rows, scrolls via `ScrollList`), car in the showroom centre,
stats panel right, footer hints. Back returns to the hub item that opened it.

```
┌ UPGRADES · Saker R5 #27 ─────────────────────────────────────── 48,250 N  ┐
│ ENGINE     ▸ Street ✓  Gravel ✓  Works  14,000 N         POWER   ▰▰▰▰▰▰▱▱ (+1) │
│ GEARBOX      Street ✓  Gravel    6,000 N                 GRIP    ▰▰▰▰▰▱▱▱       │
│ DAMPERS      Street ✓  Gravel    6,000 N    [3D car]     BRAKES  ▰▰▰▰▱▱▱▱       │
│ TYRES        Street ✓  Gravel ✓  Works ✓                 WEIGHT  1 230 kg (−40) │
│ BRAKES       Street ✓  Gravel    5,500 N                 CLASS   B 512 → B 540  │
│ WEIGHT       Street    4,000 N                                                  │
│ Times on upgraded cars go on the class's own leaderboard.                       │
│ ⏎/Ⓐ Fit   ←/→ Level   Esc/Ⓑ Back                                               │
└─────────────────────────────────────────────────────────────────────────────────┘
```
- **Upgrades:** rows = part, ←/→ picks a level, the stats panel previews the change as hollow
  segments with `(+1)` numbers. Owned levels can be refitted for free (downgrade allowed).
  Leaderboards are keyed by handling (`board::handling`), so the note above is permanent.
- **Garage:** owned cars, Drive (sets the active car), Paint (6 swatches: runtime tint of the
  paint material; full livery changes need the offline build, so out of scope), number 1–99.
- **Dealership:** cars with price, class, the three stats. Not-yet-built cars show as a dimmed
  silhouette marked "Coming". Empty state (only Saker R5 exists today): one row "More cars arrive
  as the Valley Trials grows" and Saker R5 marked Owned.
- **Confirm purchase** (modal, scrim 0.6):
  `Buy Gravel dampers?  6,000 N· balance after 42,250 N [ Buy ] [ Cancel ]`. Focus starts
  on **Buy** (it is a deliberate second press). Not enough Notes: Buy is disabled and the line
  reads "Need 3,200 N more"; the row shows the price in `bad` with a lock glyph. Save failure:
  toast "Couldn't save. Your purchase is undone." and the balance rolls back.
- Showroom framings: Garage 3/4 front at 5 m; Upgrades low side at 4 m, the camera nudges toward
  the part (wheel for tyres, nose for engine); Dealership swaps the car on the plinth with a 0.3 s
  fade.

## 5. Settings additions
| Tab | Row | Kind | Notes |
|---|---|---|---|
| World (new) | Start time | Choice: Morning / Midday / Afternoon / Dusk / Last | free roam; events keep their own |
| World | Clock runs | Toggle: Off (frozen) / On (5×: one day ≈ 4 h 48 m) | owner's 5× |
| Camera (new) | Camera view | moved from Game | same key |
| Camera | Chase distance | Choice: Near / Normal / Far | `ChaseCamera.distance`/`height` |
| Camera | Chase stiffness | Slider Loose–Tight (0.5–1.5×) | scales `follow` and `turn` |
| Camera | Field of view | moved from Video (`fov_deg`) | same key |
| Camera | Camera shake | moved from accessibility (`camera_shake`) | same key |
| Video | Motion blur | Choice: Off / Low / High | Bevy `MotionBlur` on the 3D camera; Off by default |
| HUD | Style Rhythm | Choice: Full / Compact / Off | |
| HUD | Activity cards / Race positions / Notes | Toggles | |
| HUD | HUD safe area | Slider 0–5% | |
| HUD | Units | exists (`units`) | km/h / mph; also used for zone targets |

Side panel for World: a sun-dial strip showing the start time; for Camera: a still of the chase
framing at the chosen distance.

## 6. Accessibility
- **Text size:** the engine's `text_scale` (RemSize) and UI scale both apply to all new screens;
  layouts are flex columns that grow, with long names wrapping to two lines, never clipped.
- **Colourblind:** the engine's filter works on the whole frame; on top, no meaning by colour
  alone: kinds = glyphs, stars = filled/hollow, bank/lose = ✓/✕ + word, position = ▲/▼, rev bar =
  segment position, purchase lock = glyph.
- **Reduce motion** (engine `reduce_motion`): no showroom orbit, no count-ups, no shakes or
  bumps, no slides; fades ≤ 150 ms stay. High contrast swaps the theme (engine) and HUD plates
  become opaque.
- **No flashing:** nothing pulses faster than 2 Hz; the shift pulse is the only HUD pulse.
- **Readability:** checked at 1920×1080 and 1280×720 with UI scale 100%: minimum 12 px rendered
  text in menus, 15 px on the HUD.
- **Input:** everything works keyboard-only and gamepad-only; mouse never required; holds have a
  tap alternative (none are needed in these screens).

## 7. Implementation notes (ui-programmer)
### 7.1 What the engine kit already gives us
Screen stack (`register_screen`, `ScreenRequest`, sticky screens), `MenuConfig`, `nav_list`,
`action_button`, `field_row`/`card_row`/`tab_button`, `Focusable`+`Focus` nav (keys, pad, mouse,
repeat), `ScrollList`, `Look` painting, `Toast`, theme helpers, `UiScale` by height, accessibility
(text scale, high contrast, colourblind, reduce motion, shake), `FieldDef` settings with side
panels. Missing: confirm modal, section jump, count-up text, a 3D showroom.

### 7.2 Files to change
- `src/meta/screens.rs`: `MenuConfig.main_items` → hub list; `build_main` → calls `hub::build`;
  register `events`, `garage`, `upgrades`, `dealer`, `records`, `confirm`; `crew_card` → route card.
- `src/meta/mod.rs`: `Profile` gains `last_pose`, `active_car`; `start_stage` gains "drive to
  event" (place the car at the event start, set the route).
- `src/loading.rs`: briefing card, route line on the map, tip rotation (`TipCycle` resource
  driven in `draw` on real time), `n/4` phase index, "taking longer" line.
- `src/hud.rs`: shift pulse; Notes chip; damage panel offset; HUD safe-area inset.
- `src/prefs.rs`: new `Prefs` fields and rows (§5), new tabs World and Camera in `layout`.
- `src/meta/coach.rs`: suppress the controls card in races; the event prompt stays.

### 7.3 New modules
- `src/career/` (state, no UI): `wallet.rs` (Notes, `career.json` in `DataDir`, atomic save),
  `grade.rs` (xp curve), `garage.rs` (owned cars, fitted upgrades, prices, handling deltas).
- `src/style.rs` (sim side): detects tricks and emits `StyleEvent { kind, points }`; owns Rhythm
  state (multiplier, timer, bank/lose) and pays the wallet on bank.
- `src/hud_style.rs`, `src/hud_activity.rs`, `src/hud_race.rs`: read-only views of `style`,
  activities and race state.
- `src/meta/hub.rs`: top bar, sectioned list, route card, section-jump hotkeys, `confirm` modal.
- `src/meta/showroom.rs`: diorama spawn on `OnEnter(AppState::MainMenu)` with
  `DespawnOnExit(MainMenu)`, camera framing per top screen (`Screens::top()`), fade-in when ready.
- `src/meta/garage_ui.rs`: garage, upgrades, dealership screens.

### 7.4 State
No new `AppState`: hub screens are screen-stack ids under `MainMenu`. The showroom reads
`Screens::top()` for its framing. Gameplay state the UI shows is owned by `career`, `style`,
activities and race modules; the UI only reads it and sends requests (`BuyPart`, `SetActiveCar`).

### 7.5 New patterns (for a future `design/ux/interaction-patterns.md`)
Sectioned hub list with section jump; route card for the focused item; confirm modal (default
action, disabled reason); count-up number; Rhythm panel (stack, multiplier, bank/lose); approach
card → live readout → result card; star rating; stat bar with preview delta.

### 7.6 Data, updates, nulls
| Element | Source (owner) | Updates | Null shows |
|---|---|---|---|
| Notes, grade, xp | `career::wallet/grade` | on change | `0 N `, Grade 1 |
| Rhythm | `style` | per trick / per frame for counting lines | panel hidden |
| Activity live value | activities module | per frame, text only on change | `—` |
| Position, gaps | race module | position per frame; gaps per split | `P–/n` |
| Best/leader in briefing | `Boards` | at screen spawn | "No time yet" |
| Upgrade prices, stats | `career::garage` | on screen build / purchase | row hidden |

### 7.7 Localisation limits
Hub item ≤ 18 chars; right meta ≤ 24; route card ≤ 140; Rhythm trick label ≤ 12; activity name ≤
22 (ellipsis); tip ≤ 120; all layouts allow 40% growth (flex, wrap in cards).

### 7.8 Acceptance criteria
1. From boot, the hub is interactive within 1 s of the window showing; the 3D showroom appears
   ≤ 3 s later without a visible stall.
2. Every hub item and sub-screen is reachable and usable with keyboard only and gamepad only;
   Q/E and LB/RB jump sections; Back returns focus to the opening item.
3. The loading screen shows the real phase name and `n/4`, the bar never goes down, tips change
   every 7 s, and ←/→ changes the tip.
4. Drift then jump then near miss shows `RHYTHM ×3`; 2.5 s idle banks it (`BANKED ✓`, Notes rise
   by style ÷ 10); a heavy hit before that shows `LOST ✕` and no Notes change.
5. Buying with too few Notes is impossible and says how many are missing; a purchase updates the
   top bar and survives a restart.
6. At 1280×720, UI scale 100%: no HUD text under 15 px, no menu text under 12 px, nothing clipped.
7. With reduce motion on: no orbit, slide, shake, bump or count-up anywhere.
8. In a race, `P#/n` and gaps show top left and the controls card never opens.
9. HUD update systems allocate nothing per frame while values are unchanged (as `hud.rs` today).

## 8. UX review (self-review, `/ux-review`, autonomous)
Checklists used: 3A for §1, §2, §4, §5 (screens) and 3B for §3 (HUD), because this one document
covers both.

**First pass found, all fixed above:** (1) no error states → loading slow/timeout, purchase save
rollback, empty Events/Dealership; (2) focus order and return focus → §1; (3) Notes chip vs damage
panel both top right → §3.4; (4) Rhythm loss shown by red only → ✕, word, strike; (5) SIZE_TINY at
720p renders at 9 px → minimum sizes in §3.7; (6) upgrades silently split leaderboards → note on the
Upgrades screen; (7) diorama first-draw stall → fade in when ready; (8) data nulls and update rates →
§7.6; (9) no localisation limits or acceptance criteria → §7.7–7.8; (10) controls card over the
race block → suppressed in races.

**Result**
- Completeness: 3A 14/15 (no per-screen Events Fired table: the actions are listed per screen
  instead). 3B 10/10.
- Quality issues: 0 blocking; 1 advisory (no Events Fired table).
- GDD alignment: NOT ASSESSED (no GDD UI requirements; the owner's brief is covered point by point).
- Accessibility: **NOT ASSESSED**. There is no committed tier (`design/accessibility-requirements.md`
  is absent). This was designed against an assumed Standard tier and meets it on paper.
- Pattern library: N/A (absent); new patterns are listed in §7.5.

**Verdict: NOT ASSESSED.** The spec is implementation-ready. Two dimensions have no criterion to
check against: running `/ux-design accessibility` would make accessibility checkable, and GDD UI
requirements would make GDD alignment checkable. Neither blocks implementation.

## 9. Built (stream E, 4 Nov 2026)

What shipped against this spec, the rulings it follows (`DECISIONS.md` U1-U7, E1-E12) and what
is left for other streams.

**Files.** `src/meta/hub.rs` (Service Park, top bar, route card, section jump, intro motion,
menu sounds, drawn glyphs), `src/meta/showroom.rs` (3D diorama and framings),
`src/meta/garage_ui.rs` (Garage, Upgrades, Dealership), `src/meta/confirm.rs` (confirm modal),
`src/meta/screens.rs` (Events, Challenges, Logbook, settings with the top bar),
`src/meta/mod.rs` (`Profile.last_pose`, `PendingStart`: Continue / Go now / Set route),
`src/loading.rs` (briefing card, n/4, tips, region map and route), `src/hud.rs` (shift pulse,
Notes chip), `src/meta/coach.rs` (no controls card in races), `src/audio/ui.rs` (`UiSound`),
`src/prefs.rs` (`LAYOUT` only: camera rows to Camera; World, Camera, festival HUD rows).

**Hub.** DRIVE: Continue, Events, Challenges, Play together · CAR: Garage, Upgrades,
Dealership · YOU: Logbook, Settings, Quit · Credits in the footer. ↑/↓ wrap, Q/E and LB/RB
jump sections, Esc does nothing, Quit asks first with focus on **Stay**. Every menu screen has
the top bar: brand, active car chip (class plate + PI, name, number), Grade with 7 filled /
hollow segments and xp in the grade, Notes (counts to a new balance over 0.6 s). Screen
changes slide 24 px and fade 160 ms; a rebuild of the same screen (after a purchase) does not.

**Showroom.** Gravel plinth in a canvas tent with contour rings on the floor, key / rim / fill
lights. Orbit 9°/s with the car right of centre on the hub; 3/4 front for Garage and
Dealership; low side turned toward the focused part on Upgrades. A veil (the 2D backdrop)
covers it until the model is in and two frames have drawn, then fades 0.5 s.

**Glyphs.** Barlow has no ✓ ✕ ★ ◆ ▶ ○ → or lock, so they are drawn once into small images
(`hub::Glyph`, tinted per use): stars filled / hollow, kind glyphs, lock for "can't afford".

**Car screens.** Upgrades rows `part:0..6` × Stock / Street / Gravel / Works (Turbo and
Gearbox stop at Gravel); ←/→ preview, Enter fits (owned levels free, otherwise a confirm with
the total of the missing levels); too few Notes shows a lock and red price and the dialog says
"Need N more" with Buy disabled. Dealership rows `dealer:0..15`, stats compared with the car
you drive. Garage: drive a car, paint (7 swatches; on the Saker paint 0 is its works livery),
solid / metallic, number −/+, repair when damaged, sell (confirm; never the last car or the
one you drive). All of it sends `CareerRequest`s; the screens rebuild when `Wallet` /
`Garage` change.

**Events.** Rows `ev:0..15`; details with top 3, mode Time trial / Race and rivals' level
(stored in `screens::EventPick`), Go now (loads on the start line, the event running), Set
route (free roam with the map waypoint set), Leaderboard.

**Loading.** The real phase with "n/4", "taking longer than usual" after 20 s, briefing card
(event: your best, record, mode; free roam: grade, Notes, events finished, route), tips
shuffled per mode with device-named keys, every 7 s with a 300 ms crossfade, Left / Right or
LB / RB step them. Open world: the painted region map in a frame, zoomed to the event route
(dotted, drawn start → finish over 1.2 s, green start, red finish) or 1.4 km around
the car ("you are here"); before it is painted, the contour backdrop. The screen fades in
over 200 ms and dissolves into the stage over 250 ms after loading (visual only; the load
logic is unchanged).

**Accessibility.** Reduce motion: no orbit (3/4 front), no slides, count-ups or shift pulse,
route shown at once, fades ≤ 150 ms. All menu text ≥ 18 design px (12 px at 720p). Nothing
depends on colour alone (glyphs, filled / hollow, words).

**Left for others.** F: apply `CareerRequest`s (the screens show the result), real
`build_spec` (Upgrades' stat preview and PI change come from it), the other 7 cars, start a
Race from `EventPick.mode`, career save of the parked pose (E reads `Profile.last_pose` until
then). C / D: World and Camera rows (their keys are pre-placed in `LAYOUT`; unknown keys are
ignored). The loading ambience bed (8 Nov): soft wind in the pines (`wind_pines_1`, -18 dB, ambience bus) fades in with the loading screen and out as the stage takes over (`audio::festival_recorded`, `bed.loading`).

**Follow-up (8 Nov 2026): safe area, World and Camera panels.**
- *HUD safe area* (`hud_safe_area` in `Prefs`, section `"prefs"`; HUD tab; slider 0–5 % in
  0.5 % steps, default 0). Each anchored block carries `hud::SafeArea` (which sides); its
  anchored sides get a `Vw` / `Vh` margin on top of the 40 px inset, so it follows window size
  with no work and updates live. Written only when the value changes or a block spawns.
  Blocks: speedo, clock, Notes chip, damage (`hud.rs`), Rhythm, race block and results card,
  activity card, co-driver call text, minimap, controls card and the centre pill/prompt row,
  voice indicator, and the engine's toast stack (tagged by its name `"Toasts"`; it is global,
  so toasts in menus move in too). The centre plates (countdown, Retired, wrecked) and the
  guide's screens (`src/guide/`) are not inset. The HUD preview draws the safe area as a
  frame.
- *World panel:* a 24 h strip (48 half-hour cells coloured by the sun's elevation from
  `daytime::solar`: night, a warm band through sunrise and sunset, day sky) with the start
  hour marked and labelled (`daytime::start_hours`, so Now, Last and the Night → Sunset
  fallback show where the game will actually start); sunrise / sunset times and whether the
  clock runs. Static: nothing moves, so reduce motion needs nothing.
- *Camera panel:* a side-view schematic from UI nodes: ground, car, the eye of the chosen view
  (cockpit, bonnet, or chase at `camera::rest_boom` for the chase distance) and its vertical
  field of view as a wedge (`fov_deg`), with a caption. No render-to-texture.
- Both panels rebuild only when the start time, time passes, view, FOV or chase distance
  changes, and only while they are on screen.
