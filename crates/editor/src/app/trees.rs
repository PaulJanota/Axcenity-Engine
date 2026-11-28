//! Insert > Tree (foliage plan T5): a tree grown from a recipe, drawn as a
//! MeshPart. The recipe (`.axtree`) and the model grown from it (`.glb`,
//! with its simpler copies as `_LOD1` and `_LOD2` objects) are written into
//! the project's `assets/trees/` the first time, so the tree loads, cooks,
//! scatters and packs as any model does. A recipe edited by hand grows a
//! new model the next time a tree of it is inserted.

use std::path::Path;

use axcenity_mesh_edit::MaterialMaps;
use axcenity_procedural::tree::{self, Preset, Tree, TreeRecipe, textures};

use super::*;

/// Where a project keeps its grown trees.
pub(crate) const TREES_FOLDER: &str = "assets/trees";

/// The bark and leaf maps the built-in recipes name, by their path
/// relative to the trees folder (`assets/trees/textures/README.md`).
const TEXTURES: [(&str, &[u8]); 10] = [
    (
        textures::OAK_BARK,
        include_bytes!("../../../../assets/trees/textures/bark_oak_color.jpg"),
    ),
    (
        textures::OAK_BARK_NORMAL,
        include_bytes!("../../../../assets/trees/textures/bark_oak_normal.jpg"),
    ),
    (
        textures::PINE_BARK,
        include_bytes!("../../../../assets/trees/textures/bark_pine_color.jpg"),
    ),
    (
        textures::PINE_BARK_NORMAL,
        include_bytes!("../../../../assets/trees/textures/bark_pine_normal.jpg"),
    ),
    (
        textures::BIRCH_BARK,
        include_bytes!("../../../../assets/trees/textures/bark_birch_color.jpg"),
    ),
    (
        textures::BIRCH_BARK_NORMAL,
        include_bytes!("../../../../assets/trees/textures/bark_birch_normal.jpg"),
    ),
    (
        textures::BROAD_LEAVES,
        include_bytes!("../../../../assets/trees/textures/leaves_broad.png"),
    ),
    (
        textures::BROAD_LEAVES_NORMAL,
        include_bytes!("../../../../assets/trees/textures/leaves_broad_normal.jpg"),
    ),
    (
        textures::CONIFER,
        include_bytes!("../../../../assets/trees/textures/leaves_conifer.png"),
    ),
    (
        textures::CONIFER_NORMAL,
        include_bytes!("../../../../assets/trees/textures/leaves_conifer_normal.jpg"),
    ),
];

