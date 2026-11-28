//! Limbs as tubes: rings of corners along each center line, turned from
//! ring to ring by the smallest rotation (so a limb's texture doesn't twist
//! as it bends), closed at the tip.

use glam::Quat;

use super::recipe::TreeRecipe;
use super::skeleton::{Skeleton, Stem};

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
}

/// How many corners go round a limb that starts `radius` studs thick.
pub fn ring_corners(recipe: &TreeRecipe, radius: f32) -> u32 {
    let mesh = &recipe.mesh;
    let low = mesh.radial_min.max(3);
    let high = mesh.radial_max.max(low);
    let wanted = (std::f32::consts::TAU * radius * mesh.radial_per_stud.max(0.0)).round() as u32;
    wanted.clamp(low, high)
}

/// The bark of every limb in `skeleton`.
pub fn bark(recipe: &TreeRecipe, skeleton: &Skeleton) -> TreeMesh {
    let mut mesh = TreeMesh::default();
    for stem in &skeleton.stems {
        let parent_radius = stem
            .parent
            .map(|(parent, t)| skeleton.stems[parent].at(t).2)
            .unwrap_or(0.0);
        tube(recipe, stem, parent_radius, &mut mesh);
    }
    mesh
}

fn tube(recipe: &TreeRecipe, stem: &Stem, parent_radius: f32, mesh: &mut TreeMesh) {
    let count = stem.points.len();
    if count < 2 {
        return;
    }
    let corners = ring_corners(recipe, stem.radii[0]) as usize;
    let [tile_around, tile_along] = recipe.bark.uv_tile.map(|t| t.max(0.01));
    let repeats_around = (std::f32::consts::TAU * stem.radii[0] / tile_around)
        .round()
        .max(1.0);
    let collar = recipe.mesh.collar.max(0.0);
    let first = mesh.positions.len() as u32;
    // The ring's sideways direction, carried from ring to ring.
    let mut side = stem.direction(0).any_orthonormal_vector();
    let mut previous_direction = stem.direction(0);
    for k in 0..count {
        let direction = stem.direction(k);
        side = (Quat::from_rotation_arc(previous_direction, direction) * side)
            .reject_from(direction)
            .normalize_or(direction.any_orthonormal_vector());
        previous_direction = direction;
        let other = direction.cross(side);
        let radius = radius_at(stem, k, parent_radius, collar);
        // How fast the radius shrinks along the limb tips the normal toward
        // the tip, as on a cone.
        let slope = slope_at(stem, k, parent_radius, collar);
        let v = (stem.v_start + stem.along[k]) / tile_along;
        for j in 0..=corners {
            let angle = std::f32::consts::TAU * j as f32 / corners as f32;
            let (sin, cos) = angle.sin_cos();
            let out = side * cos + other * sin;
            let position = stem.points[k] + out * radius;
            let normal = (out - direction * slope).normalize_or(out);
            let tangent = (other * cos - side * sin).normalize_or(other);
            mesh.positions.push(position.to_array());
            mesh.normals.push(normal.to_array());
            mesh.uvs
                .push([repeats_around * j as f32 / corners as f32, v]);
            mesh.tangents.push([tangent.x, tangent.y, tangent.z, 1.0]);
        }
    }
    let ring = corners as u32 + 1;
    for k in 0..count as u32 - 1 {
        for j in 0..corners as u32 {
            let a = first + k * ring + j;
            let b = a + 1;
            let d = a + ring;
            let c = d + 1;
            mesh.indices.extend_from_slice(&[a, b, d, b, c, d]);
        }
    }
    // The tip: a point a little past the last ring, fanned to it.
    let last = count - 1;
    let direction = stem.direction(last);
    let tip_radius = radius_at(stem, last, parent_radius, collar);
    let tip = stem.points[last] + direction * tip_radius * 0.5;
    let tip_index = mesh.positions.len() as u32;
    mesh.positions.push(tip.to_array());
    mesh.normals.push(direction.to_array());
    let v = (stem.v_start + stem.length() + tip_radius * 0.5) / tile_along;
    mesh.uvs.push([repeats_around * 0.5, v]);
    let tangent = direction.any_orthonormal_vector();
    mesh.tangents.push([tangent.x, tangent.y, tangent.z, 1.0]);
    let last_ring = first + last as u32 * ring;
    for j in 0..corners as u32 {
        mesh.indices
            .extend_from_slice(&[last_ring + j, last_ring + j + 1, tip_index]);
    }
}

/// The radius drawn at point `k`: the limb's own, widened where it leaves
/// its parent (the collar), wholly inside the parent and fading over about
/// one and a half of its radii outside it.
fn radius_at(stem: &Stem, k: usize, parent_radius: f32, collar: f32) -> f32 {
    let radius = stem.radii[k];
    if stem.parent.is_none() || collar <= 0.0 {
        return radius;
    }
    let outside = (stem.along[k] - parent_radius).max(0.0);
    let reach = (1.5 * stem.radii[0]).max(1e-3);
    radius * (1.0 + collar * (-outside / reach).exp())
}

fn slope_at(stem: &Stem, k: usize, parent_radius: f32, collar: f32) -> f32 {
    let last = stem.points.len() - 1;
    let (a, b) = if k == 0 {
        (0, 1)
    } else if k == last {
        (last - 1, last)
    } else {
        (k - 1, k + 1)
    };
    let span = (stem.along[b] - stem.along[a]).max(1e-4);
    let fall =
        radius_at(stem, a, parent_radius, collar) - radius_at(stem, b, parent_radius, collar);
    (fall / span).clamp(-2.0, 2.0)
}
