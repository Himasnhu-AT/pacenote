# Pacenote: third-party asset sources

Every file under `assets_src/` is listed here. Only CC0 and CC BY licences are used. No asset comes from or imitates a commercial game. Downloaded 2026-10-28. Poly Haven files come from the public API (`https://api.polyhaven.com/files/<id>`, file URLs under `https://dl.polyhaven.org/file/ph-assets/...`); ambientCG files from `https://ambientcg.com/get?file=<id>_2K-JPG.zip` (only the images were kept: the NormalDX map and the bundled .blend were deleted, the .png is ambientCG's preview).

**Total on disk: 694.6 MB** across 54 assets (2026-10-28), plus 1,645 MB across 5 assets added 2026-11-01 (`fir_tree_01`, `pine_tree_01`, `tree_small_02`, `island_tree_01`, `pine_bark`).

## Credits that must appear in the game (CC BY)

- None today. The player's car (`assets/models/rallycar.glb`, the "Talus" rally hatch) is original, made for Pacenote by `tools/art/build_rallycar.py` (no third-party content; design sheet `docs/cars/rally_hatch.md`), and the driver's arms are drawn in code (`src/art/hands.rs`).
- The CC BY GT3, First Person arms, Milano '95 and Car Concept below are no longer shipped (removed from `assets/credits.txt`). Restore their credit lines if any is ever used again; the GT3 must not be (trademarks).

CC0 assets need no credit; a courtesy line is suggested: "Textures, HDRIs and models from Poly Haven (polyhaven.com) and ambientCG (ambientcg.com), CC0. Car sounds from Freesound (freesound.org), CC0. Smoke sprites by Kenney (kenney.nl), CC0. Hay bale and fence by carlosjorgereis (OpenGameArt), CC0."


## models/car

### Car Concept (`khronos_car_concept`): former placeholder car, no longer used

- **Use:** Realistic car for driving/physics/rendering tests until a hatchback is sourced. Sporty concept with a rear hatch; separate wheel nodes (WheelFrontL/R, WheelRearL/R + Rim/BrakeDisc/BrakePad children, Axles). NOT a hatchback rally car. Contains Khronos and 3D Commerce logos (licence-plate texture, steering-wheel emblem): these are trademarks, not CC-BY, and must be removed/replaced before shipping. Uses KHR_materials_variants/clearcoat/transmission/iridescence.
- **Path:** `assets_src/models/car/khronos_car_concept/`
- **Source page:** https://github.com/KhronosGroup/glTF-Sample-Assets/tree/main/Models/CarConcept
- **Download:** https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/main/Models/CarConcept/glTF-Binary/CarConcept.glb
- **Author:** Eric Chadwick (model and textures), © 2024 Darmstadt Graphics Group GmbH; derived from a CC0 concept car by Unity Fan
- **Licence:** CC BY 4.0 (Creative Commons Attribution 4.0 International); Khronos logos: Khronos Trademark or Logo (not licensed for reuse)
- **Size:** 11.8 MB
- **Resolution / polys:** 213,347 tris, 14 embedded textures, GLB
- **Credit line:** REQUIRED: "Car Concept" by Eric Chadwick, © 2024 Darmstadt Graphics Group GmbH, based on a CC0 model by Unity Fan. Licensed under CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). Modified.


## models/sketchfab

Downloaded by the owner (Sketchfab needs a login) into `assets_src/sketchfab/`; converted by `tools/art/import_sketchfab.py`. Licence facts read from the Sketchfab API (`https://api.sketchfab.com/v3/models/<uid>`) on 2026-10-31.

### Mercedes-AMG GT3 Evo 2020 (`mercedes_amg_gt3_evo`): former player car, no longer shipped

- **Status (2026-11-02):** replaced by the original Talus rally hatch (`tools/art/build_rallycar.py`); not shipped, not to be used again (trademarks, below). Its converter profile was removed from `tools/art/import_sketchfab.py`.
- **Use (until 2026-11-02):** The player's car (`assets/models/rallycar.glb`): body, wheels and its own cabin (seat, dash, cage, yoke steering wheel). Converted by `tools/art/import_sketchfab.py -- car <file> --profile gt3`: scaled to the sim's wheelbase and track, decimated ~496k → ~124k triangles, textures capped at 2048 (opaque ones as JPEG), its steering wheel made the animated `SteeringWheel` node, our shifter and handbrake added, cabin ambient occlusion baked. Rigged: Chassis (body), Hub/Steer/Spin per wheel, detachable MirrorL/R, RearWing, FrontSplitter, Front/RearBumper, RearDiffuser; a passenger seat mirrored from the driver's (the battery box on that floor removed); CoDriverEye/CoDriverTablet anchors.
- **Path:** `assets_src/sketchfab/mercedes_amg_gt3_evo/mercedes_benz_amg_gt3_red_bull.glb` (from the owner's download `mercedes-amg-gt3-red-bull-racing.zip`, 2026-11-01; the zip's `textures/` folder only repeats the images embedded in the glb)
- **Source page:** https://sketchfab.com/3d-models/mercedes-amg-gt3-evo-2020-wwwvecarzcom-39e77243109f4e5cb7134b6f31c8f619
- **Download:** Sketchfab download dialog (login required), by the owner
- **Author:** vecarz (https://sketchfab.com/heynic)
- **Licence:** CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/)
- **Size:** 55.4 MB glb
- **Resolution / polys:** 496,438 triangles; 57 meshes, 34 materials, 41 images (one 4096², the rest 1024² or less)
- **Credit line:** REQUIRED: "Mercedes-AMG GT3 Evo 2020 | www.vecarz.com" by vecarz (sketchfab.com/heynic), licensed under CC BY 4.0. Modified: scaled, decimated, rigged (wheels, suspension, detachable parts), passenger seat mirrored, cabin tidied, shifter, handbrake and driver figure added.
- **Trademarks:** A real, branded car in a real team livery (Mercedes-Benz star and AMG badges, Red Bull, Michelin, Ravenol, DEKRA, Viaplay, Sparco and others). CC BY covers the model's copyright only, not these trademarks or the car's design; see the note at the end of this file on branded cars. Check with the trademark owners (or replace the livery and badges) before a commercial release.

### Milano '95 - Low poly model (`milano_95`): former car body, no longer shipped

- **Use:** Car body (exterior, glass, wheels). Its own interior is dropped; the cockpit is ours (`tools/art/cockpit.py`).
- **Path:** `assets_src/sketchfab/` (as downloaded)
- **Source page:** https://sketchfab.com/3d-models/milano-95-low-poly-model-122a22d210234596ad23623588d0d166
- **Download:** Sketchfab download dialog (login required), by the owner
- **Author:** Daniel Zhabotinsky (https://sketchfab.com/DanielZhabotinsky)
- **Licence:** CC BY 4.0 (Sketchfab "CC Attribution": "Author must be credited. Commercial use is allowed.", http://creativecommons.org/licenses/by/4.0/). The description adds: "I unconditionally give you permission to use this model at your discretion, modify it or implement it into a digital product."
- **Size:** not known until downloaded
- **Resolution / polys:** 18,064 faces, 11,130 vertices; 8 materials, 20 textures (Sketchfab API)
- **Credit line:** REQUIRED: "Milano '95 - Low poly model" by Daniel Zhabotinsky (sketchfab.com/DanielZhabotinsky), licensed under CC BY 4.0. Modified: scaled, wheels split, interior replaced.

### First Person arms (`first_person_arms`): not shipped

- **Status (2026-11-02):** not in `assets/` (no `driver_arms.glb`); the game draws the arms in code (`src/art/hands.rs`), so its credit line was removed from `assets/credits.txt`. Restore it if the converted arms are ever shipped.

- **Use:** The driver's arms and gloved hands in the cockpit view, posed by `src/art/hands.rs`.
- **Path:** `assets_src/sketchfab/` (as downloaded)
- **Source page:** https://sketchfab.com/3d-models/first-person-arms-e3c42c05b22944e5839deb8e003f0987
- **Download:** Sketchfab download dialog (login required), by the owner
- **Author:** DJMaesen (Sketchfab user bumstrum, https://sketchfab.com/bumstrum)
- **Licence:** CC BY 4.0 (Sketchfab "CC Attribution", http://creativecommons.org/licenses/by/4.0/)
- **Size:** not known until downloaded
- **Resolution / polys:** 7,240 faces, 3,624 vertices; "rigged low poly first person arms 2k maps" (Sketchfab API)
- **Credit line:** REQUIRED: "First Person arms" by DJMaesen (sketchfab.com/bumstrum), licensed under CC BY 4.0. Modified: rigged to the cockpit, recoloured.


## textures/ground

### Asphalt 02 (`asphalt_02`)

- **Use:** tarmac / asphalt
- **Path:** `assets_src/textures/ground/asphalt_02/`
- **Source page:** https://polyhaven.com/a/asphalt_02
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/asphalt_02/asphalt_02_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/asphalt_02)
- **Author:** Rob Tuytel / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 14.7 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Asphalt 02" by Rob Tuytel, Poly Haven

### Brown Mud 02 (`brown_mud_02`)

- **Use:** muddy ground
- **Path:** `assets_src/textures/ground/brown_mud_02/`
- **Source page:** https://polyhaven.com/a/brown_mud_02
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/brown_mud_02/brown_mud_02_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/brown_mud_02)
- **Author:** Rob Tuytel / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 11.3 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Brown Mud 02" by Rob Tuytel, Poly Haven

### Cliff Side (`cliff_side`)

- **Use:** rock / cliff
- **Path:** `assets_src/textures/ground/cliff_side/`
- **Source page:** https://polyhaven.com/a/cliff_side
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/cliff_side/cliff_side_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/cliff_side)
- **Author:** James Ray Cock, Jenelle van Heerden, Dario Barresi / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 12.7 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Cliff Side" by James Ray Cock, Jenelle van Heerden, Dario Barresi, Poly Haven

### Forest Ground 04 (`forest_ground_04`)

- **Use:** forest floor (pine needles, cones)
- **Path:** `assets_src/textures/ground/forest_ground_04/`
- **Source page:** https://polyhaven.com/a/forest_ground_04
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/forest_ground_04/forest_ground_04_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/forest_ground_04)
- **Author:** Rob Tuytel, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 19.4 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Forest Ground 04" by Rob Tuytel, Rico Cilliers, Poly Haven

