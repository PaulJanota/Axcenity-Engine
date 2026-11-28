// Occlusion culling (renderer/occlusion_cull.rs): a depth pyramid of what
// the prepass has drawn so far, and the scattered items after it tested
// against it, those wholly behind it left out of the prepass and the
// scene pass through indirect draws. A pyramid texel holds the farthest
// depth of the pixels it covers, so a test against it can only keep what
// is hidden, never leave out what shows.

// ---- The depth pyramid ----------------------------------------------------

@group(0) @binding(0) var source_depth: texture_depth_2d;
@group(0) @binding(1) var source_level: texture_2d<f32>;
@group(0) @binding(2) var destination: texture_storage_2d<r32float, write>;

// The farthest of the depth's pixels a first-level texel covers: 2 × 2, and
// the next row or column too where the depth's size is odd at the edge.
@compute @workgroup_size(8, 8)
fn cs_first(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(destination);
    if any(id.xy >= size) {
        return;
    }
    let source = textureDimensions(source_depth);
    let last = vec2<i32>(source) - 1;
    let start = vec2<i32>(id.xy) * 2;
    // The last texel of a row or column takes the odd pixel left over.
    let extra = vec2<i32>(select(vec2<u32>(0u), vec2<u32>(1u),
        (id.xy + 1u == size) & (source % 2u == vec2<u32>(1u))));
    var far = 0.0;
    for (var y = 0; y <= 1 + extra.y; y++) {
        for (var x = 0; x <= 1 + extra.x; x++) {
            let at = min(start + vec2<i32>(x, y), last);
            far = max(far, textureLoad(source_depth, at, 0));
        }
    }
    textureStore(destination, vec2<i32>(id.xy), vec4<f32>(far, 0.0, 0.0, 0.0));
}

// The next level: the farthest of the 2 × 2 (or 3 at an odd edge) below.
@compute @workgroup_size(8, 8)
fn cs_down(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(destination);
    if any(id.xy >= size) {
        return;
    }
    let source = textureDimensions(source_level);
    let last = vec2<i32>(source) - 1;
    let start = vec2<i32>(id.xy) * 2;
    let extra = vec2<i32>(select(vec2<u32>(0u), vec2<u32>(1u),
        (id.xy + 1u == size) & (source % 2u == vec2<u32>(1u))));
    var far = 0.0;
    for (var y = 0; y <= 1 + extra.y; y++) {
        for (var x = 0; x <= 1 + extra.x; x++) {
            let at = min(start + vec2<i32>(x, y), last);
            far = max(far, textureLoad(source_level, at, 0).r);
        }
    }
    textureStore(destination, vec2<i32>(id.xy), vec4<f32>(far, 0.0, 0.0, 0.0));
}

// ---- The test and the kept instances ------------------------------------
//
// The tested instances are numbered in the order their batches are drawn
// (each batch's from its `out_first`), and kept ones are compacted in that
// order, so each batch's kept instances stay together and in their order:
// `cs_test` tests 64 at a time and ranks each kept one among its 64,
// `cs_offsets` adds up the 64s and writes each batch's draw, and `cs_copy`
// copies each kept instance to its place.

// Must match `CullUniform` in renderer/occlusion_cull.rs.
struct Cull {
    // The prepass's view and projection, its jitter too.
    view_proj: mat4x4<f32>,
    // xy: the prepass's size in pixels, z: the pyramid's levels, w: unused.
    size: vec4<f32>,
    // x: the tested instances, y: their 64s, z: the batches, w: unused.
    counts: vec4<u32>,
}

// One batch tested: its instances in the frame's instance buffer, where
// its kept ones go, and its slot in `args`. Must match `CullBatch`.
struct CullBatch {
    first: u32,
    count: u32,
    out_first: u32,
    args: u32,
}

