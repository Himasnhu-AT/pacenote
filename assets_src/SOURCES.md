# Pacenote: third-party asset sources

Every file under `assets_src/` is listed here. Only CC0 and CC BY licences are used. No asset comes from or imitates a commercial game. Downloaded 2026-10-28. Poly Haven files come from the public API (`https://api.polyhaven.com/files/<id>`, file URLs under `https://dl.polyhaven.org/file/ph-assets/...`); ambientCG files from `https://ambientcg.com/get?file=<id>_2K-JPG.zip` (only the images were kept: the NormalDX map and the bundled .blend were deleted, the .png is ambientCG's preview).

**Total on disk: 694.6 MB** across 54 assets.

## Credits that must appear in the game (CC BY)

- None at the moment. (The CC BY Car Concept below is no longer shipped: the game's car is an original procedural model, `tools/art/build_rallycar.py`. Restore its credit line if it is ever used again.)

CC0 assets need no credit; a courtesy line is suggested: "Textures, HDRIs and models from Poly Haven (polyhaven.com) and ambientCG (ambientcg.com), CC0. Smoke sprites by Kenney (kenney.nl), CC0. Hay bale and fence by carlosjorgereis (OpenGameArt), CC0."


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

- `assets/textures/<set>/`: cooked KTX2 (engine cook) from the ground textures, the rock/plant model textures and foliage cards rendered in Blender from `fir_sapling`, `pine_sapling_small` and `island_tree_02` (`tools/art/prep_textures.sh`, `tools/art/render_cards.py`).
- `assets/models/*.pnm`: decimated rocks, shrubs, ferns, grass and a stump (`tools/art/build_meshes.py`).
- `assets/models/rallycar.glb`: original procedural rally hatchback with livery and cockpit (`tools/art/build_rallycar.py`); no third-party content.
- Credits: `assets/credits.txt`.

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
- **Birch trees:** no CC0/CC BY realistic birch was found as a direct download. `island_tree_02` is the deciduous stand-in.
- **Large conifers:** Poly Haven's full-size `fir_tree_01` (7.9M tris, 510 MB at 2K) and `pine_tree_01` (17.4M tris, 982 MB) were skipped for size. They are only usable if decimated in Blender. The saplings that were downloaded are also heavy (see their triangle counts; each file holds several variants) and need decimation/LODs or impostors. `textures/foliage/PineNeedles001` (needle atlas with opacity) is there for card-based LOD trees.
- **Road sign post:** no CC0/CC BY generic sign model was found (OpenGameArt and RenderHub results were paid or off-topic). Build one procedurally: a pole (a `modular_electricity_poles` part or a cylinder) plus a quad with an original sign texture.
- **Wooden fence / hay bales:** only the low-poly OpenGameArt models above. Simple procedural boxes/cylinders with `weathered_brown_planks` and `reed_roof_03` are likely to look better.
- **Barrier:** only concrete barriers and a tyre (`old_tyre`, usable for tyre-stack barriers). There are no tape or plastic rally barriers.
- **Gravel spray texture:** none found. Use the alpha'd Kenney puffs tinted brown plus small instanced stones (`stone_01` decimated) or a quad cut from `rocky_gravel`.

### Notes for import
- All Poly Haven glTFs reference their own `textures/` subfolder; keep each folder intact. Normal maps are OpenGL convention (`nor_gl`), which Bevy expects. `arm` = AO (R), roughness (G), metal (B): Bevy's occlusion + metallic-roughness layout.
- Triangle counts above are counted from the downloaded glTFs (all variants in the file summed). Multi-node models (e.g. `fir_sapling_a/b/c`) hold several variants: spawn one node each. No LODs are included; generate them (meshopt/Blender).
- The ambientCG `Displacement` / Poly Haven `disp` maps are heightmaps for terrain blending or parallax.
