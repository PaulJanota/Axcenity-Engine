//! Simpler copies of cut-out foliage ([`AlphaMode::Mask`] leaves and grass
//! made of many small cards): fewer cards, with the ones kept grown so the
//! plant covers about as much of the screen. Moving a card's corners (what
//! [`super::lod`] does to other primitives) stretches the texture's holes,
//! so leaves changed shape at the switch; dropping whole cards and growing
//! the rest about their middles keeps every leaf's outline, which is how
//! SpeedTree and Unreal's foliage thin leaf cards from far away.
//!
//! Which cards go is decided by where they are, so the ones kept are spread
//! evenly over the plant, and the simpler copy's cards are some of the
//! first copy's, so nothing pops back in from one level to the next.
//!
//! Used for the far level of a plant whose file has its own copies
//! ([`super::lod::FAR_LEVEL`]): its last copy, thinned again.
//! `AXCENITY_NO_FAR_CARDS` (set to anything) turns it off, for comparing.

use std::collections::HashMap;

use super::{AlphaMode, Primitive};

/// Plants with fewer cards than this aren't thinned: each card shows.
const MIN_CARDS: usize = 32;

/// Plants whose middle card has more triangles than this aren't thinned:
/// they're one sheet with holes (a fence, a net), not loose cards.
const MAX_CARD_TRIANGLES: usize = 16;

/// How much a kept card may grow across: thinned to a third, cards grow by
/// at most this much rather than turning into big flat sheets.
const MAX_GROWTH: f32 = 1.6;

/// A simpler copy is kept only if it has at most this share of the
/// triangles of the level before it.
const MIN_SAVING: f32 = 0.8;

/// Whether `primitive` is cut-out foliage this module may thin.
pub(super) fn may_thin(primitive: &Primitive) -> bool {
    matches!(primitive.material.alpha_mode, AlphaMode::Mask { .. })
        && primitive.morphs.is_empty()
        && primitive.joints.is_empty()
        && std::env::var_os("AXCENITY_NO_FAR_CARDS").is_none()
}

/// Simpler copies of `primitive` with each share in `keep` of its cards, the kept
/// ones grown about their middles; none if it isn't made of enough small
/// cards, or thinning wouldn't save enough.
pub(super) fn thinned(primitive: &Primitive, keep: &[f32]) -> Vec<Primitive> {
    let cards = cards(primitive);
    if cards.len() < MIN_CARDS {
        return Vec::new();
    }
    let mut sizes: Vec<usize> = cards.iter().map(|card| card.triangles.len()).collect();
    sizes.sort_unstable();
    if sizes[sizes.len() / 2] > MAX_CARD_TRIANGLES {
        return Vec::new();
    }
    let (low, high) = bounds(&primitive.positions);
    let order = spread_order(&cards, low, high);
    let total_area: f32 = cards.iter().map(|card| card.area).sum();
    if total_area <= 0.0 {
        return Vec::new();
    }

    let mut copies = Vec::new();
    let mut last = primitive.indices.len() / 3;
    for &keep in keep {
        let kept: Vec<&Card> = cards
            .iter()
            .zip(&order)
            .filter(|(_, rank)| **rank < keep)
            .map(|(card, _)| card)
            .collect();
        let kept_area: f32 = kept.iter().map(|card| card.area).sum();
        let triangles: usize = kept.iter().map(|card| card.triangles.len()).sum();
        if triangles == 0 || kept_area <= 0.0 || triangles as f32 > last as f32 * MIN_SAVING {
            break;
        }
        last = triangles;
        let growth = (total_area / kept_area).sqrt().min(MAX_GROWTH);
        let mut positions = primitive.positions.clone();
        let mut indices = Vec::with_capacity(triangles * 3);
        for card in kept {
            let scale = growth.min(room(card, &primitive.positions, low, high));
            for &corner in &card.corners {
                let at = &mut positions[corner as usize];
                for (value, middle) in at.iter_mut().zip(card.middle) {
                    *value = middle + (*value - middle) * scale;
                }
            }
            for &triangle in &card.triangles {
                let start = triangle as usize * 3;
                indices.extend_from_slice(&primitive.indices[start..start + 3]);
            }
        }
        copies.push(primitive.with_triangles_at(&indices, &positions));
    }
    copies
}

