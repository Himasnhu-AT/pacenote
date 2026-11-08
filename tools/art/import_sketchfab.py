# Converts the downloaded Sketchfab models (assets_src/sketchfab/, see assets_src/SOURCES.md)
# into the game's assets (Blender, background):
#
#   blender -b --factory-startup -P tools/art/import_sketchfab.py -- car  <file|dir> [options]
#   blender -b --factory-startup -P tools/art/import_sketchfab.py -- arms <file|dir> [options]
#
# car: a downloaded car body → assets/models/rallycar.glb, with the node names src/art/car.rs
#   expects: Car root > Chassis (the sprung body: Body, Interior, detachable parts such as
#   MirrorL/RearWing, SteeringWheel, Tacho, Speedo, Shifter, Handbrake) and Hub<FL|FR|RL|RR>
#   (suspension) > Steer<FL|FR> (front knuckle) > Spin<..> (rolling) > Wheel<..>, with
#   Caliper<..> on the knuckle / rear hub; CoDriverEye, CoDriverTablet on the root. Steps: import; bake every transform into the meshes; turn the car to
#   face +Y; decimate (per material, --decimate); find the four wheels (by name, else by
#   shape); scale to the sim's wheelbase (axles at y ±1.275) and track (hubs at x ±0.78:
#   src/art/car.rs MODEL_HALF_*) with the origin at hub height; then either
#     --interior ours (default): drop the model's own interior (parts inside the cabin, or
#       matching --drop); make the glass see-through from inside; measure the cabin; add our
#       cockpit (cockpit.py) and a lining;
#     --interior keep: keep the model's interior; its steering wheel (--steer-mat) becomes
#       the SteeringWheel node and our shifter and handbrake go on its console. The driver's
#       rig (eye, shoulders, wheel, levers) moves as one by the offset that puts our wheel
#       centre on the model's: the script prints it and the constants for car.rs / hands.rs;
#   merge the static parts per material; split each wheel onto its Hub/Spin nodes; cap the
#   textures (opaque ones as JPEG); export.
#   Options: --out PATH  --forward +y|-y|+x|-x|auto  --drop REGEX  --keep REGEX
#            --max-tex N (default 2048)  --small-tex N (parts matching --small-mat, 1024)
#            --decimate "REGEX=RATIO;..." (first match on the material names; "*" default 1)
#            --pbr "REGEX=METAL,ROUGH;..." (metallic and least roughness per material; also
#              drops clear coat, which the game renders as a pale haze)
#            --ao-mat REGEX (keep: cabin materials that get baked ambient occlusion)
#            --interior ours|keep  --steer-mat REGEX  --steer-dz M  --steer-tilt DEG
#            --parts "Name=x0,x1,y0,y1,z0,z1[@MATREGEX];..." (keep: detachable parts)
#            --mirror-seat MATREGEX (keep: a passenger seat from the driver's)
#            --codriver-eye X,Y,Z  --codriver-tablet X,Y,Z (keep: glTF model frame)
#            --profile NAME (option defaults for a known download, see PROFILES)
#            --prep FILE (a Python file run right after the import, with `meshes`, `opts` and
#              this module's names in scope: per-download fixes, see tools/art/car_prep/)
#            --paint REGEX (those materials become one plain "Paint" material, no textures:
#              the game recolours it; printed liveries and sponsor decals go with the texture)
#            --paint-color RRGGBB  --paint-rough R
#            --plain "REGEX=RRGGBB[,METAL,ROUGH[,ALPHA]];..." (those materials lose their
#              textures and get a plain colour: logos printed in a texture, tyre sidewall text)
#            --cockpit-dz M (ours: lower our cockpit by -M; default: keep the hatch's headroom)
#            --lining-y0 Y (ours: where the cabin lining ends at the back; default the tail)
#            --lining-ratio R (ours: the lining from the body decimated to R; default 1)
#            --drop-mat REGEX (drop the parts with those materials)
#            --cut "x0,x1,y0,y1,z0,z1[@MATREGEX];..." (delete the faces in those boxes, model
#              frame after scaling: badges and plates moulded into a bigger part)
#            --exhaust-mat REGEX (prints the exhaust tip for src/art/exhaust.rs)
#            --preview DIR  --no-bake  --list
#
# arms: the "First Person arms" rig → assets/models/driver_arms.glb. Steps: import; apply
#   scale/rotation; remove constraints, animations and helper objects; rename the arm bones
#   to UpperArm/ForeArm/Hand and Thumb|Index|Middle|Ring|Pinky1..3, with .L/.R; export the
#   skinned mesh. src/art/hands.rs poses those joints from its IK (shoulder, elbow, wrist, grip).
#   Options: --out PATH  --list  --map OLD=NEW (repeatable, overrides the guess)
#
# --list prints what's in the file (objects, materials, bones) and stops: run it first on a
# new download and check the guesses.
import math
import os
import re
import sys
import tempfile

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import cockpit  # noqa: E402
from blendkit import MATS, empty, link, material  # noqa: E402

ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
# Hubs about the model origin: src/art/car.rs MODEL_HALF_WHEELBASE / MODEL_HALF_TRACK (the
# sim's mounts, src/vehicle.rs Spec).
FRONT_Y, REAR_Y = 1.275, -1.275
HALF_TRACK = 0.78
# Option defaults per download (assets_src/SOURCES.md). Command-line options win.
PROFILES = {
    # (The profile for the former player car, a branded GT3 download, was removed with the car:
    # docs/ASSETS_BRIEF.md §0. See git history for an example of a cabin-keeping profile.)
    # Real-car look-alikes, debadged (owner's ruling, 8 Nov 2026): no maker's logo, badge,
    # script, plate or sponsor livery survives; the body paint is plain "Paint".
    "kestrel": {  # 2024 GT3-class racer (2024_porsche_992_gt3_r.glb)
        "out": os.path.join(ROOT, "assets/models/car_kestrel.glb"),
        "prep": "car_prep/kestrel.py",
        "forward": "-y",
        "interior": "ours",
        "paint": r"^BodyPaint$",
        # The tyre sidewall text, the crest / script / fuel-cap decals.
        "plain": "^Tyre$=1b1b1c,0,0.85;^Details$=26272a,0.3,0.5;^Glass$=1c2126,0,0.05,0.45;^LampLens$=d8dde2,0,0.05,0.3",
        "pbr": "^Rim$|Exhaust|Disc=1,0.3;Caliper=0.6,0.35;^Glass$|LampLens=0,0.05;*=0,0.4",
        "decimate": "BodyPaint|^Paint$=0.17;Exhaust=0.45;Mechanics|Underbody=0.2;Lamp=0.3;^Rim$=0.15;Tyre|Disc|Caliper=0.35;*=0.4",
        "exhaust-mat": "^Exhaust$",
        "lining-y0": "-1.25",
        "small-mat": "Lamp|Disc|Mesh|Rim|Glass",
    },
    "brawn": {  # 1970 muscle coupe (chevrolet-chevelle-ss-1970/source/x/Source_mat.FBX)
        "out": os.path.join(ROOT, "assets/models/car_brawn.glb"),
        "prep": "car_prep/brawn.py",
        "forward": "-y",
        "interior": "ours",
        "paint": r"^BodyPaint$",
        "plain": "^Glass$=1c2126,0,0.05,0.45",
        "decimate": "BodyPaint|^Paint$=0.07;^Glass$=0.2;^Wheel=0.035;*=0.035",
        "lining-y0": "-1.5",
        "exhaust-mat": "^Exhaust$",
        "small-mat": "^Glass$",
    },
}
# Original cars (concept image -> TripoSG -> asset-forge image3d_project.py + image3d_refine.py,
# see tools/art/concept_cars.py): one body surface with class materials, WheelFL.. objects.
_ORIGINAL = {
    "forward": "+y",
    "interior": "ours",
    "paint": r"^Body$",
    "plain": "^Glass$=1c2126,0,0.05,0.45;^Plastic$=0b0b0c,0,0.6;^Chrome$=d9dbe0,1,0.12;^LampAmber$=ff7a10,0,0.2;^LampRed$=b00808,0,0.2;^Tyre$=141415,0,0.85;^Rim$=8c8f94,1,0.3",
    "decimate": "^Body$|^Paint$=0.55;^Plastic$=0.5;*=1",
    "small-mat": "^Glass$",
}
for _id in ("lynx", "kite", "tarn"):
    PROFILES[_id] = dict(_ORIGINAL, out=os.path.join(ROOT, f"assets/models/car_{_id}.glb"))
