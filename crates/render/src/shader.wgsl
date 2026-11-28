// Parts and imported models: physically based materials with texture maps,
// drawn into the HDR scene. Compiled after `lighting.wgsl`, which lights
// each pixel (`light_surface`). Also the shadow pass's and the sky's entry points.
// Followed by one `surface` function: surface.wgsl's, or a material's own
// (renderer/surface_shaders.rs), so this file must not define one.

// How the item's texture maps are read. Must match `MapsUniform` in
// renderer/types.rs.
struct Maps {
    // Which channel of each map holds its value: 1 in that channel, 0 in
    // the others.
    metalness: vec4<f32>,
    roughness: vec4<f32>,
    occlusion: vec4<f32>,
    opacity: vec4<f32>,
    // 1 to use 1 minus the value, for metalness, roughness, occlusion and opacity.
    invert: vec4<f32>,
    // x: normal map scale (0: no normal map), y: 1, or -1 to flip the
    // normal map's green, z: occlusion strength, w: 1 if opacity comes from
    // its own map rather than the base color's alpha.
    settings: vec4<f32>,
    // x: 1 if there's a specular map. y: 1 if the normal map holds only
    // red and green (BC5), so its blue is worked out from them.
    specular: vec4<f32>,
    // The columns of a 2x2 matrix that scales and turns the texture
    // coordinates (read by the vertex shader).
    uv_matrix: vec4<f32>,
    // xy: added to the texture coordinates after `uv_matrix`.
    uv_offset: vec4<f32>,
    // How it's lit. x: the model (SHADING_STANDARD, _UNLIT, _TOON, _SKIN,
    // _HAIR or _CLOTH); for toon, y: its bands (0: read its ramp, bound in
    // the specular map's place), z: softness, w: highlight size; for skin,
    // hair and cloth, see skin.wgsl, hair.wgsl and cloth.wgsl; for
    // standard, y: a leaf's translucency.
    shading: vec4<f32>,
    // x: toon's rim light. Cloth's sheen colour (cloth.wgsl).
    toon: vec4<f32>,
    // The outline's linear color, then its width (0: none) in pixels of a
    // view 1080 pixels tall.
    outline: vec4<f32>,
    // The surface shader's values (`Material::surface_parameters`).
    params: array<vec4<f32>, 8>,
}

// The item's material textures. Color ones (base color, emission) are read
// through sRGB views, so samples are linear light; the rest hold plain
// values. A map an item doesn't have is a single white pixel, which
// changes nothing.
@group(1) @binding(0) var base_color_texture: texture_2d<f32>;
@group(1) @binding(1) var material_sampler: sampler;
@group(1) @binding(2) var<uniform> maps: Maps;
@group(1) @binding(3) var normal_texture: texture_2d<f32>;
@group(1) @binding(4) var metalness_texture: texture_2d<f32>;
@group(1) @binding(5) var roughness_texture: texture_2d<f32>;
@group(1) @binding(6) var occlusion_texture: texture_2d<f32>;
@group(1) @binding(7) var emissive_texture: texture_2d<f32>;
@group(1) @binding(8) var opacity_texture: texture_2d<f32>;
@group(1) @binding(9) var specular_texture: texture_2d<f32>;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(12) uv: vec2<f32>,
    @location(13) tangent: vec4<f32>,
    @builtin(vertex_index) index: u32,
}

// Animated meshes are posed once a frame, before any pass, by the posing
// pass (CO8, renderer/posing.rs, posing.wgsl): each skinned or morphed
// item's corners, leaned toward its blend shapes and moved by its joints,
// are in `posed_corners`, five words a corner from the first its instance's
// `skin.x` names.
@group(0) @binding(9) var<storage, read> posed_corners: array<u32>;

// Plants' packed data per corner (foliage W3b, ADR-0053,
// renderer/foliage.rs): one word a corner, 8 bits each of how open it is
// to the sky, its share of the wind's lean, its share of its item's
// flutter and a shade mask (a leaf's tint, bark's weathering), from the
// first corner an unposed instance's `skin.x` names when its `skin.y` is
// `FOLIAGE_CORNERS`.
@group(0) @binding(20) var<storage, read> foliage_corners: array<u32>;

// `skin.y` of an item at rest with foliage data. Must match `FOLIAGE` in
// renderer/foliage.rs.
const FOLIAGE_CORNERS: u32 = 4294967294u;

// A corner without foliage data: open, leaning by its height on the
// plant (`swayed_by`'s weight below 0), all of its item's flutter and no
// shade mask (below 0).
const NO_FOLIAGE: vec4<f32> = vec4<f32>(1.0, -1.0, 1.0, -1.0);

// Corner `index`'s foliage data, as its instance's `skin` finds it, or
// `NO_FOLIAGE`.
fn corner_foliage(index: u32, skin: vec2<u32>) -> vec4<f32> {
    if skin.y != FOLIAGE_CORNERS {
        return NO_FOLIAGE;
    }
    return unpack4x8unorm(foliage_corners[skin.x + index]);
}

// Words per posed corner. Must match `POSED_WORDS` in posing.wgsl.
const POSED_WORDS: u32 = 5u;
// A zero direction. Must match `ZERO_DIRECTION` in posing.wgsl.
const ZERO_DIRECTION: u32 = 0x80008000u;

// A direction from its octahedral word (`octahedral` in posing.wgsl).
fn from_octahedral(word: u32) -> vec3<f32> {
    if word == ZERO_DIRECTION {
        return vec3<f32>(0.0);
    }
    let p = unpack2x16snorm(word);
    var v = vec3<f32>(p, 1.0 - abs(p.x) - abs(p.y));
    let t = max(-v.z, 0.0);
    v.x += select(t, -t, v.x >= 0.0);
    v.y += select(t, -t, v.y >= 0.0);
    return normalize(v);
}

// The position of posed corner `at`.
fn posed_position(at: u32) -> vec3<f32> {
    let word = at * POSED_WORDS;
    return bitcast<vec3<f32>>(vec3<u32>(posed_corners[word], posed_corners[word + 1u],
        posed_corners[word + 2u]));
}

struct Posed {
    position: vec3<f32>,
    normal: vec3<f32>,
    tangent: vec3<f32>,
}

// Corner `index` of an item as its `skin` has it: from the posed corners
// (`skin.y` not 0: posed this frame, from corner `skin.x`), or at rest.
fn pose_corner(index: u32, skin: vec2<u32>, rest_position: vec3<f32>, rest_normal: vec3<f32>,
    tangent: vec3<f32>) -> Posed {
    if skin.y == 0u || skin.y == FOLIAGE_CORNERS {
        return Posed(rest_position, rest_normal, tangent);
    }
    let at = skin.x + index;
    let word = at * POSED_WORDS;
    return Posed(posed_position(at), from_octahedral(posed_corners[word + 3u]),
        from_octahedral(posed_corners[word + 4u]));
}

fn unit_or_zero(v: vec3<f32>) -> vec3<f32> {
    let length_squared = dot(v, v);
    return select(vec3<f32>(0.0), v * inverseSqrt(length_squared), length_squared > 0.0);
}

// Where `world` is bent by the frame's wind: `swayed_by` (sway.wgsl) at
// this frame's time, by the corner's `foliage` data (`corner_foliage`):
// its shares of the lean and the flutter.
fn swayed(world: vec3<f32>, sway: vec4<f32>, foliage: vec4<f32>) -> vec3<f32> {
    return swayed_by(world, sway, frame.clock, foliage.yz);
}

// Per draw item. Must match `InstanceRaw` in renderer.rs.
struct InstanceIn {
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(6) normal_0: vec3<f32>,
    @location(7) normal_1: vec3<f32>,
    @location(8) normal_2: vec3<f32>,
    @location(9) base_color: vec4<f32>, // rgb, opacity
    // Light given off, then how much of the item is drawn (see `dithered_out`).
    @location(10) emissive_fade: vec4<f32>,
    // Metalness, roughness, alpha cutoff, and how many studs one copy of
    // the texture covers (0 keeps the mesh's own coordinates).
    @location(11) surface: vec4<f32>,
    // Where the item's posed corners start, and 1 if it's posed (0: drawn
    // at rest; see `pose_corner`).
    @location(14) skin: vec2<u32>,
    // How it sways in the wind (see `swayed`); w's whole part is its decal role
    // (`ROLE_CHARACTER`, `ROLE_DECAL`, else 0).
    @location(15) sway: vec4<f32>,
}

