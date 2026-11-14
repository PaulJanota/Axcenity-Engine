"""Extracts the meshes the default humanoid is built on from Blender
Studio's Human Base Meshes bundle (CC0 1.0) into plain OBJ files here.

Only needed to refresh the vendored files; `build.py` reads the OBJ files.

    python extract.py path/to/human_base_meshes_bundle.blend

The bundle: https://download.blender.org/demo/asset-bundles/human-base-meshes/human-base-meshes-bundle-v1.4.1.zip
(v1.4.1, SHA-256 811f43accbb31a88266d932f8f5563b2d13586fca0ba2693aad1f5fe582b3515).

Each body's sculpt face sets are written as OBJ materials named
`fs_<id>`, so the build can find regions (the eyelid ring, the lips, each
finger joint) by set. The two bodies share one topology: vertex `i` of
one is vertex `i` of the other.
"""

import os
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))

# Object in the bundle -> file here.
BODIES = {
    "GEO-body_male_stylized": "body_masculine.obj",
    "GEO-body_female_stylized": "body_feminine.obj",
}
EYES = {
    "GEO-body_male_stylized": "eyes_masculine.obj",
    "GEO-body_female_stylized": "eyes_feminine.obj",
}
TEETH = ("Jaw - Stylized ", "teeth.obj")


def face_set_materials(obj):
    me = obj.data
    sets = [d.value for d in me.attributes[".sculpt_face_set"].data]
    ids = sorted(set(sets))
    me.materials.clear()
    for i in ids:
        me.materials.append(bpy.data.materials.get(f"fs_{i}") or bpy.data.materials.new(f"fs_{i}"))
    for poly, s in zip(me.polygons, sets):
        poly.material_index = ids.index(s)


def export(objects, name):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objects:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.wm.obj_export(
        filepath=os.path.join(HERE, name), export_selected_objects=True,
        export_materials=True, export_uv=True, export_normals=False,
        export_triangulated_mesh=False, apply_modifiers=False,
        export_object_groups=True, path_mode="STRIP")


def main(blend):
    bpy.ops.wm.open_mainfile(filepath=blend)
    for obj in bpy.data.objects:
        obj.animation_data_clear()
        for mod in list(obj.modifiers):
            obj.modifiers.remove(mod)
    for body, file in BODIES.items():
        obj = bpy.data.objects[body]
        obj.location = (0, 0, 0)
        face_set_materials(obj)
        export([obj], file)
        eyes = [bpy.data.objects[f"{body}.eye.L"], bpy.data.objects[f"{body}.eye.R"]]
        for eye in eyes:
            eye.data.materials.clear()
        export(eyes, EYES[body])
    teeth = bpy.data.objects[TEETH[0]]
    teeth.location = (0, 0, 0)
    teeth.data.materials.clear()
    export([teeth], TEETH[1])


if __name__ == "__main__":
    args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
    main(args[0])
