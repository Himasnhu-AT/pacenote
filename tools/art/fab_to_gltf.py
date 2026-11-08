# Converts the Fab (Quixel Megascans and Sketchfab photoscan) model downloads in
# assets_src/fab_batch1/ into the glTF layout tools/art/build_meshes.py reads:
#   assets_src/models/<category>/<stem>/<stem>_2k.gltf (+ .bin), one named object per mesh.
#   blender -b --factory-startup -P tools/art/fab_to_gltf.py -- <fab_batch1> <assets_src/models> [stem ...]
# For each item it imports the chosen LOD file(s) (FBX, OBJ or glTF), bakes parent and object
# transforms (Megascans FBX/glTF ship centimetres under a 0.01 scale), drops billboards and
# ground patches, joins the parts and names the result. Geometry only: the textures are
# prepared separately by tools/art/prep_fab_models.sh into assets/textures/m_*.
# Picks LODs close to the game's triangle targets rather than LOD0: collapse decimation
# wrecks alpha-card foliage much more than the scanned LODs do.
import math
import os
import random
import re
import sys

import bpy

argv = sys.argv[sys.argv.index("--") + 1:]
FAB, OUT = argv[0], argv[1]
ONLY = set(argv[2:])

# stem, category, [(object name, source file under fab_batch1, keep regex, drop regex, op)]
# op: None, "tufts" (Graminea ships one flat card holding three tufts side by side: split it
# into thirds and stand each tuft as a crossed pair around a small ring), "scatter" (lay the
# loose twigs, which ship stacked at the origin, out as a small pile) or "trim_floor" (drop the
# flat underside where a scan met the ground, and loose debris islands).
ITEMS = [
    ("coast_rauk_boulder_01", "rocks", [
        ("coast_rauk_boulder_01", "coast_rauk_boulder_l_03_wfepegb_high/Coast_Rauk_Boulder_L_03_wfepegb_High.fbx", r"LOD0", None, None),
    ]),
    ("sandstone_cliff_01", "rocks", [
        ("sandstone_cliff_01", "large_sandstone_cliff_vminadn_high/Large_Sandstone_Cliff_vminadn_High.fbx", r"LOD0", None, None),
    ]),
    ("dead_shrubs_01", "vegetation", [
        # VarG: the broad round bush (LOD2 file, 1077 tris); VarH: the long low one (LOD1, 605).
        ("dead_shrubs_01_g", "dead_shrubs_vdknafgha_high/Dead_Shrubs_vdknafgha_High_vdknafgha_VarG_LOD2.fbx", None, None, None),
        ("dead_shrubs_01_h", "dead_shrubs_vdknafgha_high/Dead_Shrubs_vdknafgha_High_vdknafgha_VarH_LOD1.fbx", None, None, None),
    ]),
    ("dry_branches_01", "vegetation", [
        ("dry_branches_01", "dry_branches_pdye3_high/Dry_Branches_pdye3_High.fbx", r"LOD0", None, "scatter"),
    ]),
    ("dead_branches_01", "vegetation", [
        ("dead_branches_01", "dead_branches_qjuwd_high/Dead_Branches_qjuwd_High.fbx", r"LOD0", None, "scatter"),
    ]),
    ("dry_grass_01", "vegetation", [
        ("dry_grass_01_a", "dry_grass_tbbqejqr_ue_high/tbbqejqr_tier_1.gltf", r"^SM_tbbqejqr_VarA_LOD1$", None, None),
        ("dry_grass_01_b", "dry_grass_tbbqejqr_ue_high/tbbqejqr_tier_1.gltf", r"^SM_tbbqejqr_VarB_LOD1$", None, None),
    ]),
    ("cocksfoot_01", "vegetation", [
        ("cocksfoot_01_a", "cocksfoot_grass_tcfoejcr_high/Cocksfoot_Grass_tcfoejcr_High_tcfoejcr_VarA_LOD1.fbx", None, None, None),
    ]),
    ("graminea_01", "vegetation", [
        # The ASCII FBX can't be read by Blender; the OBJs hold the same flat card.
        ("graminea_01", "graminea01_additional_files/Graminea01_lod2.obj", None, None, "tufts"),
    ]),
    ("kikuyu_01", "vegetation", [
        ("kikuyu_01_b", "kikuyu_grass_wiljearqx_ue_high/wiljearqx_tier_1.gltf", r"^SM_wiljearqx_VarB_LOD1$", None, None),
    ]),
    ("concrete_barrier_01", "props", [
        ("concrete_barrier_01", "concrete_barrier_ubitfhtfa_high/Concrete_Barrier_ubitfhtfa_High.fbx", r"LOD0", None, None),
    ]),
    ("industrial_barrier_01", "props", [
        ("industrial_barrier_01", "industrial_construction_barrier_concrete_l_04_xiwcfczsc_high/Industrial_Construction_Barrier_Concrete_L_04_xiwcfczsc_High.fbx", r"LOD0", None, None),
    ]),
    ("jersey_barrier_02", "props", [
        # The scan ships its cobblestone floor as a separate "Ground" node: dropped. The barrier
        # itself still has a ~3k-triangle flat underside and three crumbs of cobble.
        ("jersey_barrier_02", "jersey_road_barrier_no2_free_photoscan_gltf/scene.gltf", r"^Barrier", r"Ground", "trim_floor"),
    ]),
    ("modular_fence_01", "props", [
        # One section: the 3 m rail panel plus its post at x = 0 (tile every 3 m).
        ("modular_fence_01", "modular_fence_ufflaitdw_high/Modular_Fence_ufflaitdw_High.fbx", r"LOD0", None, None),
        # The post alone, to close the far end of a run.
        ("modular_fence_01_post", "modular_fence_ufflaitdw_high/Modular_Fence_ufflaitdw_High.fbx", r"LOD0_TIER1_000$", None, None),
    ]),
]


