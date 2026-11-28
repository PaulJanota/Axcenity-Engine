use glam::Vec3;

use super::recipe::{BranchLevel, Curve, Preset, TREE_FORMAT, TreeRecipe};
use super::{generate, mesher};

fn every_preset() -> impl Iterator<Item = TreeRecipe> {
    Preset::ALL.into_iter().map(TreeRecipe::preset)
}

#[test]
fn the_same_recipe_and_variant_give_the_same_tree() {
    for recipe in every_preset() {
        let a = generate(&recipe, 3);
        let b = generate(&recipe, 3);
        assert_eq!(a.levels.len(), b.levels.len());
        for (a, b) in a.levels.iter().zip(&b.levels) {
            for (a, b) in [(&a.base, &b.base), (&a.bark, &b.bark)] {
                assert_eq!(a.positions, b.positions, "{}", recipe.name);
                assert_eq!(a.indices, b.indices, "{}", recipe.name);
            }
        }
    }
}

#[test]
fn another_variant_is_another_tree() {
    let recipe = TreeRecipe::preset(Preset::Oak);
    let a = generate(&recipe, 0);
    let b = generate(&recipe, 1);
    assert_ne!(a.full().bark.positions, b.full().bark.positions);
}

#[test]
fn a_recipe_reads_back_as_written_and_fills_what_a_file_leaves_out() {
    for recipe in every_preset() {
        let read = TreeRecipe::from_json(&recipe.to_json()).unwrap();
        assert_eq!(read, recipe);
    }
    // An older or hand-written file: only a name and a trunk length, and a
    // field this engine doesn't know.
    let read =
        TreeRecipe::from_json(r#"{"name": "Mine", "trunk": {"length": 12}, "future": 1}"#).unwrap();
    assert_eq!(read.name, "Mine");
    assert_eq!(read.trunk.length, 12.0);
    assert_eq!(read.trunk.radius, super::Trunk::default().radius);
    // A newer format is refused, not misread.
    let newer = format!(r#"{{"format": {}}}"#, TREE_FORMAT + 1);
    assert!(TreeRecipe::from_json(&newer).is_err());
    assert!(TreeRecipe::from_json("not json").is_err());
}

#[test]
fn a_curve_runs_through_its_points_and_holds_its_ends() {
    let curve = Curve::new(&[[1.0, 0.0], [0.0, 1.0], [0.5, 0.8]]);
    assert_eq!(curve.at(-1.0), 1.0);
    assert_eq!(curve.at(0.0), 1.0);
    assert!((curve.at(0.25) - 0.9).abs() < 1e-6);
    assert!((curve.at(0.75) - 0.4).abs() < 1e-6);
    assert_eq!(curve.at(2.0), 0.0);
    assert_eq!(Curve::default().at(0.3), 1.0);
}

#[test]
fn every_branch_starts_inside_the_limb_it_grows_on() {
    for recipe in every_preset() {
        let tree = generate(&recipe, 2);
        let stems = &tree.skeleton.stems;
        assert!(
            stems.len() > 10,
            "{} grew {} limbs",
            recipe.name,
            stems.len()
        );
        for stem in stems.iter().skip(1) {
            let (parent, t) = stem.parent.expect("a branch has a parent");
            assert!(parent < stems.len() && stems[parent].level + 1 == stem.level);
            let (on, _, radius) = stems[parent].at(t);
            assert!(stem.points[0].distance(on) <= radius + 1e-4);
            // Never thicker than the limb it grows on.
            assert!(stem.radii[0] <= radius + 1e-4, "{}", recipe.name);
        }
    }
}

#[test]
fn branches_leave_their_parent_at_about_their_angle() {
    let mut recipe = TreeRecipe::preset(Preset::Oak);
    recipe.levels = vec![BranchLevel {
        angle: 60.0,
        angle_spread: 0.0,
        gravity: 0.0,
        upward: 0.0,
        randomness: 0.0,
        ..BranchLevel::default()
    }];
    let tree = generate(&recipe, 0);
    let stems = &tree.skeleton.stems;
    for stem in &stems[1..] {
        let (parent, t) = stem.parent.unwrap();
        let (_, along, _) = stems[parent].at(t);
        let angle = stem.direction(0).angle_between(along).to_degrees();
        assert!((angle - 60.0).abs() < 1.0, "{angle}");
    }
}

#[test]
fn the_pipe_model_thickens_the_trunk_under_its_limbs() {
    let mut recipe = TreeRecipe::preset(Preset::Oak);
    recipe.pipe = 0.0;
    let thin = generate(&recipe, 0);
    recipe.pipe = 1.0;
    let thick = generate(&recipe, 0);
    let trunk = |tree: &super::Tree| tree.skeleton.stems[0].at(0.5).2;
    assert!(trunk(&thick) > trunk(&thin));
    // Where nothing grows beyond, a limb keeps its own taper.
    let tip = |tree: &super::Tree| *tree.skeleton.stems[0].radii.last().unwrap();
    assert!((tip(&thick) - tip(&thin)).abs() < 1e-5);
}

#[test]
fn thin_limbs_are_left_out_with_what_would_grow_on_them() {
    let mut recipe = TreeRecipe::preset(Preset::Oak);
    let all = generate(&recipe, 0).skeleton.stems.len();
    recipe.mesh.min_radius = 0.2;
    let some = generate(&recipe, 0);
    assert!(some.skeleton.stems.len() < all);
    assert!(some.skeleton.stems.iter().all(|s| s.radii[0] >= 0.2));
}

#[test]
fn the_bark_is_a_clean_mesh_facing_out() {
    for recipe in every_preset() {
        for variant in 0..6 {
            let tree = generate(&recipe, variant);
            for level in &tree.levels {
                assert!(!level.base.indices.is_empty(), "{}", recipe.name);
                let name = format!("{} {variant}", recipe.name);
                clean_and_facing_out(&level.base, &format!("{name} base"));
                clean_and_facing_out(&level.bark, &format!("{name} bark"));
            }
        }
    }
}

fn clean_and_facing_out(mesh: &mesher::TreeMesh, name: &str) {
    {
        let n = mesh.positions.len();
        assert_eq!(mesh.normals.len(), n);
        assert_eq!(mesh.uvs.len(), n);
        assert_eq!(mesh.tangents.len(), n);
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(mesh.indices.iter().all(|&i| (i as usize) < n));
        for (p, normal) in mesh.positions.iter().zip(&mesh.normals) {
            assert!(p.iter().all(|v| v.is_finite()), "{name}");
            let length = Vec3::from(*normal).length();
            assert!((length - 1.0).abs() < 1e-3, "{name}: {length}");
        }
        // Every triangle's winding agrees with its corners' normals (it
        // faces out), but for the ones too small to have a direction.
        let mut against = Vec::new();
        for triangle in mesh.indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(mesh.positions[triangle[i] as usize]));
            let face = (b - a).cross(c - a);
            if face.length() < 1e-7 {
                continue;
            }
            let normal: Vec3 = triangle
                .iter()
                .map(|&i| Vec3::from(mesh.normals[i as usize]))
                .sum();
            if face.dot(normal) < 0.0 {
                against.push((triangle.to_vec(), [a, b, c]));
            }
        }
        assert!(
            against.is_empty(),
            "{name}: faces against their normals {against:?}"
        );
    }
}

