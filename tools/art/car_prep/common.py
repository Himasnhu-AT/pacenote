# Helpers for the car prep files (run by import_sketchfab.py --prep, before its own steps).
import re

import bpy


def merge_materials(objs, table):
    """Renames and merges materials: table [(REGEX, NEW_NAME)], first match wins. Every
    material matching a rule is replaced by the first one found (renamed NEW_NAME), so
    variants like X_0, X_1 become one material. Names carry no makers' or brands' names."""
    keep = {}
    for o in objs:
        for slot in o.material_slots:
            mt = slot.material
            if mt is None:
                continue
            new = next((n for rx, n in table if re.search(rx, mt.name, re.I)), None)
            if new is None:
                continue
            if new not in keep:
                keep[new] = mt
                mt.name = new
            slot.material = keep[new]
    for mt in list(bpy.data.materials):
        if not mt.users:
            bpy.data.materials.remove(mt)
    print("merged materials:", sorted(keep))


def drop_by_material(objs, rx):
    """Removes the objects using a material whose name matches; returns the rest."""
    rx = re.compile(rx, re.I)
    out = []
    for o in objs:
        if any(s.material and rx.search(s.material.name) for s in o.material_slots):
            bpy.data.objects.remove(o, do_unlink=True)
        else:
            out.append(o)
    return out