### Forest Leaves 02 (`forest_leaves_02`)

- **Use:** forest floor (leaves)
- **Path:** `assets_src/textures/ground/forest_leaves_02/`
- **Source page:** https://polyhaven.com/a/forest_leaves_02
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/forest_leaves_02/forest_leaves_02_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/forest_leaves_02)
- **Author:** Rob Tuytel / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 7.5 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Forest Leaves 02" by Rob Tuytel, Poly Haven

### Grass 004 (`Grass004`)

- **Use:** dense grass
- **Path:** `assets_src/textures/ground/Grass004/`
- **Source page:** https://ambientcg.com/view?id=Grass004
- **Download:** https://ambientcg.com/get?file=Grass004_2K-JPG.zip
- **Author:** Lennart Demes (ambientCG) / ambientCG
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 28.8 MB
- **Resolution / polys:** 2K JPG: Color, NormalGL, Roughness, AO, Displacement
- **Credit line:** Optional (CC0): "Grass 004" by Lennart Demes (ambientCG), ambientCG

### Grass Path 2 (`grass_path_2`)

- **Use:** road edge / verge (grass to dirt)
- **Path:** `assets_src/textures/ground/grass_path_2/`
- **Source page:** https://polyhaven.com/a/grass_path_2
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/grass_path_2/grass_path_2_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/grass_path_2)
- **Author:** Rob Tuytel / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 9.3 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Grass Path 2" by Rob Tuytel, Poly Haven

### Gravel Road (`gravel_road`)

- **Use:** loose gravel / dirt road
- **Path:** `assets_src/textures/ground/gravel_road/`
- **Source page:** https://polyhaven.com/a/gravel_road
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/gravel_road/gravel_road_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/gravel_road)
- **Author:** Amal Kumar / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 11.4 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Gravel Road" by Amal Kumar, Poly Haven

### Ground 037 (`Ground037`)

- **Use:** overgrown forest/grass verge
- **Path:** `assets_src/textures/ground/Ground037/`
- **Source page:** https://ambientcg.com/view?id=Ground037
- **Download:** https://ambientcg.com/get?file=Ground037_2K-JPG.zip
- **Author:** Lennart Demes (ambientCG) / ambientCG
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 26.4 MB
- **Resolution / polys:** 2K JPG: Color, NormalGL, Roughness, AO, Displacement
- **Credit line:** Optional (CC0): "Ground 037" by Lennart Demes (ambientCG), ambientCG

### Ground 077 (`Ground077`)

- **Use:** pine forest floor with needles and cones
- **Path:** `assets_src/textures/ground/Ground077/`
- **Source page:** https://ambientcg.com/view?id=Ground077
- **Download:** https://ambientcg.com/get?file=Ground077_2K-JPG.zip
- **Author:** Lennart Demes (ambientCG) / ambientCG
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 26.8 MB
- **Resolution / polys:** 2K JPG: Color, NormalGL, Roughness, AO, Displacement
- **Credit line:** Optional (CC0): "Ground 077" by Lennart Demes (ambientCG), ambientCG

### Leafy Grass (`leafy_grass`)

- **Use:** grass / meadow
- **Path:** `assets_src/textures/ground/leafy_grass/`
- **Source page:** https://polyhaven.com/a/leafy_grass
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/leafy_grass/leafy_grass_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/leafy_grass)
- **Author:** Charlotte Baglioni / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 23.9 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Leafy Grass" by Charlotte Baglioni, Poly Haven

### Muddy Tracks (`muddy_tracks`)

- **Use:** muddy ground with tyre ruts
- **Path:** `assets_src/textures/ground/muddy_tracks/`
- **Source page:** https://polyhaven.com/a/muddy_tracks
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/muddy_tracks/muddy_tracks_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/muddy_tracks)
- **Author:** Amal Kumar / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 20.1 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Muddy Tracks" by Amal Kumar, Poly Haven

### Rock Face 03 (`rock_face_03`)

- **Use:** rock / cliff
- **Path:** `assets_src/textures/ground/rock_face_03/`
- **Source page:** https://polyhaven.com/a/rock_face_03
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/rock_face_03/rock_face_03_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/rock_face_03)
- **Author:** Dario Barresi, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 17.5 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Rock Face 03" by Dario Barresi, Rico Cilliers, Poly Haven

### Rocky Gravel (`rocky_gravel`)

- **Use:** loose gravel
- **Path:** `assets_src/textures/ground/rocky_gravel/`
- **Source page:** https://polyhaven.com/a/rocky_gravel
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/rocky_gravel/rocky_gravel_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/rocky_gravel)
- **Author:** Dario Barresi, Dimitrios Savva / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 23.5 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Rocky Gravel" by Dario Barresi, Dimitrios Savva, Poly Haven

### Snow 02 (`snow_02`)

- **Use:** snow
- **Path:** `assets_src/textures/ground/snow_02/`
- **Source page:** https://polyhaven.com/a/snow_02
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/snow_02/snow_02_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/snow_02)
- **Author:** Rob Tuytel / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 8.9 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Snow 02" by Rob Tuytel, Poly Haven

### Sparse Grass (`sparse_grass`)

- **Use:** grass / meadow
- **Path:** `assets_src/textures/ground/sparse_grass/`
- **Source page:** https://polyhaven.com/a/sparse_grass
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/sparse_grass/sparse_grass_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/sparse_grass)
- **Author:** Amal Kumar / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 21.4 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Sparse Grass" by Amal Kumar, Poly Haven

### Stony Dirt Path (`stony_dirt_path`)

- **Use:** dirt road / stony track
- **Path:** `assets_src/textures/ground/stony_dirt_path/`
- **Source page:** https://polyhaven.com/a/stony_dirt_path
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/stony_dirt_path/stony_dirt_path_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/stony_dirt_path)
- **Author:** eye-candy.xyz / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 21.6 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Stony Dirt Path" by eye-candy.xyz, Poly Haven

### Tarred Gravel (`tarred_gravel`)

- **Use:** chip-seal country road tarmac
- **Path:** `assets_src/textures/ground/tarred_gravel/`
- **Source page:** https://polyhaven.com/a/tarred_gravel
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/tarred_gravel/tarred_gravel_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/tarred_gravel)
- **Author:** Dimitrios Savva / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 18.9 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Tarred Gravel" by Dimitrios Savva, Poly Haven


## textures/bark

### Pine Bark (`pine_bark`), added 2026-11-01

- **Use:** conifer trunks (`assets/textures/bark`, `tools/art/prep_textures.sh`)
- **Path:** `assets_src/textures/bark/pine_bark/`
- **Source page:** https://polyhaven.com/a/pine_bark
- **Download:** https://api.polyhaven.com/files/pine_bark → `Diffuse`, `nor_gl`, `arm` at 2k jpg; md5-checked
- **Author:** Dimitrios Savva / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 9.6 MB
- **Resolution / polys:** 2048x2048, 2 m x 2 m
- **Credit line:** Optional (CC0): "Pine Bark" by Dimitrios Savva, Poly Haven


## textures/decals

### Ground 017 (`Ground017`)

- **Use:** ground with tyre tracks
- **Path:** `assets_src/textures/decals/Ground017/`
- **Source page:** https://ambientcg.com/view?id=Ground017
- **Download:** https://ambientcg.com/get?file=Ground017_2K-JPG.zip
- **Author:** Lennart Demes (ambientCG) / ambientCG
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 16.3 MB
- **Resolution / polys:** 2K JPG: Color, NormalGL, Roughness, AO, Displacement
- **Credit line:** Optional (CC0): "Ground 017" by Lennart Demes (ambientCG), ambientCG

### Tire Tracks 001 (`TireTracks001`)

