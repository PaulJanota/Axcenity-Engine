"""Shared helpers for the humanoid QA scripts (`measure.py`, `render_baseline.py`).

Loads a humanoid glTF with Blender's importer, sets a look (frame, build,
face and expression shapes, clothing masks, the bone offsets the
customization file lists), poses the skeleton with fixed test poses, gives
deformed geometry for measuring, and sets up one fixed studio for renders.

Blender space: metres, Z up, the character faces -Y, its left is +X.
"""

import json
import math
import os

import bpy
import numpy as np
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

QA_DIR = os.path.dirname(os.path.abspath(__file__))
ASSET_DIR = os.path.dirname(QA_DIR)
CUSTOM_PATH = os.path.join(ASSET_DIR, "Humanoid.customization.json")

FWD = Vector((0, -1, 0))
BACK = Vector((0, 1, 0))
UP = Vector((0, 0, 1))
DOWN = Vector((0, 0, -1))
SIDES = (("Left", 1), ("Right", -1))
FINGERS = ("Index", "Middle", "Ring", "Little")
JAW_OPEN_DEG = 25.0  # crates/assets/src/face_profile.rs: jaw_open limit

BUILDS = ("Build_Heavy", "Build_Slim", "Build_Muscular")


def load_custom(path=CUSTOM_PATH):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def gltf_to_blender(d):
    """A glTF-space vector (Y up, front +Z) in Blender space (Z up, front -Y)."""
    return Vector((d[0], -d[2], d[1]))


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def update():
    bpy.context.view_layer.update()


