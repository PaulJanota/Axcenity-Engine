"""Shared helpers for the humanoid build: loading the vendored base meshes,
face sets, small vector maths and object plumbing.

Units are metres, Blender Z up, the character faces -Y (its left is +X).
"""

import math
import os

import bpy
import bmesh
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(HERE, "source")

# The character's height (crown) in metres: 5 studs in the engine.
HEIGHT = 1.75
# The relaxed A-pose: arms 45 degrees below horizontal (default-humanoid D6).
ARM_DOWN = math.radians(45.0)

# Sculpt face sets of the base bodies (`source/extract.py` writes them as
# materials `fs_<id>`). The two frames share them.
FS_NECK = 6
FS_LID_OUTER = 27       # eyelid, outer ring
FS_LID_INNER = 28       # eyelid margin, rolling into the socket
FS_EAR = 29
FS_SOCKET = 37          # the socket behind the eyeball
FS_FACE = 44            # forehead, cheeks and temples
FS_NOSE = 45
FS_JAW_SIDE = 46
FS_UPPER_LIP = 47
FS_LOWER_LIP = 48
FS_UNDER_JAW = 49
FS_THIGH = 256
FS_SHIN = 257
FS_PELVIS = 258
FS_TORSO = 259
FS_SHOULDERS = 260
FS_FOREARM = 262
FS_UPPER_ARM = 263
FS_PALM = 273
FS_ANKLE = 288
FS_FOOT = 316
FS_TOE_BASE = 317
# Fingers: (proximal, intermediate, distal) sets.
FS_FINGERS = {
    "Index": (278, 283, 282),
    "Middle": (277, 279, 272),
    "Ring": (280, 274, 275),
    "Little": (281, 286, 276),
    "Thumb": (359, 285, 284),
}

SIDES = (("Left", 1), ("Right", -1))


def v(x, y, z):
    return Vector((x, y, z))


def lerp(a, b, t):
    return a + (b - a) * t


def smoothstep(e0, e1, x):
    if e0 == e1:
        return 1.0 if x >= e1 else 0.0
    t = max(0.0, min(1.0, (x - e0) / (e1 - e0)))
    return t * t * (3 - 2 * t)


def centroid(points):
    points = list(points)
    total = Vector((0.0, 0.0, 0.0))
    for p in points:
        total += p
    return total / max(1, len(points))


def reset_scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.unit_settings.system = "METRIC"
    return scene


def import_obj(name):
    """Imports `source/<name>`, its object transform applied to the mesh
    (the OBJ importer turns Y up into an object rotation)."""
    before = set(bpy.data.objects)
    bpy.ops.wm.obj_import(filepath=os.path.join(SOURCE, name))
    objs = [o for o in bpy.data.objects if o not in before]
    for o in objs:
        o.data.transform(o.matrix_world)
        o.matrix_world = Matrix.Identity(4)
    return objs


def face_sets(obj):
    """The face set of each polygon, from the `fs_<id>` materials."""
    ids = [int(m.name.split("_")[1].split(".")[0]) for m in obj.data.materials]
    return [ids[p.material_index] for p in obj.data.polygons]


def positions(obj):
    return [vert.co.copy() for vert in obj.data.vertices]


def set_positions(obj, points):
    for vert, p in zip(obj.data.vertices, points):
        vert.co = p
    obj.data.update()


def new_object(name, mesh):
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def mesh_from(name, verts, faces, uvs=None):
    """A mesh from vertex positions, polygons (index lists) and optional
    per-corner uvs (one list of (u, v) per polygon)."""
    me = bpy.data.meshes.new(name)
    me.from_pydata([tuple(p) for p in verts], [], [tuple(f) for f in faces])
    if uvs is not None:
        layer = me.uv_layers.new(name="UVMap")
        i = 0
        for poly, corner_uvs in zip(me.polygons, uvs):
            for uv in corner_uvs:
                layer.data[i].uv = uv
                i += 1
    me.update()
    return me


def shade_smooth(obj):
    for p in obj.data.polygons:
        p.use_smooth = True


def select_only(objs, active=None):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = active or objs[0]


def tri_count(obj):
    return sum(len(p.vertices) - 2 for p in obj.data.polygons)


def edge_neighbours(mesh):
    """Vertex index -> set of neighbouring vertex indices."""
    nbr = [set() for _ in mesh.vertices]
    for e in mesh.edges:
        a, b = e.vertices
        nbr[a].add(b)
        nbr[b].add(a)
    return nbr


def smooth_field(values, nbr, iterations, keep=None):
    """Laplacian smoothing of a per-vertex scalar field."""
    vals = list(values)
    for _ in range(iterations):
        nxt = list(vals)
        for i, ns in enumerate(nbr):
            if keep is not None and keep[i]:
                continue
            if ns:
                nxt[i] = 0.5 * vals[i] + 0.5 * sum(vals[n] for n in ns) / len(ns)
        vals = nxt
    return vals


def remesh(obj, voxel, smooth_iterations=6, smooth_factor=0.5, max_triangles=None, thicken=0.0):
    """Merges overlapping pieces of `obj` into one closed surface (a voxel
    remesh), relaxes it, and decimates it to at most `max_triangles`. The
    materials come back from the nearest face of the original. `thicken`
    first gives open surfaces a wall that thick (a remesh needs volumes)."""
    from mathutils.bvhtree import BVHTree

    if thicken:
        select_only([obj])
        mod = obj.modifiers.new("Solidify", "SOLIDIFY")
        mod.thickness = thicken
        mod.offset = -1.0
        bpy.ops.object.modifier_apply(modifier="Solidify")
    source = obj.copy()
    source.data = obj.data.copy()
    bpy.context.scene.collection.objects.link(source)
    deps = bpy.context.evaluated_depsgraph_get()
    bvh = BVHTree.FromObject(source, deps)
    mats = list(obj.data.materials)
    src_index = [p.material_index for p in source.data.polygons]
    obj.data.remesh_voxel_size = voxel
    obj.data.remesh_voxel_adaptivity = 0.0
    select_only([obj])
    bpy.ops.object.voxel_remesh()
    if smooth_iterations:
        mod = obj.modifiers.new("Smooth", "SMOOTH")
        mod.factor = smooth_factor
        mod.iterations = smooth_iterations
        bpy.ops.object.modifier_apply(modifier="Smooth")
    if max_triangles and tri_count(obj) > max_triangles:
        mod = obj.modifiers.new("Decimate", "DECIMATE")
        mod.ratio = max_triangles / tri_count(obj)
        bpy.ops.object.modifier_apply(modifier="Decimate")
    obj.data.materials.clear()
    for m in mats:
        obj.data.materials.append(m)
    for poly in obj.data.polygons:
        loc, _, index, _ = bvh.find_nearest(poly.center)
        poly.material_index = src_index[index] if index is not None else 0
        poly.use_smooth = True
    bpy.data.objects.remove(source)
    return obj


def bm_of(obj):
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bm.verts.ensure_lookup_table()
    bm.edges.ensure_lookup_table()
    bm.faces.ensure_lookup_table()
    return bm