// The words of an `InstanceRaw` (renderer/types.rs).
const INSTANCE_WORDS: u32 = 43u;
const GROUP: u32 = 64u;
// In a tested instance's `ranks` word: kept, and its batch's index above
// its rank among its 64.
const KEPT: u32 = 0x80000000u;
const BATCH_SHIFT: u32 = 6u;

@group(0) @binding(0) var<uniform> cull: Cull;
@group(0) @binding(1) var pyramid: texture_2d<f32>;
@group(0) @binding(2) var<storage, read> batches: array<CullBatch>;
// A tested instance's bounding sphere, and which batch it's in.
@group(0) @binding(3) var<storage, read> spheres: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> owners: array<u32>;
@group(0) @binding(5) var<storage, read> instances: array<u32>;
@group(0) @binding(6) var<storage, read_write> kept: array<u32>;
// Five words a batch: `DrawIndexedIndirectArgs`; the second is written here.
@group(0) @binding(7) var<storage, read_write> args: array<u32>;
// A word a tested instance (`KEPT`, its batch, its rank), then a word a 64:
// how many it kept, then where its kept start.
@group(0) @binding(8) var<storage, read_write> ranks: array<u32>;

var<workgroup> scan: array<u32, 256>;
var<workgroup> total_kept: u32;

// Whether the sphere (`ball`: centre and radius) is wholly behind the
// depth the pyramid holds: every corner of the box round it in front of
// the camera, and its nearest depth farther than the farthest the pyramid
// has over the pixels the box covers (one more each way).
fn hidden(ball: vec4<f32>) -> bool {
    var low = vec2<f32>(1e9);
    var high = vec2<f32>(-1e9);
    var nearest = 2.0;
    for (var i = 0u; i < 8u; i++) {
        let corner = ball.xyz + ball.w * vec3<f32>(
            select(-1.0, 1.0, (i & 1u) != 0u),
            select(-1.0, 1.0, (i & 2u) != 0u),
            select(-1.0, 1.0, (i & 4u) != 0u));
        let clip = cull.view_proj * vec4<f32>(corner, 1.0);
        // Reaching behind the camera: it can't be tested.
        if clip.w <= 1e-4 {
            return false;
        }
        let ndc = clip.xyz / clip.w;
        low = min(low, ndc.xy);
        high = max(high, ndc.xy);
        nearest = min(nearest, ndc.z);
    }
    if nearest <= 0.0 {
        return false;
    }
    let size = cull.size.xy;
    // Pixels, y down, one more each way.
    let first = max(vec2<f32>(low.x * 0.5 + 0.5, 0.5 - high.y * 0.5) * size - 1.0, vec2<f32>(0.0));
    let last = min(vec2<f32>(high.x * 0.5 + 0.5, 0.5 - low.y * 0.5) * size + 1.0, size - 1.0);
    if any(last < first) {
        // Off the screen: nothing of it is drawn either way.
        return false;
    }
    let span = max(last.x - first.x, last.y - first.y);
    // The level whose texels (2^(level + 1) pixels each) are at least
    // as wide as the box, so it covers at most 2 × 2 of them.
    let levels = u32(cull.size.z);
    let level = u32(max(ceil(log2(max(span, 1.0))) - 1.0, 0.0));
    if level >= levels {
        return false;
    }
    let texel = f32(1u << (level + 1u));
    let lowest = vec2<i32>(floor(first / texel));
    let highest = vec2<i32>(floor(last / texel));
    let edge = vec2<i32>(textureDimensions(pyramid, level)) - 1;
    var far = 0.0;
    for (var y = lowest.y; y <= highest.y; y++) {
        for (var x = lowest.x; x <= highest.x; x++) {
            far = max(far, textureLoad(pyramid, min(vec2<i32>(x, y), edge), level).r);
        }
    }
    return nearest > far;
}