class Character:
    """One imported humanoid file: the armature and its mesh parts."""

    def __init__(self, path, merge_vertices=False, custom=None):
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=path, merge_vertices=merge_vertices)
        new = [o for o in bpy.data.objects if o not in before]
        self.path = path
        self.arm = next(o for o in new if o.type == "ARMATURE")
        self.parts = {o.name: o for o in new if o.type == "MESH" and o.parent == self.arm}
        for o in new:  # the importer's bone-shape icosphere
            if o.type == "MESH" and o.parent is None:
                o.hide_render = True
                o.hide_viewport = True
        self.custom = custom or load_custom()
        self.rest = {b.name: (b.head_local.copy(), b.tail_local.copy()) for b in self.arm.data.bones}
        self.bone_frame = 0.0
        self.look = {"frame": 0.0, "face": {}}
        self.defaults = {}
        for name, o in self.parts.items():
            sk = o.data.shape_keys
            if sk:
                self.defaults[name] = {kb.name: kb.value for kb in sk.key_blocks[1:]}
                for kb in sk.key_blocks[1:]:
                    kb.slider_min = -1.0

    @staticmethod
    def _mode(obj, mode):
        for o in bpy.context.view_layer.objects:
            o.select_set(False)
        bpy.context.view_layer.objects.active = obj
        obj.select_set(True)
        bpy.ops.object.mode_set(mode=mode)

    def show_only(self, names):
        """Hides every part not in `names` (renders and the viewport)."""
        for n, o in self.parts.items():
            hide = n not in names
            o.hide_render = hide
            o.hide_viewport = hide

    # -- the look --------------------------------------------------------------------

    def masks_for(self, garments):
        out = []
        for g in garments:
            for m in self.custom["parts"].get(g, {}).get("masks", []):
                if m not in out:
                    out.append(m)
        return out

    def set_look(self, frame=0.0, build=None, face=None, expr=None, masks=None, keep_defaults=False):
        """Shape weights on every part that carries them; everything else 0
        (or the file's default weight with `keep_defaults`). `build` is
        {Build_*: w}; `face` {Face_*: w}; `expr` {expression: w};
        `masks` a list of mask names at 1. Also moves the skeleton for the
        frame and the eye bones for the frame and eye identity shapes."""
        w = {"Frame_Feminine": frame}
        w.update(build or {})
        w.update(face or {})
        w.update(expr or {})
        for m in masks or []:
            w[m] = 1.0
        for name, o in self.parts.items():
            sk = o.data.shape_keys
            if not sk:
                continue
            for kb in sk.key_blocks[1:]:
                base = self.defaults[name].get(kb.name, 0.0) if keep_defaults else 0.0
                kb.value = w.get(kb.name, base)
        self.set_frame_bones(frame)
        self.look = {"frame": frame, "face": dict(face or {})}
        self.set_eyes()
        update()

    def set_frame_bones(self, frame):
        """Moves the rest skeleton by Frame_Feminine's bone offsets (eyes
        are moved in the pose, `set_eyes`). The mesh at rest is unchanged;
        poses then turn about the feminine joints."""
        if abs(frame - self.bone_frame) < 1e-9:
            return
        offs = {b: gltf_to_blender(c["offset"]) for b, c in
                self.custom["body"]["Frame_Feminine"]["bones"].items() if "offset" in c}
        children = {}
        for b in self.arm.data.bones:
            if b.parent:
                children.setdefault(b.parent.name, []).append(b.name)
        self._mode(self.arm, "EDIT")
        ebs = self.arm.data.edit_bones
        for eb in ebs:
            eb.use_connect = False
        for eb in ebs:
            if eb.name in ("LeftEye", "RightEye"):
                continue
            h0, t0 = self.rest[eb.name]
            oh = offs.get(eb.name, Vector())
            ot = oh
            for c in children.get(eb.name, []):
                if (self.rest[c][0] - t0).length < 1e-4:
                    ot = offs.get(c, Vector())
                    break
            eb.head = h0 + oh * frame
            eb.tail = t0 + ot * frame
        self._mode(self.arm, "OBJECT")
        self.bone_frame = frame

    def eye_transform(self, side):
        """(offset, scale) of an eye bone for the current look."""
        name = f"{side}Eye"
        frame = self.look["frame"]
        fem = self.custom["body"]["Frame_Feminine"]
        off = Vector()
        scale = 1.0 + (fem.get("eye_scale", 1.0) - 1.0) * frame
        if name in fem["bones"]:
            off += gltf_to_blender(fem["bones"][name]["offset"]) * frame
        for fname, wt in self.look["face"].items():
            c = self.custom["face"].get(fname, {}).get("bones", {}).get(name)
            if not c:
                continue
            if "offset" in c:
                off += gltf_to_blender(c["offset"]) * wt
            if "scale" in c:
                scale *= 1.0 + (c["scale"] - 1.0) * wt
        return off, scale

    def set_eyes(self):
        for side, _ in SIDES:
            name = f"{side}Eye"
            pb = self.arm.pose.bones.get(name)
            if pb is None:
                continue
            off, s = self.eye_transform(side)
            R = self.arm.data.bones[name].matrix_local
            c = R.to_translation()
            X = Matrix.Translation(c + off) @ Matrix.Scale(s, 4) @ Matrix.Translation(-c)
            pb.matrix_basis = R.inverted() @ X @ R

    def eye_centre(self, side):
        off, _ = self.eye_transform(side)
        return self.rest[f"{side}Eye"][0] + off

    # -- posing ----------------------------------------------------------------------

    def head(self, bone):
        return self.arm.data.bones[bone].head_local.copy()

    def tail(self, bone):
        return self.arm.data.bones[bone].tail_local.copy()

    def pose_reset(self):
        for pb in self.arm.pose.bones:
            pb.matrix_basis = Matrix()
        self.set_eyes()
        update()

    def bend(self, bone, axis, deg, toward=None):
        """Turns `bone` by `deg` about `axis` (armature space, through its
        rest head), composed on its current local turn. With `toward`, the
        sign is the one that moves the bone's tail further that way."""
        b = self.arm.data.bones[bone]
        R = b.matrix_local
        h = R.to_translation()
        axis = Vector(axis).normalized()

        def X(a):
            return Matrix.Translation(h) @ Matrix.Rotation(math.radians(a), 4, axis) @ Matrix.Translation(-h)

        if toward is not None:
            t = b.tail_local
            if ((X(deg) @ t) - t).dot(Vector(toward)) < ((X(-deg) @ t) - t).dot(Vector(toward)):
                deg = -deg
        pb = self.arm.pose.bones[bone]
        pb.matrix_basis = (R.inverted() @ X(deg) @ R) @ pb.matrix_basis

    def palm_normal(self, side, s):
        wrist = self.head(f"{side}Hand")
        knuckle = self.head(f"{side}MiddleProximal")
        along = (knuckle - wrist).normalized()
        across = (self.head(f"{side}IndexProximal") - self.head(f"{side}LittleProximal")).normalized()
        return (along.cross(across) * s).normalized()


# -- test poses ------------------------------------------------------------------------
# Angles are joint flexion from straight (elbow 135 = forearm turned 135
# degrees toward the biceps). Limb poses use the left side; `both` mirrors.