#[test]
fn the_bark_texture_runs_on_along_each_limb_and_from_its_parent() {
    let recipe = TreeRecipe::preset(Preset::Oak);
    let tree = generate(&recipe, 0);
    let stems = &tree.skeleton.stems;
    for stem in &stems[1..] {
        let (parent, t) = stem.parent.unwrap();
        let expected = stems[parent].v_start + stems[parent].length() * t;
        assert!((stem.v_start - expected).abs() < 1e-3);
        assert!(stem.along.windows(2).all(|w| w[1] >= w[0]));
    }
    // Around a limb, U makes whole repeats, so its seam meets itself.
    let corners = mesher::ring_corners(&recipe, stems[0].radii[0]) as usize;
    // (The grain's spiral shifts a whole ring alike.)
    let uvs = &tree.full().bark.uvs;
    let around = uvs[corners][0] - uvs[0][0];
    assert!(around >= 1.0 && (around - around.round()).abs() < 1e-4);
}

#[test]
fn rings_have_as_many_corners_as_their_thickness_needs_within_the_limits() {
    let recipe = TreeRecipe::preset(Preset::Oak);
    assert_eq!(mesher::ring_corners(&recipe, 0.001), recipe.mesh.radial_min);
    assert_eq!(mesher::ring_corners(&recipe, 50.0), recipe.mesh.radial_max);
    let middle = mesher::ring_corners(&recipe, 0.3);
    assert!(middle > recipe.mesh.radial_min && middle < recipe.mesh.radial_max);
}

