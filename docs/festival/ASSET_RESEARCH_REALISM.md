# Pacenote: asset research for a realistic rally-sim look

Research only (2026-11-07). Nothing has been downloaded or integrated. Rules applied: `docs/ASSETS_BRIEF.md` §0–2. Only CC0 or CC BY (credited) assets, OFL for fonts. No CC-BY-NC, no "free for personal use", no ripped files, no real brands. Every URL below was returned by a search. **"verify"** means the licence, price, file contents or exact page was not visible in the search result and has to be checked on the page before download. Poly Haven publishes everything as CC0, so its items are marked "yes", but the page should still be checked on download.

References: three streamer clips, frames in the session scratchpad. Current game: `docs/festival/shots/look_hairpin_1200.jpg`, `after_horizon_1600.jpg`.

## 0. What the references actually show (checked against the frames)

- **v1 (cockpit, dry stage).** Confirmed. Ochre gravel road with a **brick-red soil berm** along the outside of the bend. Grey concrete safety-shape barriers. Two crowd lines in red/blue clothing, each figure casting a hard shadow. Sparse **pale-trunk trees with small round canopies** (eucalyptus-like) and low red-brown shrubs on yellow-green ground. Dark cabin with a roll-cage bar framing the top of the view. It is worth knowing that **this sim is not high-fidelity**: the crowd looks like simple low-poly figures and the trees are basic. It reads as real because the light, shadows, colour and density all agree with each other.
- **v2 (bonnet, forest).** Confirmed. Fast forest gravel road, dense green undergrowth and broadleaf trees, a strong **speed blur** and a dusty, bright haze. Almost no detail can be resolved. The realism comes from post-processing (motion blur, haze, exposure).
- **v3 (bonnet, grassland).** Mostly confirmed. Pale pink-grey gravel road, **golden dry grass** right up to the edge with lighter tyre lines, a distant line of round-canopy trees, big soft cumulus. The pink/lilac sky cast is partly the phone camera filming the monitor, so don't copy that colour.

## 1. Gap analysis (ranked by visual impact)

1. **Light and shadow contrast: rendering, not assets.** The references have hard, dark sun shadows (every spectator casts one), a warm key light and a low-value cabin. In our shots the shadows are grey-blue and shallow, the sun looks weak and the ochre terrain looks flat. This is exactly `ART_BIBLE.md` §2.1 items 2–4: flat ambient at 2400, grey fog, fixed exposure. **No asset fixes this.** Do §7 priority 1 first.
2. **Ground cover density and colour harmony: about half assets, half rendering.** v3 has continuous golden grass to the horizon with a soft edge into the road. v1 has a red berm, then yellow-green ground, then shrubs. Ours has a uniform ochre splat with visible tiling, isolated bushes and a hard line at the road edge. Assets needed: dry-grass cards and a dry-soil/berm texture. Rendering needed: grass-card density (brief §4), macro colour variation and a verge strip mesh (brief §3.1).
3. **Tree species and silhouette: assets, plus an art-bible decision.** The dry look needs pale smooth trunks with sparse, clumped canopies. We only have conifer, birch-crown and olive cards. **There is no good free eucalyptus anywhere** (§4). It has to be made in-house.
4. **The road surface: mostly shader and decals.** The references show compacted lighter wheel lines, a darker centre ridge of loose gravel and berms of thrown gravel. Ours reads as a flat strip. One or two better textures help, but the wheel-line/berm mask along the road station (brief §3.2 "wear" channel) is what sells it.
5. **Event dressing and people: assets plus in-house work.** Crowds, barriers, tape and parked cars are what make it read as a rally rather than a road. We have none in game. Concrete barriers are already in `assets_src` but not shipped. **No free realistic crowd exists.** The best route is a CC0 generator (MakeHuman) baked into impostor cards in-house.
6. **Speed: motion blur, dust, haze. Rendering and VFX.** v2 is mostly motion blur and lingering dust. This is `ART_BIBLE.md` §3.3–3.4. Flipbook textures (below) are a small input. The system work is the cost.
7. **Sky: rendering.** v3's sky is cumulus. Our HDRIs only light and reflect the scene; the visible clouds come from the cloud layer in §2.2. The new HDRIs below are for image-based lighting that matches the dry, warm stages.
8. **Cockpit: in-house.** The reference cabin is dark and low-contrast, so the view through the screen dominates. Ours has a navy dash and a pale roof liner that compete with the view. This is a material and value change on the Talus model, not a download.