def _elbow(ch, deg, sides=("Left",)):
    for side in sides:
        d = (ch.head(f"{side}LowerArm") - ch.head(f"{side}UpperArm")).normalized()
        ch.bend(f"{side}LowerArm", d.cross(FWD), deg, toward=FWD)


def _knee(ch, deg, sides=("Left",)):
    for side in sides:
        d = (ch.head(f"{side}LowerLeg") - ch.head(f"{side}UpperLeg")).normalized()
        ch.bend(f"{side}LowerLeg", d.cross(FWD), deg, toward=BACK)


def _hip(ch, deg, sides=("Left",)):
    for side in sides:
        d = (ch.head(f"{side}LowerLeg") - ch.head(f"{side}UpperLeg")).normalized()
        ch.bend(f"{side}UpperLeg", d.cross(FWD), deg, toward=FWD)


def arm_abduction(ch, side):
    d = (ch.head(f"{side}LowerArm") - ch.head(f"{side}UpperArm")).normalized()
    return math.degrees(math.acos(max(-1.0, min(1.0, -d.z))))


def _shoulder(ch, target, sides=("Left",)):
    """Upper arm abducted (in the frontal plane) to `target` degrees from
    hanging; 90 is horizontal. The whole turn is at the shoulder joint."""
    for side in sides:
        ch.bend(f"{side}UpperArm", (0, 1, 0), target - arm_abduction(ch, side), toward=UP)


def _wrist(ch, deg, sides=("Left",)):
    for side, s in SIDES:
        if side not in sides:
            continue
        along = (ch.head(f"{side}MiddleProximal") - ch.head(f"{side}Hand")).normalized()
        pn = ch.palm_normal(side, s)
        ch.bend(f"{side}Hand", along.cross(pn), deg, toward=pn)


def _spine(ch, deg):
    for b in ("Spine", "Chest", "UpperChest"):
        ch.bend(b, UP, deg / 3.0)


def _fist(ch, sides=("Left", "Right")):
    curl = {"Proximal": 85, "Intermediate": 100, "Distal": 70}
    thumb = {"Proximal": 25, "Intermediate": 40, "Distal": 45}
    for side, s in SIDES:
        if side not in sides:
            continue
        pn = ch.palm_normal(side, s)
        for f in FINGERS + ("Thumb",):
            for seg in ("Proximal", "Intermediate", "Distal"):
                b = f"{side}{f}{seg}"
                d = (ch.tail(b) - ch.head(b)).normalized()
                ang = (thumb if f == "Thumb" else curl)[seg]
                ch.bend(b, d.cross(pn), ang, toward=pn)


def jaw_open(ch, amount=1.0):
    ch.bend("Jaw", (1, 0, 0), JAW_OPEN_DEG * amount, toward=DOWN)


# name -> (function, the joint to measure: (pivot bone, parent bone, radius m))
POSES = {
    "rest": (lambda ch: None, None),
    "elbow_135": (lambda ch: _elbow(ch, 135), ("LeftLowerArm", "LeftUpperArm", 0.075)),
    "knee_135": (lambda ch: _knee(ch, 135), ("LeftLowerLeg", "LeftUpperLeg", 0.10)),
    "shoulder_abd_90": (lambda ch: _shoulder(ch, 90), ("LeftUpperArm", "LeftShoulder", 0.09)),
    "shoulder_abd_150": (lambda ch: _shoulder(ch, 150), ("LeftUpperArm", "LeftShoulder", 0.09)),
    "hip_flex_100": (lambda ch: _hip(ch, 100), ("LeftUpperLeg", "Hips", 0.12)),
    "wrist_70": (lambda ch: _wrist(ch, 70), ("LeftHand", "LeftLowerArm", 0.05)),
    "spine_twist_45": (lambda ch: _spine(ch, 45), ("Chest", "Spine", 0.16)),
    "fist": (lambda ch: _fist(ch), ("LeftMiddleProximal", "LeftHand", 0.06)),
}


def apply_pose(ch, name):
    ch.pose_reset()
    POSES[name][0](ch)
    update()


# -- deformed geometry -----------------------------------------------------------------