EXTS = (".glb", ".gltf", ".fbx", ".blend", ".obj")
WHEEL_RE = re.compile(r"wheel|tire|tyre|rim|disc|disk|brake|hub|caliper|rotor", re.I)
CALIPER_RE = re.compile(r"caliper", re.I)
NOT_WHEEL_RE = re.compile(r"steer|spare|arch|well", re.I)
INTERIOR_RE = re.compile(r"interior|seat|dash|steer|pedal|gear|shift|cockpit|cabin|inside|belt|mirror_int|console", re.I)
GLASS_RE = re.compile(r"glass|window|windshield|windscreen", re.I)


# ---------------------------------------------------------------- shared


def args():
    a = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if len(a) < 2 or a[0] not in ("car", "arms"):
        sys.exit("usage: ... -- car|arms <file|dir> [options]")
    opts = {"mode": a[0], "src": a[1], "map": []}
    i = 2
    while i < len(a):
        k = a[i]
        if k in ("--list", "--no-bake"):
            opts[k[2:]] = True
        elif k == "--map":
            opts["map"].append(a[i + 1])
            i += 1
        elif k.startswith("--"):
            opts[k[2:]] = a[i + 1]
            i += 1
        i += 1
    if "profile" in opts:
        for k, v in PROFILES[opts["profile"]].items():
            opts.setdefault(k, v)
    return opts


def find_file(src, hint):
    """`src` itself, or the best model file under the directory `src` (name containing `hint`
    preferred; glTF before FBX, .blend, OBJ)."""
    if os.path.isfile(src):
        return src
    found = []
    for d, _, files in os.walk(src):
        for f in files:
            if f.lower().endswith(EXTS):
                found.append(os.path.join(d, f))
    if not found:
        sys.exit(f"no model file ({', '.join(EXTS)}) under {src}")
    rank = lambda p: (hint not in p.lower(), [p.lower().endswith(e) for e in EXTS].index(True), len(p))
    return sorted(found, key=rank)[0]


def import_any(path):
    ext = os.path.splitext(path)[1].lower()
    if ext in (".glb", ".gltf"):
        bpy.ops.import_scene.gltf(filepath=path)
    elif ext == ".fbx":
        bpy.ops.import_scene.fbx(filepath=path)
    elif ext == ".obj":
        bpy.ops.wm.obj_import(filepath=path)
    elif ext == ".blend":
        with bpy.data.libraries.load(path) as (src, dst):
            dst.objects = src.objects
        for o in dst.objects:
            if o is not None:
                link(o)
    print(f"imported {path}")


def select(objs, active=None):
    for o in bpy.context.view_layer.objects:
        if o is not None:
            o.select_set(False)
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = active or (objs[0] if objs else None)


def export(path, selection=None, skins=False):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if selection is not None:
        select(selection)
    bpy.ops.export_scene.gltf(filepath=path, export_format="GLB", export_yup=True, export_apply=not skins,
                              use_selection=selection is not None, export_image_format="AUTO",
                              export_skins=skins, export_animations=False,
                              export_vertex_color="NAME", export_vertex_color_name="AO")
    print(f"wrote {path} ({os.path.getsize(path) / 1e6:.1f} MB)")


def bbox(objs):
    lo, hi = Vector((1e9,) * 3), Vector((-1e9,) * 3)
    for o in objs:
        for v in o.data.vertices:
            p = o.matrix_world @ v.co
            lo = Vector(map(min, lo, p))
            hi = Vector(map(max, hi, p))
    return lo, hi


def shrink_textures(n, small=None, small_re=None, jpeg=False):
    """Caps every image at n pixels (images only used by materials matching `small_re` at
    `small`). With `jpeg`, opaque images are written as JPEG (much smaller than PNG)."""
    users = {}
    for mt in bpy.data.materials:
        if mt.node_tree is None:
            continue
        for nd in mt.node_tree.nodes:
            if nd.type == "TEX_IMAGE" and nd.image is not None:
                users.setdefault(nd.image.name, []).append(mt.name)
    for im in bpy.data.images:
        if im.size[0] == 0:
            continue
        cap = n
        mats = users.get(im.name, [])
        if small and small_re and mats and all(small_re.search(m) for m in mats):
            cap = min(cap, small)
        if im.size[0] > cap or im.size[1] > cap:
            f = cap / max(im.size)
            w, h = max(1, int(im.size[0] * f)), max(1, int(im.size[1] * f))
            print(f"texture {im.name} ({', '.join(mats)}): {im.size[0]}x{im.size[1]} -> {w}x{h}")
            im.scale(w, h)
            im.pack()
        if not jpeg or im.is_float or not mats:
            continue
        if im.channels == 4:
            px = np.empty(im.size[0] * im.size[1] * 4, dtype=np.float32)
            im.pixels.foreach_get(px)
            if px[3::4].min() < 0.995:
                continue
        # The exporter keeps a file's own format: write it out as a JPEG and use that.
        global TMP
        TMP = TMP or tempfile.mkdtemp(prefix="pacenote_tex_")
        path = os.path.join(TMP, re.sub(r"[^\w.-]", "_", im.name) + ".jpg")
        im.file_format = "JPEG"
        im.save(filepath=path, quality=88)
        if im.packed_file:
            im.unpack(method="REMOVE")
        im.source = "FILE"
        im.filepath_raw = path
        im.reload()


TMP = None


