# Decimates the Poly Haven rocks / ferns / grass / stump models (the shrubs come from
# tools/art/build_bushes.py) and writes them
# as small binary meshes the game reads at load (src/art/meshfile.rs):
#   blender -b --factory-startup -P tools/art/build_meshes.py -- <assets_src> <assets/models>
# Format ("PNM1"): u32 vertex count, u32 index count, then f32 positions (xyz), normals (xyz),
# uvs (uv), u32 indices; little endian. Coordinates are Bevy's (Y up), metres; the pivot is
# centred in X/Z at the model's base (y = 0 at its lowest point + `sink` of its height).
import os
import struct
import sys

import bmesh
import bpy
import numpy as np

argv = sys.argv[sys.argv.index("--") + 1:]
SRC, OUT = argv[0], argv[1]

# out name, folder (under models/), gltf stem, object, target triangles, uniform scale, sink
ITEMS = [
    ("boulder", "rocks/boulder_01", "boulder_01", "boulder_01", 1600, 1.0, 0.12),
    ("rock_moss_a", "rocks/rock_moss_set_01", "rock_moss_set_01", "rock_moss_set_01_rock02", 1400, 0.7, 0.1),
    ("rock_moss_b", "rocks/rock_moss_set_01", "rock_moss_set_01", "rock_moss_set_01_rock04", 1400, 0.7, 0.1),
    ("rock_moss_c", "rocks/rock_moss_set_02", "rock_moss_set_02", "rock_moss_set_02_rock13", 1200, 0.9, 0.08),
    ("rock_moss_d", "rocks/rock_moss_set_02", "rock_moss_set_02", "rock_moss_set_02_rock10", 1200, 0.9, 0.08),
    ("rock_dry", "rocks/namaqualand_boulder_05", "namaqualand_boulder_05", "namaqualand_boulder_05", 1400, 1.6, 0.1),
    ("fern_a", "vegetation/fern_02", "fern_02", "fern_02_b", 600, 1.1, 0.0),
    ("fern_b", "vegetation/fern_02", "fern_02", "fern_02_c", 600, 1.1, 0.0),
    ("grass_a", "vegetation/grass_medium_01", "grass_medium_01", "grass_medium_01_mid_c_LOD0", 700, 2.6, 0.0),
    ("grass_b", "vegetation/grass_medium_01", "grass_medium_01", "grass_medium_01_tall_a_LOD0", 400, 2.6, 0.0),
    ("grass_c", "vegetation/grass_medium_02", "grass_medium_02", "grass_medium_02_c", 600, 2.4, 0.0),
    ("stump", "vegetation/tree_stump_01", "tree_stump_01", "tree_stump_01", 1200, 1.0, 0.05),
]


def tri_count(me):
    return sum(len(p.vertices) - 2 for p in me.polygons)


def export(name, folder, stem, obj_name, target, scale, sink):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=os.path.join(SRC, "models", folder, f"{stem}_2k.gltf"))
    obj = bpy.data.objects[obj_name]
    for o in list(bpy.data.objects):
        if o != obj:
            bpy.data.objects.remove(o, do_unlink=True)
    obj.parent = None
    obj.location = (0, 0, 0)
    obj.rotation_euler = (0, 0, 0)
    obj.scale = (scale, scale, scale)
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    before = tri_count(obj.data)
    # Merge split vertices first (scans often come with every face separate) so the collapse
    # decimation can work across them; UVs stay per corner.
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bmesh.ops.remove_doubles(bm, verts=bm.verts[:], dist=0.0005)
    bm.to_mesh(obj.data)
    bm.free()
    if before > target:
        mod = obj.modifiers.new("dec", "DECIMATE")
        mod.decimate_type = "COLLAPSE"
        mod.ratio = target / before
        mod.use_collapse_triangulate = True
        bpy.ops.object.modifier_apply(modifier=mod.name)
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    bm.to_mesh(obj.data)
    bm.free()
    me = obj.data
    me.update()
    uvl = me.uv_layers.active.data if me.uv_layers.active else None
    # Corner (loop) normals: smooth/sharp as authored.
    loop_n = np.zeros(len(me.loops) * 3, np.float32)
    me.corner_normals.foreach_get("vector", loop_n)
    loop_n = loop_n.reshape(-1, 3)
    co = np.array([v.co[:] for v in me.vertices], np.float32)
    lo, hi = co.min(0), co.max(0)
    cx, cy = (lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2
    base = lo[2] + sink * (hi[2] - lo[2])
    verts, index, remap = [], [], {}
    for poly in me.polygons:
        for li in poly.loop_indices:
            vi = me.loops[li].vertex_index
            p = co[vi]
            n = loop_n[li]
            uv = uvl[li].uv if uvl else (0.0, 0.0)
            # Blender (x, y, z), Z up → Bevy (x, z, -y), Y up. UV v flips (glTF convention).
            key = (vi, round(float(n[0]), 3), round(float(n[1]), 3), round(float(n[2]), 3), round(uv[0], 5), round(uv[1], 5))
            k = remap.get(key)
            if k is None:
                k = len(verts)
                remap[key] = k
                verts.append(((p[0] - cx, p[2] - base, -(p[1] - cy)), (n[0], n[2], -n[1]), (uv[0], 1.0 - uv[1])))
            index.append(k)
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, f"{name}.pnm"), "wb") as f:
        f.write(b"PNM1")
        f.write(struct.pack("<II", len(verts), len(index)))
        for v in verts:
            f.write(struct.pack("<3f", *v[0]))
        for v in verts:
            f.write(struct.pack("<3f", *v[1]))
        for v in verts:
            f.write(struct.pack("<2f", *v[2]))
        f.write(struct.pack(f"<{len(index)}I", *index))
    size = hi - lo
    print(f"mesh {name}: {before} -> {len(index) // 3} tris, {len(verts)} verts, size {size[0]:.2f} x {size[2]:.2f} x {size[1]:.2f} m (w h d)")


for it in ITEMS:
    export(*it)
