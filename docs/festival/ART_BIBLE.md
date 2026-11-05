# Pacenote — Festival art bible (lighting, sky, car FX, roadside, towns, UI)

Art direction + technical-art spec for the open-world "festival" pass. Extends `docs/ART.md`
(which stays the source of truth for what exists); this doc says what to build next and to
what standard. Autonomous mode: every decision carries a one-line rationale. Budgets are GPU
ms at 1920×1080, **High**, on a base Apple M-series laptop (M2/M3 10-core GPU class; the
`ART.md` numbers were taken on an M5 Pro, so assume roughly 2× those there).

---

## 1. Visual identity (kept from ART.md)

- **Grounded, real-world rally**: scanned-material terrain, card trees from real scans, an
  original car ("Talus") with honest wear; no stylised outlines, no neon. *Why: the game is
  read through the windscreen; the road must look like a real road.*
- **The road is the hero**: compacted wheel lines, loose stones at the edges, surface patches,
  verges. Everything else is quieter in value and saturation than the road edge.
- **Air you can see**: depth comes from aerial perspective and sun direction, not from fog walls.
- **Late-European countryside**: pine/spruce forest, birch, moor, scrub; terracotta and slate
  roofs, pale rendered walls; stone parapets; red/white and black/yellow road furniture.
- **Neutral, slightly warm grade**: Tony McMapface tonemap (AgX greyed the sky, see
  `look.rs`), contrast ≈ 1.08, temperature +0.02 in sun. No teal/orange push.
- **Light tells the time**: low sun = long shadows and warm rim; overcast = soft and desaturated
  (0.86); night = only what the car and the town light.
- **Mechanical honesty in FX**: dust, chips, mud, flames, smoke come from simulated causes
  (surface, slip, rpm, shift), never on a timer.
- **Original everywhere**: no real brands, liveries, sign typefaces or game UI imitated.

---

## 2. Sky & lighting

### 2.1 Why the open-world sky looks unrealistic (diagnosis)

The open world doesn't choose a look; it borrows a stage definition, and `src/open/mod.rs:48`
pins it to `TimeOfDay::Afternoon` + `Weather::Clear`. `art::spawn_stage` → `look::stage_look`
then produces:

1. **No clouds at all.** The `Atmosphere` sky with `Weather::Clear` is a perfect,
   empty Rayleigh gradient. The Unreal version had SkyAtmosphere **plus volumetric clouds**:
   clouds supply most of a real sky's scale, parallax and contrast. This is the main gap.
2. **The fog contradicts the atmosphere.** `FogMode::Atmospheric` with a hard-coded neutral grey
   (`0.66, 0.71, 0.78`), `visibility` 1400 m, is drawn **on top of** Bevy's own aerial
   perspective. The far terrain goes flat grey-lavender while the sky just above it is
   saturated blue, which puts a visible seam at the horizon. The fog colour also ignores the
   sun's position and the time of day.