**Art-bible conflict.** `ART_BIBLE.md` §1 says "late-European countryside: pine/spruce, birch, moor, scrub". The v1 look (eucalyptus, red soil) reads as Australian. **Recommended reconciliation:** frame it as an **Iberian/Mediterranean dry stage**. Eucalyptus plantations, red laterite verges and golden summer grass are all real in Portugal and Spain, so the stage stays European and the existing scrub/olive biome becomes its neighbour. The decision belongs to the creative director and the owner; the alternative is an explicitly non-European biome.

## 2. Shortlist by category (priority order within each)

### A. Road, verge and soil textures (P0)

| # | Asset | Source | URL | Author | Licence (as stated) | Passes? | Format | Size / res | Fixes | Pri |
|---|---|---|---|---|---|---|---|---|---|---|
| A1 | Rocks Ground 02 | Poly Haven | https://polyhaven.com/a/rocks_ground_02 | Poly Haven (verify author) | CC0 | yes | PBR set (jpg/exr, gltf/blend) | 2 m tile, 41 px/cm, 1–8K | Dry rocky shoulder and gravel berm. Base for the red-soil berm when recoloured | P0 |
| A2 | Mud Cracked Dry 03 | Poly Haven | https://polyhaven.com/a/mud_cracked_dry_03 | verify | CC0 | yes | PBR set | 1–8K | Dry baked soil for verges and run-off. Tint to laterite red for v1 | P0 |
| A3 | Rocky Trail | Poly Haven | https://polyhaven.com/a/rocky_trail | verify (pine_forest collection) | CC0 | yes | PBR set | 1–8K | Rough gravel path with stones; alternative road base to `gravel_road` | P1 |
| A4 | Aerial Mud 1 | Poly Haven | https://polyhaven.com/a/aerial_mud_1 | verify | CC0 | yes | PBR set | 8 m wide | Real tyre ruts at road scale. Use its height/normal as the wheel-line mask; desaturate the albedo for dry roads | P1 |
| A5 | Coast Sand Rocks 02 | Poly Haven | https://polyhaven.com/a/coast_sand_rocks_02 | verify | CC0 | yes | PBR set (gltf/blend/MaterialX) | 1–8K | Pale sandy gravel close to v3's pale road | P2 |
| A6 | Sand Rocks Small 01 | Poly Haven | https://polyhaven.com/a/sand_rocks_small_01 | verify | CC0 | yes | PBR set | verify | Loose-stone layer for the road edges | P2 |
| A7 | Dirt Floor | Poly Haven | https://polyhaven.com/a/dirt_floor | verify | CC0 | yes | PBR set | verify | Fine compacted dirt for the wheel lines | P2 |
| A8 | Brown Mud Leaves 01 | Poly Haven | https://polyhaven.com/a/brown_mud_leaves_01 | verify | CC0 | yes | PBR set | 1.3 m | Broadleaf forest floor for v2-type stages (ours is mostly needles) | P2 |
| A9 | Dry Straw Grass / Hay (Ground 0018) | TextureCan | https://www.texturecan.com/details/190/ | TextureCan | "FREE for personal and commercial use" (CC0 not confirmed) | verify | PBR set | verify | Golden dry-grass ground splat for v3 | P1 if CC0 is confirmed |

### B. Dry grass and ground cover (P0)

