//! A tree's recipe (`.axtree`, JSON): a hierarchy of generators in the
//! manner of SpeedTree's, Trunk > Branches > Twigs, each level a list entry
//! with its own settings and curves. Every field has a default, so a file
//! written by an older or newer engine reads (unknown fields are ignored).

use serde::{Deserialize, Serialize};

/// The format a recipe is written in. Bump it only for a change an older
/// engine would read wrongly.
pub const TREE_FORMAT: u32 = 1;

/// The most branch levels under the trunk (branches, twigs and two more).
pub const MAX_LEVELS: usize = 4;

/// A tree: its trunk, its levels of branches and how its mesh is made.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeRecipe {
    pub format: u32,
    pub name: String,
    /// With the variant number ([`super::generate`]), picks every random
    /// choice: the same seed and variant give the same tree.
    pub seed: u64,
    pub trunk: Trunk,
    /// The levels grown off the trunk, then off each other, in order: the
    /// first grows on the trunk, the second on the first's branches. At most
    /// [`MAX_LEVELS`] are used.
    pub levels: Vec<BranchLevel>,
    /// 0 to 1: how far each limb is thickened so its cross-section holds
    /// those of the limbs beyond it (the "pipe model": a fork's two arms
    /// together are as thick as the limb they grow from), so the trunk
    /// widens under its crown. 0 keeps each limb's own taper.
    pub pipe: f32,
    pub mesh: MeshDetail,
    pub bark: Bark,
    pub leaves: Leaves,
}

impl Default for TreeRecipe {
    fn default() -> Self {
        Self::preset(Preset::Oak)
    }
}

/// The trunk: one limb from the ground up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Trunk {
    /// Studs from the ground to its tip.
    pub length: f32,
    /// Studs, at the ground before the flare.
    pub radius: f32,
    /// Its radius at the tip, as a share of `radius`.
    pub tip_radius: f32,
    /// How much wider it is right at the ground (root flare), as a share of
    /// `radius`, fading out over `flare_height` studs.
    pub flare: f32,
    pub flare_height: f32,
    /// Degrees it leans from upright, toward a random side.
    pub lean: f32,
    /// How far it wanders sideways (studs) and over how many studs of height
    /// one wave runs.
    pub wobble: f32,
    pub wobble_length: f32,
    /// Studs it starts under the ground, so it meets a slope without a gap.
    pub sink: f32,
    /// Root buttresses: how far each lobe stands out at the ground, as a
    /// share of the radius there, how many go round, and over how many
    /// studs of height they sink into the trunk. Only the trunk's base
    /// ([`MeshDetail::base_height`]) has them.
    pub buttress: f32,
    pub buttress_count: u32,
    pub buttress_height: f32,
    /// The bark's relief at the base: how deep its ridges are, as a share
    /// of the radius, and how many go round. Only the most detailed copy
    /// has it; the bark's normal map carries the rest.
    pub relief: f32,
    pub relief_ridges: u32,
    /// How much wider the trunk spreads under the ground, as a share of its
    /// radius at the ground, reached at the sunk foot: a skirt of root
    /// tendrils (the buttresses run on into it, narrower), so a tree on a
    /// slope shows no edge on its downhill side.
    pub root_spread: f32,
    /// Its top snapped off: splinters instead of a point (dead trees).
    pub broken: bool,
}

impl Default for Trunk {
    fn default() -> Self {
        Self {
            length: 30.0,
            radius: 1.1,
            tip_radius: 0.15,
            flare: 0.5,
            flare_height: 2.5,
            lean: 3.0,
            wobble: 0.6,
            wobble_length: 14.0,
            sink: 0.5,
            buttress: 0.4,
            buttress_count: 5,
            buttress_height: 3.0,
            relief: 0.035,
            relief_ridges: 14,
            root_spread: 0.4,
            broken: false,
        }
    }
}