class Geo:
    """An object's deformed (shape keys + skin) triangles in world space."""

    def __init__(self, obj):
        dg = bpy.context.evaluated_depsgraph_get()
        ev = obj.evaluated_get(dg)
        me = ev.to_mesh()
        n = len(me.vertices)
        co = np.empty(n * 3, dtype=np.float64)
        me.vertices.foreach_get("co", co)
        co = co.reshape(n, 3)
        M = np.array(obj.matrix_world)
        self.co = co @ M[:3, :3].T + M[:3, 3]
        me.calc_loop_triangles()
        t = np.empty(len(me.loop_triangles) * 3, dtype=np.int64)
        me.loop_triangles.foreach_get("vertices", t)
        self.tris = t.reshape(-1, 3)
        nrm = np.empty(n * 3, dtype=np.float64)
        me.vertex_normals.foreach_get("vector", nrm)
        self.nrm = nrm.reshape(n, 3) @ M[:3, :3].T
        ln = np.linalg.norm(self.nrm, axis=1, keepdims=True)
        self.nrm = self.nrm / np.maximum(ln, 1e-12)
        ev.to_mesh_clear()
        self._bvh = None

    @property
    def bvh(self):
        if self._bvh is None:
            self._bvh = BVHTree.FromPolygons([tuple(p) for p in self.co], [tuple(t) for t in self.tris],
                                             all_triangles=True)
        return self._bvh

    def tri_normals(self):
        a, b, c = (self.co[self.tris[:, k]] for k in range(3))
        n = np.cross(b - a, c - a)
        return n / np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-18)


def merged_bvh(geos):
    """One BVH over several Geo objects (for 'any garment')."""
    verts, tris, off = [], [], 0
    for g in geos:
        verts.extend(tuple(p) for p in g.co)
        tris.extend(tuple(int(i) + off for i in t) for t in g.tris)
        off += len(g.co)
    return BVHTree.FromPolygons(verts, tris, all_triangles=True)


def signed_distance(bvh, p, max_dist=0.1):
    """(p - nearest) . face normal at the nearest point, or None."""
    loc, nrm, _, d = bvh.find_nearest(Vector(p), max_dist)
    if loc is None:
        return None
    return (Vector(p) - loc).dot(nrm)


POKE_MAX = 0.015


def cover_test(bvh, p, n, out_window=0.05, in_window=0.03, poke_max=POKE_MAX):
    """Is skin point `p` (outward normal `n`) under the layer in `bvh`?
    'covered' (the layer is outside it within `out_window`), 'poke' (the
    layer's outward face is up to `poke_max` inside it: the skin shows
    through the layer), 'exposed' (that face is deeper, up to `in_window`:
    the skin is bare over an opening, such as a shoe's throat), or None."""
    p, n = Vector(p), Vector(n)
    loc, nrm, _, d = bvh.ray_cast(p + n * 1e-5, n, out_window)
    if loc is not None:
        return "covered", d
    loc, nrm, _, d = bvh.ray_cast(p - n * 1e-5, -n, in_window)
    if loc is not None and nrm.dot(n) > 0.2:
        return ("poke" if d <= poke_max else "exposed"), d
    return None, None


def band_gaps(bvh_layers, bvh_body, z0, z1, centre=(0.0, -0.02), step=0.002, angles=16, start=0.24,
              skip=None):
    """Horizontal rays toward the body's axis between z0 and z1, starting
    `start` m out (inside the A-pose arms): per direction, the height (mm)
    where the body is hit before any layer (bare skin; in Humanoid.glb,
    whose covered skin is removed, a hole). `skip(face index)` leaves out
    body hits that aren't the torso (a heavy build's arm reaches inside
    `start`)."""
    out = {}
    for k in range(angles):
        a = 2 * math.pi * k / angles
        d = Vector((-math.sin(a), math.cos(a), 0.0))  # a=0: from the front
        gap = 0.0
        z = z0
        while z <= z1 + 1e-9:
            o = Vector((centre[0], centre[1], z)) - d * start
            lb = bvh_body.ray_cast(o, d, 2 * start)
            ll = bvh_layers.ray_cast(o, d, 2 * start)
            if lb[0] is not None and (ll[0] is None or ll[3] > lb[3]) and not (skip and skip(lb[2])):
                gap += step
            elif lb[0] is None and ll[0] is None:
                pass
            z += step
        out[f"{round(math.degrees(a)):03d}"] = round(gap * 1000, 1)
    return out


# -- studio ----------------------------------------------------------------------------

STUDIO = {
    "samples": 32,
    "key": ((-2.4, -3.0, 3.1), 650.0, 2.2),
    "fill": ((3.0, -2.6, 1.7), 220.0, 3.0),
    "rim": ((1.4, 3.0, 3.2), 520.0, 1.6),
    "aim": (0.0, 0.0, 1.15),
    "backdrop": (0.30, 0.30, 0.30),
    "backdrop_glow": 0.6,
    "world": (0.05, 0.05, 0.055),
    "seen_bg": (0.42, 0.42, 0.42),
}