#[test]
fn a_tree_is_a_model_as_tall_as_it_was_grown() {
    for recipe in every_preset() {
        let tree = generate(&recipe, 0);
        let model = tree.to_model(&recipe).unwrap();
        let names: Vec<&str> = model
            .primitives
            .iter()
            .map(|p| p.material.name.as_str())
            .collect();
        if recipe.leaves.enabled {
            assert_eq!(names, ["TrunkBase", "Bark", "Leaves"]);
            let leaves = &model.primitives[2].material;
            assert!(leaves.double_sided && leaves.translucency > 0.0 && leaves.flutter > 0.0);
        } else {
            assert_eq!(names, ["TrunkBase", "Bark"]);
        }
        for primitive in &model.primitives {
            // The base sways with the rest, so they never part.
            assert_eq!(primitive.material.sway, model.primitives[1].material.sway);
            assert!(primitive.material.sway > 0.0);
            assert_eq!(primitive.lods.len(), tree.levels.len() - 1);
            // Fitted as the full model is: about its 1×1×1 box (a simpler
            // copy's grown leaves may reach a little past it).
            for copy in &primitive.lods {
                let far = copy
                    .positions
                    .iter()
                    .flatten()
                    .fold(0.0f32, |m, v| m.max(v.abs()));
                assert!(far <= 0.65, "{}: {far}", recipe.name);
            }
        }
        // The trunk's length and sunk foot, give or take its lean and the
        // limbs that reach above it.
        let tall = recipe.trunk.length + recipe.trunk.sink;
        assert!(
            model.size[1] > tall * 0.85 && model.size[1] < tall * 1.6,
            "{}: {:?}",
            recipe.name,
            model.size
        );
    }
}

#[test]
fn every_copy_is_simpler_than_the_last_and_within_its_budget() {
    for recipe in every_preset() {
        let tree = generate(&recipe, 0);
        assert_eq!(tree.levels.len(), 4, "{}", recipe.name);
        let budgets = std::iter::once(recipe.mesh.triangles)
            .chain(recipe.mesh.lods.iter().map(|lod| lod.triangles));
        let mut before = usize::MAX;
        for (level, budget) in tree.levels.iter().zip(budgets) {
            let triangles = level.triangles();
            assert!(triangles <= budget as usize, "{}: {triangles}", recipe.name);
            assert!(triangles < before, "{}: {triangles}", recipe.name);
            before = triangles;
        }
        assert!(tree.triangles() > 500, "{}", recipe.name);
    }
    // A tight budget leaves the thin limbs out, the trunk always in.
    let mut recipe = TreeRecipe::preset(Preset::Oak);
    let full = generate(&recipe, 0).triangles();
    recipe.mesh.triangles = (full / 3) as u32;
    let tight = generate(&recipe, 0);
    assert!(tight.triangles() <= full / 3);
    assert!(!tight.full().bark.indices.is_empty());
}

#[test]
fn simpler_copies_keep_the_limbs_where_they_are() {
    let tree = generate(&TreeRecipe::preset(Preset::Oak), 0);
    let bounds = |level: &super::TreeLevel| {
        level
            .base
            .positions
            .iter()
            .chain(&level.bark.positions)
            .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
            })
    };
    let (low, high) = bounds(tree.full());
    for level in &tree.levels[1..] {
        let (lo, hi) = bounds(level);
        assert!(lo.cmpge(low - 0.05).all() && hi.cmple(high + 0.05).all());
        // The trunk's top is the same point in every copy.
        let tip = |l: &super::TreeLevel| l.bark.positions[trunk_tip(&tree, l)];
        assert_eq!(tip(level), tip(tree.full()));
    }
}

/// Where the trunk's tip point is in a copy's bark: right after its rings.
fn trunk_tip(tree: &super::Tree, level: &super::TreeLevel) -> usize {
    // The trunk's tube comes first; its tip is the first corner with the
    // tip's own normal, straight along the trunk.
    let along = tree.skeleton.stems[0].direction(tree.skeleton.stems[0].points.len() - 1);
    level
        .bark
        .normals
        .iter()
        .position(|n| Vec3::from(*n).distance(along) < 1e-6)
        .unwrap()
}

