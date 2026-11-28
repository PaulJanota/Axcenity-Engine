//! Limbs as tubes: rings of corners along each center line, turned from
//! ring to ring by the smallest rotation (so a limb's texture doesn't twist
//! as it bends), closed at the tip. The trunk's base is its own finer mesh
//! with root buttresses and bark relief (foliage plan T2), and each simpler
//! copy is meshed from the same limbs with fewer rings and corners, so
//! nothing moves at the switch.

use glam::{Quat, Vec3};

use super::recipe::{LodDetail, MeshDetail, TreeRecipe};
use super::skeleton::{Skeleton, Stem};
use crate::rng::Rng;

/// Triangles with their corners: one primitive's worth.
#[derive(Clone, Debug, Default)]
pub struct TreeMesh {
    pub positions: Vec<[f32; 3]>,
    /// One per position, one unit long.
    pub normals: Vec<[f32; 3]>,
    /// One per position: U around the limb (whole repeats, so the seam
    /// meets itself), V along it in copies of the bark's texture, running
    /// on from the parent's.
    pub uvs: Vec<[f32; 2]>,
    /// One per position: the direction U grows, and 1 in `w` (V grows
    /// along `cross(normal, tangent)`, toward the limb's tip).
    pub tangents: Vec<[f32; 4]>,
    /// Three per triangle, counter-clockwise seen from outside.
    pub indices: Vec<u32>,
    /// Leaves only, one per position: out of the crown there (`xyz`) and
    /// how much sky reaches it (`w`, 0 to 1); empty for bark.
    pub crown: Vec<[f32; 4]>,
    /// Bark only, one per position: how weathered the bark is there, 0 to 1
    /// ([`super::Bark::weathering`]); empty for leaves.
    pub weathering: Vec<f32>,
    /// One per position: the packed colour the renderer's foliage shading
    /// and wind read (`R` occlusion, `G` sway, `B` flutter, `A` weathering
    /// or the cluster's shade; `paint.rs`).
    pub colors: Vec<[f32; 4]>,
    /// Bark only, one per position: the limb (index into the skeleton's
    /// stems) the corner belongs to.
    pub(super) limbs: Vec<u32>,
}

impl TreeMesh {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// A bark corner, its weathering left for [`weather`] (below 0).
    fn push(&mut self, position: Vec3, normal: Vec3, uv: [f32; 2], tangent: Vec3) {
        self.push_weathered(position, normal, uv, tangent, -1.0);
    }

    fn push_weathered(
        &mut self,
        position: Vec3,
        normal: Vec3,
        uv: [f32; 2],
        tangent: Vec3,
        weathering: f32,
    ) {
        self.positions.push(position.to_array());
        self.normals.push(normal.to_array());
        self.uvs.push(uv);
        self.tangents.push([tangent.x, tangent.y, tangent.z, 1.0]);
        self.weathering.push(weathering);
    }

    /// Joins `rings` rings of `ring` corners each (the last repeating the
    /// first, for the texture's seam), starting at corner `first`.
    fn strip(&mut self, first: u32, rings: u32, ring: u32) {
        for k in 0..rings.saturating_sub(1) {
            for j in 0..ring - 1 {
                let a = first + k * ring + j;
                let b = a + 1;
                let d = a + ring;
                let c = d + 1;
                self.indices.extend_from_slice(&[a, b, d, b, c, d]);
            }
        }
    }
}

/// One copy of the tree: the trunk's base (empty when the recipe gives it
/// none), the rest of the bark, and the leaves.
#[derive(Clone, Debug, Default)]
pub struct TreeLevel {
    pub base: TreeMesh,
    pub bark: TreeMesh,
    pub leaves: TreeMesh,
}

impl TreeLevel {
    pub fn triangles(&self) -> usize {
        self.base.triangles() + self.bark.triangles() + self.leaves.triangles()
    }
}

/// How finely one copy is meshed.
#[derive(Clone, Copy, Debug)]
struct Detail {
    /// The least studs between two rings (0: every point of the limb), and
    /// between the trunk base's.
    spacing: f32,
    base_spacing: f32,
    /// The share of a ring's corners it keeps.
    radial: f32,
    /// Limbs thinner than this at their start are left out.
    min_radius: f32,
    /// The base's corners as a multiple of the trunk's.
    base_radial: u32,
    /// Whether the base has its relief.
    relief: bool,
    triangles: usize,
}

impl Detail {
    fn full(recipe: &TreeRecipe) -> Self {
        let mesh = &recipe.mesh;
        Self {
            spacing: 0.0,
            base_spacing: 0.0,
            radial: 1.0,
            min_radius: mesh.min_radius,
            base_radial: mesh.base_radial.max(1),
            relief: true,
            triangles: mesh.triangles as usize,
        }
    }