- **Use:** tyre track decal
- **Path:** `assets_src/textures/decals/TireTracks001/`
- **Source page:** https://ambientcg.com/view?id=TireTracks001
- **Download:** https://ambientcg.com/get?file=TireTracks001_2K-JPG.zip
- **Author:** Lennart Demes (ambientCG) / ambientCG
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 4.1 MB
- **Resolution / polys:** 2K JPG: Color, NormalGL, Roughness, AO, Displacement
- **Credit line:** Optional (CC0): "Tire Tracks 001" by Lennart Demes (ambientCG), ambientCG


## textures/foliage

### Pine Needles 001 (`PineNeedles001`)

- **Use:** pine needle atlas (tree cards / LOD billboards)
- **Path:** `assets_src/textures/foliage/PineNeedles001/`
- **Source page:** https://ambientcg.com/view?id=PineNeedles001
- **Download:** https://ambientcg.com/get?file=PineNeedles001_2K-JPG.zip
- **Author:** Lennart Demes (ambientCG) / ambientCG
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 7.9 MB
- **Resolution / polys:** 2K JPG: Color, NormalGL, Roughness, AO, Displacement, Opacity
- **Credit line:** Optional (CC0): "Pine Needles 001" by Lennart Demes (ambientCG), ambientCG


## textures/props

### Reed Roof 03 (`reed_roof_03`)

- **Use:** straw-like surface for procedural hay bales
- **Path:** `assets_src/textures/props/reed_roof_03/`
- **Source page:** https://polyhaven.com/a/reed_roof_03
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/reed_roof_03/reed_roof_03_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/reed_roof_03)
- **Author:** Rob Tuytel / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 12.9 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Reed Roof 03" by Rob Tuytel, Poly Haven

### Weathered Brown Planks (`weathered_brown_planks`)

- **Use:** wood for fences / posts / sign boards
- **Path:** `assets_src/textures/props/weathered_brown_planks/`
- **Source page:** https://polyhaven.com/a/weathered_brown_planks
- **Download:** https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/weathered_brown_planks/weathered_brown_planks_<map>_2k.jpg for <map> in diff, nor_gl, rough, ao, arm, disp (listing: https://api.polyhaven.com/files/weathered_brown_planks)
- **Author:** Dimitrios Savva, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 4.7 MB
- **Resolution / polys:** 2K (2048²) JPG: diff, nor_gl, rough, ao, arm (ORM), disp
- **Credit line:** Optional (CC0): "Weathered Brown Planks" by Dimitrios Savva, Rico Cilliers, Poly Haven


## textures/particles

### Smoke Particles (`kenney_smoke_particles`, White puff + Black smoke subsets)

- **Use:** Dust / exhaust / gravel-cloud particle sprites (25 white puffs, 25 black smoke). Explosion/Flash/Fart sets in the pack were not copied.
- **Path:** `assets_src/textures/particles/kenney_smoke_particles/`
- **Source page:** https://kenney.nl/assets/smoke-particles
- **Download:** https://kenney.nl/assets/smoke-particles (the zip link is a short-lived signed URL issued by the page: https://kenney.nl/download/…?expires=…)
- **Author:** Kenney Vleugels (www.kenney.nl)
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 3.0 MB
- **Resolution / polys:** ~380×350 PNG with alpha, 50 sprites
- **Credit line:** Optional (CC0): "Smoke Particles" by Kenney (www.kenney.nl)


## hdri

### Kloofendal 48d Partly Cloudy (Pure Sky) (`kloofendal_48d_partly_cloudy_puresky`)

- **Use:** sunny afternoon
- **Path:** `assets_src/hdri/kloofendal_48d_partly_cloudy_puresky/`
- **Source page:** https://polyhaven.com/a/kloofendal_48d_partly_cloudy_puresky
- **Download:** https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/2k/kloofendal_48d_partly_cloudy_puresky_2k.hdr
- **Author:** Greg Zaal, Jarod Guest / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 5.5 MB
- **Resolution / polys:** 2048x1024 equirect .hdr
- **Credit line:** Optional (CC0): "Kloofendal 48d Partly Cloudy (Pure Sky)" by Greg Zaal, Jarod Guest, Poly Haven

### Kloofendal Misty Morning (Pure Sky) (`kloofendal_misty_morning_puresky`)

- **Use:** foggy / misty morning sky
- **Path:** `assets_src/hdri/kloofendal_misty_morning_puresky/`
- **Source page:** https://polyhaven.com/a/kloofendal_misty_morning_puresky
- **Download:** https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/2k/kloofendal_misty_morning_puresky_2k.hdr
- **Author:** Greg Zaal / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 3.0 MB
- **Resolution / polys:** 2048x1024 equirect .hdr
- **Credit line:** Optional (CC0): "Kloofendal Misty Morning (Pure Sky)" by Greg Zaal, Poly Haven

### Kloofendal Overcast (Pure Sky) (`kloofendal_overcast_puresky`)

- **Use:** overcast
- **Path:** `assets_src/hdri/kloofendal_overcast_puresky/`
- **Source page:** https://polyhaven.com/a/kloofendal_overcast_puresky
- **Download:** https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/2k/kloofendal_overcast_puresky_2k.hdr
- **Author:** Greg Zaal / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 4.3 MB
- **Resolution / polys:** 2048x1024 equirect .hdr
- **Credit line:** Optional (CC0): "Kloofendal Overcast (Pure Sky)" by Greg Zaal, Poly Haven

### Misty Pines (`misty_pines`)

- **Use:** foggy morning in pine forest
- **Path:** `assets_src/hdri/misty_pines/`
- **Source page:** https://polyhaven.com/a/misty_pines
- **Download:** https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/2k/misty_pines_2k.hdr
- **Author:** Greg Zaal / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 7.1 MB
- **Resolution / polys:** 2048x1024 equirect .hdr
- **Credit line:** Optional (CC0): "Misty Pines" by Greg Zaal, Poly Haven

### Qwantani Sunset (Pure Sky) (`qwantani_sunset_puresky`)

- **Use:** golden hour / dusk
- **Path:** `assets_src/hdri/qwantani_sunset_puresky/`
- **Source page:** https://polyhaven.com/a/qwantani_sunset_puresky
- **Download:** https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/2k/qwantani_sunset_puresky_2k.hdr
- **Author:** Greg Zaal, Jarod Guest / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 4.2 MB
- **Resolution / polys:** 2048x1024 equirect .hdr
- **Credit line:** Optional (CC0): "Qwantani Sunset (Pure Sky)" by Greg Zaal, Jarod Guest, Poly Haven

### Snow Field (Pure Sky) (`snow_field_puresky`)

- **Use:** overcast winter (snow stage)
- **Path:** `assets_src/hdri/snow_field_puresky/`
- **Source page:** https://polyhaven.com/a/snow_field_puresky
- **Download:** https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/2k/snow_field_puresky_2k.hdr
- **Author:** Jarod Guest, Sergej Majboroda / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 4.2 MB
- **Resolution / polys:** 2048x1024 equirect .hdr
- **Credit line:** Optional (CC0): "Snow Field (Pure Sky)" by Jarod Guest, Sergej Majboroda, Poly Haven


## models/vegetation

### Fir Tree 01 (`fir_tree_01`), added 2026-11-01

- **Use:** spruce / fir card atlas (variant `fir_tree_01_a_LOD0`, rendered by `tools/art/render_tree_cards.py` into `assets/textures/card_fir` and `card_fir_snow`)
- **Path:** `assets_src/models/vegetation/fir_tree_01/`
- **Source page:** https://polyhaven.com/a/fir_tree_01
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/fir_tree_01/fir_tree_01_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/fir_tree_01 under gltf.2k, + `twig_alpha` 2k jpg); md5-checked
- **Author:** Rob Tuytel, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 495 MB
- **Resolution / polys:** 6,982,937 triangles in the glTF, 3 trees (a: 18.9 m, 4.18M tris; b: 14.1 m; c: 14.5 m) · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Fir Tree 01" by Rob Tuytel, Rico Cilliers, Poly Haven

### Island Tree 01 (`island_tree_01`), added 2026-11-01

- **Use:** the scrub stages' olive-like tree (atlas `assets/textures/card_olive`, `tools/art/render_tree_cards.py`); replaces the `island_tree_02` card
- **Path:** `assets_src/models/vegetation/island_tree_01/`
- **Source page:** https://polyhaven.com/a/island_tree_01
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/island_tree_01/island_tree_01_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/island_tree_01 under gltf.2k, + `leaves_alpha` 2k jpg); md5-checked
- **Author:** Rob Tuytel, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 79 MB
- **Resolution / polys:** 3,729,692 triangles (Poly Haven listing), one 5.0 m tree · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Island Tree 01" by Rob Tuytel, Rico Cilliers, Poly Haven

### Tree Small 02 (`tree_small_02`), added 2026-11-01

- **Use:** birch crown card atlas and the roadside bushes' leaf clumps (rendered without its trunk by `tools/art/render_tree_cards.py` into `assets/textures/card_birch`; the game draws a white birch trunk)
- **Path:** `assets_src/models/vegetation/tree_small_02/`
- **Source page:** https://polyhaven.com/a/tree_small_02
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/tree_small_02/tree_small_02_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/tree_small_02 under gltf.2k, + `leaves_alpha` 2k jpg); md5-checked
- **Author:** Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 117 MB
- **Resolution / polys:** 2,062,487 triangles in the glTF, one 4.6 m tree · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Tree Small 02" by Rico Cilliers, Poly Haven