/// One level of limbs, grown on each limb of the level before it (the first
/// level on the trunk).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BranchLevel {
    /// What the level is called in the editor ("Branches", "Twigs").
    pub name: String,
    /// How many grow on each parent limb.
    pub count: u32,
    /// Where along its parent they grow, from 0 (its base) to 1 (its tip).
    pub start: f32,
    pub end: f32,
    /// Degrees between a limb and its parent where it starts, give or take
    /// `angle_spread`.
    pub angle: f32,
    pub angle_spread: f32,
    /// Degrees each limb is turned round its parent from the one before:
    /// 137.5 (the golden angle) spreads them evenly, as many plants do.
    pub roll: f32,
    /// Its length, as a share of its parent's whole length, times
    /// `length_along` at the place it grows (shorter limbs higher up make a
    /// cone; longer ones a dome).
    pub length: f32,
    pub length_along: Curve,
    /// Its radius where it starts, as a share of its parent's there (never
    /// more than nine tenths of it), and at its tip as a share of its own
    /// start.
    pub radius: f32,
    pub tip_radius: f32,
    /// How much it bends down under its weight, and up toward the light,
    /// over its length (0 to about 2).
    pub gravity: f32,
    pub upward: f32,
    /// 0 to 1: how much it wanders as it grows.
    pub randomness: f32,
    /// Whether one of its limbs also grows from each parent's tip, carrying
    /// it on, so no limb ends bare in the air.
    pub terminal: bool,
    /// 0 to 1: the share of its limbs snapped off short, ending in
    /// splinters (snags on a dead tree).
    pub broken: f32,
}

impl Default for BranchLevel {
    fn default() -> Self {
        Self {
            name: "Branches".to_owned(),
            count: 10,
            start: 0.3,
            end: 0.95,
            angle: 55.0,
            angle_spread: 10.0,
            roll: 137.5,
            length: 0.45,
            length_along: Curve::new(&[[0.0, 1.0], [1.0, 0.35]]),
            radius: 0.55,
            tip_radius: 0.2,
            gravity: 0.3,
            upward: 0.2,
            randomness: 0.25,
            terminal: false,
            broken: 0.0,
        }
    }
}

/// How finely limbs are meshed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeshDetail {
    /// Studs between the rings along a limb (each limb has at least 3 and
    /// at most 32 segments).
    pub segment_length: f32,
    /// Corners around a ring per stud of its circumference, kept between
    /// `radial_min` and `radial_max`.
    pub radial_per_stud: f32,
    pub radial_min: u32,
    pub radial_max: u32,
    /// Limbs thinner than this (studs, at their start) aren't made, nor
    /// anything that would grow on them.
    pub min_radius: f32,
    /// How much wider a limb is where it leaves its parent (a collar), as a
    /// share of its radius, fading out over about one and a half radii.
    pub collar: f32,
    /// Studs of the trunk above the ground meshed finely, as its own
    /// primitive (`TrunkBase`): rings every `base_ring_spacing` studs and
    /// `base_radial` times the trunk's corners, for the buttresses and
    /// relief the eye sees up close. 0 meshes the base as the rest.
    pub base_height: f32,
    pub base_ring_spacing: f32,
    pub base_radial: u32,
    /// The most triangles the most detailed copy may have: thinner limbs
    /// are left out until it fits.
    pub triangles: u32,
    /// The simpler copies drawn farther away, each coarser than the one
    /// before (the renderer draws at most three).
    pub lods: Vec<LodDetail>,
}

impl Default for MeshDetail {
    fn default() -> Self {
        Self {
            segment_length: 1.0,
            radial_per_stud: 3.0,
            radial_min: 3,
            radial_max: 16,
            min_radius: 0.02,
            collar: 0.35,
            base_height: 8.0,
            base_ring_spacing: 0.3,
            base_radial: 4,
            triangles: 60_000,
            lods: vec![
                LodDetail {
                    segment_scale: 1.5,
                    radial_scale: 0.6,
                    min_radius: 0.03,
                    base_radial: 2,
                    triangles: 25_000,
                    leaves: 0.5,
                },
                LodDetail {
                    segment_scale: 2.5,
                    radial_scale: 0.4,
                    min_radius: 0.07,
                    base_radial: 1,
                    triangles: 8_000,
                    leaves: 0.25,
                },
                LodDetail {
                    segment_scale: 4.0,
                    radial_scale: 0.25,
                    min_radius: 0.18,
                    base_radial: 1,
                    triangles: 2_000,
                    leaves: 0.1,
                },
            ],
        }
    }
}