/// One card: triangles joined by their corners.
struct Card {
    /// Its triangles, by number.
    triangles: Vec<u32>,
    /// The corners they use, each once.
    corners: Vec<u32>,
    /// The average of those corners.
    middle: [f32; 3],
    area: f32,
}

/// `primitive`'s cards: groups of triangles that share corners, or corners
/// at the same place, in the order they first appear.
fn cards(primitive: &Primitive) -> Vec<Card> {
    let corners = primitive.positions.len();
    let mut parent: Vec<u32> = (0..corners as u32).collect();
    fn root(parent: &mut [u32], mut at: u32) -> u32 {
        while parent[at as usize] != at {
            let up = parent[parent[at as usize] as usize];
            parent[at as usize] = up;
            at = up;
        }
        at
    }
    let join = |parent: &mut [u32], a: u32, b: u32| {
        let (a, b) = (root(parent, a), root(parent, b));
        if a != b {
            parent[a.max(b) as usize] = a.min(b);
        }
    };
    // Corners at the same place belong together, even when a file splits
    // them for a texture seam.
    let mut same_place: HashMap<[u32; 3], u32> = HashMap::new();
    for (index, position) in primitive.positions.iter().enumerate() {
        let place = position.map(f32::to_bits);
        let first = *same_place.entry(place).or_insert(index as u32);
        join(&mut parent, first, index as u32);
    }
    for triangle in primitive.indices.chunks_exact(3) {
        if triangle.iter().any(|&corner| corner as usize >= corners) {
            continue;
        }
        join(&mut parent, triangle[0], triangle[1]);
        join(&mut parent, triangle[0], triangle[2]);
    }

    let mut card_of: HashMap<u32, usize> = HashMap::new();
    let mut cards: Vec<Card> = Vec::new();
    let mut seen = vec![false; corners];
    for (number, triangle) in primitive.indices.chunks_exact(3).enumerate() {
        if triangle.iter().any(|&corner| corner as usize >= corners) {
            continue;
        }
        let group = root(&mut parent, triangle[0]);
        let card = *card_of.entry(group).or_insert_with(|| {
            cards.push(Card {
                triangles: Vec::new(),
                corners: Vec::new(),
                middle: [0.0; 3],
                area: 0.0,
            });
            cards.len() - 1
        });
        let card = &mut cards[card];
        card.triangles.push(number as u32);
        let [a, b, c] = [0, 1, 2].map(|i| primitive.positions[triangle[i] as usize]);
        card.area += 0.5 * length(cross(sub(b, a), sub(c, a)));
        for &corner in triangle {
            if !std::mem::replace(&mut seen[corner as usize], true) {
                card.corners.push(corner);
            }
        }
    }
    for card in &mut cards {
        let mut sum = [0.0f32; 3];
        for &corner in &card.corners {
            let at = primitive.positions[corner as usize];
            for axis in 0..3 {
                sum[axis] += at[axis];
            }
        }
        card.middle = sum.map(|total| total / card.corners.len() as f32);
    }
    cards
}

/// For each card, a number in 0..1: cards under a share `keep` are the ones
/// kept. Cards are ordered along a curve through the plant (so neighbours
/// are near each other), and numbered by reversing the bits of their place
/// on it, so neighbours get far-apart numbers and every share is spread
/// evenly.
fn spread_order(cards: &[Card], low: [f32; 3], high: [f32; 3]) -> Vec<f32> {
    let mut along: Vec<(u32, usize)> = cards
        .iter()
        .enumerate()
        .map(|(index, card)| {
            let cell = [0, 1, 2].map(|axis| {
                let size = (high[axis] - low[axis]).max(f32::EPSILON);
                (((card.middle[axis] - low[axis]) / size) * 1023.0).clamp(0.0, 1023.0) as u32
            });
            (morton(cell), index)
        })
        .collect();
    along.sort_unstable();
    let mut order = vec![0.0; cards.len()];
    for (place, &(_, card)) in along.iter().enumerate() {
        order[card] = radical_inverse(place as u32);
    }
    order
}

