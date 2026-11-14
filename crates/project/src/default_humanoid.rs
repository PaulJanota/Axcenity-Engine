//! The default humanoid (`docs/research/default-humanoid.md`, stage HD3):
//! the skinned character in the repository's `assets/characters/humanoid`,
//! built into the engine like the default sky, so a project can have it
//! without importing anything. [`add_default_humanoid`] writes its model,
//! its starter animations (clips linked to the glTF animation file's takes
//! and a state machine) and a `Character` prefab into a project; New
//! Project's character templates set that prefab as
//! `StarterPlayer.StarterCharacter`, and Insert > Character places one.
//! [`default_character`] builds the prefab's Instances.
//!
//! The character is a Model holding an invisible `HumanoidRootPart` (the
//! body the controller measures, walks and collides with: 0.60 m across and
//! 5 studs tall, 1 stud = 0.35 m) and the visible MeshPart `Body`, welded
//! to it and not colliding, whose Animator plays the state machine. The
//! Humanoid plays the Idle, Walk, Run, Jump and Fall states as it moves.

use super::*;
use axcenity_core::{Value, Vector3};

/// The model, in a project.
pub const DEFAULT_HUMANOID_MODEL_PATH: &str = "assets/models/Humanoid/Humanoid.glb";
/// The starter animations (a glTF file of takes), in a project.
pub const DEFAULT_HUMANOID_ANIMATIONS_PATH: &str = "assets/models/Humanoid/HumanoidAnimations.glb";
/// The folder of its clips and state machine, in a project.
pub const DEFAULT_HUMANOID_CLIPS_FOLDER: &str = "assets/animations/Humanoid";
/// Its state machine, in a project.
pub const DEFAULT_HUMANOID_STATE_MACHINE_PATH: &str =
    "assets/animations/Humanoid/Humanoid.statemachine.json";
/// The prefab of the whole character, in a project: what
/// `StarterPlayer.StarterCharacter` names in a new character template.
pub const DEFAULT_CHARACTER_PREFAB_PATH: &str = "assets/prefabs/Character.prefab.json";

/// The default humanoid as one skinned mesh (`Humanoid.glb`).
pub static DEFAULT_HUMANOID_MODEL: &[u8] =
    include_bytes!("../../../assets/characters/humanoid/Humanoid.glb");

/// Its parts on the same skeleton, each its own object
/// (`HumanoidParts.glb`): see [`HUMANOID_PARTS`].
pub static DEFAULT_HUMANOID_PARTS: &[u8] =
    include_bytes!("../../../assets/characters/humanoid/HumanoidParts.glb");

/// What the parts file's shapes and garments need besides vertices
/// (`Humanoid.customization.json`): the bones the frame and face shapes
/// move, each garment's category and the clothing masks it needs, the
/// hair styles and the default outfit. Character Studio reads it.
pub static DEFAULT_HUMANOID_CUSTOMIZATION: &str =
    include_str!("../../../assets/characters/humanoid/Humanoid.customization.json");

/// The parts file, in a project ([`add_humanoid_parts`]).
pub const DEFAULT_HUMANOID_PARTS_PATH: &str = "assets/models/Humanoid/HumanoidParts.glb";

/// The parts in [`DEFAULT_HUMANOID_PARTS`], by object name: the body (one
/// seamless mesh, head included, with the face, frame, build, identity and
/// clothing mask shapes), the face's eyes, teeth, brows and lashes, the
/// clothes and the hair styles (no hair is bald). Which masks each garment
/// needs is in `Humanoid.customization.json`.
pub const HUMANOID_PARTS: [&str; 17] = [
    "Body",
    "Eyes",
    "Teeth",
    "Brows",
    "Lashes",
    "Shirt_Tee",
    "Jacket_Cropped",
    "Pants_Cargo",
    "Shoes_Sneakers",
    "Underwear_Briefs",
    "Underwear_Top",
    "Hair_Swept",
    "Hair_Bob",
    "Hair_Long",
    "Hair_Crop",
    "Hair_Buzz",
    "Hair_Ponytail",
];

/// Its 32 starter animations (`HumanoidAnimations.glb`).
pub static DEFAULT_HUMANOID_ANIMATIONS: &[u8] =
    include_bytes!("../../../assets/characters/humanoid/HumanoidAnimations.glb");

/// Studs per metre: the 1.75 m humanoid is 5 studs tall, as Axcenity's
/// characters are.
pub const STUDS_PER_METRE: f32 = 5.0 / 1.75;

