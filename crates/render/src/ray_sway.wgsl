// Swaying plants in the ray traced scene (renderer/ray_sway.rs,
// docs/rendering/ray-tracing.md): each corner of a swaying item's mesh
// bent by the wind exactly as `vs_main` and `vs_shadow` bend it
// (sway.wgsl's `swayed_at`), where it stands in the world, as one geometry
// of a BLAS it shares with other bent items. One dispatch bends every copy
// of one mesh (a run). Compiled after clouds.wgsl and sway.wgsl.

// Must match `RunRaw` in renderer/ray_sway.rs.
struct BentRun {
    // As `Frame::clock`: the time, then the wind.
    clock: vec4<f32>,
    // x: how many corners a copy has, y: how many f32s each corner of
    // `corners` takes, z: the first copy's item in `items`, w: its first
    // corner in `positions`.
    counts: vec4<u32>,
    // x: how many copies, y: 1 if the mesh has foliage data (`foliage`).
    copies: vec4<u32>,
}

// Must match `ItemRaw` in renderer/ray_sway.rs.
struct BentItem {
    // The rows of the item's own space to the world.
    model: mat3x4<f32>,
    // As `InstanceRaw::sway`.
    sway: vec4<f32>,
}

@group(0) @binding(0) var<uniform> run: BentRun;
// The mesh's corners (`Vertex`s; the position comes first).
@group(0) @binding(1) var<storage, read> corners: array<f32>;
// Every copy's bent positions in the world, three f32s each, for the BLAS.
@group(0) @binding(2) var<storage, read_write> positions: array<f32>;
@group(0) @binding(3) var<storage, read> items: array<BentItem>;
// The mesh's foliage data, a word a corner (renderer/foliage.rs), when
// `run.copies.y` is 1: each corner's shares of the lean and the flutter,
// as `vs_main` reads them from the frame's table (`corner_foliage`).
@group(0) @binding(4) var<storage, read> foliage: array<u32>;

// Corners a row of workgroups covers (65535 workgroups of 64).
const ROW: u32 = 4194240u;

@compute @workgroup_size(64)
fn cs_bend(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x + id.y * ROW;
    let per_copy = run.counts.x;
    if index >= per_copy * run.copies.x {
        return;
    }
    let item = items[run.counts.z + index / per_copy];
    let at = (index % per_copy) * run.counts.y;
    let rest = vec4<f32>(corners[at], corners[at + 1u], corners[at + 2u], 1.0);
    // A vector times a matrix of rows: each row dotted with it.
    let world = rest * item.model;
    var weights = vec2<f32>(-1.0, 1.0);
    if run.copies.y == 1u {
        weights = unpack4x8unorm(foliage[index % per_copy]).yz;
    }
    let moved = swayed_by(world, item.sway, run.clock, weights);
    let to = (run.counts.w + index) * 3u;
    positions[to] = moved.x;
    positions[to + 1u] = moved.y;
    positions[to + 2u] = moved.z;
}