    fn lod(recipe: &TreeRecipe, lod: &LodDetail) -> Self {
        let mesh = &recipe.mesh;
        let spacing = mesh.segment_length.max(0.05) * lod.segment_scale.max(1.0);
        // A base with buttresses keeps enough rings to show them.
        let base_spacing = if lod.base_radial >= 2 {
            (mesh.base_ring_spacing.max(0.05) * lod.segment_scale.max(1.0) * 2.0).min(spacing)
        } else {
            spacing
        };
        Self {
            spacing,
            base_spacing,
            radial: lod.radial_scale.clamp(0.05, 1.0),
            min_radius: mesh.min_radius.max(lod.min_radius),
            base_radial: lod.base_radial.max(1),
            relief: false,
            triangles: lod.triangles as usize,
        }
    }
}

/// How far out a side limb's weld flares onto its parent (the slope of
/// its edge, sideways per stud back along the limb), widest first: a limb
/// whose wide flare would fold tries the next.
const FLARES: [f32; 3] = [0.7, 0.35, 0.0];

/// The simpler copies the renderer can draw.
const MAX_COPIES: usize = 3;

/// How many corners go round a limb that starts `radius` studs thick.
pub fn ring_corners(recipe: &TreeRecipe, radius: f32) -> u32 {
    corners(recipe, radius, 1.0)
}

fn corners(recipe: &TreeRecipe, radius: f32, share: f32) -> u32 {
    let mesh = &recipe.mesh;
    let low = mesh.radial_min.max(3);
    let high = ((mesh.radial_max as f32 * share).round() as u32).max(low);
    let wanted =
        (std::f32::consts::TAU * radius * mesh.radial_per_stud.max(0.0) * share).round() as u32;
    wanted.clamp(low, high)
}

/// The bark of every limb in `skeleton`, most detailed first, then each
/// simpler copy, each within its triangle budget (thinner limbs are left
/// out until it fits). Their leaves are left empty for [`super::leaves`].
pub fn bark_levels(recipe: &TreeRecipe, skeleton: &Skeleton) -> Vec<TreeLevel> {
    let frames: Vec<Vec<Vec3>> = skeleton.stems.iter().map(sides).collect();
    let details = std::iter::once(Detail::full(recipe)).chain(
        recipe
            .mesh
            .lods
            .iter()
            .take(MAX_COPIES)
            .map(|lod| Detail::lod(recipe, lod)),
    );
    let mut levels: Vec<TreeLevel> = Vec::new();
    for mut detail in details {
        let mut level = mesh_level(recipe, skeleton, &frames, detail);
        // Thin limbs go first until it fits, then the base's detail, then
        // rings and corners all over.
        let thickest = skeleton.stems[1..]
            .iter()
            .map(|stem| stem.radii[0])
            .fold(0.0, f32::max);
        for _ in 0..48 {
            if level.triangles() <= detail.triangles {
                break;
            }
            if detail.min_radius <= thickest {
                detail.min_radius = (detail.min_radius * 1.3).max(detail.min_radius + 0.01);
            } else if detail.base_radial > 1 {
                detail.base_radial -= 1;
            } else {
                detail.spacing = detail.spacing.max(recipe.mesh.segment_length.max(0.05)) * 1.5;
                detail.base_spacing = detail.spacing;
                detail.radial *= 0.8;
                if detail.radial < 0.05 {
                    break;
                }
            }
            level = mesh_level(recipe, skeleton, &frames, detail);
        }
        // A copy no simpler than the one before it isn't worth keeping.
        if let Some(before) = levels.last()
            && level.triangles() >= before.triangles()
        {
            break;
        }
        levels.push(level);
    }
    levels
}

fn mesh_level(
    recipe: &TreeRecipe,
    skeleton: &Skeleton,
    frames: &[Vec<Vec3>],
    detail: Detail,
) -> TreeLevel {
    let stems = &skeleton.stems;
    let mut kept = vec![false; stems.len()];
    let mut level = TreeLevel::default();
    for (index, stem) in stems.iter().enumerate() {
        let parent = stem.parent.map(|(parent, _)| parent);
        kept[index] = match parent {
            None => true,
            Some(parent) => kept[parent] && stem.radii[0] >= detail.min_radius,
        };
        if !kept[index] {
            continue;
        }
        let parent_radius = stem
            .parent
            .map(|(parent, t)| stems[parent].at(t).2)
            .unwrap_or(0.0);
        // Where a side limb leaves its parent, its tube is welded onto the
        // parent's surface (not onto a terminal limb, which carries its
        // parent straight on).
        let weld = stem.parent.and_then(|(parent, t)| {
            (t < 0.999).then(|| {
                let (center, axis, _) = stems[parent].at(t);
                let drawn = drawn_radius(&recipe.mesh, stems, parent, t);
                let sides = corners(recipe, stems[parent].radii[0], detail.radial) as f32;
                Weld {
                    center,
                    axis,
                    // Inside the parent's flat faces, so no corner floats.
                    radius: drawn * (std::f32::consts::PI / sides).cos() * 0.98,
                }
            })
        });
        let base_end = if index == 0 { skeleton.base_end } else { 0 };
        // Rings closer than about a radius apart would overlap where the
        // limb bends (a short, thick stub).
        let spacing = detail.spacing.max(stem.radii[0] * 0.75);
        let picks = picks(stem, spacing, detail.base_spacing, base_end);
        let corners = corners(recipe, stem.radii[0], detail.radial) as usize;
        let tube = Tube {
            recipe,
            stem,
            sides: &frames[index],
            corners,
            parent_radius,
            weld,
            seed: recipe.seed ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
        };
        if base_end > 0 {
            let split = picks.iter().position(|&k| k == base_end).unwrap_or(0);
            base(
                &tube,
                &picks[..=split],
                &picks[split..],
                &detail,
                &mut level.base,
            );
            tube.mesh(&picks[split..], &mut level.bark);
        } else {
            tube.mesh(&picks, &mut level.bark);
        }
        for mesh in [&mut level.base, &mut level.bark] {
            let new = mesh.positions.len() - mesh.limbs.len();
            mesh.limbs.extend(std::iter::repeat_n(index as u32, new));
        }
    }
    weather(recipe, stems, &mut level.base);
    weather(recipe, stems, &mut level.bark);
    level
}