/// The model's box in its file (metres, glTF axes: Y up, facing +Z), its
/// A-posed hands out to the sides and its hair above 1.75 m. A test checks
/// it against the file.
pub(crate) const MODEL_MIN: [f32; 3] = [-0.6092, 0.0, -0.1511];
pub(crate) const MODEL_MAX: [f32; 3] = [0.6092, 1.7819, 0.2153];

/// The root part's size in studs: 0.30 m round (the spec's capsule radius)
/// and 5 studs tall.
pub const ROOT_PART_SIZE: [f32; 3] = [0.6 * STUDS_PER_METRE, 5.0, 0.6 * STUDS_PER_METRE];

/// A state of the default state machine: its name, the take it plays,
/// its speed and what it does at its end.
struct State {
    name: &'static str,
    take: &'static str,
    speed: f64,
    end: End,
}

#[derive(Clone, Copy, PartialEq)]
enum End {
    /// Goes round.
    Loops,
    /// Holds its last frame (Jump, until the Humanoid plays Fall).
    Holds,
    /// Goes back to Idle.
    ReturnsToIdle,
}

/// The default state machine's states. Walk and Run play the Run and
/// Sprint takes faster, so the feet keep up with the controller's default
/// `WalkSpeed` (16 studs a second) and `RunSpeed` (32): the Run take covers
/// 3.6 m a second (10.3 studs), Sprint 6.0 m (17.1 studs).
const STATES: [State; 26] = {
    use End::*;
    const fn s(name: &'static str, take: &'static str, speed: f64, end: End) -> State {
        State {
            name,
            take,
            speed,
            end,
        }
    }
    [
        s("Idle", "Idle", 1.0, Loops),
        s("Walk", "Run", 16.0 / (3.6 * 5.0 / 1.75), Loops),
        s("Run", "Sprint", 32.0 / (6.0 * 5.0 / 1.75), Loops),
        s("Jump", "Jump", 1.0, Holds),
        s("Fall", "Fall", 1.0, Loops),
        s("Land", "Land", 1.0, ReturnsToIdle),
        s("Sit", "Sit", 1.0, Loops),
        s("Crouch", "Crouch", 1.0, Loops),
        s(
            "CrouchWalk",
            "CrouchWalk",
            16.0 * CROUCH_SPEED_SCALE / (0.9 * 5.0 / 1.75),
            Loops,
        ),
        s("Swim", "Swim", 1.0, Loops),
        s("Climb", "Climb", 1.0, Loops),
        s("Push", "Push", 1.0, Loops),
        s("Pull", "Pull", 1.0, Loops),
        s("Wave", "Wave", 1.0, ReturnsToIdle),
        s("Point", "Point", 1.0, ReturnsToIdle),
        s("PickUp", "PickUp", 1.0, ReturnsToIdle),
        s("Use", "Use", 1.0, ReturnsToIdle),
        s("Interact", "Interact", 1.0, ReturnsToIdle),
        s("Cheer", "Cheer", 1.0, ReturnsToIdle),
        s("Clap", "Clap", 1.0, Loops),
        s("Shrug", "Shrug", 1.0, ReturnsToIdle),
        s("Dance", "Dance", 1.0, Loops),
        s("HitReact", "HitReact", 1.0, ReturnsToIdle),
        s("WalkBackward", "WalkBackward", 1.0, Loops),
        s("WalkLeft", "WalkLeft", 1.0, Loops),
        s("WalkRight", "WalkRight", 1.0, Loops),
    ]
};

/// Writes the default humanoid into `project` (its model, animation file,
/// a clip per state, the state machine and the `Character` prefab) and
/// returns the prefab's path. Files already there are kept, so this never
/// writes over a creator's own (a changed clip or prefab stays changed).
pub fn add_default_humanoid(project: &Path) -> Result<&'static str, SaveError> {
    let write = |path: &str, bytes: &[u8]| write_missing(project, path, bytes);
    write(DEFAULT_HUMANOID_MODEL_PATH, DEFAULT_HUMANOID_MODEL)?;
    write(
        DEFAULT_HUMANOID_ANIMATIONS_PATH,
        DEFAULT_HUMANOID_ANIMATIONS,
    )?;
    for (path, text) in default_humanoid_animation_files() {
        write(&path, text.as_bytes())?;
    }
    let mut world = World::new();
    let character = default_character(&mut world, Vector3::default());
    write(
        DEFAULT_CHARACTER_PREFAB_PATH,
        world_file::write_prefab(&world, &[character]).as_bytes(),
    )?;
    Ok(DEFAULT_CHARACTER_PREFAB_PATH)
}