/// One simpler copy of the tree: the same limbs (none move, so nothing
/// jumps at the switch), with fewer rings and corners and the thin ones
/// left out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LodDetail {
    /// Its rings are at least this many times [`MeshDetail::segment_length`]
    /// apart, and its rings have this share of the corners.
    pub segment_scale: f32,
    pub radial_scale: f32,
    /// Limbs thinner than this (studs, at their start) are left out, with
    /// what grows on them.
    pub min_radius: f32,
    /// The trunk base's corners as a multiple of the trunk's (1: no
    /// buttresses); the relief is only on the most detailed copy.
    pub base_radial: u32,
    /// The most triangles it may have, leaves included.
    pub triangles: u32,
    /// The share of the leaf clusters it keeps (spread evenly over the
    /// crown, the kept ones grown to cover about as much), each leaf of two
    /// triangles instead of four.
    pub leaves: f32,
}

impl Default for LodDetail {
    fn default() -> Self {
        Self {
            segment_scale: 2.0,
            radial_scale: 0.5,
            min_radius: 0.1,
            base_radial: 1,
            triangles: 25_000,
            leaves: 0.5,
        }
    }
}

/// The leaves: clusters of a few leaves each along the outer parts of the
/// finest limbs, fewer where the crown shades itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Leaves {
    pub enabled: bool,
    /// Clusters per place on a limb (one place per ring of its outer part),
    /// on average.
    pub clusters: f32,
    /// Leaves in each cluster, fanned round its stalk.
    pub per_cluster: u32,
    /// A leaf's length in studs, give or take `size_jitter` (a share), and
    /// its width as a share of its length.
    pub size: f32,
    pub size_jitter: f32,
    pub width: f32,
    /// Degrees each leaf leans out from its cluster's stalk.
    pub spread: f32,
    /// How far a leaf's tip hangs down and its halves fold up along the
    /// middle, as shares of its length and width.
    pub droop: f32,
    pub fold: f32,
    /// 0 to 1: how far each cluster turns to face out of the crown.
    pub face_out: f32,
    /// 0 to 1: how many clusters deep in the crown's own shade are left
    /// out (where little light reaches, a real tree grows few leaves).
    pub shade_thinning: f32,
    /// Linear color.
    pub color: [f32; 3],
    pub roughness: f32,
    /// How much light passes through a leaf from behind.
    pub translucency: f32,
    /// How far (studs) each leaf shivers in the wind.
    pub flutter: f32,
    /// A picture of leaves each leaf is drawn with, cut out by its alpha,
    /// and its normal map (OpenGL-style), relative to the recipe's folder;
    /// empty draws plain green blades.
    pub texture: String,
    pub normal_map: String,
    /// Where each leaf is in `texture`, as `[u0, v0, u1, v1]` (0 to 1, V
    /// down), its stem at the bottom (or at the right, `stem_right`). Each
    /// leaf picks one.
    pub cells: Vec<[f32; 4]>,
    pub stem_right: bool,
}

impl Default for Leaves {
    fn default() -> Self {
        Self {
            enabled: true,
            clusters: 3.5,
            per_cluster: 6,
            size: 1.0,
            size_jitter: 0.25,
            width: 0.5,
            spread: 55.0,
            droop: 0.15,
            fold: 0.2,
            face_out: 0.5,
            shade_thinning: 0.6,
            color: [0.09, 0.2, 0.045],
            roughness: 0.6,
            translucency: 0.42,
            flutter: 0.06,
            texture: String::new(),
            normal_map: String::new(),
            cells: Vec::new(),
            stem_right: false,
        }
    }
}

impl Leaves {
    /// The cells of a picture of `columns` × `rows` leaves.
    pub fn grid(columns: u32, rows: u32) -> Vec<[f32; 4]> {
        let (columns, rows) = (columns.max(1), rows.max(1));
        (0..rows)
            .flat_map(|row| {
                (0..columns).map(move |column| {
                    let (w, h) = (1.0 / columns as f32, 1.0 / rows as f32);
                    [
                        column as f32 * w,
                        row as f32 * h,
                        (column + 1) as f32 * w,
                        (row + 1) as f32 * h,
                    ]
                })
            })
            .collect()
    }

    /// Whether leaves are cut out of a picture.
    pub fn textured(&self) -> bool {
        !self.texture.is_empty() && !self.cells.is_empty()
    }
}