/// The share of the side limbs (not terminal ones, nor three-cornered
/// twigs) the most detailed copy welds onto their parents rather than
/// burying; `None` when it has none thick enough.
#[cfg(test)]
pub(super) fn welded_share(recipe: &TreeRecipe, skeleton: &Skeleton) -> Option<f32> {
    let stems = &skeleton.stems;
    let frames: Vec<Vec<Vec3>> = stems.iter().map(sides).collect();
    let detail = Detail::full(recipe);
    let (mut side, mut welded) = (0, 0);
    for (index, stem) in stems.iter().enumerate() {
        let Some((parent, t)) = stem.parent else {
            continue;
        };
        let round = corners(recipe, stem.radii[0], detail.radial) as usize;
        if t >= 0.999 || stem.radii[0] < detail.min_radius || round < 4 {
            continue;
        }
        side += 1;
        let (center, axis, _) = stems[parent].at(t);
        let drawn = drawn_radius(&recipe.mesh, stems, parent, t);
        let corners_round = corners(recipe, stems[parent].radii[0], detail.radial) as f32;
        let weld = Weld {
            center,
            axis,
            radius: drawn * (std::f32::consts::PI / corners_round).cos() * 0.98,
        };
        let tube = Tube {
            recipe,
            stem,
            sides: &frames[index],
            corners: round,
            parent_radius: stems[parent].at(t).2,
            weld: Some(weld),
            seed: 0,
        };
        let picks = picks(stem, detail.spacing, detail.base_spacing, 0);
        welded += usize::from(tube.welded(weld, &picks).is_some());
    }
    (side > 0).then(|| welded as f32 / side as f32)
}

/// A limb's radius as drawn at `t` along it: its own, widened by its
/// collar near where it leaves its parent ([`Tube::radius`]).
fn drawn_radius(mesh: &MeshDetail, stems: &[Stem], index: usize, t: f32) -> f32 {
    let stem = &stems[index];
    let radius = stem.at(t).2;
    let collar = mesh.collar.max(0.0);
    let Some((parent, at)) = stem.parent else {
        return radius;
    };
    if collar <= 0.0 {
        return radius;
    }
    let outside = (stem.length() * t - stems[parent].at(at).2).max(0.0);
    let reach = (1.5 * stem.radii[0]).max(1e-3);
    radius * (1.0 + collar * (-outside / reach).exp())
}

/// Fills in how weathered the bark is at each corner not yet given one:
/// the recipe's [`super::Bark::weathering`] on the high limbs, fading out
/// below its `weathering_from`; splinters (already given 1) are bare wood.
fn weather(recipe: &TreeRecipe, stems: &[Stem], mesh: &mut TreeMesh) {
    let bark = &recipe.bark;
    let strength = bark.weathering.clamp(0.0, 1.0);
    let height = stems
        .iter()
        .flat_map(|stem| stem.points.iter().map(|p| p.y))
        .fold(1.0f32, f32::max);
    let from = bark.weathering_from.clamp(0.0, 0.99) * height;
    for (weathering, position) in mesh.weathering.iter_mut().zip(&mesh.positions) {
        if *weathering < 0.0 {
            *weathering = strength * smoothstep(from, height, position[1]);
        }
    }
}

/// The parent's surface a side limb is welded onto, as a cylinder round
/// its center line where the limb leaves it.
#[derive(Clone, Copy, Debug)]
struct Weld {
    center: Vec3,
    axis: Vec3,
    radius: f32,
}

impl Weld {
    /// How far `point` is from the parent's center line.
    fn distance(&self, point: Vec3) -> f32 {
        (point - self.center).reject_from(self.axis).length()
    }