// `DrawItem::decal_role` (renderer/lighting.rs's `instance_raw`): a
// character's item, which projected decals that ignore characters leave
// bare, and a Part's Decal, drawn a hair toward the camera.
const ROLE_CHARACTER: f32 = 1.0;
const ROLE_DECAL: f32 = 2.0;
// The prepass normal's w on a cut-out's pixel (1 on others, 0.5 on a
// character's). Must match `PREPASS_CUT_OUT` in ray_shadows.wgsl and
// ray_ao.wgsl.
const PREPASS_CUT_OUT: f32 = 0.875;
// The prepass normal's w on a cut-out pixel of the standard material
// (`fs_prepass_standard`), whose depth the scene pass's depth start keeps
// exactly (depth_start.wgsl): near enough `PREPASS_CUT_OUT` that everything
// else reads it as that. A surface shader's cut-outs are lit with a depth
// test of their own and keep `PREPASS_CUT_OUT`.
const PREPASS_STANDARD_CUT_OUT: f32 = 0.85;
// The same on a character's cut-out pixel (hair cards): read as a
// character's (0.5) by everything but the depth start.
const PREPASS_CHARACTER_CUT_OUT: f32 = 0.625;
// How far toward the camera a Decal's depth is moved, in clip-space depth
// (0 to 1): many times the depth's rounding, so it always wins over the
// face it lies on, yet far less than anything placed in front of it.
const DECAL_DEPTH_BIAS: f32 = 1e-6;

struct VertexOut {
    // Invariant, so the prepass and the moving items' motion pass
    // (`vs_motion`) put a corner at exactly the same depth.
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Flat: the item's own, the same at every corner. Interpolated, the
    // opacity of a solid item could come out a hair under 1 in places,
    // which then weren't treated as solid.
    @location(2) @interpolate(flat) base_color: vec4<f32>,
    @location(3) emissive: vec3<f32>,
    @location(4) surface: vec3<f32>,
    @location(5) uv: vec2<f32>,
    @location(6) tangent: vec4<f32>,
    @location(7) @interpolate(flat) fade: f32,
    // Its decal role (`ROLE_CHARACTER`, `ROLE_DECAL`, else 0).
    @location(8) @interpolate(flat) role: f32,
    // The middle of the item and half its size, for leaves (`leaf_canopy`).
    @location(9) @interpolate(flat) item_middle: vec3<f32>,
    @location(10) @interpolate(flat) item_half_size: vec3<f32>,
    // How far the snow lying on the item lifted this point, in studs
    // (lighting.wgsl's `snow_cap`): above 0 it's the slab of snow itself.
    @location(11) rise: f32,
    // 1 for something that hangs (`Sway::hanging`: drapes, washing), which
    // lets light through as cloth does, not as a leaf.
    @location(12) @interpolate(flat) hangs: f32,
    // A plant's baked data (`corner_foliage`): how open it is to the sky,
    // and its shade mask (below 0 for an item without the data).
    @location(13) foliage: vec2<f32>,
}

@vertex
fn vs_main(vertex: VertexIn, instance: InstanceIn) -> VertexOut {
    var out = item_vertex(vertex, instance);
    if out.role == ROLE_DECAL {
        out.clip_position.z -= DECAL_DEPTH_BIAS * out.clip_position.w;
    }
    return out;
}

// A corner of an item, posed, swayed and placed (`vs_main`'s work, shared
// with `vs_outline`).
fn item_vertex(vertex: VertexIn, instance: InstanceIn) -> VertexOut {
    let model = mat4x4<f32>(instance.model_0, instance.model_1, instance.model_2, instance.model_3);
    let normal_matrix = mat3x3<f32>(instance.normal_0, instance.normal_1, instance.normal_2);
    let posed = pose_corner(vertex.index, instance.skin, vertex.position, vertex.normal,
        vertex.tangent.xyz);
    let foliage = corner_foliage(vertex.index, instance.skin);
    var world_position = vec4<f32>(swayed((model * vec4<f32>(posed.position, 1.0)).xyz,
        instance.sway, foliage), 1.0);
    let world_normal = normal_matrix * posed.normal;

    var out: VertexOut;
    // Snow with a thickness lies on a solid thing as a slab: the corners
    // that face up rise by its depth. Not on what's see-through or cut out
    // (leaves take snow by their crown, `lit_leaf`), on what sways or is
    // posed, or on a decal.
    let moves = instance.sway.z > 0.0 || fract(instance.sway.w) > 0.0;
    out.hangs = select(0.0, 1.0, instance.sway.y < 0.0 && moves);
    out.rise = 0.0;
    let at_rest = instance.skin.y == 0u || instance.skin.y == FOLIAGE_CORNERS;
    if frame.weather.y > 0.0 && frame.weather.z > 0.0 && instance.base_color.a >= 1.0
        && instance.surface.z <= 0.0 && !moves && at_rest
        && floor(instance.sway.w) != ROLE_DECAL {
        let facing = world_normal / max(length(world_normal), 1e-6);
        out.rise = snow_cap(world_position.xyz, facing.y);
        world_position.y += out.rise;
    }
    out.clip_position = frame.view_proj * world_position;
    out.world_position = world_position.xyz;
    out.normal = world_normal;
    out.foliage = foliage.xw;
    out.base_color = instance.base_color;
    out.emissive = instance.emissive_fade.xyz;
    out.fade = instance.emissive_fade.w;
    out.role = floor(instance.sway.w);
    out.item_middle = instance.model_3.xyz;
    out.item_half_size = 0.5 * vec3<f32>(length(instance.model_0.xyz),
        length(instance.model_1.xyz), length(instance.model_2.xyz));
    out.surface = instance.surface.xyz;
    let tile = instance.surface.w;
    var uv = vertex.uv;
    if (tile > 0.0) {
        // The coordinates are distances on the unit mesh. Scale them by
        // how long those edges are on this part, so the texture repeats
        // every `tile` studs instead of stretching once across it.
        let along_u = (model * vec4<f32>(posed.tangent, 0.0)).xyz;
        let v_dir = cross(posed.normal, posed.tangent) * vertex.tangent.w;
        let along_v = (model * vec4<f32>(v_dir, 0.0)).xyz;
        uv = uv * vec2<f32>(length(along_u), length(along_v)) / tile;
    }
    // Then the material's own tiling, turn and offset.
    out.uv = mat2x2<f32>(maps.uv_matrix.xy, maps.uv_matrix.zw) * uv + maps.uv_offset.xy;
    // A direction along the surface, so it stretches with the shape.
    out.tangent = vec4<f32>((model * vec4<f32>(posed.tangent, 0.0)).xyz, vertex.tangent.w);
    return out;
}

// An outline's corner: the item's, pushed out on screen along its normal
// by the outline's width, the same number of pixels however far away it
// is. Only back faces are kept (`fs_outline`), so the line shows just past
// the item's silhouette, hidden by the item itself everywhere else.
@vertex
fn vs_outline(vertex: VertexIn, instance: InstanceIn) -> VertexOut {
    var out = item_vertex(vertex, instance);
    // `frame.ambient_occlusion.zw` is 1 over the scene's size in pixels.
    let size = 1.0 / frame.ambient_occlusion.zw;
    // Never under one pixel, so a small view still shows it.
    let pixels = max(maps.outline.w * size.y / 1080.0, 1.0);
    // The normal's direction on screen, in pixels (so the view's shape
    // doesn't squash it).
    let across = (frame.view_proj * vec4<f32>(out.normal, 0.0)).xy * size;
    if dot(across, across) > 1e-12 {
        let push = normalize(across) * pixels * 2.0 / size;
        out.clip_position = vec4<f32>(out.clip_position.xy + push * out.clip_position.w,
            out.clip_position.zw);
    }
    return out;
}

// The outline's color, unlit but fogged, on back faces only. Cut-out holes
// aren't followed: the outline is of the whole mesh.
@fragment
fn fs_outline(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if front || dithered_out(in.fade, in.clip_position.xy) {
        discard;
    }
    let seen = fogged(frame.fog, frame.camera_position.xyz, in.world_position, maps.outline.rgb);
    return vec4<f32>(seen, 1.0);
}

// The shadow pass's cascade: turns world space to that cascade's view from
// the sun. Bound with a different offset for each cascade.
@group(0) @binding(5) var<uniform> shadow_cascade: mat4x4<f32>;

// Draws solid items into one cascade of the shadow map, from the sun's point of view.
@vertex
fn vs_shadow(
    @builtin(vertex_index) index: u32,
    @location(0) position: vec3<f32>,
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(14) skin: vec2<u32>,
    @location(15) sway: vec4<f32>,
) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(model_0, model_1, model_2, model_3);
    let posed = pose_corner(index, skin, position, vec3<f32>(0.0), vec3<f32>(0.0));
    // Swaying plants' shadows sway with them.
    let world = swayed((model * vec4<f32>(posed.position, 1.0)).xyz, sway,
        corner_foliage(index, skin));
    return shadow_cascade * vec4<f32>(world, 1.0);
}