/// Writes `bytes` to `path` in `project` unless a file is there already.
fn write_missing(project: &Path, path: &str, bytes: &[u8]) -> Result<(), SaveError> {
    let file = project.join(path);
    if file.is_file() {
        return Ok(());
    }
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder).map_err(SaveError::new(folder))?;
    }
    fs::write(&file, bytes).map_err(SaveError::new(&file))
}

/// Writes the default humanoid's parts file ([`DEFAULT_HUMANOID_PARTS_PATH`])
/// into `project` if it isn't there, for [`humanoid_part`] (stage HD4).
pub fn add_humanoid_parts(project: &Path) -> Result<(), SaveError> {
    write_missing(project, DEFAULT_HUMANOID_PARTS_PATH, DEFAULT_HUMANOID_PARTS)
}

/// The `MeshId` of the part `name` (one of [`HUMANOID_PARTS`]) in the
/// parts file, with the LODs that come with it.
pub fn humanoid_part_mesh_id(name: &str) -> String {
    format!("{DEFAULT_HUMANOID_PARTS_PATH}#AxcenityHumanoid/{name}")
}

/// Puts the part `name` (one of [`HUMANOID_PARTS`]) on the character
/// whose skinned MeshPart is `body`: a MeshPart named `name` inside it,
/// which takes its pose and its place from `body` (`anim`'s followers), so
/// it needs no size or position, and doesn't collide, so the controller's
/// root part stays the body. Swapping it for another part is setting its
/// `MeshId`. The project needs [`add_humanoid_parts`].
pub fn humanoid_part(world: &mut World, body: InstanceId, name: &str) -> InstanceId {
    let part = world.create(ClassName::MeshPart).expect("not a service");
    world.set_name(part, name).expect("just made");
    for (property, value) in [
        ("MeshId", Value::String(humanoid_part_mesh_id(name))),
        ("CanCollide", Value::Bool(false)),
        ("CanQuery", Value::Bool(false)),
        ("CanTouch", Value::Bool(false)),
        ("Massless", Value::Bool(true)),
    ] {
        world
            .set_property(part, property, value)
            .expect("a MeshPart has it");
    }
    world.set_parent(part, Some(body)).expect("a part");
    part
}

/// A MeshPart's `Size` and its middle's offset from the feet, in studs, for
/// a model whose box in its file is `min` to `max` (metres): turned 180°
/// so its front (+Z in the file) faces -Z, which puts its middle at -x, -z.
fn body_box(min: [f32; 3], max: [f32; 3]) -> (Vector3, Vector3) {
    let size = |axis: usize| (max[axis] - min[axis]) * STUDS_PER_METRE;
    let middle = |axis: usize| (max[axis] + min[axis]) * 0.5 * STUDS_PER_METRE;
    (
        Vector3::new(size(0), size(1), size(2)),
        Vector3::new(-middle(0), middle(1), -middle(2)),
    )
}

/// A box's lowest and highest corners.
type Corners = ([f32; 3], [f32; 3]);

/// The box of each of [`HUMANOID_PARTS`] in the parts file (metres), read
/// once.
fn part_box(name: &str) -> Option<Corners> {
    static BOXES: std::sync::OnceLock<Vec<Option<Corners>>> = std::sync::OnceLock::new();
    let boxes = BOXES.get_or_init(|| {
        HUMANOID_PARTS
            .iter()
            .map(|name| {
                let mesh_id = humanoid_part_mesh_id(name);
                let (_, pick) = axcenity_assets::Pick::of_mesh_id(&mesh_id);
                let model = axcenity_assets::load_models(
                    DEFAULT_HUMANOID_PARTS,
                    Path::new("."),
                    axcenity_assets::ModelFormat::Gltf,
                    &[pick],
                )
                .ok()?
                .pop()?;
                let (center, size) = (model.center, model.size);
                Some((
                    std::array::from_fn(|axis| center[axis] - size[axis] * 0.5),
                    std::array::from_fn(|axis| center[axis] + size[axis] * 0.5),
                ))
            })
            .collect()
    });
    let index = HUMANOID_PARTS.iter().position(|part| *part == name)?;
    boxes[index]
}