    /// Where the segment from `outside` to `inside` first meets the
    /// parent's surface, as a share of the way (`None` if it doesn't).
    fn cross(&self, outside: Vec3, inside: Vec3) -> Option<f32> {
        let a = (outside - self.center).reject_from(self.axis);
        let d = (inside - outside).reject_from(self.axis);
        let (qa, qb, qc) = (
            d.dot(d),
            2.0 * a.dot(d),
            a.dot(a) - self.radius * self.radius,
        );
        if qa < 1e-12 {
            return None;
        }
        let disc = qb * qb - 4.0 * qa * qc;
        if disc < 0.0 {
            return None;
        }
        let s = (-qb - disc.sqrt()) / (2.0 * qa);
        (0.0..=1.0).contains(&s).then_some(s)
    }

    /// The parent's surface normal at `point`.
    fn normal(&self, point: Vec3) -> Vec3 {
        (point - self.center)
            .reject_from(self.axis)
            .normalize_or(self.axis.any_orthonormal_vector())
    }
}

/// The sideways direction of each ring of `stem`, carried along it by the
/// smallest rotation from point to point. Every copy uses the same ones,
/// so its texture sits where the most detailed copy's does.
fn sides(stem: &Stem) -> Vec<Vec3> {
    let mut side = stem.direction(0).any_orthonormal_vector();
    let mut previous = stem.direction(0);
    (0..stem.points.len())
        .map(|k| {
            let direction = stem.direction(k);
            side = (Quat::from_rotation_arc(previous, direction) * side)
                .reject_from(direction)
                .normalize_or(direction.any_orthonormal_vector());
            previous = direction;
            side
        })
        .collect()
}

/// The points of `stem` a copy has rings at: its first and last, `keep`
/// (where the trunk's base ends), and between them each point at least
/// `spacing` studs on from the last one taken (`base_spacing` up to
/// `keep`).
fn picks(stem: &Stem, spacing: f32, base_spacing: f32, keep: usize) -> Vec<usize> {
    let last = stem.points.len() - 1;
    let mut picks = vec![0];
    for k in 1..last {
        let spacing = if k < keep { base_spacing } else { spacing };
        let since = stem.along[k] - stem.along[*picks.last().unwrap()];
        let room = stem.along[last] - stem.along[k];
        // Not so close to the tip that the last segment is a sliver.
        if k == keep || (since >= spacing * 0.999 && room >= spacing * 0.5) {
            picks.push(k);
        }
    }
    picks.push(last);
    picks
}

/// One limb's tube in one copy.
struct Tube<'a> {
    recipe: &'a TreeRecipe,
    stem: &'a Stem,
    sides: &'a [Vec3],
    corners: usize,
    parent_radius: f32,
    weld: Option<Weld>,
    /// Picks its splinters, when it's broken.
    seed: u64,
}