// A cut-out caster's corner in the shadow pass (`fs_shadow_cut_out`): its
// texture coordinates, placed as `item_vertex` places them, and what its
// opacity is cut at.
struct ShadowCutOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // The item's own opacity, its cutoff, and how much of its shadow it
    // casts (1 all of it; less near the end of its shadow's reach).
    @location(1) @interpolate(flat) cut: vec3<f32>,
}

// Draws a cut-out caster (a leaf card, a blade of grass, a hair card) into
// a shadow map, as `vs_shadow` does, with what its fragment needs to leave
// out its holes. Its texture coordinates aren't tiled by its size: a
// cut-out drawn that way (`DrawItem::uv_tile`) casts its whole mesh.
@vertex
fn vs_shadow_cut_out(
    @builtin(vertex_index) index: u32,
    @location(0) position: vec3<f32>,
    @location(12) uv: vec2<f32>,
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(9) base_color: vec4<f32>,
    @location(10) emissive_fade: vec4<f32>,
    @location(11) surface: vec4<f32>,
    @location(14) skin: vec2<u32>,
    @location(15) sway: vec4<f32>,
) -> ShadowCutOut {
    let model = mat4x4<f32>(model_0, model_1, model_2, model_3);
    let posed = pose_corner(index, skin, position, vec3<f32>(0.0), vec3<f32>(0.0));
    let world = swayed((model * vec4<f32>(posed.position, 1.0)).xyz, sway,
        corner_foliage(index, skin));
    var out: ShadowCutOut;
    out.clip_position = shadow_cascade * vec4<f32>(world, 1.0);
    out.uv = mat2x2<f32>(maps.uv_matrix.xy, maps.uv_matrix.zw) * uv + maps.uv_offset.xy;
    out.cut = vec3<f32>(base_color.a, surface.z, emissive_fade.w);
    return out;
}

// A cut-out caster's pixel of a shadow map: left out where its texture is
// cut out, so leaves and strands cast their own shapes, not their cards'.
// A hair card's opacity is hashed against a fixed per-texel pattern over
// the band round its cutoff (`HAIR_HASH_BAND`, as the scene pass hashes
// it under TAA), so the shadow map's filter averages a strand's soft
// fringe into a partial shadow, not a hard slab edge.
@fragment
fn fs_shadow_cut_out(in: ShadowCutOut) {
    let texel = textureSample(base_color_texture, material_sampler, in.uv);
    let opacity_texel = textureSample(opacity_texture, material_sampler, in.uv);
    let opacity = in.cut.x * mix(texel.a, map_value(opacity_texel, maps.opacity, maps.invert.w),
        maps.settings.w);
    var threshold = in.cut.y;
    if maps.shading.x == SHADING_HAIR {
        threshold = clamp(in.cut.y + HAIR_HASH_BAND * (pixel_noise(in.clip_position.xy) - 0.5),
            0.002, 0.998);
    }
    // A caster near the end of its shadow's reach (renderer/lod.rs) casts
    // a share of it: the rest of its texels are left out by the same fixed
    // pattern, and the shadow filter averages them into a lighter shadow.
    if opacity < threshold || dithered_out(in.cut.z, in.clip_position.xy) {
        discard;
    }
}

// One map's value: the channel picked by `mask`, inverted if `invert` is 1.
fn map_value(texel: vec4<f32>, mask: vec4<f32>, invert: f32) -> f32 {
    let value = dot(texel, mask);
    return mix(value, 1.0 - value, invert);
}

// The surface normal at `uv`, tipped by the normal map if there is one,
// from the mesh's normal `n` (unit length) and `tangent`.
fn surface_normal(n: vec3<f32>, tangent: vec4<f32>, uv: vec2<f32>) -> vec3<f32> {
    let texel = textureSample(normal_texture, material_sampler, uv).xyz;
    // Skin's defined normal map, when it has a share (skin.wgsl).
    let defined = skin_defined_texel(uv);
    return mapped_normal(n, tangent, texel, defined);
}

// A normal map's texel as a direction in its tangent space; `bc5` if it
// holds only red and green.
fn unpacked_normal(texel: vec3<f32>, bc5: bool) -> vec3<f32> {
    var tipped = texel * 2.0 - 1.0;
    if bc5 {
        // A unit normal's Z from its X and Y.
        tipped.z = sqrt(saturate(1.0 - dot(tipped.xy, tipped.xy)));
    }
    return tipped;
}

// `surface_normal` from the normal map's `texel` (and, on skin with a
// share of it, the defined normal map's `defined`, blended in tangent
// space by `Maps::shading`'s y).
fn mapped_normal(n: vec3<f32>, tangent: vec4<f32>, texel: vec3<f32>, defined: vec3<f32>)
    -> vec3<f32> {
    // The tangent, made perpendicular to the normal.
    let along = tangent.xyz - n * dot(n, tangent.xyz);
    if maps.settings.x == 0.0 || dot(along, along) < 1e-12 {
        return n;
    }
    let t = normalize(along);
    let b = cross(n, t) * tangent.w;
    var tipped = unpacked_normal(texel, maps.specular.y != 0.0);
    if maps.shading.x == SHADING_SKIN && maps.shading.y > 0.0 {
        // Written so a share of 1 gives the defined map's value exactly.
        let share = maps.shading.y;
        tipped = tipped * (1.0 - share) + unpacked_normal(defined, maps.shading.w != 0.0) * share;
    }
    tipped = vec3<f32>(tipped.x, tipped.y * maps.settings.y, tipped.z);
    tipped = vec3<f32>(tipped.xy * maps.settings.x, max(tipped.z, 0.0));
    let mapped = mat3x3<f32>(t, b, n) * tipped;
    if dot(mapped, mapped) < 1e-12 {
        return n;
    }
    return normalize(mapped);
}

// What a material's `surface` function is given about the pixel it shades.
// Colors are linear light. Documented in docs/rendering/models.md.
struct SurfaceInput {
    // The texture coordinates, already tiled and offset by the material.
    uv: vec2<f32>,
    world_position: vec3<f32>,
    // The mesh's normal (unit length, facing the camera), before the normal map.
    normal: vec3<f32>,
    // Along the texture's U; w is the side its V is on. Turned with `normal`.
    tangent: vec4<f32>,
    // Seconds, for surfaces that move (`DrawList::time`).
    time: f32,
    // The item's own values, before its maps: the part's color and
    // opacity, metalness, roughness and glow.
    color: vec3<f32>,
    opacity: f32,
    metalness: f32,
    roughness: f32,
    emissive: vec3<f32>,
}

// What a `surface` function gives back, lit by the scene's lights.
struct Surface {
    base_color: vec3<f32>,
    metalness: f32,
    roughness: f32,
    // Unit length, in world space.
    normal: vec3<f32>,
    // Light given off, added after lighting.
    emissive: vec3<f32>,
    // Below the material's cutoff, the pixel is cut out. An item is only
    // see-through when its material's opacity is below 1.
    opacity: f32,
}

// The item's own material at `input`: its values times its maps, read at
// `input.uv`. A custom surface can call it and change what it gives back.
fn standard_surface(input: SurfaceInput) -> Surface {
    let uv = input.uv;
    let texel = textureSample(base_color_texture, material_sampler, uv);
    let metalness_texel = textureSample(metalness_texture, material_sampler, uv);
    let roughness_texel = textureSample(roughness_texture, material_sampler, uv);
    let emissive_texel = textureSample(emissive_texture, material_sampler, uv);
    let opacity_texel = textureSample(opacity_texture, material_sampler, uv);
    let opacity_map = map_value(opacity_texel, maps.opacity, maps.invert.w);
    var out: Surface;
    out.base_color = input.color * texel.rgb;
    // A skin between two colour maps (two photographed ages): the second
    // is in the occlusion map's place, read as colour, and `uv_offset.z`
    // of it shows. `maps` is uniform, so every pixel takes the same branch.
    if maps.shading.x == SHADING_SKIN && maps.uv_offset.z > 0.0 {
        let second = textureSample(occlusion_texture, material_sampler, uv);
        out.base_color = input.color * mix(texel.rgb, second.rgb, maps.uv_offset.z);
    }
    out.metalness = input.metalness * map_value(metalness_texel, maps.metalness, maps.invert.x);
    out.roughness = input.roughness * map_value(roughness_texel, maps.roughness, maps.invert.y);
    out.normal = surface_normal(input.normal, input.tangent, uv);
    out.emissive = input.emissive * emissive_texel.rgb;
    out.opacity = input.opacity * mix(texel.a, opacity_map, maps.settings.w);
    // Skin's pores and fine lines (skin.wgsl), finer than its maps, under
    // any makeup. `maps` is uniform, so every pixel takes the same branch.
    if maps.shading.x == SHADING_SKIN && maps.settings.x > 0.0 {
        let micro = skin_micro(out.normal, input.world_position, uv);
        out.normal = micro.xyz;
        out.base_color *= 1.0 - micro.w;
    }
    // Skin's makeup and markings (skin.wgsl): their masks are in the
    // metalness, emissive and opacity maps' places. `maps` is uniform, so
    // every pixel takes the same branch.
    if maps.shading.x == SHADING_SKIN && maps.specular.z != 0.0 {
        let layered = skin_layered(out.base_color, out.roughness, input.color, uv,
            metalness_texel, emissive_texel, opacity_texel);
        out.base_color = layered.rgb;
        out.roughness = layered.a;
        out.metalness = 0.0;
        out.emissive = input.emissive;
        // (Only with a scar on: tilting by nothing still renormalizes.)
        if maps.toon.z > 0.0 {
            out.normal = skin_scar_relief(out.normal, input.world_position,
                emissive_texel.b * maps.toon.z);
        }
    }
    return out;
}