| # | Asset | Source | URL | Author | Licence | Passes? | Format | Size | Fixes | Pri |
|---|---|---|---|---|---|---|---|---|---|---|
| B1 | Grass Bermuda 01 | Poly Haven | https://polyhaven.com/a/grass_bermuda_01 | verify | CC0 | yes | glTF + 2–8K maps | 224K tris (3.1 m patch) | Source for **dry-grass cards**. Bake clumps to an atlas with `render_tree_cards.py`, then hue-shift to straw. Way over budget as geometry | P0 |
| B2 | Dry Branches Medium 01 | Poly Haven | https://polyhaven.com/a/dry_branches_medium_01 | verify | CC0 | yes | glTF | 17K tris | Dead scrub and debris on dry verges; adds the v1 red-brown shrub texture | P1 |
| B3 | Namaqualand collection (dry scrub, rocks, 10+ HDRIs) | Poly Haven | https://polyhaven.com/collections/namaqualand (seen as /es/ and /de/ pages) | Poly Haven team | CC0 | yes | glTF / HDR | various | Dry-biome dressing set from one location, so the colours match | P1 |

### C. Trees for the dry stage (P0: no ready-made eucalyptus, see §4)

| # | Asset | Source | URL | Author | Licence | Passes? | Format | Size | Fixes | Pri |
|---|---|---|---|---|---|---|---|---|---|---|
| C1 | Bark Platanus | Poly Haven | https://polyhaven.com/a/bark_platanus | verify | CC0 | yes | PBR set | 1–8K | Mottled pale plane-tree bark, the closest free match to **gum-tree bark**, for an in-house eucalyptus | P0 |
| C2 | Searsia Lucida | Poly Haven | https://polyhaven.com/a/searsia_lucida | James Ray Cock (modelling) | CC0 | yes | glTF | 842K tris, 4.8 m | Dry-country small tree with a sparse canopy. Bake to cards like the existing trees | P1 |
| C3 | Gum tree | OpenGameArt | https://opengameart.org/content/gum-tree | lattice | CC-BY 3.0 | yes (credit) | .blend + bark/leaf textures | 1,266 tris (2010) | Low-quality stop-gap eucalyptus. Its leaf/bark textures may seed the in-house one | P2 |
| C4 | Japanese Camphor Bark | Poly Haven | https://polyhaven.com/a/japanese_camphor_bark | verify | CC0 | yes | PBR set | 1–8K | Alternative darker trunk bark for the forest broadleaf (v2) | P2 |
| C5 | Silver Princess gum nut (Eucalyptus caesia) | Sketchfab | https://sketchfab.com/3d-models/silver-princess-gum-nut-9dad20e1357d4631a3e1e5007d6b02a6 | verify | CC Attribution | yes (credit), verify download | scan | 93.4K tris | A real eucalyptus leaf/nut scan to render a **leaf atlas** for C1 | P2 |
| C6 | eucalyptus tree | Sketchfab | https://sketchfab.com/3d-models/eucalyptus-tree-a985e00a7ccb475f8e7d989f87b7505b | ndt5292 | not shown | verify | verify | ~999 tris | Possible low-poly eucalyptus; quality unknown | P2 |
| C7 | Lathamus Keep, Tasmania (colour) | Sketchfab | https://sketchfab.com/3d-models/lathamus-keep-tasmania-colour-3135167f4e254c7b9eabdd2ec83a8416 | Q-ForestLab (Ghent Univ.) | not shown | verify | LiDAR scan | 80 m E. globulus | Proportion/silhouette reference only (a point cloud, not a game mesh) | P2 |

### D. Sky HDRIs, used for lighting and reflections (P1)

Already in the game: Kloofendal partly cloudy, misty and overcast; Misty Pines; Qwantani Sunset; Snow Field. All of the following are Poly Haven, CC0, passes = yes, `.hdr`/`.exr` from 1K to 16K. Ship them at 2K, as now.