impl Tube<'_> {
    fn repeats_around(&self) -> f32 {
        let tile = self.recipe.bark.uv_tile[0].max(0.01);
        (std::f32::consts::TAU * self.stem.radii[0] / tile)
            .round()
            .max(1.0)
    }

    fn v(&self, k: usize) -> f32 {
        (self.stem.v_start + self.stem.along[k]) / self.recipe.bark.uv_tile[1].max(0.01)
    }

    /// The ring's frame at point `k`: along the limb, sideways, and the
    /// third way round.
    fn frame(&self, k: usize) -> (Vec3, Vec3, Vec3) {
        let direction = self.stem.direction(k);
        let side = self.sides[k];
        (direction, side, direction.cross(side))
    }

    /// The radius drawn at point `k`: the limb's own, widened where it
    /// leaves its parent (the collar), wholly inside the parent and fading
    /// over about one and a half of its radii outside it.
    fn radius(&self, k: usize) -> f32 {
        let stem = self.stem;
        let radius = stem.radii[k];
        let collar = self.recipe.mesh.collar.max(0.0);
        if stem.parent.is_none() || collar <= 0.0 {
            return radius;
        }
        let outside = (stem.along[k] - self.parent_radius).max(0.0);
        let reach = (1.5 * stem.radii[0]).max(1e-3);
        let widened = radius * (1.0 + collar * (-outside / reach).exp());
        // Never wider than the parent it leaves (a limb nearly as thick as
        // its parent would otherwise flare past it).
        widened.min(radius.max(self.parent_radius * 0.95))
    }

    /// How fast the radius shrinks along the limb at the `i`th of `picks`
    /// (which tips the normal toward the tip, as on a cone).
    fn slope(&self, picks: &[usize], i: usize) -> f32 {
        let last = picks.len() - 1;
        let (a, b) = if i == 0 {
            (picks[0], picks[1])
        } else if i == last {
            (picks[last - 1], picks[last])
        } else {
            (picks[i - 1], picks[i + 1])
        };
        let span = (self.stem.along[b] - self.stem.along[a]).max(1e-4);
        ((self.radius(a) - self.radius(b)) / span).clamp(-2.0, 2.0)
    }

    /// The ring at point `k` (the `i`th of `picks`).
    fn ring(&self, picks: &[usize], i: usize) -> Ring {
        let k = picks[i];
        let (direction, side, other) = self.frame(k);
        let radius = self.radius(k);
        let slope = self.slope(picks, i);
        let mut ring = Ring::default();
        for j in 0..=self.corners {
            let angle = std::f32::consts::TAU * j as f32 / self.corners as f32;
            let (sin, cos) = angle.sin_cos();
            let out = side * cos + other * sin;
            ring.points.push(self.stem.points[k] + out * radius);
            ring.normals
                .push((out + direction * slope).normalize_or(out));
            ring.tangents
                .push((other * cos - side * sin).normalize_or(other));
            ring.along.push(self.stem.along[k]);
        }
        ring
    }

    /// The rings that weld a side limb onto its parent: one lying on the
    /// parent's surface, flared out round the limb, and one curving from
    /// there onto the limb (a fillet, so the crotch has no hard edge); and
    /// which of `picks` is the first ring wholly outside the parent, where
    /// the limb's own rings carry on. `None` when the limb never leaves its
    /// parent.
    fn welded(&self, weld: Weld, picks: &[usize]) -> Option<(usize, Vec<Ring>)> {
        FLARES
            .iter()
            .find_map(|&flare| self.welded_with(weld, picks, flare))
    }

    fn welded_with(&self, weld: Weld, picks: &[usize], flare: f32) -> Option<(usize, Vec<Ring>)> {
        // A three-cornered twig is too fine to show its join.
        if self.corners < 4 {
            return None;
        }
        let outside = (1..picks.len()).find(|&i| {
            let ring = self.ring(picks, i);
            ring.points
                .iter()
                .all(|&point| weld.distance(point) > weld.radius * 1.03)
        })?;
        let k = picks[outside];
        let (direction, side, other) = self.frame(k);
        let radius = self.radius(k);
        let slope = self.slope(picks, outside);
        let root = self.stem.points[picks[0]];
        let start = self.stem.direction(picks[0]);
        // The flare's inner end, inside the parent: the limb's own radius
        // there, widened by its collar, but never out of the parent.
        let inner = self.radius(picks[0]).min(weld.radius * 0.9);
        let reach = (weld.radius + radius) * 4.0;
        let along = |point: Vec3| (point - root).dot(start).max(0.0);
        let (mut surface, mut fillet) = (Ring::default(), Ring::default());
        for j in 0..=self.corners {
            let angle = std::f32::consts::TAU * j as f32 / self.corners as f32;
            let (sin, cos) = angle.sin_cos();
            let out = side * cos + other * sin;
            let tangent = (other * cos - side * sin).normalize_or(other);
            let on_limb = self.stem.points[k] + out * radius;
            let hit = |towards: Vec3| {
                let end = on_limb + towards * reach;
                weld.cross(on_limb, end).map(|s| on_limb.lerp(end, s))
            };
            // Where the limb's straight side would meet the parent: the
            // corner the fillet rounds off.
            let corner = hit(-direction);
            // The flare's edge on the parent: back along the limb and out
            // from it, so it's always wider than the limb (never folded
            // under it); else the corner itself, else straight in.
            let deep = root + out * inner;
            let on_parent = hit((out * flare - direction).normalize())
                .or(corner)
                .unwrap_or_else(|| on_limb.lerp(deep, weld.cross(on_limb, deep).unwrap_or(1.0)));
            let corner = corner.unwrap_or(on_parent);
            let middle = on_parent * 0.25 + corner * 0.5 + on_limb * 0.25;
            let limb_normal = (out + direction * slope).normalize_or(out);
            let parent_normal = weld.normal(on_parent);
            surface.points.push(on_parent);
            surface.normals.push(parent_normal);
            surface.tangents.push(tangent);
            surface.along.push(along(on_parent));
            fillet.points.push(middle);
            fillet
                .normals
                .push((parent_normal + limb_normal).normalize_or(limb_normal));
            fillet.tangents.push(tangent);
            fillet.along.push(along(middle));
        }
        let rings = vec![surface, fillet];
        // A limb nearly as thick as its parent would wrap its flare round
        // it and fold: seen down the limb, each welded ring must go round
        // it once, corner after corner. Such a limb keeps its plain buried
        // start.
        let center = self.stem.points[k];
        // And each corner must move on along the limb from ring to ring.
        let forward = |p: Vec3| (p - center).dot(direction);
        let backward = (0..=self.corners).any(|j| {
            forward(rings[0].points[j]) >= forward(rings[1].points[j]) - 1e-4
                || forward(rings[1].points[j]) >= -1e-4
        });
        let folds = backward
            || rings.iter().any(|ring| {
                let angle = |p: Vec3| {
                    let d = p - center;
                    d.dot(other).atan2(d.dot(side))
                };
                (0..self.corners).any(|j| {
                    let turn = (angle(ring.points[j + 1]) - angle(ring.points[j]))
                        .rem_euclid(std::f32::consts::TAU);
                    turn <= 0.0 || turn >= std::f32::consts::PI
                })
            });
        if folds {
            return None;
        }
        // Last, the faces the welded rings make must face as their light
        // does (the check the tube's tests make).
        let mut check = rings.clone();
        check.extend((outside..picks.len().min(outside + 2)).map(|i| self.ring(picks, i)));
        smooth_normals(&mut check, self.corners);
        let facing = |a: usize, b: usize, c: usize, ring: [&Ring; 3]| {
            let [pa, pb, pc] = [ring[0].points[a], ring[1].points[b], ring[2].points[c]];
            let face = (pb - pa).cross(pc - pa);
            let normal = ring[0].normals[a] + ring[1].normals[b] + ring[2].normals[c];
            face.length_squared() < 1e-14 || face.dot(normal) > 0.0
        };
        let clean = check[..3].windows(2).all(|pair| {
            let (low, high) = (&pair[0], &pair[1]);
            (0..self.corners).all(|j| {
                facing(j, j + 1, j, [low, low, high]) && facing(j + 1, j + 1, j, [low, high, high])
            })
        });
        clean.then_some((outside, rings))
    }

    /// The tube through the points `picks`, closed at the tip (or ending in
    /// splinters when the limb is broken).
    fn mesh(&self, picks: &[usize], mesh: &mut TreeMesh) {
        if picks.len() < 2 {
            return;
        }
        let corners = self.corners;
        let repeats = self.repeats_around();
        let twist = self.recipe.bark.twist / 360.0;
        let tile = self.recipe.bark.uv_tile[1].max(0.01);
        let mut rings = Vec::new();
        let mut from = 0;
        if let Some(weld) = self.weld {
            if let Some((outside, welded)) = self.welded(weld, picks) {
                rings.extend(welded);
                from = outside;
            } else {
                // Buried instead: from its last ring still wholly inside the
                // parent (the ones before it only fold up unseen).
                from = (0..picks.len() - 1)
                    .rev()
                    .find(|&i| {
                        self.ring(picks, i)
                            .points
                            .iter()
                            .all(|&point| weld.distance(point) < weld.radius)
                    })
                    .unwrap_or(0);
            }
        }
        rings.extend((from..picks.len()).map(|i| self.ring(picks, i)));
        smooth_normals(&mut rings, self.corners);
        let first = mesh.positions.len() as u32;
        for ring in &rings {
            for j in 0..=corners {
                let along = ring.along[j];
                mesh.push(
                    ring.points[j],
                    ring.normals[j],
                    [
                        repeats * (j as f32 / corners as f32 + twist * along),
                        (self.stem.v_start + along) / tile,
                    ],
                    ring.tangents[j],
                );
            }
        }
        let ring = corners as u32 + 1;
        mesh.strip(first, rings.len() as u32, ring);
        let last_ring = first + (rings.len() as u32 - 1) * ring;
        let last = *picks.last().unwrap();
        let direction = self.stem.direction(last);
        let tip_radius = self.radius(last);
        let length = self.stem.length();
        let uv = |j: f32, along: f32| {
            [
                repeats * (j + twist * along),
                (self.stem.v_start + along) / tile,
            ]
        };
        if self.stem.broken {
            self.splinters(last, last_ring, uv, mesh);
            return;
        }
        // The tip: a point a little past the last ring, fanned to it.
        let tip_index = mesh.positions.len() as u32;
        mesh.push(
            self.stem.points[last] + direction * tip_radius * 0.5,
            direction,
            uv(0.5, length + tip_radius * 0.5),
            direction.any_orthonormal_vector(),
        );
        for j in 0..corners as u32 {
            mesh.indices
                .extend_from_slice(&[last_ring + j, last_ring + j + 1, tip_index]);
        }
    }

    /// A snapped-off end after the ring at point `last` (whose first corner
    /// is `last_ring`): splinters of bare wood of random lengths round the
    /// edge, and a ragged break between them, a little sunk.
    fn splinters(
        &self,
        last: usize,
        last_ring: u32,
        uv: impl Fn(f32, f32) -> [f32; 2],
        mesh: &mut TreeMesh,
    ) {
        let corners = self.corners;
        let (direction, side, other) = self.frame(last);
        let radius = self.radius(last);
        let center = self.stem.points[last];
        let length = self.stem.length();
        let mut rng = Rng::new(self.seed, 0x5B11_7E25);
        let spikes: Vec<(f32, f32)> = (0..corners)
            .map(|_| {
                // Each rises more than it leans in, so the splintered wall
                // still faces out.
                let long = if rng.unit() < 0.35 {
                    rng.range(0.8, 2.5)
                } else {
                    rng.range(0.3, 0.6)
                };
                (long * radius, rng.range(0.1, 0.3))
            })
            .collect();
        // Each splinter stands over its own corner of the last ring, so the
        // strip between them never twists over itself.
        let points: Vec<Vec3> = (0..corners)
            .map(|j| {
                let (long, inward) = spikes[j];
                let below = Vec3::from(mesh.positions[last_ring as usize + j]);
                let out = below - center;
                let out = out - direction * out.dot(direction);
                center + direction * long + out * (1.0 - inward)
            })
            .collect();
        let jagged = mesh.positions.len() as u32;
        for j in 0..=corners {
            let angle = std::f32::consts::TAU * j as f32 / corners as f32;
            let (sin, cos) = angle.sin_cos();
            let out = side * cos + other * sin;
            // Lit by its own shape: round the splinters, crossed with the
            // way from the bark below.
            let below = Vec3::from(mesh.positions[(last_ring as usize) + j]);
            let round = points[(j + 1) % corners] - points[(j + corners - 1) % corners];
            // Leaned on along the limb, as the break's edge faces both ways.
            let normal = (round.cross(points[j % corners] - below).normalize_or(out)
                + direction * 0.6)
                .normalize_or((out + direction).normalize_or(out));
            mesh.push_weathered(
                points[j % corners],
                normal,
                uv(j as f32 / corners as f32, length + spikes[j % corners].0),
                (other * cos - side * sin).normalize_or(other),
                1.0,
            );
        }
        // `strip` joins consecutive rings, so the splinters come right
        // after the last ring.
        debug_assert_eq!(jagged, last_ring + corners as u32 + 1);
        mesh.strip(last_ring, 2, corners as u32 + 1);
        // The break between them faces on along the limb: its own corners,
        // lit as the end grain.
        let cap = mesh.positions.len() as u32;
        for j in 0..=corners {
            let i = (jagged + j as u32) as usize;
            let (position, uv) = (Vec3::from(mesh.positions[i]), mesh.uvs[i]);
            mesh.push_weathered(position, direction, uv, side, 1.0);
        }
        let jagged = cap;
        let middle = mesh.positions.len() as u32;
        mesh.push_weathered(
            center + direction * radius * 0.1,
            direction,
            uv(0.5, length),
            direction.any_orthonormal_vector(),
            1.0,
        );
        for j in 0..corners as u32 {
            mesh.indices
                .extend_from_slice(&[jagged + j, jagged + j + 1, middle]);
        }
    }
}