/// The default character built from parts ([`HUMANOID_PARTS`]) instead of
/// one mesh: like [`default_character`], but its `Body` is the body from
/// the parts file (head included), and each of `parts` (the rest of
/// [`HUMANOID_PARTS`]: the face's parts, clothes, a hair style) is a
/// MeshPart inside it that follows its pose ([`humanoid_part`]). One draw
/// per part instead of one in all, for a character whose parts change.
/// Clothes cover the body by the body's mask shapes, which the character's
/// appearance sets (Character Studio, `docs/research/character-studio.md`
/// CS3); until then the body is drawn whole under them. The project needs
/// [`add_default_humanoid`] and [`add_humanoid_parts`].
pub fn modular_character(world: &mut World, position: Vector3, parts: &[&str]) -> InstanceId {
    let character = default_character(world, position);
    let body = world
        .find_first_child(character, "Body")
        .expect("default_character makes it");
    if let Some((min, max)) = part_box("Body") {
        let (size, middle) = body_box(min, max);
        let middle = Vector3::new(
            position.x + middle.x,
            position.y + middle.y,
            position.z + middle.z,
        );
        for (property, value) in [
            ("MeshId", Value::String(humanoid_part_mesh_id("Body"))),
            ("Size", Value::Vector3(size)),
            ("Position", Value::Vector3(middle)),
        ] {
            world
                .set_property(body, property, value)
                .expect("a MeshPart has it");
        }
    }
    for part in parts.iter().filter(|part| **part != "Body") {
        humanoid_part(world, body, part);
    }
    character
}

/// The default humanoid's clip files and state machine, as `(path, text)`:
/// one clip per state, linked to its take in the animation file over the
/// take's whole length.
pub fn default_humanoid_animation_files() -> Vec<(String, String)> {
    let takes = axcenity_assets::load_takes(
        DEFAULT_HUMANOID_ANIMATIONS,
        axcenity_assets::ModelFormat::Gltf,
    )
    .unwrap_or_default();
    let mut files = Vec::new();
    let mut states = Vec::new();
    for (index, state) in STATES.iter().enumerate() {
        let duration = takes
            .iter()
            .find(|take| take.name == state.take)
            .map_or(1.0, |take| f64::from(take.duration));
        let clip = format!("{DEFAULT_HUMANOID_CLIPS_FOLDER}/{}.anim.json", state.name);
        let text = serde_json::json!({
            "format_version": 2,
            "duration": duration,
            "loop": state.end == End::Loops,
            "link": {
                "source": DEFAULT_HUMANOID_ANIMATIONS_PATH,
                "take": state.take,
                "start": 0.0,
                "end": duration,
            },
            "tracks": [],
            "events": [],
        });
        files.push((clip.clone(), pretty(&text)));
        // Laid out in rows of six in the editor's graph.
        let (column, row) = (index % 6, index / 6);
        let mut entry = serde_json::json!({
            "name": state.name,
            "motion": { "clip": clip },
            "speed": state.speed,
            "position": [column as f64 * 180.0, row as f64 * 110.0],
        });
        if state.end == End::ReturnsToIdle {
            entry["returns"] =
                serde_json::json!({ "to": "Idle", "when": "finished", "blend": 0.2 });
        }
        states.push(entry);
    }
    let machine = serde_json::json!({
        "format_version": 2,
        "parameters": [],
        "default_state": "Idle",
        "states": states,
        "transitions": [],
    });
    files.push((
        DEFAULT_HUMANOID_STATE_MACHINE_PATH.to_owned(),
        pretty(&machine),
    ));
    files
}

fn pretty(value: &serde_json::Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text
}

/// The default character's `CrouchSpeedScale`.
const CROUCH_SPEED_SCALE: f64 = 0.25;

