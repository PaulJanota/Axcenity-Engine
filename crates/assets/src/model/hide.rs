//! A model with some of its triangles left out: how a body's hidden
//! regions are made, and a garment's cloth under another garment
//! (`hidden.rs`, CO9 and ADR-0045).

use std::collections::HashMap;

use glam::{Mat3, Vec3};

use super::{Model, Primitive, new_key};

impl Model {
    /// A copy without the triangles `hide` says to leave out (it is given
    /// the middle of each triangle's texture coordinates; a primitive
    /// without them keeps every triangle), at every level of detail. The
    /// corners, shapes and skeleton are this model's; its key and its
    /// primitives' keys are new.
    pub fn without_triangles(&self, hide: &dyn Fn([f32; 2]) -> bool) -> Model {
        self.without(&|primitive| by_middle(primitive, hide))
    }

    /// A copy without the triangles that lie under another garment: those
    /// whose three corners are all over skin `covered` names (it is given
    /// each corner's [`Primitive::skin_uvs`]; a primitive without them
    /// is kept as it is), at every level of detail.
    ///
    /// A triangle with a corner out from under the cover stays, so the
    /// cloth still reaches the cover's edge; its covered corners **sink**
    /// `sink` metres (of the model's file) toward the body, against
    /// their normals, so what is left of it dives under the garment over
    /// it and doesn't stand through that garment's hem (a waistband
    /// thicker than the shirt worn over it). Only a skinned model's
    /// corners sink (the file's metres are known from its skeleton's
    /// fit).
    pub fn without_covered(&self, covered: &dyn Fn([f32; 2]) -> bool, sink: f32) -> Model {
        // A normal in the fitted space back in the file's, and a move in
        // the file's in the fitted space.
        let fit = self
            .rig
            .as_ref()
            .filter(|_| sink != 0.0)
            .map(|rig| Mat3::from_mat4(rig.fit));
        self.without(&|primitive| by_skin(primitive, covered, fit.map(|fit| (fit, sink)), false))
    }

    /// [`without_covered`](Self::without_covered) for a garment that lies
    /// on the skin (the underwear): only the triangles **well** under the
    /// cover go, those none of whose corners is shared with a triangle
    /// that reaches out from under it, and nothing sinks. What is left
    /// then ends a ring of triangles farther under the garment over it,
    /// where its sawtooth edge can't be seen beside that garment's own
    /// edge.
    pub fn without_well_covered(&self, covered: &dyn Fn([f32; 2]) -> bool) -> Model {
        self.without(&|primitive| by_skin(primitive, covered, None, true))
    }

    /// Whether every primitive has skin coordinates ([`Primitive::skin_uvs`]):
    /// a garment fitted by the import has, one made on the body by its
    /// package has none.
    pub fn has_skin_uvs(&self) -> bool {
        self.primitives
            .iter()
            .all(|primitive| primitive.skin_uvs.len() == primitive.positions.len())
    }