3. **Double fill light flattens the shading.** `GlobalAmbientLight` at 2400 cd/m² (a "forest
   shade" fix) is added to an env-map light at intensity 1.0. Shadows come out grey-blue and
   shallow, form shading on terrain and buildings disappears, and the sun reads as weak. In
   Unreal the sky light is occluded (Lumen/DFAO); we only have SSAO, so the flat term has to
   drop.
4. **Fixed exposure.** EV100 is fixed at 13.8 (14.2 − 0.4) and `exposure.auto = false`. Forest
   shade comes out dark and the open moor flat. Unreal's eye adaptation hides this.
5. **The time of day is frozen at 30° sun / 235° az.** One angle, so the light never has the
   low, raking quality that makes the Unreal shots look good.
6. **The world edge shows.** Trees stop at the impostor reach (1.2 km) and the fog hides the
   edge with grey. Bare far hills plus grey haze read as smog.
7. **Reflections disagree with the sky.** Probes capture the *gradient* approximation of the
   sky (`apply.rs` ~339), so car paint and glass reflect a different sky.

### 2.2 Target stack (engine `look` + one new cloud pass)

| Item | Spec | Rationale |
|---|---|---|
| Sky | Keep `SkyMode::Atmosphere` (Bevy PBR atmosphere, `ScatteringMedium::earth`). LUT samples from tier `sky_samples` (8/12/16/24). Cost ≈ 0.25–0.4 ms on High (sky-view + aerial-view LUTs + raymarch of the sky pixels). | Already integrated; it follows the sun for free and gives correct twilight. |
| Aerial perspective | Use the atmosphere's aerial-view LUT as the **only** distance haze on clear days: `FogMode::Off` (or `Atmospheric` with `extinction`/`color` sampled from the atmosphere at the horizon each keyframe) and `visibility` ≥ 4 km. Weather fog (rain/fog) layers on top as `Exponential`. | Removes the grey seam; haze colour and sky colour come from one model. |
| Clouds | **New: a 2.5D cloud-layer shader.** One full-screen sky pass at ½ resolution with bilinear upsample, drawn after the atmosphere and before the opaque fog. It ray-intersects a spherical shell at 1.8–3.2 km and samples 2 tiling noise textures (a 128² base + a 64³ detail volume, BC4) along **8 steps** through the shell. Light uses Beer–powder with **2 sun-ward taps**; ambient is the atmosphere's sky colour at the zenith/horizon. Wind scroll 6–12 m/s. Parameters: `coverage` 0–1, `density`, `base_altitude`, `thickness`, `wind`. Cost ≈ 0.5–0.8 ms on High, 0.3 ms on Medium (4 steps, ¼ res), Low = a static cloud cubemap (0.05 ms). | Real volumetric clouds (64+ steps) cost 2–4 ms on base M-series. Billboards break with parallax at 150 km/h. A static cubemap can't follow a moving sun. The shell shader looks volumetric from the ground, which is the only place we look from. |
| Cloud shadows | Project the same base noise from the sun direction onto the ground. One texture fetch in `terrain.wesl`, the road and the tree shaders multiplies the sun term (min 0.55). ≈ 0.05 ms. | Drifting cloud shadows on hills give most of the "realistic" feel at nearly no cost. |
| Clouds in reflections/env | Render the cloud layer into the atmosphere env cubemap when it is refreshed (see cadence below) at 64² per face. | Car paint reflects the same sky. |
| Sun disk | `disk_size_deg` 0.53, `disk_intensity` 1.0 by day; fade to 0 between el +1° and −1°. Bloom intensity 0.08–0.12 so the disk and the specular glints bloom softly. | Physically sized; the bloom sells brightness. |
| Ambient | `GlobalAmbientLight` down to **350–600 cd/m²** by day (table below); `env_intensity` 1.0; SSAO on Medium+. Forest shade is fixed by auto exposure, not by flat fill. | Brings back form and deep shadows. |
| Exposure | **Keyframed base EV100 + Bevy `AutoExposure`** (tier `auto_exposure`) in a narrow window `auto_min −1.5 / auto_max +1.0`, `speed_brighten` 1.5 EV/s, `speed_darken` 2.5 EV/s, compensation −0.5. Low tier: base curve only. Cost ≈ 0.1 ms (histogram compute). | The curve keeps day/night intent; the window adapts to shade and tunnels without pumping. |
| Shadows | 4 cascades on High/Ultra (3 Medium, 2 Low), `shadow_distance` 160 m, `first_cascade` 10 m, map 2048 (1024 Low). Cascade split lambda 0.75. Below sun el 8° shorten to 110 m (long shadows waste resolution). Under the moon: 2 cascades, 60 m. | Matches the current 140 m but moves resolution near the car. |
| Volumetric shafts | High/Ultra only, at dawn/dusk keyframes and in forest biomes: `VolumetricLook` box 80×24×80 following the camera, density 0.02. 0.8–1.2 ms, so only at el < 20°. | Expensive; only pays off with low sun through trunks. |

**Env map refresh cadence.** Today `sync_env_worker` marks the atmosphere env dirty on *every*
look change, and a moving clock changes the look every frame (filter ≈ 0.4–0.6 ms at 256²).
Rule: refilter only when the sun has moved **≥ 0.75°** or **20 s** have passed (whichever comes
first), or on a weather change. Time-slice it: render the cube one frame, filter the next.
Crossfade the old and new diffuse/specular maps over 2 s. At 5× speed the sun moves 1.25°/real
min, so that is about one refilter every 36 s, ≈ 0.02 ms amortised. Probes (`probe.rs`) refresh
on the same trigger, one probe per frame.

**Sun movement and shadow swim.** Update the sun direction every frame, but snap each cascade's
projection to its shadow texel size (stable cascades). If Bevy doesn't snap, quantise the
direction to 0.02° steps instead (about one per second at 5×). Moving cascades without
snapping shimmer at the edges.

### 2.3 Dynamic time of day

- **Clock**: game hours, 24 h period, **5× real time**: 1 game day = 4.8 real hours,
  `LookTimeline::speed = 24 / (4.8 × 3600) = 0.0013889` h/s. Driven by `Time<Virtual>`, so pause
  freezes it. Online: the host's clock is authoritative and sent with the session state.
- **Sun path**: latitude 46° N, early summer, solar noon 13:00 game time (summer time). Sunrise
  ≈ 05:40, sunset ≈ 20:20. Azimuth/elevation come from a small solar formula (not keyframed)
  so shadows move smoothly. Keyframes set everything else.
- **Moon**: a second `DirectionalLight` that is on only while the sun is below −6°. It sits
  opposite the sun's azimuth, elevation 35°, 0.6 lux, colour `0.62 0.72 1.0`, 2 cascades.
  The 0.6 lux is ≈ 2× a real full moon: night has to stay drivable.
- **Night sky**: below sun el −8° switch `SkyMode` to `Gradient` with `stars` 1.0 (the engine
  night preset). `SkyMode` blends as a step, so make the switch at −8° where the atmosphere is
  already near black, and dip the exposure by 0.3 EV for 1 s to hide it. Clouds stay on at
  night, lit by the moon.
- **Keyframes** (sun values are the light's *colour/lux*; el is from the solar formula, listed
  for reference; blend between keys with `Look::blend`: lux, sky brightness, ambient and fog
  visibility in log space, which the engine already does):

| time | sun el | sun colour (sRGB) | sun lux | base EV100 | fog / haze | ambient cd/m² (colour) | env int. | clouds cov. |
|---|---|---|---|---|---|---|---|---|
| 05:00 | −6° | — (sun off; atmosphere glow only) | 0 | 6.0 | Exp, 900 m, `0.30 0.34 0.45` | 40 `0.55 0.62 0.9` | 1.0 | 0.35 |
| 06:30 | +6° | `1.00 0.62 0.38` | 9 000 | 10.6 | aerial + thin Exp 2.5 km `0.85 0.68 0.55` (valley mist) | 220 `0.70 0.74 0.95` | 1.0 | 0.35 |
| 09:00 | 33° | `1.00 0.90 0.80` | 72 000 | 13.6 | aerial only (vis 5 km) | 450 `0.75 0.82 1.0` | 1.0 | 0.30 |
| 12:00 | 60° | `1.00 0.97 0.93` | 105 000 | 14.6 | aerial only (vis 6 km) | 600 `0.78 0.84 1.0` | 1.0 | 0.40 |
| 16:00 | 45° | `1.00 0.94 0.86` | 92 000 | 14.3 | aerial only (vis 5 km) | 550 `0.78 0.83 1.0` | 1.0 | 0.45 |
| 19:00 | 12° | `1.00 0.74 0.50` | 32 000 | 12.4 | aerial + Exp 3 km `0.95 0.75 0.6` | 300 `0.68 0.72 0.95` | 1.0 | 0.40 |
| 20:30 | −2° | `1.00 0.55 0.35` (disk off) | 1 500 | 8.8 | Exp 1.6 km `0.45 0.42 0.55` | 90 `0.55 0.6 0.95` | 1.0 | 0.35 |
| 23:00 | −25° | moon `0.62 0.72 1.0` | 0.6 (moon) | 1.2 | Exp 700 m `0.06 0.07 0.10` | 6 `0.45 0.55 1.0` | 0.8 | 0.30 |

  Weather (overcast/rain/fog) is applied **on top** of the sampled key, as `stage_look` does
  today: sun ×0.1–0.2, coverage 0.85–1.0 (the cloud layer replaces the flat gradient sky),
  saturation 0.86, base EV −2.2. Classic rally stages (stage list) keep their authored
  `TimeOfDay`: the clock is frozen at that key.
- **Night requirements** (these are features we lack, not polish):
  - **Car headlights** (none today): 2 low-beam `SpotLight`s, 30° outer cone, 12 000 lm each,
    range 80 m, shadowed only for the player car on High+ (1 shadowed spot). The 4-lamp
    `LightPod` is 1 merged unshadowed spot, 40 000 lm, 15° cone, range 160 m, toggled with the
    lights key and on automatically in events below sun el 2°. Emissive lens faces as well.
  - **Traffic**: emissive head/tail lamps (they exist) plus one unshadowed spot for the 4
    nearest cars only.
  - **Street lamps** in the town centre and on the main street: warm 3000 K sodium-free LED
    look (`1.0 0.85 0.65`), lit when sun el < 4°, 6 m poles every 30 m. Emissive heads
    everywhere; real `PointLight`s (1 800 lm, range 18 m, no shadows) for the **24 nearest**
    only. Windows: 30 % of house windows get an emissive tint after dusk (vertex-colour bit in
    the facade atlas).
- **Settings** (Video › *Time of day*; `prefs.rs`):
  - `Start time`: **Dawn 06:00 · Morning 09:00 · Noon 12:30 · Afternoon 16:00 · Sunset 19:30 ·
    Night 23:00 · Real clock** (the local wall clock). Default: Morning, because it looks best
    on first boot.
  - `Time passes`: On/Off (On = 5×; Off freezes at the start time). Default On.
  - Dev command `look.time <h>` / `look.speed <x>` for screenshots.

---

## 3. Car realism FX

### 3.1 Exhaust flames, pops and bangs
- **Single source of truth**: the pop decisions currently live in the audio thread
  (`audio/powertrain.rs`: lift-off crackle window, `shift()` upshift bang, `pop()`). Move the
  decision (RNG + rules) to the main-thread car-audio parameter step and send each pop as a
  `Backfire { strength, kind }` message to **both** audio and VFX. *Why: a flame without its
  bang, or a bang without its flame, reads as a bug.*
- **Triggers** (match the audio rules): lift-off from ≥ 50 % throttle at > 3 200 rpm opens a
  crackle window (0.4–0.8 s, 3–6 pops, strength 0.3–0.7). Upshift under power > 4 200 rpm gives
  one bang (75 %, strength 1.0). Downshift > 2 800 rpm gives a crackle (0.7 window). Limiter
  bounces give small pops (0.3). Damage/misfire gives an irregular pop with **black smoke**,
  no flame.
- **Look** (exhaust tip: right rear, under the bumper, model (0.46, −1.99), 0.27 m up, axis
  rearward):
  - Flame = 2 nested additive cones, mesh (16 tris each), emissive and unlit, with a scrolling
    noise mask.
    - Core: blue-white `0.55 0.7 1.0`, length 0.12–0.18 m.
    - Outer: orange `1.0 0.45 0.12` → tip `1.0 0.2 0.05`, length 0.25 m (pop) / 0.45 m (bang),
      radius 0.06 → 0.13 m.
  - Duration 40–70 ms (pop) / 90–120 ms (bang); scale in over 15 ms, out over the rest; random
    roll and ±15 % length per shot.
  - **Exposure-relative emissive**: scene white ≈ 1.2·2^EV100 cd/m². Core = 8×, outer = 3× that,
    so the flame blooms equally at noon and at night.
  - Light flash: one `PointLight` per car at the tip, `1.0 0.55 0.25`, 4 000 lm (pop) / 9 000
    (bang), range 4 m, no shadows, same envelope. Player car + 2 nearest cars only.
  - Sparks on bangs: 6–10 additive streaks (`engine::vfx`), 0.15 s, 3–6 m/s. After the bang a
    grey-blue smoke puff (alpha 0.25, 0.6 s, grows 0.1 → 0.5 m).
- **Views**: full effect in chase/bonnet/replay. In the cockpit only the light flash (it lights
  the road behind in the mirrors and the cabin rear at night).

### 3.2 Chase camera spring arm (replaces the exponential lerps in `drive.rs` ~576–612)
- **Model**: a damped spring per axis **in the car's yaw frame** (closed form, frame-rate
  independent, stepped on the render dt). Rest pose stays: boom 6.8 m back, 2.3 m up, aim
  4.0 m ahead and 0.9 m up, and `chase_dir` keeps blending toward velocity (shows the slide).

