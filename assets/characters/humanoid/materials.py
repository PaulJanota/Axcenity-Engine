"""Materials: PBR, one per name so a tint can find it (`MaterialName`).

Colour carries the design and roughness separates the materials. Two
textures: the skin's (white where the skin is its plain tone, warmer on the
lips, cheeks, nose, ears, knuckles, elbows and knees, baked from a colour
field over the body) and the iris's. Both are multiplied by the
material's colour, so a tint changes the tone and keeps the variation.
"""

import math
import os

import bpy
import numpy as np
from mathutils import Vector

# name -> (base colour (sRGB), roughness, metalness)
PALETTE = {
    "Skin": ((0.93, 0.70, 0.58), 0.52, 0.0),
    "Mouth": ((0.42, 0.13, 0.14), 0.45, 0.0),
    "Teeth": ((0.93, 0.91, 0.85), 0.30, 0.0),
    "Gums": ((0.80, 0.42, 0.42), 0.45, 0.0),
    "Sclera": ((0.95, 0.94, 0.92), 0.12, 0.0),
    "Iris": ((0.42, 0.27, 0.15), 0.10, 0.0),
    "Pupil": ((0.02, 0.02, 0.02), 0.05, 0.0),
    "Brow": ((0.20, 0.12, 0.07), 0.75, 0.0),
    "Lash": ((0.05, 0.035, 0.03), 0.60, 0.0),
    "Hair": ((0.30, 0.17, 0.09), 0.45, 0.0),
    "HairSecondary": ((0.36, 0.21, 0.11), 0.45, 0.0),
    "Underwear": ((0.20, 0.21, 0.24), 0.80, 0.0),
    "Shirt": ((0.86, 0.85, 0.82), 0.88, 0.0),
    "Jacket": ((0.13, 0.19, 0.33), 0.70, 0.0),
    "JacketTrim": ((0.62, 0.65, 0.68), 0.35, 0.0),
    "JacketRib": ((0.10, 0.14, 0.24), 0.90, 0.0),
    "Trousers": ((0.11, 0.11, 0.12), 0.82, 0.0),
    "Shoes": ((0.92, 0.92, 0.90), 0.55, 0.0),
    "ShoeAccent": ((0.13, 0.19, 0.33), 0.55, 0.0),
    "Sole": ((0.18, 0.18, 0.19), 0.90, 0.0),
    "Laces": ((0.92, 0.92, 0.90), 0.85, 0.0),
    "Metal": ((0.75, 0.75, 0.72), 0.30, 1.0),
    "Leather": ((0.20, 0.14, 0.10), 0.45, 0.0),
}


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def material(name, texture=None):
    """The named material: texture (if any) times its colour."""
    m = bpy.data.materials.get(name)
    if m is not None:
        return m
    color, rough, metal = PALETTE[name]
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    lin = (*[srgb_to_linear(c) for c in color], 1.0)
    bsdf.inputs["Roughness"].default_value = rough
    bsdf.inputs["Metallic"].default_value = metal
    if texture is None:
        bsdf.inputs["Base Color"].default_value = lin
    else:
        tex = nt.nodes.new("ShaderNodeTexImage")
        tex.image = texture
        mix = nt.nodes.new("ShaderNodeMix")
        mix.data_type = "RGBA"
        mix.blend_type = "MULTIPLY"
        mix.inputs["Factor"].default_value = 1.0
        nt.links.new(tex.outputs["Color"], mix.inputs[6])
        mix.inputs[7].default_value = lin
        nt.links.new(mix.outputs[2], bsdf.inputs["Base Color"])
    return m


def assign(obj, names_per_face):
    """Gives `obj` the materials named per polygon."""
    order = []
    for n in names_per_face:
        if n not in order:
            order.append(n)
    obj.data.materials.clear()
    for n in order:
        obj.data.materials.append(material(n) if n != "Skin" else bpy.data.materials["Skin"])
    for poly, n in zip(obj.data.polygons, names_per_face):
        poly.material_index = order.index(n)


# -- the skin texture ----------------------------------------------------------

def skin_colours(P, face, joints, lm):
    """Per-vertex multiplier of the skin tone (linear RGB), 1 for plain skin."""
    from face import gauss

    out = []
    J = {k: (h, t) for k, (h, t, _) in joints.items()}
    spots = []  # (centre, radius, colour at full strength)
    for _, s in ((None, 1), (None, -1)):
        corner = face.corner[s]
        spots.append((corner + Vector((s * 0.022, 0.012, 0.03)), 0.019, (1.0, 0.80, 0.78)))  # cheek
        ear = J["Head"][0] + Vector((s * 0.085, 0.0, 0.11))
        spots.append((ear, 0.03, (1.0, 0.86, 0.84)))
        for f in ("Index", "Middle", "Ring", "Little"):
            spots.append((J[f"{'Left' if s > 0 else 'Right'}{f}Proximal"][0], 0.009, (1.0, 0.86, 0.84)))
            spots.append((J[f"{'Left' if s > 0 else 'Right'}{f}Distal"][1], 0.008, (1.0, 0.85, 0.83)))
        side = "Left" if s > 0 else "Right"
        spots.append((J[f"{side}LowerArm"][0], 0.025, (1.0, 0.9, 0.88)))
        spots.append((J[f"{side}LowerLeg"][0] + Vector((0, -0.04, 0)), 0.035, (1.0, 0.88, 0.86)))
        spots.append((J[f"{side}Toes"][1], 0.03, (1.0, 0.88, 0.86)))
    nose_tip = min((P[i] for i in lm.verts(45)), key=lambda p: p.y)
    spots.append((nose_tip, 0.014, (1.0, 0.84, 0.82)))
    for i, p in enumerate(P):
        c = [1.0, 1.0, 1.0]
        for centre, r, col in spots:
            d2 = (p - centre).length_squared
            if d2 > 9 * r * r:
                continue
            g = gauss(d2, r)
            for k in range(3):
                c[k] *= 1.0 + (col[k] - 1.0) * g
        if face.head[i]:
            lips = max(face.upper_lip_band(i), face.lower_lip_band(i))
            lips = smoothstep_(0.35, 0.65, lips)
            for k, col in enumerate((0.86, 0.60, 0.60)):
                c[k] *= 1.0 + (col - 1.0) * lips
            # Lids a little rosier.
            for e in face.eyes.values():
                d = (p - e.c).length
                g = math.exp(-((d - e.r - 0.006) / 0.006) ** 2) if p.y < e.c.y else 0.0
                for k, col in enumerate((0.97, 0.88, 0.88)):
                    c[k] *= 1.0 + (col - 1.0) * g
        out.append(tuple(srgb_to_linear(x) for x in c))
    return out