#[test]
fn the_trunk_base_meets_the_trunk_without_a_gap() {
    for recipe in every_preset() {
        let tree = generate(&recipe, 0);
        for level in &tree.levels {
            let trunk = mesher::ring_corners(&recipe, tree.skeleton.stems[0].radii[0]);
            // The trunk's corners in this copy: its first ring's U reaches
            // a whole number again at the last corner.
            let first_ring: Vec<Vec3> = level
                .bark
                .positions
                .iter()
                .zip(&level.bark.uvs)
                .take_while(|(_, uv)| uv[1] == level.bark.uvs[0][1])
                .map(|(p, _)| Vec3::from(*p))
                .collect();
            let n = first_ring.len() - 1;
            assert!(n as u32 <= trunk);
            let base = &level.base.positions;
            let top_count = base
                .iter()
                .rev()
                .zip(level.base.uvs.iter().rev())
                .take_while(|(_, uv)| uv[1] == level.base.uvs.last().unwrap()[1])
                .count();
            let top: Vec<Vec3> = base[base.len() - top_count..]
                .iter()
                .map(|p| Vec3::from(*p))
                .collect();
            let times = (top.len() - 1) / n;
            assert_eq!((top.len() - 1) % n, 0, "{}", recipe.name);
            for (j, point) in top.iter().enumerate() {
                let low = first_ring[j / times];
                let high = first_ring[(j / times + 1).min(n)];
                let f = (j % times) as f32 / times as f32;
                let on = low.lerp(high, f);
                assert!(point.distance(on) < 1e-4, "{}: {j}", recipe.name);
            }
        }
    }
}

#[test]
fn buttresses_swell_the_base_only_where_it_is_detailed() {
    let recipe = TreeRecipe::preset(Preset::Oak);
    let tree = generate(&recipe, 0);
    let trunk = &tree.skeleton.stems[0];
    // How far round the ground's ring the radius swings, as a share.
    let swing = |level: &super::TreeLevel| {
        let ring = level
            .base
            .uvs
            .iter()
            .take_while(|uv| uv[1] == level.base.uvs[0][1])
            .count();
        let center = trunk.points[0];
        let radii: Vec<f32> = level.base.positions[..ring]
            .iter()
            .map(|p| {
                (Vec3::from(*p) - center)
                    .reject_from(trunk.direction(0))
                    .length()
            })
            .collect();
        let max = radii.iter().copied().fold(0.0, f32::max);
        let min = radii.iter().copied().fold(f32::INFINITY, f32::min);
        max / min - 1.0
    };
    assert!(
        swing(tree.full()) > recipe.trunk.buttress * 0.6,
        "{}",
        swing(tree.full())
    );
    // The coarsest copies' bases are plain round tubes.
    let last = tree.levels.last().unwrap();
    assert!(swing(last) < 0.02, "{}", swing(last));
}

#[test]
fn leafy_trees_have_leaves_in_every_copy_with_their_crown_data() {
    for recipe in every_preset().filter(|r| r.leaves.enabled) {
        let tree = generate(&recipe, 0);
        let full = &tree.full().leaves;
        assert!(
            full.triangles() > 1000,
            "{}: {}",
            recipe.name,
            full.triangles()
        );
        assert_eq!(
            tree.leaf_clusters * recipe.leaves.per_cluster as usize * 4,
            full.triangles()
        );
        for level in &tree.levels {
            let leaves = &level.leaves;
            assert_eq!(leaves.crown.len(), leaves.positions.len());
            for crown in &leaves.crown {
                let out = Vec3::new(crown[0], crown[1], crown[2]);
                assert!((out.length() - 1.0).abs() < 1e-3);
                assert!((0.0..=1.0).contains(&crown[3]));
            }
            assert!(level.bark.crown.is_empty() && level.base.crown.is_empty());
            if !leaves.indices.is_empty() {
                clean_and_facing_out(leaves, &recipe.name);
            }
        }
        // A simpler copy's leaves are the first clusters' (same places):
        // its first card's base (between its first and last corners) is
        // where the full tree's first leaf starts, give or take its fold.
        let lod1 = &tree.levels[1].leaves;
        assert!(lod1.triangles() < full.triangles());
        let corner = |mesh: &mesher::TreeMesh, i: usize| Vec3::from(mesh.positions[i]);
        let base = (corner(lod1, 0) + corner(lod1, 3)) / 2.0;
        let full_base = if recipe.leaves.textured() {
            (corner(full, 0) + corner(full, 5)) / 2.0
        } else {
            corner(full, 0)
        };
        let gap = base.distance(full_base);
        assert!(gap < recipe.leaves.size * 0.2, "{}: {gap}", recipe.name);
    }
    let dead = generate(&TreeRecipe::preset(Preset::Dead), 0);
    assert!(dead.levels.iter().all(|l| l.leaves.indices.is_empty()));
}