| axis | ω (rad/s) | ζ | limits | intent |
|---|---|---|---|---|
| longitudinal (boom length) | 5.0 | 0.80 | stretch ≤ +1.6 m, squash ≤ −1.0 m | acceleration pulls away, braking closes in |
| lateral | 4.0 | 0.90 | ±1.2 m | the slide shows, then catches up |
| vertical | 7.0 | 1.00 | −0.4 / +0.6 m | landings dip, no bobbing |
| heading (yaw of the arm) | 3.5 | 1.00 | ≤ 25° behind the car | hairpins show the corner |
| aim point | 9.0 | 1.00 | — | the car stays centred |

  - **Speed**: boom +0.8 m and height −0.25 m from 60 → 180 km/h (smoothstep).
  - **Collision probe**: sphere radius 0.35 m swept pivot→eye against terrain, traffic, walls,
    buildings, guard rails and big rocks (extends `chase_clearance`). Pull in instantly, ease out
    at 2.5/s. Keep the existing lift-before-pull (0 / 1.2 / 2.4 m) and the reset on
    view switch or teleport.
  - Never below the ground + 0.8 m (as today).

### 3.3 Motion blur (decision: speed-driven, on in every view, tier High+)
- Bevy `MotionBlur` (tier `camera_fx`, High/Ultra). `shutter_angle` = 0 below 60 km/h, rising
  smoothly to **0.35** at 160 km/h and **0.45** at 220 km/h. Samples 4 (High) / 8 (Ultra).