| # | HDRI | URL | Use | Pri |
|---|---|---|---|---|
| D1 | Noga | https://polyhaven.com/a/noga | **Midday dry**: clear, high-contrast veld. Drives v1 hard shadows | P0 |
| D2 | Evening Field | https://polyhaven.com/a/evening_field | **Golden-hour grassland** (field, birch, partly cloudy). v3 and it's European | P0 |
| D3 | Golden Gate Hills | https://polyhaven.com/a/golden_gate_hills | Dry golden hills by day | P1 |
| D4 | Straw Rolls Field 01 | https://polyhaven.com/a/straw_rolls_field_01 | Harvested field with straw rolls; also a reference for hay-bale dressing | P1 |
| D5 | Whipple Creek Regional Park 01 | https://polyhaven.com/a/whipple_creek_regional_park_01 | **Overcast forest** (v2-type stages) | P1 |
| D6 | Kloofendal 43d Clear | https://polyhaven.com/a/kloofendal_43d_clear | Clear-sky alternative to D1 | P1 |
| D7 | Kloofendal 38d Partly Cloudy | https://polyhaven.com/a/kloofendal_38d_partly_cloudy | Partly cloudy day | P2 |
| D8 | Meadow 2 | https://polyhaven.com/a/meadow_2 | Forest clearing | P2 |
| D9 | Citrus Orchard Road | https://polyhaven.com/a/citrus_orchard_road | Mediterranean rural road (fits the Iberian framing) | P2 |
| D10 | Alps Field / Farm Field / Golf Course Sunrise / Zwartkops Straight Sunset | https://polyhaven.com/a/alps_field · https://polyhaven.com/a/farm_field · https://polyhaven.com/a/golf_course_sunrise · https://polyhaven.com/a/zwartkops_straight_sunset | Spares for the time-of-day keys | P2 |
| D11 | Goegap Road; Mpumalanga Veld (Pure Sky); Rustig Koppie (Pure Sky) | verify (named by third parties as Poly Haven; asset page not seen) | Dry veld; "strongest directional light" per a forum user | P1 if found |

### E. Event dressing: barriers, bales, tents, signage (P1)

| # | Asset | Source | URL | Author | Licence | Passes? | Format | Size | Fixes | Pri |
|---|---|---|---|---|---|---|---|---|---|---|
| E1 | Concrete Road Barrier / 02 (**already in `assets_src`, not shipped**) | Poly Haven | https://polyhaven.com/a/concrete_road_barrier | Amal Kumar | CC0 | yes | glTF | 60.9K / 23.8K tris | The v1 barrier line. Just needs decimating to <2K plus LODs and placing on the outside of bends | P0 (no download) |
| E2 | Hay bales | Sketchfab | https://sketchfab.com/3d-models/hay-bales-9a9f28d0e59a43e49d860912c47b8767 | Abby Crawford | CC Attribution (+ "NoAI" dataset clause; it doesn't restrict game use) | yes (credit) | photoscan | 333K tris | Realistic hay-bale chicanes and corner protection; bake down to ~2K. Replaces the 308-tri OGA bale | P1 |
| E3 | Hay bale (round) | Sketchfab | https://sketchfab.com/3d-models/hay-bale-198960b1802b4bf3bcd6b8032b20c790 | SenYul | not shown | verify | scan | verify | Round-bale variant | P2 |
| E4 | Tent | Sketchfab | https://sketchfab.com/3d-models/tent-df12f702f3ea41f7bb395242aa61b0b0 | verify | CC Attribution | yes (credit), verify | verify | verify | Spectator-area tents | P2 |
| E5 | City Kit (Roads): barriers, construction fences | Kenney | https://kenney-assets.itch.io/city-kit-roads | Kenney | CC0 | yes | glTF/FBX | low-poly | Stylised. **Base shapes only**, restyle (brief §3.3) | P2 |
| E6 | Racing Kit | Kenney | https://www.kenney.nl/assets/racing-kit | Kenney | CC0 | yes | glTF/FBX | 110 files | Stylised; contents unconfirmed (flags/barriers?) | P2 |

### F. Spectators and crowd (P1)