/// The pictures that come with the engine for the presets, beside a
/// recipe in `textures/` (CC0, from ambientCG).
pub mod textures {
    pub const OAK_BARK: &str = "textures/bark_oak_color.jpg";
    pub const OAK_BARK_NORMAL: &str = "textures/bark_oak_normal.jpg";
    pub const PINE_BARK: &str = "textures/bark_pine_color.jpg";
    pub const PINE_BARK_NORMAL: &str = "textures/bark_pine_normal.jpg";
    pub const BIRCH_BARK: &str = "textures/bark_birch_color.jpg";
    pub const BIRCH_BARK_NORMAL: &str = "textures/bark_birch_normal.jpg";
    pub const BROAD_LEAVES: &str = "textures/leaves_broad.png";
    pub const BROAD_LEAVES_NORMAL: &str = "textures/leaves_broad_normal.jpg";
    pub const CONIFER: &str = "textures/leaves_conifer.png";
    pub const CONIFER_NORMAL: &str = "textures/leaves_conifer_normal.jpg";
    /// The three sprays in [`CONIFER`], small to large, their stems at the
    /// right.
    pub const CONIFER_SPRAYS: [[f32; 4]; 3] = [
        [0.285, 0.062, 0.715, 0.27],
        [0.078, 0.312, 0.898, 0.609],
        [0.039, 0.641, 0.934, 0.977],
    ];
}

/// The bark's look.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bark {
    /// Linear color, multiplied with the texture when there is one.
    pub color: [f32; 3],
    pub roughness: f32,
    /// Studs one copy of the bark's textures covers around a limb and along
    /// it.
    pub uv_tile: [f32; 2],
    /// The bark's color and normal maps (OpenGL-style, green up), relative
    /// to the recipe's folder; empty for none.
    pub texture: String,
    pub normal_map: String,
    /// Degrees the bark's grain turns round a limb per stud along it (a
    /// spiral), continuous along each limb.
    pub twist: f32,
    /// 0 to 1: how weathered (grey, stripped) the bark is on the highest,
    /// thinnest limbs, fading to none below `weathering_from` (a share of
    /// the tree's height). Carried per corner for the renderer.
    pub weathering: f32,
    pub weathering_from: f32,
}

impl Default for Bark {
    fn default() -> Self {
        Self {
            color: [0.24, 0.17, 0.11],
            roughness: 0.9,
            uv_tile: [4.0, 6.0],
            texture: String::new(),
            normal_map: String::new(),
            twist: 0.0,
            weathering: 0.0,
            weathering_from: 0.5,
        }
    }
}

/// A value over 0 to 1, through points joined by straight lines: SpeedTree
/// and Unity call these curves. Before the first point and after the last
/// it holds their values; with no points it is 1 everywhere.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct Curve {
    /// `[x, value]` pairs, sorted by `x` when read.
    pub points: Vec<[f32; 2]>,
}

impl Curve {
    pub fn new(points: &[[f32; 2]]) -> Self {
        Self {
            points: points.to_vec(),
        }
    }

    pub fn at(&self, x: f32) -> f32 {
        let mut points: Vec<[f32; 2]> = self
            .points
            .iter()
            .copied()
            .filter(|[x, y]| x.is_finite() && y.is_finite())
            .collect();
        points.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let (Some(first), Some(last)) = (points.first(), points.last()) else {
            return 1.0;
        };
        if x <= first[0] {
            return first[1];
        }
        if x >= last[0] {
            return last[1];
        }
        for pair in points.windows(2) {
            let ([x0, y0], [x1, y1]) = (pair[0], pair[1]);
            if x <= x1 {
                let t = if x1 > x0 { (x - x0) / (x1 - x0) } else { 1.0 };
                return y0 + (y1 - y0) * t;
            }
        }
        last[1]
    }
}

/// The recipes the editor starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// A broad deciduous tree: a short trunk, long spreading limbs.
    Oak,
    /// A conifer: one straight trunk to the top, short level whorls.
    Pine,
    /// A slender tree with fine, drooping twigs.
    Birch,
    /// A bare, broken tree.
    Dead,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Oak, Preset::Pine, Preset::Birch, Preset::Dead];

    pub fn name(self) -> &'static str {
        match self {
            Preset::Oak => "Oak",
            Preset::Pine => "Pine",
            Preset::Birch => "Birch",
            Preset::Dead => "Dead Tree",
        }
    }
}