- **Cockpit: on at 0.6×.** The cabin is still relative to the eye, so only the world streaks,
  which is what speed looks like from a seat.
- The blur turns off for 3 frames after a camera cut or teleport (no full-screen smear).
- Setting: Video › *Motion blur* Off / Low (0.6×) / High (1×). Default High on High+. *Reduce
  motion* forces Off.
- Cost ≈ 0.5 ms (4 samples) / 0.9 ms (8). It needs motion vectors, which TAA already provides
  on High.

### 3.4 Shake, FOV, dust
- **Speed FOV exists** (`speed_fov`, +7° by 180 km/h). Keep it, cap at +5° in the cockpit (the
  frame of the cabin makes it look larger).
- **Chase shake** (new; cockpit/bonnet shake already exists): above 110 km/h, low-frequency
  noise at 6–9 Hz, amplitude 0.012 m × surface roughness (`surface_buzz`). **Landing kick**: an
  impulse into the vertical spring of 0.06 m per m/s of vertical landing speed above 2 m/s
  (max 0.35 m). **Impacts**: 0.15 s rotational jolt ≤ 1.5°. All scaled by *Camera shake*;
  *Reduce motion* turns them off.
- **Wheel spray/dust exists** (`spray.rs`). Add a **lingering dust cloud** on dry gravel: large
  soft sprites that hang 4–8 s and drift with the wind, lit by the sun (the following car
  drives into it). High+ only, ≤ 60 sprites.