### Pine Tree 01 (`pine_tree_01`), added 2026-11-01

- **Use:** Scots pine card atlas (variant `pine_tree_01_a_LOD0`, rendered by `tools/art/render_tree_cards.py` into `assets/textures/card_pine`)
- **Path:** `assets_src/models/vegetation/pine_tree_01/`
- **Source page:** https://polyhaven.com/a/pine_tree_01
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/pine_tree_01/pine_tree_01_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/pine_tree_01 under gltf.2k, + `twig_alpha` 2k jpg); md5-checked
- **Author:** Rob Tuytel, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 944 MB
- **Resolution / polys:** 17,427,094 triangles in the glTF, 3 trees (a: 20.4 m) · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Pine Tree 01" by Rob Tuytel, Rico Cilliers, Poly Haven

### Dead Tree Trunk (`dead_tree_trunk`)

- **Use:** fallen log
- **Path:** `assets_src/models/vegetation/dead_tree_trunk/`
- **Source page:** https://polyhaven.com/a/dead_tree_trunk
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/dead_tree_trunk/dead_tree_trunk_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/dead_tree_trunk under gltf.2k)
- **Author:** Rob Tuytel / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 11.3 MB
- **Resolution / polys:** 101,802 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Dead Tree Trunk" by Rob Tuytel, Poly Haven

### Fern 02 (`fern_02`)

- **Use:** forest fern
- **Path:** `assets_src/models/vegetation/fern_02/`
- **Source page:** https://polyhaven.com/a/fern_02
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/fern_02/fern_02_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/fern_02 under gltf.2k)
- **Author:** Rob Tuytel, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 3.4 MB
- **Resolution / polys:** 6,232 triangles in the glTF, 4 mesh nodes (fern_02_b, fern_02_c, fern_02_a, fern_02_d) · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Fern 02" by Rob Tuytel, Rico Cilliers, Poly Haven

### Fir Sapling (`fir_sapling`)

- **Use:** conifer (young fir)
- **Path:** `assets_src/models/vegetation/fir_sapling/`
- **Source page:** https://polyhaven.com/a/fir_sapling
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/fir_sapling/fir_sapling_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/fir_sapling under gltf.2k)
- **Author:** Rob Tuytel, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 34.6 MB
- **Resolution / polys:** 433,021 triangles in the glTF, 3 mesh nodes (fir_sapling_a, fir_sapling_b, fir_sapling_c) · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Fir Sapling" by Rob Tuytel, Rico Cilliers, Poly Haven

### Grass Medium 01 (`grass_medium_01`)

- **Use:** grass clump
- **Path:** `assets_src/models/vegetation/grass_medium_01/`
- **Source page:** https://polyhaven.com/a/grass_medium_01
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/grass_medium_01/grass_medium_01_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/grass_medium_01 under gltf.2k)
- **Author:** Rob Tuytel, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 8.0 MB
- **Resolution / polys:** 24,730 triangles in the glTF, 17 mesh nodes (grass_medium_01_small_a_LOD0, grass_medium_01_large_b_LOD0, grass_medium_01_mid_b_LOD0, grass_medium_01_tall_b_LOD0, grass_medium_01_tall_a_LOD0, grass_medium_01_tall_c_LOD0…) · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Grass Medium 01" by Rob Tuytel, Rico Cilliers, Poly Haven

### Grass Medium 02 (`grass_medium_02`)

- **Use:** grass clump
- **Path:** `assets_src/models/vegetation/grass_medium_02/`
- **Source page:** https://polyhaven.com/a/grass_medium_02
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/grass_medium_02/grass_medium_02_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/grass_medium_02 under gltf.2k)
- **Author:** Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 1.8 MB
- **Resolution / polys:** 7,842 triangles in the glTF, 5 mesh nodes (grass_medium_02_a, grass_medium_02_b, grass_medium_02_c, grass_medium_02_d, grass_medium_02_e) · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Grass Medium 02" by Rico Cilliers, Poly Haven

### Island Tree 02 (`island_tree_02`)

- **Use:** deciduous tree
- **Path:** `assets_src/models/vegetation/island_tree_02/`
- **Source page:** https://polyhaven.com/a/island_tree_02
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/island_tree_02/island_tree_02_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/island_tree_02 under gltf.2k)
- **Author:** Rob Tuytel, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 59.2 MB
- **Resolution / polys:** 1,072,213 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Island Tree 02" by Rob Tuytel, Rico Cilliers, Poly Haven

### Pine Sapling Small (`pine_sapling_small`)

- **Use:** conifer (young pine)
- **Path:** `assets_src/models/vegetation/pine_sapling_small/`
- **Source page:** https://polyhaven.com/a/pine_sapling_small
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/pine_sapling_small/pine_sapling_small_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/pine_sapling_small under gltf.2k)
- **Author:** Rob Tuytel, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 32.2 MB
- **Resolution / polys:** 398,144 triangles in the glTF, 3 mesh nodes (pine_sapling_small_b, pine_sapling_small_a, pine_sapling_small_c) · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Pine Sapling Small" by Rob Tuytel, Rico Cilliers, Poly Haven

### Shrub 02 (`shrub_02`)

- **Use:** bush
- **Path:** `assets_src/models/vegetation/shrub_02/`
- **Source page:** https://polyhaven.com/a/shrub_02
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/shrub_02/shrub_02_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/shrub_02 under gltf.2k)
- **Author:** Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 4.8 MB
- **Resolution / polys:** 27,254 triangles in the glTF, 4 mesh nodes (shrub_02_a, shrub_02_b, shrub_02_c, shrub_02_d) · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Shrub 02" by Rico Cilliers, Poly Haven

### Shrub 04 (`shrub_04`)

- **Use:** bush
- **Path:** `assets_src/models/vegetation/shrub_04/`
- **Source page:** https://polyhaven.com/a/shrub_04
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/shrub_04/shrub_04_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/shrub_04 under gltf.2k)
- **Author:** Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 4.4 MB
- **Resolution / polys:** 27,327 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Shrub 04" by Rico Cilliers, Poly Haven

### Tree Stump 01 (`tree_stump_01`)

- **Use:** tree stump
- **Path:** `assets_src/models/vegetation/tree_stump_01/`
- **Source page:** https://polyhaven.com/a/tree_stump_01
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/tree_stump_01/tree_stump_01_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/tree_stump_01 under gltf.2k)
- **Author:** Rob Tuytel / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 11.4 MB
- **Resolution / polys:** 41,046 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Tree Stump 01" by Rob Tuytel, Poly Haven


## models/rocks

### Boulder 01 (`boulder_01`)

- **Use:** large boulder
- **Path:** `assets_src/models/rocks/boulder_01/`
- **Source page:** https://polyhaven.com/a/boulder_01
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/boulder_01/boulder_01_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/boulder_01 under gltf.2k)
- **Author:** Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 13.5 MB
- **Resolution / polys:** 66,122 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Boulder 01" by Rico Cilliers, Poly Haven

### Namaqualand Boulder 05 (`namaqualand_boulder_05`)

- **Use:** boulder
- **Path:** `assets_src/models/rocks/namaqualand_boulder_05/`
- **Source page:** https://polyhaven.com/a/namaqualand_boulder_05
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/namaqualand_boulder_05/namaqualand_boulder_05_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/namaqualand_boulder_05 under gltf.2k)
- **Author:** Jenelle van Heerden, Dario Barresi / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 11.9 MB
- **Resolution / polys:** 89,618 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Namaqualand Boulder 05" by Jenelle van Heerden, Dario Barresi, Poly Haven

### Rock 07 (`rock_07`)

- **Use:** rock
- **Path:** `assets_src/models/rocks/rock_07/`
- **Source page:** https://polyhaven.com/a/rock_07
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/rock_07/rock_07_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/rock_07 under gltf.2k)
- **Author:** Jenelle van Heerden / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 6.3 MB
- **Resolution / polys:** 14,844 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Rock 07" by Jenelle van Heerden, Poly Haven

### Rock 09 (`rock_09`)

- **Use:** rock
- **Path:** `assets_src/models/rocks/rock_09/`
- **Source page:** https://polyhaven.com/a/rock_09
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/rock_09/rock_09_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/rock_09 under gltf.2k)
- **Author:** Jenelle van Heerden / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 5.8 MB
- **Resolution / polys:** 12,416 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Rock 09" by Jenelle van Heerden, Poly Haven

### Rock Moss Set 01 (`rock_moss_set_01`)