    /// A copy whose corners (at every level of detail) take the texture
    /// coordinates of `body`'s corner nearest them at rest as their skin
    /// coordinates, for a garment that has none of its own: what is worn
    /// over it then hides it as it hides a fitted garment. A corner
    /// farther than [`SKIN_REACH`] from the body gets none (not a
    /// number), and is never under a cover. Both models need a skeleton
    /// (their files' spaces are known from its fit); without, the copy is
    /// as this model.
    pub fn with_skin_from(&self, body: &Model) -> Model {
        let (Some(own), Some(theirs)) = (&self.rig, &body.rig) else {
            return self.without(&|primitive| Kept {
                indices: primitive.indices.clone(),
                positions: None,
            });
        };
        // The body's corners in its file's space, in a grid.
        let from_body = theirs.fit.inverse();
        let mut corners: Vec<(Vec3, [f32; 2])> = Vec::new();
        for primitive in &body.primitives {
            if primitive.uvs.len() != primitive.positions.len() {
                continue;
            }
            for (p, uv) in primitive.positions.iter().zip(&primitive.uvs) {
                corners.push((from_body.transform_point3(Vec3::from(*p)), *uv));
            }
        }
        let cell = |p: Vec3| (p / SKIN_CELL).floor().as_ivec3().to_array();
        let mut grid: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        for (index, (p, _)) in corners.iter().enumerate() {
            grid.entry(cell(*p)).or_default().push(index as u32);
        }
        let rings = (SKIN_REACH / SKIN_CELL).ceil() as i32;
        let nearest = |p: Vec3| -> [f32; 2] {
            let [x, y, z] = cell(p);
            let mut best = (SKIN_REACH * SKIN_REACH, [f32::NAN; 2]);
            for dx in -rings..=rings {
                for dy in -rings..=rings {
                    for dz in -rings..=rings {
                        let Some(list) = grid.get(&[x + dx, y + dy, z + dz]) else {
                            continue;
                        };
                        for &index in list {
                            let (at, uv) = corners[index as usize];
                            let far = at.distance_squared(p);
                            if far < best.0 {
                                best = (far, uv);
                            }
                        }
                    }
                }
            }
            best.1
        };
        let from_own = own.fit.inverse();
        fn give(
            primitive: &mut Primitive,
            from_own: glam::Mat4,
            nearest: &dyn Fn(Vec3) -> [f32; 2],
        ) {
            primitive.skin_uvs = primitive
                .positions
                .iter()
                .map(|p| nearest(from_own.transform_point3(Vec3::from(*p))))
                .collect();
            for lod in &mut primitive.lods {
                give(lod, from_own, nearest);
            }
        }
        let mut model = self.without(&|primitive| Kept {
            indices: primitive.indices.clone(),
            positions: None,
        });
        for primitive in &mut model.primitives {
            give(primitive, from_own, &nearest);
        }
        model
    }

    fn without(&self, kept: &dyn Fn(&Primitive) -> Kept) -> Model {
        Model {
            key: new_key(),
            size: self.size,
            center: self.center,
            primitives: self
                .primitives
                .iter()
                .map(|primitive| without(primitive, kept))
                .collect(),
            hull_points: self.hull_points.clone(),
            collision: self.collision.clone(),
            proxy: self.proxy.iter().map(Primitive::shared).collect(),
            warnings: Vec::new(),
            rig: self.rig.clone(),
            morph_targets: self.morph_targets.clone(),
            identity_pose: self.identity_pose.clone(),
        }
    }
}

/// How far from the body a garment's corner still takes the body's
/// texture coordinates ([`Model::with_skin_from`]; metres of the file),
/// and the side of the cells the body's corners are looked up in.
const SKIN_REACH: f32 = 0.09;
const SKIN_CELL: f32 = 0.03;

/// What is left of a primitive: its triangles, and its corners where
/// they moved (`None`: as they were).
struct Kept {
    indices: Vec<u32>,
    positions: Option<Vec<[f32; 3]>>,
}

/// The triangles of `primitive` whose middle `hide` doesn't name.
fn by_middle(primitive: &Primitive, hide: &dyn Fn([f32; 2]) -> bool) -> Kept {
    if primitive.uvs.len() != primitive.positions.len() {
        return Kept {
            indices: primitive.indices.clone(),
            positions: None,
        };
    }
    let indices = primitive
        .indices
        .chunks_exact(3)
        .filter(|t| {
            let uv = t.iter().fold([0.0f32; 2], |sum, &v| {
                let uv = primitive.uvs[v as usize];
                [sum[0] + uv[0] / 3.0, sum[1] + uv[1] / 3.0]
            });
            !hide(uv)
        })
        .flatten()
        .copied()
        .collect();
    Kept {
        indices,
        positions: None,
    }
}

