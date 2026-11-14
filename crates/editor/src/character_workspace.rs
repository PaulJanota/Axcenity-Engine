//! The Character Studio workspace (`docs/research/character-studio.md`,
//! stage CS2, decision D9): a character editor of its own, opened from the
//! start screen's Character Studio page with no project open.
//!
//! It edits a [`Look`] (a character file: build, hair, clothes) and shows
//! it live on the default humanoid built from its parts file, in a preview
//! World of its own that the engine's `Animators` pose (the character
//! idles) and the scene renderer draws. It shares `assets`, `anim`,
//! `scene` and `render` with the Game Editor and has none of its panels.
//! Its files live in the editor's data folder (`Character Studio/`): the
//! humanoid's model, parts, clips and state machine, written where missing,
//! and the saved characters, `characters/<name>.character.json`.
//!
//! `ui.rs` draws it; `stage.rs` is the viewport (camera, lighting,
//! backdrop and drawing).

mod stage;
mod ui;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use axcenity_anim::Animators;
use axcenity_assets::Assets;
use axcenity_core::{InstanceId, Value, Vector3, World};
use axcenity_editor_tools::preview::Canvas;
use serde::{Deserialize, Serialize};

pub use stage::{Backdrop, Lighting, Orbit};
pub use ui::WorkspaceAction;

/// The command-line flag that opens the editor straight into the workspace.
pub const FLAG: &str = "--character-studio";

/// The character file's format version.
pub const LOOK_VERSION: u32 = 1;

/// Where a workspace's files live, in the editor's data folder.
pub fn default_folder() -> Option<PathBuf> {
    eframe::storage_dir(crate::APP_ID).map(|data| data.join("Character Studio"))
}

/// The folder of saved characters, inside the workspace's folder.
pub const CHARACTERS_FOLDER: &str = "characters";

/// The face's parts, always worn.
const FACE_PARTS: [&str; 4] = ["Eyes", "Teeth", "Brows", "Lashes"];

/// Where a part is worn: one part per slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Slot {
    Hair,
    Top,
    Outerwear,
    Bottom,
    Shoes,
}

impl Slot {
    pub const ALL: [Slot; 5] = [
        Slot::Hair,
        Slot::Top,
        Slot::Outerwear,
        Slot::Bottom,
        Slot::Shoes,
    ];

    /// The slot a part of the humanoid's parts file is worn in, by its
    /// name; `None` for the body and the face's parts.
    pub fn of(part: &str) -> Option<Slot> {
        let prefixes: [(&str, Slot); 7] = [
            ("Hair_", Slot::Hair),
            ("Shirt_", Slot::Top),
            ("Underwear_Top", Slot::Top),
            ("Jacket_", Slot::Outerwear),
            ("Pants_", Slot::Bottom),
            ("Underwear_Briefs", Slot::Bottom),
            ("Shoes_", Slot::Shoes),
        ];
        prefixes
            .iter()
            .find(|(prefix, _)| part.starts_with(prefix))
            .map(|(_, slot)| *slot)
    }

    pub fn label(self) -> &'static str {
        match self {
            Slot::Hair => "Hairstyle",
            Slot::Top => "Top",
            Slot::Outerwear => "Outerwear",
            Slot::Bottom => "Bottoms",
            Slot::Shoes => "Shoes",
        }
    }

    /// What wearing nothing here is called.
    pub fn none_label(self) -> &'static str {
        match self {
            Slot::Hair => "Bald",
            _ => "None",
        }
    }

    /// The parts of the parts file worn here.
    pub fn options(self) -> Vec<&'static str> {
        axcenity_project::HUMANOID_PARTS
            .iter()
            .copied()
            .filter(|part| Slot::of(part) == Some(self))
            .collect()
    }
}

/// A part's name for people: `Jacket_Cropped` is "Cropped jacket".
pub fn part_label(part: &str) -> String {
    let words: Vec<&str> = part.split('_').collect();
    match words.as_slice() {
        ["Hair", style] => (*style).to_owned(),
        ["Underwear", "Top"] => "Sports top".to_owned(),
        ["Underwear", "Briefs"] => "Briefs".to_owned(),
        ["Shirt", "Tee"] => "T-shirt".to_owned(),
        [kind, style] => format!("{} {}", style, kind.to_lowercase()),
        _ => part.to_owned(),
    }
}