def fix_pbr(rules):
    """Metallic and least roughness per material (rules: "REGEX=METAL,ROUGH;...", first
    match; "*" all). Downloads often leave glTF's metallicFactor unset, which means 1: every
    part turns to chrome."""
    table = []
    for r in rules.split(";"):
        rx, v = r.rsplit("=", 1)
        metal, rough = (float(x) for x in v.split(","))
        table.append((None if rx == "*" else re.compile(rx, re.I), metal, rough))
    for mt in bpy.data.materials:
        b = mt.node_tree and next((n for n in mt.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
        if not b:
            continue
        hit = next(((m, r) for rx, m, r in table if rx is None or rx.search(mt.name)), None)
        if hit is None:
            continue
        if not b.inputs["Metallic"].is_linked:
            b.inputs["Metallic"].default_value = hit[0]
        if not b.inputs["Roughness"].is_linked:
            b.inputs["Roughness"].default_value = max(b.inputs["Roughness"].default_value, hit[1])
    # No clear coat: the game's clear coat lights a dark paint up to a pale haze.
    for mt in bpy.data.materials:
        b = mt.node_tree and next((n for n in mt.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
        if b and "Coat Weight" in b.inputs:
            for l in list(b.inputs["Coat Weight"].links):
                mt.node_tree.links.remove(l)
            b.inputs["Coat Weight"].default_value = 0.0


def hex_rgb(h):
    """"RRGGBB" (sRGB) → linear RGB."""
    s = [int(h[i:i + 2], 16) / 255 for i in (0, 2, 4)]
    return tuple(c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4 for c in s)


def plain_material(mt, color, metal=0.0, rough=0.5, alpha=None):
    """Rebuilds `mt` in place (name and users kept) as a plain Principled material (alpha:
    see-through, blended; else the old material's fixed alpha)."""
    mt.use_nodes = True
    nt = mt.node_tree
    blend = mt.surface_render_method
    old = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
    if alpha is None:
        alpha = 1.0
        if old is not None and not old.inputs["Alpha"].is_linked:
            alpha = old.inputs["Alpha"].default_value
    else:
        blend = "BLENDED"
    for n in list(nt.nodes):
        nt.nodes.remove(n)
    b = nt.nodes.new("ShaderNodeBsdfPrincipled")
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    nt.links.new(b.outputs[0], out.inputs["Surface"])
    b.inputs["Base Color"].default_value = (*color, 1.0)
    b.inputs["Metallic"].default_value = metal
    b.inputs["Roughness"].default_value = rough
    b.inputs["Alpha"].default_value = alpha
    mt.surface_render_method = blend if alpha < 0.98 else "DITHERED"
    mt.diffuse_color = (*color, 1.0)


def paint_and_plain(objs, opts):
    """--paint: the matching materials → one plain "Paint" (the game's livery colour);
    --plain: the matching materials lose their textures."""
    if "paint" in opts:
        rx = re.compile(opts["paint"], re.I)
        paint = bpy.data.materials.get("Paint")
        if paint is None or not paint.users:
            paint = bpy.data.materials.new("Paint")
        plain_material(paint, hex_rgb(opts.get("paint-color", "1d5c5e")), 0.0, float(opts.get("paint-rough", 0.3)))
        hit = set()
        for o in objs:
            for slot in o.material_slots:
                if slot.material and slot.material is not paint and rx.search(slot.material.name):
                    hit.add(slot.material.name)
                    slot.material = paint
        print(f"paint: {sorted(hit)} -> Paint")
    if "plain" in opts:
        for r in opts["plain"].split(";"):
            rx, v = r.rsplit("=", 1)
            v = v.split(",")
            rx = re.compile(rx, re.I)
            for mt in bpy.data.materials:
                if mt.name != "Paint" and rx.search(mt.name):
                    plain_material(mt, hex_rgb(v[0]), float(v[1]) if len(v) > 1 else 0.0, float(v[2]) if len(v) > 2 else 0.5,
                                   float(v[3]) if len(v) > 3 else None)
                    print(f"plain: {mt.name} = #{v[0]}")


def game_constants(body, r, opts, exhaust=()):
    """Prints the constants src/art/car.rs and exhaust.rs need for this model (glTF frame: x, z,
    -y; origin at hub height)."""
    g = lambda v: f"Vec3::new({v[0]:.3f}, {v[2]:.3f}, {-v[1]:.3f})"
    print(f"CONST MODEL_WHEEL_RADIUS = {r:.3f}")
    eye = Vector((cockpit.DRV_X, -0.60, cockpit.zg(1.12))) + getattr(cockpit, "EYE_OFFSET", Vector())
    print(f"CONST COCKPIT_EYE_MODEL = {g(eye)}  ({eye.z + r:.3f} m above the ground)")
    print(f"CONST hands.rs RIG = {g(getattr(cockpit, 'EYE_OFFSET', Vector()))}  (the cockpit's offset from cockpit.py's)")
    print(f"CONST CABIN_FILL_MODEL = {g(Vector((0.0, -0.50, 0.80 + getattr(cockpit, 'EYE_OFFSET', Vector()).z)))}")
    # Bonnet camera: 12 cm over the body's top near the middle, 0.3 m behind the nose's front
    # axle line... measured where the bonnet is (y 1.3..1.9, |x| < 0.25).
    mw = body.matrix_world
    zs = [(mw @ v.co).z for v in body.data.vertices if abs((mw @ v.co).x) < 0.25 and 1.45 < (mw @ v.co).y < 1.75]
    if zs:
        print(f"CONST BONNET_CAM_MODEL = {g((0.0, 1.6, max(zs) + 0.13))}  (bonnet top {max(zs):.3f} at y 1.6)")
    lo, hi = bbox([body])
    print(f"CONST body extent (model frame) x {lo.x:.3f}..{hi.x:.3f} y {lo.y:.3f}..{hi.y:.3f} z {lo.z:.3f}..{hi.z:.3f}; roof {hi.z + r:.3f} m above the ground")
    if exhaust:
        # The rearmost 4 cm of the exhaust parts' vertices (taken before the body is joined).
        ymin = min(p.y for p in exhaust)
        pts = [p for p in exhaust if p.y < ymin + 0.04]
        for side in (-1, 1):
            s = [p for p in pts if p.x * side > 0]
            if s:
                c = sum(s, Vector()) / len(s)
                print(f"CONST exhaust tip ({'right' if side > 0 else 'left'}) TIP_MODEL = {g(c)}  (z {c.z:.3f} = {c.z + r:.3f} - MODEL_WHEEL_RADIUS)")


def strip_uvs(objs):
    """Drops UV layers no material reads (downloads often carry a stack of lightmap UVs)."""
    for o in objs:
        uvs = o.data.uv_layers
        want = {uvs.active.name} if uvs.active else set()
        for l in uvs:
            if l.active_render:
                want.add(l.name)
        for mt in used_mats(o):
            if mt.node_tree is not None:
                for nd in mt.node_tree.nodes:
                    if nd.type == "UVMAP" and nd.uv_map:
                        want.add(nd.uv_map)
        for l in [l for l in uvs if l.name not in want]:
            uvs.remove(l)


def decimate(objs, rules):
    """Collapse-decimates each object by the ratio of the first rule matching one of its
    materials (rules: "REGEX=RATIO;...", "*" matches all). The shading normals come from the
    full mesh (copied onto the decimated one), so the reflections keep their shape."""
    table = []
    for r in rules.split(";"):
        rx, ratio = r.rsplit("=", 1)
        table.append((None if rx == "*" else re.compile(rx, re.I), float(ratio)))
    before = after = 0
    for o in objs:
        names = [m.name for m in used_mats(o)] + [o.name]
        ratio = next((q for rx, q in table if rx is None or any(rx.search(n) for n in names)), 1.0)
        o.data.calc_loop_triangles()
        t0 = len(o.data.loop_triangles)
        if ratio < 0.999 and t0 > 200:
            full = o.copy()
            full.data = o.data.copy()
            link(full)
            mod = o.modifiers.new("dec", "DECIMATE")
            mod.ratio = ratio
            mod.use_collapse_triangulate = True
            select([o])
            bpy.ops.object.modifier_apply(modifier=mod.name)
            o.data.shade_smooth()
            o.data.set_sharp_from_angle(angle=math.radians(40))
            dt = o.modifiers.new("normals", "DATA_TRANSFER")
            dt.object = full
            dt.use_loop_data = True
            dt.data_types_loops = {"CUSTOM_NORMAL"}
            dt.loop_mapping = "POLYINTERP_NEAREST"
            bpy.ops.object.modifier_apply(modifier=dt.name)
            mesh = full.data
            bpy.data.objects.remove(full, do_unlink=True)
            bpy.data.meshes.remove(mesh)
        o.data.calc_loop_triangles()
        before += t0
        after += len(o.data.loop_triangles)
        print(f"  {o.name}: {t0} -> {len(o.data.loop_triangles)} ({', '.join(names[:-1])})")
    print(f"decimated {before} -> {after} triangles")


# ---------------------------------------------------------------- car


def bake_transforms(objs):
    """World transforms into the mesh data; no parents left."""
    for o in objs:
        if o.data.users > 1:
            o.data = o.data.copy()
        mw = o.matrix_world.copy()
        # (FBX files can carry transform keys: they would undo the bake on the next update.)
        x = o
        while x is not None:
            x.animation_data_clear()
            x = x.parent
        o.parent = None
        o.data.transform(mw)
        if mw.determinant() < 0:
            # A mirrored transform (common in FBX): keep the faces' winding outward.
            o.data.flip_normals()
        o.matrix_world = Matrix.Identity(4)


def split_loose(objs):
    out = []
    for o in objs:
        select([o])
        bpy.ops.object.mode_set(mode="EDIT")
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.mesh.separate(type="LOOSE")
        bpy.ops.object.mode_set(mode="OBJECT")
        out += [x for x in bpy.context.selected_objects]
    return out


def used_mats(o):
    """Materials the object's faces use (a split part keeps every slot of its source)."""
    idx = {p.material_index for p in o.data.polygons}
    return [o.data.materials[i] for i in sorted(idx) if i < len(o.data.materials) and o.data.materials[i] is not None]


# Lamp lenses are see-through too, but not the cabin's glass (the cabin is measured from it).
NOT_GLASS_RE = re.compile(r"lamp|light|lens|indicator|reflector", re.I)


def is_glass(o):
    for m in used_mats(o):
        if m is None or NOT_GLASS_RE.search(m.name):
            continue
        if GLASS_RE.search(m.name) or GLASS_RE.search(o.name):
            return True
        if m.use_nodes:
            b = next((n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
            # A fixed translucent alpha (an alpha *texture* is usually a cut-out, not glass).
            if b is not None and b.inputs["Alpha"].default_value < 0.98 and not b.inputs["Alpha"].is_linked:
                return True
            if b is not None and "Transmission Weight" in b.inputs and b.inputs["Transmission Weight"].default_value > 0.3:
                return True
    return False


def list_car(objs):
    lo, hi = bbox(objs)
    print(f"{len(objs)} meshes, bbox {tuple(round(x, 3) for x in lo)} .. {tuple(round(x, 3) for x in hi)}")
    for o in objs:
        l, h = bbox([o])
        mats = [m.name for m in used_mats(o)]
        o.data.calc_loop_triangles()
        print(f"  {o.name:32s} tris {len(o.data.loop_triangles):6d} verts {len(o.data.vertices):6d}  centre {tuple(round(x, 2) for x in (l + h) / 2)}  size {tuple(round(x, 2) for x in h - l)}  mats {mats}{'  [glass]' if is_glass(o) else ''}")
    for m in bpy.data.materials:
        print(f"  material {m.name}")
    for im in bpy.data.images:
        print(f"  image {im.name} {im.size[0]}x{im.size[1]}")


def orient(objs, forward):
    """Turns the car to run along Y, nose at +Y. `forward`: where the nose points now, or
    "auto" (the roof's highest part is behind the middle on a hatchback)."""
    lo, hi = bbox(objs)
    size = hi - lo
    rot = Matrix.Identity(4)
    if forward == "auto":
        along_x = size.x > size.y
        top = []
        for o in objs:
            for v in o.data.vertices:
                if v.co.z > hi.z - 0.04 * size.z:
                    top.append(v.co.x if along_x else v.co.y)
        mid = ((lo.x + hi.x) if along_x else (lo.y + hi.y)) / 2
        roof = sum(top) / max(len(top), 1)
        sign = "-" if roof > mid else "+"
        forward = sign + ("x" if along_x else "y")
        print(f"forward guess: nose towards {forward} (roof centre {roof:.2f}, middle {mid:.2f}); override with --forward")
    turn = {"+y": 0.0, "-y": math.pi, "+x": math.pi / 2, "-x": -math.pi / 2}[forward]
    rot = Matrix.Rotation(turn, 4, "Z")
    for o in objs:
        o.data.transform(rot)


def find_wheels(parts):
    """Wheel parts per corner (FL, FR, RL, RR): parts named like wheels, else low thin round
    parts near the corners. Returns {corner: [parts]}, centres and radius."""
    lo, hi = bbox(parts)
    c = (lo + hi) / 2
    H, W = hi.z - lo.z, hi.x - lo.x
    named = [p for p in parts if WHEEL_RE.search(p.name) and not NOT_WHEEL_RE.search(p.name)]
    named += [p for p in parts if p not in named and any(WHEEL_RE.search(m.name) and not NOT_WHEEL_RE.search(m.name) for m in used_mats(p))]

    def shaped(p):
        l, h = bbox([p])
        s = h - l
        m = (l + h) / 2
        return (m.z - lo.z < 0.42 * H and abs(m.x - c.x) > 0.22 * W and s.x < 0.7 * max(s.y, s.z)
                and 0.6 < s.y / max(s.z, 1e-6) < 1.6 and max(s.y, s.z) > 0.12 * H)

    pick = named if len(named) >= 4 else [p for p in parts if shaped(p)]
    corners = {"FL": [], "FR": [], "RL": [], "RR": []}
    for p in pick:
        l, h = bbox([p])
        m = (l + h) / 2
        k = ("F" if m.y > c.y else "R") + ("L" if m.x < c.x else "R")
        corners[k].append(p)
    # Small parts near a wheel that sit inside its radius belong to it (nuts, discs).
    centres, radius = {}, []
    for k, ps in corners.items():
        if not ps:
            sys.exit(f"no wheel found at {k}: check --list and name the parts, or pass --keep/--drop")
        l, h = bbox(ps)
        centres[k] = (l + h) / 2
        radius.append(max(h.y - l.y, h.z - l.z) / 2)
    for p in parts:
        if any(p in ps for ps in corners.values()):
            continue
        l, h = bbox([p])
        m = (l + h) / 2
        for k, ce in centres.items():
            if (m - ce).length < 0.6 * min(radius) and (h - l).length < 1.6 * min(radius):
                corners[k].append(p)
    return corners, centres, sum(radius) / 4


def measure_cabin(glass, body_lo, body_hi):
    """Cabin dimensions (heights above the cockpit's ground line) from the glass, defaults where
    it's unclear."""
    cab = type("Cab", (cockpit.Cabin,), {})
    ground = cockpit.G + COCKPIT_DZ
    front, side = [], []
    for o in glass:
        for f in o.data.polygons:
            n = f.normal
            p = f.center
            if n.y > 0.35 and n.z > 0.2:
                front.append(p)
            elif abs(n.x) > 0.6:
                side.append(p)
    if front:
        cab.screen_base = (max(p.y for p in front), min(p.z for p in front) - ground)
        cab.screen_top_y = min(p.y for p in front)
    if side:
        cab.belt = min(p.z for p in side) - ground
        cab.door_y = (cab.screen_base[0] - 0.04, max(cab.door_y[1], min(p.y for p in side) + 0.10))
    cab.firewall_y = cab.screen_base[0] + 0.02
    roof = (body_hi.z if CABIN_ROOF is None else CABIN_ROOF) - ground
    cab.cage_roof = roof - 0.12
    if FLOOR_Z is not None:
        cab.floor = max(cab.floor, FLOOR_Z - ground)
    if CAGE_ROOF is not None:
        # Under the roof where the cage's roof bars run (a curved roof is lower there).
        cab.cage_roof = min(cab.cage_roof, CAGE_ROOF - ground - 0.07)
    # Width: the side windows (the bounding box includes the door mirrors).
    xs = sorted(abs(p.x) for p in side)
    w = xs[int(0.9 * (len(xs) - 1))] + 0.03 if xs else (body_hi.x - body_lo.x) / 2 - 0.12
    cab.side_x = w - 0.07
    cab.cage_x_belt = w - 0.12
    cab.cage_x_roof = w - 0.29
    print("cabin:", {k: getattr(cab, k) for k in ("floor", "firewall_y", "side_x", "door_y", "belt", "cage_roof", "screen_top_y", "screen_base", "cage_x_belt", "cage_x_roof")})
    if not (1.25 < roof < 1.55):
        print(f"WARNING: roof {roof:.2f} m above the ground; the driver's eye is at 1.12 m (car.rs COCKPIT_EYE)")
    if not (0.40 < cab.screen_base[0] < 0.80):
        print(f"WARNING: windscreen base at y {cab.screen_base[0]:.2f}; the dash (cockpit.py) sits at 0.18..0.64")
    return cab


def car(opts):
    path = find_file(opts["src"], opts.get("hint", "milano"))
    import_any(path)
    for o in list(bpy.data.objects):
        if o.type not in ("MESH", "EMPTY"):
            bpy.data.objects.remove(o, do_unlink=True)
    meshes = [o for o in bpy.data.objects if o.type == "MESH"]
    bake_transforms(meshes)
    for o in [o for o in bpy.data.objects if o.type == "EMPTY"]:
        bpy.data.objects.remove(o, do_unlink=True)
    if "prep" in opts:
        prep = opts["prep"] if os.path.isabs(opts["prep"]) else os.path.join(HERE, opts["prep"])
        scope = dict(globals(), meshes=meshes, opts=opts, __file__=prep)
        exec(compile(open(prep).read(), prep, "exec"), scope)
        meshes = [o for o in bpy.data.objects if o.type == "MESH"]
        print(f"prep {prep}: {len(meshes)} meshes")
    if opts.get("list"):
        list_car(meshes)
        return
    paint_and_plain(meshes, opts)
    keep_interior = opts.get("interior", "ours") == "keep"
    if "drop-mat" in opts:
        rx = re.compile(opts["drop-mat"], re.I)
        for o in [o for o in meshes if any(rx.search(m.name) for m in used_mats(o))]:
            print(f"dropped {o.name} (--drop-mat)")
            meshes.remove(o)
            bpy.data.objects.remove(o, do_unlink=True)
    orient(meshes, opts.get("forward", "auto"))
    if "decimate" in opts:
        decimate(meshes, opts["decimate"])
        strip_uvs(meshes)
    if "pbr" in opts:
        fix_pbr(opts["pbr"])
    if keep_interior:
        # Only the wheels need splitting into parts (the interior stays as it is).
        wheelish = [o for o in meshes if any(WHEEL_RE.search(n) and not NOT_WHEEL_RE.search(n) for n in [o.name] + [m.name for m in used_mats(o)])]
        parts = [o for o in meshes if o not in wheelish] + split_loose(wheelish)
    else:
        parts = list(dict.fromkeys(split_loose(meshes)))
    corners, centres, r = find_wheels(parts)
    # Scale to the sim's wheelbase (and squeeze the width to its track); origin at hub height,
    # axles at the sim's y, centred in x.
    wb = (centres["FL"].y + centres["FR"].y - centres["RL"].y - centres["RR"].y) / 2
    s = (FRONT_Y - REAR_Y) / wb
    half_track = (centres["FR"].x + centres["RR"].x - centres["FL"].x - centres["RL"].x) / 4
    sx = HALF_TRACK / half_track
    hub_z = sum(c.z for c in centres.values()) / 4
    front_y = (centres["FL"].y + centres["FR"].y) / 2
    mid_x = sum(c.x for c in centres.values()) / 4
    m = Matrix.Translation((0, FRONT_Y, 0)) @ Matrix.Diagonal((sx, s, s, 1.0)) @ Matrix.Translation((-mid_x, -front_y, -hub_z))
    for p in parts:
        p.data.transform(m)
    for k in centres:
        centres[k] = m @ centres[k]
    r *= s
    print(f"scale {s:.4f} (width {sx:.4f}); tyre radius {r:.3f} m (src/art/car.rs MODEL_WHEEL_RADIUS: set it to this)")
    print("hubs:", {k: tuple(round(x, 3) for x in v) for k, v in centres.items()})
    wheel_parts = {p for ps in corners.values() for p in ps}
    rest = [p for p in parts if p not in wheel_parts]
    glass = [p for p in rest if is_glass(p)]
    lo, hi = bbox(rest)
    root = bpy.data.objects.new("Car", None)
    link(root)
    # Everything carried by the sprung body hangs off one node (the game can lean it on top
    # of the simulated chassis); the wheels hang off the root.
    chassis = empty("Chassis", (0, 0, 0), root)
    detached = {}
    if keep_interior:
        rx = re.compile(opts["steer-mat"], re.I)
        steer = [p for p in rest if any(rx.search(mt.name) for mt in used_mats(p))]
        rest = [p for p in rest if p not in steer]
        if "cut" in opts:
            cut_faces(rest, opts["cut"])
        if "mirror-seat" in opts:
            rest.append(mirror_seat(rest, re.compile(opts["mirror-seat"], re.I)))
        if "plates" in opts:
            cockpit.materials()
            rest += plates(opts["plates"])
        if "parts" in opts:
            detached = split_parts(rest, opts["parts"])
        if "ao-mat" in opts and not opts.get("no-bake"):
            bake_ao(rest + steer + [p for ps in detached.values() for p in ps], re.compile(opts["ao-mat"], re.I), glass)
        interior_keep(opts, steer, glass, chassis, r)
        codriver_nodes(opts, root)
        if "driver-eye" in opts:
            driver_body(opts, chassis)
        if "passenger-harness" in opts:
            import driver
            driver.seat_harness(chassis, float(opts["passenger-harness"]))
    else:
        if "cut" in opts:
            cut_faces(rest, opts["cut"])
        cockpit_drop(opts, rest)
        cab = interior_drop(opts, rest, glass, lo)
    exhaust = []
    if "exhaust-mat" in opts:
        rx = re.compile(opts["exhaust-mat"], re.I)
        for p in rest:
            mats = [m.name if m else "" for m in p.data.materials]
            for f in p.data.polygons:
                if f.material_index < len(mats) and rx.search(mats[f.material_index]):
                    exhaust += [p.matrix_world @ p.data.vertices[i].co for i in f.vertices]
    select(rest)
    bpy.ops.object.join()
    body = bpy.context.view_layer.objects.active
    body.name = body.data.name = "Body"
    body.parent = chassis
    # Detachable parts: one mesh each, its origin at its middle.
    for name, ps in detached.items():
        select(ps)
        if len(ps) > 1:
            bpy.ops.object.join()
        o = bpy.context.view_layer.objects.active
        o.name = o.data.name = name
        l, h = bbox([o])
        c = (l + h) / 2
        o.data.transform(Matrix.Translation(-c))
        o.location = c
        o.parent = chassis
        if name in opts.get("mirror-glass", "").split(","):
            mirror_glass(o)
        o.data.calc_loop_triangles()
        print(f"detachable {name}: {len(o.data.loop_triangles)} triangles at {tuple(round(x, 3) for x in c)}")
    # Wheels: Hub<k> (suspension travel) > Steer<k> (front: the knuckle, steering) > Spin<k>
    # (rolling) > Wheel<k> (rim, tyre, disc); the calliper rides the knuckle (rear: the hub).
    for k, ps in corners.items():
        ce = centres[k]
        hub = bpy.data.objects.new(f"Hub{k}", None)
        link(hub)
        hub.parent = root
        hub.location = (ce.x, ce.y, 0.0)
        carrier = hub
        if k[0] == "F":
            carrier = empty(f"Steer{k}", (0, 0, 0), hub)
        spin = empty(f"Spin{k}", (0, 0, 0), carrier)
        for group, name, parent in (([p for p in ps if not is_caliper(p)], f"Wheel{k}", spin),
                                    ([p for p in ps if is_caliper(p)], f"Caliper{k}", carrier)):
            if not group:
                continue
            select(group)
            if len(group) > 1:
                bpy.ops.object.join()
            w = bpy.context.view_layer.objects.active
            w.name = w.data.name = name
            w.data.transform(Matrix.Translation((-ce.x, -ce.y, -ce.z)))
            w.parent = parent
    if not keep_interior:
        # Our cockpit, lined with the body's own cabin faces.
        cockpit.materials()
        glass_mats = {i for i, mt in enumerate(body.data.materials) if mt and is_glass_mat(mt)}
        ground = cockpit.G + COCKPIT_DZ
        # The lining's rear end: the tail, unless --lining-y0 says where the cabin ends (an
        # engine behind the seats: its body faces would show through as grey patches).
        # The front end: the windscreen base, but not into the front wheel arches.
        y0, y1 = float(opts.get("lining-y0", lo.y + 0.12)), min(cab.screen_base[0] + 0.06, FRONT_Y - r - 0.05)
        mid_z = ground + 0.75
        # The lining comes from a coarser copy of the body (--lining-ratio): only seen from
        # inside, in the cabin's shade.
        src = body
        if float(opts.get("lining-ratio", 1.0)) < 0.999:
            src = body.copy()
            src.data = body.data.copy()
            link(src)
            mod = src.modifiers.new("dec", "DECIMATE")
            mod.ratio = float(opts["lining-ratio"])
            mod.use_collapse_triangulate = True
            select([src])
            bpy.ops.object.modifier_apply(modifier=mod.name)
            print(f"lining source: {len(body.data.polygons)} -> {len(src.data.polygons)} faces")
        # (Whole faces in the cabin's length: cockpit.bake_ao splits long edges, a sliver
        # reaching the nose would come out as dozens of faces.)
        skin = cockpit.inner_skin(src, lambda f, c: f.material_index not in glass_mats and y0 < c.y < y1 and c.z - ground > cab.floor and f.normal.z > -0.5
                                  and all(y0 - 0.1 < v.co.y < y1 + 0.1 for v in f.verts)
                                  # Faces looking out of the cabin only (flipped, they look in):
                                  # a face the wrong way round would show outside.
                                  and f.normal.dot(Vector((c.x, 0.0, c.z - mid_z))) > 0.0)
        print(f"lining: {len(skin.f)} faces")
        if src is not body:
            mesh = src.data
            bpy.data.objects.remove(src, do_unlink=True)
            bpy.data.meshes.remove(mesh)
        # The cockpit is built at its own heights, then lowered by COCKPIT_DZ as a whole. The
        # lining joins it afterwards with a flat shade (the cockpit's AO bake splits long edges
        # first: a body's lining would come out several times bigger).
        before = set(chassis.children)
        cockpit.build(chassis, None, cab, bake=not opts.get("no-bake"))
        interior = None
        for o in set(chassis.children) - before:
            if o.type == "MESH" and o.name == "Interior":
                o.data.transform(Matrix.Translation((0, 0, COCKPIT_DZ)))
                interior = o
            else:
                o.location.z += COCKPIT_DZ
        cockpit.EYE_OFFSET = Vector((0, 0, COCKPIT_DZ))
        # Nothing of the cockpit below its floor (a raised floor: seat bases, pedal boxes
        # would hang under the body).
        fz = ground + cab.floor - 0.03
        bm = bmesh.new()
        bm.from_mesh(interior.data)
        geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
        bmesh.ops.bisect_plane(bm, geom=geom, plane_co=(0, 0, fz), plane_no=(0, 0, 1), clear_inner=True)
        bm.to_mesh(interior.data)
        bm.free()
        lining = skin.obj("CabinLining", recalc=False, smooth_angle=35)
        if interior.data.color_attributes.get("AO") is not None:
            a = lining.data.color_attributes.new("AO", "BYTE_COLOR", "CORNER")
            a.data.foreach_set("color", np.tile(np.array([0.55, 0.55, 0.55, 1.0], dtype=np.float32), len(a.data)))
        select([interior, lining], interior)
        bpy.ops.object.join()
    small_re = re.compile(opts["small-mat"], re.I) if "small-mat" in opts else None
    shrink_textures(int(opts.get("max-tex", 2048)), int(opts.get("small-tex", 1024)), small_re, jpeg=keep_interior or "small-mat" in opts)
    tris = 0
    for o in bpy.data.objects:
        if o.type == "MESH":
            o.data.calc_loop_triangles()
            tris += len(o.data.loop_triangles)
    print(f"car: {tris} triangles, {len(bpy.data.materials)} materials")
    game_constants(body, r, opts, exhaust)
    if "preview" in opts:
        import build_rallycar
        build_rallycar.preview(opts["preview"])
    export(opts.get("out", os.path.join(ROOT, "assets/models/rallycar.glb")))


def is_caliper(p):
    return CALIPER_RE.search(p.name) or any(CALIPER_RE.search(m.name) for m in used_mats(p))


COCKPIT_DZ = 0.0  # how far our cockpit is lowered (m; cockpit_drop)
CABIN_ROOF = None  # roof height over the seats (model z), if measured
CAGE_ROOF = None  # lowest roof height over the cage's roof bars (model z), if measured
FLOOR_Z = None  # lowest the cabin floor may be (model z): above the sills' underside


def cockpit_drop(opts, rest):
    """--interior ours in a low body: lowers our cockpit (eye, seats, cage, wheel, levers) so
    the driver keeps the rally hatch's headroom (0.34 m from the eye to the roof over the seats).
    --cockpit-dz M sets it; 0 for a body as tall as the hatch or taller."""
    global COCKPIT_DZ, CABIN_ROOF, CAGE_ROOF
    zs = [v.co.z for p in rest for v in p.data.vertices if abs(v.co.x) < 0.45 and -0.9 < v.co.y < 0.1]
    CABIN_ROOF = max(zs) if zs else None
    # The roof over the cage's roof bars: its highest point near each of a few spots along
    # the bars (x ±0.5, y -1.1..-0.1); the lowest of those.
    co = np.array([tuple(v.co) for p in rest for v in p.data.vertices if 0.3 < abs(v.co.x) < 0.65 and -1.2 < v.co.y < 0.0])
    tops = []
    if len(co):
        for sx in (-1, 1):
            for y in np.linspace(-1.1, -0.1, 6):
                m = (np.abs(co[:, 0] - sx * 0.5) < 0.08) & (np.abs(co[:, 1] - y) < 0.08)
                if m.any():
                    tops.append(co[m, 2].max())
    CAGE_ROOF = min(tops) if tops else None
    eye = cockpit.zg(1.12)
    if "cockpit-dz" in opts:
        COCKPIT_DZ = float(opts["cockpit-dz"])
    elif CABIN_ROOF is not None:
        COCKPIT_DZ = min(0.0, round((CABIN_ROOF - 0.34 - eye) / 0.005) * 0.005)
    # The cabin floor stays above the sills' underside (else it shows under the body): see
    # measure_cabin.
    global FLOOR_Z
    zs = [v.co.z for p in rest for v in p.data.vertices if 0.6 < abs(v.co.x) < 1.0 and -1.0 < v.co.y < 0.4]
    FLOOR_Z = min(zs) + 0.05 if zs else None
    print(f"roof over the seats z {CABIN_ROOF}, over the cage bars {CAGE_ROOF}; cockpit lowered by {-COCKPIT_DZ:.3f} m")


def interior_drop(opts, rest, glass, lo):
    """Drops the model's interior (our cockpit replaces it) and thins the glass. Returns the
    measured cabin."""
    _, hi = bbox(rest)
    cab = measure_cabin(glass, lo, hi)
    ground = cockpit.G + COCKPIT_DZ
    # Drop the model's interior: parts inside the cabin (or named so), unless --keep says not.
    drop_re = re.compile(opts["drop"], re.I) if "drop" in opts else None
    keep_re = re.compile(opts["keep"], re.I) if "keep" in opts else None
    dropped = []
    for p in list(rest):
        if p in glass or (keep_re and keep_re.search(p.name)):
            continue
        l, h = bbox([p])
        inside = (abs(l.x) < cab.side_x - 0.03 and abs(h.x) < cab.side_x - 0.03 and l.z - ground > 0.15
                  and h.z - ground < cab.cage_roof + 0.08 and l.y > lo.y + 0.25 and h.y < cab.screen_base[0] + 0.15)
        # Wheel-arch liners sit inside that box too: keep parts round an axle, out at the sides.
        arch = max(abs(l.x), abs(h.x)) > 0.45 and min(abs((l.y + h.y) / 2 - FRONT_Y), abs((l.y + h.y) / 2 - REAR_Y)) < 0.5
        inside = inside and not arch
        named = INTERIOR_RE.search(p.name) or any(INTERIOR_RE.search(mt.name) for mt in used_mats(p))
        if inside or named or (drop_re and drop_re.search(p.name)):
            dropped.append(p.name)
            rest.remove(p)
            bpy.data.objects.remove(p, do_unlink=True)
    print(f"dropped {len(dropped)} interior parts: {dropped[:40]}{' ...' if len(dropped) > 40 else ''}")
    # Single-sided materials (the body vanishes from inside, our lining shows); glass thinner.
    for mt in bpy.data.materials:
        mt.use_backface_culling = True
    thin_glass(glass)
    return cab


def thin_glass(glass, alpha=0.45):
    for p in glass:
        for mt in used_mats(p):
            if mt and mt.use_nodes:
                b = next((n for n in mt.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
                if b is not None and not b.inputs["Alpha"].is_linked:
                    b.inputs["Alpha"].default_value = min(b.inputs["Alpha"].default_value, alpha)
                # Plain alpha blending, not transmission (a costly pass in the game).
                if b is not None and "Transmission Weight" in b.inputs:
                    for l in list(b.inputs["Transmission Weight"].links):
                        mt.node_tree.links.remove(l)
                    b.inputs["Transmission Weight"].default_value = 0.0
                mt.surface_render_method = "BLENDED"


def boxes_of(spec):
    """"x0,x1,y0,y1,z0,z1[,extra...]@MATREGEX;..." → [(lo, hi, extras, regex)]."""
    out = []
    for item in spec.split(";"):
        v, _, mat = item.partition("@")
        n = [float(t) for t in v.split(",")]
        out.append((Vector((n[0], n[2], n[4])), Vector((n[1], n[3], n[5])), n[6:], re.compile(mat or ".", re.I)))
    return out


def cut_faces(objs, spec):
    """Deletes the faces whose middle lies in a box and whose material matches."""
    boxes = boxes_of(spec)
    n = 0
    for o in objs:
        mats = [m.name if m else "" for m in o.data.materials]
        bm = bmesh.new()
        bm.from_mesh(o.data)
        gone = []
        for f in bm.faces:
            c = f.calc_center_median()
            m = mats[f.material_index] if f.material_index < len(mats) else ""
            if any(all(lo[k] <= c[k] <= hi[k] for k in range(3)) and rx.search(m) for lo, hi, _, rx in boxes):
                gone.append(f)
        if gone:
            bmesh.ops.delete(bm, geom=gone, context="FACES")
            bm.to_mesh(o.data)
            n += len(gone)
        bm.free()
    print(f"cut {n} faces")


def plates(spec):
    """Flat panels (boxes, tilted about x by the 7th number, degrees) in the first material
    matching the regex, UVs planar at 0.2 m a repeat."""
    out = []
    for lo, hi, extra, rx in boxes_of(spec):
        mat = next(m for m in bpy.data.materials if rx.search(m.name))
        c, size = (lo + hi) / 2, hi - lo
        me = bpy.data.meshes.new("Plate")
        bm = bmesh.new()
        bmesh.ops.create_cube(bm, size=1.0)
        bm.transform(Matrix.Diagonal((*size, 1.0)))
        bm.transform(Matrix.Translation(c) @ Matrix.Rotation(math.radians(extra[0] if extra else 0.0), 4, "X"))
        uv = bm.loops.layers.uv.new("UVMap")
        for f in bm.faces:
            for l in f.loops:
                l[uv].uv = (l.vert.co.x * 5.0, (l.vert.co.y + l.vert.co.z) * 5.0)
        bm.to_mesh(me)
        bm.free()
        ob = bpy.data.objects.new("Plate", me)
        link(ob)
        me.materials.append(mat)
        out.append(ob)
    return out


def mirror_glass(o):
    """The door mirror's glass: its rear-facing faces get a dark mirror material."""
    mt = bpy.data.materials.get("MirrorGlass") or material("MirrorGlass", (0.10, 0.11, 0.12), rough=0.04, metal=1.0)
    o.data.materials.append(mt)
    k = len(o.data.materials) - 1
    n3 = o.matrix_world.to_3x3()
    n = 0
    for f in o.data.polygons:
        if (n3 @ f.normal).normalized().y < -0.75:
            f.material_index = k
            n += 1
    print(f"mirror glass on {o.name}: {n} faces")


def driver_body(opts, parent):
    """The seated driver (tools/art/driver.py) for the outside and co-driver views."""
    import driver
    gx, gy, gz = (float(t) for t in opts["driver-eye"].split(","))
    eye = Vector((gx, -gz, gy))
    off = cockpit.EYE_OFFSET
    # The arms' shoulders (src/art/hands.rs SHOULDER, the hatchback's + the rig offset).
    shoulders = [Vector((-0.58, -0.74, cockpit.zg(0.90))) + off, Vector((-0.16, -0.74, cockpit.zg(0.90))) + off]
    py, pz = (float(t) for t in opts["driver-pedals"].split(","))
    driver.build(parent, eye, shoulders, float(opts["driver-seat-z"]), (py, pz))


def mirror_seat(rest, rx):
    """A passenger seat: the driver's (left) half of the parts whose materials match `rx`,
    mirrored to the right."""
    out = []
    for o in [p for p in rest if any(rx.search(m.name) for m in used_mats(p))]:
        c = o.copy()
        c.data = o.data.copy()
        link(c)
        bm = bmesh.new()
        bm.from_mesh(c.data)
        # The seat only: left of the middle, clear of the door (x < -0.68) and the harness
        # mounts behind the seat.
        keep = lambda f: all(-0.68 < v.co.x < -0.02 and v.co.y > -1.05 for v in f.verts)
        bmesh.ops.delete(bm, geom=[f for f in bm.faces if not keep(f)], context="FACES")
        bmesh.ops.delete(bm, geom=[v for v in bm.verts if not v.link_faces], context="VERTS")
        bm.transform(Matrix.Diagonal((-1.0, 1.0, 1.0, 1.0)))
        bmesh.ops.reverse_faces(bm, faces=bm.faces)
        bm.to_mesh(c.data)
        bm.free()
        if c.data.has_custom_normals:
            select([c])
            bpy.ops.mesh.customdata_custom_splitnormals_clear()
        c.data.shade_smooth()
        c.data.set_sharp_from_angle(angle=math.radians(40))
        if len(c.data.polygons):
            l, h = bbox([c])
            print(f"  seat copy of {o.name} {[m.name for m in used_mats(c)]}: {len(c.data.polygons)} faces, {tuple(round(x, 2) for x in l)} .. {tuple(round(x, 2) for x in h)}")
            out.append(c)
        else:
            bpy.data.objects.remove(c, do_unlink=True)
    select(out)
    if len(out) > 1:
        bpy.ops.object.join()
    seat = bpy.context.view_layer.objects.active
    seat.name = seat.data.name = "PassengerSeat"
    print(f"passenger seat from {len(out)} parts")
    return seat


def split_parts(rest, spec):
    """Detachable parts (spec: "Name=x0,x1,y0,y1,z0,z1[@MATREGEX];...", model frame, a name may
    repeat; the name "-" deletes): every loose piece whose middle is in a box (and whose
    materials match) goes to that part. Returns {name: [pieces]}; `rest` keeps the others."""
    boxes = []
    for item in spec.split(";"):
        name, v = item.split("=", 1)
        v, _, mat = v.partition("@")
        x0, x1, y0, y1, z0, z1 = (float(t) for t in v.split(","))
        boxes.append((name, Vector((x0, y0, z0)), Vector((x1, y1, z1)), re.compile(mat or ".", re.I)))
    out = {}
    for o in list(rest):
        l, h = bbox([o])
        if not any(all(l[i] <= b1[i] and h[i] >= b0[i] for i in range(3)) for _, b0, b1, _ in boxes):
            continue
        # One part at a time (a separate renumbers the faces left behind).
        while True:
            pick = island_pick(o, boxes)
            if not pick:
                break
            name, faces = next(iter(pick.items()))
            whole = len(faces) == len(o.data.polygons)
            if whole:
                rest.remove(o)
                out.setdefault(name, []).append(o)
                break
            select([o])
            bpy.ops.object.mode_set(mode="EDIT")
            bm = bmesh.from_edit_mesh(o.data)
            bm.faces.ensure_lookup_table()
            for v in bm.verts:
                v.select = False
            for e in bm.edges:
                e.select = False
            for f in bm.faces:
                f.select = False
            for i in faces:
                bm.faces[i].select_set(True)
            bm.select_mode = {"FACE"}
            bm.select_flush_mode()
            bmesh.update_edit_mesh(o.data)
            bpy.ops.mesh.separate(type="SELECTED")
            bpy.ops.object.mode_set(mode="OBJECT")
            out.setdefault(name, []).extend(x for x in bpy.context.selected_objects if x is not o)
    for o in out.pop("-", []):
        print(f"dropped {len(o.data.polygons)} faces ({', '.join(m.name for m in used_mats(o))})")
        bpy.data.objects.remove(o, do_unlink=True)
    return out


def island_pick(o, boxes):
    """{part name: set of face indices} for the face islands of `o` whose middle lies in a
    part's box (first matching box wins; its material must match)."""
    bm = bmesh.new()
    bm.from_mesh(o.data)
    bm.faces.ensure_lookup_table()
    mats = [m.name if m else "" for m in o.data.materials]
    seen = set()
    pick = {}
    for f0 in bm.faces:
        if f0.index in seen:
            continue
        island, stack = [], [f0]
        seen.add(f0.index)
        while stack:
            f = stack.pop()
            island.append(f)
            for e in f.edges:
                for g in e.link_faces:
                    if g.index not in seen:
                        seen.add(g.index)
                        stack.append(g)
        co = [v.co for f in island for v in f.verts]
        c = Vector(tuple((min(p[k] for p in co) + max(p[k] for p in co)) / 2 for k in range(3)))
        ms = {mats[f.material_index] for f in island if f.material_index < len(mats)}
        hit = next((n for n, b0, b1, rx in boxes if all(b0[k] <= c[k] <= b1[k] for k in range(3)) and any(rx.search(m) for m in ms)), None)
        if hit:
            pick.setdefault(hit, set()).update(f.index for f in island)
    bm.free()
    return pick


def codriver_nodes(opts, root):
    """Empties for the co-driver's view (src/guide): the passenger's eye and a tablet on their
    lap (glTF model frame in the options, like car.rs)."""
    for name in ("CoDriverEye", "CoDriverTablet"):
        key = "codriver-eye" if name == "CoDriverEye" else "codriver-tablet"
        if key in opts:
            x, y, z = (float(t) for t in opts[key].split(","))
            empty(name, (x, -z, y), root)


def bake_ao(objs, rx, glass, samples=48):
    """Bakes contact ambient occlusion (30 cm, the glass not counted) into an "AO" vertex
    colour layer (0.55..1: the game adds its own screen-space occlusion) of the parts whose materials match `rx`; the rest get a white one
    (every part joined into the body needs the layer)."""
    targets = [o for o in objs if any(rx.search(m.name) for m in used_mats(o))]
    for o in objs:
        a = o.data.color_attributes.new("AO", "BYTE_COLOR", "CORNER")
        o.data.color_attributes.active_color = a
        c = np.ones(len(a.data) * 4, dtype=np.float32)
        a.data.foreach_set("color", c)
    sc = bpy.context.scene
    sc.render.engine = "CYCLES"
    sc.cycles.device = "CPU"
    sc.cycles.samples = samples
    if sc.world is None:
        sc.world = bpy.data.worlds.new("World")
    sc.world.light_settings.distance = 0.30
    for g in glass:
        g.hide_render = True
    select(targets)
    bpy.ops.object.bake(type="AO", target="VERTEX_COLORS")
    for g in glass:
        g.hide_render = False
    for o in targets:
        a = o.data.color_attributes["AO"]
        c = np.zeros(len(a.data) * 4, dtype=np.float32)
        a.data.foreach_get("color", c)
        c = c.reshape(-1, 4)
        c[:, :3] = 0.55 + 0.45 * c[:, :3]
        c[:, 3] = 1.0
        a.data.foreach_set("color", c.ravel())
    print(f"baked ambient occlusion into {len(targets)} cabin parts")


def interior_keep(opts, steer, glass, root, r):
    """Keeps the model's cabin. Its steering wheel (`steer`: the parts with a --steer-mat
    material) becomes the SteeringWheel node; our levers go in; the driver's rig moves by the
    offset that puts our wheel centre on the model's."""
    if not steer:
        sys.exit(f"no steering wheel parts match --steer-mat {opts['steer-mat']}")
    lo, hi = bbox(steer)
    tilt = math.radians(float(opts.get("steer-tilt", math.degrees(cockpit.STEER_TILT))))
    # Rim centre: middle of the wheel's width and height, its grip plane ~3 cm in front of the
    # rearmost point (the grips' middle).
    centre = Vector(((lo.x + hi.x) / 2, lo.y + 0.03, (lo.z + hi.z) / 2 + float(opts.get("steer-dz", 0))))
    off = centre - cockpit.STEER_C
    off = Vector(tuple(round(x / 0.005) * 0.005 for x in off))
    nudge = cockpit.STEER_C + off - centre
    print(f"steering wheel centre {tuple(round(x, 3) for x in centre)}; driver offset {tuple(off)} (nudge {tuple(round(x, 4) for x in nudge)})")
    select(steer)
    if len(steer) > 1:
        bpy.ops.object.join()
    w = bpy.context.view_layer.objects.active
    node = empty("SteeringWheel", cockpit.STEER_C + off, root, rot=(-tilt, 0, 0))
    w.data.transform(Matrix.Translation(nudge))
    w.data.transform(Matrix.Rotation(tilt, 4, "X") @ Matrix.Translation(-(cockpit.STEER_C + off)))
    w.name = w.data.name = "SteeringWheelMesh"
    w.parent = node
    if "Metal" not in MATS:
        cockpit.materials()
    cockpit.add_nodes(root, parts=("levers",), offset=off)
    cockpit.EYE_OFFSET = off
    # The constants for the game (glTF frame: x, z, -y).
    g = lambda v: f"Vec3::new({v.x:.4f}, {v.z:.4f}, {-v.y:.4f})"
    eye = Vector((cockpit.DRV_X, -0.60, cockpit.zg(1.12))) + off
    print("src/art/car.rs COCKPIT_EYE_MODEL =", g(eye), f"(eye {eye.z + r:.3f} m above the ground)")
    print("src/art/hands.rs driver offset (glTF) =", g(off), f"WHEEL_TILT = {-math.degrees(tilt):.1f} deg")
    thin_glass(glass, alpha=0.35)


def is_glass_mat(mt):
    if NOT_GLASS_RE.search(mt.name):
        return False
    if GLASS_RE.search(mt.name):
        return True
    if mt.use_nodes:
        b = next((n for n in mt.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
        return b is not None and b.inputs["Alpha"].default_value < 0.98 and not b.inputs["Alpha"].is_linked
    return False


# ---------------------------------------------------------------- arms

FINGERS = ("Thumb", "Index", "Middle", "Ring", "Pinky")
FINGER_RE = {
    "Thumb": re.compile(r"thumb|pollex", re.I),
    "Index": re.compile(r"index|point|finger.?1|f1", re.I),
    "Middle": re.compile(r"middle|mid|finger.?2|f2", re.I),
    "Ring": re.compile(r"ring|finger.?3|f3", re.I),
    "Pinky": re.compile(r"pinky|little|small|finger.?4|f4", re.I),
}
SIDE_L = re.compile(r"(^|[._\s-])(l|left)([._\s-]|$)|left", re.I)
SIDE_R = re.compile(r"(^|[._\s-])(r|right)([._\s-]|$)|right", re.I)


def side_of(bone, arm):
    n = bone.name
    if SIDE_L.search(n) and not SIDE_R.search(n):
        return "L"
    if SIDE_R.search(n) and not SIDE_L.search(n):
        return "R"
    x = (arm.matrix_world @ bone.head_local).x
    return "L" if x < 0 else "R"


def list_arms(arm, meshes):
    print(f"armature {arm.name}: {len(arm.data.bones)} bones; meshes {[m.name for m in meshes]}")
    def walk(b, d):
        h = arm.matrix_world @ b.head_local
        t = arm.matrix_world @ b.tail_local
        print(f"  {'  ' * d}{b.name:28s} head {tuple(round(x, 3) for x in h)} len {(t - h).length:.3f} deform {b.use_deform}")
        for c in b.children:
            walk(c, d + 1)
    for b in arm.data.bones:
        if b.parent is None:
            walk(b, 0)
    for m in bpy.data.materials:
        print(f"  material {m.name}")
    for im in bpy.data.images:
        print(f"  image {im.name} {im.size[0]}x{im.size[1]}")


def guess_names(arm):
    """Canonical names for the arm bones: hands by name, forearm and upper arm up the chain,
    finger chains under the hand (by name, else ordered thumb → pinky across the hand)."""
    bones = arm.data.bones
    out = {}
    hands = [b for b in bones if re.search(r"hand|wrist|palm", b.name, re.I) and not any(r.search(b.name) for r in FINGER_RE.values())]
    # The hand is the bone the finger chains hang from: the shallowest match per side.
    per_side = {}
    for b in hands:
        s = side_of(b, arm)
        depth = len(b.parent_recursive)
        if s not in per_side or depth < per_side[s][0]:
            per_side[s] = (depth, b)
    for s, (_, h) in per_side.items():
        out[h.name] = f"Hand.{s}"
        fore = h.parent
        if fore is not None:
            out[fore.name] = f"ForeArm.{s}"
            if fore.parent is not None:
                out[fore.parent.name] = f"UpperArm.{s}"
        chains = [c for c in h.children if c.children or len(h.children) >= 4]
        named = {}
        for c in chains:
            for f, rx in FINGER_RE.items():
                if rx.search(c.name) and f not in named:
                    named[f] = c
                    break
        if len(named) < len(chains):
            # Order the chains across the hand: thumb is the one furthest from the others.
            hp = h.head_local
            pts = [(c, c.head_local) for c in chains]
            cen = sum((p for _, p in pts), Vector()) / max(len(pts), 1)
            thumb = max(pts, key=lambda cp: (cp[1] - cen).length)[0]
            rest = [c for c in chains if c is not thumb]
            ax = (cen - thumb.head_local).normalized()
            rest.sort(key=lambda c: (c.head_local - hp).dot(ax))
            named = {"Thumb": thumb}
            for f, c in zip(FINGERS[1:], rest):
                named[f] = c
        for f, c in named.items():
            k, b = 1, c
            while b is not None and k <= 3:
                out[b.name] = f"{f}{k}.{s}"
                b = b.children[0] if b.children else None
                k += 1
    return out


def arms(opts):
    path = find_file(opts["src"], "arm")
    import_any(path)
    for o in list(bpy.data.objects):
        if o.type in ("CAMERA", "LIGHT"):
            bpy.data.objects.remove(o, do_unlink=True)
    rigs = [o for o in bpy.data.objects if o.type == "ARMATURE"]
    if not rigs:
        sys.exit("no armature in the file")
    arm = max(rigs, key=lambda o: len(o.data.bones))
    meshes = [o for o in bpy.data.objects if o.type == "MESH" and any(m.type == "ARMATURE" for m in o.modifiers)]
    # Bake rotation and scale (FBX centimetres, Z-up fixes) into the rig and meshes.
    select([arm] + meshes, arm)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    if opts.get("list"):
        list_arms(arm, meshes)
        print("guessed names:", guess_names(arm))
        return
    for o in list(bpy.data.objects):
        if o is not arm and o not in meshes:
            bpy.data.objects.remove(o, do_unlink=True)
    arm.animation_data_clear()
    for a in list(bpy.data.actions):
        bpy.data.actions.remove(a)
    for pb in arm.pose.bones:
        for c in list(pb.constraints):
            pb.constraints.remove(c)
        pb.matrix_basis = Matrix.Identity(4)
    names = guess_names(arm)
    for m in opts["map"]:
        old, new = m.split("=", 1)
        names[old] = new
    want = [f"{p}.{s}" for s in "LR" for p in ("UpperArm", "ForeArm", "Hand")]
    missing = [w for w in want if w not in names.values()]
    if missing:
        sys.exit(f"bones not found: {missing}; run --list and pass --map OLD=NEW")
    # Rename bones (vertex groups follow bone renames in Blender).
    for old, new in names.items():
        arm.data.bones[old].name = new
    print("bones:", names)
    arm.name = arm.data.name = "DriverArms"
    export(opts.get("out", os.path.join(ROOT, "assets/models/driver_arms.glb")), [arm] + meshes, skins=True)


def main():
    o = args()
    bpy.ops.wm.read_factory_settings(use_empty=True)
    (car if o["mode"] == "car" else arms)(o)


if __name__ == "__main__":
    main()