/// The bits of `x`, `y` and `z` (ten each) interleaved.
fn morton([x, y, z]: [u32; 3]) -> u32 {
    fn spread(mut v: u32) -> u32 {
        v &= 0x3ff;
        v = (v | (v << 16)) & 0x0300_00ff;
        v = (v | (v << 8)) & 0x0300_f00f;
        v = (v | (v << 4)) & 0x030c_30c3;
        (v | (v << 2)) & 0x0924_9249
    }
    spread(x) | (spread(y) << 1) | (spread(z) << 2)
}

/// `place`'s bits reversed after the point: 0, 0.5, 0.25, 0.75, 0.125…
fn radical_inverse(place: u32) -> f32 {
    (place.reverse_bits() as f64 / (1u64 << 32) as f64) as f32
}

/// How much `card` can grow about its middle and stay inside the plant's
/// box (`low` to `high`), never less than 1.
fn room(card: &Card, positions: &[[f32; 3]], low: [f32; 3], high: [f32; 3]) -> f32 {
    let mut most = f32::INFINITY;
    for &corner in &card.corners {
        let at = positions[corner as usize];
        for axis in 0..3 {
            let out = at[axis] - card.middle[axis];
            let limit = if out > 1e-6 {
                (high[axis] - card.middle[axis]) / out
            } else if out < -1e-6 {
                (low[axis] - card.middle[axis]) / out
            } else {
                continue;
            };
            most = most.min(limit);
        }
    }
    most.max(1.0)
}