/// Builds the default character with no parent: a Model named Character,
/// its feet at `position`, facing -Z (as characters do), holding the
/// invisible `HumanoidRootPart`, the MeshPart `Body` welded to it (with an
/// Animator playing [`DEFAULT_HUMANOID_STATE_MACHINE_PATH`]), a Humanoid,
/// a ThirdPersonController, a Health and an Inventory. Its files are the
/// project's (see [`add_default_humanoid`]).
pub fn default_character(world: &mut World, position: Vector3) -> InstanceId {
    let mut make =
        |class, name: &str, parent: Option<InstanceId>, properties: Vec<(&str, Value)>| {
            let id = world.create(class).expect("not a service");
            world.set_name(id, name).expect("just made");
            for (property, value) in properties {
                world
                    .set_property(id, property, value)
                    .expect("the class has it");
            }
            if let Some(parent) = parent {
                world.set_parent(id, Some(parent)).expect("a new Model");
            }
            id
        };
    let at = |x: f32, y: f32, z: f32| {
        Value::Vector3(Vector3::new(position.x + x, position.y + y, position.z + z))
    };
    let model = make(ClassName::Model, "Character", None, Vec::new());
    let [rx, ry, rz] = ROOT_PART_SIZE;
    let root = make(
        ClassName::Part,
        "HumanoidRootPart",
        Some(model),
        vec![
            ("Size", Value::Vector3(Vector3::new(rx, ry, rz))),
            ("Position", at(0.0, ry * 0.5, 0.0)),
            ("Transparency", Value::Number(1.0)),
            ("CastShadow", Value::Bool(false)),
        ],
    );
    let (size, middle) = body_box(MODEL_MIN, MODEL_MAX);
    let body = make(
        ClassName::MeshPart,
        "Body",
        Some(model),
        vec![
            (
                "MeshId",
                Value::String(DEFAULT_HUMANOID_MODEL_PATH.to_owned()),
            ),
            ("Size", Value::Vector3(size)),
            ("Position", at(middle.x, middle.y, middle.z)),
            ("Orientation", Value::Vector3(Vector3::new(0.0, 180.0, 0.0))),
            ("CanCollide", Value::Bool(false)),
            ("CanQuery", Value::Bool(false)),
            ("CanTouch", Value::Bool(false)),
            ("Massless", Value::Bool(true)),
        ],
    );
    make(
        ClassName::Animator,
        "Animator",
        Some(body),
        vec![(
            "StateMachine",
            Value::String(DEFAULT_HUMANOID_STATE_MACHINE_PATH.to_owned()),
        )],
    );
    make(
        ClassName::WeldConstraint,
        "BodyWeld",
        Some(root),
        vec![
            ("Part0", Value::Instance(Some(root))),
            ("Part1", Value::Instance(Some(body))),
        ],
    );
    // A quarter of WalkSpeed crouched (4 studs/s, 1.4 m/s at this scale),
    // so CrouchWalk's steps keep up at about the pace Walk's do.
    let controller_properties = vec![("CrouchSpeedScale", Value::Number(CROUCH_SPEED_SCALE))];
    for (class, name, properties) in [
        (ClassName::Humanoid, "Humanoid", Vec::new()),
        (
            ClassName::ThirdPersonController,
            "ThirdPersonController",
            controller_properties,
        ),
        (ClassName::Health, "Health", Vec::new()),
        (ClassName::Inventory, "Inventory", Vec::new()),
    ] {
        make(class, name, Some(model), properties);
    }
    model
}

#[cfg(test)]
mod tests {
    use super::*;
    use axcenity_anim::{ClipFile, StateMachineFile};

    /// The JSON chunk of a GLB file.
    fn glb_json(bytes: &[u8]) -> serde_json::Value {
        let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        serde_json::from_slice(&bytes[20..20 + length]).unwrap()
    }

    #[test]
    fn the_model_box_matches_the_file() {
        let json = glb_json(DEFAULT_HUMANOID_MODEL);
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for mesh in json["meshes"].as_array().unwrap() {
            for primitive in mesh["primitives"].as_array().unwrap() {
                let index = primitive["attributes"]["POSITION"].as_u64().unwrap() as usize;
                let accessor = &json["accessors"][index];
                for axis in 0..3 {
                    min[axis] = min[axis].min(accessor["min"][axis].as_f64().unwrap() as f32);
                    max[axis] = max[axis].max(accessor["max"][axis].as_f64().unwrap() as f32);
                }
            }
        }
        for axis in 0..3 {
            assert!((min[axis] - MODEL_MIN[axis]).abs() < 0.002, "{min:?}");
            assert!((max[axis] - MODEL_MAX[axis]).abs() < 0.002, "{max:?}");
        }
    }