impl TreeRecipe {
    pub fn preset(preset: Preset) -> Self {
        let level = BranchLevel::default();
        let twigs = BranchLevel {
            name: "Twigs".to_owned(),
            count: 7,
            start: 0.2,
            end: 1.0,
            angle: 45.0,
            angle_spread: 15.0,
            length: 0.35,
            length_along: Curve::new(&[[0.0, 1.0], [1.0, 0.5]]),
            radius: 0.5,
            tip_radius: 0.3,
            gravity: 0.15,
            upward: 0.25,
            randomness: 0.4,
            ..level.clone()
        };
        let base = TreeRecipe {
            format: TREE_FORMAT,
            name: preset.name().to_owned(),
            seed: 1,
            trunk: Trunk::default(),
            levels: Vec::new(),
            pipe: 0.6,
            mesh: MeshDetail::default(),
            bark: Bark::default(),
            leaves: Leaves::default(),
        };
        match preset {
            Preset::Oak => TreeRecipe {
                trunk: Trunk {
                    length: 26.0,
                    radius: 1.3,
                    tip_radius: 0.2,
                    lean: 4.0,
                    ..Trunk::default()
                },
                levels: vec![
                    BranchLevel {
                        count: 9,
                        start: 0.32,
                        angle: 58.0,
                        length: 0.6,
                        length_along: Curve::new(&[[0.0, 1.0], [0.6, 0.85], [1.0, 0.4]]),
                        gravity: 0.35,
                        upward: 0.3,
                        ..level.clone()
                    },
                    BranchLevel {
                        name: "Limbs".to_owned(),
                        count: 6,
                        angle: 50.0,
                        length: 0.5,
                        radius: 0.55,
                        tip_radius: 0.25,
                        gravity: 0.25,
                        upward: 0.2,
                        randomness: 0.35,
                        ..twigs.clone()
                    },
                    BranchLevel {
                        count: 4,
                        length: 0.4,
                        terminal: true,
                        ..twigs.clone()
                    },
                ],
                bark: Bark {
                    color: [0.85, 0.8, 0.72],
                    uv_tile: [4.0, 8.0],
                    texture: textures::OAK_BARK.to_owned(),
                    normal_map: textures::OAK_BARK_NORMAL.to_owned(),
                    twist: 1.5,
                    weathering: 0.2,
                    ..Bark::default()
                },
                leaves: Leaves {
                    per_cluster: 11,
                    size: 0.6,
                    spread: 70.0,
                    fold: 0.1,
                    color: [0.85, 0.95, 0.8],
                    texture: textures::BROAD_LEAVES.to_owned(),
                    normal_map: textures::BROAD_LEAVES_NORMAL.to_owned(),
                    cells: Leaves::grid(3, 3),
                    ..Leaves::default()
                },
                ..base
            },
            Preset::Pine => TreeRecipe {
                trunk: Trunk {
                    length: 38.0,
                    radius: 0.9,
                    tip_radius: 0.05,
                    flare: 0.35,
                    lean: 1.5,
                    wobble: 0.2,
                    buttress: 0.15,
                    relief: 0.05,
                    relief_ridges: 18,
                    ..Trunk::default()
                },
                levels: vec![
                    BranchLevel {
                        count: 26,
                        start: 0.25,
                        end: 0.98,
                        angle: 82.0,
                        angle_spread: 6.0,
                        length: 0.3,
                        length_along: Curve::new(&[[0.0, 1.0], [1.0, 0.15]]),
                        radius: 0.35,
                        gravity: 0.45,
                        upward: 0.05,
                        randomness: 0.1,
                        ..level.clone()
                    },
                    BranchLevel {
                        count: 6,
                        angle: 60.0,
                        length: 0.3,
                        gravity: 0.1,
                        upward: 0.0,
                        terminal: true,
                        ..twigs.clone()
                    },
                ],
                pipe: 0.3,
                bark: Bark {
                    color: [1.0, 1.0, 1.0],
                    uv_tile: [3.0, 6.0],
                    texture: textures::PINE_BARK.to_owned(),
                    normal_map: textures::PINE_BARK_NORMAL.to_owned(),
                    twist: 2.0,
                    ..Bark::default()
                },
                leaves: Leaves {
                    clusters: 3.0,
                    per_cluster: 3,
                    size: 2.2,
                    size_jitter: 0.2,
                    spread: 35.0,
                    droop: 0.1,
                    fold: 0.05,
                    face_out: 0.3,
                    color: [0.9, 1.0, 0.9],
                    texture: textures::CONIFER.to_owned(),
                    normal_map: textures::CONIFER_NORMAL.to_owned(),
                    cells: textures::CONIFER_SPRAYS.to_vec(),
                    stem_right: true,
                    ..Leaves::default()
                },
                ..base
            },
            Preset::Birch => TreeRecipe {
                trunk: Trunk {
                    length: 30.0,
                    radius: 0.55,
                    tip_radius: 0.1,
                    flare: 0.25,
                    lean: 5.0,
                    wobble: 0.8,
                    buttress: 0.05,
                    relief: 0.015,
                    ..Trunk::default()
                },
                levels: vec![
                    BranchLevel {
                        count: 12,
                        start: 0.35,
                        angle: 40.0,
                        length: 0.4,
                        radius: 0.45,
                        gravity: 0.2,
                        upward: 0.35,
                        ..level.clone()
                    },
                    BranchLevel {
                        count: 8,
                        gravity: 0.9,
                        upward: 0.0,
                        ..twigs.clone()
                    },
                    BranchLevel {
                        name: "Fine twigs".to_owned(),
                        count: 3,
                        start: 0.3,
                        angle: 35.0,
                        length: 0.35,
                        gravity: 1.2,
                        upward: 0.0,
                        terminal: true,
                        ..twigs.clone()
                    },
                ],
                bark: Bark {
                    color: [1.0, 1.0, 0.95],
                    roughness: 0.8,
                    uv_tile: [3.0, 4.0],
                    texture: textures::BIRCH_BARK.to_owned(),
                    normal_map: textures::BIRCH_BARK_NORMAL.to_owned(),
                    ..Bark::default()
                },
                leaves: Leaves {
                    clusters: 5.0,
                    per_cluster: 10,
                    size: 0.42,
                    spread: 70.0,
                    droop: 0.25,
                    fold: 0.1,
                    color: [0.95, 1.0, 0.85],
                    flutter: 0.09,
                    texture: textures::BROAD_LEAVES.to_owned(),
                    normal_map: textures::BROAD_LEAVES_NORMAL.to_owned(),
                    cells: Leaves::grid(3, 3),
                    ..Leaves::default()
                },
                mesh: MeshDetail {
                    min_radius: 0.012,
                    ..MeshDetail::default()
                },
                ..base
            },
            Preset::Dead => TreeRecipe {
                trunk: Trunk {
                    length: 22.0,
                    radius: 1.0,
                    tip_radius: 0.35,
                    lean: 8.0,
                    wobble: 1.0,
                    buttress: 0.4,
                    buttress_count: 4,
                    relief: 0.06,
                    relief_ridges: 10,
                    broken: true,
                    ..Trunk::default()
                },
                levels: vec![
                    BranchLevel {
                        count: 14,
                        start: 0.25,
                        angle: 50.0,
                        angle_spread: 20.0,
                        length: 0.35,
                        tip_radius: 0.4,
                        gravity: 0.1,
                        randomness: 0.5,
                        broken: 0.5,
                        ..level.clone()
                    },
                    BranchLevel {
                        count: 7,
                        tip_radius: 0.5,
                        randomness: 0.6,
                        broken: 0.5,
                        ..twigs
                    },
                ],
                bark: Bark {
                    color: [0.75, 0.72, 0.7],
                    uv_tile: [4.0, 8.0],
                    texture: textures::OAK_BARK.to_owned(),
                    normal_map: textures::OAK_BARK_NORMAL.to_owned(),
                    twist: 3.0,
                    weathering: 0.9,
                    weathering_from: 0.25,
                    ..Bark::default()
                },
                leaves: Leaves {
                    enabled: false,
                    ..Leaves::default()
                },
                // Bare limbs are seen whole: rounder twigs, its triangles
                // spent on wood.
                mesh: MeshDetail {
                    radial_min: 4,
                    ..MeshDetail::default()
                },
                ..base
            },
        }
    }

    /// Reads a recipe from its file's text.
    pub fn from_json(text: &str) -> Result<TreeRecipe, String> {
        let recipe: TreeRecipe =
            serde_json::from_str(text).map_err(|error| format!("isn't a tree recipe: {error}"))?;
        if recipe.format > TREE_FORMAT {
            return Err(format!(
                "was written by a newer engine (format {}, this one reads {TREE_FORMAT})",
                recipe.format
            ));
        }
        Ok(recipe)
    }

    /// The recipe as its file's text.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a recipe is plain data")
    }
}
