# Prep for the "kestrel" body (a 2024 GT3-class racer, Sketchfab CC BY, see SOURCES.md).
# Run by import_sketchfab.py --profile kestrel. Drops the cabin, banners and motion-blur
# wheel cards; merges material variants under our own names (no brand names). The printed
# livery, the crest and script decals and the tyre sidewall text go in the profile's --paint
# and --plain options.
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from common import drop_by_material, merge_materials  # noqa: E402

meshes = drop_by_material(meshes, r"^INT_|BELT_|SPARCO|PHISICS|DISPLAY|LED_RACE|STEER_|SEAT_|WorldGrid|Banner|RIM_BLUR|^EXT_RIM(_[135])?$|^MIRROR$")
merge_materials(meshes, [
    (r"^EXT_Carpaint", "BodyPaint"),
    (r"^MI_Tyre", "Tyre"),
    (r"^EXT_RIM", "Rim"),
    (r"^EXT_Disc", "Disc"),
    (r"^EXT_CALIPER", "Caliper"),
    (r"^EXT_Details", "Details"),
    (r"^EXT_Mechanics", "Mechanics"),
    (r"^EXT_Emissive_Light_Front", "HeadLamp"),
    (r"^EXT_Emissive_Light_Rear", "TailLamp"),
    (r"^EXT_Glass_Emissive", "LampLens"),
    (r"^EXT_Windows", "Glass"),
    (r"^AUX_LIGHT", "AuxLamp"),
    (r"^EXT_EXALTS", "Exhaust"),
    (r"^EXT_Grid", "Mesh"),
    (r"^EXT_RADIATOR", "Radiator"),
    (r"^EXT_TAMP_FUEL", "FuelCap"),
    (r"^CHASSIS_KEVLAR", "Underbody"),
])