#[test]
fn the_crown_shades_itself_and_thins_where_dark() {
    let mut recipe = TreeRecipe::preset(Preset::Oak);
    // Few leaves a cluster, so neither crown is cut to the triangle budget.
    recipe.leaves.per_cluster = 3;
    recipe.leaves.shade_thinning = 0.0;
    let dense = generate(&recipe, 0);
    recipe.leaves.shade_thinning = 1.0;
    let thinned = generate(&recipe, 0);
    assert!(thinned.leaf_clusters < dense.leaf_clusters);
    // Low in the crown is darker than its top.
    let leaves = &dense.full().leaves;
    let crown = dense.crown;
    let (mut low, mut high) = ((0.0, 0), (0.0, 0));
    for (p, c) in leaves.positions.iter().zip(&leaves.crown) {
        let y = p[1] - crown.center.y;
        if y < -crown.half_size.y * 0.3 {
            low = (low.0 + c[3], low.1 + 1);
        } else if y > crown.half_size.y * 0.3 {
            high = (high.0 + c[3], high.1 + 1);
        }
    }
    assert!(low.1 > 0 && high.1 > 0);
    assert!(low.0 / low.1 as f32 + 0.1 < high.0 / high.1 as f32);
}

#[test]
fn any_first_few_leaf_clusters_spread_over_the_whole_crown() {
    let recipe = TreeRecipe::preset(Preset::Oak);
    let tree = generate(&recipe, 0);
    let leaves = &tree.full().leaves;
    let per_cluster = recipe.leaves.per_cluster as usize * 6;
    let bounds = |corners: &[[f32; 3]]| {
        corners
            .iter()
            .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
            })
    };
    let (lo, hi) = bounds(&leaves.positions);
    let first = tree.leaf_clusters / 10 * per_cluster;
    let (flo, fhi) = bounds(&leaves.positions[..first]);
    let share = (fhi - flo) / (hi - lo);
    assert!(share.min_element() > 0.6, "{share}");
}

#[test]
fn leaves_have_places_on_the_outer_half_of_the_finest_limbs() {
    let recipe = TreeRecipe::preset(Preset::Oak);
    let tree = generate(&recipe, 0);
    assert!(!tree.leaf_anchors.is_empty());
    let deepest = tree.skeleton.stems.iter().map(|s| s.level).max().unwrap();
    assert_eq!(deepest, recipe.levels.len());
    assert!(
        tree.leaf_anchors
            .iter()
            .all(|a| a.position.is_finite() && (a.direction.length() - 1.0).abs() < 1e-3)
    );
}

/// Writes each preset's bark as an OBJ into `AXCENITY_TREE_OBJ` (a folder),
/// to look at outside the engine. `cargo test -p axcenity-procedural
/// write_preset_trees -- --ignored`.
#[test]
#[ignore]
fn write_preset_trees() {
    let Ok(folder) = std::env::var("AXCENITY_TREE_OBJ") else {
        return;
    };
    for recipe in every_preset() {
        let tree = generate(&recipe, 0);
        for (index, level) in tree.levels.iter().enumerate() {
            let mut obj = String::new();
            let mut offset = 1;
            for (name, mesh) in [
                ("TrunkBase", &level.base),
                ("Bark", &level.bark),
                ("Leaves", &level.leaves),
            ] {
                obj.push_str(&format!("o {name}\n"));
                for p in &mesh.positions {
                    obj.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
                }
                for n in &mesh.normals {
                    obj.push_str(&format!("vn {} {} {}\n", n[0], n[1], n[2]));
                }
                for uv in &mesh.uvs {
                    obj.push_str(&format!("vt {} {}\n", uv[0], 1.0 - uv[1]));
                }
                for t in mesh.indices.chunks(3) {
                    let [a, b, c] = [t[0] + offset, t[1] + offset, t[2] + offset];
                    obj.push_str(&format!("f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}\n"));
                }
                offset += mesh.positions.len() as u32;
            }
            let path =
                std::path::Path::new(&folder).join(format!("{}_LOD{index}.obj", recipe.name));
            std::fs::write(&path, obj).unwrap();
            println!(
                "{} LOD{index}: {} triangles (base {})",
                recipe.name,
                level.triangles(),
                level.base.triangles()
            );
        }
        println!("{}: {} limbs", recipe.name, tree.skeleton.stems.len());
    }
}