| # | Asset | Source | URL | Author | Licence | Passes? | Format | Size | Fixes | Pri |
|---|---|---|---|---|---|---|---|---|---|---|
| F1 | MakeHuman (generator; exports are CC0) | MakeHuman | https://static.makehumancommunity.org/makehuman/faq/are_makehuman_files_free.html | MakeHuman team | Exports **CC0** from an official, unmodified build. **User-contributed repository assets can be CC BY**: check each one | yes (core assets only) | export → glTF/FBX via Blender | n/a | **Recommended crowd source.** Generate 20–40 varied realistic people, pose them, bake them into impostor/card atlases (2 quads each) | P1 |
| F2 | Universal Base Characters | Quaternius | https://quaternius.com/packs/universalbasecharacters.html | Quaternius | CC0 (stated on pack page). A secondary source mentions a newer "Quaternius Asset Licence": verify it doesn't apply to this pack | yes, verify | glTF/FBX, humanoid rig | ~13K tris avg | Rigged, mid-poly. Usable for near-road animated spectators. Stylised proportions: retexture | P1 |
| F3 | Universal Animation Library | Quaternius | https://quaternius.com/packs/universalanimationlibrary.html | Quaternius | CC0 | yes | glTF/FBX | 120+ clips | Idle, wave and emote animations for F1/F2 (no "cheer" clip confirmed) | P1 |
| F4 | Set of woman sit/walk/stand posed scan | Sketchfab | https://sketchfab.com/3d-models/set-of-woman-sit-walk-and-stand-posed-3d-scan-0d1b6a5e956f47e3b2a1a47c26b4cd14 | sharplaninac | not shown | verify | scan, 8K maps | verify | Realistic posed scans for impostors, if CC | P2 |
| — | Mixamo characters/animations | Adobe | — | Adobe | Adobe terms, **not CC**: no standalone redistribution | **no** (fails the rule) | | | Flagged: do not use | — |

### G. Dust and smoke VFX textures (P1)

Already in the game: Kenney Smoke Particles (white and black puffs).

| # | Asset | Source | URL | Author | Licence | Passes? | Format | Size | Fixes | Pri |
|---|---|---|---|---|---|---|---|---|---|---|
| G1 | Free VFX image sequences and flipbooks | Unity Labs | https://unity.com/blog/engine-platform/free-vfx-image-sequences-flipbooks | Unity Labs Paris | CC0 | yes (2016: check the links still work) | EXR / TGA sequences + some flipbook sheets | various | **Animated smoke/wisp flipbooks** for the lingering gravel dust cloud (§3.4). Tint ochre/grey | P0 |
| G2 | Particle Pack | Kenney | https://kenney.nl/assets/particle-pack | Kenney | CC0 | yes | PNG | 80 sprites, 512² | Soft masks, sparks and dirt flecks for gravel spray | P1 |
| G3 | EmberGen free simulations (dust, smoke, clouds) | JangaFX (via 80.lv) | https://80.lv/articles/get-free-fire-smoke-dust-clouds-volumetric-simulations-made-in-embergen | JangaFX | CC0 (per article) | verify on JangaFX | VDB | volumes | Render real **dust/ground-impact** volumes in Blender into our own lit flipbooks | P1 |
| G4 | Smoke sprite sheet | OpenGameArt | https://opengameart.org/content/smoke-sprite-sheet | ohyhei | CC0 | yes | PNG sheet | verify | Spare flipbook | P2 |
| G5 | Golgotha smoke puffs | OpenGameArt | https://opengameart.org/node/7760 | Golgotha / bart | CC0 | yes | JPG 256² | 256² | Spare single puffs | P2 |

### H. Cockpit parts (P2: the cockpit is ours, keep it original)

| # | Asset | Source | URL | Author | Licence | Passes? | Format | Size | Fixes | Pri |
|---|---|---|---|---|---|---|---|---|---|---|
| H1 | Racing seat (scan) | Sketchfab | https://sketchfab.com/3d-models/racing-seat-5484796999024ce78889c5507e3449aa | peel-3d.com | CC Attribution | yes (credit). **Check for and remove maker logos** | scan | 2.2M tris | Bake its normal/AO onto our low-poly seat for real fabric and foam detail | P2 |
| H2 | Doutrive '78 (pickup with stripped interior + roll cage) | Sketchfab | https://sketchfab.com/3d-models/doutrive-78-low-poly-model-30700a84311c4bea9ef722f0773ce92a | Daniel Zhabotinsky | CC Attribution | yes (credit) | glTF | verify | Reference for cage tube layout and stripped-cabin materials only; don't ship it | P2 |