/// Each ring's normals from the tube's own shape (round the ring, crossed
/// with along the tube), so the light always agrees with the faces, even
/// where a weld flares or a short limb's collar narrows fast. A corner
/// whose shape gives no direction keeps the one it has.
fn smooth_normals(rings: &mut [Ring], corners: usize) {
    let shaped: Vec<Vec<Vec3>> = (0..rings.len())
        .map(|r| {
            let (before, after) = (r.saturating_sub(1), (r + 1).min(rings.len() - 1));
            (0..=corners)
                .map(|j| {
                    let ring = &rings[r];
                    let round =
                        ring.points[(j + 1) % corners] - ring.points[(j + corners - 1) % corners];
                    let along = rings[after].points[j] - rings[before].points[j];
                    let normal = round.cross(along);
                    let given = ring.normals[j];
                    if normal.length_squared() < 1e-14 {
                        return given;
                    }
                    normal.normalize()
                })
                .collect()
        })
        .collect();
    for (ring, normals) in rings.iter_mut().zip(shaped) {
        ring.normals = normals;
    }
}

/// One ring of a tube: its corners (the first repeated last, for the
/// texture's seam), and how far along the limb each is.
#[derive(Clone, Default)]
struct Ring {
    points: Vec<Vec3>,
    normals: Vec<Vec3>,
    tangents: Vec<Vec3>,
    along: Vec<f32>,
}