- **Use:** mossy rock set
- **Path:** `assets_src/models/rocks/rock_moss_set_01/`
- **Source page:** https://polyhaven.com/a/rock_moss_set_01
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/rock_moss_set_01/rock_moss_set_01_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/rock_moss_set_01 under gltf.2k)
- **Author:** Kless Gyzen / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 3.3 MB
- **Resolution / polys:** 63,127 triangles in the glTF, 6 mesh nodes (rock_moss_set_01_rock01, rock_moss_set_01_rock02, rock_moss_set_01_rock03, rock_moss_set_01_rock04, rock_moss_set_01_rock05, rock_moss_set_01_rock06) · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Rock Moss Set 01" by Kless Gyzen, Poly Haven

### Rock Moss Set 02 (`rock_moss_set_02`)

- **Use:** mossy rock set
- **Path:** `assets_src/models/rocks/rock_moss_set_02/`
- **Source page:** https://polyhaven.com/a/rock_moss_set_02
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/rock_moss_set_02/rock_moss_set_02_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/rock_moss_set_02 under gltf.2k)
- **Author:** Kless Gyzen / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 3.6 MB
- **Resolution / polys:** 57,647 triangles in the glTF, 7 mesh nodes (rock_moss_set_02_rock08, rock_moss_set_02_rock09, rock_moss_set_02_rock12, rock_moss_set_02_rock13, rock_moss_set_02_rock10, rock_moss_set_02_rock11…) · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Rock Moss Set 02" by Kless Gyzen, Poly Haven

### Stone 01 (`stone_01`)

- **Use:** stone
- **Path:** `assets_src/models/rocks/stone_01/`
- **Source page:** https://polyhaven.com/a/stone_01
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/stone_01/stone_01_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/stone_01 under gltf.2k)
- **Author:** Dario Barresi, Rico Cilliers / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 10.1 MB
- **Resolution / polys:** 53,520 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Stone 01" by Dario Barresi, Rico Cilliers, Poly Haven


## models/props

### Concrete Road Barrier (`concrete_road_barrier`)

- **Use:** barrier
- **Path:** `assets_src/models/props/concrete_road_barrier/`
- **Source page:** https://polyhaven.com/a/concrete_road_barrier
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/concrete_road_barrier/concrete_road_barrier_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/concrete_road_barrier under gltf.2k)
- **Author:** Amal Kumar / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 11.0 MB
- **Resolution / polys:** 60,928 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Concrete Road Barrier" by Amal Kumar, Poly Haven

### Concrete Road Barrier 02 (`concrete_road_barrier_02`)

- **Use:** barrier
- **Path:** `assets_src/models/props/concrete_road_barrier_02/`
- **Source page:** https://polyhaven.com/a/concrete_road_barrier_02
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/concrete_road_barrier_02/concrete_road_barrier_02_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/concrete_road_barrier_02 under gltf.2k)
- **Author:** Amal Kumar / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 9.8 MB
- **Resolution / polys:** 23,822 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Concrete Road Barrier 02" by Amal Kumar, Poly Haven

### Hay bale (`oga_hay_bale`)

- **Use:** Hay bale. Low-poly COLLADA (.dae) with 256 px base colour/normal/AO PNGs: needs conversion to glTF (e.g. Blender) and probably re-texturing with `textures/props/reed_roof_03`.
- **Path:** `assets_src/models/props/oga_hay_bale/`
- **Source page:** https://opengameart.org/content/hay-bale
- **Download:** https://opengameart.org/sites/default/files/hay_bale.7z
- **Author:** carlosjorgereis / OpenGameArt
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 0.4 MB
- **Resolution / polys:** 308 tris, 256² PNG textures, .dae
- **Credit line:** Optional (CC0): "Hay bale" by carlosjorgereis, OpenGameArt

### Modular Electricity Poles (`modular_electricity_poles`)

- **Use:** roadside wooden poles
- **Path:** `assets_src/models/props/modular_electricity_poles/`
- **Source page:** https://polyhaven.com/a/modular_electricity_poles
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/modular_electricity_poles/modular_electricity_poles_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/modular_electricity_poles under gltf.2k)
- **Author:** James Ray Cock / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 25.9 MB
- **Resolution / polys:** 200,610 triangles in the glTF, 103 mesh nodes (bolt_large, bolt_small, cap, connection_large_01, connection_large_02, connection_large_offset_01…) · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Modular Electricity Poles" by James Ray Cock, Poly Haven

### Old fence (`oga_old_fence`)

- **Use:** Wooden fence module. Low-poly .dae, 256 px PNG maps: convert to glTF; can be re-textured with `textures/props/weathered_brown_planks`.
- **Path:** `assets_src/models/props/oga_old_fence/`
- **Source page:** https://opengameart.org/content/old-fence
- **Download:** https://opengameart.org/sites/default/files/old_fence.7z
- **Author:** carlosjorgereis / OpenGameArt
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 0.3 MB
- **Resolution / polys:** 138 tris, 256² PNG textures, .dae
- **Credit line:** Optional (CC0): "Old fence" by carlosjorgereis, OpenGameArt

### Old Tyre (`old_tyre`)