fn bounds(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for at in positions {
        for axis in 0..3 {
            low[axis] = low[axis].min(at[axis]);
            high[axis] = high[axis].max(at[axis]);
        }
    }
    (low, high)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

#[cfg(test)]
mod tests {
    /// Two copies, as a plant without its own copies would get.
    const KEEP: [f32; 2] = [0.6, 0.35];

    use super::*;
    use crate::model::new_key;

    /// A bush of `count` small square leaf cards, two triangles each,
    /// scattered over a 2 m box, cut out.
    fn bush(count: u32) -> Primitive {
        let mut bush = Primitive {
            key: new_key(),
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            regions: Vec::new(),
            skin_uvs: Vec::new(),
            colors: Vec::new(),
            tangents: Vec::new(),
            indices: Vec::new(),
            material: Default::default(),
            joints: Vec::new(),
            weights: Vec::new(),
            morphs: Vec::new(),
            lods: Vec::new(),
            level: 0,
        };
        bush.material.alpha_mode = AlphaMode::Mask { cutoff: 0.5 };
        for card in 0..count {
            // An even but jumbled spread (golden-ratio steps).
            let step = |k: f32| ((card as f32 + 1.0) * k).fract() * 2.0 - 1.0;
            let middle = [step(0.618_034), step(0.754_878), step(0.569_840)];
            let first = bush.positions.len() as u32;
            for (du, dv) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                bush.positions.push([
                    middle[0] + (du - 0.5) * 0.1,
                    middle[1] + (dv - 0.5) * 0.1,
                    middle[2],
                ]);
                bush.normals.push([0.0, 0.0, 1.0]);
                bush.uvs.push([du, dv]);
            }
            bush.indices
                .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
        }
        bush
    }

    fn card_middles(primitive: &Primitive) -> Vec<[f32; 3]> {
        cards(primitive).iter().map(|card| card.middle).collect()
    }

    #[test]
    fn a_bush_of_cards_gets_copies_with_fewer_bigger_cards() {
        let bush = bush(400);
        let copies = thinned(&bush, &KEEP);
        assert_eq!(copies.len(), 2);
        let full_area: f32 = cards(&bush).iter().map(|card| card.area).sum();
        let mut last = 400;
        for (copy, keep) in copies.iter().zip(KEEP) {
            let kept = cards(copy);
            let share = kept.len() as f32 / 400.0;
            assert!((share - keep).abs() < 0.05, "kept {share} for {keep}");
            assert!(kept.len() < last);
            last = kept.len();
            // Every kept card is a whole square, grown, with its middle
            // where it was.
            for card in &kept {
                assert_eq!(card.triangles.len(), 2);
            }
            let area: f32 = kept.iter().map(|card| card.area).sum();
            assert!(
                area > full_area * 0.8,
                "covers {area} of {full_area} with {keep}"
            );
            assert_eq!(copy.uvs.len(), copy.positions.len());
            assert!(copy.lods.is_empty());
            assert_ne!(copy.key(), bush.key());
        }
        let full = card_middles(&bush);
        for middle in card_middles(&copies[0]) {
            assert!(
                full.iter()
                    .any(|at| (0..3).all(|axis| (at[axis] - middle[axis]).abs() < 1e-5)),
                "a kept card stayed where it was"
            );
        }
    }

    #[test]
    fn the_simpler_copy_keeps_some_of_the_first_copys_cards() {
        let bush = bush(400);
        let copies = thinned(&bush, &KEEP);
        let first = card_middles(&copies[0]);
        for middle in card_middles(&copies[1]) {
            assert!(
                first
                    .iter()
                    .any(|at| (0..3).all(|axis| (at[axis] - middle[axis]).abs() < 1e-5)),
                "every card of the simpler copy is in the first"
            );
        }
    }

    #[test]
    fn kept_cards_are_spread_over_the_whole_bush() {
        let bush = bush(400);
        let copies = thinned(&bush, &KEEP);
        // Each eighth of the box keeps about its share of the cards.
        let octant = |at: [f32; 3]| {
            (at[0] > 0.0) as usize | ((at[1] > 0.0) as usize) << 1 | ((at[2] > 0.0) as usize) << 2
        };
        let mut before = [0usize; 8];
        for middle in card_middles(&bush) {
            before[octant(middle)] += 1;
        }
        let mut after = [0usize; 8];
        for middle in card_middles(&copies[1]) {
            after[octant(middle)] += 1;
        }
        for (b, a) in before.iter().zip(after) {
            let share = a as f32 / *b as f32;
            assert!((share - KEEP[1]).abs() < 0.15, "{after:?} of {before:?}");
        }
    }

    #[test]
    fn grown_cards_stay_inside_the_bush() {
        let bush = bush(400);
        let (low, high) = bounds(&bush.positions);
        for copy in thinned(&bush, &KEEP) {
            for at in &copy.positions {
                for axis in 0..3 {
                    assert!(at[axis] >= low[axis] - 1e-5 && at[axis] <= high[axis] + 1e-5);
                }
            }
        }
    }

    #[test]
    fn a_few_cards_or_one_sheet_with_holes_are_not_thinned() {
        assert!(thinned(&bush(MIN_CARDS as u32 - 1), &KEEP).is_empty());
        // A fence: one sheet of many joined squares.
        let mut fence = bush(1);
        fence.indices.clear();
        fence.positions.clear();
        fence.uvs.clear();
        fence.normals.clear();
        for y in 0..=20u32 {
            for x in 0..=20u32 {
                fence.positions.push([x as f32, y as f32, 0.0]);
                fence.normals.push([0.0, 0.0, 1.0]);
                fence.uvs.push([x as f32, y as f32]);
            }
        }
        for y in 0..20u32 {
            for x in 0..20u32 {
                let c = y * 21 + x;
                fence.indices.extend([c, c + 1, c + 22, c, c + 22, c + 21]);
            }
        }
        assert!(thinned(&fence, &KEEP).is_empty());
    }

    #[test]
    fn corners_at_the_same_place_make_one_card() {
        let mut bush = bush(40);
        // Split the first card's corner 0 for a texture seam.
        let copy = bush.positions[0];
        bush.positions.push(copy);
        bush.normals.push([0.0, 0.0, 1.0]);
        bush.uvs.push([0.5, 0.5]);
        bush.indices[3] = bush.positions.len() as u32 - 1;
        assert_eq!(cards(&bush).len(), 40);
    }
}