// The material's value `index` (0 to 7: its `parameters` in name order).
// A number is in x, a color or vector in xyz (colors as written, sRGB).
fn parameter(index: i32) -> vec4<f32> {
    return maps.params[index];
}

// An sRGB color (as a Part's `Color` is written) as linear light.
fn srgb(color: vec3<f32>) -> vec3<f32> {
    return select(pow((color + 0.055) / 1.055, vec3<f32>(2.4)), color / 12.92,
        color <= vec3<f32>(0.04045));
}

// The pixel's surface: the material's `surface` function (appended after
// this file, see renderer/surface_shaders.rs) at the pixel. Only
// double-sided items show their backs, which are lit as fronts facing the
// other way, so the normal and tangent are turned to face the camera.
fn pixel_surface(in: VertexOut, facing: f32) -> Surface {
    var input: SurfaceInput;
    input.uv = in.uv;
    input.world_position = in.world_position;
    input.normal = normalize(in.normal) * facing;
    input.tangent = in.tangent * facing;
    input.time = frame.clock.x;
    input.color = in.base_color.rgb;
    input.opacity = in.base_color.a;
    input.metalness = in.surface.x;
    input.roughness = in.surface.y;
    input.emissive = in.emissive;
    var shaded = surface(input);
    // What the snow lifted is the snow: the top of the slab, and its sides
    // (the mesh's skirt), whichever way they face.
    if in.rise > 0.0 {
        let snow = snow_cap_cover(in.world_position, in.rise);
        shaded.base_color = mix(shaded.base_color, SNOW_COLOR, snow);
        shaded.metalness = mix(shaded.metalness, 0.0, snow);
        shaded.roughness = mix(shaded.roughness, SNOW_ROUGHNESS, snow);
        shaded.emissive = shaded.emissive * (1.0 - snow);
        shaded.opacity = max(shaded.opacity, snow);
    }
    return shaded;
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // Every texture is sampled before anything branches, as WGSL requires.
    let facing = select(-1.0, 1.0, front);
    let shaded = pixel_surface(in, facing);
    let occlusion_texel = textureSample(occlusion_texture, material_sampler, in.uv);
    let specular_texel = textureSample(specular_texture, material_sampler, in.uv);
    let data_texel = textureSample(metalness_texture, material_sampler, in.uv);
    let footprint = skin_footprint(in, shaded.normal, facing);

    // Cut-out materials (leaves, fences) leave holes where the texture is
    // clear (a hair card's, hashed under TAA: `hair_cut_threshold`; far
    // off, any cut-out's: `cut_threshold`).
    let blur = cut_out_blur(in.uv);
    let cut = cut_threshold(in, in.surface.z, data_texel.g, blur);
    if shaded.opacity < cut || dithered_out(in.fade, in.clip_position.xy) {
        discard;
    }
    return lit_pixel(in, facing, shaded, occlusion_texel, specular_texel, data_texel, footprint);
}

// A surface every pixel of which is drawn, in the scene pass
// (`PrefillPipelines` in renderer/types.rs). With no `discard` here the
// GPU tests depth before running this, so what's hidden is never lit;
// `fs_main`, which can discard, is run first and tested after.
//
// A cut-out surface's depth was drawn just before (`fs_depth_only`), so
// its depth test (equal) passes only its nearest layer, and only where it
// isn't cut out: the leaves hidden behind the nearest one are never lit.
// A solid item with no cutoff that isn't fading (`dithered_out`) has
// nothing `fs_main` would discard, so it's lit here with the plain test.
@fragment
fn fs_prefilled(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let facing = select(-1.0, 1.0, front);
    let shaded = pixel_surface(in, facing);
    let occlusion_texel = textureSample(occlusion_texture, material_sampler, in.uv);
    let specular_texel = textureSample(specular_texture, material_sampler, in.uv);
    let data_texel = textureSample(metalness_texture, material_sampler, in.uv);
    let footprint = skin_footprint(in, shaded.normal, facing);
    return lit_pixel(in, facing, shaded, occlusion_texel, specular_texel, data_texel, footprint);
}

// Measuring only (`AXCENITY_PLANT_PROBE`, docs/rendering/performance.md):
// scattered items' pixels in the scene pass with part of `fs_main`'s work
// left out, to tell what their time is spent on. The picture is wrong
// while one is on.
//
// `flat`: nothing read, so the vertex shader carries nothing to a corner
// but its position, and a pixel costs nothing.
@fragment
fn fs_probe_flat() -> @location(0) vec4<f32> {
    return vec4<f32>(0.18, 0.18, 0.18, 1.0);
}

// `carried`: everything the vertex shader carries is read, and nothing is
// done with it. Against `flat`, the cost of carrying it.
@fragment
fn fs_probe_carried(in: VertexOut) -> @location(0) vec4<f32> {
    let sum = in.world_position + in.normal + in.base_color.rgb + in.emissive + in.surface
        + in.tangent.xyz + in.item_middle + in.item_half_size
        + vec3<f32>(in.uv, in.fade) + vec3<f32>(in.role, in.rise, in.hangs);
    return vec4<f32>(fract(sum * 0.01), in.base_color.a + in.tangent.w);
}

// `surface`: the material's textures are read as `fs_prefilled` reads
// them, and the surface is not lit. Against `carried`, the textures'
// cost; against the pass as it is, the lighting's.
@fragment
fn fs_probe_surface(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let facing = select(-1.0, 1.0, front);
    let shaded = pixel_surface(in, facing);
    let occlusion_texel = textureSample(occlusion_texture, material_sampler, in.uv);
    let specular_texel = textureSample(specular_texture, material_sampler, in.uv);
    let data_texel = textureSample(metalness_texture, material_sampler, in.uv);
    let rest = occlusion_texel.rgb + specular_texel.rgb + data_texel.rgb + shaded.normal
        + shaded.emissive + vec3<f32>(shaded.metalness, shaded.roughness, shaded.opacity);
    return vec4<f32>(shaded.base_color + rest * 0.001, 1.0);
}

// How far mipmaps have averaged a cut-out's alpha where this pixel reads
// it: 0 while its texture is read within 2.5 levels of full size, rising to
// 1 four levels down. Nearer, the mips keep most of an atlas's shapes and
// the cutoff loses little. A leaf atlas's alpha is whole or clear, and its mips
// average that into a grey: under the cutoff a far bush thinned to
// nothing, over it it filled in solid. That grey is the share of texels
// covered, so under TAA it is kept as coverage instead (`cut_threshold`). The level is the hardware's, from the shorter of
// the pixel's two steps across the texture (anisotropic filtering reads
// along the longer one at about that level). Called before any branch:
// it takes derivatives.
fn cut_out_blur(uv: vec2<f32>) -> f32 {
    let size = vec2<f32>(select(textureDimensions(base_color_texture),
        textureDimensions(opacity_texture), maps.settings.w > 0.5));
    let across = dpdx(uv) * size;
    let down = dpdy(uv) * size;
    let level = 0.5 * log2(max(min(dot(across, across), dot(down, down)), 1e-8));
    return clamp((level - 2.5) / 1.5, 0.0, 1.0);
}