#[test]
fn textured_leaves_are_cards_showing_one_leaf_of_their_picture_each() {
    for recipe in every_preset().filter(|r| r.leaves.enabled) {
        assert!(recipe.leaves.textured(), "{}", recipe.name);
        let tree = generate(&recipe, 0);
        let materials = super::Tree::textures(&recipe);
        let leaves = materials.iter().find(|m| m.material == "Leaves").unwrap();
        assert_eq!(
            leaves.color.as_deref(),
            Some(recipe.leaves.texture.as_str())
        );
        for level in &tree.levels {
            let mesh = &level.leaves;
            // Six corners a card when detailed (folded along its midrib),
            // four in the simpler copies.
            let corners = match mesh.positions.len() * 2 / mesh.triangles().max(1) {
                3 => 6,
                4 => 4,
                ratio => panic!("{}: {ratio}", recipe.name),
            };
            for card in mesh.uvs.chunks(corners) {
                let inside = |cell: &[f32; 4]| {
                    card.iter().all(|uv| {
                        (cell[0] - 1e-4..=cell[2] + 1e-4).contains(&uv[0])
                            && (cell[1] - 1e-4..=cell[3] + 1e-4).contains(&uv[1])
                    })
                };
                assert!(
                    recipe.leaves.cells.iter().any(inside),
                    "{}: {card:?}",
                    recipe.name
                );
            }
        }
    }
    let plain = TreeRecipe::preset(Preset::Dead);
    assert!(
        super::Tree::textures(&plain)
            .iter()
            .all(|m| m.material != "Leaves")
    );
}

/// Prints each preset's limbs per level and triangles per copy, to check
/// budgets: `cargo test -p axcenity-procedural print_tree_stats -- --ignored
/// --nocapture`.
#[test]
#[ignore]
fn print_tree_stats() {
    for recipe in every_preset() {
        let tree = generate(&recipe, 0);
        let mut per_level = [0usize; 6];
        for stem in &tree.skeleton.stems {
            per_level[stem.level] += 1;
        }
        println!(
            "{}: limbs per level {:?}",
            recipe.name,
            &per_level[..=recipe.levels.len()]
        );
        for (index, level) in tree.levels.iter().enumerate() {
            println!(
                "  LOD{index}: base {} bark {} leaves {} = {}",
                level.base.triangles(),
                level.bark.triangles(),
                level.leaves.triangles(),
                level.triangles()
            );
        }
    }
}

#[test]
fn side_limbs_are_welded_onto_their_parents() {
    for recipe in every_preset() {
        let tree = generate(&recipe, 0);
        // (A slender birch's limbs are all fine twigs.)
        if let Some(share) = mesher::welded_share(&recipe, &tree.skeleton) {
            assert!(share > 0.6, "{}: {share}", recipe.name);
        }
    }
}

#[test]
fn limbs_carry_on_into_terminal_twigs() {
    let recipe = TreeRecipe::preset(Preset::Oak);
    assert!(recipe.levels[2].terminal);
    let tree = generate(&recipe, 0);
    let stems = &tree.skeleton.stems;
    // Every limb of the level before whose tip is thick enough to mesh a
    // twig has one growing on from it, about straight on.
    let parents: Vec<usize> = (0..stems.len())
        .filter(|&i| {
            // (Its tip after the pipe model; a little thicker than when the
            // twig grew.)
            stems[i].level == 2 && *stems[i].radii.last().unwrap() >= recipe.mesh.min_radius * 1.5
        })
        .collect();
    assert!(parents.len() >= 10, "{}", parents.len());
    let carried = parents
        .iter()
        .filter(|&&parent| {
            let tip = stems[parent].direction(stems[parent].points.len() - 1);
            stems.iter().any(|child| {
                child.parent == Some((parent, 1.0)) && child.direction(0).dot(tip) > 0.7
            })
        })
        .count();
    assert_eq!(carried, parents.len());
}