---

## 4. Roadside furniture and road conditions

**Style**: plausible generic continental-European furniture, original pictograms and lettering
(no national typeface), worn but maintained. Existing kinds in `art/roadside.rs`: Parapet,
Culvert, SnowPole, Chevron (red `0.8 0.08 0.06` / white), TimberPile. New kinds below, each
one shared mesh + material (or one atlas), instanced.

| Kind | Look | Placement rule |
|---|---|---|
| **Guard rail (W-beam)** | Galvanised steel `0.62 0.63 0.62`, metal 0.9, rough 0.45, dull streaks; 0.31 m beam with the centre 0.75 m up, posts every 2 m (C-section, dark grey). Flared end terminals buried into the ground. A white reflector button on every 4th post. Dents where hit (vertex offset + darker). | **Outside of a curve** (corner grade ≤ 4) on paved/mountain roads where the ground **0.5–4 m beyond the edge drops > 2.5 m** or slopes > 35°; also every station where the drop is > 5 m within 6 m, curve or not. Start 15 m before the curve, end 10 m after. Merge runs closer than 20 m. Never across a junction mouth. **Solid** (feeds `vehicle::Wall`). |
| **Timber rail** | Peeled log rail on round posts, `0.48 0.36 0.24`, rough 0.9. | Same drop rule on **forest/gravel** roads below the snow line. Solid. |
| **Low stone wall** | Dry stone 0.6–0.9 m, grey-brown, same stone as the parapet. | Gravel roads through farmland and moor; the outside of fast corners above meadows. Solid. |
| **Concrete barrier** | 0.8 m safety-shape profile, `0.72 0.71 0.68` with a dirty base, 4 m segments, a red/white tape band on the end blocks. | Town bypass, bridges with no parapet, roadworks zones. Solid. |
| **Reflector post** | 1.0 m white post, black band at the top with an amber (right) / white (left) reflector, emissive only in headlight range at night. | Every 50 m on both sides of paved country roads; 25 m through curves; not in town. Breakable (prop). |
| **Kilometre post** | White 0.6 m stone, red cap, black number (km from the town centre). | Every 1 km on paved roads. Breakable. |
| **Chevron board** (exists) | Keep red/white. Add a **sequence of 3–5 boards** (1 per 12 m around the outside) on hairpins after fast runs. | Extends the current rule. |
| **Warning signs** | Triangle: white face, red 6 cm border, black pictogram, on a 2.2 m galvanised pole. Set: curve L/R, double curve, crest, slippery, falling rocks, deer, loose gravel, roadworks, steep descent (%). | 80–120 m before the hazard, on the driver's side; at most 1 per 150 m; curve signs only for grade ≤ 3 corners not already chevroned. |
| **Speed / regulatory** | Round with a red ring and black numbers (30/50/70/90); end-of-limit with a grey bar. | 50 at town entry, 30 in the centre, 70/90 on country roads after the town exit. |
| **Town boards** | Yellow rectangle, black border, town name in our own condensed grotesque; a red diagonal on the exit board. | At the town limits, both directions. |
| **Direction signs** | White panel, black text, arrow end; 1–3 destinations (original place names). | At junctions of the main roads. |

