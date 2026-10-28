# Prepares the Car Concept glTF (CC BY 4.0, Eric Chadwick / Darmstadt Graphics Group; see
# assets/credits.txt) for the game (Blender, background):
#   blender -b --factory-startup -P tools/art/build_car.py -- <CarConcept.glb> <assets/models/car.glb>
# - scaled to the simulation's wheelbase (src/vehicle.rs Spec: 2.47 m), nose towards -Z (game
#   forward), the body origin at the wheelbase centre, wheel hubs at the axle height;
# - body meshes joined per material (fewer draws), axles removed;
# - each wheel as `Hub<FL|FR|RL|RR>` (steer + suspension; brake caliper) → `Spin<..>` (tyre,
#   rim, disc) with pivots at the wheel centre;
# - the trademarked logos removed: the steering-wheel emblem is deleted, the licence plate gets a
#   plain texture-free material, the "Hardware" material loses its emissive (logo) texture;
# - a rally livery (paint colours) and textures downsized to 1024 px.
import sys

import bpy
import numpy as np
from mathutils import Matrix, Vector

argv = sys.argv[sys.argv.index("--") + 1:]
SRC, OUT = argv[0], argv[1]
WHEELBASE = 2.47
# Livery: the main panels ("Paint 1") deep metallic blue; the trim ("Paint 2") stays graphite.
PAINT1 = (0.03, 0.12, 0.42, 1.0)

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=SRC)


def world_bbox(objs):
    pts = [o.matrix_world @ v.co for o in objs if o.type == "MESH" for v in o.data.vertices]
    a = np.array(pts)
    return a.min(0), a.max(0)


wheels = {"FL": "WheelFrontL", "FR": "WheelFrontR", "RL": "WheelRearL", "RR": "WheelRearR"}
centers = {}
for k, n in wheels.items():
    lo, hi = world_bbox([c for c in bpy.data.objects[n].children_recursive if c.type == "MESH"])
    centers[k] = Vector(((lo + hi) / 2).tolist())
front_y = (centers["FL"].y + centers["FR"].y) / 2
rear_y = (centers["RL"].y + centers["RR"].y) / 2
scale = WHEELBASE / abs(rear_y - front_y)
hub_z = centers["FL"].z
mid_y = (front_y + rear_y) / 2
radius = max(world_bbox([o for o in bpy.data.objects["WheelFrontL"].children_recursive if o.type == "MESH" and "Mesh" in o.name])[1][2] - hub_z, 0.1)

# Remove the logo emblem and the axles; strip logo textures.
for n in ("InteriorSteeringEmblem", "Axles"):
    o = bpy.data.objects.get(n)
    if o:
        bpy.data.objects.remove(o, do_unlink=True)
