//! Each corner's packed colour (foliage W3), for the renderer's foliage
//! shading and wind: `R` ambient occlusion baked from the crown and the
//! crotches (1 open, darker inside), `G` how far the trunk and limbs carry
//! it in the wind (0 at the ground, growing out along each limb), `B` how
//! much it flutters on its own (thin twigs, leaf tips and edges) and `A`
//! the bark's weathering, or each leaf cluster's own shade of the season's
//! colour (0 to 1).

use glam::Vec3;

use super::leaves::Crown;
use super::mesher::TreeMesh;
use super::skeleton::{Skeleton, Stem};

/// How much more of the wind's lean each level of limbs takes on, as a
/// share of what is left (the trunk takes [`TRUNK_SWAY`] at its top).
const LIMB_SWAY: f32 = 0.45;
const TRUNK_SWAY: f32 = 0.35;

/// Wood thinner than this (studs) flutters, more the thinner it is.
const FLUTTER_RADIUS: f32 = 0.1;

/// Each limb's sway weight where it starts: its parent's where it grows.
pub(super) struct Sway {
    starts: Vec<f32>,
}

impl Sway {
    pub(super) fn new(skeleton: &Skeleton) -> Self {
        let stems = &skeleton.stems;
        let mut starts = vec![0.0; stems.len()];
        // Limbs come after the one they grow on.
        for (index, stem) in stems.iter().enumerate() {
            if let Some((parent, t)) = stem.parent {
                starts[index] = Self::along(&starts, stems, parent, t);
            }
        }
        Self { starts }
    }

    fn along(starts: &[f32], stems: &[Stem], index: usize, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if stems[index].parent.is_none() {
            return TRUNK_SWAY * t * t;
        }
        let start = starts[index];
        start + (1.0 - start) * LIMB_SWAY * t.powf(1.5)
    }

    /// The sway weight `t` (0 to 1) along limb `index`.
    pub(super) fn at(&self, stems: &[Stem], index: usize, t: f32) -> f32 {
        Self::along(&self.starts, stems, index, t)
    }
}

/// How deep `point` is in the crown: 0 at its middle, 1 on its outline
/// (and beyond).
pub(super) fn depth(crown: &Crown, point: Vec3) -> f32 {
    ((point - crown.center) / crown.half_size.max(Vec3::splat(1e-3)))
        .length()
        .min(1.0)
}

/// Light the crown's inside lets through to wood: dim deep inside.
fn canopy(crown: Option<&Crown>, point: Vec3) -> f32 {
    crown.map_or(1.0, |crown| {
        0.55 + 0.45 * smoothstep(0.15, 1.0, depth(crown, point))
    })
}

/// Paints the bark (or the trunk's base) `mesh`, whose corners' limbs are
/// in `mesh.limbs`: occlusion in the crotches, near the ground and inside
/// the crown, sway out along the limbs, flutter on thin twigs and the
/// weathering already worked out.
pub(super) fn bark(skeleton: &Skeleton, sway: &Sway, crown: Option<&Crown>, mesh: &mut TreeMesh) {
    let stems = &skeleton.stems;
    mesh.colors = mesh
        .positions
        .iter()
        .enumerate()
        .map(|(corner, &position)| {
            let position = Vec3::from(position);
            let index = mesh.limbs.get(corner).map_or(0, |&i| i as usize);
            let stem = &stems[index.min(stems.len() - 1)];
            let t = nearest(stem, position);
            let length = stem.length();
            // Dark in the crotch where a limb leaves its parent.
            let crotch = if stem.parent.is_some() {
                let reach = (1.2 * stem.radii[0]).max(1e-3);
                1.0 - 0.4 * (-(t * length) / reach).exp()
            } else {
                1.0
            };
            let ground = 1.0 - 0.3 * (-position.y.max(0.0) / 0.6).exp();
            let occlusion = crotch * ground * canopy(crown, position);
            let radius = stem.at(t).2;
            let flutter = ((FLUTTER_RADIUS - radius) / FLUTTER_RADIUS).clamp(0.0, 1.0) * 0.5 * t;
            let weathering = mesh.weathering.get(corner).copied().unwrap_or(0.0);
            [
                occlusion.clamp(0.0, 1.0),
                sway.at(stems, index, t).clamp(0.0, 1.0),
                flutter,
                weathering.clamp(0.0, 1.0),
            ]
        })
        .collect();
}

/// How far along `stem` (0 to 1) the point of its center line nearest
/// `point` is.
fn nearest(stem: &Stem, point: Vec3) -> f32 {
    let length = stem.length().max(1e-6);
    let mut best = (f32::INFINITY, 0.0);
    for (i, pair) in stem.points.windows(2).enumerate() {
        let span = pair[1] - pair[0];
        let share =
            ((point - pair[0]).dot(span) / span.length_squared().max(1e-12)).clamp(0.0, 1.0);
        let distance = point.distance_squared(pair[0] + span * share);
        if distance < best.0 {
            let along = stem.along[i] + (stem.along[i + 1] - stem.along[i]) * share;
            best = (distance, along / length);
        }
    }
    best.1
}

pub(super) fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
