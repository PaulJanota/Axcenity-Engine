//! Leaves (foliage plan T3): clusters of a few leaves each along the outer
//! parts of the finest limbs, left out where the crown shades itself (a coarse
//! grid over the crown counts what lies between each cluster and the sky),
//! each leaf a little folded blade of real triangles (no cut-out texture,
//! so it costs no alpha test and the ray tracer takes it as solid).
//!
//! Every corner carries its crown data ([`TreeMesh::crown`]): which way is
//! out of the crown there and how much sky reaches it, for the leaf shader
//! to light the crown as one (ADR-0053). Clusters are kept in an order that
//! spreads any first few of them evenly over the crown, so a simpler copy
//! (and a season's leaf drop) keeps the first clusters and grows them.

use glam::Vec3;

use super::LeafAnchor;
use super::mesher::TreeMesh;
use super::recipe::TreeRecipe;
use crate::rng::Rng;

/// The crown's box: around every place leaves grow, grown by a leaf.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crown {
    pub center: Vec3,
    pub half_size: Vec3,
}

impl Crown {
    /// Out of the crown at `point`: the normal of the ellipsoid filling its
    /// box, through the point.
    pub fn normal(&self, point: Vec3) -> Vec3 {
        let half = self.half_size.max(Vec3::splat(1e-3));
        ((point - self.center) / (half * half)).normalize_or(Vec3::Y)
    }
}

/// One cluster: where its stalk starts, which way it points, how much sky
/// reaches it (0 to 1) and its own random choices.
#[derive(Clone, Copy, Debug)]
pub struct Cluster {
    pub position: Vec3,
    pub axis: Vec3,
    pub light: f32,
    /// Its size as a share of the recipe's (smaller toward the crown's top
    /// when [`Leaves::top_scale`](super::Leaves) says so).
    pub scale: f32,
    /// How far its limb carries it in the wind ([`LeafAnchor::sway`]).
    pub sway: f32,
    seed: u64,
}

/// Cells along each side of the grid the crown's shade is counted on.
const GRID: usize = 12;

/// How much the clusters in one cell dim the light passing through it, per
/// cluster as many as an average occupied cell has.
const DIMMING: f32 = 0.3;

/// The most a kept cluster grows when a simpler copy keeps a share of them.
const MAX_GROWTH: f32 = 1.8;

/// The most the stand-in's cards grow: it is only seen in far shadows and
/// reflections, where a few big cards read as the crown.
const PROXY_GROWTH: f32 = 3.0;

/// How much bigger a card is than the leaf it shows: a leaf fills about
/// 70% of its cell in a picture of leaves.
const CELL_FILL: f32 = 1.3;

/// Grows the clusters on `anchors`, shade-thinned, in their spread order.
pub fn clusters(recipe: &TreeRecipe, anchors: &[LeafAnchor]) -> (Vec<Cluster>, Crown) {
    let leaves = &recipe.leaves;
    let crown = crown(anchors, leaves.size.max(0.0));
    if !leaves.enabled || anchors.is_empty() {
        return (Vec::new(), crown);
    }
    let up = Vec3::Y;
    let pockets = leaves.pockets.clamp(0.0, 1.0);
    let pocket_size = leaves.pocket_size.max(0.1);
    let low = crown.center.y - crown.half_size.y;
    let height = (crown.half_size.y * 2.0).max(1e-3);
    let mut clusters = Vec::new();
    for (index, anchor) in anchors.iter().enumerate() {
        let mut rng = Rng::new(recipe.seed ^ 0x1EAF_0000_0000_0000, index as u64);
        let count = (leaves.clusters.max(0.0) + rng.unit()).floor() as u32;
        for _ in 0..count {
            let side = Vec3::new(
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
            );
            let position = anchor.position + side * anchor.radius * 2.0;
            let seed = rng.next();
            // Air pockets inside the crown (its outer shell kept whole, so
            // its outline stays full): where a smooth noise over the crown
            // is low.
            let depth =
                ((position - crown.center) / crown.half_size.max(Vec3::splat(1e-3))).length();
            if pockets > 0.0
                && depth < 0.8
                && value_noise(position / pocket_size, recipe.seed) < pockets
            {
                continue;
            }
            let out = crown.normal(position);
            let face = leaves.face_out.clamp(0.0, 1.0);
            let axis = (anchor.direction * (1.0 - face) + out * face + up * 0.2)
                .normalize_or(anchor.direction);
            // Toward the crown's top: smaller and turned up.
            let top = smoothstep(0.65, 1.0, (position.y - low) / height);
            let axis = axis
                .lerp(up, leaves.top_lift.clamp(0.0, 1.0) * top)
                .normalize_or(axis);
            clusters.push(Cluster {
                position,
                axis,
                light: 1.0,
                scale: 1.0 + (leaves.top_scale.max(0.05) - 1.0) * top,
                sway: anchor.sway,
                seed,
            });
        }
    }
    // Fewer leaves where the crown shades itself.
    shade(&mut clusters, &crown);
    let thinning = leaves.shade_thinning.clamp(0.0, 1.0);
    clusters.retain(|cluster| {
        let keep = 1.0 - thinning * (1.0 - cluster.light);
        Rng::new(cluster.seed, 1).unit() < keep
    });
    shade(&mut clusters, &crown);
    spread(&mut clusters, &crown);
    (clusters, crown)
}