**Sign texture**: one 2048² sign atlas (BC7) with plain shapes, flat face colours, retro-reflective
spec (roughness 0.25, a little emissive in headlight range at night), and a dirt/rust mask for
the back faces and edges. Backs are grey galvanised. 10 % of signs lean 2–6°, 5 % are faded.

**Road conditions** (visual + grip: each patch type gets a `Surface` / grip multiplier owned by
the gameplay team; art provides the decals and the masks):

| Condition | Look | Implementation | Grip hint |
|---|---|---|---|
| Potholes | 0.4–1.2 m ragged holes, 4–8 cm deep: dark wet-ish interior, broken edge chips | Clustered decal (`look_clustered_decals`) with a normal + depth-tinted albedo; the physics gets a height dip | bump + 0.85 |
| Patched tarmac | Darker, smoother rectangles with sealed seams | Forward decal, 1–6 m | 1.0 (just reads as old road) |
| Cracks | Crazed/longitudinal cracks, tar-sealed lines | Decal atlas tiling along the edges | 1.0 |
| Puddles / wet patches | Mirror-smooth (rough 0.05), darker albedo, sky reflection via the env map (SSR on Ultra); rain rings in rain | Decal + material wetness mask in the low spots; dries over game hours when the clock runs | 0.6–0.75 + splash spray |
| Gravel washout | Loose gravel fan across tarmac below steep banks, at the ends of culverts | Decal + loose stones from the dressing set | 0.7 |
| Mud tongue | Brown wet streak from a field entrance | Decal | 0.6 |
| Edge break-up | Crumbling tarmac edge, grass growing in | Verge strip variant | — |