def import_any(path):
    ext = os.path.splitext(path)[1].lower()
    if ext == ".fbx":
        bpy.ops.import_scene.fbx(filepath=path)
    elif ext == ".obj":
        bpy.ops.wm.obj_import(filepath=path)
    else:
        bpy.ops.import_scene.gltf(filepath=path)


def bake(objs):
    """Unparents each mesh keeping its world transform, then applies it to the vertices."""
    bpy.context.view_layer.update()
    for o in objs:
        mw = o.matrix_world.copy()
        o.parent = None
        o.matrix_world = mw
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)


def join(objs):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    if len(objs) > 1:
        bpy.ops.object.join()
    return bpy.context.view_layer.objects.active


def lowest(o):
    return min(v.co.z for v in o.data.vertices)


def scatter(objs, seed):
    """Lays loose twigs out as a pile about 0.6 m across: random headings, small offsets,
    each resting on z = 0 (Megascans ship every piece centred on the origin)."""
    rng = random.Random(seed)
    for i, o in enumerate(objs):
        a = rng.uniform(0, math.pi)
        r = rng.uniform(0.0, 0.18)
        t = rng.uniform(0, 2 * math.pi)
        o.rotation_euler = (0, 0, a)
        o.location = (r * math.cos(t), r * math.sin(t), 0)
        bpy.context.view_layer.update()
        bpy.ops.object.select_all(action="DESELECT")
        o.select_set(True)
        bpy.context.view_layer.objects.active = o
        bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
        z0 = lowest(o)
        for v in o.data.vertices:
            v.co.z -= z0


def tufts(o, n=3, ring=0.12):
    """Splits a flat card (in the XZ plane) into n tufts by x, then stands each as two cards
    crossed at 90°, spread around a ring of `ring` m at random headings."""
    import bmesh
    me = o.data
    xs = [v.co.x for v in me.vertices]
    x0, x1 = min(xs), max(xs)
    rng = random.Random("tufts")
    parts = []
    for k in range(n):
        lo, hi = x0 + (x1 - x0) * k / n, x0 + (x1 - x0) * (k + 1) / n
        c = o.copy()
        c.data = me.copy()
        bpy.context.collection.objects.link(c)
        bm = bmesh.new()
        bm.from_mesh(c.data)
        drop = [f for f in bm.faces if not (lo <= f.calc_center_median().x < hi or (k == n - 1 and f.calc_center_median().x >= hi))]
        bmesh.ops.delete(bm, geom=drop, context="FACES")
        cx = sum(v.co.x for v in bm.verts) / max(1, len(bm.verts))
        for v in bm.verts:
            v.co.x -= cx
        bm.to_mesh(c.data)
        bm.free()
        a = 2 * math.pi * k / n + rng.uniform(-0.3, 0.3)
        for turn in (0.0, math.pi / 2):
            d = c if turn == 0.0 else c.copy()
            if d is not c:
                d.data = c.data.copy()
                bpy.context.collection.objects.link(d)
            d.rotation_euler = (0, 0, a + turn + rng.uniform(-0.2, 0.2))
            d.location = (ring * math.cos(a), ring * math.sin(a), 0)
            parts.append(d)
    bpy.data.objects.remove(o, do_unlink=True)
    bake(parts)
    return parts