    #[test]
    fn every_state_plays_a_take_the_animation_file_has_and_the_files_read() {
        let takes = axcenity_assets::load_takes(
            DEFAULT_HUMANOID_ANIMATIONS,
            axcenity_assets::ModelFormat::Gltf,
        )
        .unwrap();
        let files = default_humanoid_animation_files();
        assert_eq!(files.len(), STATES.len() + 1);
        for (path, text) in &files[..STATES.len()] {
            let clip = ClipFile::from_json(text).unwrap_or_else(|err| panic!("{path}: {err}"));
            let link = clip.link.unwrap();
            assert!(takes.iter().any(|take| take.name == link.take), "{path}");
            assert!(clip.duration > 0.1, "{path}");
        }
        let machine = StateMachineFile::from_json(&files.last().unwrap().1).unwrap();
        // The states the Humanoid plays, and those Insert > Character has.
        for state in ["Idle", "Walk", "Run", "Jump", "Fall", "Sit"] {
            assert!(machine.state(state).is_some(), "{state}");
        }
    }

    #[test]
    fn adding_it_writes_the_prefab_and_its_files_and_keeps_changed_ones() {
        let folder = std::env::temp_dir().join(format!(
            "axcenity-default-humanoid-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        assert_eq!(
            add_default_humanoid(&folder).unwrap(),
            DEFAULT_CHARACTER_PREFAB_PATH
        );
        for path in [
            DEFAULT_HUMANOID_MODEL_PATH,
            DEFAULT_HUMANOID_ANIMATIONS_PATH,
            DEFAULT_HUMANOID_STATE_MACHINE_PATH,
            "assets/animations/Humanoid/Walk.anim.json",
        ] {
            assert!(folder.join(path).is_file(), "{path}");
        }
        // The prefab is one Model: the root part, the welded mesh with its
        // Animator, and the character's pieces.
        let mut world = World::new();
        let workspace = world.workspace();
        let roots = crate::Prefabs::in_folder(&folder)
            .instantiate(DEFAULT_CHARACTER_PREFAB_PATH, &mut world, workspace)
            .unwrap();
        let [model] = roots.as_slice() else {
            panic!("{roots:?}");
        };
        for name in [
            "HumanoidRootPart",
            "Body",
            "Humanoid",
            "ThirdPersonController",
            "Health",
            "Inventory",
        ] {
            assert!(world.find_first_child(*model, name).is_some(), "{name}");
        }
        let body = world.find_first_child(*model, "Body").unwrap();
        assert!(world.find_first_child(body, "Animator").is_some());
        // A creator's change is kept.
        let walk = folder.join("assets/animations/Humanoid/Walk.anim.json");
        fs::write(&walk, "changed").unwrap();
        add_default_humanoid(&folder).unwrap();
        assert_eq!(fs::read_to_string(&walk).unwrap(), "changed");
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn its_feet_are_at_the_position_and_it_faces_minus_z() {
        let mut world = World::new();
        let model = default_character(&mut world, Vector3::new(4.0, 10.0, -2.0));
        let property = |name: &str, what: &str| {
            let id = world.find_first_child(model, name).unwrap();
            world
                .get(id)
                .unwrap()
                .property(what)
                .unwrap()
                .as_vector3()
                .unwrap()
        };
        let (root, root_size) = (
            property("HumanoidRootPart", "Position"),
            property("HumanoidRootPart", "Size"),
        );
        assert!((root.y - root_size.y * 0.5 - 10.0).abs() < 1e-4);
        assert!((root_size.x - 0.6 * STUDS_PER_METRE).abs() < 1e-4);
        let (body, body_size) = (property("Body", "Position"), property("Body", "Size"));
        assert!(
            (body.y - body_size.y * 0.5 - 10.0).abs() < 1e-4,
            "feet on the ground"
        );
        assert!((body_size.y - 1.7819 * STUDS_PER_METRE).abs() < 1e-3);
        assert_eq!(
            property("Body", "Orientation"),
            Vector3::new(0.0, 180.0, 0.0)
        );
    }

    #[test]
    fn every_part_is_an_object_of_the_parts_file_on_the_same_skeleton() {
        let picks: Vec<axcenity_assets::Pick> = HUMANOID_PARTS
            .iter()
            .map(|name| {
                let mesh_id = humanoid_part_mesh_id(name);
                let (file, pick) = axcenity_assets::Pick::of_mesh_id(&mesh_id);
                assert_eq!(file, DEFAULT_HUMANOID_PARTS_PATH);
                pick
            })
            .collect();
        let models = axcenity_assets::load_models(
            DEFAULT_HUMANOID_PARTS,
            Path::new("."),
            axcenity_assets::ModelFormat::Gltf,
            &picks,
        )
        .unwrap();
        let whole = axcenity_assets::load_model(
            DEFAULT_HUMANOID_MODEL,
            Path::new("."),
            axcenity_assets::ModelFormat::Gltf,
        )
        .unwrap();
        let bones = |model: &axcenity_assets::Model| -> Vec<String> {
            let rig = model.rig.as_ref().expect("skinned");
            rig.bones.iter().map(|bone| bone.name.clone()).collect()
        };
        for (name, model) in HUMANOID_PARTS.iter().zip(&models) {
            assert_eq!(bones(model), bones(&whole), "{name}");
        }
        let mut world = World::new();
        let character = default_character(&mut world, Vector3::default());
        let body = world.find_first_child(character, "Body").unwrap();
        let hair = humanoid_part(&mut world, body, "Hair_Crop");
        let instance = world.get(hair).unwrap();
        assert_eq!(instance.parent(), Some(body));
        assert_eq!(
            instance.property("CanCollide").and_then(Value::as_bool),
            Some(false)
        );
    }

    #[test]
    fn a_modular_character_leads_its_parts_from_the_body() {
        let mut world = World::new();
        let dressed = [
            "Eyes",
            "Teeth",
            "Brows",
            "Lashes",
            "Shirt_Tee",
            "Pants_Cargo",
            "Shoes_Sneakers",
            "Hair_Long",
        ];
        let character = modular_character(&mut world, Vector3::new(4.0, 0.0, 0.0), &dressed);
        let body = world.find_first_child(character, "Body").unwrap();
        let property = |world: &World, id, name| world.get(id).unwrap().property(name).cloned();
        assert_eq!(
            property(&world, body, "MeshId"),
            Some(Value::String(humanoid_part_mesh_id("Body")))
        );
        // The body is the whole person, head included, so it stays drawn:
        // clothes hide skin with its mask shapes, not by hiding it.
        assert_ne!(
            property(&world, body, "Transparency"),
            Some(Value::Number(1.0))
        );
        for part in dressed {
            let id = world.find_first_child(body, part).unwrap();
            assert_eq!(
                property(&world, id, "MeshId"),
                Some(Value::String(humanoid_part_mesh_id(part)))
            );
        }
        // The body's box: feet on the ground at the position, the whole
        // 1.75 m (5 studs) person.
        let size = property(&world, body, "Size")
            .and_then(|v| v.as_vector3())
            .unwrap();
        let at = property(&world, body, "Position")
            .and_then(|v| v.as_vector3())
            .unwrap();
        assert!((at.y - size.y * 0.5).abs() < 0.05, "{at:?} {size:?}");
        assert!(size.y > 4.8 && size.y < 5.2, "{size:?}");
        assert!((at.x - 4.0).abs() < 0.5);
    }

    /// A model's triangles in its file's units (metres; glTF axes, Y up and
    /// the front +Z), each with its material's name. Loading fits a model
    /// into the unit cube per axis; `size` and `center` undo that.
    fn file_triangles(model: &axcenity_assets::Model) -> Vec<([[f32; 3]; 3], String)> {
        let file = |p: [f32; 3]| -> [f32; 3] {
            std::array::from_fn(|axis| p[axis] * model.size[axis] + model.center[axis])
        };
        let mut out = Vec::new();
        for primitive in &model.primitives {
            for corner in primitive.indices.chunks_exact(3) {
                let triangle = [0, 1, 2].map(|k| file(primitive.positions[corner[k] as usize]));
                out.push((triangle, primitive.material.name.clone()));
            }
        }
        out
    }

    /// How far along `dir` from `origin` a ray meets `triangle`, if it does
    /// (Möller–Trumbore).
    fn ray_hit(origin: [f32; 3], dir: [f32; 3], triangle: &[[f32; 3]; 3]) -> Option<f32> {
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let (e1, e2) = (sub(triangle[1], triangle[0]), sub(triangle[2], triangle[0]));
        let p = cross(dir, e2);
        let det = dot(e1, p);
        if det.abs() < 1e-12 {
            return None;
        }
        let s = sub(origin, triangle[0]);
        let u = dot(s, p) / det;
        let q = cross(s, e1);
        let v = dot(dir, q) / det;
        if u < 0.0 || v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = dot(e2, q) / det;
        (t > 0.0).then_some(t)
    }

    /// Horizontal rays round the waist, from the hips to the lower chest
    /// (0.88 to 1.15 m, every centimetre, 16 directions; higher, the A-pose
    /// arms come within 30 cm of the axis): a point on the body's axis (2 cm
    /// in front of the origin) and the direction out.
    fn waist_rays() -> Vec<([f32; 3], [f32; 3])> {
        let mut rays = Vec::new();
        for step in 0..=27 {
            let height = 0.88 + step as f32 * 0.01;
            for k in 0..16 {
                let angle = k as f32 * std::f32::consts::TAU / 16.0;
                rays.push(([0.0, height, 0.02], [angle.sin(), 0.0, angle.cos()]));
            }
        }
        rays
    }

    /// The triangles within `reach` of the body's axis between the hips and
    /// the chest, which the waist rays can meet.
    fn near_the_waist(
        triangles: Vec<([[f32; 3]; 3], String)>,
        reach: f32,
    ) -> Vec<([[f32; 3]; 3], String)> {
        triangles
            .into_iter()
            .filter(|(triangle, _)| {
                triangle
                    .iter()
                    .any(|p| p[1] > 0.85 && p[1] < 1.23 && p[0].hypot(p[2] - 0.02) < reach)
            })
            .collect()
    }

    /// The shipped default has no hole round its waist. The skin its outfit
    /// covers is removed from it, so a gap between its garments is a hole
    /// into the empty body: every waist ray must leave through a triangle
    /// within 30 cm. (CS1's T-shirt was cut along each arm by an infinite
    /// plane, which also cut the torso and lifted the hem at the flanks:
    /// defect 1 of `docs/development/character-studio/audit.md`.)
    #[test]
    fn the_default_character_has_no_hole_round_its_waist() {
        let model = axcenity_assets::load_models(
            DEFAULT_HUMANOID_MODEL,
            Path::new("."),
            axcenity_assets::ModelFormat::Gltf,
            &[axcenity_assets::Pick::Whole],
        )
        .unwrap()
        .pop()
        .unwrap();
        let triangles = near_the_waist(file_triangles(&model), 0.35);
        let holes: Vec<String> = waist_rays()
            .into_iter()
            .filter(|(origin, dir)| {
                !triangles
                    .iter()
                    .any(|(triangle, _)| ray_hit(*origin, *dir, triangle).is_some_and(|t| t < 0.30))
            })
            .map(|(origin, dir)| format!("{:.2} m toward {:.2},{:.2}", origin[1], dir[0], dir[2]))
            .collect();
        assert!(holes.is_empty(), "rays out through a hole: {holes:?}");
    }

    /// The modular character shows no skin between its garments round the
    /// waist, with the default outfit and with the T-shirt and trousers
    /// alone: from 26 cm out (inside the A-pose arms), each waist ray meets
    /// clothing before the body's skin.
    #[test]
    fn the_modular_outfit_covers_the_waist_with_and_without_the_jacket() {
        for outfit in [
            &["Shirt_Tee", "Pants_Cargo", "Jacket_Cropped"][..],
            &["Shirt_Tee", "Pants_Cargo"][..],
        ] {
            let mut objects = vec!["AxcenityHumanoid/Body".to_owned()];
            objects.extend(outfit.iter().map(|part| format!("AxcenityHumanoid/{part}")));
            let model = axcenity_assets::load_models(
                DEFAULT_HUMANOID_PARTS,
                Path::new("."),
                axcenity_assets::ModelFormat::Gltf,
                &[axcenity_assets::Pick::Objects(objects)],
            )
            .unwrap()
            .pop()
            .unwrap();
            let triangles = near_the_waist(file_triangles(&model), 0.35);
            let bare: Vec<String> = waist_rays()
                .into_iter()
                .filter_map(|(axis, dir)| {
                    let origin = [axis[0] + dir[0] * 0.26, axis[1], axis[2] + dir[2] * 0.26];
                    let inward = [-dir[0], 0.0, -dir[2]];
                    let first = triangles
                        .iter()
                        .filter_map(|(triangle, material)| {
                            ray_hit(origin, inward, triangle).map(|t| (t, material))
                        })
                        .min_by(|a, b| a.0.total_cmp(&b.0))?;
                    (first.1 == "Skin")
                        .then(|| format!("{:.2} m from {:.2},{:.2}", axis[1], dir[0], dir[2]))
                })
                .collect();
            assert!(bare.is_empty(), "{outfit:?}: skin shows at {bare:?}");
        }
    }
}
