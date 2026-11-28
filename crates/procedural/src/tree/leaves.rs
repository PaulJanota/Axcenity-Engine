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
    seed: u64,
}

/// Cells along each side of the grid the crown's shade is counted on.
const GRID: usize = 12;

/// How much the clusters in one cell dim the light passing through it, per
/// cluster as many as an average occupied cell has.
const DIMMING: f32 = 0.3;

/// The most a kept cluster grows when a simpler copy keeps a share of them.
const MAX_GROWTH: f32 = 1.8;

/// Grows the clusters on `anchors`, shade-thinned, in their spread order.
pub fn clusters(recipe: &TreeRecipe, anchors: &[LeafAnchor]) -> (Vec<Cluster>, Crown) {
    let leaves = &recipe.leaves;
    let crown = crown(anchors, leaves.size.max(0.0));
    if !leaves.enabled || anchors.is_empty() {
        return (Vec::new(), crown);
    }
    let up = Vec3::Y;
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
            let out = crown.normal(position);
            let face = leaves.face_out.clamp(0.0, 1.0);
            let axis = (anchor.direction * (1.0 - face) + out * face + up * 0.2)
                .normalize_or(anchor.direction);
            clusters.push(Cluster {
                position,
                axis,
                light: 1.0,
                seed: rng.next(),
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

/// The leaves of the first `keep` share of `clusters` (at most `budget`
/// triangles), the kept ones grown to cover about as much as all would.
pub fn mesh(
    recipe: &TreeRecipe,
    clusters: &[Cluster],
    crown: &Crown,
    keep: f32,
    detailed: bool,
    budget: usize,
) -> TreeMesh {
    let leaves = &recipe.leaves;
    let mut mesh = TreeMesh::default();
    let per_cluster = leaves.per_cluster.max(1) as usize * leaf_triangles(detailed);
    let kept = ((clusters.len() as f32 * keep.clamp(0.0, 1.0)).ceil() as usize)
        .min(budget / per_cluster)
        .min(clusters.len());
    if kept == 0 {
        return mesh;
    }
    let growth = (clusters.len() as f32 / kept as f32).sqrt().min(MAX_GROWTH);
    for cluster in &clusters[..kept] {
        let mut rng = Rng::new(cluster.seed, 2);
        let out = crown.normal(cluster.position);
        let side = cluster.axis.any_orthonormal_vector();
        let count = leaves.per_cluster.max(1);
        let turn = rng.range(0.0, std::f32::consts::TAU);
        for i in 0..count {
            let around =
                turn + std::f32::consts::TAU * (i as f32 + rng.range(-0.2, 0.2)) / count as f32;
            let lean = (leaves.spread + rng.range(-10.0, 10.0)).to_radians();
            let tilt = glam::Quat::from_axis_angle(cluster.axis, around) * side;
            let along = (glam::Quat::from_axis_angle(tilt, lean) * cluster.axis).normalize();
            let jitter = leaves.size_jitter.clamp(0.0, 0.9);
            let length = leaves.size.max(0.01) * rng.range(1.0 - jitter, 1.0 + jitter) * growth;
            leaf(
                &mut mesh,
                Leaf {
                    base: cluster.position,
                    along,
                    facing: (Vec3::Y * 0.6 + out * 0.4).normalize_or(Vec3::Y),
                    length,
                    width: length * leaves.width.max(0.0),
                    droop: leaves.droop,
                    fold: leaves.fold,
                },
                detailed,
                [out.x, out.y, out.z, cluster.light],
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
    length: f32,
    width: f32,
    droop: f32,
    fold: f32,
}

/// One leaf: a blade from its base to its tip, its halves folded up along
/// the middle and its tip hanging, counter-clockwise seen from its face.
fn leaf(mesh: &mut TreeMesh, leaf: Leaf, detailed: bool, crown: [f32; 4]) {
    let across = leaf
        .along
        .cross(leaf.facing)
        .normalize_or(leaf.along.any_orthonormal_vector());
    let face = across.cross(leaf.along);
    // (along, across) in shares of the length and width.
    let shape: &[[f32; 2]] = if detailed {
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
    let triangles: &[[u32; 3]] = if detailed {
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
        let normal = normal.normalize_or(face);
        let tangent = across.reject_from(normal).normalize_or(across);
        mesh.positions.push(corner.to_array());
        mesh.normals.push(normal.to_array());
        mesh.uvs.push([0.5 + s, 1.0 - t]);
        mesh.tangents.push([tangent.x, tangent.y, tangent.z, 1.0]);
        mesh.crown.push(crown);
    }
}
