# Pacenote website: design record

## Phase 0: Brief

- **Subject:** Pacenote, an original two-person rally game (Rust, Bevy 0.19, in-house engine).
  A **driver** sees only the road from the cockpit; a **guide** sees the stage map and the
  corner list and reads pace notes over built-in voice. Solo, an AI co-driver reads the notes
  out loud. 20 fixed stages, roadside things that break (never the car), leaderboards and ghosts per stage.
- **Audience:** rally and sim-racing players, couch/online co-op pairs, Reddit readers who come
  from a post with real gameplay stats.
- **Job:** launch a free **Steam demo** and collect **wishlists**. In 3 seconds: two people,
  one car; one drives blind, one reads the road. Then prove it with the real stages and the
  real numbers.
- **Assets:** real captures from the game (`play.py`, offscreen 1920×1080; raw PNGs in
  `_source-shots/`, not shipped), the real geometry of all 20 stages
  (`src/data/stages.json`, from `examples/stage_dump.rs`: a point every 10 m, corners with
  grades 1 to 6, crests), the co-driver's real phrasing (`src/codriver/calls.rs`), the
  telemetry schema (`docs/TELEMETRY.md`).
- **Constraints:** no fake reviews, awards, player counts or quotes. Stats are labelled
  "sample" until `analyze.py` writes real ones.
- **Placeholders (`placeholder: true` in `src/data/site.ts`):** Steam wishlist and demo links
  (no app id yet), studio name, production domain (`pacenote.marlstudio.com`), release date,
  price of the full game, press email, Discord/mailing list.

## Phase 1: Concept

| | A. The pace-note book (material) | B. Two seats (how they work) | C. Start clock (formal) |
|---|---|---|---|
| Idea | The site reads you the road: a real stage drawn from the game's geometry, a car on it, and the co-driver's calls for the corners ahead, written and (on request) spoken before you get there | Every section is split down the middle: the driver's half shows only road, the guide's half only the map; you pick a seat | Everything is timed like a stage: a start countdown, split times as section numbers, a finish line |
| Anchor | the game's own corner grades (1 hairpin … 6 flat), its call phrasing ("150 left four long, into right two, careful"), the 20 real stage polylines | the game's role split: the driver never sees the map, the guide never touches the car | the stage clock, splits and the countdown in `drive.rs` |
| Signature | scroll is the throttle: the car runs down Pinewater, the next 500 m light up like the guide's map, the call sheet reads ahead; type width follows the corner grade | a divider you drag between the two views | a running clock in the corner |
| Emotion | sitting in the right seat with the notes on your lap | being half-blind | pressure |
| Screenshot | the stage map in navy, an orange car with the next 500 m lit, a huge condensed "HAIRPIN" next to a wide "SIX" | half cockpit, half map | a 00:00.0 clock |
| O / F / Fe / P | 5 / 5 / 4 / 1 | 3 / 5 / 4 / 3 | 2 / 3 / 5 / 4 |