// The opacity under which a cut-out's pixel at `pixel` is cut: a hair
// card's (`hair_cut_threshold`), or a Standard cut-out's cutoff. While TAA
// is on (`frame.noise.y`), a Standard cut-out's drifts with `blur`
// (`cut_out_blur`) toward a threshold that changes from pixel to pixel and
// frame to frame (hashed alpha), so the share of pixels kept is the
// averaged alpha, the share of the texture covered, and TAA averages them
// smooth. Without TAA it stays the cutoff: nothing averages a still pattern,
// and MSAA's alpha to coverage or a fixed hash showed as a dither over every
// far canopy (a fixed 2×2 pattern on RADV) where the cutoff's solid crowns
// had looked right.
fn cut_threshold(in: VertexOut, cutoff: f32, strand: f32, blur: f32) -> f32 {
    if maps.shading.x == SHADING_HAIR {
        return hair_cut_threshold(in, cutoff, strand);
    }
    return far_cut_threshold(cutoff, blur, in.clip_position.xy);
}

// `cut_threshold` for a Standard cut-out.
fn far_cut_threshold(cutoff: f32, blur: f32, pixel: vec2<f32>) -> f32 {
    if cutoff <= 0.0 || blur <= 0.0 || frame.noise.y == 0.0 {
        return cutoff;
    }
    let noise = fract(pixel_noise(pixel) + 0.618034 * frame.noise.x);
    return mix(cutoff, clamp(noise, 0.002, 0.998), blur);
}

// A cut-out surface's opacity at this pixel, as `standard_surface` and
// `fs_main` work it out.
fn cut_out_opacity(in: VertexOut) -> f32 {
    let texel = textureSample(base_color_texture, material_sampler, in.uv);
    let opacity_texel = textureSample(opacity_texture, material_sampler, in.uv);
    let opacity_map = map_value(opacity_texel, maps.opacity, maps.invert.w);
    return in.base_color.a * mix(texel.a, opacity_map, maps.settings.w);
}

// Draws a cut-out surface's depth before the scene is lit, with one sample
// per pixel. What it cuts out must match `standard_surface` and `fs_main`.
@fragment
fn fs_depth_only(in: VertexOut) -> @location(0) vec4<f32> {
    let strand = textureSample(metalness_texture, material_sampler, in.uv).g;
    let blur = cut_out_blur(in.uv);
    let cut = cut_threshold(in, in.surface.z, strand, blur);
    if cut_out_opacity(in) < cut || dithered_out(in.fade, in.clip_position.xy) {
        discard;
    }
    return vec4<f32>(0.0);
}

// `fs_depth_only` when multisampled. MSAA smooths the edges of triangles,
// not the holes a texture cuts in them, so the returned alpha says how much
// of the pixel the surface covers and the GPU writes depth to that share of
// its samples (alpha to coverage). The cut-off's edge is ramped over one
// pixel, at the same place as `fs_depth_only`'s, so distant leaves don't thin
// out. `fs_prefilled` then lights just the samples this left its depth in.
//
// A hair card's coverage is its opacity itself, so its soft edges and
// thin strands cover a graded share of each pixel's samples.
@fragment
fn fs_depth_coverage(in: VertexOut) -> @location(0) vec4<f32> {
    let opacity = cut_out_opacity(in);
    var coverage = (opacity - in.surface.z) / max(fwidth(opacity), 0.0001) + 0.5;
    if maps.shading.x == SHADING_HAIR {
        coverage = select(opacity, 0.0, opacity < 0.002);
    }
    if coverage <= 0.0 || dithered_out(in.fade, in.clip_position.xy) {
        discard;
    }
    return vec4<f32>(0.0, 0.0, 0.0, min(coverage, 1.0));
}

// The lit color of a pixel of a surface: `shaded`, its two remaining maps'
// texels already read, and (for skin) its `footprint`.
// `data_texel` is the metalness map's texel, which on hair is the card
// atlas's data (and `specular_texel` its flow).
fn lit_pixel(
    in: VertexOut,
    facing: f32,
    shaded: Surface,
    occlusion_texel: vec4<f32>,
    specular_texel: vec4<f32>,
    data_texel: vec4<f32>,
    footprint: SkinFootprint,
) -> vec4<f32> {
    let metalness = clamp(shaded.metalness, 0.0, 1.0);
    // Perfectly smooth surfaces make the highlight infinitely small, so keep a minimum.
    let roughness = clamp(shaded.roughness, 0.04, 1.0);
    // The map's, and for solid items the screen-space ambient occlusion's.
    let solid = in.base_color.a >= 1.0;
    let mapped = mix(
        1.0, map_value(occlusion_texel, maps.occlusion, maps.invert.z), maps.settings.z);
    let screen = select(1.0, screen_occlusion(in.clip_position.xy), solid);
    // A plant's own baked occlusion (foliage W3b): darker deep in a crown
    // and in the crotches of its limbs.
    let foliage = in.foliage.y >= 0.0;
    let baked = select(1.0, clamp(in.foliage.x, 0.0, 1.0), foliage);
    let occlusion = mapped * screen * baked;
    // Non-metals reflect about 4% of light, or as their specular map says.
    let dielectric = mix(vec3<f32>(0.04), 0.08 * specular_texel.rgb, maps.specular.x);

    let geometric = normalize(in.normal) * facing;
    var color: vec3<f32>;
    if maps.shading.x == SHADING_UNLIT {
        color = shaded.base_color;
    } else if maps.shading.x == SHADING_TOON {
        color = toon_surface(in.world_position, geometric, shaded.normal, in.clip_position.xy,
            shaded.base_color, occlusion, solid);
    } else if maps.shading.x == SHADING_SKIN {
        color = skin_surface(in, facing, geometric, shaded, roughness, occlusion, solid, footprint);
    } else if maps.shading.x == SHADING_HAIR {
        color = hair_surface(in, facing, shaded, roughness, occlusion, solid, data_texel,
            specular_texel);
    } else if maps.shading.x == SHADING_CLOTH {
        color = cloth_surface(in, facing, geometric, shaded, metalness, roughness, occlusion,
            dielectric, solid);
    } else if maps.shading.y > 0.0 && in.hangs > 0.5 && maps.shading.z < 0.5 {
        color = lit_thin_cloth(in, shaded, geometric, metalness, roughness, occlusion, dielectric,
            solid, maps.shading.y);
    } else if maps.shading.y > 0.0 {
        // A crown of leaf cards isn't a solid ball: light comes through its
        // leaves and its holes, and `lit_leaf` shades its inside and its foot
        // itself, so screen-space occlusion counts for a share of its strength.
        color = lit_leaf(in, shaded, geometric, metalness, roughness,
            mapped * pow(screen, LEAF_SCREEN_OCCLUSION) * baked, dielectric, solid, maps.shading.y,
            maps.shading.z > 0.5);
    } else {
        // Bump shadowing only with a normal map (`maps.settings.x`, its
        // scale, is 0 without one).
        // Where its ambient occlusion map is dark, it lies low: water
        // gathers there first when it's wet.
        var cavity = -1.0;
        if maps.settings.z > 0.0 {
            let open = map_value(occlusion_texel, maps.occlusion, maps.invert.z);
            cavity = saturate((1.0 - open) * WET_CAVITY_FROM_OCCLUSION * maps.settings.z);
            // Its own relief shades slanting sunlight in its cracks.
            micro_open = mapped;
        }
        // A tree's bark greys where its mask says it's weathered.
        var base_color = shaded.base_color;
        if foliage {
            base_color = weathered_wood(base_color, in.foliage.y);
        }
        color = light_surface_low(in.world_position, geometric, shaded.normal, in.clip_position.xy,
            base_color, metalness, roughness, occlusion, dielectric, solid,
            maps.settings.x != 0.0, cavity, true);
    }

    // Solid items stay solid whatever the texture's alpha, so the window's
    // own alpha stays 1.
    let alpha = select(clamp(shaded.opacity, 0.0, 1.0), 1.0, solid);
    let seen = fogged(frame.fog, frame.camera_position.xyz, in.world_position, color + shaded.emissive);
    return vec4<f32>(seen, alpha);
}

// How far bark with a weathering mask of 1 goes toward silver-grey wood
// (`weathered_wood`), and how much paler that is than the bark's own
// brightness.
const WOOD_WEATHERING: f32 = 0.65;
const WEATHERED_WOOD_LIFT: f32 = 1.35;

// Bark's colour where it's weathered by `amount` (0 to 1, a tree's
// foliage mask): stripped and greyed, as dead wood bleaches in the sun.
fn weathered_wood(color: vec3<f32>, amount: f32) -> vec3<f32> {
    let grey = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    let silver = vec3<f32>(grey * WEATHERED_WOOD_LIFT + 0.04) * vec3<f32>(1.0, 0.98, 0.94);
    return mix(color, min(silver, vec3<f32>(1.0)), clamp(amount, 0.0, 1.0) * WOOD_WEATHERING);
}

