"""Clearance: clothes stay outside the body, and each layer under the next.

Garments follow the body's shapes by binding to its surface
(`build.follow_shapes`). That keeps them close, but not always outside:
where a shape makes the skin bulge more than the garment's binding allows
(a heavy build's thighs), or where one garment is worn over another (the
tee under the jacket, the trousers under the tee), points can end up on
the wrong side. These passes push them back, in the rest pose and in each
of the garment's shape keys on its own.
"""

from mathutils import Vector
from mathutils.bvhtree import BVHTree


def _keys(obj):
    keys = obj.data.shape_keys
    return [kb.name for kb in keys.key_blocks] if keys else []


def _positions(obj, key):
    if key is None:
        return [vert.co.copy() for vert in obj.data.vertices]
    return [d.co.copy() for d in obj.data.shape_keys.key_blocks[key].data]


def _set_positions(obj, key, positions):
    if key is not None:
        for d, p in zip(obj.data.shape_keys.key_blocks[key].data, positions):
            d.co = p
    if key in (None, "Basis"):
        for vert, p in zip(obj.data.vertices, positions):
            vert.co = p


def _body(P, shapes, names):
    """The body's positions with the shapes `names` at weight 1."""
    out = [p.copy() for p in P]
    for name in names:
        for i, d in shapes.get(name, {}).items():
            out[i] += d
    return out


def _bvh(positions, faces):
    return BVHTree.FromPolygons([tuple(p) for p in positions], faces)


def _obj_faces(obj):
    return [tuple(poly.vertices) for poly in obj.data.polygons]


def smooth_field(obj, field, iterations=3):
    """A shape's deltas ({vertex: delta}) on `obj`, smoothed over its edges:
    binding each point to the nearest skin can give one point of a rim a
    very different delta from its neighbours (spikes at a shoe's collar on
    the feminine frame); smoothing keeps the garment moving as one piece."""
    nbr = _neighbours(obj)
    zero = Vector()
    d = [field.get(i, zero) for i in range(len(nbr))]
    for _ in range(iterations):
        d = [d[i] * 0.5 + sum((d[j] for j in nbr[i]), Vector()) * (0.5 / len(nbr[i])) if nbr[i] else d[i]
             for i in range(len(d))]
    return {i: v for i, v in enumerate(d) if v.length_squared > 1e-12}


def smooth_weights(obj, iterations=4, only=None):
    """Smooths `obj`'s skin weights over its own edges. Copying each point's
    weights from the nearest skin gives cloth that bridges a crease (behind
    the thigh, under the arm) weights that jump from one bone to the next,
    so its triangles fold over when the joint bends; smoothed, the cloth
    bends as one piece. `only` limits it to points whose weights involve
    those bones (None: all). Call `rig.limit_weights` after."""
    nbr = _neighbours(obj)
    names = {vg.index: vg.name for vg in obj.vertex_groups}
    w = [{names[g.group]: g.weight for g in vert.groups if g.group in names} for vert in obj.data.vertices]
    moving = [only is None or any(b in only for b in wi) for wi in w]
    for _ in range(iterations):
        nxt = []
        for i, wi in enumerate(w):
            if not moving[i] or not nbr[i]:
                nxt.append(wi)
                continue
            mix = {b: x * 0.5 for b, x in wi.items()}
            share = 0.5 / len(nbr[i])
            for j in nbr[i]:
                for b, x in w[j].items():
                    mix[b] = mix.get(b, 0.0) + x * share
            total = sum(mix.values()) or 1.0
            nxt.append({b: x / total for b, x in mix.items()})
        w = nxt
    groups = {vg.name: vg for vg in obj.vertex_groups}
    for i, wi in enumerate(w):
        if not moving[i]:
            continue
        for b, x in wi.items():
            g = groups.get(b) or obj.vertex_groups.new(name=b)
            groups[b] = g
            g.add([i], x, "REPLACE")


FRAME = "Frame_Feminine"


def _neighbours(obj):
    nbr = [[] for _ in obj.data.vertices]
    for e in obj.data.edges:
        a, b = e.vertices
        nbr[a].append(b)
        nbr[b].append(a)
    return nbr