- **Use:** tyre (tyre-stack barriers)
- **Path:** `assets_src/models/props/old_tyre/`
- **Source page:** https://polyhaven.com/a/old_tyre
- **Download:** https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/old_tyre/old_tyre_2k.gltf (+ .bin and textures listed in https://api.polyhaven.com/files/old_tyre under gltf.2k)
- **Author:** MP / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Size:** 8.7 MB
- **Resolution / polys:** 2,880 triangles in the glTF · 2K JPG textures (diff/nor_gl/arm) · glTF 2.0
- **Credit line:** Optional (CC0): "Old Tyre" by MP, Poly Haven


## Added by the art stream (2026-10-28)

### Foliage alpha maps (Poly Haven, CC0)

Poly Haven's 2K JPG foliage textures carry no alpha (the leaf/needle cut-outs are in a separate map), so these were downloaded from the same API (`https://api.polyhaven.com/files/<id>`, key `Alpha` or `<part>_alpha`, 2K JPG) into each model's `textures/` folder:

| Model | File |
|---|---|
| shrub_02 | `shrub_02_alpha_2k.jpg` |
| shrub_04 | `shrub_04_alpha_2k.jpg` |
| fern_02 | `fern_02_alpha_2k.jpg` |
| grass_medium_01 | `grass_medium_01_alpha_2k.jpg` |
| grass_medium_02 | `grass_medium_02_alpha_2k.jpg` |
| pine_sapling_small | `pine_sapling_small_twig_alpha_2k.jpg` |
| fir_sapling | `fir_sapling_twigs_alpha_2k.jpg` |
| island_tree_02 | `island_tree_02_leaves_alpha_2k.jpg` |

Same authors and licence as the models above. ~3.3 MB in total.

### What the game ships (assets/, built by tools/art/)

- `assets/textures/<set>/`: cooked KTX2 (engine cook) from the ground textures, the rock/plant model textures, the `pine_bark` trunk texture, tree card atlases rendered in Blender from `fir_tree_01`, `pine_tree_01` and `tree_small_02` (`tools/art/render_tree_cards.py`) and `island_tree_01` (scrub tree; `tools/art/prep_textures.sh`).
- `assets/models/*.pnm`: decimated rocks, ferns, grass and a stump (`tools/art/build_meshes.py`); the bushes `shrub_a`/`shrub_b` are clusters of leaf-clump cards cut from `tree_small_02` (texture set `m_shrub`, `tools/art/build_bushes.py`), since Poly Haven's `shrub_02` is a sparse weed rather than a bush.
- `assets/models/rallycar.glb`: the Talus rally hatch, original, made for Pacenote: built procedurally by `tools/art/build_rallycar.py` (with `cockpit.py`, `driver.py`), no third-party content (design sheet `docs/cars/rally_hatch.md`).
- Credits: `assets/credits.txt`.

## audio_car (car sounds, added 2026-11-02)

Recordings for the car: engine, tyres, impacts, scrape, pops. All **CC0** (Creative Commons 0, https://creativecommons.org/publicdomain/zero/1.0/), checked on each Freesound page on 2026-11-02; no credit is required. Downloaded as Freesound's HQ Ogg previews (96 or 44.1 kHz, ~190 kbps; no login needed for CC0 sounds) from `https://cdn.freesound.org/previews/<id/1000>/<id>_<user id>-hq.ogg` into `assets_src/audio_car/freesound/<id>_<name>.ogg` (`meta.tsv` there lists every download with its page and file URL). 25 MB on disk, including candidates that were not used.

Cooked by `tools/audio/cook_car.py` into `assets/sounds/car/*.ogg` (mono, 48 kHz, Ogg Vorbis via `oggenc`, loudness-normalised, loops crossfaded into themselves so they wrap seamlessly): 63 files, ~1 MB.

| Shipped file(s) | Source | Author | Part used |
|---|---|---|---|
| `engine_idle_{bay,exh}` | [21_500_4062.wav](https://freesound.org/people/Chippy569/sounds/162516/), [21_500_D6.wav](https://freesound.org/people/Chippy569/sounds/162515/) | Chippy569 | 2.6 s of whole firings (measured ~1006 rpm, no load) |
| `engine_off_2431_{bay,exh}` | [21_2500_4062.wav](https://freesound.org/people/Chippy569/sounds/162519/), [21_2500_D6.wav](https://freesound.org/people/Chippy569/sounds/162518/) | Chippy569 | 2.6 s (~2431 rpm, no load) |
| `engine_off_6297_{bay,exh}` | [21_6500_4062.wav](https://freesound.org/people/Chippy569/sounds/162522/), [21_6500_D6.wav](https://freesound.org/people/Chippy569/sounds/162521/) | Chippy569 | whole loop (~6297 rpm, no load) |
| `engine_on_2556_{bay,exh}` | [19_2500_4062.wav](https://freesound.org/people/Chippy569/sounds/162510/), [19_2500_D6.wav](https://freesound.org/people/Chippy569/sounds/162509/) | Chippy569 | 2.6 s (~2556 rpm, full load) |
| `engine_on_6208_{bay,exh}` | [20_6500_4062.wav](https://freesound.org/people/Chippy569/sounds/162513/), [20_6500_D6.wav](https://freesound.org/people/Chippy569/sounds/162512/) | Chippy569 | whole loop (~6208 rpm, full load) |
| `engine_on_<rpm>_*` (10 more: 1450–7200), `engine_off_<rpm>_*` (11 more: 1166–7300) | the recordings above | Chippy569 | re-synthesised at cook time by pitch-synchronous overlap-add from the two recordings of the same load that bracket the rpm (idle at the bottom of both) |
| `gravel_slide` | [S36-15 Car skids; dirt.wav](https://freesound.org/people/craigsmith/sounds/675282/) | craigsmith | the bodies of nine skids, joined |
| `tarmac_squeal` | [Chrysler LHS tire squeal 03](https://freesound.org/people/audible-edge/sounds/71738/) + [AE0090 Volvo 740 GLE handbrake turn 01](https://freesound.org/people/audible-edge/sounds/76804/) | audible-edge | 6.0–9.3 s + 7.55–8.95 s, high-passed 350 Hz |
| `scrape` | [MetalScrape_6.wav](https://freesound.org/people/deerlord/sounds/534851/) | deerlord | 12–18 s |
| `impact_heavy` | [S36-09 Metal foley for car crash; much crunching](https://freesound.org/people/craigsmith/sounds/675475/) | craigsmith | 0.10–0.95 s |
| `impact_light` | [Clank Car Crash Collision](https://freesound.org/people/qubodup/sounds/151624/) | qubodup | whole |
| `snap` | [Crack of branch 3.wav](https://freesound.org/people/deleted_user_2104797/sounds/164472/) | (account since deleted) | 0.04–0.59 s |
| `pop1`, `pop2`, `pop3` | [Car Antilag (No cleanup, 32float)](https://freesound.org/people/modusmogulus/sounds/797835/) | modusmogulus | three bangs at 1.57, 2.17 and 5.27 s |

About the engine: a Mercedes-Benz 190E 2.3-16V (2.3 l 16-valve 4-cylinder) on a dyno, recorded for "the Kickstarter Dyno Session" with three mics at once: a DPA 4062 on the radiator support above the headlight (`_4062`, shipped as `_bay`), an Audix D6 a foot behind the exhaust (`_D6`, shipped as `_exh`) and a Rode NT1a 30 ft in front (`_Distant`, downloaded, not used). The files named "500" are ~1000 rpm (no load) and ~2580 rpm (full load) by their firing frequency; the full-load "500" set and the sweeps (`162499`–`162504`) were downloaded but not used. Checked again on 2026-11-02 (all CC0): the session's steady recordings are only the 500/2500/6500 sets; the sweeps are a 4.3 s and a 2.4 s acceleration pull (engine bay and exhaust mic from different takes, ~1000–3700 rpm) and two "deceleration" sweeps under load (6500→1350 rpm in ~4 s on the bay mic; 3450→1300 rpm in ~8 s on the exhaust, another take). Too fast and too mismatched for steady loops, so the extra rpm points are re-synthesised from the steady sets instead (`tools/audio/cook_car.py`). The recording is of an engine, not a branded product: nothing in the shipped sound identifies the car.

Downloaded and not used (all CC0): Chippy569's distant-mic loops and sweeps; TRP 577504; felix.blume 685233; loganzsound 784177; kyles 637192, 637161; Marschal 718299; orlandorizo 591079; audible-edge 71736; craigsmith 675286; mwchristian95 725428; qubodup 332056, 332058; LPA134 329516; Logicogonist 807437; squareal 237375; P%C3%B3l 385935; CeebFrack 105351; conicity 697837; alegemaate 364699; selfdestructbutton 569571 (a GarageBand piece, not a recording).

## voice_piper (co-driver phrase bank, added 2026-11-02)

The co-driver's voice: 136 clips (the six sim-rally note words added 2026-11-03 with `--only`) (numbers, pace-note words, damage and recovery lines), rendered once at build time by `tools/audio/cook_voice.py` with Piper into `assets/sounds/voice/*.ogg` (mono 24 kHz Ogg Vorbis, silence trimmed, levelled; `bank.txt` lists the words each clip says), ~1 MB. Model downloaded into `assets_src/voice_piper/` (114 MB, not shipped).

### Piper voice en_GB-cori-high (`piper_cori`)
- **Source page:** https://huggingface.co/rhasspy/piper-voices/tree/main/en/en_GB/cori/high
- **Author:** Bryce Beattie (voice model; trained from scratch on public-domain LibriVox recordings, ~24 h of one reader)
- **Licence:** Public domain (model card: dataset "public domain"; the author's page https://brycebeattie.com/files/tts/ lists Cori as public domain: "Feel free to use these for any legal and ethical purpose"). Checked 2026-11-02.
- **Used for:** `assets/sounds/voice/*.ogg` (the co-driver's phrase bank).
- Notes: picked because it is trained from scratch. Most other English Piper voices are fine-tuned from `en_US-lessac` (Blizzard 2013 data, non-commercial) or trained on CC BY-NC data (`ryan`, `hfc_male`), so they were not used. The Piper program (OHF-Voice/piper1-gpl, GPL-3.0) only runs at build time; its output is not covered by its licence. `norman` (US male, also public domain, from scratch) is the alternative if a male voice is wanted.

## Not found / not downloaded

### Car (rally hatchback): solved with an original procedural model; the options below were the download candidates
No realistic, unbranded hatchback in glTF is downloadable without an account under CC0/CC BY. Poly Haven has no cars (only `covered_car`, a car under a tarp). Khronos samples only have Car Concept (above) and the toy-like ToyCar. Sketchfab's download API returns 401 without login. These CC BY 4.0 models are downloadable as glTF from Sketchfab with a free account (licence read from the Sketchfab API on 2026-10-28; re-check the page before use):

| Model | Author | Faces | Why |
|---|---|---|---|
| [Milano '95 - Low poly model](https://sketchfab.com/3d-models/milano-95-low-poly-model-122a22d210234596ad23623588d0d166) | Daniel Zhabotinsky | 18k | Fictional mid-90s European compact hatchback, unbranded. The best fit for a generic rally hatchback. Mid-poly, game-ready. |
| [Kiri '86 - Low poly model](https://sketchfab.com/3d-models/kiri-86-low-poly-model-5c17c582ef9549798f694b660feea42a) | Daniel Zhabotinsky | 20k | Fictional 80s Japanese compact sedan. Same author and style; the other cars in his series (Shvan '92, Sanremo '70, Asti Stradale '89, Ace '11) are fictional too. |
| [Generic Toyama XR Sedan](https://sketchfab.com/3d-models/generic-toyama-xr-sedan-a161542c38ac44fca3b1c283f6d986bb) | mk2design | 25k | Described as a "generic rally sedan". The author asks for credit. |
| [Generic passenger car pack](https://sketchfab.com/3d-models/generic-passenger-car-pack-20f9af9b8a404d5cb022ac6fe87f21f5) | Comrade1280 | 69k (10 cars, ~7k tris each) | Includes a "hatchback" and a "compact". Described as trademark-free. Low-poly: good for AI/traffic or LODs. |
| [Ariel_ hatchback V2](https://sketchfab.com/3d-models/ariel--hatchback-v2-52c7d1106dff4c7dbce94ce5f0a7a0fb) | orcha | 108k | Sporty hatchback with roll cage, opening doors, rig-ready. **Caution:** "Ariel" is a real UK car maker's name, so rename it. Check that the body doesn't copy a real car. |

Rejected because they are real branded cars: Lancia Delta HF Integrale, Ford Puma Rally1, Toyota GR Yaris Rally, Hyundai Getz, VW Golf Mk2, Honda Civic Type R, Fiat Punto, Mini Cooper S, Subaru Impreza, BMW E30 (all CC BY on Sketchfab, but they are recognisable trademarks/trade dress).

When downloading from Sketchfab, pick "glTF" (or "Autoconverted format (glTF)"), and record the credit line Sketchfab shows ("<title> by <author> is licensed under CC BY 4.0").

### Other gaps
- **Birch trees:** no CC0/CC BY realistic birch was found as a direct download. Since 2026-11-01 the birch crowns come from `tree_small_02` (slender broadleaf, rendered without its trunk) over a white trunk made in code; the olive-like scrub tree comes from `island_tree_01` (`island_tree_02` is no longer used).
- **Large conifers:** solved 2026-11-01: `fir_tree_01` and `pine_tree_01` (above) are downloaded and rendered into card atlases (whole tree + three branch slices) by `tools/art/render_tree_cards.py`; the game never loads their geometry. The saplings (`fir_sapling`, `pine_sapling_small`) are no longer used for cards.
- **Road sign post:** no CC0/CC BY generic sign model was found (OpenGameArt and RenderHub results were paid or off-topic). Build one procedurally: a pole (a `modular_electricity_poles` part or a cylinder) plus a quad with an original sign texture.
- **Wooden fence / hay bales:** only the low-poly OpenGameArt models above. Simple procedural boxes/cylinders with `weathered_brown_planks` and `reed_roof_03` are likely to look better.
- **Barrier:** only concrete barriers and a tyre (`old_tyre`, usable for tyre-stack barriers). There are no tape or plastic rally barriers.
- **Gravel spray texture:** none found. Use the alpha'd Kenney puffs tinted brown plus small instanced stones (`stone_01` decimated) or a quad cut from `rocky_gravel`.

### Notes for import
- All Poly Haven glTFs reference their own `textures/` subfolder; keep each folder intact. Normal maps are OpenGL convention (`nor_gl`), which Bevy expects. `arm` = AO (R), roughness (G), metal (B): Bevy's occlusion + metallic-roughness layout.
- Triangle counts above are counted from the downloaded glTFs (all variants in the file summed). Multi-node models (e.g. `fir_sapling_a/b/c`) hold several variants: spawn one node each. No LODs are included; generate them (meshopt/Blender).
- The ambientCG `Displacement` / Poly Haven `disp` maps are heightmaps for terrain blending or parallax.

## Fab (Quixel Megascans): Fab Standard License

Downloaded 7 Nov 2026 by the owner from fab.com (free, Professional tier). The owner allowed Fab Standard
assets that work in any engine on 7 Nov 2026 (docs/ASSETS_BRIEF.md §0). Fab Standard: no credit required;
raw files must not be redistributed, so only cooked KTX2 / .pnm files ship. Folder list and per-item notes:
`assets_src/fab_batch1/SOURCES.md`. Fab lists several items per title, so each is identified by its
Megascans asset id (in the folder name and the bundled JSON); search the id on fab.com to find the listing.

### Gravel on Soil (`fab_gravel_on_soil`)

- **Use:** road gravel (`gravel`: forest, birch, moor and open-world roads)
- **Path:** `assets_src/fab_batch1/gravel_on_soil_rmsiidp0_4k_ue_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `rmsiidp0`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K (8K for Layered Rock) PBR scan, shipped at 1024²
- **Credit line:** Not required (Fab Standard)

### Soil and Gravel (`fab_soil_and_gravel`)

- **Use:** ochre gravel road (`gravel_red`: Scrub roads)
- **Path:** `assets_src/fab_batch1/soil_and_gravel_pcciq0_4k/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `pcciQ0`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K (8K for Layered Rock) PBR scan, shipped at 1024²
- **Credit line:** Not required (Fab Standard)

### Dirt Road (`fab_dirt_road`)

- **Use:** packed dirt verge (`dirt_road`: Scrub verges)
- **Path:** `assets_src/fab_batch1/dirt_road_omdg10_4k/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `omDg10`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K (8K for Layered Rock) PBR scan, shipped at 1024²
- **Credit line:** Not required (Fab Standard)

### Mud Tire Tracks (`fab_mud_tire_tracks`)

- **Use:** mud road surface (`mud`)
- **Path:** `assets_src/fab_batch1/mud_tire_tracks_wcksce0_4k_ue_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `wcksce0`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K (8K for Layered Rock) PBR scan, shipped at 1024²
- **Credit line:** Not required (Fab Standard)

### Layered Rock (`fab_layered_rock`)

- **Use:** terrain rock layer (`rock`)
- **Path:** `assets_src/fab_batch1/layered_rock_vl0fdhdo_8k/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `vl0fdhdo`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K (8K for Layered Rock) PBR scan, shipped at 1024²
- **Credit line:** Not required (Fab Standard)

### Dry Soil Surface (`fab_dry_soil_surface`)

- **Use:** dry ground (`soil_dry`: Scrub and Moor base layer)
- **Path:** `assets_src/fab_batch1/dry_soil_surface_sbkiznp0_4k_ue_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `sbkiznp0`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K (8K for Layered Rock) PBR scan, shipped at 1024²
- **Credit line:** Not required (Fab Standard)

### Arid Soil Ground (`fab_arid_soil_ground`)

- **Use:** red earth (`soil_red`: Scrub patches)
- **Path:** `assets_src/fab_batch1/arid_soil_ground_wihgahybw_4k_ue_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `wihgahybw`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K (8K for Layered Rock) PBR scan, shipped at 1024²
- **Credit line:** Not required (Fab Standard)

### Cracked Soil (`fab_cracked_soil`)

- **Use:** cracked dry ground (`soil_crack`: Scrub patches)
- **Path:** `assets_src/fab_batch1/cracked_soil_rmgfzqp0_4k_ue_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `rmgfzqp0`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K (8K for Layered Rock) PBR scan, shipped at 1024²
- **Credit line:** Not required (Fab Standard)

### Coast Rauk Boulder L 03 (`fab_coast_rauk_boulder`)

- **Use:** granite boulder (`boulder_rauk`)
- **Path:** `assets_src/fab_batch1/coast_rauk_boulder_l_03_wfepegb_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `wfepegb`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K scan, decimated by tools/art/fab_to_gltf.py + build_meshes.py; textures at 1024²
- **Credit line:** Not required (Fab Standard)

### Dead Shrubs (`fab_dead_shrubs`)

- **Use:** dead shrubs (`dead_shrub_a/b`; Scrub ground cover and roadside shrubs)
- **Path:** `assets_src/fab_batch1/dead_shrubs_vdknafgha_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `vdknafgha`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K scan, decimated by tools/art/fab_to_gltf.py + build_meshes.py; textures at 1024²
- **Credit line:** Not required (Fab Standard)

### Dry Branches (`fab_dry_branches`)

- **Use:** branch pile (`dry_branches`)
- **Path:** `assets_src/fab_batch1/dry_branches_pdye3_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `pdye3`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K scan, decimated by tools/art/fab_to_gltf.py + build_meshes.py; textures at 1024²
- **Credit line:** Not required (Fab Standard)

### Dead Branches (`fab_dead_branches`)

- **Use:** branch pile (`dead_branches`)
- **Path:** `assets_src/fab_batch1/dead_branches_qjuwd_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `qjuwd`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K scan, decimated by tools/art/fab_to_gltf.py + build_meshes.py; textures at 1024²
- **Credit line:** Not required (Fab Standard)

### Dry Grass (`fab_dry_grass`)

- **Use:** dry grass tufts (`dry_grass_a/b`)
- **Path:** `assets_src/fab_batch1/dry_grass_tbbqejqr_ue_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `tbbqejqr`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K scan, decimated by tools/art/fab_to_gltf.py + build_meshes.py; textures at 1024²
- **Credit line:** Not required (Fab Standard)

### Cocksfoot Grass (`fab_cocksfoot_grass`)

- **Use:** seed-head grass (`cocksfoot`)
- **Path:** `assets_src/fab_batch1/cocksfoot_grass_tcfoejcr_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `tcfoejcr`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K scan, decimated by tools/art/fab_to_gltf.py + build_meshes.py; textures at 1024²
- **Credit line:** Not required (Fab Standard)

### Kikuyu Grass (`fab_kikuyu_grass`)

- **Use:** grass clump (`kikuyu`)
- **Path:** `assets_src/fab_batch1/kikuyu_grass_wiljearqx_ue_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `wiljearqx`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K scan, decimated by tools/art/fab_to_gltf.py + build_meshes.py; textures at 1024²
- **Credit line:** Not required (Fab Standard)

### Concrete Barrier (`fab_concrete_barrier`)

- **Use:** concrete barrier block (`barrier_concrete`; stage hairpins)
- **Path:** `assets_src/fab_batch1/concrete_barrier_ubitfhtfa_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `ubitfhtfa`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K scan, decimated by tools/art/fab_to_gltf.py + build_meshes.py; textures at 1024²
- **Credit line:** Not required (Fab Standard)

### Graminea 01 - Tall Grass (`fab_graminea`)

- **Use:** tall grass clumps (`graminea`; Moor)
- **Path:** `assets_src/fab_batch1/graminea01_additional_files/`
- **Source page:** https://www.fab.com/listings/6d44f8b5-2759-4cc3-b89c-6d269c2aca5f
- **Author:** Neon Grid
- **Licence:** Fab Standard License (free Professional tier; any engine; no raw redistribution). Tagged NoAI on Fab: not for AI training or texture tools.
- **Resolution / polys:** 2K atlas; one card split into three crossed-card tufts by tools/art/fab_to_gltf.py
- **Credit line:** Not required (Fab Standard)

### Hay Bale (`fab_hay_bale`)

- **Use:** straw colours for the in-house hay bale texture (`m_hay`, painted in asset-forge/assets_out/HayBale/build_haybale.py)
- **Path:** `assets_src/fab_batch1/hay_bale_tlvmcfvl_4k_ue_high/`
- **Source page:** https://www.fab.com/sellers/Quixel%20Megascans (Megascans asset id `tlvmcfvl`)
- **Author:** Quixel Megascans
- **Licence:** Fab Standard License (free; any engine; no raw redistribution)
- **Resolution / polys:** 4K texture set; sampled into a 1024² in-house atlas
- **Credit line:** Not required (Fab Standard)

### Models of people x 10 01 (`fab_people_x10_01`)

- **Use:** seated spectator impostor cards (`card_crowd`, tools/art/render_crowd_cards.py)
- **Path:** `assets_src/fab_batch1/blend/`
- **Source page:** https://www.fab.com/listings/d374612c-c07f-4825-8688-4b57da7522f2
- **Author:** Volandemor0026
- **Licence:** Fab Standard License, claimed by the owner on 7 Nov 2026. **TO CONFIRM before release:** on 8 Nov the listing showed a price on both tiers (Personal 468.93 INR, Professional 938.8 INR), so it was probably a limited-time free claim; check in the owner's Fab library which licence tier was claimed (Professional covers commercial use without limits).
- **Resolution / polys:** 10 seated scans (~86k tris each), rendered to a 2048² 8-angle atlas
- **Credit line:** Not required (Fab Standard)

### Bark Platanus (`ph_bark_platanus`)

- **Use:** bark micro-normal in the in-house eucalyptus bark (`bark_gum`)
- **Path:** `assets_src/realism_batch1/bark_platanus/`
- **Source page:** https://polyhaven.com/a/bark_platanus
- **Author:** Dimitrios Savva / Poly Haven
- **Licence:** CC0 1.0 Universal (Public Domain Dedication)
- **Resolution / polys:** 2K texture set
- **Credit line:** Optional (CC0)

### Yale Bright Star Catalogue, 5th ed. (`yale_bsc5`)

- **Use:** the night sky's naked-eye stars (positions, magnitudes, colours): `assets/sky/stars.bin` (tools/art/bake_sky.py)
- **Path:** `assets_src/night/bsc5/heasarc_bsc5p.tdat.gz`
- **Source page:** https://heasarc.gsfc.nasa.gov/W3Browse/star-catalog/bsc5p.html
- **Author:** D. Hoffleit, W. H. Warren Jr. (Yale University Observatory, 1991); NASA HEASARC distribution
- **Licence:** Public catalogue data (HEASARC table security: public); factual star data, free to use with acknowledgement
- **Resolution / polys:** 9,110 stars; 8,404 to magnitude 6.5 baked
- **Credit line:** "Star positions: Yale Bright Star Catalogue (Hoffleit & Warren 1991), via NASA HEASARC"

### CGI Moon Kit (`nasa_cgi_moon_kit`)

- **Use:** the moon's albedo map: `assets/sky/moon.bin` (resized to 512 × 256)
- **Path:** `assets_src/night/moon/lroc_color_poles_2k.tif`
- **Source page:** https://svs.gsfc.nasa.gov/4720
- **Author:** NASA's Scientific Visualization Studio (Ernie Wright); LRO LROC data, NASA/GSFC/Arizona State University
- **Licence:** Public domain (NASA imagery, not copyrighted; NASA media guidelines)
- **Resolution / polys:** 2048 × 1024 colour mosaic
- **Credit line:** "Moon: NASA's Scientific Visualization Studio (LRO)"

### Chevrolet Chevelle SS 1970 (`sf_chevelle_ss_1970`)

- **Use:** the Brawn 70 body (`assets/models/car_brawn.glb`): debadged look-alike (owner's ruling 8 Nov 2026): every emblem, script and number plate removed, the body repainted plain `Paint`, our name
- **Path:** `asset-forge/refs/_cars_eval/src/chevrolet-chevelle-ss-1970/` (from ~/Downloads/cars)
- **Source page:** https://sketchfab.com/3d-models/chevrolet-chevelle-ss-1970-ddcba6667cce46ad92545fc38beda4ed
- **Author:** mohamedhussien (Sketchfab)
- **Licence:** CC BY 4.0 (credit required). Provenance note: the FBX records an "E3D" source folder (often an Element 3D commercial pack); the owner chose to ship it on 8 Nov 2026
- **Resolution / polys:** 892k faces, converted to 57.7k triangles by tools/art/import_sketchfab.py --profile brawn
- **Credit line:** "Chevrolet Chevelle SS 1970" by mohamedhussien, CC BY 4.0, https://sketchfab.com/3d-models/chevrolet-chevelle-ss-1970-ddcba6667cce46ad92545fc38beda4ed (modified: debadged, repainted, rigged)

### Original concept cars (`original_concept_cars`)

- **Use:** the Lynx 1600, Kite Coupe and Tarn 4WD bodies (`assets/models/car_{lynx,kite,tarn}.glb`): original designs, no real car's name, badge or livery
- **Path:** `assets_src/concept_cars/`: the picked concept images (`<id>_<seed>.png`), their rear views (`_rear.png`) and the refined bodies (`_body.glb`, the import's input)
- **Source page:** n/a (made for Pacenote; recipe below)
- **Author:** Pacenote (generated and converted in-house)
- **Made with:** concept images by Qwen-Image 2.1 Turbo (Apache-2.0, local via mflux; `tools/art/concept_cars.py`, fixed seeds); shapes by TripoSG (VAST-AI, local; `asset-forge/tools/triposg_run.py`); materials from the concept images (`asset-forge/tools/image3d_project.py`); clean-up, real dimensions and new wheels (`asset-forge/tools/image3d_refine.py`, Blender); rig and cockpit by `tools/art/import_sketchfab.py --profile <id>`
- **Licence:** ours. **Caveat (TripoSG):** its DiT core and package carry Tencent Hunyuan community licences whose territory excludes the EU, the UK and South Korea and which reach generated outputs (see `image3d/triposg/THIRD_PARTY_LICENSES.md`). Before shipping in those regions, regenerate the shapes with a model without that limit or remodel them by hand.
- **Resolution / polys:** 0.57–1.28M-face raw meshes, refined to ~67k faces, about 64–67k triangles per car after the import
- **Credit line:** none required

<!-- SFX BEGIN (tools/audio/cook_sfx.py) -->
## audio/openverse

Downloaded with `tools/audio/openverse_sounds.py` (packs in `tools/audio/sfx_wishlist.json`), cooked by `tools/audio/cook_sfx.py` into `assets/sounds/sfx/`. Only CC0, PDM and CC BY are cooked.

- `car_horn/2_Car_Horns_273272fd.mp3`: "2 Car Horns" by alukahn (BY 3.0, freesound): https://freesound.org/people/alukahn/sounds/144137 (downloaded 2026-11-05)
- `car_horn/Car_Horn_Beepsmp3_eab7cb51.mp3`: "Car Horn Beeps.mp3" by hsaunier10 (CC0 1.0, freesound): https://freesound.org/people/hsaunier10/sounds/254678 (downloaded 2026-11-05)
- `car_horn/My_Car_Horn_-_Beep_Beepwav_9680f167.mp3`: "My Car Horn - Beep Beep.wav" by fogma (BY 3.0, freesound): https://freesound.org/people/fogma/sounds/26185 (downloaded 2026-11-05)
- `dog_bark/A_dog_barking_close_recording_f3344937.mp3`: "A dog barking (close recording)" by felix.blume (CC0 1.0, freesound): https://freesound.org/people/felix.blume/sounds/199261 (downloaded 2026-11-05)
- `dog_bark/Small_Dog_Bark_01_d5dcb31d.mp3`: "Small Dog Bark 01" by noctaro (BY 4.0, freesound): https://freesound.org/people/noctaro/sounds/242414 (downloaded 2026-11-05)
- `gravel_hit/Gravel_Impacts_and_Fallswav_3bb6fd08.mp3`: "Gravel Impacts and Falls.wav" by lolamadeus (CC0 1.0, freesound): https://freesound.org/people/lolamadeus/sounds/179341 (downloaded 2026-11-05)
- `gravel_hit/Gravel_impact_3_acb293ba.mp3`: "Gravel impact 3" by jorickhoofd (BY 4.0, freesound): https://freesound.org/people/jorickhoofd/sounds/169551 (downloaded 2026-11-05)
- `service_park/Auto_Repair_Shop_with_Pneumatic_Drill_Sounds_562bf3af.mp3`: "Auto Repair Shop with Pneumatic Drill Sounds" by ericreinert (BY 3.0, freesound): https://freesound.org/people/ericreinert/sounds/120227 (downloaded 2026-11-05)
- `service_park/CarAuto_Mechanic_Shop_93b4133b.mp3`: "Car/Auto Mechanic Shop" by producerdan (CC0 1.0, freesound): https://freesound.org/people/producerdan/sounds/266626 (downloaded 2026-11-05)
<!-- SFX END -->