**Pick A.** Only Pacenote has these 20 roads and this co-driver, and it puts the visitor in
the guide's seat, which is the game's own pitch. **Steal from B** for the hero (the two seats
side by side: a real cockpit capture next to the live map) and **from C** for the loader (the
start countdown) and the 404 (the co-driver's real off-road line).

### Concept card

```
Idea:        The site reads you the road: scroll drives a car down a real Pacenote stage and
             the co-driver's calls for the corners ahead appear before you reach them.
Metaphor:    the right seat of a rally car: the stage map, the pace-note book, the call.
Signature:   "Read the road" (src/motion/recce.ts): a pinned stage map from stages.json; scroll
             progress = metres along the road. The car follows the real polyline, the next
             500 m light up (as on the guide's map), corners take their grade colour, and the
             call sheet lists the next calls built by a port of calls.rs, with live distances.
             "Read it to me" speaks each call with the browser's voice (off until clicked).
             The hero runs the same module on its own clock (idle drive, no scroll).
Emotion:     first 3 s: "one of us drives, the other one talks." Then: "I want to read this
             to someone."
Motion verb: read ahead, then commit: things appear a beat early (a call), then the move
             lands decisively (expo out). Lines draw along their own length like a road.
Screenshot:  the hero: a cockpit capture on the left, the navy stage map with the orange car
             and the lit 500 m on the right, PACENOTE set wide across the bottom, the current
             call in large type.
```

## Phase 2: Art direction

**Palette** (from the game's rally car livery, `tools/art/build_rallycar.py`: white body, navy
lower half, orange slash; and the guide map):

| Role | Hex | Source / use |
|---|---|---|
| ground | `#0b1530` | livery navy (`NAVY = 0.05, 0.09, 0.24`), the whole site |
| ground-2 | `#111d3f` | panels, the call sheet card |
| ink | `#f3f4f1` | livery white |
| muted | `#a3abc2` | secondary text, 7.9:1 on ground |
| line | `rgb(243 244 241 / 0.14)` | hairlines, the road at rest |
| accent | `#ff6b14` | livery orange (`ORANGE = 1.0, 0.42, 0.04`): the car, the lit 500 m, hairpins, primary CTA, focus |
| grade scale | `#ff6b14` → `#ffb067` → `#f3f4f1` at falling opacity | corner grade 1 to 6: the tighter, the hotter |
| curtain | `#ff6b14` | the page transition is the orange slash of the livery wiping across |

Orange means one thing: danger ahead or "do this" (the car, tight corners, the primary CTA).
Navy + orange is not on the AI-default list; it is the car the player drives.

Why the livery and not the main menu: the game's menu is near-black with gold contour lines
and Barlow Condensed (see the "main-menu" capture on the site). That reads very close to
Shroud's site (near-black, one warm accent, Barlow Condensed), and the brief asks Pacenote to
look like itself. The livery is equally the game's own, belongs only to this game, and its
navy ground makes the orange car on the map the brightest thing on screen.

**Type:** Archivo (variable, wdth 62 to 125, wght 400 to 900, OFL, Google Fonts) for display,
calls and numbers; Barlow 400/600 (the game's own UI typeface, from the engine) for body text.
**Grade = width:** a call's type width follows its grade: hairpin is set at wdth 62 (tight,
condensed), "six" at 125 (wide open, flat out). Display headings use wdth 112 to 125 at 800 to
900, tracking −0.02 to −0.04em, leading 0.86 (livery and door-number lettering). Numbers are
tabular.

**Scale:** `--t-mega clamp(4rem, 0.5rem + 15vw, 17rem)` (PACENOTE), xl / l / m, body 17 px.

**Grid:** 12 columns, gutter = outer margin (clamp 16 to 40 px). Text starts on col 1 or
col 7 (the two seats). Rhythm: hero full height, roles airy, recce long and pinned, stages
dense index, damage asymmetric, numbers tight, gallery wall, FAQ narrow, finale full.

**Shape:** radius 2 px (the game's UI panels). The car is a chevron. Roads are round-capped
strokes.

**Motion language:** `expo.out` entrances, `expo.inOut` transitions, `power3.out` hovers.
- Primary hover (CTAs, nav, stage rows): **the slash.** An orange parallelogram (the livery's
  slash, skewed −18°) sweeps in from the left behind the label; the label shifts 2 px right.
  Current nav keeps a short slash under it (touch sees state).
- Stage rows: the slash plus the row's real stage outline drawing itself along its length.
- Inline links: underline at rest that thickens into orange.
- Page transition: the orange slash crosses the screen left to right, covering it, with the
  destination named in wide type ("Stage 9 · Frostline"), then exits to the right.
- Loader (first home visit, desktop, ≤ 2 s): the start countdown, 5 to 1 driven by real
  loading progress, then "Go" and the hero.
- Idle: the hero car drives Pinewater on the GSAP ticker at a stage pace, calls ticking; paused
  off screen; under reduced motion the car stands at a fixed point with the calls shown.

**Composition plan:**

```
HOME
1 Hero (two seats)     [nav: PACENOTE            The roles  Stages  Numbers  [Wishlist]]
                       [ DRIVER SEES: cockpit capture  | GUIDE SEES: live map + call  ]
                       [ (cols 1-6)                    | (cols 7-12)                  ]
                       [ PACENOTE mega, full width                                    ]
                       [ one line: two people, one car | Wishlist (primary) · Demo     ]
2 Roles (asym trio)    [ Driver: big capture cols 1-7 | text cols 9-12                ]
                       [ Guide: text cols 1-4 | guide-map capture cols 6-12           ]
                       [ Solo: co-driver lines set as a transcript, wide type         ]
3 Read the road        [ pinned: map cols 1-7 | call sheet cols 8-12, grade legend    ]
  (signature)          [ elevation strip under the map, progress in metres            ]
4 Stages (index)       [ 20 rows: no. | name | outline | km | surface | weather → page ]
5 What breaks (asym)   [ chase capture full bleed | four things, speed-lost bars | lines ]
6 Numbers (data)       [ sample badge | 4 totals | co-driver on/off | worst corner map ]
7 Gallery wall         [ biome captures, mixed sizes, captions = stage + conditions  ]
8 FAQ (narrow)         [ details/summary list, col 4-10                               ]
9 Finale               [ "Find a guide." mega | Wishlist · Demo | press              ]

STAGE (/stages/<slug>/)  title + meta row (km, surface, weather, time, corners, hairpins),
                         big map with every corner numbered, elevation profile, full call
                         sheet (the whole stage read as the co-driver would), stats row from
                         stats.json, previous / next stage.
STAGES (/stages/)        all 20 as a contact sheet of outlines, grouped by biome.
NUMBERS (/stats/)        "The stages, by the numbers": totals, finish/retire per stage
                         (bars), deadliest corners each drawn on its real stage, co-driver
                         on vs off. Sample banner while sample = true.
PRIVACY (/privacy/)      exactly what telemetry sends, how to turn it off.
404                      "We're off the road. Get back on, or press R." (the game's line);
                         R goes home.
```

**Explainer figure:** none. The stage map and call sheet are the explainer, drawn from the
real data.

**Genericness test** (what a generic model would do, and what changed):
- Hero with a car render over a gradient and "Experience the thrill": replaced by the two
  seats, a real cockpit capture and a live map from the game's geometry.
- Racing red + black + italic slanted type: replaced by the car's own livery and a width
  axis that carries meaning (grade).
- "Features" as three cards: the roles are three different compositions (capture, map,
  transcript).
- Fake "players say" quotes and "10,000 players": none; the numbers section is telemetry,
  labelled sample until real.

**Honesty:** the browser call sheet is a port of the game's `calls.rs` on the game's own
corner data; the page says the voice is the browser's, not the game's.

## QA decisions

- **Two stages, one module.** The hero drives Pinewater (stage 1, the one the demo starts on)
  on its own clock; the pinned section drives Kettle Ridge (stage 2) because it has every grade
  from hairpin to six and a six-note chain. Same driver module, same visuals.
- **Lit 500 m in road metres.** The map's road strokes are non-scaling (screen pixels), but the
  lit stretch and the driven road are measured along the road (`pathLength` = stage length), so
  "500 m" is true on every stage and screen. The lit band is a translucent wide stroke *under*
  the grade colours, so corner grades stay readable inside it.
- **Long chains.** Calls are one note per line with a small "into"; chains of 3+ notes shrink
  (`--k`) instead of overflowing. Kettle Ridge has a six-note chain at 2.05 km.
- **Reduced motion** keeps everything visible, unpins the section and shows a range slider that
  drives the same stage (the lead copy switches from "Scroll" to "Drag the slider"). The hero
  car stands at 60 m with its call shown.
- **Loader** only on the first home visit per session, desktop only, ≤ 2 s: 5, 4, 3, 2, 1
  follows real loading, then GO and an orange wipe. Phones go straight to the hero (LCP first).
- **Hero driver capture is not revealed by JS**, so it can be the phone LCP without waiting
  for the engine; the guide seat clips in beside it ("the guide joins").
- **Header CTA** is "Wishlist on Steam" everywhere; under 480 px the label shortens to
  "Wishlist" (accessible name unchanged).
- **Sample stats** are labelled in every block that shows them (home, /stats, each stage page)
  and the copy around them avoids claims ("hardest to finish (sample)"). The co-driver verdict
  sentence has three outcomes (helps, hurts, no clear difference) and disappears without data.
- **Speech** is opt-in per click, never autoplay, cancelled on navigation; prefers an
  on-device English voice. The privacy page says some browsers' voices are remote.
- **Idle:** the hero car keeps driving (paused off screen). The pinned section is still when the
  visitor is still: scroll is its throttle by design.
- **Footer wordmark** is decorative (`aria-hidden`) but still kept at 3.3:1.
- **Header** turns solid navy with a hairline once scrolled past 80 px (the critic saw body
  text running under the transparent bar), and still steps aside while scrolling down.
- **The main-menu capture stays in the gallery** although its gold-on-black look and condensed
  logotype differ from the site: it is the real game and shows "Host as driver / Join as
  guide". If the game's menu moves to the livery palette, recapture it.

## QA record

- `audit.mjs`: 24 pages, 0 FAIL, 0 WARN. `shoot.mjs --crawl --nav` (pass 3): 0 console errors,
  no overflow, 0 serious axe violations, every focus visible, reduced-motion hidden text 0,
  throttled phone LCP 1.2 to 2.3 s, CLS ≤ 0.028, scroll 58 to 60 fps.
- Separate critic (fresh subagent, evidence only) on pass 2: **7.1 overall** (Design 7,
  Usability 7, Creativity 7.5, Content 7; SEO 8, Animations 7, Accessibility 7, WPO 7,
  Responsive 6, Markup 8). Its read of the idea matched the concept card. Fixed after it:
  card contrast, stage-title break on phones, CLS, small targets, header overlap, 404 phone
  layout, utility-link hovers, dead space. Not re-scored after those fixes.
- What would raise it most: real stats instead of the sample, real Steam links, and a short
  gameplay clip (driver + guide audio) for the hero.