def trim_floor(o, band=0.02, min_share=0.01):
    """Removes near-flat faces within `band` m of the lowest point (a scan's ground contact,
    never seen) and islands smaller than `min_share` of the faces (cobble crumbs)."""
    import bmesh
    bm = bmesh.new()
    bm.from_mesh(o.data)
    bmesh.ops.remove_doubles(bm, verts=bm.verts[:], dist=0.0005)
    z0 = min(v.co.z for v in bm.verts)
    drop = {f for f in bm.faces if max(v.co.z for v in f.verts) < z0 + band and abs(f.normal.z) > 0.6}
    seen, small = set(), []
    for f in bm.faces:
        if f in seen:
            continue
        comp, st = [], [f]
        seen.add(f)
        while st:
            g = st.pop()
            comp.append(g)
            for e in g.edges:
                for h in e.link_faces:
                    if h not in seen:
                        seen.add(h)
                        st.append(h)
        if len(comp) < min_share * len(bm.faces):
            small += comp
    bmesh.ops.delete(bm, geom=list(drop | set(small)), context="FACES")
    bm.to_mesh(o.data)
    bm.free()


def tris(o):
    return sum(len(p.vertices) - 2 for p in o.data.polygons)


def build(stem, cat, parts):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    made = []
    for name, src, keep, drop, op in parts:
        before = set(bpy.data.objects)
        import_any(os.path.join(FAB, src))
        new = [o for o in bpy.data.objects if o not in before]
        meshes = [o for o in new if o.type == "MESH"
                  and (keep is None or re.search(keep, o.name) or (o.parent and re.search(keep, o.parent.name)))
                  and not (drop and (re.search(drop, o.name) or (o.parent and re.search(drop, o.parent.name))))
                  and "billboard" not in " ".join(m.name.lower() for m in o.data.materials if m)]
        if not meshes:
            raise SystemExit(f"{stem}/{name}: nothing matched in {src}: {[o.name for o in new]}")
        bake(meshes)
        for o in new:
            if o not in meshes:
                bpy.data.objects.remove(o, do_unlink=True)
        if op == "scatter":
            scatter(meshes, stem)
        elif op == "tufts":
            meshes = tufts(meshes[0])
        o = join(meshes)
        if op == "trim_floor":
            trim_floor(o)
        o.name = name
        o.data.name = name
        # One material slot is all build_meshes needs; the textures live in assets/textures.
        o.data.materials.clear()
        made.append(o)
        lo = [min(v.co[i] for v in o.data.vertices) for i in range(3)]
        hi = [max(v.co[i] for v in o.data.vertices) for i in range(3)]
        print(f"fab {stem}/{name}: {tris(o)} tris, {hi[0]-lo[0]:.2f} x {hi[2]-lo[2]:.2f} x {hi[1]-lo[1]:.2f} m (w h d)")
    for o in list(bpy.data.objects):
        if o not in made:
            bpy.data.objects.remove(o, do_unlink=True)
    d = os.path.join(OUT, cat, stem)
    os.makedirs(d, exist_ok=True)
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.export_scene.gltf(filepath=os.path.join(d, f"{stem}_2k.gltf"), export_format="GLTF_SEPARATE",
                              use_selection=True, export_materials="NONE", export_apply=True)


for item in ITEMS:
    if not ONLY or item[0] in ONLY:
        build(*item)