for m in bpy.data.materials:
    if not m.use_nodes:
        continue
    bsdf = next((n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
    if bsdf is None:
        continue
    if m.name == "License":
        for l in list(bsdf.inputs["Base Color"].links):
            m.node_tree.links.remove(l)
        bsdf.inputs["Base Color"].default_value = (0.85, 0.82, 0.62, 1.0)
        bsdf.inputs["Roughness"].default_value = 0.5
    # Many materials sample the plate image (logos) as an "emissive" through a texture
    # transform Bevy ignores: no emission except the lamps.
    if m.name not in ("Signallight", "Brakelight", "Headlight"):
        for l in list(bsdf.inputs["Emission Color"].links):
            m.node_tree.links.remove(l)
        bsdf.inputs["Emission Strength"].default_value = 0.0
    if m.name.startswith("Paint"):
        # The paint's flake normal map relies on a 30x texture transform Bevy ignores (it
        # sparkles as big flakes): drop it. Partly metallic so the colour reads.
        for l in list(bsdf.inputs["Normal"].links):
            m.node_tree.links.remove(l)
        bsdf.inputs["Metallic"].default_value = 0.35
        # Bevy 0.19's clearcoat lobe washes the paint out to white here: no coat (the low
        # roughness keeps the shine).
        bsdf.inputs["Coat Weight"].default_value = 0.0
    if m.name == "Paint 1 Carmine":
        bsdf.inputs["Base Color"].default_value = PAINT1

# One paint scheme only: drop the material variants (Bevy shows an arbitrary one).
sc = bpy.context.scene
if hasattr(sc, "gltf2_KHR_materials_variants_variants"):
    sc.gltf2_KHR_materials_variants_variants.clear()
for me in bpy.data.meshes:
    if hasattr(me, "gltf2_variant_mesh_data"):
        me.gltf2_variant_mesh_data.clear()

# Bake every object's world transform into its mesh, unparented (so joins and re-parents are exact).
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.make_single_user(object=True, obdata=True)
for o in list(bpy.data.objects):
    if o.type == "MESH":
        mw = o.matrix_world.copy()
        o.parent = None
        o.matrix_world = mw
bpy.ops.object.select_all(action="DESELECT")
for o in bpy.data.objects:
    if o.type == "MESH":
        o.select_set(True)
bpy.context.view_layer.objects.active = next(o for o in bpy.data.objects if o.type == "MESH")
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

# Game frame: origin at (wheelbase centre, hub height), nose towards Blender +Y (= glTF -Z), scaled.
to_game = Matrix.Scale(scale, 4) @ Matrix.Rotation(np.pi, 4, "Z") @ Matrix.Translation((0, -mid_y, -hub_z))
for o in bpy.data.objects:
    if o.type == "MESH":
        o.data.transform(to_game)
        o.matrix_world = Matrix.Identity(4)
        o.data.update()
bpy.context.view_layer.update()

wheel_parts = {}
for k, n in wheels.items():
    wheel_parts[k] = [c for c in bpy.data.objects if c.type == "MESH" and (c.name.startswith(n) or c.name in ())]
# The tyre meshes are unnamed children ("Mesh_89" …): pick them by position.
for o in bpy.data.objects:
    if o.type != "MESH" or not o.name.startswith("Mesh_"):
        continue
    lo, hi = world_bbox([o])
    c = Vector(((lo + hi) / 2).tolist())
    best = min(wheels, key=lambda k: (Vector((-(centers[k].x) * scale, -(centers[k].y - mid_y) * scale, 0)) - Vector((c.x, c.y, 0))).length)
    wheel_parts[best].append(o)

root = bpy.data.objects.new("Car", None)
bpy.context.scene.collection.objects.link(root)
coll = bpy.context.scene.collection


def join(objs, name):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    if len(objs) > 1:
        bpy.ops.object.join()
    o = bpy.context.view_layer.objects.active
    o.name = name
    return o


for k, parts in wheel_parts.items():
    lo, hi = world_bbox(parts)
    c = Vector(((lo + hi) / 2).tolist())
    c.z = 0.0
    hub = bpy.data.objects.new(f"Hub{k}", None)
    coll.objects.link(hub)
    hub.location = c
    hub.parent = root
    spin = bpy.data.objects.new(f"Spin{k}", None)
    coll.objects.link(spin)
    spin.parent = hub
    rolling = [p for p in parts if "BrakePad" not in p.name]
    pads = [p for p in parts if "BrakePad" in p.name]
    for group, parent, name in ((rolling, spin, f"Wheel{k}"), (pads, hub, f"Caliper{k}")):
        if not group:
            continue
        o = join(group, name)
        o.data.transform(Matrix.Translation(-c))
        o.parent = parent
        o.matrix_parent_inverse = Matrix.Identity(4)
        o.location = (0, 0, 0)

# Everything else is the body: one object with a material slot per material.
body = [o for o in bpy.data.objects if o.type == "MESH" and o.parent is None]
b = join(body, "Body")
b.parent = root

# Textures ≤ 1024 px.
for img in bpy.data.images:
    if img.size[0] > 1024 or img.size[1] > 1024:
        f = 1024 / max(img.size)
        img.scale(max(1, int(img.size[0] * f)), max(1, int(img.size[1] * f)))

bpy.ops.object.select_all(action="DESELECT")
tris = sum(len(o.data.polygons) for o in bpy.data.objects if o.type == "MESH")
print(f"car: scale {scale:.4f}, wheel radius {radius * scale:.3f} m, tris {tris}")
for k in wheels:
    print("hub", k, tuple(round(v, 3) for v in bpy.data.objects[f"Hub{k}"].location))
bpy.ops.export_scene.gltf(filepath=OUT, export_format="GLB", export_yup=True, export_image_format="AUTO", export_apply=False, use_selection=False)