def look_at(obj, target):
    d = Vector(target) - obj.location
    obj.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()


def setup_studio(width, height, samples=None):
    """Cycles CPU, AgX, denoised; three area lights and a grey cyclorama.
    Identical every run (no randomness, fixed seed)."""
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = samples or STUDIO["samples"]
    scene.cycles.use_adaptive_sampling = False
    scene.cycles.seed = 7
    scene.cycles.use_denoising = True
    try:
        scene.cycles.denoiser = "OPENIMAGEDENOISE"
    except Exception:
        pass
    scene.cycles.max_bounces = 6
    scene.render.resolution_x, scene.render.resolution_y = width, height
    scene.render.resolution_percentage = 100
    scene.render.film_transparent = False
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGB"
    scene.view_settings.view_transform = "AgX"
    scene.view_settings.look = "None"
    scene.view_settings.exposure = 0.0
    scene.display_settings.display_device = "sRGB"
    world = bpy.data.worlds.new("QAWorld")
    scene.world = world
    try:
        world.use_nodes = True
    except Exception:
        pass
    nt = world.node_tree
    bg = nt.nodes.get("Background")
    bg.inputs[0].default_value = (*STUDIO["world"], 1.0)
    bg.inputs[1].default_value = 1.0
    # Camera rays that miss the cyclorama see plain grey; lighting sees
    # only the dim ambient.
    seen = nt.nodes.new("ShaderNodeBackground")
    seen.inputs[0].default_value = (*STUDIO["seen_bg"], 1.0)
    path = nt.nodes.new("ShaderNodeLightPath")
    mix = nt.nodes.new("ShaderNodeMixShader")
    nt.links.new(path.outputs["Is Camera Ray"], mix.inputs[0])
    nt.links.new(bg.outputs[0], mix.inputs[1])
    nt.links.new(seen.outputs[0], mix.inputs[2])
    nt.links.new(mix.outputs[0], nt.nodes["World Output"].inputs["Surface"])
    for name in ("key", "fill", "rim"):
        loc, energy, size = STUDIO[name]
        L = bpy.data.lights.new("QA_" + name, "AREA")
        L.energy, L.size = energy, size
        o = bpy.data.objects.new("QA_" + name, L)
        scene.collection.objects.link(o)
        o.location = loc
        look_at(o, STUDIO["aim"])
    _cyclorama(scene)
    cam_data = bpy.data.cameras.new("QA_Camera")
    cam = bpy.data.objects.new("QA_Camera", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    return cam


def _cyclorama(scene, back=3.2, radius=1.6, width=24.0, height=8.0):
    import bmesh
    bm = bmesh.new()
    prof = [(y, 0.0) for y in np.linspace(-10.0, back - radius, 6)]
    for k in range(1, 12):
        a = (math.pi / 2) * k / 12
        prof.append((back - radius + radius * math.sin(a), radius - radius * math.cos(a)))
    prof += [(back, z) for z in np.linspace(radius, height, 4)[1:]]
    rows = []
    for x in (-width / 2, width / 2):
        rows.append([bm.verts.new((x, y, z)) for y, z in prof])
    for k in range(len(prof) - 1):
        bm.faces.new((rows[0][k], rows[1][k], rows[1][k + 1], rows[0][k + 1]))
    me = bpy.data.meshes.new("QA_Backdrop")
    bm.to_mesh(me)
    bm.free()
    for p in me.polygons:
        p.use_smooth = True
    o = bpy.data.objects.new("QA_Backdrop", me)
    scene.collection.objects.link(o)
    m = flat_material("QA_Backdrop", STUDIO["backdrop"], 0.9)
    # A little self-light so the wall doesn't go black in side views.
    b = m.node_tree.nodes.get("Principled BSDF")
    b.inputs["Emission Color"].default_value = (*STUDIO["backdrop"], 1.0)
    b.inputs["Emission Strength"].default_value = STUDIO["backdrop_glow"]
    me.materials.append(m)


def flat_material(name, color, rough=0.6):
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    try:
        m.use_nodes = True
    except Exception:
        pass
    b = m.node_tree.nodes.get("Principled BSDF")
    b.inputs["Base Color"].default_value = (*color, 1.0)
    b.inputs["Roughness"].default_value = rough
    return m


CLAY = {
    "skin": (0.55, 0.42, 0.36),
    "cloth": (0.42, 0.45, 0.50),
    "cloth2": (0.30, 0.32, 0.36),
    "hair": (0.22, 0.18, 0.15),
    "dark": (0.06, 0.06, 0.06),
    "eye": (0.80, 0.80, 0.78),
}


def clay_parts(objs, kind_of):
    """Every material slot of each object to one clay material by kind."""
    for o in objs:
        m = flat_material("Clay_" + kind_of(o.name), CLAY[kind_of(o.name)], 0.55)
        for slot in o.material_slots:
            slot.material = m


def clay_kind(name):
    if name.startswith("Body"):
        return "skin"
    if name.startswith("Hair") or name in ("Brows", "Lashes"):
        return "hair"
    if name in ("Eyes", "Teeth"):
        return "eye"
    if name.startswith(("Jacket", "Pants")):
        return "cloth2"
    return "cloth"


def wire_overlay(obj, thickness=0.0004, name=None, quads=True):
    """A static copy of `obj` as it's deformed now, shown as its edges
    (quads joined back from the glTF triangles), dark, for a topology view."""
    import bmesh
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(obj.evaluated_get(dg))
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.remove_doubles(bm, verts=bm.verts[:], dist=1e-6)
    if quads:
        bmesh.ops.join_triangles(bm, faces=bm.faces[:], angle_face_threshold=math.radians(40),
                                 angle_shape_threshold=math.radians(40), cmp_seam=False,
                                 cmp_sharp=False, cmp_uvs=False, cmp_vcols=False, cmp_materials=False)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new(name or (obj.name + "_wire"), me)
    o.matrix_world = obj.matrix_world.copy()
    bpy.context.scene.collection.objects.link(o)
    mod = o.modifiers.new("Wire", "WIREFRAME")
    mod.thickness = thickness
    mod.use_even_offset = False
    mod.use_relative_offset = False
    mod.use_replace = True
    o.data.materials.clear()
    o.data.materials.append(flat_material("QA_Wire", CLAY["dark"], 0.8))
    return o


def shot(cam, path, pos, target, lens=50.0, ortho=None):
    cam.location = Vector(pos)
    look_at(cam, target)
    if ortho:
        cam.data.type = "ORTHO"
        cam.data.ortho_scale = ortho
    else:
        cam.data.type = "PERSP"
        cam.data.lens = lens
    cam.data.sensor_fit = "AUTO"
    cam.data.sensor_width = 36.0
    cam.data.clip_start = 0.01
    bpy.context.scene.render.filepath = path
    bpy.ops.render.render(write_still=True)
    return path


def turn_character(ch, deg):
    ch.arm.rotation_mode = "XYZ"
    ch.arm.rotation_euler = (0.0, 0.0, math.radians(deg))
    update()


def contact_sheet(out, tiles, cols, labels=None, title=None, max_kb=600, grey=False):
    """Tiles (image paths) in a grid with labels, saved as PNG (8-bit grey
    with `grey`, for clay sheets); palette-quantized if the file would be
    over `max_kb`."""
    from PIL import Image, ImageDraw, ImageFont
    ims = [Image.open(t).convert("RGB") for t in tiles]
    w, h = ims[0].size
    rows = (len(ims) + cols - 1) // cols
    top = 28 if title else 0
    sheet = Image.new("RGB", (w * cols, h * rows + top), (24, 24, 26))
    d = ImageDraw.Draw(sheet)
    try:
        font = ImageFont.truetype("arial.ttf", 15)
        tfont = ImageFont.truetype("arial.ttf", 17)
    except Exception:
        font = tfont = ImageFont.load_default()
    if title:
        d.text((8, 5), title, fill=(235, 235, 235), font=tfont)
    for k, im in enumerate(ims):
        x, y = (k % cols) * w, (k // cols) * h + top
        sheet.paste(im, (x, y))
        if labels:
            d.rectangle((x, y, x + 8 + 8 * len(labels[k]), y + 22), fill=(0, 0, 0))
            d.text((x + 5, y + 3), labels[k], fill=(255, 255, 255), font=font)
    if grey:
        sheet = sheet.convert("L")
    sheet.save(out, optimize=True)
    if os.path.getsize(out) > max_kb * 1024:
        q = sheet.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
        q.save(out, optimize=True)
    return out
