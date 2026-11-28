//! A tree's limbs as center lines with a radius at each point, grown from
//! a recipe: the trunk, then each level on the limbs of the level before.
//! Meshing them is `mesher.rs`'s.

use glam::{Quat, Vec3};

use super::recipe::{BranchLevel, MAX_LEVELS, TreeRecipe};
use crate::rng::Rng;

/// One limb: the trunk (level 0) or a branch of a later level.
#[derive(Clone, Debug)]
pub struct Stem {
    /// 0 for the trunk, 1 for the first branch level, and so on.
    pub level: usize,
    /// The limb it grows on and where along it (0 at its base, 1 at its
    /// tip); `None` for the trunk.
    pub parent: Option<(usize, f32)>,
    /// Its center line from base to tip, at least two points. A branch's
    /// first point is on its parent's center line, so its base is buried in
    /// the parent and no gap shows where they meet.
    pub points: Vec<Vec3>,
    /// Its radius at each point, in studs.
    pub radii: Vec<f32>,
    /// Studs along the center line from its base to each point.
    pub along: Vec<f32>,
    /// Studs along its parent's center line from the trunk's base to where
    /// it grows, so the bark's texture runs on from the parent's.
    pub v_start: f32,
    /// Snapped off: its tip is splinters rather than a point.
    pub broken: bool,
    /// The most its radius may be anywhere: a little under its parent's
    /// where it grows, so the pipe model never makes a limb burst out of
    /// the one it grows on.
    cap: f32,
}

impl Stem {
    /// Its whole length in studs.
    pub fn length(&self) -> f32 {
        self.along.last().copied().unwrap_or(0.0)
    }

    /// The point, direction and radius at `t` (0 at its base, 1 at its tip).
    pub fn at(&self, t: f32) -> (Vec3, Vec3, f32) {
        let target = t.clamp(0.0, 1.0) * self.length();
        let last = self.points.len() - 1;
        let mut i = 0;
        while i + 1 < last && self.along[i + 1] < target {
            i += 1;
        }
        let span = (self.along[i + 1] - self.along[i]).max(1e-6);
        let f = ((target - self.along[i]) / span).clamp(0.0, 1.0);
        let point = self.points[i].lerp(self.points[i + 1], f);
        let direction = (self.points[i + 1] - self.points[i]).normalize_or(Vec3::Y);
        let radius = self.radii[i] + (self.radii[i + 1] - self.radii[i]) * f;
        (point, direction, radius)
    }

    /// The unit direction at point `i` (along the segment after it; at the
    /// tip, the segment before).
    pub fn direction(&self, i: usize) -> Vec3 {
        let last = self.points.len() - 1;
        let (a, b) = if i < last {
            (i, i + 1)
        } else {
            (last - 1, last)
        };
        (self.points[b] - self.points[a]).normalize_or(Vec3::Y)
    }
}

/// Every limb of a tree, the trunk first, each before the limbs on it.
#[derive(Clone, Debug)]
pub struct Skeleton {
    pub stems: Vec<Stem>,
    /// The trunk's point where its finely meshed base ends
    /// ([`super::MeshDetail::base_height`]); 0 when it has none.
    pub base_end: usize,
}

/// The seed for variant `variant` of a recipe seeded with `seed`.
pub(super) fn variant_seed(seed: u64, variant: u32) -> u64 {
    Rng::new(seed, u64::from(variant) ^ 0x7EE5_0000_0000_0000).next()
}

/// Grows the limbs of `recipe`'s tree number `variant`.
pub fn grow(recipe: &TreeRecipe, variant: u32) -> Skeleton {
    let seed = variant_seed(recipe.seed, variant);
    let (trunk, base_end) = trunk(recipe, seed);
    let mut stems = vec![trunk];
    let mut parents = vec![0];
    for (level_index, level) in recipe.levels.iter().take(MAX_LEVELS).enumerate() {
        let mut grown = Vec::new();
        for &parent in &parents {
            // A terminal limb is number `count`, after the level's own.
            let count = level.count + u32::from(level.terminal);
            for i in 0..count {
                // Each limb's own generator, from its parent and number, so
                // a change to one level's count doesn't move another's.
                let mut rng = Rng::new(
                    seed ^ ((level_index as u64 + 1) << 56),
                    ((parent as u64) << 20) | u64::from(i),
                );
                if let Some(stem) =
                    branch(recipe, level, level_index + 1, &stems, parent, i, &mut rng)
                {
                    stems.push(stem);
                    grown.push(stems.len() - 1);
                }
            }
        }
        parents = grown;
    }
    pipe_radii(&mut stems, recipe.pipe);
    Skeleton { stems, base_end }
}

/// How many segments a limb `length` studs long is cut into: at least 3,
/// unless that would make them shorter than `least` studs (a short, thick
/// stub's rings would overlap), then 2.
fn segments(length: f32, segment_length: f32, least: f32) -> usize {
    if length / 3.0 < least {
        return 2;
    }
    ((length / segment_length.max(0.05)).ceil() as usize).clamp(3, 32)
}