/// A character: what Character Studio edits and saves as a
/// `*.character.json` file. Values a file leaves out take the default
/// character's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    pub format_version: u32,
    pub name: String,
    /// Slim (-1) to heavy (1).
    pub build: f32,
    /// How muscular, 0 to 1.
    pub muscle: f32,
    pub hair: Option<String>,
    pub top: Option<String>,
    pub outerwear: Option<String>,
    pub bottom: Option<String>,
    pub shoes: Option<String>,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            format_version: LOOK_VERSION,
            name: "New Character".to_owned(),
            build: 0.0,
            muscle: 0.0,
            hair: Some("Hair_Swept".to_owned()),
            top: Some("Shirt_Tee".to_owned()),
            outerwear: Some("Jacket_Cropped".to_owned()),
            bottom: Some("Pants_Cargo".to_owned()),
            shoes: Some("Shoes_Sneakers".to_owned()),
        }
    }
}

impl Look {
    pub fn part(&self, slot: Slot) -> Option<&str> {
        match slot {
            Slot::Hair => self.hair.as_deref(),
            Slot::Top => self.top.as_deref(),
            Slot::Outerwear => self.outerwear.as_deref(),
            Slot::Bottom => self.bottom.as_deref(),
            Slot::Shoes => self.shoes.as_deref(),
        }
    }

    pub fn set_part(&mut self, slot: Slot, part: Option<&str>) {
        let part = part.map(str::to_owned);
        match slot {
            Slot::Hair => self.hair = part,
            Slot::Top => self.top = part,
            Slot::Outerwear => self.outerwear = part,
            Slot::Bottom => self.bottom = part,
            Slot::Shoes => self.shoes = part,
        }
    }

    /// The parts worn: the face's, then one per filled slot (only parts the
    /// parts file has).
    pub fn parts(&self) -> Vec<&str> {
        let mut parts = FACE_PARTS.to_vec();
        for slot in Slot::ALL {
            if let Some(part) = self.part(slot)
                && axcenity_project::HUMANOID_PARTS.contains(&part)
            {
                parts.push(part);
            }
        }
        parts
    }

    /// The body shapes this look leans on, by shape name: its build and
    /// the clothing masks its clothes need (`masks`: garment → masks).
    pub fn shape_weights(&self, masks: &HashMap<String, Vec<String>>) -> BTreeMap<String, f32> {
        let mut weights = BTreeMap::new();
        weights.insert("Build_Heavy".to_owned(), self.build.max(0.0));
        weights.insert("Build_Slim".to_owned(), (-self.build).max(0.0));
        weights.insert("Build_Muscular".to_owned(), self.muscle.clamp(0.0, 1.0));
        for list in masks.values() {
            for mask in list {
                weights.entry(mask.clone()).or_insert(0.0);
            }
        }
        for part in self.parts() {
            for mask in masks.get(part).into_iter().flatten() {
                weights.insert(mask.clone(), 1.0);
            }
        }
        weights
    }

    /// The safe ranges: build -1 to 1, muscle 0 to 1, a name.
    fn clamped(mut self) -> Self {
        self.build = self.build.clamp(-1.0, 1.0);
        self.muscle = self.muscle.clamp(0.0, 1.0);
        if self.name.trim().is_empty() {
            self.name = "New Character".to_owned();
        }
        self
    }

    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }

    pub fn from_json(text: &str) -> Result<Look, String> {
        let look: Look = serde_json::from_str(text).map_err(|err| err.to_string())?;
        if look.format_version > LOOK_VERSION {
            return Err(format!(
                "It was saved by a newer Axcenity (character format {}).",
                look.format_version
            ));
        }
        Ok(look.clamped())
    }
}

/// The starting points on the Presets page: a name, a line about it, and
/// the look.
pub fn presets() -> Vec<(&'static str, &'static str, Look)> {
    let base = Look::default();
    vec![
        (
            "Everyday",
            "The default outfit: jacket, tee, cargo joggers, sneakers.",
            base.clone(),
        ),
        (
            "Casual",
            "No jacket, a bob, a slimmer build.",
            Look {
                build: -0.4,
                hair: Some("Hair_Bob".to_owned()),
                outerwear: None,
                ..base.clone()
            },
        ),
        (
            "Athlete",
            "Muscular, a buzz cut, tee and joggers.",
            Look {
                muscle: 0.9,
                hair: Some("Hair_Buzz".to_owned()),
                outerwear: None,
                ..base.clone()
            },
        ),
        (
            "Long hair",
            "Long hair and a heavier build under the jacket.",
            Look {
                build: 0.5,
                hair: Some("Hair_Long".to_owned()),
                ..base.clone()
            },
        ),
        (
            "Base body",
            "Underwear only: a start for your own clothes.",
            Look {
                hair: Some("Hair_Crop".to_owned()),
                top: None,
                outerwear: None,
                bottom: Some("Underwear_Briefs".to_owned()),
                shoes: None,
                ..base
            },
        ),
    ]
}