#[test]
fn the_roots_spread_under_the_ground() {
    for recipe in every_preset() {
        let tree = generate(&recipe, 0);
        let trunk = &tree.skeleton.stems[0];
        assert!((trunk.points[0].y + recipe.trunk.sink).abs() < 0.01);
        let at_ground = trunk.at(recipe.trunk.sink / trunk.length()).2;
        assert!(trunk.radii[0] > at_ground * 1.3, "{}", recipe.name);
        let base = &tree.full().base;
        let lowest = base
            .positions
            .iter()
            .map(|p| p[1])
            .fold(f32::INFINITY, f32::min);
        assert!(
            // Deeper where the leaning foot and its tendrils tip down.
            (-1.2..=-0.3).contains(&lowest),
            "{}: {lowest}",
            recipe.name
        );
    }
}

#[test]
fn a_dead_tree_ends_in_weathered_splinters_and_snags() {
    let recipe = TreeRecipe::preset(Preset::Dead);
    let tree = generate(&recipe, 0);
    let stems = &tree.skeleton.stems;
    assert!(stems[0].broken);
    let snags = stems.iter().filter(|stem| stem.broken).count();
    assert!(snags >= 4, "{snags}");
    let bark = &tree.full().bark;
    assert_eq!(bark.weathering.len(), bark.positions.len());
    // Splinters are bare wood; the bark weathers going up, none at the foot.
    let splinters = bark.weathering.iter().filter(|&&w| w == 1.0).count();
    assert!(splinters > 0);
    let top = stems[0].points.last().unwrap().y;
    for (w, p) in bark.weathering.iter().zip(&bark.positions) {
        assert!((0.0..=1.0).contains(w));
        if p[1] < top * 0.2 {
            assert_eq!(*w, 0.0);
        }
    }
    let base = &tree.full().base;
    for (w, p) in base.weathering.iter().zip(&base.positions) {
        if p[1] < top * 0.2 {
            assert_eq!(*w, 0.0);
        }
    }
    // A living oak has no splinters.
    let oak = generate(&TreeRecipe::preset(Preset::Oak), 0);
    assert!(oak.skeleton.stems.iter().all(|stem| !stem.broken));
}

#[test]
fn a_pine_grows_its_boughs_in_whorls_with_dead_stubs_at_the_bottom() {
    let recipe = TreeRecipe::preset(Preset::Pine);
    let level = &recipe.levels[0];
    let tree = generate(&recipe, 0);
    let stems = &tree.skeleton.stems;
    let mut heights: Vec<f32> = stems
        .iter()
        .filter(|s| s.level == 1)
        .map(|s| s.parent.unwrap().1)
        .collect();
    heights.sort_by(f32::total_cmp);
    // Tiers: limbs of one whorl start within a few hundredths of each
    // other, and the tiers are far apart.
    let gap = (level.end - level.start) / level.whorls as f32;
    let tiers = 1 + heights
        .windows(2)
        .filter(|pair| pair[1] - pair[0] > gap * 0.5)
        .count();
    assert_eq!(tiers, level.whorls as usize, "{heights:?}");
    // The lowest boughs are dead stubs: snapped, bare and leafless.
    let stubs: Vec<usize> = (0..stems.len())
        .filter(|&i| stems[i].level == 1 && stems[i].parent.unwrap().1 < level.bare_below)
        .collect();
    assert!(!stubs.is_empty());
    for &i in &stubs {
        assert!(stems[i].broken);
        assert!(stems.iter().all(|s| s.parent.map(|p| p.0) != Some(i)));
    }
    let lowest_leaf = tree
        .leaf_anchors
        .iter()
        .map(|a| a.position.y)
        .fold(f32::INFINITY, f32::min);
    let highest_stub = stubs
        .iter()
        .map(|&i| stems[i].points[0].y)
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(lowest_leaf > highest_stub, "{lowest_leaf} {highest_stub}");
}