Total candidates: ~50 (41 numbered plus the D10/D11 groups and the existing items).

## 3. Fab (owner asked specifically)

**How Fab licenses free content.** Epic's Fab documentation lists two licence types: **Creative Commons Attribution (CC BY), always free**, and **Standard, free or paid** (https://dev.epicgames.com/documentation/en-us/fab/licenses-and-pricing-in-fab). Only listings marked CC BY pass our rule. Most free Fab items, including every Quixel/Megascans scan, are **Standard**. Megascans were free for all engines only **until the end of 2024**. Since 2025 they are mostly paid, with a rotating set of limited-time free items (CG Channel, 80.lv). Older Bridge/Quixel downloads were under the Epic Content License, which is **Unreal-Engine-only**.

| Fab listing | URL | Licence / price as seen | Passes? |
|---|---|---|---|
| Giant Eucalyptus Tree 3D Scan (KOREYBA) | https://www.fab.com/listings/137079c7-e901-44a0-b675-3a574973257a | Paid, from $49.99, Standard; 500K / 57M tris, 16K textures | **no** |
| Gravel Dirt Road (Quixel scan, 2×4 m, 4096 px/m) | https://www.fab.com/listings/ceea31b4-4b19-492f-9691-506a0336460b | Quixel → Standard; price verify | **no** |
| Gravel Road (Quixel scan, 4×8 m) | https://www.fab.com/listings/c39c2988-4646-42a8-b824-a9b7f32a954e | Quixel → Standard | **no** |
| Uncut Grass (Quixel material; listing ID verify) | https://www.fab.com/listings/b955074d-b540-4519-94fe-7f362d269476 | Standard (Quixel), shown free in a category page | **no** |
| Dry Grass / Dry Patchy Grass Ground (two listings; which is which: verify) | https://www.fab.com/listings/a1bf290b-d62f-43d0-b5a0-852479a1541e · https://www.fab.com/listings/893c3745-6203-4dbe-bfbb-7c489174d88d | not shown | verify (most likely Standard → no) |
| Low-poly Animated Cheering Crowd (~34K faces, 5 cheering figures) | https://www.fab.com/listings/551903f0-5843-4fee-a90a-56dd9d6d1965 | not shown | verify. Would be the best crowd find **if** it is CC BY |
| Low Poly Autumn People Pack – Posed (90 posed, glTF) and others in the same result set | https://www.fab.com/listings/12dcc233-584c-43ca-ae8f-da3880cefd58 (title-to-ID match: verify) | not shown | verify |
| Barrier Pack 1 | https://www.fab.com/listings/cdc7df4c-645e-46fb-958f-52aa6c72ad7b | not shown | verify |
| Traffic cones (PBR, colours) | https://www.fab.com/listings/dd673afa-a859-46ae-a1a5-e16776c3491a | not shown | verify |
| Street Props Big Pack | https://www.fab.com/listings/2abe534a-d150-4646-966d-6caa23c2b4ac | not shown | verify |
| Gravel materials category (many free Quixel) | https://www.fab.com/category/material/nature-terrain--gravel | Standard | **no** |

**Result:** no Fab item was confirmed to pass the rule. Fab's CC BY items are mostly former Sketchfab uploads, and the same models are usually easier to licence-check on Sketchfab itself. On Fab, filter on licence = "Creative Commons" and read the licence box on each page.

**If the owner changed the rule (for information only):**
- **Fab Standard License.** Epic says it covers use "in any engine or tool", including commercial games, with no attribution. You may not redistribute the raw assets, and there are EULA restrictions, including on AI use. Standard-licensed items would then be legal in the Bevy build as well, but the full Fab EULA must be read first. Epic labels its summary "not legally binding". Re-claiming legacy Megascans on Fab converts them to Standard.
- **UE-only (Epic Content License, legacy Quixel/Bridge downloads).** Usable only in Unreal Engine projects, so it would be legal in `/Users/himanshu/codes/game/pacenote_ue` and **not** in the Bevy game. The two builds would then look different and `ATTRIBUTION.md` would need a per-build split.
- The big payoff from either change is Megascans' photoscanned dry grass, gravel road and scrub, which are much better than anything free. The cost is a second licence regime to track.