/// Garment → the clothing masks it needs, from the humanoid's
/// customization data.
pub fn garment_masks() -> HashMap<String, Vec<String>> {
    let data: serde_json::Value =
        serde_json::from_str(axcenity_project::DEFAULT_HUMANOID_CUSTOMIZATION).unwrap_or_default();
    let mut out = HashMap::new();
    if let Some(parts) = data["parts"].as_object() {
        for (name, info) in parts {
            let masks = info["masks"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|mask| mask.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            out.insert(name.clone(), masks);
        }
    }
    out
}

/// The categories down the left of the workspace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    Presets,
    Body,
    Hair,
    Clothing,
    Stage,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Presets,
        Category::Body,
        Category::Hair,
        Category::Clothing,
        Category::Stage,
    ];
}

/// The workspace: the character being edited, its preview and its undo.
pub struct CharacterWorkspace {
    folder: PathBuf,
    assets: Assets,
    world: World,
    body: Option<InstanceId>,
    animators: Animators,
    masks: HashMap<String, Vec<String>>,
    look: Look,
    /// The look as last saved or opened (`None`: never saved).
    saved: Option<Look>,
    history: Vec<Look>,
    future: Vec<Look>,
    /// A slider being dragged: its edit is one undo step.
    dragging: bool,
    pub category: Category,
    pub orbit: Orbit,
    pub lighting: Lighting,
    pub backdrop: Backdrop,
    canvas: Canvas,
    last_frame: Option<Instant>,
    /// A line for the status bar: what was saved, what failed.
    pub notice: Option<String>,
    /// Home was asked for with unsaved changes: the editor asks to save.
    pub leaving: bool,
}

impl CharacterWorkspace {
    /// Opens the workspace on `look`, its files in `folder` (written where
    /// missing: the humanoid's model, parts and animations).
    pub fn open(folder: &Path, look: Look, saved: bool) -> Result<Self, String> {
        std::fs::create_dir_all(folder.join(CHARACTERS_FOLDER))
            .map_err(|err| format!("Could not make the Character Studio folder: {err}"))?;
        axcenity_project::add_default_humanoid(folder)
            .map_err(|err| format!("Could not write the humanoid's files: {err}"))?;
        axcenity_project::add_humanoid_parts(folder)
            .map_err(|err| format!("Could not write the humanoid's parts: {err}"))?;
        let look = look.clamped();
        let mut workspace = Self {
            folder: folder.to_owned(),
            assets: Assets::new(folder),
            world: World::new(),
            body: None,
            animators: Animators::new(),
            masks: garment_masks(),
            saved: saved.then(|| look.clone()),
            look,
            history: Vec::new(),
            future: Vec::new(),
            dragging: false,
            category: Category::default(),
            orbit: Orbit::default(),
            lighting: Lighting::default(),
            backdrop: Backdrop::default(),
            canvas: Canvas::default(),
            last_frame: None,
            notice: None,
            leaving: false,
        };
        workspace.build_world();
        Ok(workspace)
    }

    /// Opens a saved character file.
    pub fn open_file(folder: &Path, file: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(file)
            .map_err(|err| format!("Could not read {}: {err}", file.display()))?;
        let look = Look::from_json(&text)?;
        Self::open(folder, look, true)
    }