// How dark an ambient occlusion map must be (times this) for water to
// gather there as in the deepest crack (`light_surface_low`).
const WET_CAVITY_FROM_OCCLUSION: f32 = 1.6;
// The smoothest a film of water leaves a leaf (`lit_leaf`).
const WET_LEAF_ROUGHNESS: f32 = 0.4;

// How much of what a translucent surface passes on it no longer reflects.
const LEAF_REFLECTION_LOSS: f32 = 0.6;

// How much of screen-space ambient occlusion's darkening a leaf takes (an
// exponent on it). The prepass sees a crown as a heap of solid cards, so
// each leaf in it was darkened as if it were deep in a solid ball, on top of
// the crown's own shade (`CANOPY_DEPTH_SHADE`, `LEAF_FOOT_SHADE`): at the
// Overgrown Street showcase's strength (1.6) every crown went near black.
// Leaves pass light and the cards have holes, so they take half of it
// (its square root): enough to keep the clusters of leaves apart (0.35
// flattened a forest seen from above).
const LEAF_SCREEN_OCCLUSION: f32 = 0.5;

// How much of each color a leaf lets through, compared with what it
// reflects: green and red pass, blue is taken, so lit from behind it glows
// a brighter, yellower green.
const THROUGH_LEAF_TINT: vec3<f32> = vec3<f32>(1.0, 1.12, 0.35);

// How far a leaf's light is turned from its own card toward the outside of
// its plant. Foliage is a volume: it's lit by where in the crown it is far
// more than by which way one card happens to face.
const CANOPY_BEND: f32 = 0.6;
// How much sky reaches the very middle of a crown, compared with its outside.
const CANOPY_DEPTH_SHADE: f32 = 0.5;
// A crown's mass sits above the middle of the plant, by this share of half
// its height: the trunk or stems are below.
const CANOPY_LIFT: f32 = 0.3;
// How rough a leaf is at least, on the outside of a crown and deep inside it.
// Rough enough, a leaf seen edge-on doesn't mirror a white sky as grey.
const LEAF_ROUGHNESS_OUTSIDE: f32 = 0.72;
const LEAF_ROUGHNESS_INSIDE: f32 = 0.9;
// How far what lies on a leaf (rain, snow) follows its crown's shape and
// not its own card's, and how much further up the outside of a crown faces
// for it: snow caps a bush.
const LEAF_SNOW_CROWN: f32 = 0.65;
const LEAF_SNOW_LIFT: f32 = 0.45;
// How far a low plant's leaves face up for the light, whichever way their
// cards face (added to the bent normal before it's made unit length), and
// the heights in studs between which that fades out: grass and low brush
// are ground cover, lit like the ground they stand on. An upright card
// lit by the way it faces takes little of a high sun and most of the
// horizon's light, so pale grass looked dull at noon and glowed at dusk
// while the ground around it went dark. A tree's crown keeps its own shape.
const LEAF_GROUND_LIFT: f32 = 0.8;
const LEAF_GROUND_HEIGHTS: vec2<f32> = vec2<f32>(3.0, 9.0);
// How much of the surrounding light reaches the foot of a plant, and how far
// up it (of its height) the shade lasts: its own leaves and stems stand in
// the way, so it's darker toward the ground and sits on it. Ground cover
// (`LEAF_GROUND_HEIGHTS`) is shaded more: a tuft of grass or a fern is
// densest at its foot, and its foot is what meets the ground. Screen-space
// occlusion, which had darkened it there, counts for less on leaves now
// (`LEAF_SCREEN_OCCLUSION`).
const LEAF_FOOT_SHADE: f32 = 0.55;
const LEAF_FOOT_SHADE_LOW: f32 = 0.3;
const LEAF_FOOT_REACH: f32 = 0.5;
// How far the sky that comes through a leaf is taken from above rather than
// from straight behind it (added to the far side's direction before it's
// made unit length): light comes down through a crown or a tuft from leaf
// to leaf, so even a leaf whose far side faces the ground glows a little.
const LEAF_SKY_WRAP: f32 = 0.35;
// How much individual leaves differ in brightness and in hue.
const LEAF_TONE_VARIATION: f32 = 0.2;
const LEAF_HUE_VARIATION: f32 = 0.18;

// Where a leaf is in its plant.
struct Canopy {
    // Away from the crown's middle, in world space.
    normal: vec3<f32>,
    // 0 deep inside the crown, 1 on its outside.
    openness: f32,
}

// The crown is taken as the ball filling the item, a little above its middle.
fn leaf_canopy(world_position: vec3<f32>, middle: vec3<f32>, half_size: vec3<f32>, fallback: vec3<f32>)
    -> Canopy {
    let safe = max(half_size, vec3<f32>(0.001));
    let along = (world_position - middle) / safe - vec3<f32>(0.0, CANOPY_LIFT, 0.0);
    let reach = length(along);
    if reach < 0.001 {
        return Canopy(fallback, 0.0);
    }
    return Canopy(along / reach, smoothstep(0.2, 0.95, reach));
}

// A leaf's own brightness and hue: every card faces its own way, so the way
// it faces tells them apart without any data of its own, and where it is
// shifts whole sprays of them together, or its cluster's shade (`mask`, 0
// to 1, a tree's foliage data; below 0 for none) does. Smooth, so a bent
// card has no seams.
fn leaf_tone(unflipped_normal: vec3<f32>, world_position: vec3<f32>, base_color: vec3<f32>,
    mask: f32) -> vec3<f32> {
    let n = unflipped_normal;
    let spray = select(sin(dot(world_position, vec3<f32>(1.13, 0.71, 1.37))), 2.0 * mask - 1.0,
        mask >= 0.0);
    let tone = 0.65 * sin(dot(n, vec3<f32>(41.3, 67.1, 29.7))) + 0.35 * spray;
    let hue = 0.65 * sin(dot(n, vec3<f32>(23.9, 13.7, 57.1))) + 0.35 * spray;
    // Olive one way, blue-green the other.
    let tint = vec3<f32>(1.0 + LEAF_HUE_VARIATION * hue, 1.0, 1.0 - LEAF_HUE_VARIATION * hue);
    return base_color * tint * (1.0 + LEAF_TONE_VARIATION * tone);
}

// A leaf's light: reflected, and come through it from behind.
fn lit_leaf(
    in: VertexOut,
    shaded: Surface,
    geometric: vec3<f32>,
    metalness: f32,
    roughness: f32,
    occlusion: f32,
    dielectric: vec3<f32>,
    solid: bool,
    amount: f32,
    // A leaf with a sheen of its own (ivy; `Material::glossy_leaf`): at its
    // own roughness, not raised to a leaf's least.
    glossy_leaf: bool,
) -> vec3<f32> {
    let canopy = leaf_canopy(in.world_position, in.item_middle, in.item_half_size, shaded.normal);
    let low = 1.0 - smoothstep(LEAF_GROUND_HEIGHTS.x, LEAF_GROUND_HEIGHTS.y,
        2.0 * in.item_half_size.y);
    let crown = mix(shaded.normal, canopy.normal, CANOPY_BEND);
    let bent = normalize(crown + vec3<f32>(0.0, LEAF_GROUND_LIFT * low, 0.0));
    // The sky comes through from the leaf's far side in its crown, not lifted
    // as a low plant's light is (lifted, a tuft's far side faced the ground,
    // so grass let through only the ground's dark and was darker than if it
    // let nothing through), and partly from above (`LEAF_SKY_WRAP`).
    let away = -crown / max(length(crown), 1e-4);
    let far = normalize(away + vec3<f32>(0.0, LEAF_SKY_WRAP, 0.0));
    let base_color = leaf_tone(normalize(in.normal), in.world_position, shaded.base_color,
        in.foliage.y);
    // Darker toward its foot, where the plant shades itself, and deep in
    // its crown; a tree with its own baked occlusion (foliage W3b) has that
    // in `occlusion` already, worked out from its crown's real shape.
    let foot = in.item_middle.y - in.item_half_size.y;
    let up_it = (in.world_position.y - foot) / max(2.0 * in.item_half_size.y, 0.001);
    let open = select(mix(CANOPY_DEPTH_SHADE, 1.0, canopy.openness)
        * mix(mix(LEAF_FOOT_SHADE, LEAF_FOOT_SHADE_LOW, low), 1.0,
            smoothstep(0.0, LEAF_FOOT_REACH, up_it)), 1.0, in.foliage.y >= 0.0);
    let rough = select(max(roughness,
        mix(LEAF_ROUGHNESS_INSIDE, LEAF_ROUGHNESS_OUTSIDE, canopy.openness)), roughness,
        glossy_leaf);
    // Rain and snow lie on the plant, not on each card of it: on the top of
    // its crown, the way the crown faces there, and more on the cards that
    // face up themselves. No water stands on leaves.
    let lie = normalize(mix(shaded.normal, canopy.normal, LEAF_SNOW_CROWN)
        + vec3<f32>(0.0, LEAF_SNOW_LIFT * canopy.openness, 0.0));
    var weather = weathered_lying(in.world_position, lie, false, false, -1.0, bent, base_color,
        metalness, rough);
    // A wet leaf has a sheen, never a mirror's shine.
    weather.roughness = max(weather.roughness, min(rough, WET_LEAF_ROUGHNESS));
    // `bent` leans by the crown, not by bumps: no bump shadowing.
    let reflected_light = shade_surface(in.world_position, geometric, weather.normal,
        in.clip_position.xy, weather.base_color, weather.metalness, weather.roughness,
        occlusion * mix(open, 1.0, weather.snow), dielectric, solid, false);
    // What passes through isn't also reflected, and nothing passes snow.
    let through = amount * (1.0 - weather.snow);
    return reflected_light * (1.0 - LEAF_REFLECTION_LOSS * through)
        + through_thin(in.world_position, geometric, shaded.normal, far, in.clip_position.xy,
            weather.base_color, THROUGH_LEAF_TINT, occlusion, open, through);
}

