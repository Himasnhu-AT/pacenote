# Converts the downloaded Sketchfab models (assets_src/sketchfab/, see assets_src/SOURCES.md)
# into the game's assets (Blender, background):
#
#   blender -b --factory-startup -P tools/art/import_sketchfab.py -- car  <file|dir> [options]
#   blender -b --factory-startup -P tools/art/import_sketchfab.py -- arms <file|dir> [options]
#
# car: the "Milano '95" body → assets/models/rallycar.glb, with the node names src/art/car.rs
#   expects (Car root, Body, Interior, Hub<FL|FR|RL|RR>/Spin<..>, SteeringWheel, Tacho, Speedo,
#   Shifter, Handbrake). Steps: import; bake every transform into the meshes; turn the car to
#   face +Y; find the four wheels (by name, else by shape); scale to the sim's wheelbase
#   (front axle y +1.25, rear -1.22) with the origin at hub height; drop the model's own
#   interior (parts inside the cabin, or matching --drop); make the glass see-through from
#   inside; measure the cabin; add our cockpit (cockpit.py) and a lining; merge the static
#   parts per material; split each wheel onto its Hub/Spin nodes; export.
#   Options: --out PATH  --forward +y|-y|+x|-x|auto  --drop REGEX  --keep REGEX
#            --max-tex N (default 2048)  --preview DIR  --no-bake  --list
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

import bmesh
import bpy
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import cockpit  # noqa: E402
from blendkit import MATS, link  # noqa: E402

ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
FRONT_Y, REAR_Y = 1.25, -1.22  # src/vehicle.rs Spec mounts
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


def shrink_textures(n):
    for im in bpy.data.images:
        if im.size[0] > n or im.size[1] > n:
            f = n / max(im.size)
            w, h = max(1, int(im.size[0] * f)), max(1, int(im.size[1] * f))
            print(f"texture {im.name}: {im.size[0]}x{im.size[1]} -> {w}x{h}")
            im.scale(w, h)
            im.pack()


# ---------------------------------------------------------------- car


def bake_transforms(objs):
    """World transforms into the mesh data; no parents left."""
    for o in objs:
        if o.data.users > 1:
            o.data = o.data.copy()
        mw = o.matrix_world.copy()
        o.parent = None
        o.data.transform(mw)
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


def is_glass(o):
    for m in used_mats(o):
        if m is None:
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
        print(f"  {o.name:32s} verts {len(o.data.vertices):6d}  centre {tuple(round(x, 2) for x in (l + h) / 2)}  size {tuple(round(x, 2) for x in h - l)}  mats {mats}{'  [glass]' if is_glass(o) else ''}")
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
    ground = cockpit.G
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
    roof = body_hi.z - ground
    cab.cage_roof = roof - 0.12
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
    path = find_file(opts["src"], "milano")
    import_any(path)
    for o in list(bpy.data.objects):
        if o.type not in ("MESH", "EMPTY"):
            bpy.data.objects.remove(o, do_unlink=True)
    meshes = [o for o in bpy.data.objects if o.type == "MESH"]
    bake_transforms(meshes)
    for o in [o for o in bpy.data.objects if o.type == "EMPTY"]:
        bpy.data.objects.remove(o, do_unlink=True)
    if opts.get("list"):
        list_car(meshes)
        return
    orient(meshes, opts.get("forward", "auto"))
    parts = split_loose(meshes)
    corners, centres, r = find_wheels(parts)
    # Scale to the sim's wheelbase; origin at hub height, axles at the sim's y, centred in x.
    wb = (centres["FL"].y + centres["FR"].y - centres["RL"].y - centres["RR"].y) / 2
    s = (FRONT_Y - REAR_Y) / wb
    hub_z = sum(c.z for c in centres.values()) / 4
    front_y = (centres["FL"].y + centres["FR"].y) / 2
    lo, hi = bbox(parts)
    mid_x = (lo.x + hi.x) / 2
    m = Matrix.Translation((0, FRONT_Y, 0)) @ Matrix.Scale(s, 4) @ Matrix.Translation((-mid_x, -front_y, -hub_z))
    for p in parts:
        p.data.transform(m)
    for k in centres:
        centres[k] = m @ centres[k]
    print(f"scale {s:.4f}; tyre radius {r * s:.3f} m (src/art/car.rs MODEL_WHEEL_RADIUS is 0.325: update it if this differs)")
    print("hubs:", {k: tuple(round(x, 3) for x in v) for k, v in centres.items()})
    wheel_parts = {p for ps in corners.values() for p in ps}
    rest = [p for p in parts if p not in wheel_parts]
    glass = [p for p in rest if is_glass(p)]
    lo, hi = bbox(rest)
    cab = measure_cabin(glass, lo, hi)
    ground = cockpit.G
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
    for p in glass:
        for mt in used_mats(p):
            if mt and mt.use_nodes:
                b = next((n for n in mt.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
                if b is not None and not b.inputs["Alpha"].is_linked:
                    b.inputs["Alpha"].default_value = min(b.inputs["Alpha"].default_value, 0.45)
                mt.surface_render_method = "BLENDED"
    root = bpy.data.objects.new("Car", None)
    link(root)
    select(rest)
    bpy.ops.object.join()
    body = bpy.context.view_layer.objects.active
    body.name = body.data.name = "Body"
    body.parent = root
    # Wheels on Hub/Spin nodes (callipers on the hub, not spinning).
    for k, ps in corners.items():
        ce = centres[k]
        hub = bpy.data.objects.new(f"Hub{k}", None)
        link(hub)
        hub.parent = root
        hub.location = (ce.x, ce.y, 0.0)
        spin = bpy.data.objects.new(f"Spin{k}", None)
        link(spin)
        spin.parent = hub
        for group, name, parent in (([p for p in ps if not CALIPER_RE.search(p.name)], f"Wheel{k}", spin),
                                    ([p for p in ps if CALIPER_RE.search(p.name)], f"Caliper{k}", hub)):
            if not group:
                continue
            select(group)
            if len(group) > 1:
                bpy.ops.object.join()
            w = bpy.context.view_layer.objects.active
            w.name = w.data.name = name
            w.data.transform(Matrix.Translation((-ce.x, -ce.y, -ce.z)))
            w.parent = parent
    # Our cockpit, lined with the body's own cabin faces.
    cockpit.materials()
    glass_mats = {i for i, mt in enumerate(body.data.materials) if mt and is_glass_mat(mt)}
    y0, y1 = lo.y + 0.12, cab.screen_base[0] + 0.06
    skin = cockpit.inner_skin(body, lambda f, c: f.material_index not in glass_mats and y0 < c.y < y1 and c.z - ground > cab.floor and f.normal.z > -0.5)
    cockpit.build(root, skin, cab, bake=not opts.get("no-bake"))
    shrink_textures(int(opts.get("max-tex", 2048)))
    tris = 0
    for o in bpy.data.objects:
        if o.type == "MESH":
            o.data.calc_loop_triangles()
            tris += len(o.data.loop_triangles)
    print(f"car: {tris} triangles, {len(bpy.data.materials)} materials")
    if "preview" in opts:
        import build_rallycar
        build_rallycar.preview(opts["preview"])
    export(opts.get("out", os.path.join(ROOT, "assets/models/rallycar.glb")))


def is_glass_mat(mt):
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