#[test]
fn clusters_toward_a_pines_top_are_smaller_and_turned_up() {
    let recipe = TreeRecipe::preset(Preset::Pine);
    let tree = generate(&recipe, 0);
    let (clusters, crown) = super::leaves::clusters(&recipe, &tree.leaf_anchors);
    let top = crown.center.y + crown.half_size.y * 0.8;
    let middle = crown.center.y;
    let (high, low): (Vec<_>, Vec<_>) = clusters
        .iter()
        .filter(|c| c.position.y > top || c.position.y < middle)
        .partition(|c| c.position.y > top);
    assert!(!high.is_empty() && !low.is_empty());
    let mean = |set: &[&super::leaves::Cluster], f: &dyn Fn(&super::leaves::Cluster) -> f32| {
        set.iter().map(|c| f(c)).sum::<f32>() / set.len() as f32
    };
    assert!(mean(&high, &|c| c.scale) < 0.85);
    assert!(mean(&low, &|c| c.scale) > 0.99);
    assert!(mean(&high, &|c| c.axis.y) > mean(&low, &|c| c.axis.y) + 0.2);
}

#[test]
fn a_broadleaf_crown_keeps_air_pockets_inside_but_a_full_outline() {
    let mut recipe = TreeRecipe::preset(Preset::Oak);
    recipe.leaves.pockets = 0.0;
    let anchors = generate(&recipe, 0).leaf_anchors;
    let (solid, crown) = super::leaves::clusters(&recipe, &anchors);
    recipe.leaves.pockets = 0.5;
    let (holed, _) = super::leaves::clusters(&recipe, &anchors);
    let depth = |p: Vec3| ((p - crown.center) / crown.half_size).length();
    let inner =
        |set: &[super::leaves::Cluster]| set.iter().filter(|c| depth(c.position) < 0.8).count();
    let outer = |set: &[super::leaves::Cluster]| set.len() - inner(set);
    assert!((inner(&holed) as f32) < inner(&solid) as f32 * 0.85);
    // The shell is thinned by shade a little differently, never by pockets.
    assert!(outer(&holed) as f32 > outer(&solid) as f32 * 0.9);
}

#[test]
fn leaf_cards_face_out_of_the_crown_in_crossed_pairs_with_bent_light() {
    for recipe in every_preset().filter(|r| r.leaves.enabled) {
        let tree = generate(&recipe, 0);
        let leaves = &tree.full().leaves;
        let crown = tree.crown;
        let corners = 6;
        let cards = leaves.positions.len() / corners;
        let (mut outward, mut crossed, mut bent) = (0, 0, 0.0);
        let face = |card: usize| {
            let p = |k: usize| Vec3::from(leaves.positions[card * corners + k]);
            (p(1) - p(0)).cross(p(5) - p(0)).normalize_or_zero()
        };
        for card in 0..cards {
            let p = |k: usize| Vec3::from(leaves.positions[card * corners + k]);
            let middle = (p(0) + p(2) + p(3) + p(5)) / 4.0;
            let f = face(card);
            if f.dot(crown.normal(middle)) > 0.0 {
                outward += 1;
            }
            // Crosswise to the card before, from the same foot.
            if card > 0 {
                let q = |k: usize| Vec3::from(leaves.positions[(card - 1) * corners + k]);
                // (Give or take the fold, which lifts a card off its foot.)
                let foot = |a: Vec3, b: Vec3| (a + b) / 2.0;
                let same_foot = foot(p(0), p(5)).distance(foot(q(0), q(5)))
                    < recipe.leaves.fold * recipe.leaves.size * 2.0 + 1e-4;
                if same_foot && f.dot(face(card - 1)).abs() < 0.3 {
                    crossed += 1;
                }
            }
            for k in 0..corners {
                let n = Vec3::from(leaves.normals[card * corners + k]);
                bent += 1.0 - n.dot(f).clamp(-1.0, 1.0);
            }
        }
        let share = outward as f32 / cards as f32;
        assert!(share > 0.7, "{}: {share}", recipe.name);
        assert!(
            crossed as f32 > cards as f32 * 0.1,
            "{}: {crossed} of {cards}",
            recipe.name
        );
        // On average the light leans well off the flat card.
        let bent = bent / (cards * corners) as f32;
        assert!(bent > 0.05, "{}: {bent}", recipe.name);
    }
}