// Thin cloth that hangs (a drape, washing on a line) with light coming
// through it: lit by the way it faces and in its own colors, with none of
// what makes a leaf a leaf (the crown's shape, the tones from leaf to leaf,
// the green of the light that comes through). Before 2026-11-23 a drape
// with a translucency was lit as a leaf, and glowed yellow-green in the sun.
fn lit_thin_cloth(
    in: VertexOut,
    shaded: Surface,
    geometric: vec3<f32>,
    metalness: f32,
    roughness: f32,
    occlusion: f32,
    dielectric: vec3<f32>,
    solid: bool,
    amount: f32,
) -> vec3<f32> {
    let weather = weathered(in.world_position, geometric, shaded.normal, shaded.base_color,
        metalness, roughness);
    let reflected_light = shade_surface(in.world_position, geometric, weather.normal,
        in.clip_position.xy, weather.base_color, weather.metalness, weather.roughness,
        occlusion, dielectric, solid, false);
    let through = amount * (1.0 - weather.snow);
    return reflected_light * (1.0 - LEAF_REFLECTION_LOSS * through)
        + through_thin(in.world_position, geometric, shaded.normal, -shaded.normal,
            in.clip_position.xy, weather.base_color, vec3<f32>(1.0), occlusion, 1.0, through);
}

// The light that comes through a thin surface from its far side, from the
// sun and from the sky behind it, scattered evenly toward the viewer. The
// sun lights each leaf by its own card (`n`), a sharp thing; the sky by the
// crown around it (`far`, the way its far side faces, and `open`), a soft
// one. Occlusion counts by half: what's close in front of a leaf doesn't
// block what reaches it from behind. What passes is at most what the
// surface doesn't reflect (`LEAF_REFLECTION_LOSS`), so a pale leaf with a
// strong tint can't give back more than reaches it.
fn through_thin(
    world_position: vec3<f32>,
    geometric: vec3<f32>,
    n: vec3<f32>,
    far: vec3<f32>,
    pixel: vec2<f32>,
    base_color: vec3<f32>,
    tint: vec3<f32>,
    occlusion: f32,
    open: f32,
    amount: f32,
) -> vec3<f32> {
    let sun_visible = soft_sun_visibility(world_position, -geometric, pixel)
        * cloud_shadow(frame.clouds, world_position, frame.sun_direction.xyz);
    let from_sun = frame.sun_radiance.rgb * sun_visible
        * max(dot(-n, frame.sun_direction.xyz), 0.0) / PI;
    let from_sky = surrounding_diffuse(world_position, far, frame.sky_top.w) * open;
    let reflects = base_color * (1.0 - LEAF_REFLECTION_LOSS * amount);
    let passes = min(amount * base_color * tint, max(vec3<f32>(1.0) - reflects, vec3<f32>(0.0)));
    return passes * mix(1.0, occlusion, 0.5) * (from_sun + from_sky);
}

// `Maps::shading`'s models. Must match `SHADING_*` in renderer/types.rs.
const SHADING_STANDARD: f32 = 0.0;
const SHADING_UNLIT: f32 = 1.0;
const SHADING_TOON: f32 = 2.0;
const SHADING_SKIN: f32 = 3.0;
const SHADING_HAIR: f32 = 4.0;
const SHADING_CLOTH: f32 = 5.0;

// How far round from the camera a toon surface's rim light starts (in
// 1 - cos of the angle), and how large a highlight of size 1 is (its
// half-angle in radians).
const TOON_RIM_WIDTH: f32 = 0.35;
const TOON_HIGHLIGHT_ANGLE: f32 = 0.5;

// Cuts `x` (0 to 1) into the material's bands, or reads its ramp at `x`
// (its middle row, left to right, one pixel unblended).
fn toon_band(x: f32) -> vec3<f32> {
    let light = clamp(x, 0.0, 1.0);
    let steps = maps.shading.y;
    if steps < 1.0 {
        let size = textureDimensions(specular_texture);
        let at = vec2<u32>(u32(round(light * f32(size.x - 1u))), size.y / 2u);
        return textureLoad(specular_texture, at, 0).rgb;
    }
    // Any light at all lifts a point to the first band, and each band's
    // lower edge blends over `softness` of a band.
    let scaled = light * steps;
    let below = floor(scaled);
    let blend = clamp((scaled - below) / max(maps.shading.z, 1e-4), 0.0, 1.0);
    return vec3<f32>(min((below + blend) / steps, 1.0));
}

// 0 below `edge`, 1 above it, blended over `softness` of `width`.
fn toon_step(value: f32, edge: f32, width: f32) -> f32 {
    return clamp((value - edge) / max(maps.shading.z * width, 1e-4), 0.0, 1.0);
}

// One light on a toon surface: its banded share (how squarely it falls,
// times its shadow) of the base color, plus the highlight, in its color.
fn toon_light(n: vec3<f32>, v: vec3<f32>, arrival: Arrival, base_color: vec3<f32>) -> vec3<f32> {
    let n_dot_l = max(dot(n, arrival.l), 0.0);
    let diffuse = base_color / PI * toon_band(n_dot_l * arrival.visible);
    let h = normalize(v + arrival.l);
    let cut = cos(maps.shading.w * TOON_HIGHLIGHT_ANGLE);
    let highlight = toon_step(dot(n, h), cut, 1.0 - cut) * arrival.visible
        * select(0.0, 1.0, n_dot_l > 0.0 && maps.shading.w > 0.0);
    return (diffuse + vec3<f32>(highlight / PI)) * arrival.radiance;
}

// The light a toon surface gives back toward the camera, from the same
// lights and shadows as `light_surface` (the weather too), but cut into
// bands, with a rim light around its outline and flat ambient light.
fn toon_surface(
    world_position: vec3<f32>,
    geometric: vec3<f32>,
    normal: vec3<f32>,
    pixel: vec2<f32>,
    base_color: vec3<f32>,
    occlusion: f32,
    solid: bool,
) -> vec3<f32> {
    let weather = weathered(world_position, geometric, normal, base_color, 0.0, 1.0);
    let n = weather.normal;
    let color = weather.base_color;
    let v = normalize(frame.camera_position.xyz - world_position);

    let sun_parts = sun_arrival_parts(world_position, geometric, pixel, solid);
    let sun = sun_parts.arrival;
    var lit = toon_light(n, v, sun, color);
    let flash = lightning_arrival(world_position, sun.l, sun_parts.shadow);
    if flash.radiance.b > 0.0 {
        lit += toon_light(n, v, flash, color) + lightning_fill(world_position, n, color, flash);
    }
    let lamps = tile_lamps(pixel);
    var slot = 0u;
    for (var i = 0u; i < lamps.count; i++) {
        let lamp = lamp_arrival(lamps, i, slot, world_position, geometric, pixel, solid);
        slot += lamp_slots(lamps, i);
        if lamp.visible > 0.0 {
            lit += toon_light(n, v, lamp, color);
        }
    }

    let surrounding = frame.ambient.rgb
        + surrounding_diffuse(world_position, n, frame.sky_top.w);
    let ambient = occlusion * surrounding * color;
    // The rim catches the sun (where it isn't shadowed) and the sky.
    let rim = maps.toon.x * toon_step(1.0 - max(dot(n, v), 0.0), 1.0 - TOON_RIM_WIDTH,
        TOON_RIM_WIDTH) * (sun.radiance * sun.visible / PI + surrounding);
    return lit + ambient + rim;
}