Density: paved country road ≈ 1 condition per 120 m, the mountain pass 1 per 80 m, town 1 per
200 m, never two hazards in the same braking zone.

---

## 5. Town and building look (dense centre, sparse outskirts, rare mountain houses)

- **Zoning by distance from the centre**:
  - Centre (< 250 m): 3–5 storey rows with shopfronts, shared walls, flat or mansard roofs, no
    gaps.
  - Inner ring (250–600 m): 2–3 storey semi-detached houses with 3–6 m front gardens.
  - Outskirts: detached 1–2 storey houses on 20–35 m lots, plus farm clusters.
  - Mountains: 1 house per 1–2 km of road, a chalet or stone hut with a woodpile.
- **Roof rules**: terracotta 55 %, slate 25 %, brown tile 15 %, light grey (flat roofs only)
  in the centre, barn red on barns only. One street is ≥ 70 % one roof colour (towns grow in
  phases). Pitch 30–45° for houses, 20° for barns. Mountain houses: slate or dark wood
  shingle, broad eaves.
- **Walls**: use `WALLS` from `buildings.rs` but by zone. Centre: cream, ochre, white, pale
  blue, pink (painted render). Outskirts: sand, grey, sage, brick. Never two adjacent row
  houses in the same colour; saturation ≤ the road-sign red.
- **Sizes**: house footprint 8–12 × 9–14 m with ±15 % jitter. Ridges run along the street in
  the centre and turn freely outside. 1 in 6 houses has an annex/garage box.
- **Gardens and fences**: low hedge 40 %, picket fence (white/brown) 25 %, low rendered wall
  20 %, none 15 %. Driveway gravel or paving. Gardens have 1–3 small trees (birch/fruit) and a
  shed in 20 %.
- **Detail that sells "lived-in"**: chimneys on 70 % of pitched roofs, satellite dish 10 %,
  balconies on centre buildings, parked traffic cars along centre kerbs, lit windows at night
  (§2.3), street lamps, benches, a church/tower landmark visible from the approach roads.
- **Budget**: one merged mesh + one atlas material per block (as now). LOD by block beyond
  300 m: drop chimneys, fences and gardens. Impostor-free (buildings are boxes; they stay
  cheap).

---

## 6. UI visual direction

