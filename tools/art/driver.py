# The seated driver's body (Blender), as the co-driver and the outside cameras see it: helmet
# with a dark visor, HANS collar, race suit (torso, thighs, shins), boots and a harness.
# The arms are not here: src/art/hands.rs draws and animates them from the shoulders given
# below, so the body ends at the shoulders. The game hides this node in the driver's own
# cockpit view (the camera is inside the helmet).
#
# Used by import_sketchfab.py (--interior keep). All points are in the model frame (x right,
# y forward, z up, origin at hub height); `build` takes the rig's eye and shoulders.
import math

import bmesh
import bpy
from mathutils import Matrix, Vector

from blendkit import empty, lin, link, material

# The suit and glove colours match the arms (src/art/hands.rs ArmAssets).
SUIT = (0.16, 0.28, 0.55)


def materials():
    material("DriverSuit", lin(SUIT), rough=0.85)
    material("DriverHelmet", lin((0.92, 0.92, 0.93)), rough=0.25)
    material("DriverHelmetStripe", lin((0.85, 0.10, 0.08)), rough=0.3)
    material("DriverVisor", lin((0.02, 0.02, 0.025)), rough=0.05, metal=0.6)
    material("DriverHans", lin((0.05, 0.05, 0.055)), rough=0.5)
    material("DriverStrap", lin((0.04, 0.04, 0.045)), rough=0.8)
    material("DriverBoot", lin((0.05, 0.05, 0.05)), rough=0.6)


def _skin(name, chain, mat):
    """A smooth limb or torso: a vertex chain with per-vertex (x, y) radii, Skin modifier,
    subdivided once, applied."""
    me = bpy.data.meshes.new(name)
    ob = bpy.data.objects.new(name, me)
    link(ob)
    me.from_pydata([p for p, _ in chain], [(i, i + 1) for i in range(len(chain) - 1)], [])
    ob.modifiers.new("skin", "SKIN")
    for v, (_, r) in zip(me.skin_vertices[0].data, chain):
        v.radius = r
    me.skin_vertices[0].data[0].use_root = True
    sub = ob.modifiers.new("sub", "SUBSURF")
    sub.levels = 2
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    ob.select_set(True)
    bpy.context.view_layer.objects.active = ob
    for m in list(ob.modifiers):
        bpy.ops.object.modifier_apply(modifier=m.name)
    me.materials.append(bpy.data.materials[mat])
    me.shade_smooth()
    return ob


def _sphere(name, c, r, scale, mat, segs=24, rings=16):
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=segs, v_segments=rings, radius=1.0)
    bm.transform(Matrix.Translation(c) @ Matrix.Diagonal((r * scale[0], r * scale[1], r * scale[2], 1.0)))
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    link(ob)
    me.materials.append(bpy.data.materials[mat])
    me.shade_smooth()
    return ob


def _box(name, c, size, mat, rot=Matrix.Identity(3)):
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bm.transform(Matrix.Translation(c) @ rot.to_4x4() @ Matrix.Diagonal((*size, 1.0)))
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    link(ob)
    me.materials.append(bpy.data.materials[mat])
    return ob


def _strap(name, pts, width, mat, lift=0.012):
    """A flat strap along `pts` (lifted `lift` towards -y... along each segment's normal)."""
    me = bpy.data.meshes.new(name)
    vs, fs = [], []
    for i, p in enumerate(pts):
        d = (pts[min(i + 1, len(pts) - 1)] - pts[max(i - 1, 0)]).normalized()
        side = d.cross(Vector((0, 1, 0))).normalized() * (width / 2)
        vs += [p - side, p + side]
        if i:
            k = 2 * i
            fs.append((k - 2, k - 1, k + 1, k))
    me.from_pydata(vs, [], fs)
    ob = bpy.data.objects.new(name, me)
    link(ob)
    ob.modifiers.new("solid", "SOLIDIFY").thickness = 0.006
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    ob.select_set(True)
    bpy.context.view_layer.objects.active = ob
    bpy.ops.object.modifier_apply(modifier="solid")
    me.materials.append(bpy.data.materials[mat])
    return ob