    pub fn look(&self) -> &Look {
        &self.look
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Whether the look changed since it was saved (a new one always has).
    pub fn dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.look)
    }

    /// Changes the look as one undo step and shows it.
    pub fn edit(&mut self, change: impl FnOnce(&mut Look)) {
        let before = self.look.clone();
        change(&mut self.look);
        self.look = std::mem::take(&mut self.look).clamped();
        if self.look != before {
            self.history.push(before);
            self.future.clear();
            self.sync_parts();
        }
    }

    /// A slider's change: the whole drag is one undo step (`done` ends it).
    pub fn drag(&mut self, change: impl FnOnce(&mut Look), done: bool) {
        if !self.dragging {
            self.history.push(self.look.clone());
            self.future.clear();
            self.dragging = true;
        }
        change(&mut self.look);
        self.look = std::mem::take(&mut self.look).clamped();
        if done {
            self.dragging = false;
            if self.history.last() == Some(&self.look) {
                self.history.pop();
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(before) = self.history.pop() {
            self.future.push(std::mem::replace(&mut self.look, before));
            self.dragging = false;
            self.sync_parts();
        }
    }

    pub fn redo(&mut self) {
        if let Some(after) = self.future.pop() {
            self.history.push(std::mem::replace(&mut self.look, after));
            self.sync_parts();
        }
    }

    /// Back to the default character (keeping the name), as an undo step.
    pub fn reset(&mut self) {
        let name = self.look.name.clone();
        self.edit(|look| {
            *look = Look {
                name,
                ..Look::default()
            }
        });
    }

    /// The saved file's path for the look's name.
    pub fn file_path(&self) -> Option<PathBuf> {
        let name = crate::character_studio::file_name(&self.look.name)?;
        Some(
            self.folder
                .join(CHARACTERS_FOLDER)
                .join(format!("{name}.character.json")),
        )
    }

    /// Saves the look as `characters/<name>.character.json`.
    pub fn save(&mut self) -> Result<PathBuf, String> {
        let path = self
            .file_path()
            .ok_or_else(|| "Give the character a name to save it.".to_owned())?;
        std::fs::write(&path, self.look.to_json())
            .map_err(|err| format!("Could not save {}: {err}", path.display()))?;
        self.saved = Some(self.look.clone());
        Ok(path)
    }

    /// Builds the preview World: a floor and the character from parts.
    fn build_world(&mut self) {
        let mut world = World::new();
        let workspace = world.workspace();
        let character =
            axcenity_project::modular_character(&mut world, Vector3::default(), &self.look.parts());
        let _ = world.set_parent(character, Some(workspace));
        self.body = world.find_first_child(character, "Body");
        stage::add_floor(&mut world);
        self.world = world;
        self.animators.clear();
    }

    /// Makes the preview's worn parts match the look: parts no longer worn
    /// go, new ones are put on (the rest keep their pose).
    fn sync_parts(&mut self) {
        let Some(body) = self.body else {
            return;
        };
        let wanted = self.look.parts();
        let children: Vec<InstanceId> = self
            .world
            .get(body)
            .map(|body| body.children().to_vec())
            .unwrap_or_default();
        let mut worn = Vec::new();
        for child in children {
            let Some(instance) = self.world.get(child) else {
                continue;
            };
            if instance.property("MeshId").is_none() {
                continue;
            }
            let name = instance.name().to_owned();
            if wanted.contains(&name.as_str()) {
                worn.push(name);
            } else {
                let _ = self.world.destroy(child);
            }
        }
        for part in wanted {
            if !worn.iter().any(|name| name == part) {
                axcenity_project::humanoid_part(&mut self.world, body, part);
            }
        }
    }

    /// The parts worn in the preview, by name.
    #[cfg(test)]
    pub fn worn(&self) -> Vec<String> {
        let Some(body) = self.body.and_then(|body| self.world.get(body)) else {
            return Vec::new();
        };
        let mut names: Vec<String> = body
            .children()
            .iter()
            .filter_map(|child| self.world.get(*child))
            .filter(|child| child.property("MeshId").is_some())
            .map(|child| child.name().to_owned())
            .collect();
        names.sort();
        names
    }

    /// Moves the preview on by the time since the last frame: the
    /// character idles.
    pub fn step(&mut self) -> f64 {
        let now = Instant::now();
        let seconds = self
            .last_frame
            .map_or(0.0, |last| now.duration_since(last).as_secs_f64())
            .min(0.1);
        self.last_frame = Some(now);
        self.step_by(seconds);
        seconds
    }

    pub(crate) fn step_by(&mut self, seconds: f64) {
        self.animators
            .update(&mut self.world, &self.assets, seconds);
    }

    /// The preview's poses with the look's body shapes leaned on: each
    /// posed part (the body and what it wears) gets the build and the
    /// masks by name; its face shapes keep what its pose has.
    pub fn poses(&self) -> axcenity_scene::Poses {
        let weights = self.look.shape_weights(&self.masks);
        let mut poses = axcenity_scene::Poses::new();
        for (id, posed) in self.animators.poses() {
            let Some(model) = self
                .world
                .get(*id)
                .and_then(|part| part.property("MeshId"))
                .and_then(Value::as_str)
                .and_then(|mesh| self.assets.model_ready(mesh))
            else {
                poses.insert(*id, posed.clone());
                continue;
            };
            let mut values = posed.morph_weights().to_vec();
            let mut changed = false;
            for (value, target) in values.iter_mut().zip(&model.morph_targets) {
                if let Some(weight) = weights.get(&target.name)
                    && *value != *weight
                {
                    *value = *weight;
                    changed = true;
                }
            }
            if changed {
                let mut leaned = (**posed).clone();
                leaned.set_morph_weights(&values);
                poses.insert(*id, Arc::new(leaned));
            } else {
                poses.insert(*id, posed.clone());
            }
        }
        poses
    }
}

/// The saved characters in `folder`, newest first: (name, path).
pub fn saved_characters(folder: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(folder.join(CHARACTERS_FOLDER)) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path
                .file_name()?
                .to_str()?
                .strip_suffix(".character.json")?;
            let modified = entry.metadata().and_then(|m| m.modified()).ok()?;
            let shown = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| Look::from_json(&text).ok())
                .map_or_else(|| name.to_owned(), |look| look.name);
            Some((modified, shown, path))
        })
        .collect();
    found.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    found
        .into_iter()
        .map(|(_, name, path)| (name, path))
        .collect()
}