/// The trunk's base through the points `picks` (ending where the rest of
/// the trunk, `above`, begins): `detail.base_radial` times the trunk's
/// corners, swollen into root buttresses and cut by bark ridges that fade
/// out before its top, whose ring lies exactly on the trunk's first, so the
/// two meet without a gap.
fn base(trunk: &Tube<'_>, picks: &[usize], above: &[usize], detail: &Detail, mesh: &mut TreeMesh) {
    if picks.len() < 2 {
        return;
    }
    let recipe = trunk.recipe;
    let shape = &recipe.trunk;
    let stem = trunk.stem;
    let times = detail.base_radial.max(1) as usize;
    let corners = trunk.corners * times;
    let repeats = trunk.repeats_around();
    let sink = shape.sink.max(0.0);
    let top = stem.along[*picks.last().unwrap()] - sink;
    // Buttresses need corners to show; on a coarse copy the base is plain.
    let buttress = if times >= 2 {
        shape.buttress.max(0.0)
    } else {
        0.0
    };
    let relief = if detail.relief {
        shape.relief.max(0.0)
    } else {
        0.0
    };
    let lobes = shape.buttress_count.max(1) as f32;
    // The grain's spiral, as the rest of the trunk has it.
    let twist = recipe.bark.twist / 360.0;
    let ridges = shape.relief_ridges.max(1);
    // Where the lobes and ridges fall round the trunk, from the trunk's own
    // seed, so every copy agrees.
    let mut rng = Rng::new(recipe.seed, 0xBA5E);
    let lobe_phase = rng.range(0.0, std::f32::consts::TAU);
    let ridge_phases: [f32; 6] = std::array::from_fn(|_| rng.range(0.0, std::f32::consts::TAU));
    // How much the trunk's surface stands out at height `h` and angle `a`,
    // as a share of its radius: none at the top.
    let swell = |a: f32, h: f32| -> f32 {
        let fade = 1.0 - smoothstep(top * 0.55, top, h);
        // Under the ground the lobes run on as narrower, longer root
        // tendrils into the skirt.
        let under = if sink > 0.0 {
            (-h / sink).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let lobe = (0.5 + 0.5 * (lobes * (a - lobe_phase)).cos()).powf(3.0 + 5.0 * under);
        let rise = (-h.max(0.0) / shape.buttress_height.max(0.1)).exp() * (1.0 + under);
        let mut ridge = 0.0;
        for (octave, weight) in [(1u32, 0.6f32), (2, 0.3), (5, 0.1)] {
            let n = (ridges * octave) as f32;
            let i = octave as usize % 3;
            let wander = 0.4 * (h * 0.7 / octave as f32 + ridge_phases[i + 3]).sin();
            // `|sin(n a / 2)|` repeats every turn for any whole `n`: narrow
            // furrows between broad ridges, about 0 on average.
            ridge += weight * ((0.5 * n * a + ridge_phases[i] + wander).sin().abs() - 0.637);
        }
        fade * (buttress * lobe * rise + relief * ridge)
    };
    let first = mesh.positions.len() as u32;
    let rings = picks.len();
    let mut grid = Vec::with_capacity(rings * (corners + 1));
    for &k in picks {
        let (_, side, other) = trunk.frame(k);
        let radius = trunk.radius(k);
        let h = stem.along[k] - sink;
        for j in 0..=corners {
            let at = |j: usize| {
                let a = std::f32::consts::TAU * j as f32 / corners as f32;
                let out = side * a.cos() + other * a.sin();
                stem.points[k] + out * radius * (1.0 + swell(a, h))
            };
            let point = if k == *picks.last().unwrap() && j % times != 0 {
                // The top ring: between the trunk's own corners, on the
                // straight edge between them, where the trunk's ring is.
                let low = j - j % times;
                let f = (j % times) as f32 / times as f32;
                at(low).lerp(at(low + times), f)
            } else {
                at(j)
            };
            grid.push(point);
        }
    }
    let ring = corners + 1;
    let seam_slope = trunk.slope(above, 0);
    let (top_direction, top_side, top_other) = trunk.frame(*picks.last().unwrap());
    for (r, &k) in picks.iter().enumerate() {
        let (_, side, other) = trunk.frame(k);
        for j in 0..=corners {
            let a = std::f32::consts::TAU * j as f32 / corners as f32;
            let point = grid[r * ring + j];
            let out = side * a.cos() + other * a.sin();
            let (normal, tangent) = if r == rings - 1 {
                // As the trunk's first ring has it, so the light doesn't
                // break at the join.
                let corner_out = |j: usize| {
                    let a = std::f32::consts::TAU * j as f32 / corners as f32;
                    top_side * a.cos() + top_other * a.sin()
                };
                let out = if j % times == 0 {
                    corner_out(j)
                } else {
                    let low = j - j % times;
                    let f = (j % times) as f32 / times as f32;
                    corner_out(low).lerp(corner_out(low + times), f)
                };
                let normal = (out + top_direction * seam_slope).normalize_or(out);
                let tangent = top_direction.cross(normal).normalize_or(top_side);
                (normal, tangent)
            } else {
                // From the neighbours on the grid: round the ring and along.
                let previous = grid[r * ring + (j + corners - 1) % corners];
                let next = grid[r * ring + (j + 1) % corners];
                let below = grid[r.saturating_sub(1) * ring + j];
                let higher = grid[(r + 1).min(rings - 1) * ring + j];
                let round = next - previous;
                let up = higher - below;
                let mut normal = round.cross(up).normalize_or(out);
                if normal.dot(out) < 0.0 {
                    normal = -normal;
                }
                let tangent = round.reject_from(normal).normalize_or(other);
                (normal, tangent)
            };
            mesh.push(
                point,
                normal,
                [
                    repeats * (j as f32 / corners as f32 + twist * stem.along[k]),
                    trunk.v(k),
                ],
                tangent,
            );
        }
    }
    mesh.strip(first, rings as u32, ring as u32);
}

fn smoothstep(low: f32, high: f32, x: f32) -> f32 {
    let t = ((x - low) / (high - low).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