/// A smooth noise of `p` (0 to 1, changing over about one unit), the same
/// for the same `seed`: values at whole points from a hash, blended
/// smoothly between them.
fn value_noise(p: Vec3, seed: u64) -> f32 {
    let cell = p.floor();
    let f = p - cell;
    let f = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let at = |dx: f32, dy: f32, dz: f32| {
        let c = cell + Vec3::new(dx, dy, dz);
        let key = (c.x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (c.y as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ (c.z as i64 as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
        Rng::new(seed ^ 0x0090_C4E7, key).unit()
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(at(0.0, 0.0, 0.0), at(1.0, 0.0, 0.0), f.x);
    let x10 = lerp(at(0.0, 1.0, 0.0), at(1.0, 1.0, 0.0), f.x);
    let x01 = lerp(at(0.0, 0.0, 1.0), at(1.0, 0.0, 1.0), f.x);
    let x11 = lerp(at(0.0, 1.0, 1.0), at(1.0, 1.0, 1.0), f.x);
    lerp(lerp(x00, x10, f.y), lerp(x01, x11, f.y), f.z)
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn crown(anchors: &[LeafAnchor], grow: f32) -> Crown {
    let (low, high) = anchors.iter().fold(
        (Vec3::INFINITY, Vec3::NEG_INFINITY),
        |(low, high), anchor| (low.min(anchor.position), high.max(anchor.position)),
    );
    if !low.cmple(high).all() {
        return Crown {
            center: Vec3::ZERO,
            half_size: Vec3::ONE,
        };
    }
    Crown {
        center: (low + high) * 0.5,
        half_size: (high - low) * 0.5 + Vec3::splat(grow),
    }
}

/// How much sky reaches each cluster: straight up and four ways leaning
/// 45° off it, each dimmed by the clusters it passes on the grid.
fn shade(clusters: &mut [Cluster], crown: &Crown) {
    let low = crown.center - crown.half_size;
    let cell = (crown.half_size * 2.0 / GRID as f32).max(Vec3::splat(1e-3));
    let cell_of = |p: Vec3| -> Option<usize> {
        let c = ((p - low) / cell).floor();
        if c.cmplt(Vec3::ZERO).any() || c.cmpge(Vec3::splat(GRID as f32)).any() {
            return None;
        }
        Some(c.x as usize + GRID * (c.y as usize + GRID * c.z as usize))
    };
    let mut count = vec![0u32; GRID * GRID * GRID];
    for cluster in clusters.iter() {
        if let Some(i) = cell_of(cluster.position) {
            count[i] += 1;
        }
    }
    let occupied = count.iter().filter(|&&c| c > 0).count().max(1);
    let average = clusters.len().max(1) as f32 / occupied as f32;
    let per = DIMMING / average;
    let directions = [
        Vec3::Y,
        Vec3::new(1.0, 1.0, 0.0).normalize(),
        Vec3::new(-1.0, 1.0, 0.0).normalize(),
        Vec3::new(0.0, 1.0, 1.0).normalize(),
        Vec3::new(0.0, 1.0, -1.0).normalize(),
    ];
    let step = cell.min_element();
    for cluster in clusters.iter_mut() {
        let own = cell_of(cluster.position);
        let mut light = 0.0;
        for direction in directions {
            let mut passed = 0u32;
            let mut last = own;
            let mut at = cluster.position;
            loop {
                at += direction * step;
                let Some(i) = cell_of(at) else { break };
                if Some(i) != last {
                    passed += count[i];
                    last = Some(i);
                }
            }
            light += (-(passed as f32) * per).exp();
        }
        cluster.light = light / directions.len() as f32;
    }
}

/// Puts `clusters` in an order whose first few, any number of them, are
/// spread evenly over the crown: along a Morton curve through the crown's
/// box, then by the bits of each one's place on it reversed.
fn spread(clusters: &mut Vec<Cluster>, crown: &Crown) {
    let low = crown.center - crown.half_size;
    let size = (crown.half_size * 2.0).max(Vec3::splat(1e-3));
    let morton = |p: Vec3| -> u64 {
        let q = ((p - low) / size).clamp(Vec3::ZERO, Vec3::ONE) * 1023.0;
        let spread_bits = |v: u32| -> u64 {
            let mut out = 0u64;
            for bit in 0..10 {
                out |= u64::from((v >> bit) & 1) << (3 * bit);
            }
            out
        };
        spread_bits(q.x as u32) | spread_bits(q.y as u32) << 1 | spread_bits(q.z as u32) << 2
    };
    let mut keyed: Vec<(u64, usize)> = clusters
        .iter()
        .enumerate()
        .map(|(i, c)| (morton(c.position), i))
        .collect();
    keyed.sort();
    let bits = (clusters.len().max(2) - 1).ilog2() + 1;
    let mut ranked: Vec<(u32, usize)> = keyed
        .iter()
        .enumerate()
        .map(|(place, &(_, i))| ((place as u32).reverse_bits() >> (32 - bits), i))
        .collect();
    ranked.sort();
    *clusters = ranked.iter().map(|&(_, i)| clusters[i]).collect();
}

/// Triangles in each leaf: a detailed leaf has six corners, a simple one
/// four.
pub fn leaf_triangles(detailed: bool) -> usize {
    if detailed { 4 } else { 2 }
}

/// What one copy of the tree keeps of its leaves.
#[derive(Clone, Copy, Debug)]
pub struct CopyLeaves {
    /// The share of the clusters it keeps.
    pub keep: f32,
    /// Clusters deeper in the crown than this (0 to 1) are left out
    /// ([`LodDetail::shell`](super::LodDetail)).
    pub shell: f32,
    /// Leaves of four triangles (else two).
    pub detailed: bool,
    /// The most triangles its leaves may have.
    pub budget: usize,
    /// Leaves in each cluster, in place of the recipe's (the stand-in's
    /// few big crossed cards).
    pub cards: Option<u32>,
    /// The most a kept leaf grows to cover for those left out.
    pub max_growth: f32,
}

impl CopyLeaves {
    /// A simpler copy keeping `keep` of the clusters, those inside `shell`
    /// left out.
    pub fn copy(keep: f32, shell: f32, detailed: bool, budget: usize) -> Self {
        Self {
            keep,
            shell,
            detailed,
            budget,
            cards: None,
            max_growth: MAX_GROWTH,
        }
    }

    /// The stand-in for far shadows and ray tracing: a crossed pair of big
    /// cards for each of `keep` of the clusters on the crown's shell.
    pub fn proxy(keep: f32, shell: f32, budget: usize) -> Self {
        Self {
            cards: Some(2),
            max_growth: PROXY_GROWTH,
            ..Self::copy(keep, shell, false, budget)
        }
    }
}

/// How far out in its crown each cluster is, by the crown's own shape (a
/// cone, a dome or a column alike): out from its middle as a share of how
/// far the crown reaches at that height, or up as a share of the way to
/// the crown's top above it.
pub struct Shell {
    center: Vec3,
    low: Vec3,
    cell: Vec3,
    /// The farthest any cluster reaches from the middle, by height.
    reach: [f32; SLICES],
    /// The highest cluster in each column (`COLUMNS` × `COLUMNS` over the
    /// crown's box).
    top: [f32; COLUMNS * COLUMNS],
    half_height: f32,
}

/// Slices of the crown by height, and columns along each side.
const SLICES: usize = 12;
const COLUMNS: usize = 8;

impl Shell {
    pub fn of(clusters: &[Cluster], crown: &Crown) -> Self {
        let low = crown.center - crown.half_size;
        let size = (crown.half_size * 2.0).max(Vec3::splat(1e-3));
        let cell = Vec3::new(
            size.x / COLUMNS as f32,
            size.y / SLICES as f32,
            size.z / COLUMNS as f32,
        );
        let mut shell = Self {
            center: crown.center,
            low,
            cell,
            reach: [0.0; SLICES],
            top: [f32::NEG_INFINITY; COLUMNS * COLUMNS],
            half_height: crown.half_size.y.max(1e-3),
        };
        let mut reach = [0.0f32; SLICES];
        for cluster in clusters {
            let p = cluster.position;
            let slice = shell.slice(p);
            reach[slice] = reach[slice].max(shell.out(p));
            let column = shell.column(p);
            shell.top[column] = shell.top[column].max(p.y);
        }
        // Each slice reaches as far as its neighbours do, so a thin slice
        // doesn't make its few clusters all the outside.
        for (i, each) in shell.reach.iter_mut().enumerate() {
            let near = i.saturating_sub(1)..=(i + 1).min(SLICES - 1);
            *each = reach[near].iter().copied().fold(1e-3, f32::max);
        }
        shell
    }

    fn out(&self, p: Vec3) -> f32 {
        Vec3::new(p.x - self.center.x, 0.0, p.z - self.center.z).length()
    }

    fn slice(&self, p: Vec3) -> usize {
        (((p.y - self.low.y) / self.cell.y) as usize).min(SLICES - 1)
    }

    fn column(&self, p: Vec3) -> usize {
        let x = (((p.x - self.low.x) / self.cell.x).max(0.0) as usize).min(COLUMNS - 1);
        let z = (((p.z - self.low.z) / self.cell.z).max(0.0) as usize).min(COLUMNS - 1);
        x + COLUMNS * z
    }

    /// 0 deep inside the crown to 1 on its outside.
    pub fn depth(&self, p: Vec3) -> f32 {
        let across = self.out(p) / self.reach[self.slice(p)];
        let up = 1.0 - (self.top[self.column(p)] - p.y) / self.half_height;
        across.max(up).clamp(0.0, 1.0)
    }

    /// Whether `cluster` lies deeper in the crown than `limit`, hidden by
    /// the clusters round it.
    pub fn inside(&self, cluster: &Cluster, limit: f32) -> bool {
        limit > 0.0 && self.depth(cluster.position) < limit
    }
}

/// The leaves of the first `copy.keep` share of `clusters` (at most
/// `copy.budget` triangles), those inside its shell left out, the kept
/// ones grown to cover about as much as all would.
pub fn mesh(
    recipe: &TreeRecipe,
    clusters: &[Cluster],
    crown: &Crown,
    copy: CopyLeaves,
) -> TreeMesh {
    let leaves = &recipe.leaves;
    let mut mesh = TreeMesh::default();
    let count = copy.cards.unwrap_or(leaves.per_cluster).max(1);
    let per_cluster = count as usize * leaf_triangles(copy.detailed);
    // Still in their spread order, so any first few cover the shell.
    let shell = Shell::of(clusters, crown);
    let outer: Vec<&Cluster> = clusters
        .iter()
        .filter(|cluster| !shell.inside(cluster, copy.shell))
        .collect();
    let kept = ((clusters.len() as f32 * copy.keep.clamp(0.0, 1.0)).ceil() as usize)
        .min(copy.budget / per_cluster)
        .min(outer.len());
    if kept == 0 {
        return mesh;
    }
    // Grown to cover about as much as all the leaves would.
    let all = clusters.len() * leaves.per_cluster.max(1) as usize;
    let growth = (all as f32 / (kept * count as usize) as f32)
        .sqrt()
        .min(copy.max_growth.max(1.0));
    let crossed = leaves.crossed.clamp(0.0, 1.0);
    let detailed = copy.detailed;
    for &cluster in &outer[..kept] {
        let mut rng = Rng::new(cluster.seed, 2);
        let out = crown.normal(cluster.position);
        let side = cluster.axis.any_orthonormal_vector();
        let turn = rng.range(0.0, std::f32::consts::TAU);
        let size = leaves.size.max(0.01) * growth * cluster.scale;
        // The middle of the cluster's ball of leaves, which its leaves'
        // light bends out from.
        let middle = cluster.position + cluster.axis * size * 0.5;
        // Its own shade of the season's colour, the same for all its leaves.
        let tint = Rng::new(cluster.seed, 3).unit();
        // The leaf before: its base, its way along and the way across it.
        let mut before: Option<(Vec3, Vec3, Vec3)> = None;
        for i in 0..count {
            let around =
                turn + std::f32::consts::TAU * (i as f32 + rng.range(-0.2, 0.2)) / count as f32;
            let lean = (leaves.spread + rng.range(-10.0, 10.0)).to_radians();
            let tilt = glam::Quat::from_axis_angle(cluster.axis, around) * side;
            let mut along = (glam::Quat::from_axis_angle(tilt, lean) * cluster.axis).normalize();
            let jitter = leaves.size_jitter.clamp(0.0, 0.9);
            let mut length = size * rng.range(1.0 - jitter, 1.0 + jitter);
            let mut width = length * leaves.width.max(0.0);
            // Up a short stalk (the first at its foot), so the cluster is a
            // ball rather than a flat rosette.
            let up_stalk = if i == 0 { 0.0 } else { rng.range(0.1, 0.6) };
            let mut base = cluster.position + cluster.axis * size * up_stalk;
            // Facing out of the ball, turned a little at random, the crown
            // and the sky having a say.
            let roll = rng.range(-0.6, 0.6);
            let away = (base + along * length * 0.5 - middle).reject_from(along);
            let mut facing =
                (away.normalize_or(out) + out * 0.4 + Vec3::Y * 0.3).normalize_or(Vec3::Y);
            facing = glam::Quat::from_axis_angle(along, roll) * facing;
            // Or crosswise to the leaf before, on the same stalk.
            if rng.unit() < crossed
                && let Some((at, way, across)) = before.take()
            {
                base = at;
                along = way;
                facing = across;
            } else {
                before = Some((base, along, along.cross(facing).normalize_or(side)));
            }
            // Its front out of the crown, so the crown seen from outside is
            // the fronts of its leaves, lit by their bent normals.
            if facing.reject_from(along).dot(out) < 0.0 {
                facing = -facing;
            }
            // A leaf cut out of a picture fills about 70% of its cell, so
            // the card is a little bigger, and as wide as its cell.
            let cell = leaves.textured().then(|| {
                let cell = leaves.cells[rng.below(leaves.cells.len())];
                let (across, long) = if leaves.stem_right {
                    (cell[3] - cell[1], cell[2] - cell[0])
                } else {
                    (cell[2] - cell[0], cell[3] - cell[1])
                };
                length *= CELL_FILL;
                width = length * (across / long.max(1e-4));
                (cell, leaves.stem_right)
            });
            leaf(
                &mut mesh,
                Leaf {
                    base,
                    along,
                    facing,
                    middle,
                    out,
                    length,
                    width,
                    droop: leaves.droop,
                    fold: leaves.fold,
                    cell,
                },
                detailed,
                [out.x, out.y, out.z, cluster.light],
                [cluster.light, cluster.sway, tint],
            );
        }
    }
    mesh
}

struct Leaf {
    base: Vec3,
    along: Vec3,
    /// Which way its face would rather turn.
    facing: Vec3,
    /// Its cluster's middle and the way out of the crown there: its light
    /// bends out from both, so a crown is shaded as one volume.
    middle: Vec3,
    out: Vec3,
    length: f32,
    width: f32,
    droop: f32,
    fold: f32,
    /// The part of the leaf picture it shows (`[u0, v0, u1, v1]`), and
    /// whether the stem is at its right; `None` for a plain blade.
    cell: Option<([f32; 4], bool)>,
}

/// One leaf: a blade from its base to its tip, its halves folded up along
/// the middle and its tip hanging, counter-clockwise seen from its face.
/// `paint` is the cluster's light, sway and tint, for each corner's packed
/// colour (`paint.rs`).
fn leaf(mesh: &mut TreeMesh, leaf: Leaf, detailed: bool, crown: [f32; 4], paint: [f32; 3]) {
    let across = leaf
        .along
        .cross(leaf.facing)
        .normalize_or(leaf.along.any_orthonormal_vector());
    let face = across.cross(leaf.along);
    // (along, across) in shares of the length and width: a card for a
    // leaf cut out of a picture, else a leaf's outline.
    // A card is bent along its length (a row of corners across its
    // middle, so it hangs in a curve).
    let shape: &[[f32; 2]] = if leaf.cell.is_some() {
        if detailed {
            &[
                [0.0, 0.5],
                [0.5, 0.5],
                [1.0, 0.5],
                [1.0, -0.5],
                [0.5, -0.5],
                [0.0, -0.5],
            ]
        } else {
            &[[0.0, 0.5], [1.0, 0.5], [1.0, -0.5], [0.0, -0.5]]
        }
    } else if detailed {
        &[
            [0.0, 0.0],
            [0.3, 0.45],
            [0.7, 0.35],
            [1.0, 0.0],
            [0.7, -0.35],
            [0.3, -0.45],
        ]
    } else {
        &[[0.0, 0.0], [0.5, 0.45], [1.0, 0.0], [0.5, -0.45]]
    };
    let triangles: &[[u32; 3]] = if leaf.cell.is_some() && detailed {
        &[[0, 1, 5], [1, 4, 5], [1, 2, 4], [2, 3, 4]]
    } else if shape.len() == 6 {
        &[[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5]]
    } else {
        &[[0, 1, 2], [0, 2, 3]]
    };
    let first = mesh.positions.len() as u32;
    let corners: Vec<Vec3> = shape
        .iter()
        .map(|&[t, s]| {
            leaf.base
                + leaf.along * leaf.length * t
                + across * leaf.width * s
                + face * leaf.fold * leaf.width * s.abs()
                + Vec3::NEG_Y * leaf.droop * leaf.length * t * t
        })
        .collect();
    let mut normals = vec![Vec3::ZERO; corners.len()];
    for triangle in triangles {
        let [a, b, c] = triangle.map(|i| corners[i as usize]);
        let n = (b - a).cross(c - a);
        for &i in triangle {
            normals[i as usize] += n;
        }
        mesh.indices.extend(triangle.iter().map(|&i| first + i));
    }
    for (k, (&corner, normal)) in corners.iter().zip(&normals).enumerate() {
        let [t, s] = shape[k];
        let flat = normal.normalize_or(face);
        // Bent out from the cluster's middle and out of the crown, but
        // never past the face's own side (it is lit from the side it faces).
        let bent = flat + (corner - leaf.middle).normalize_or(leaf.out) * 0.6 + leaf.out * 0.4;
        let bent = bent.normalize_or(flat);
        let normal = if bent.dot(flat) < 0.3 {
            (bent + flat).normalize_or(flat)
        } else {
            bent
        };
        // The picture upright on the card, its stem at the leaf's base.
        let (uv, u_grows) = match leaf.cell {
            Some(([u0, v0, u1, v1], false)) => {
                ([u0 + (s + 0.5) * (u1 - u0), v1 - t * (v1 - v0)], across)
            }
            Some(([u0, v0, u1, v1], true)) => (
                [u1 - t * (u1 - u0), v1 - (s + 0.5) * (v1 - v0)],
                -leaf.along,
            ),
            None => ([0.5 + s, 1.0 - t], across),
        };
        let tangent = u_grows.reject_from(normal).normalize_or(across);
        mesh.positions.push(corner.to_array());
        mesh.normals.push(normal.to_array());
        mesh.uvs.push(uv);
        mesh.tangents.push([tangent.x, tangent.y, tangent.z, 1.0]);
        mesh.crown.push(crown);
        let [light, sway, tint] = paint;
        mesh.colors.push([
            // Darker deep in the crown and at the leaf's foot.
            (0.35 + 0.65 * light) * (0.8 + 0.2 * t),
            sway,
            // Tips and edges flutter most.
            (0.2 + 0.6 * t + 0.4 * s.abs()).min(1.0),
            tint,
        ]);
    }
}