/// The trunk, and the point its finely meshed base ends at (0 for none).
fn trunk(recipe: &TreeRecipe, seed: u64) -> (Stem, usize) {
    let trunk = &recipe.trunk;
    let mut rng = Rng::new(seed, 0);
    let length = trunk.length.max(0.1);
    let radius = trunk.radius.max(0.01);
    let lean_side = rng.range(0.0, std::f32::consts::TAU);
    let lean = trunk.lean.to_radians();
    let up =
        Quat::from_axis_angle(Vec3::new(lean_side.cos(), 0.0, lean_side.sin()), lean) * Vec3::Y;
    let phases = [
        rng.range(0.0, std::f32::consts::TAU),
        rng.range(0.0, std::f32::consts::TAU),
    ];
    let wave = std::f32::consts::TAU / trunk.wobble_length.max(0.5);
    // The wobble grows from nothing at the ground, so the foot stands where
    // the tree is placed.
    let wobble = |h: f32| {
        let grow = (h / length).min(1.0);
        trunk.wobble
            * grow
            * Vec3::new(
                (h * wave + phases[0]).sin(),
                0.0,
                (h * wave * 1.37 + phases[1]).sin(),
            )
    };
    let sink = trunk.sink.max(0.0);
    // The base, finely: rings every `base_ring_spacing` studs up to its
    // top (never more than half the trunk), then the rest as any limb.
    let base_top = recipe.mesh.base_height.min(length * 0.5);
    let mut heights = Vec::new();
    let mut base_end = 0;
    if base_top > 0.0 {
        let spacing = recipe.mesh.base_ring_spacing.max(0.05);
        let n = ((base_top + sink) / spacing).ceil().clamp(1.0, 96.0) as usize;
        heights.extend((0..n).map(|k| -sink + (base_top + sink) * k as f32 / n as f32));
        base_end = n;
    }
    let from = if base_end > 0 { base_top } else { -sink };
    let n = segments(length - from, recipe.mesh.segment_length, 0.0);
    heights.extend((0..=n).map(|k| from + (length - from) * k as f32 / n as f32));
    let mut points = Vec::with_capacity(heights.len());
    let mut radii = Vec::with_capacity(heights.len());
    for h in heights {
        points.push(up * h + wobble(h.max(0.0)));
        let t = (h / length).clamp(0.0, 1.0);
        let taper = 1.0 + (trunk.tip_radius - 1.0) * t;
        let flare = 1.0 + trunk.flare * (-h.max(0.0) / trunk.flare_height.max(0.1)).exp();
        // Under the ground it spreads on into a skirt of roots, widest at
        // the sunk foot.
        let under = if sink > 0.0 {
            (-h / sink).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let skirt = 1.0 + trunk.root_spread.max(0.0) * under * under;
        radii.push(radius * taper.max(0.0) * flare * skirt);
    }
    let along = lengths_along(&points);
    let stem = Stem {
        level: 0,
        parent: None,
        points,
        radii,
        along,
        v_start: 0.0,
        broken: trunk.broken,
        cap: f32::INFINITY,
    };
    (stem, base_end)
}

#[allow(clippy::too_many_arguments)]
fn branch(
    recipe: &TreeRecipe,
    level: &BranchLevel,
    level_index: usize,
    stems: &[Stem],
    parent: usize,
    i: u32,
    rng: &mut Rng,
) -> Option<Stem> {
    let on = &stems[parent];
    // The terminal limb carries its parent on from the tip.
    let terminal = level.terminal && i == level.count;
    let count = level.count.max(1) as f32;
    let (start, end) = (level.start.clamp(0.0, 1.0), level.end.clamp(0.0, 1.0));
    let jitter = rng.range(-0.35, 0.35);
    let t = if terminal {
        1.0
    } else {
        (start + (end - start) * ((i as f32 + 0.5 + jitter) / count)).clamp(0.0, 1.0)
    };
    // The trunk's sunk foot isn't a place to grow from.
    let t = if on.level == 0 {
        let ground = recipe.trunk.sink.max(0.0) / on.length().max(1e-3);
        t.max(ground)
    } else {
        t
    };
    let (base, parent_direction, parent_radius) = on.at(t);
    if terminal && on.broken {
        return None;
    }
    let broken = !terminal && level.broken > 0.0 && rng.unit() < level.broken.min(1.0);
    let mut length = on.length() * level.length * level.length_along.at(t) * rng.range(0.85, 1.15);
    // How much of the limb it would have grown is there: a snapped-off
    // stub bends only as much as that part of the whole limb would.
    let mut kept = 1.0;
    if broken {
        // Snapped off somewhere along its first half.
        kept = rng.range(0.2, 0.55);
        length *= kept;
    }
    let cap = parent_radius * 0.9;
    // A terminal limb starts as thick as its parent's tip, so the two read
    // as one limb.
    let radius = if terminal {
        parent_radius * 0.9
    } else {
        (parent_radius * level.radius).min(cap)
    };
    // A limb that hardly comes out of its parent isn't a tube (its rings
    // would overlap): a knot the bark's texture shows better.
    let out_of_parent = if terminal {
        length
    } else {
        length - parent_radius
    };
    if length < 0.2 || out_of_parent < radius * 3.0 || radius < recipe.mesh.min_radius {
        return None;
    }
    // Turned round its parent by the roll, from a side that doesn't twist
    // with the parent's own direction.
    let side = parent_direction.any_orthonormal_vector();
    let roll = (level.roll * i as f32 + rng.range(-12.0, 12.0)).to_radians();
    let axis = Quat::from_axis_angle(parent_direction, roll) * side;
    let angle = if terminal {
        // Nearly straight on, as a leader carries on its limb.
        rng.range(5.0, 15.0)
    } else {
        level.angle + rng.range(-1.0, 1.0) * level.angle_spread
    }
    .to_radians();
    let mut direction = (Quat::from_axis_angle(axis, angle) * parent_direction).normalize();
    let n = segments(length, recipe.mesh.segment_length, radius * 2.0);
    let step = length / n as f32;
    let mut points = Vec::with_capacity(n + 1);
    let mut radii = Vec::with_capacity(n + 1);
    let mut point = base;
    let tip = level.tip_radius.clamp(0.0, 1.0);
    // A smooth wander, a wave or so along the limb on each axis, so it
    // curves as wood does rather than kinking at each ring.
    let waves: [(f32, f32); 3] = std::array::from_fn(|_| {
        (
            rng.range(0.6, 1.6) * std::f32::consts::TAU,
            rng.range(0.0, std::f32::consts::TAU),
        )
    });
    for k in 0..=n {
        points.push(point);
        let along = k as f32 / n as f32;
        radii.push(radius * (1.0 + (tip - 1.0) * along));
        // Bends: down under its weight, which grows with how level it is,
        // up toward the light, and a little wander.
        let share = kept / n as f32;
        let level_ness = 1.0 - direction.y.abs();
        let wave = |(frequency, phase): (f32, f32)| (frequency * along + phase).sin();
        let wander = Vec3::new(wave(waves[0]), wave(waves[1]), wave(waves[2]));
        let bent = (direction
            + Vec3::NEG_Y * level.gravity * share * (0.4 + level_ness)
            + Vec3::Y * level.upward * share
            + wander * level.randomness * share * 3.0)
            .normalize_or(direction);
        // Never more than a gentle turn at one ring, or a limb of few
        // segments kinks and its tube folds at the bend.
        direction = turned_at_most(direction, bent, MAX_TURN.to_radians());
        point += direction * step;
    }
    let along = lengths_along(&points);
    let v_start = on.v_start + on.length() * t;
    Some(Stem {
        level: level_index,
        parent: Some((parent, t)),
        points,
        radii,
        along,
        v_start,
        broken,
        cap,
    })
}

/// The most a limb turns at one of its points, in degrees.
const MAX_TURN: f32 = 35.0;

/// `to`, or `from` turned toward it by no more than `limit` radians.
fn turned_at_most(from: Vec3, to: Vec3, limit: f32) -> Vec3 {
    let angle = from.angle_between(to);
    if angle <= limit {
        return to;
    }
    from.slerp(to, limit / angle).normalize_or(to)
}

fn lengths_along(points: &[Vec3]) -> Vec<f32> {
    let mut total = 0.0;
    let mut along = Vec::with_capacity(points.len());
    along.push(0.0);
    for pair in points.windows(2) {
        total += pair[0].distance(pair[1]);
        along.push(total);
    }
    along
}

/// Thickens each limb toward the pipe model: at each point, its cross-section
/// at least that of its own tip and every limb growing beyond that point.
/// Limbs come after the one they grow on, so going backwards finishes each
/// limb before its parent counts it.
fn pipe_radii(stems: &mut [Stem], strength: f32) {
    let strength = strength.clamp(0.0, 1.0);
    if strength <= 0.0 {
        return;
    }
    for index in (0..stems.len()).rev() {
        // (where along this limb, its child's starting area)
        let children: Vec<(f32, f32)> = stems
            .iter()
            .filter_map(|child| match child.parent {
                Some((parent, t)) if parent == index => Some((t, child.radii[0].powi(2))),
                _ => None,
            })
            .collect();
        let stem = &mut stems[index];
        let length = stem.length().max(1e-6);
        let tip_area = stem.radii.last().copied().unwrap_or(0.0).powi(2);
        let cap = stem.cap;
        for k in 0..stem.points.len() {
            let t = stem.along[k] / length;
            let beyond: f32 = children
                .iter()
                .filter(|(at, _)| *at >= t)
                .map(|(_, area)| area)
                .sum();
            let pipe = (beyond + tip_area).sqrt();
            let own = stem.radii[k];
            if pipe > own {
                stem.radii[k] = (own + (pipe - own) * strength).min(cap.max(own));
            }
        }
    }
}
