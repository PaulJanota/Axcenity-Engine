use super::stage::ViewPreset;
use super::*;
use crate::character_studio::test_folder;
use crate::theme;

#[test]
fn a_look_wears_the_face_and_one_part_per_slot() {
    let look = Look::default();
    assert_eq!(
        look.parts(),
        vec![
            "Eyes",
            "Teeth",
            "Brows",
            "Lashes",
            "Hair_Swept",
            "Shirt_Tee",
            "Jacket_Cropped",
            "Pants_Cargo",
            "Shoes_Sneakers"
        ]
    );
    // Every part of the parts file but the body and face is worn somewhere.
    for part in axcenity_project::HUMANOID_PARTS {
        let face = part == "Body" || FACE_PARTS.contains(&part);
        assert_eq!(Slot::of(part).is_none(), face, "{part}");
    }
    assert_eq!(Slot::Hair.options().len(), 6);
    assert_eq!(part_label("Jacket_Cropped"), "Cropped jacket");
    assert_eq!(part_label("Hair_Bob"), "Bob");
    // A part the file doesn't have isn't worn.
    let mut odd = Look::default();
    odd.set_part(Slot::Hair, Some("Hair_Mohawk"));
    assert!(!odd.parts().contains(&"Hair_Mohawk"));
}

#[test]
fn the_body_shapes_follow_the_build_and_the_clothes_masks() {
    let masks = garment_masks();
    assert_eq!(masks["Shoes_Sneakers"], vec!["Mask_Feet".to_owned()]);
    let look = Look {
        build: 0.6,
        muscle: 0.3,
        outerwear: None,
        ..Look::default()
    };
    let weights = look.shape_weights(&masks);
    assert_eq!(weights["Build_Heavy"], 0.6);
    assert_eq!(weights["Build_Slim"], 0.0);
    assert_eq!(weights["Build_Muscular"], 0.3);
    // The tee covers the torso and the tops of the arms; without the
    // jacket the forearms show.
    assert_eq!(weights["Mask_Torso"], 1.0);
    assert_eq!(weights["Mask_UpperArms_Top"], 1.0);
    assert_eq!(weights["Mask_Forearms"], 0.0);
    let slim = Look {
        build: -0.5,
        ..Look::default()
    };
    assert_eq!(slim.shape_weights(&masks)["Build_Slim"], 0.5);
}