def _pushes_out(pos, bvh, margin, reach, facing=None, nbr=None, most=0.012, spread=3):
    """Per point, the move that puts it `margin` outside the surface in
    `bvh` (None where it already is). Points whose `facing` normal looks
    at the skin are left alone (an opening's rolled-in band: hidden, and
    pushing it out would fold it through the cloth). Moves are capped at
    `most` and spread over `spread` rings of `nbr`, so the cloth bulges
    smoothly instead of spiking."""
    out = []
    for n, q in enumerate(pos):
        loc, nrm, _, _ = bvh.find_nearest(q, reach)
        if loc is None or (facing is not None and facing[n].dot(nrm) < 0.0):
            out.append(None)
            continue
        s = (q - loc).dot(nrm)
        if s >= margin:
            out.append(None)
            continue
        d = nrm * (margin - s)
        if d.length > most:
            d = d.normalized() * most
        out.append(d)
    if nbr is None:
        return out
    for _ in range(spread):
        nxt = list(out)
        for n, ring in enumerate(nbr):
            moves = [out[j] for j in ring if out[j] is not None]
            if not moves:
                continue
            # Half the ring's average move: a bulge that fades over the
            # rings, which also carries an opening's inner band along.
            avg = sum(moves, moves[0] * 0.0) * (0.5 / len(ring))
            if out[n] is None or avg.length > out[n].length:
                nxt[n] = avg
        out = nxt
    return out


def clear_body(obj, P, tris, shapes, masks, margin=0.002, reach=0.03):
    """Moves `obj`'s points that are inside the body, or closer to the skin
    than `margin`, out along the skin's normal. The body has the garment's
    `masks` on (they pull the covered skin in) and, for each shape key of
    `obj`, the body shape of the same name. Then the frame with each other
    shape (feminine and heavy): shape keys add, so a mix can still poke
    where neither does alone; that correction goes into the other shape's
    key (the masculine frame with it is then a little looser there).
    Returns {key or mix: points moved}."""
    moved = {}
    keys = _keys(obj)
    nbr = _neighbours(obj)
    facing = [vert.normal.copy() for vert in obj.data.vertices]
    for key in keys or [None]:
        names = list(masks) + ([key] if key not in (None, "Basis") else [])
        pos = _positions(obj, key)
        pushes = _pushes_out(pos, _bvh(_body(P, shapes, names), tris), margin, reach, facing, nbr)
        for n, d in enumerate(pushes):
            if d is not None:
                pos[n] = pos[n] + d
        _set_positions(obj, key, pos)
        moved[key or "Basis"] = sum(1 for d in pushes if d is not None)
    if FRAME not in keys:
        return moved
    base = _positions(obj, "Basis")
    frame = _positions(obj, FRAME)
    for key in keys:
        if key in ("Basis", FRAME):
            continue
        own = _positions(obj, key)
        mix = [b + (f - b) + (o - b) for b, f, o in zip(base, frame, own)]
        bvh = _bvh(_body(P, shapes, list(masks) + [FRAME, key]), tris)
        pushes = _pushes_out(mix, bvh, margin, reach, facing, nbr)
        for n, d in enumerate(pushes):
            if d is not None:
                own[n] = own[n] + d
        _set_positions(obj, key, own)
        moved[f"{FRAME}+{key}"] = sum(1 for d in pushes if d is not None)
    return moved


def _covered(inner_pos, outer_bvh, skin_bvh, behind, reach):
    """For each inner point: (hit point, face index, distance ahead) of the
    outer garment along the skin's normal, or None where it isn't covered."""
    out = []
    for q in inner_pos:
        _, nrm, _, _ = skin_bvh.find_nearest(q, reach)
        if nrm is None:
            out.append(None)
            continue
        hit, _, fi, dist = outer_bvh.ray_cast(q - nrm * behind, nrm, reach + behind)
        out.append(None if hit is None else (hit, fi, dist - behind, nrm))
    return out


def follow_layer(inner, outer, P, tris, behind=0.02, reach=0.06, feather=4):
    """Where `outer` covers `inner`, `inner`'s shape keys take `outer`'s
    deltas (at the point it lies under), feathered over a few edges into
    its own: a layer then moves with the garment over it in every shape and
    every mix of shapes, so being under it at rest keeps it under it."""
    keys = [k for k in _keys(inner) if k != "Basis"]
    outer_keys = set(_keys(outer))
    if not keys or not outer_keys:
        return 0
    faces = _obj_faces(outer)
    base_in = _positions(inner, "Basis")
    base_out = _positions(outer, "Basis")
    cover = _covered(base_in, _bvh(base_out, faces), _bvh(P, tris), behind, reach)
    # Weight 1 where covered, feathered outward over the inner mesh's edges
    # (covered points keep 1).
    w = [1.0 if c is not None else 0.0 for c in cover]
    nbr = [[] for _ in base_in]
    for e in inner.data.edges:
        a, b = e.vertices
        nbr[a].append(b)
        nbr[b].append(a)
    for _ in range(feather):
        w = [max(w[i], 0.75 * sum(w[j] for j in nbr[i]) / len(nbr[i])) if nbr[i] else w[i]
             for i in range(len(w))]
    # The outer face each inner point lies under (or its nearest), with
    # barycentric weights, to read the outer garment's deltas there.
    outer_bvh = _bvh(base_out, faces)
    binds = []
    for i, q in enumerate(base_in):
        if w[i] <= 0.0:
            binds.append(None)
            continue
        if cover[i] is not None:
            loc, fi = cover[i][0], cover[i][1]
        else:
            loc, _, fi, _ = outer_bvh.find_nearest(q)
        tri = faces[fi]
        if len(tri) != 3:
            # Quads and n-gons: use the face's corners nearest the point.
            tri = sorted(tri, key=lambda k: (base_out[k] - loc).length_squared)[:3]
        binds.append((tri, _barycentric(loc, *(base_out[k] for k in tri))))
    for key in keys:
        if key not in outer_keys:
            continue
        pos_out = _positions(outer, key)
        pos_in = _positions(inner, key)
        for i, bd in enumerate(binds):
            if bd is None:
                continue
            tri, bc = bd
            d_out = sum(((pos_out[k] - base_out[k]) * c for k, c in zip(tri, bc)), base_out[0] * 0.0)
            d_own = pos_in[i] - base_in[i]
            pos_in[i] = base_in[i] + d_own.lerp(d_out, w[i])
        _set_positions(inner, key, pos_in)
    return sum(1 for x in w if x > 0.0)


