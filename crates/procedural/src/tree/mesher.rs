//! Limbs as tubes: rings of corners along each center line, turned from
//! ring to ring by the smallest rotation (so a limb's texture doesn't twist
//! as it bends), closed at the tip. The trunk's base is its own finer mesh
//! with root buttresses and bark relief (foliage plan T2), and each simpler
//! copy is meshed from the same limbs with fewer rings and corners, so
//! nothing moves at the switch.

use glam::{Quat, Vec3};

use super::recipe::{LodDetail, TreeRecipe};
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
}

impl TreeMesh {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    fn push(&mut self, position: Vec3, normal: Vec3, uv: [f32; 2], tangent: Vec3) {
        self.positions.push(position.to_array());
        self.normals.push(normal.to_array());
        self.uvs.push(uv);
        self.tangents.push([tangent.x, tangent.y, tangent.z, 1.0]);
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

/// One copy of the tree's bark: the trunk's base (empty when the recipe
/// gives it none) and everything else.
#[derive(Clone, Debug, Default)]
pub struct BarkLevel {
    pub base: TreeMesh,
    pub bark: TreeMesh,
}

impl BarkLevel {
    pub fn triangles(&self) -> usize {
        self.base.triangles() + self.bark.triangles()
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
/// out until it fits).
pub fn bark_levels(recipe: &TreeRecipe, skeleton: &Skeleton) -> Vec<BarkLevel> {
    let frames: Vec<Vec<Vec3>> = skeleton.stems.iter().map(sides).collect();
    let details = std::iter::once(Detail::full(recipe)).chain(
        recipe
            .mesh
            .lods
            .iter()
            .take(MAX_COPIES)
            .map(|lod| Detail::lod(recipe, lod)),
    );
    let mut levels: Vec<BarkLevel> = Vec::new();
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
) -> BarkLevel {
    let stems = &skeleton.stems;
    let mut kept = vec![false; stems.len()];
    let mut level = BarkLevel::default();
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
        let base_end = if index == 0 { skeleton.base_end } else { 0 };
        let picks = picks(stem, detail.spacing, detail.base_spacing, base_end);
        let corners = corners(recipe, stem.radii[0], detail.radial) as usize;
        let tube = Tube {
            recipe,
            stem,
            sides: &frames[index],
            corners,
            parent_radius,
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
    }
    level
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
        radius * (1.0 + collar * (-outside / reach).exp())
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

    /// The tube through the points `picks`, closed at the tip.
    fn mesh(&self, picks: &[usize], mesh: &mut TreeMesh) {
        if picks.len() < 2 {
            return;
        }
        let corners = self.corners;
        let repeats = self.repeats_around();
        let first = mesh.positions.len() as u32;
        for (i, &k) in picks.iter().enumerate() {
            let (direction, side, other) = self.frame(k);
            let radius = self.radius(k);
            let slope = self.slope(picks, i);
            for j in 0..=corners {
                let angle = std::f32::consts::TAU * j as f32 / corners as f32;
                let (sin, cos) = angle.sin_cos();
                let out = side * cos + other * sin;
                let normal = (out - direction * slope).normalize_or(out);
                let tangent = (other * cos - side * sin).normalize_or(other);
                mesh.push(
                    self.stem.points[k] + out * radius,
                    normal,
                    [repeats * j as f32 / corners as f32, self.v(k)],
                    tangent,
                );
            }
        }
        let ring = corners as u32 + 1;
        mesh.strip(first, picks.len() as u32, ring);
        // The tip: a point a little past the last ring, fanned to it.
        let last = *picks.last().unwrap();
        let direction = self.stem.direction(last);
        let tip_radius = self.radius(last);
        let tip_index = mesh.positions.len() as u32;
        let v = (self.stem.v_start + self.stem.length() + tip_radius * 0.5)
            / self.recipe.bark.uv_tile[1].max(0.01);
        mesh.push(
            self.stem.points[last] + direction * tip_radius * 0.5,
            direction,
            [repeats * 0.5, v],
            direction.any_orthonormal_vector(),
        );
        let last_ring = first + (picks.len() as u32 - 1) * ring;
        for j in 0..corners as u32 {
            mesh.indices
                .extend_from_slice(&[last_ring + j, last_ring + j + 1, tip_index]);
        }
    }
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
        let lobe = (0.5 + 0.5 * (lobes * (a - lobe_phase)).cos()).powi(3);
        let rise = (-h.max(0.0) / shape.buttress_height.max(0.1)).exp();
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
                let normal = (out - top_direction * seam_slope).normalize_or(out);
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
                [repeats * j as f32 / corners as f32, trunk.v(k)],
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