/// The triangles of `primitive` with a corner over skin `covered`
/// doesn't name, their covered corners sunk (`sink`: the model's fit and
/// how far, in the file's metres).
fn by_skin(
    primitive: &Primitive,
    covered: &dyn Fn([f32; 2]) -> bool,
    sink: Option<(Mat3, f32)>,
    well: bool,
) -> Kept {
    if primitive.skin_uvs.len() != primitive.positions.len() {
        return Kept {
            indices: primitive.indices.clone(),
            positions: None,
        };
    }
    // A corner with no place on the skin (not a number) is under nothing.
    let under: Vec<bool> = primitive
        .skin_uvs
        .iter()
        .map(|uv| uv[0].is_finite() && uv[1].is_finite() && covered(*uv))
        .collect();
    // Well under: no triangle at the corner reaches out from under the
    // cover (corners at one place counted as one, so a seam is no edge).
    let under = if well {
        let place = |v: u32| primitive.positions[v as usize].map(f32::to_bits);
        let mut out: std::collections::HashSet<[u32; 3]> = std::collections::HashSet::new();
        for t in primitive.indices.chunks_exact(3) {
            if !t.iter().all(|&v| under[v as usize]) {
                out.extend(t.iter().map(|&v| place(v)));
            }
        }
        (0..under.len())
            .map(|v| under[v] && !out.contains(&place(v as u32)))
            .collect()
    } else {
        under
    };
    let indices: Vec<u32> = primitive
        .indices
        .chunks_exact(3)
        .filter(|t| !t.iter().all(|&v| under[v as usize]))
        .flatten()
        .copied()
        .collect();
    let positions = sink
        .filter(|_| primitive.normals.len() == primitive.positions.len())
        .map(|(fit, sink)| {
            let mut positions = primitive.positions.clone();
            let mut sunk = vec![false; positions.len()];
            for &v in &indices {
                let v = v as usize;
                if under[v] && !sunk[v] {
                    sunk[v] = true;
                    // The normal back in the file's space, and the move
                    // from there in the fitted space.
                    let normal =
                        (fit.transpose() * Vec3::from(primitive.normals[v])).normalize_or_zero();
                    positions[v] = (Vec3::from(positions[v]) - fit * normal * sink).to_array();
                }
            }
            positions
        });
    Kept { indices, positions }
}