/// Writes the built-in maps `recipe` names that `folder` doesn't have yet
/// (a map someone replaced is kept), and says how the model links them.
fn texture_maps(folder: &Path, recipe: &TreeRecipe) -> Result<Vec<(String, MaterialMaps)>, String> {
    let write = |file: &Option<String>| -> Result<(), String> {
        let Some(file) = file else { return Ok(()) };
        let path = folder.join(file);
        if path.exists() {
            return Ok(());
        }
        if let Some((_, bytes)) = TEXTURES.iter().find(|(name, _)| name == file) {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    let mut maps = Vec::new();
    for textures in Tree::textures(recipe) {
        write(&textures.color)?;
        write(&textures.normal)?;
        let link = |file: Option<String>| file.filter(|file| folder.join(file).exists());
        maps.push((
            textures.material,
            MaterialMaps {
                base_color: link(textures.color),
                normal: link(textures.normal),
                ..MaterialMaps::default()
            },
        ));
    }
    Ok(maps)
}

/// A tree's files in a project, and its model's size in studs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TreeFiles {
    /// The model's `MeshId`, relative to the project.
    pub mesh_id: String,
    pub size: Vector3,
    /// Whether the model was grown just now (it was missing, or older than
    /// its recipe).
    pub grown: bool,
}

/// Makes sure `project` has `preset`'s recipe and the model grown from it,
/// and says where the model is. A recipe already there is kept (it may
/// have been edited) and its model grown again when it's newer.
pub(crate) fn tree_files(project: &Path, preset: Preset) -> Result<TreeFiles, String> {
    let folder = project.join(TREES_FOLDER);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let name = preset.name().replace(' ', "");
    let recipe_path = folder.join(format!("{name}.axtree"));
    let model_path = folder.join(format!("{name}.glb"));
    let recipe = match std::fs::read_to_string(&recipe_path) {
        Ok(text) => TreeRecipe::from_json(&text).map_err(|e| format!("`{name}.axtree` {e}"))?,
        Err(_) => {
            let recipe = TreeRecipe::preset(preset);
            std::fs::write(&recipe_path, recipe.to_json()).map_err(|e| e.to_string())?;
            recipe
        }
    };
    let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let stale = match (modified(&model_path), modified(&recipe_path)) {
        (Some(model), Some(recipe)) => model < recipe,
        _ => true,
    };
    let maps = texture_maps(&folder, &recipe)?;
    let grown = tree::generate(&recipe, 0);
    if stale {
        let glb = axcenity_mesh_edit::write_objects_glb(&grown.objects(&recipe), &maps);
        std::fs::write(&model_path, glb).map_err(|e| e.to_string())?;
    }
    let size = grown
        .to_model(&recipe)
        .map_err(|e| format!("`{name}.axtree` grows no tree: {e}"))?
        .size;
    Ok(TreeFiles {
        mesh_id: format!("{TREES_FOLDER}/{name}.glb"),
        size: Vector3::new(size[0], size[1], size[2]),
        grown: stale,
    })
}

impl EditorApp {
    /// Inserts a tree of the default kind (an oak) in front of the camera,
    /// writing its files into the project first.
    pub(super) fn insert_tree(&mut self, near: Option<InstanceId>) {
        let Some(project) = self.project.as_ref().map(|project| project.folder.clone()) else {
            self.output.warning(
                "A tree's model is written into the project: save the project first, then \
                 insert the tree again.",
            );
            return;
        };
        let files = match tree_files(&project, Preset::Oak) {
            Ok(files) => files,
            Err(err) => {
                self.output
                    .error(format!("Could not write the tree's files: {err}"));
                return;
            }
        };
        if files.grown {
            self.output.info(format!(
                "Grew `{}` from its recipe beside it. Edit the recipe and insert a tree again \
                 to grow it anew.",
                files.mesh_id
            ));
        }
        self.insert_named(
            Insertable::Tree,
            near,
            "Tree",
            vec![
                ("MeshId", Value::String(files.mesh_id)),
                ("Size", Value::Vector3(files.size)),
                ("Anchored", Value::Bool(true)),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_writes_its_recipe_and_model_once_and_loads_as_a_model() {
        let project = crate::character_studio::test_folder("insert-tree");
        let first = tree_files(&project, Preset::Oak).unwrap();
        assert!(first.grown);
        assert_eq!(first.mesh_id, "assets/trees/Oak.glb");
        assert!(
            first.size.y > 20.0 && first.size.y < 45.0,
            "{:?}",
            first.size
        );
        let again = tree_files(&project, Preset::Oak).unwrap();
        assert!(!again.grown);
        assert_eq!(again.size, first.size);

        // The model loads with its bark, base and leaves, each with its
        // file's two simpler copies, and sways as a tree.
        let bytes = std::fs::read(project.join(&first.mesh_id)).unwrap();
        let folder = project.join(TREES_FOLDER);
        let model =
            axcenity_assets::load_model(&bytes, &folder, axcenity_assets::ModelFormat::Gltf)
                .unwrap();
        let names: Vec<&str> = model
            .primitives
            .iter()
            .map(|p| p.material.name.as_str())
            .collect();
        assert_eq!(names, ["TrunkBase", "Bark", "Leaves"]);
        for primitive in &model.primitives {
            let material = &primitive.material;
            // The leaves, cut out, also get the loader's far card copy.
            let copies = if material.name == "Leaves" { 3 } else { 2 };
            assert_eq!(primitive.lods.len(), copies, "{}", material.name);
            assert!(material.sway > 0.0);
            // Its bark and leaves are textured, from maps written beside it.
            assert!(material.base_color_texture.is_some(), "{}", material.name);
            assert!(material.normal_texture.is_some(), "{}", material.name);
        }
        assert!(folder.join(textures::OAK_BARK).exists());
        assert!(!folder.join(textures::CONIFER).exists());
        let leaves = &model.primitives[2].material;
        assert!(matches!(
            leaves.alpha_mode,
            axcenity_assets::AlphaMode::Mask { .. }
        ));
        assert!((model.size[1] - first.size.y).abs() < 1e-3);

        // A recipe edited by hand is kept, and grows its model again.
        let recipe = project.join("assets/trees/Oak.axtree");
        let mut edited = TreeRecipe::from_json(&std::fs::read_to_string(&recipe).unwrap()).unwrap();
        edited.trunk.length = 14.0;
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&recipe, edited.to_json()).unwrap();
        let regrown = tree_files(&project, Preset::Oak).unwrap();
        assert!(regrown.grown);
        assert!(regrown.size.y < first.size.y);
    }
}