def _barycentric(p, a, b, c):
    v0, v1, v2 = b - a, c - a, p - a
    d00, d01, d11 = v0.dot(v0), v0.dot(v1), v1.dot(v1)
    d20, d21 = v2.dot(v0), v2.dot(v1)
    den = d00 * d11 - d01 * d01
    if abs(den) < 1e-12:
        return (1.0, 0.0, 0.0)
    v = (d11 * d20 - d01 * d21) / den
    w = (d00 * d21 - d01 * d20) / den
    v, w = max(0.0, v), max(0.0, w)
    u = max(0.0, 1.0 - v - w)
    s = u + v + w
    return (u / s, v / s, w / s)


def _pulls_under(pos, cover, skin, margin, behind, reach):
    """Per inner point, the move that puts it `margin` under the outer
    garment in `cover`, where that garment covers it (None elsewhere)."""
    out = []
    for q in pos:
        _, nrm, _, _ = skin.find_nearest(q, reach)
        if nrm is None:
            out.append(None)
            continue
        hit, _, _, dist = cover.ray_cast(q - nrm * behind, nrm, reach + behind)
        if hit is None or dist - behind >= margin:
            out.append(None)
            continue
        out.append(hit - nrm * margin - q)
    return out


def clear_layer(inner, outer, P, tris, shapes, margin=0.003, behind=0.02, reach=0.06):
    """Keeps `inner` under `outer` wherever `outer` covers it: a ray from
    just inside each inner point, along the skin's normal there, finds the
    outer garment; if the inner point isn't at least `margin` under it, it
    moves there. Points the outer garment doesn't cover (an open front,
    below a cropped hem) are left alone. Shape keys are cleared one by one,
    each against the outer garment's key of the same name and the body's
    shape, then the frame mixed with each other shape (the correction goes
    into that shape's key, as in `clear_body`). Returns {key or mix:
    points moved}."""
    moved = {}
    outer_faces = _obj_faces(outer)
    outer_keys = set(_keys(outer))
    keys = _keys(inner)

    def outer_at(key):
        return _positions(outer, key if key in outer_keys else ("Basis" if outer_keys else None))

    for key in keys or [None]:
        body_names = [key] if key not in (None, "Basis") else []
        skin = _bvh(_body(P, shapes, body_names), tris)
        cover = _bvh(outer_at(key), outer_faces)
        pos = _positions(inner, key)
        moves = _pulls_under(pos, cover, skin, margin, behind, reach)
        for n, d in enumerate(moves):
            if d is not None:
                pos[n] = pos[n] + d
        _set_positions(inner, key, pos)
        moved[key or "Basis"] = sum(1 for d in moves if d is not None)
    if FRAME not in keys:
        return moved
    base, frame = _positions(inner, "Basis"), _positions(inner, FRAME)
    obase, oframe = outer_at("Basis"), outer_at(FRAME)
    for key in keys:
        if key in ("Basis", FRAME):
            continue
        own, oown = _positions(inner, key), outer_at(key)
        mix = [b + (f - b) + (o - b) for b, f, o in zip(base, frame, own)]
        omix = [b + (f - b) + (o - b) for b, f, o in zip(obase, oframe, oown)]
        skin = _bvh(_body(P, shapes, [FRAME, key]), tris)
        moves = _pulls_under(mix, _bvh(omix, outer_faces), skin, margin, behind, reach)
        for n, d in enumerate(moves):
            if d is not None:
                own[n] = own[n] + d
        _set_positions(inner, key, own)
        moved[f"{FRAME}+{key}"] = sum(1 for d in moves if d is not None)
    return moved