#[test]
fn a_character_file_round_trips_and_a_newer_one_is_refused() {
    let look = Look {
        name: "Mira".to_owned(),
        build: 0.25,
        hair: Some("Hair_Ponytail".to_owned()),
        shoes: None,
        ..Look::default()
    };
    assert_eq!(Look::from_json(&look.to_json()).unwrap(), look);
    // Values out of range are brought in, missing ones are the default's.
    let read = Look::from_json(r#"{"name": "Odd", "build": 7}"#).unwrap();
    assert_eq!(read.build, 1.0);
    assert_eq!(read.hair, Look::default().hair);
    let newer = format!(r#"{{"format_version": {}}}"#, LOOK_VERSION + 1);
    assert!(Look::from_json(&newer).unwrap_err().contains("newer"));
}

#[test]
fn every_preset_wears_only_parts_the_parts_file_has() {
    let all = presets();
    assert!(all.len() >= 4);
    assert_eq!(all[0].2, Look::default(), "the first preset is the default");
    for (name, _, look) in all {
        for slot in Slot::ALL {
            if let Some(part) = look.part(slot) {
                assert!(
                    axcenity_project::HUMANOID_PARTS.contains(&part),
                    "{name}: {part}"
                );
                assert_eq!(Slot::of(part), Some(slot), "{name}: {part}");
            }
        }
    }
}

#[test]
fn the_workspace_dresses_the_preview_undoes_and_saves() {
    let folder = test_folder("workspace");
    let mut workspace = CharacterWorkspace::open(&folder, Look::default(), false).unwrap();
    // The humanoid's files are written for the preview.
    assert!(
        folder
            .join(axcenity_project::DEFAULT_HUMANOID_PARTS_PATH)
            .is_file()
    );
    assert!(
        folder
            .join(axcenity_project::DEFAULT_HUMANOID_STATE_MACHINE_PATH)
            .is_file()
    );
    let mut wanted: Vec<String> = Look::default()
        .parts()
        .iter()
        .map(|p| p.to_string())
        .collect();
    wanted.sort();
    assert_eq!(workspace.worn(), wanted);
    assert!(workspace.dirty(), "a new character isn't saved yet");

    workspace.edit(|look| {
        look.set_part(Slot::Hair, Some("Hair_Long"));
        look.set_part(Slot::Outerwear, None);
    });
    let worn = workspace.worn();
    assert!(worn.contains(&"Hair_Long".to_owned()));
    assert!(!worn.contains(&"Hair_Swept".to_owned()));
    assert!(!worn.contains(&"Jacket_Cropped".to_owned()));
    // A slider's drag is one undo step.
    workspace.drag(|look| look.build = 0.2, false);
    workspace.drag(|look| look.build = 0.4, false);
    workspace.drag(|look| look.build = 0.5, true);
    workspace.undo();
    assert_eq!(workspace.look().build, 0.0);
    assert!(workspace.worn().contains(&"Hair_Long".to_owned()));
    workspace.undo();
    assert!(workspace.worn().contains(&"Jacket_Cropped".to_owned()));
    assert!(!workspace.can_undo());
    workspace.redo();
    assert!(workspace.worn().contains(&"Hair_Long".to_owned()));

    workspace.edit(|look| look.name = "Test Hero".to_owned());
    let path = workspace.save().unwrap();
    assert!(
        path.ends_with("characters/Test Hero.character.json")
            || path.ends_with("characters\\Test Hero.character.json"),
        "{}",
        path.display()
    );
    assert!(!workspace.dirty());
    let listed = saved_characters(&folder);
    assert_eq!(listed, vec![("Test Hero".to_owned(), path.clone())]);
    let reopened = CharacterWorkspace::open_file(&folder, &path).unwrap();
    assert_eq!(reopened.look(), workspace.look());
    assert!(!reopened.dirty());
    // Reset is the default character, keeping the name, and undoable.
    workspace.reset();
    assert_eq!(workspace.look().hair, Look::default().hair);
    assert_eq!(workspace.look().name, "Test Hero");
    workspace.undo();
    assert_eq!(workspace.look().hair.as_deref(), Some("Hair_Long"));
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_preview_poses_every_part_with_the_looks_shapes() {
    let folder = test_folder("workspace-poses");
    let look = Look {
        build: 0.7,
        ..Look::default()
    };
    let mut workspace = CharacterWorkspace::open(&folder, look, false).unwrap();
    for part in ["Body", "Hair_Swept", "Pants_Cargo"] {
        let mesh = axcenity_project::humanoid_part_mesh_id(part);
        assert!(workspace.assets.model_blocking(&mesh).is_some(), "{part}");
    }
    for _ in 0..4 {
        workspace.step_by(1.0 / 30.0);
    }
    let poses = workspace.poses();
    let body = workspace.body.unwrap();
    let posed = poses.get(&body).expect("the body is posed");
    let model = workspace
        .assets
        .model_blocking(&axcenity_project::humanoid_part_mesh_id("Body"))
        .unwrap();
    let weight = |name: &str| {
        let index = model
            .morph_targets
            .iter()
            .position(|target| target.name == name)
            .unwrap();
        posed.morph_weights()[index]
    };
    assert_eq!(weight("Build_Heavy"), 0.7);
    assert_eq!(weight("Mask_Torso"), 1.0);
    assert_eq!(weight("Blink_Left"), 0.0);
    // The parts it wears are posed too (they follow the body).
    let hair = workspace
        .world
        .find_first_child(body, "Hair_Swept")
        .unwrap();
    assert!(poses.contains_key(&hair));
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_workspace_draws_every_category_without_a_gpu() {
    let folder = test_folder("workspace-ui");
    let mut workspace = CharacterWorkspace::open(&folder, Look::default(), false).unwrap();
    let ctx = theme::test_context();
    for category in Category::ALL {
        workspace.category = category;
        for _ in 0..2 {
            let _ = theme::run(&ctx, Default::default(), |ui| {
                assert_eq!(workspace.ui(ui, None), None);
            });
        }
    }
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_views_look_at_the_character_from_where_they_say() {
    use axcenity_render::glam::Vec3;
    let camera = |preset: ViewPreset| preset.orbit().camera();
    // It faces -Z: the front view stands at -Z, the back at +Z.
    assert!(camera(ViewPreset::Front).position.z < -5.0);
    assert!(camera(ViewPreset::Back).position.z > 5.0);
    assert!(camera(ViewPreset::Side).position.x > 5.0);
    // The face view is close, at head height.
    let face = camera(ViewPreset::Face);
    assert!((face.position - face.look_at).length() < 4.0);
    assert!(face.look_at.y > 4.0);
    // Turning, zooming and panning stay in range.
    let mut orbit = ViewPreset::FullBody.orbit();
    orbit.turn(0.0, 10_000.0);
    assert!(orbit.pitch <= 80.0);
    orbit.zoom(1.0e6);
    assert!(orbit.distance >= 1.2);
    orbit.pan(1.0e6, 1.0e6);
    assert!(orbit.target.length() < Vec3::new(4.0, 6.0, 4.0).length() + 0.01);
}
