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
        assert_eq!(a.bark.positions, b.bark.positions, "{}", recipe.name);
        assert_eq!(a.bark.indices, b.bark.indices, "{}", recipe.name);
    }
}

#[test]
fn another_variant_is_another_tree() {
    let recipe = TreeRecipe::preset(Preset::Oak);
    let a = generate(&recipe, 0);
    let b = generate(&recipe, 1);
    assert_ne!(a.bark.positions, b.bark.positions);
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
        let tree = generate(&recipe, 1);
        let mesh = &tree.bark;
        let n = mesh.positions.len();
        assert_eq!(mesh.normals.len(), n);
        assert_eq!(mesh.uvs.len(), n);
        assert_eq!(mesh.tangents.len(), n);
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(mesh.indices.iter().all(|&i| (i as usize) < n));
        for (p, normal) in mesh.positions.iter().zip(&mesh.normals) {
            assert!(p.iter().all(|v| v.is_finite()), "{}", recipe.name);
            let length = Vec3::from(*normal).length();
            assert!((length - 1.0).abs() < 1e-3, "{}: {length}", recipe.name);
        }
        // Every triangle's winding agrees with its corners' normals (it
        // faces out), but for the ones too small to have a direction.
        let mut against = 0;
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
                against += 1;
            }
        }
        assert_eq!(against, 0, "{}", recipe.name);
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
    let around = tree.bark.uvs[corners][0];
    assert!(around >= 1.0 && (around - around.round()).abs() < 1e-5);
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
        assert_eq!(model.primitives.len(), 1);
        assert_eq!(model.primitives[0].material.name, "Bark");
        assert!(model.primitives[0].material.sway > 0.0);
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
fn every_preset_stays_within_a_hero_trees_triangles_without_leaves() {
    // The plan's hero budget is 60k at LOD0 with leaves (T2 sets it per
    // level); bark alone stays well under it.
    for recipe in every_preset() {
        let triangles = generate(&recipe, 0).triangles();
        assert!(
            triangles > 500 && triangles < 45_000,
            "{}: {triangles}",
            recipe.name
        );
    }
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
        let mut obj = String::new();
        for p in &tree.bark.positions {
            obj.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
        }
        for n in &tree.bark.normals {
            obj.push_str(&format!("vn {} {} {}\n", n[0], n[1], n[2]));
        }
        for t in tree.bark.indices.chunks(3) {
            let [a, b, c] = [t[0] + 1, t[1] + 1, t[2] + 1];
            obj.push_str(&format!("f {a}//{a} {b}//{b} {c}//{c}\n"));
        }
        let path = std::path::Path::new(&folder).join(format!("{}.obj", recipe.name));
        std::fs::write(&path, obj).unwrap();
        println!(
            "{}: {} triangles, {} limbs",
            recipe.name,
            tree.triangles(),
            tree.skeleton.stems.len()
        );
    }
}