def smoothstep_(a, b, x):
    t = max(0.0, min(1.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)


def bake_vertex_colours(obj, colours, name, size, out_dir):
    """Bakes per-vertex colours into a texture over the object's UVs (the
    colours interpolate across faces) and saves it as a PNG."""
    me = obj.data
    attr = me.color_attributes.new(name="BakeColour", type="FLOAT_COLOR", domain="POINT")
    for i, c in enumerate(colours):
        attr.data[i].color = (*c, 1.0)
    img = bpy.data.images.new(name, size, size, alpha=False, float_buffer=False)
    img.colorspace_settings.name = "sRGB"
    tmp = bpy.data.materials.new("BakeEmit")
    tmp.use_nodes = True
    nt = tmp.node_tree
    for n in list(nt.nodes):
        nt.nodes.remove(n)
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    em = nt.nodes.new("ShaderNodeEmission")
    ca = nt.nodes.new("ShaderNodeVertexColor")
    ca.layer_name = "BakeColour"
    nt.links.new(ca.outputs["Color"], em.inputs["Color"])
    nt.links.new(em.outputs["Emission"], out.inputs["Surface"])
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    nt.nodes.active = tex
    saved = list(me.materials)
    saved_index = [p.material_index for p in me.polygons]
    me.materials.clear()
    me.materials.append(tmp)
    for p in me.polygons:
        p.material_index = 0
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = 1
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.bake(type="EMIT", margin=8, use_clear=True)
    me.materials.clear()
    for m in saved:
        me.materials.append(m)
    for p, k in zip(me.polygons, saved_index):
        p.material_index = k
    me.color_attributes.remove(attr)
    bpy.data.materials.remove(tmp)
    path = os.path.join(out_dir, f"{name}.png")
    img.filepath_raw = path
    img.file_format = "PNG"
    img.save()
    return img


# -- eyes -------------------------------------------------------------------------

PUPIL_DEG = 8.5
IRIS_DEG = 20.5


def iris_texture(out_dir, size=256):
    """A light, tintable iris: radial fibres, a darker limbal ring, a lighter
    collar round the pupil, a black pupil. Mapped flat over the eye front."""
    rng = np.random.default_rng(7)
    y, x = np.mgrid[0:size, 0:size]
    u = (x + 0.5) / size * 2 - 1
    v = (y + 0.5) / size * 2 - 1
    r = np.sqrt(u * u + v * v)
    a = np.arctan2(v, u)
    iris_r = math.sin(math.radians(IRIS_DEG))
    pupil_r = math.sin(math.radians(PUPIL_DEG))
    t = np.clip((r - pupil_r) / (iris_r - pupil_r), 0, 1)
    fibres = np.zeros_like(r)
    for k, amp in ((23, 0.10), (41, 0.07), (67, 0.05)):
        phase = rng.uniform(0, 2 * np.pi)
        fibres += amp * np.sin(a * k + phase + 3.0 * t)
    base = 0.82 + fibres
    base *= 1.0 + 0.25 * np.exp(-((t - 0.18) / 0.12) ** 2)       # collar
    base *= 1.0 - 0.55 * np.clip((t - 0.78) / 0.22, 0, 1) ** 1.5  # limbal ring
    val = np.clip(base, 0, 1)
    rgb = np.stack([val, val, val], axis=-1)
    rgb[r < pupil_r] = 0.0
    rgb[r > iris_r * 1.02] = 1.0
    img = bpy.data.images.new("Iris", size, size, alpha=False)
    pixels = np.concatenate([rgb, np.ones((size, size, 1))], axis=-1).astype(np.float32)
    img.pixels.foreach_set(pixels.ravel())
    path = os.path.join(out_dir, "Iris.png")
    img.filepath_raw = path
    img.file_format = "PNG"
    img.save()
    return img


def setup_eye(eye, centre, radius, iris_img):
    """Sclera, iris and pupil by angle from the eye's axis (-Y), and flat
    uvs over the front for the iris texture."""
    me = eye.data
    names = []
    for poly in me.polygons:
        d = poly.center - centre
        ang = math.degrees(math.acos(max(-1.0, min(1.0, -d.y / d.length))))
        names.append("Iris" if ang < IRIS_DEG + 0.5 else "Sclera")
    uv = me.uv_layers.get("UVMap") or me.uv_layers.new(name="UVMap")
    for loop in me.loops:
        p = me.vertices[loop.vertex_index].co - centre
        uv.data[loop.index].uv = (0.5 + p.x / (2 * radius), 0.5 + p.z / (2 * radius))
    if bpy.data.materials.get("Iris") is None:
        material("Iris", iris_img)
    assign(eye, names)