def build(parent, eye, shoulders, seat_z, pedal):
    """The driver's body under `parent` as one mesh "DriverBody". `eye`, `shoulders` (left,
    right): the rig's points; `seat_z`: the seat cushion's top; `pedal`: where the feet rest
    (y, z)."""
    materials()
    x = (shoulders[0].x + shoulders[1].x) / 2
    sh_y = (shoulders[0].y + shoulders[1].y) / 2
    sh_z = shoulders[0].z
    half = abs(shoulders[1].x - shoulders[0].x) / 2
    parts = []
    # Helmet: its centre a little behind and above the eye; dark visor across the front.
    hc = eye + Vector((0, -0.075, 0.045))
    parts.append(_sphere("helmet", hc, 0.125, (1.0, 1.12, 1.05), "DriverHelmet"))
    # Stripe over the crown (front to back).
    parts.append(_box("stripe", hc + Vector((0, -0.01, 0.128)), (0.05, 0.22, 0.012), "DriverHelmetStripe", Matrix.Rotation(math.radians(-8), 3, "X")))
    visor = _sphere("visor", hc + Vector((0, 0.012, 0.0)), 0.127, (1.0, 1.12, 1.05), "DriverVisor", 32, 20)
    bm = bmesh.new()
    bm.from_mesh(visor.data)
    c = hc + Vector((0, 0.012, 0.0))
    gone = [f for f in bm.faces if not ((f.calc_center_median() - c).normalized().y > 0.55 and -0.30 < (f.calc_center_median() - c).normalized().z < 0.38)]
    bmesh.ops.delete(bm, geom=gone, context="FACES")
    bm.to_mesh(visor.data)
    bm.free()
    parts.append(visor)
    # Chin bar under the visor.
    parts.append(_sphere("chin", hc + Vector((0, 0.07, -0.085)), 0.06, (1.35, 0.9, 0.7), "DriverHelmet"))
    # HANS: a collar on the shoulders, its upright behind the helmet.
    parts.append(_skin("hans", [(Vector((x - half * 0.55, sh_y + 0.10, sh_z + 0.03)), (0.035, 0.03)),
                                (Vector((x - half * 0.45, sh_y - 0.02, sh_z + 0.05)), (0.04, 0.03)),
                                (Vector((x, sh_y - 0.06, sh_z + 0.06)), (0.05, 0.03)),
                                (Vector((x + half * 0.45, sh_y - 0.02, sh_z + 0.05)), (0.04, 0.03)),
                                (Vector((x + half * 0.55, sh_y + 0.10, sh_z + 0.03)), (0.035, 0.03))], "DriverHans"))
    parts.append(_box("hans_back", Vector((x, sh_y - 0.07, sh_z + 0.13)), (0.10, 0.03, 0.16), "DriverHans", Matrix.Rotation(math.radians(-15), 3, "X")))
    # Neck.
    parts.append(_skin("neck", [(Vector((x, sh_y + 0.02, sh_z + 0.02)), (0.05, 0.05)), (hc + Vector((0, 0.0, -0.1)), (0.05, 0.05))], "DriverSuit"))
    # Torso: hips on the cushion, leaning back with the seat, shoulders at the rig's.
    hip = Vector((x, sh_y + 0.17, seat_z + 0.11))
    parts.append(_skin("torso", [
        (hip, (0.16, 0.12)),
        (hip.lerp(Vector((x, sh_y, sh_z)), 0.35) + Vector((0, 0.015, 0)), (0.15, 0.11)),
        (hip.lerp(Vector((x, sh_y, sh_z)), 0.7), (0.17, 0.11)),
        (Vector((x, sh_y, sh_z - 0.03)), (half + 0.03, 0.09)),
    ], "DriverSuit"))
    # Legs: thighs forward to the knees, shins down to the pedals, boots.
    for side in (-1, 1):
        hx = x + side * 0.095
        knee = Vector((hx + side * 0.02, pedal[0] - 0.40, seat_z + 0.20))
        ankle = Vector((hx + side * 0.01, pedal[0] - 0.05, pedal[1] + 0.06))
        parts.append(_skin("leg", [(Vector((hx, hip.y + 0.02, hip.z - 0.02)), (0.085, 0.085)),
                                   (knee, (0.065, 0.065)),
                                   (ankle, (0.045, 0.045))], "DriverSuit"))
        parts.append(_box("boot", ankle + Vector((0, 0.07, -0.03)), (0.085, 0.22, 0.08), "DriverBoot", Matrix.Rotation(math.radians(-25), 3, "X")))
    # Harness: shoulder straps over the shoulders down to the buckle, lap belt.
    buckle = hip + Vector((0, 0.13, 0.06))
    for side in (-1, 1):
        top = Vector((x + side * 0.09, sh_y - 0.05, sh_z + 0.06))
        front = Vector((x + side * 0.08, sh_y + 0.10, sh_z - 0.02))
        mid = front.lerp(buckle, 0.5) + Vector((0, 0.055, 0))
        parts.append(_strap("strap", [top, front, mid, buckle + Vector((side * 0.02, 0.0, 0.0))], 0.05, "DriverStrap"))
    parts.append(_strap("lap", [buckle + Vector((-0.17, -0.10, -0.04)), buckle + Vector((-0.08, 0.0, 0)), buckle + Vector((0.08, 0.0, 0)), buckle + Vector((0.17, -0.10, -0.04))], 0.05, "DriverStrap"))
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    for p in parts:
        p.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    ob = bpy.context.view_layer.objects.active
    ob.name = ob.data.name = "DriverBody"
    ob.parent = parent
    ob.data.calc_loop_triangles()
    print(f"driver body: {len(ob.data.loop_triangles)} triangles")
    return ob


def seat_harness(parent, x):
    """An empty seat's harness: shoulder straps down the seat back from its slots, lap belts
    lying on the cushion's sides, as one mesh "SeatHarness" (the passenger's seat, whose
    occupant the game draws itself)."""
    materials()
    parts = []
    for side in (-1, 1):
        sx = x + side * 0.075
        pts = [Vector((sx, -0.935, 0.50)), Vector((sx, -0.885, 0.30)), Vector((sx, -0.82, 0.10)), Vector((sx + side * 0.01, -0.765, -0.06)), Vector((x + side * 0.03, -0.66, -0.135))]
        parts.append(_strap("hstrap", pts, 0.05, "DriverStrap"))
        parts.append(_strap("hlap", [Vector((x + side * 0.215, -0.70, -0.06)), Vector((x + side * 0.20, -0.62, -0.13)), Vector((x + side * 0.12, -0.55, -0.145))], 0.05, "DriverStrap"))
    parts.append(_box("buckle", Vector((x, -0.62, -0.13)), (0.07, 0.07, 0.012), "DriverHans"))
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    for p in parts:
        p.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    ob = bpy.context.view_layer.objects.active
    ob.name = ob.data.name = "SeatHarness"
    ob.parent = parent
    return ob