// Tests 64 instances and ranks the kept among them.
@compute @workgroup_size(64)
fn cs_test(@builtin(global_invocation_id) id: vec3<u32>,
           @builtin(local_invocation_index) lane: u32,
           @builtin(workgroup_id) group: vec3<u32>) {
    let total = cull.counts.x;
    let at = id.x;
    var keep = 0u;
    if at < total && !hidden(spheres[at]) {
        keep = 1u;
    }
    // A scan of the 64: each lane's count of the kept up to it.
    scan[lane] = keep;
    workgroupBarrier();
    for (var step = 1u; step < GROUP; step <<= 1u) {
        var sum = scan[lane];
        if lane >= step {
            sum += scan[lane - step];
        }
        workgroupBarrier();
        scan[lane] = sum;
        workgroupBarrier();
    }
    if at < total {
        ranks[at] = select(0u, KEPT, keep == 1u) | (owners[at] << BATCH_SHIFT)
            | (scan[lane] - keep);
    }
    if lane == GROUP - 1u {
        ranks[total + group.x] = scan[lane];
    }
}

// Where the kept of instance `at` (or the end, for `at` = the tested
// instances) start among all the kept, once `cs_offsets` made the 64s'
// counts their starts.
fn place(at: u32) -> u32 {
    let total = cull.counts.x;
    if at >= total {
        return total_kept;
    }
    return ranks[total + at / GROUP] + (ranks[at] & ((1u << BATCH_SHIFT) - 1u));
}

// One workgroup: turns the 64s' counts into where each 64's kept start,
// then writes each batch's draw: as many instances as it kept.
@compute @workgroup_size(256)
fn cs_offsets(@builtin(local_invocation_index) lane: u32) {
    let total = cull.counts.x;
    let groups = cull.counts.y;
    // Each lane adds up a run of the 64s, the runs in order.
    let run = (groups + 255u) / 256u;
    let start = min(lane * run, groups);
    let end = min(start + run, groups);
    var sum = 0u;
    for (var i = start; i < end; i++) {
        sum += ranks[total + i];
    }
    scan[lane] = sum;
    workgroupBarrier();
    for (var step = 1u; step < 256u; step <<= 1u) {
        var added = scan[lane];
        if lane >= step {
            added += scan[lane - step];
        }
        workgroupBarrier();
        scan[lane] = added;
        workgroupBarrier();
    }
    if lane == 255u {
        total_kept = scan[lane];
    }
    // The run's 64s' starts, from the kept before the run.
    var before = scan[lane] - sum;
    for (var i = start; i < end; i++) {
        let count = ranks[total + i];
        ranks[total + i] = before;
        before += count;
    }
    storageBarrier();
    workgroupBarrier();
    for (var b = lane; b < cull.counts.z; b += 256u) {
        let batch = batches[b];
        args[batch.args * 5u + 1u] = place(batch.out_first + batch.count) - place(batch.out_first);
    }
}

// Copies each kept instance to its batch's place, after the kept before it
// in its batch: a word a lane, so neighbouring lanes read and write
// neighbouring words. Dispatched as rows of `COPY_ROW` workgroups.
const COPY_GROUP: u32 = 256u;
const COPY_ROW: u32 = 4096u;

@compute @workgroup_size(256)
fn cs_copy(@builtin(workgroup_id) group: vec3<u32>,
           @builtin(local_invocation_index) lane: u32) {
    let word = (group.y * COPY_ROW + group.x) * COPY_GROUP + lane;
    let at = word / INSTANCE_WORDS;
    if at >= cull.counts.x {
        return;
    }
    let rank = ranks[at];
    if (rank & KEPT) == 0u {
        return;
    }
    let batch = batches[(rank & ~KEPT) >> BATCH_SHIFT];
    let within = word - at * INSTANCE_WORDS;
    let to = (batch.out_first + place(at) - place(batch.out_first)) * INSTANCE_WORDS;
    kept[to + within] = instances[(batch.first + at - batch.out_first) * INSTANCE_WORDS + within];
}