// Whether a fading item leaves out the pixel at `pixel`, in a fine pattern,
// so two meshes cross-fade without sorting or blending (level of detail, and
// foliage near its cull distance). A fade of 1 draws every pixel. Below 1,
// only the pixels whose noise is under it are drawn; above 1, only the
// others. So a mesh fading in at `t` and one fading out at `1 + t` draw
// each pixel exactly once between them.
fn dithered_out(fade: f32, pixel: vec2<f32>) -> bool {
    let noise = pixel_noise(pixel);
    // The comparison is in brackets so WGSL doesn't read `noise < ... >` as a
    // template list.
    return select(noise >= fade, (noise < fade - 1.0), fade > 1.0);
}

// The depth and normal prepass, for ambient occlusion: solid items' depth
// and view-space normals. Cut-out pixels are left out, as in `fs_main`.
@fragment
fn fs_prepass(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return prepass_pixel(in, front, false);
}

// `fs_prepass` for the standard material's items, whose cut-outs the scene
// pass lights over the prepass's depth (their pixels are marked as such).
@fragment
fn fs_prepass_standard(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return prepass_pixel(in, front, true);
}

fn prepass_pixel(in: VertexOut, front: bool, standard: bool) -> vec4<f32> {
    let facing = select(-1.0, 1.0, front);
    let shaded = pixel_surface(in, facing);
    let strand = textureSample(metalness_texture, material_sampler, in.uv).g;
    let blur = cut_out_blur(in.uv);
    let cut = cut_threshold(in, in.surface.z, strand, blur);
    if shaded.opacity < cut || dithered_out(in.fade, in.clip_position.xy) {
        discard;
    }
    // As the weather leaves it: wet, it's smoother, and rain rings it.
    let weather = weathered_prepass(in.world_position, normalize(in.normal) * facing,
        shaded.roughness);
    var normal = prepass_normal(weather.normal);
    // A plain solid pixel keeps its roughness, 1 plus it, for ray traced
    // reflections (ray_reflections.wgsl); anything reading the flags below
    // sees it as solid (1 or more).
    normal.w = 1.0 + clamp(weather.roughness, 0.0, 0.999);
    // A character's pixels are marked for projected decals (decal.wgsl).
    if in.role == ROLE_CHARACTER {
        normal.w = select(0.5, PREPASS_CHARACTER_CUT_OUT, standard && in.surface.z > 0.0);
    } else if in.surface.z > 0.0 {
        // A cut-out's pixel, for ray traced shadows and AO
        // (ray_shadows.wgsl's `cut_out_near`); decals treat it as solid.
        normal.w = select(PREPASS_CUT_OUT, PREPASS_STANDARD_CUT_OUT, standard);
    }
    return normal;
}

// Motion vectors for items that moved since last frame (renderer/temporal.rs):
// how far each pixel of a moved item went across the screen, drawn over
// the camera's motion where the prepass shows the item.

// Must match the start of `TemporalUniform` in renderer/temporal.rs: this
// frame's and last frame's views, without TAA's jitter.
struct MotionView {
    view_proj: mat4x4<f32>,
    previous_view_proj: mat4x4<f32>,
    // Last frame's time and wind (as `Frame::clock`), for swayed plants.
    previous_clock: vec4<f32>,
}

@group(3) @binding(0) var<uniform> motion_view: MotionView;
// Last frame's posed corners (renderer/posing.rs copies them aside).
@group(3) @binding(1) var<storage, read> previous_corners: array<u32>;

// A moved item: where it is now and where it was. Must match
// `MotionInstance` in renderer/temporal.rs.
struct MotionIn {
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(6) previous_0: vec4<f32>,
    @location(7) previous_1: vec4<f32>,
    @location(8) previous_2: vec4<f32>,
    @location(9) previous_3: vec4<f32>,
    @location(10) skin: vec2<u32>,
    @location(11) sway: vec4<f32>,
    @location(12) fade: f32,
}

struct MotionOut {
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) now: vec4<f32>,
    @location(1) before: vec4<f32>,
    @location(2) @interpolate(flat) fade: f32,
}

// Corner `index` where it was last frame: `skin.y` is 0 for an item that
// isn't posed (at rest), 1 for one posed this frame and not the last (where
// it is now), `FOLIAGE_CORNERS` for one at rest with foliage data, else 2
// more than its first corner in last frame's posed corners.
fn previous_corner(index: u32, skin: vec2<u32>, rest: vec3<f32>) -> vec3<f32> {
    if skin.y == 0u || skin.y == FOLIAGE_CORNERS {
        return rest;
    }
    if skin.y == 1u {
        return posed_position(skin.x + index);
    }
    let word = (skin.y - 2u + index) * POSED_WORDS;
    return bitcast<vec3<f32>>(vec3<u32>(previous_corners[word], previous_corners[word + 1u],
        previous_corners[word + 2u]));
}

@vertex
fn vs_motion(@builtin(vertex_index) index: u32, @location(0) position: vec3<f32>,
    instance: MotionIn) -> MotionOut {
    let model = mat4x4<f32>(instance.model_0, instance.model_1, instance.model_2, instance.model_3);
    let previous = mat4x4<f32>(instance.previous_0, instance.previous_1, instance.previous_2,
        instance.previous_3);
    // Placed as `item_vertex` places it, so the depth matches the prepass's.
    let posed = pose_corner(index, instance.skin, position, vec3<f32>(0.0), vec3<f32>(0.0));
    let foliage = corner_foliage(index, instance.skin);
    let world = vec4<f32>(swayed((model * vec4<f32>(posed.position, 1.0)).xyz, instance.sway,
        foliage), 1.0);
    let was = previous_corner(index, instance.skin, position);
    let world_before = vec4<f32>(swayed_by((previous * vec4<f32>(was, 1.0)).xyz, instance.sway,
        motion_view.previous_clock, foliage.yz), 1.0);
    var out: MotionOut;
    out.clip_position = frame.view_proj * world;
    out.now = motion_view.view_proj * world;
    out.before = motion_view.previous_view_proj * world_before;
    out.fade = instance.fade;
    return out;
}

// How far the pixel moved since last frame, in texture coordinates (0 to 1
// across the scene, down the screen).
@fragment
fn fs_motion(in: MotionOut) -> @location(0) vec4<f32> {
    if dithered_out(in.fade, in.clip_position.xy) {
        discard;
    }
    let now = in.now.xy / in.now.w;
    let before = in.before.xy / in.before.w;
    return vec4<f32>((now - before) * vec2<f32>(0.5, -0.5), 0.0, 0.0);
}

// The sky's clouds as the traced clouds' passes left them, a texel for each
// of the frame's pixels (rgb: the light they add, a: the share of the sky
// behind that shows, or below 0 where they weren't traced), read only by
// the sky pass (renderer/cloud_trace.rs).
@group(2) @binding(0) var traced_clouds: texture_2d<f32>;

// A full-screen triangle at the far plane, for drawing the sky.
struct SkyOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) clip_xy: vec2<f32>,
}

@vertex
fn vs_sky(@builtin(vertex_index) index: u32) -> SkyOut {
    let xy = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    var out: SkyOut;
    out.clip_position = vec4<f32>(xy, 1.0, 1.0);
    out.clip_xy = xy;
    return out;
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    let far = frame.inv_view_proj * vec4<f32>(in.clip_xy, 1.0, 1.0);
    let dir = normalize(far.xyz / far.w - frame.camera_position.xyz);
    // How far the direction turns from one pixel to the next: about the
    // angle each pixel covers (taken here, where every pixel runs together).
    let spread = length(fwidth(dir));
    var sky = sky_radiance(dir, spread);
    if frame.clouds.color.w != 0.0 {
        // Traced in passes of their own (renderer/cloud_trace.rs), which
        // leave this pixel's clouds in a texture: unless it was left out
        // there (an alpha below 0: a pixel the prepass covered and this
        // pass didn't), and then they're worked out here as without tracing.
        var layer = vec4<f32>(0.0, 0.0, 0.0, -1.0);
        if frame.clouds.quality.w != 0.0 {
            layer = textureLoad(traced_clouds, vec2<i32>(in.clip_position.xy), 0);
        }
        if layer.a < 0.0 {
            layer = cloud_layer(frame.clouds, frame.camera_position.xyz, dir, frame.sun_direction.xyz,
                frame.sun_radiance.rgb, frame.sky_top.rgb, frame.sky_horizon.rgb,
                pixel_noise(in.clip_position.xy));
        }
        sky = sky * layer.a + layer.rgb;
        sky += lightning_sky(frame.camera_position.xyz, dir);
    }
    return vec4<f32>(fogged_along(frame.fog, frame.camera_position.xyz, dir, FOG_SKY_DISTANCE, sky), 1.0);
}
