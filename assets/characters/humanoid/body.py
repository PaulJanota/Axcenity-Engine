"""The base body: both frames loaded from `source/`, stood on the ground at
1.75 m, arms re-posed to the A-pose, and the eyes' centres."""

import bmesh
import bpy
from mathutils import Matrix, Vector

from common import HEIGHT, centroid, import_obj, new_object, positions, set_positions
from rig import Landmarks, repose_arms


def bbox_centre(obj):
    xs = [vert.co for vert in obj.data.vertices]
    return Vector([(min(p[k] for p in xs) + max(p[k] for p in xs)) / 2 for k in range(3)])


class Frame:
    """One frame of the body: its positions, its eyes' centres and radius,
    and (masculine frame only) the eye objects."""

    def __init__(self, body_positions, eyes):
        self.P = body_positions
        self.eyes = eyes  # {1: left eye object, -1: right eye object}
        self.centres = {s: bbox_centre(eye) for s, eye in eyes.items()}
        eye = eyes[1]
        self.eye_radius = max((vert.co - self.centres[1]).length for vert in eye.data.vertices)

    def eye_centres(self):
        return dict(self.centres)


def _normalize(obj_list, height):
    """Feet on the ground (z = 0) and the crown at `height`."""
    body = obj_list[0]
    zs = [vert.co.z for vert in body.data.vertices]
    lo, hi = min(zs), max(zs)
    m = Matrix.Scale(height / (hi - lo), 4) @ Matrix.Translation(Vector((0, 0, -lo)))
    for o in obj_list:
        o.data.transform(m)
        o.data.update()


def _split_eyes(eyes_obj):
    """The two eyes of `eyes_*.obj` (one object) as {1: left, -1: right}."""
    (both,) = eyes_obj
    out = {}
    for s in (1, -1):
        bm = bmesh.new()
        bm.from_mesh(both.data)
        bmesh.ops.delete(bm, geom=[vert for vert in bm.verts if vert.co.x * s < 0], context="VERTS")
        me = bpy.data.meshes.new(f"Eye{s}")
        bm.to_mesh(me)
        bm.free()
        out[s] = new_object(f"Eye{'Left' if s > 0 else 'Right'}", me)
    bpy.data.objects.remove(both)
    return out


def pack_udims(obj):
    """The base body's UVs use four UDIM tiles (1001, 1002, 1003, 1005);
    glTF has one texture square, so each tile becomes a quarter of it."""
    cells = {0: (0.0, 0.5), 1: (0.5, 0.5), 2: (0.0, 0.0), 4: (0.5, 0.0)}
    for d in obj.data.uv_layers[0].data:
        u, v_ = d.uv
        tile = int(u // 1)
        cu, cv = cells[tile]
        d.uv = (cu + (u - tile) * 0.5, cv + min(max(v_, 0.0), 1.0) * 0.5)


def load():
    """The masculine body object (with face sets), its landmarks and both
    frames, normalized and re-posed. The feminine body object is removed
    once its positions are read."""
    masc = import_obj("body_masculine.obj")[0]
    masc.name = "Body"
    pack_udims(masc)
    masc_eyes = _split_eyes(import_obj("eyes_masculine.obj"))
    fem = import_obj("body_feminine.obj")[0]
    fem_eyes = _split_eyes(import_obj("eyes_feminine.obj"))
    _normalize([masc, *masc_eyes.values()], HEIGHT)
    _normalize([fem, *fem_eyes.values()], HEIGHT)
    lm = Landmarks(masc)
    frames = {}
    from face import relax_lids
    for name, obj, eyes in (("Masculine", masc, masc_eyes), ("Feminine", fem, fem_eyes)):
        frame = Frame(positions(obj), eyes)
        # Twice: the second pass takes out what the first left, since the
        # final fit's shoulder sits a little off the first guess.
        for _ in range(2):
            frame.P = repose_arms(obj, frame.P, lm, frame.eye_centres())
        frame.P = relax_lids(frame.P, lm, frame.eye_centres(), frame.eye_radius)
        frames[name] = frame
    set_positions(masc, frames["Masculine"].P)
    bpy.data.objects.remove(fem)
    for eye in fem_eyes.values():
        bpy.data.objects.remove(eye)
    frames["Feminine"].eyes = {}
    return masc, lm, frames