## 4. Gaps with no good free asset, and fallbacks

| Gap | Why | Fallback |
|---|---|---|
| **Eucalyptus / gum tree** | No CC0/CC BY realistic model found. Fab and CGTrader ones are paid; the OGA one is from 2010 | **In-house**: Blender Sapling or Geometry Nodes trunk (tall, leaning, few big limbs), C1 bark tinted cream-grey with ochre strips, leaf clumps from a leaf atlas (C5 scan or our own photo), then render to cards with `tools/art/render_tree_cards.py`. About 1–2 days of tech-art work |
| **Realistic spectators** | Free scans aren't CC; Mixamo is not CC | MakeHuman (F1) → 30 posed variants → impostor atlas (front/side) with a team-colour clothing tint per instance. Quaternius (F2/F3) for the few animated people next to the road |
| **Rally tape, flags, stage boards, start arch, spectator signs** | None found (and they have to be original anyway) | **In-house**: a ribbon mesh along stakes with an original red/white tape texture, simple cloth flags with vertex wind, an original stage-board atlas. Matches the sign-atlas plan in `ART_BIBLE.md` §4 |
| **Laterite red berm / red soil** | No CC0 "red laterite" texture confirmed (the aitextured one is not CC) | Recolour A1/A2 in the terrain shader (hue + value), plus a berm strip mesh. Check ambientCG's Ground series directly (not indexed by search) |
| **Wheel-line / gravel berm road look** | A texture can't follow the road's curves | Procedural mask along the road station (brief §3.2), with A4 used as the detail map |
| **Dust cloud flipbook lit by the sun** | The free flipbooks are generic smoke | Render G3 VDBs, or a Blender smoke sim, into a 8×8 normal-mapped flipbook in-house |
| **Cockpit parts** (cage padding, harness, kill switch, intercom) | Free CC ones are branded or too heavy | Keep the cockpit in-house. Use H1/H2 only as bake and reference sources |
| **Motion blur, haze, exposure, cloud layer** | Not assets at all | `ART_BIBLE.md` §2.2 / §3.3 / §7 priorities 1, 2 and 4 |

## 5. Recommended first batch (download and verify)

Pick order puts the biggest look gain per hour first. All are Poly Haven CC0 unless noted.

1. **Noga** HDRI (D1): midday dry key light.
2. **Evening Field** HDRI (D2): golden-hour grassland.
3. **Whipple Creek Regional Park 01** HDRI (D5): overcast forest.
4. **Golden Gate Hills** HDRI (D3).
5. **Rocks Ground 02** (A1): dry berm and shoulder.
6. **Mud Cracked Dry 03** (A2): dry/red soil verge.
7. **Rocky Trail** (A3): alternative gravel road base.
8. **Aerial Mud 1** (A4): wheel-line detail.
9. **Grass Bermuda 01** (B1): dry-grass card source.
10. **Bark Platanus** (C1): gum-bark proxy for the in-house eucalyptus.
11. **Searsia Lucida** (C2): dry-stage tree cards.
12. **Dry Branches Medium 01** (B2): dry scrub debris.
13. **Unity Labs VFX flipbooks** (G1, CC0): dust cloud.
14. **Kenney Particle Pack** (G2, CC0).
15. **Hay bales**, Abby Crawford (E2, **CC BY: add credit**). Also promote the already-downloaded concrete barriers (E1).

Do **in parallel, before judging any of these:** `ART_BIBLE.md` §7 priority 1 (ambient down, fog off, auto exposure). Without it, new textures will still look flat. Gaps 1, 6 and 7 are rendering work, not assets.

When downloading: record each item in `assets_src/SOURCES.md` with the author shown on the page (many "verify" authors above). Add CC BY credit lines to `assets/credits.txt`. Bake the Poly Haven models (100K–800K tris) to cards or decimate them to budget: tree LOD0 4–8K, props <2K.