- **Palette** = the existing `meta::screens::theme()` ("night-forest greens, map-paper text,
  rally-plate yellow"): bg `#0B100E`, panels at 92 % alpha, text `#F2EDE0` (map paper),
  accent `#FFC41F` (rally plate yellow, used *only* for focus/primary action and records),
  good `#6BE08C`, bad `#FF6652`, info `#85CCFF`. No gradients except a 6 % vignette on screens.
- **Typography**: one in-house/OFL **condensed grotesque** for headings, all caps, +4 %
  tracking, and a humanist sans for body. **Tabular figures** everywhere there are times and
  speeds. Pacenote calls (HUD) set in the condensed face at weight 700 with a 1 px dark
  outline for readability over snow/sky.
- **Main menu**: a live 3D backdrop: the car on a gravel lay-by at the current time of day,
  slow 20 s orbit, menu on a left-hand panel. Items in a column with a yellow bar marking
  focus. The look is a **road-book**: thin rules, stage numbers, kilometre ticks as
  decoration.
- **Loading screens**: a full-bleed in-engine still of the destination, the stage's road-book
  strip (corner symbols along a line) as the progress bar, and one driving tip. No spinners.
- **HUD skill popups** (drift, jump, near-miss, clean section): bottom-centre-right stack. A
  chip with a condensed caps label plus points in tabular figures. Enters with a 120 ms slide
  up of 12 px + fade, holds 1.6 s, exits with a 180 ms fade. Combos grow the number with a
  1.06 scale pulse (no bounce). Colour: paper text on a panel; yellow only for a new best;
  max 3 visible.
- **Motion**: ease-out-cubic 120–220 ms; nothing loops in the HUD while driving. *Reduce
  motion* = fades only.
- **Originality guard**: no hexagons, no diagonal neon slashes, no festival-wristband tropes.
  The road-book and map-paper motif is ours.

---

## 7. Performance budgets and priority

Frame target on a base M-series laptop, High, 1080p: **16.6 ms GPU** (60 fps), with today's
scene ≈ 11–13 ms there. The festival pass may add **≤ 3.5 ms**:

| Item | Budget (GPU ms, High) | Low tier |
|---|---|---|
| Atmosphere sky + aerial perspective (exists; LUT sizes per tier) | 0.35 | 0.15 (8 samples) |
| Cloud layer, ½ res, 8 steps | 0.7 | static cubemap 0.05 |
| Cloud shadows on the ground | 0.05 | 0.05 |
| Env-map refresh (amortised, time-sliced) | 0.05 | 0.02 (64²) |
| Auto exposure | 0.1 | off |
| Motion blur (4 samples) | 0.5 | off |
| Volumetric shafts (only at sun el < 20°) | 1.0 (when active; replaces the cloud budget headroom) | off |
| Headlights (2 spots, 1 shadowed) + pod | 0.4 at night | unshadowed 0.15 |
| Street lamps (24 point lights, clustered) | 0.3 at night | 8 lights 0.1 |
| Exhaust flames + flash light + sparks | 0.05 | 0.03 |
| Lingering dust | 0.25 | off |
| Road-condition decals (clustered, ≤ 64 in view) | 0.2 | forward decals ≤ 24, 0.1 |
| Roadside furniture (instanced, ≤ 3 k instances in view) | 0.3 | 0.2 (shorter range) |

Memory: cloud noise 0.5 MB, sign atlas 4 MB (BC7), decal atlas 8 MB, furniture meshes < 1 MB.

**Priority** (biggest realism per ms first):
1. **Fix the existing look** (no new tech): drop the flat ambient to the table values, turn
   off the grey clear-day fog in favour of aerial perspective, add auto exposure with a narrow
   window, lower the default sun. ≈ 0.1 ms. *Probably half of the "Unreal" gap.*
2. **Cloud layer + cloud shadows** (+ clouds in the env map).
3. **Time-of-day clock** with keyframes, settings, env refresh cadence, stable shadows.
4. **Chase spring arm + motion blur + landing kick** (cheap, big feel).
5. **Exhaust flames/bangs** (needs the shared `Backfire` message).
6. **Guard rails + signs + reflector posts** (placement rules above; rails are solid).
7. **Night kit**: headlights, moon, street lamps, lit windows (needed before *Night* is offered
   as a start time; until then the *Night* preset is hidden).
8. **Road-condition decals**, then grip hookup by the gameplay team.
9. **Town variation rules** and lingering dust.

Verification per item: `perf.stats` A/B on the same binary, plus screenshots at 06:30 / 12:00 /
19:00 / 23:00 from three fixed spots (town square, mountain hairpin, forest straight), kept in
`docs/festival/shots/` for comparison.
