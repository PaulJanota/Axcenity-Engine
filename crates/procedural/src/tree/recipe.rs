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
}

impl Default for MeshDetail {
    fn default() -> Self {
        Self {
            segment_length: 1.5,
            radial_per_stud: 3.0,
            radial_min: 3,
            radial_max: 16,
            min_radius: 0.02,
            collar: 0.35,
        }
    }
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
}

impl Default for Bark {
    fn default() -> Self {
        Self {
            color: [0.24, 0.17, 0.11],
            roughness: 0.9,
            uv_tile: [4.0, 6.0],
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
                        ..twigs.clone()
                    },
                ],
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
                        ..twigs.clone()
                    },
                ],
                pipe: 0.3,
                bark: Bark {
                    color: [0.21, 0.13, 0.09],
                    ..Bark::default()
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
                ],
                bark: Bark {
                    color: [0.72, 0.7, 0.66],
                    roughness: 0.8,
                    ..Bark::default()
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
                    ..Trunk::default()
                },
                levels: vec![
                    BranchLevel {
                        count: 6,
                        start: 0.4,
                        angle: 50.0,
                        angle_spread: 20.0,
                        length: 0.35,
                        tip_radius: 0.4,
                        gravity: 0.1,
                        randomness: 0.5,
                        ..level.clone()
                    },
                    BranchLevel {
                        count: 3,
                        tip_radius: 0.5,
                        randomness: 0.6,
                        ..twigs
                    },
                ],
                bark: Bark {
                    color: [0.3, 0.27, 0.24],
                    ..Bark::default()
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