/// `primitive` (and its simpler copies) with what `kept` leaves of it.
fn without(primitive: &Primitive, kept: &dyn Fn(&Primitive) -> Kept) -> Primitive {
    let Kept { indices, positions } = kept(primitive);
    Primitive {
        key: new_key(),
        positions: positions.unwrap_or_else(|| primitive.positions.clone()),
        normals: primitive.normals.clone(),
        uvs: primitive.uvs.clone(),
        tangents: primitive.tangents.clone(),
        regions: primitive.regions.clone(),
        skin_uvs: primitive.skin_uvs.clone(),
        colors: primitive.colors.clone(),
        indices,
        material: primitive.material.clone(),
        joints: primitive.joints.clone(),
        weights: primitive.weights.clone(),
        morphs: primitive.morphs.clone(),
        lods: primitive
            .lods
            .iter()
            .map(|lod| without(lod, kept))
            .collect(),
        level: primitive.level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelMaterial;

    /// Two triangles side by side: corners 0 to 2, and 1 to 3.
    fn strip(skin_uvs: Vec<[f32; 2]>) -> Model {
        let mut primitive = Primitive::new(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            vec![[0.0, 0.0, 1.0]; 4],
            vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            vec![0, 1, 2, 1, 3, 2],
            ModelMaterial::default(),
        );
        primitive.skin_uvs = skin_uvs;
        Model::from_primitives(vec![primitive]).unwrap()
    }

    /// ADR-0045: a garment's triangle goes only when all its corners lie
    /// over covered skin; one that reaches out from under the cover stays.
    #[test]
    fn a_garment_loses_only_the_triangles_wholly_under_a_cover() {
        // Corners 1, 2 and 3 are over covered skin (u over a half).
        let model = strip(vec![[0.1, 0.5], [0.9, 0.5], [0.8, 0.5], [0.7, 0.5]]);
        let covered = |uv: [f32; 2]| uv[0] > 0.5;
        let left = model.without_covered(&covered, 0.0);
        assert_eq!(left.primitives[0].indices, vec![0, 1, 2]);
        // The corners are all still there: shapes and weights still fit.
        assert_eq!(left.primitives[0].positions, model.primitives[0].positions);
        assert_eq!(left.primitives[0].skin_uvs, model.primitives[0].skin_uvs);
    }

    /// Underwear loses only what is well under the cover: a triangle that
    /// touches one reaching out from under it stays.
    #[test]
    fn a_garment_on_the_skin_keeps_a_ring_of_triangles_at_the_cover_s_edge() {
        let model = strip(vec![[0.1, 0.5], [0.9, 0.5], [0.8, 0.5], [0.7, 0.5]]);
        let covered = |uv: [f32; 2]| uv[0] > 0.5;
        // The second triangle is wholly covered, and shares two corners
        // with the first, which is not.
        let left = model.without_well_covered(&covered);
        assert_eq!(left.primitives[0].indices, model.primitives[0].indices);
        assert_eq!(left.primitives[0].positions, model.primitives[0].positions);
        // Wholly covered, both go.
        let none = model.without_well_covered(&|_| true);
        assert!(none.primitives[0].indices.is_empty());
    }

    /// A model without skin coordinates isn't a fitted garment: nothing
    /// of it is left out.
    #[test]
    fn a_model_without_skin_coordinates_keeps_every_triangle() {
        let model = strip(Vec::new());
        let left = model.without_covered(&|_| true, 0.015);
        assert_eq!(left.primitives[0].indices, model.primitives[0].indices);
    }

    /// A garment made on the body by its package has no skin coordinates:
    /// each corner borrows those of the body's corner nearest it at rest,
    /// in the files' own spaces (the two models are fitted to different
    /// boxes), and what is worn over it then hides it.
    #[test]
    fn a_garment_without_skin_coordinates_borrows_the_body_s() {
        let rigged = |mut model: Model, fit: glam::Mat4| {
            model.rig = Some(std::sync::Arc::new(crate::Rig {
                bones: Vec::new(),
                joints: Vec::new(),
                fit,
            }));
            model
        };
        let corners = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [10.0, 10.0, 0.0],
        ];
        // The body: the strip, its file ten times the fitted size.
        let body_fit = glam::Mat4::from_scale(Vec3::splat(0.1));
        let mut body = strip(Vec::new());
        for (p, at) in body.primitives[0].positions.iter_mut().zip(corners) {
            *p = body_fit.transform_point3(Vec3::from(at)).to_array();
        }
        let body = rigged(body, body_fit);
        // The garment: the same strip in the file, a hair off the body,
        // fitted to a box of its own.
        let fit = glam::Mat4::from_translation(Vec3::new(0.5, 0.0, 0.0))
            * glam::Mat4::from_scale(Vec3::splat(0.05));
        let mut garment = strip(Vec::new());
        for (p, at) in garment.primitives[0].positions.iter_mut().zip(corners) {
            let off = Vec3::from(at) + Vec3::new(0.0, 0.0, 0.01);
            *p = fit.transform_point3(off).to_array();
        }
        let garment = rigged(garment, fit);
        assert!(!garment.has_skin_uvs());
        let worn = garment.with_skin_from(&body);
        assert!(worn.has_skin_uvs());
        assert_eq!(
            worn.primitives[0].skin_uvs,
            vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
        );
        // Under a cover over the right half of the skin, the triangle
        // wholly there goes.
        let left = worn.without_covered(&|uv| uv[0] + uv[1] > 0.9, 0.0);
        assert_eq!(left.primitives[0].indices, vec![0, 1, 2]);

        // A corner far from any skin has no place on it, and stays.
        let mut far = strip(Vec::new());
        for p in &mut far.primitives[0].positions {
            p[2] = 50.0;
        }
        let far = rigged(far, glam::Mat4::IDENTITY).with_skin_from(&body);
        assert!(far.primitives[0].skin_uvs.iter().all(|uv| uv[0].is_nan()));
        let kept = far.without_covered(&|_| true, 0.0);
        assert_eq!(kept.primitives[0].indices, far.primitives[0].indices);
    }

    /// The body's triangles go by their middles, as before.
    #[test]
    fn a_body_loses_the_triangles_whose_middles_are_hidden() {
        let model = strip(Vec::new());
        // The second triangle's middle is at u = 2/3.
        let left = model.without_triangles(&|uv| uv[0] > 0.5);
        assert_eq!(left.primitives[0].indices, vec![0, 1, 2]);
    }
}
