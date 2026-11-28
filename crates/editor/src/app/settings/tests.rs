use super::*;
use axcenity_project::branding::Picture;

#[test]
fn every_settings_page_draws_and_changes_apply_to_the_look() {
    let ctx = theme::test_context();
    let mut app = EditorApp::new(None);
    app.settings.open = true;
    let snapshots = std::env::var_os("AXCENITY_SNAPSHOTS").map(PathBuf::from);
    for page in SettingsPage::ALL {
        app.settings.page = page;
        let mut draw = |ui: &mut egui::Ui| {
            let ctx = &ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |_| {});
            app.settings_window(ctx);
        };
        match &snapshots {
            Some(folder)
                if matches!(
                    page,
                    SettingsPage::Appearance
                        | SettingsPage::Typography
                        | SettingsPage::Graphics
                        | SettingsPage::Shortcuts
                        | SettingsPage::Updates
                        | SettingsPage::Ai
                ) =>
            {
                crate::snapshot::snapshot(
                    &folder.join(format!("settings-{}.png", page.label().to_lowercase())),
                    egui::vec2(900.0, 640.0),
                    vec![Vec::new(); 3],
                    &mut draw,
                )
            }
            _ => {
                let _ = crate::theme::run(&ctx, Default::default(), |ui| draw(ui));
            }
        }
    }
    assert!(app.settings.open);

    let light = Appearance {
        theme: ThemeChoice::Light,
        accent: Accent::Orange,
        ..Appearance::default()
    };
    let _ = crate::theme::run(&ctx, Default::default(), |ui| {
        let ctx = &ui.ctx().clone();
        app.set_appearance(ctx, light.clone())
    });
    assert_eq!(app.appearance, light);
    assert!(!ctx.global_style().visuals.dark_mode);
    assert_eq!(
        theme::palette(&ctx).accent,
        Palette::new(false, Accent::Orange).accent
    );
}

/// eframe's saved settings, in memory.
#[derive(Default)]
struct MemoryStorage(std::collections::HashMap<String, String>);

impl eframe::Storage for MemoryStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }
    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }
    fn flush(&mut self) {}
}

/// Layouts saved by name (design system V17) are there after a restart.
#[test]
fn saved_layouts_survive_a_restart() {
    let ctx = theme::test_context();
    let mut app = EditorApp::new(None);
    app.layout.use_preset(crate::layout::Preset::Modeling);
    app.save_layout("Props").unwrap();
    let mut storage = MemoryStorage::default();
    eframe::App::save(&mut app, &mut storage);

    let mut restarted = EditorApp::new(None);
    assert!(restarted.saved_layouts.0.is_empty());
    restarted.load_settings(&ctx, Some(&storage));
    assert_eq!(restarted.saved_layouts, app.saved_layouts);
    restarted.use_saved_layout("Props");
    assert!(restarted.layout.is_open(Pane::ModelStudio));
}

/// The movement guide is on until it's turned off, and stays off after
/// a restart; the other viewport guides keep their own choices.
#[test]
fn the_movement_guide_choice_survives_a_restart() {
    let ctx = theme::test_context();
    let mut app = EditorApp::new(None);
    assert!(app.move_guide, "on by default");
    app.move_guide = false;
    let mut storage = MemoryStorage::default();
    eframe::App::save(&mut app, &mut storage);

    let mut restarted = EditorApp::new(None);
    restarted.load_settings(&ctx, Some(&storage));
    assert!(!restarted.move_guide);
    assert!(restarted.grid && restarted.view_cube);

    restarted.move_guide = true;
    eframe::App::save(&mut restarted, &mut storage);
    let mut again = EditorApp::new(None);
    again.load_settings(&ctx, Some(&storage));
    assert!(again.move_guide);
}

#[test]
fn graphics_settings_survive_a_restart() {
    let ctx = theme::test_context();
    let mut app = EditorApp::new(None);
    app.graphics = GraphicsSettings {
        quality: Quality::Medium,
        automatic: false,
        shadow_quality: Some(ShadowQuality::Off),
        shadow_distance: Some(60.0),
        post_processing: None,
        render_scale: Some(0.5),
        texture_quality: Some(TextureQuality::Half),
        texture_budget: Some(128),
        anti_aliasing: Some(AntiAliasing::Taa),
        ambient_occlusion: Some(AmbientOcclusion::Low),
        reflection_quality: Some(ReflectionQuality::Medium),
        contact_shadows: Some(false),
        shadow_lod: Some(false),
        screen_space_reflections: Some(false),
        realtime_gi: Some(true),
        volumetric_fog: Some(true),
        cloud_quality: Some(CloudQuality::Ultra),
        depth_of_field: Some(DepthOfFieldQuality::Off),
        vignette: Some(false),
        film_grain: Some(true),
        chromatic_aberration: None,
        sharpen: Some(false),
        sun_rays: Some(true),
        motion_blur: Some(MotionBlurQuality::Low),
        ray_tracing_distance: Some(300.0),
        ray_traced_shadows: Some(RayTracedShadows::Medium),
        ray_traced_shadow_resolution: Some(axcenity_render::RayShadowResolution::Half),
        ray_traced_ambient_occlusion: Some(axcenity_render::RayTracedAmbientOcclusion::Low),
        ray_traced_global_illumination: Some(axcenity_render::RayTracedGlobalIllumination::High),
        ray_traced_reflections: Some(axcenity_render::RayTracedReflections::Medium),
        ray_tracing: Some(true),
    };
    let mut storage = MemoryStorage::default();
    eframe::App::save(&mut app, &mut storage);

    let mut restarted = EditorApp::new(None);
    assert_eq!(restarted.graphics, GraphicsSettings::default());
    restarted.load_settings(&ctx, Some(&storage));
    assert_eq!(restarted.graphics, app.graphics);
}

#[test]
fn changing_the_graphics_setting_changes_the_viewport_at_once() {
    let mut app = EditorApp::new(Some(&super::super::tests::example("graphics")));
    let mut frame = 0;
    let mut seen = Vec::new();
    crate::snapshot::snapshot_rendered(
        &std::env::temp_dir().join(format!("axcenity-graphics-{}.png", std::process::id())),
        egui::vec2(320.0, 200.0),
        vec![Vec::new(); 3],
        |ui, render_state| {
            match frame {
                1 => app.graphics.shadow_quality = Some(ShadowQuality::Off),
                2 => app.graphics.anti_aliasing = Some(AntiAliasing::Fxaa),
                _ => {}
            }
            frame += 1;
            egui::CentralPanel::default().show(ui, |ui| {
                app.scene_pane(ui, Some(render_state));
            });
            seen.push(
                app.viewport
                    .graphics()
                    .map(|used| (used.shadow_quality, used.anti_aliasing)),
            );
        },
    );
    if seen.is_empty() {
        return; // No GPU.
    }
    let (high, off) = (ShadowQuality::High, ShadowQuality::Off);
    assert_eq!(
        seen,
        [
            Some((high, AntiAliasing::Msaa4)),
            Some((off, AntiAliasing::Msaa4)),
            Some((off, AntiAliasing::Fxaa))
        ]
    );
}

/// Lighting's Exposure and ToneMapping, set as the Properties panel sets
/// them, change what the viewport draws.
#[test]
fn exposure_and_tone_mapping_set_in_properties_change_the_viewport() {
    use axcenity_core::{ToneMapping, Value};
    let mut app = EditorApp::new(Some(&super::super::tests::example("graphics")));
    let lighting = app.world.lighting();
    let mut frame = 0;
    let mut drawn = Vec::new();
    crate::snapshot::snapshot_rendered(
        &std::env::temp_dir().join(format!("axcenity-look-{}.png", std::process::id())),
        egui::vec2(320.0, 200.0),
        vec![Vec::new(); 3],
        |ui, render_state| {
            match frame {
                1 => app.apply(lighting, Edit::Property("Exposure", Value::Number(2.0))),
                2 => app.apply(
                    lighting,
                    Edit::Property("ToneMapping", Value::ToneMapping(ToneMapping::Filmic)),
                ),
                _ => {}
            }
            frame += 1;
            egui::CentralPanel::default().show(ui, |ui| {
                app.scene_pane(ui, Some(render_state));
            });
            drawn.extend(app.viewport.pixels());
        },
    );
    if drawn.is_empty() {
        return; // No GPU.
    }
    let [before, brighter, filmic] = &drawn[..] else {
        panic!("drew {} frames, not 3", drawn.len());
    };
    let changed = |a: &[u8], b: &[u8]| {
        a.chunks(4)
            .zip(b.chunks(4))
            .filter(|(a, b)| a[..3] != b[..3])
            .count()
    };
    let pixels = before.len() / 4;
    let exposed = changed(before, brighter);
    assert!(
        exposed > pixels / 4,
        "exposure changed {exposed} of {pixels}"
    );
    let curved = changed(brighter, filmic);
    assert!(
        curved > pixels / 4,
        "tone mapping changed {curved} of {pixels}"
    );
}

/// graphics' LocalScript steps the quality on G. In Play that
/// changes the viewport, starting from the editor's own settings, and
/// Stop puts them back without saving the script's choice.
#[test]
fn a_local_scripts_graphics_change_lasts_until_stop() {
    use axcenity_script::KeyEvent;
    let mut app = EditorApp::new(Some(&super::super::tests::example("graphics")));
    app.play();
    let press = KeyEvent {
        key: axcenity_core::KeyCode::G,
        pressed: true,
    };
    let mut frame = 0;
    let mut seen = Vec::new();
    crate::snapshot::snapshot_rendered(
        &std::env::temp_dir().join(format!("axcenity-play-graphics-{}.png", std::process::id())),
        egui::vec2(320.0, 200.0),
        vec![Vec::new(); 3],
        |ui, render_state| {
            match frame {
                1 => app.play_frame(1.0 / 60.0, &[press.into()], None, None),
                2 => app.stop(),
                _ => app.play_frame(1.0 / 60.0, &[], None, None),
            }
            frame += 1;
            egui::CentralPanel::default().show(ui, |ui| {
                app.scene_pane(ui, Some(render_state));
            });
            seen.push(app.viewport.graphics().map(|used| used.shadow_quality));
        },
    );
    if seen.is_empty() {
        return; // No GPU.
    }
    // The editor's High, then Ultra (the one after it), then High again.
    assert_eq!(
        seen,
        [
            Some(ShadowQuality::High),
            Some(ShadowQuality::Ultra),
            Some(ShadowQuality::High)
        ]
    );
    assert_eq!(app.graphics, GraphicsSettings::default());
    assert!(
        app.output
            .lines()
            .iter()
            .any(|line| line.text == "Graphics quality: Ultra"),
        "{:?}",
        app.output.lines()
    );
}

#[test]
fn branding_is_kept_in_the_manifest_used_by_branding_paths_and_previewed() {
    let folder = crate::app::tests::copy_of_example("part", "branding-settings");
    let mut app = EditorApp::new(Some(&folder));
    std::fs::create_dir_all(folder.join("assets/branding")).unwrap();
    image::RgbaImage::from_pixel(256, 256, image::Rgba([30, 90, 200, 255]))
        .save(folder.join("assets/branding/icon.png"))
        .unwrap();
    // Square, so the banner is the wrong shape and says so.
    image::RgbaImage::from_pixel(400, 400, image::Rgba([200, 90, 30, 255]))
        .save(folder.join("assets/branding/banner.png"))
        .unwrap();
    let branding = axcenity_project::branding::Branding {
        icon: Some("assets/branding/icon.png".into()),
        banner: Some("assets/branding/banner.png".into()),
    };
    app.set_branding(branding.clone());
    assert_eq!(
        axcenity_project::read_manifest(&folder).unwrap().branding,
        branding
    );
    assert_eq!(
        &*app.assets.resolve("branding://icon"),
        "assets/branding/icon.png"
    );
    assert!(app.assets.texture_blocking("branding://icon").is_some());

    app.settings.open = true;
    app.settings.page = SettingsPage::Project;
    let ctx = theme::test_context();
    let mut draw = |ui: &mut egui::Ui| {
        let ctx = &ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |_| {});
        app.settings_window(ctx);
    };
    for _ in 0..2 {
        let _ = crate::theme::run(&ctx, Default::default(), |ui| draw(ui));
    }
    if let Some(folder) = std::env::var_os("AXCENITY_SNAPSHOTS") {
        crate::snapshot::snapshot(
            &PathBuf::from(folder).join("settings-branding.png"),
            egui::vec2(900.0, 1100.0),
            // Scrolled down to Branding.
            std::iter::once(Vec::new())
                .chain((0..8).map(|_| {
                    vec![
                        egui::Event::PointerMoved(egui::pos2(500.0, 600.0)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -400.0),
                            modifiers: Default::default(),
                            phase: egui::TouchPhase::Move,
                        },
                    ]
                }))
                .chain([Vec::new(), Vec::new()])
                .collect(),
            &mut draw,
        );
    }
    let banner = &app.branding_previews;
    assert!(
        banner
            .problem(Picture::Banner)
            .is_some_and(|p| p.contains("400×400"))
    );
    assert_eq!(banner.problem(Picture::Icon), None);
}

#[test]
fn the_ray_tracing_section_says_what_a_restart_will_do() {
    use super::graphics::{needs_restart, ray_tracing_status};
    assert!(ray_tracing_status(false, false, true).contains("can't ray trace"));
    assert!(ray_tracing_status(false, true, false).contains("Turn on Hardware ray tracing"));
    assert!(ray_tracing_status(false, true, true).contains("turns on when the editor restarts"));
    assert!(ray_tracing_status(true, true, false).contains("turns off"));
    assert!(ray_tracing_status(true, true, true).contains("is on"));
    // Restart now shows only when a restart changes what the GPU was opened with.
    assert!(needs_restart(false, true, true));
    assert!(needs_restart(true, true, false));
    assert!(
        !needs_restart(false, false, true),
        "a GPU that can't, can't"
    );
    assert!(!needs_restart(true, true, true));
    assert!(!needs_restart(false, true, false));
}

#[test]
fn the_texture_memory_row_says_where_its_budget_comes_from() {
    use super::graphics::automatic_texture_budget;
    let high = GraphicsSettings::preset(Quality::High);
    let card = |video_memory| Capabilities {
        video_memory,
        ..Capabilities::default()
    };
    // An RX 9070 XT's 16 GB, as the system reports it.
    let (text, why) = automatic_texture_budget(&high, &card(Some(17_095_983_104)));
    assert_eq!(text, "Automatic (4096 MB)");
    assert!(why.contains("16 GB") && why.contains("1024 MB"), "{why}");
    // A small card keeps the preset's.
    let (text, why) = automatic_texture_budget(&high, &card(Some(2 << 30)));
    assert_eq!(text, "Preset (1024 MB)");
    assert!(why.contains("2 GB"), "{why}");
    let (text, why) = automatic_texture_budget(&high, &card(None));
    assert_eq!(text, "Preset (1024 MB)");
    assert!(why.contains("doesn't say"), "{why}");
}

/// Every piece of text drawn in `shapes`.
fn drawn_texts(shapes: &[egui::epaint::ClippedShape]) -> String {
    fn walk(shape: &egui::Shape, found: &mut String) {
        match shape {
            egui::Shape::Text(text) => {
                found.push_str(text.galley.text());
                found.push('\n');
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
            _ => {}
        }
    }
    let mut found = String::new();
    for clipped in shapes {
        walk(&clipped.shape, &mut found);
    }
    found
}

#[test]
fn the_graphics_page_says_when_the_world_needs_more_than_the_memory_budgets() {
    use super::graphics::graphics_page;
    const MIB: u64 = 1024 * 1024;
    let ctx = theme::test_context();
    // Desert Homestead on High: 1,579 MiB of textures shrunk into 1,024.
    let drawn = axcenity_render::RenderStats {
        texture_bytes: 1018 * MIB,
        texture_bytes_wanted: 1579 * MIB,
        mesh_bytes: 174 * MIB,
        ..Default::default()
    };
    let page = |capabilities: &Capabilities, drawn: Option<&axcenity_render::RenderStats>| {
        let mut settings = GraphicsSettings::preset(Quality::High);
        // Texture memory is under Advanced, which search opens.
        find_setting(&ctx, "Texture memory");
        // Tall enough that nothing is scrolled out of sight.
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 4000.0),
            )),
            ..Default::default()
        };
        let mut draw = |ui: &mut egui::Ui| {
            graphics_page(ui, &mut settings, capabilities, drawn);
        };
        if let Some(folder) = std::env::var_os("AXCENITY_SNAPSHOTS").map(PathBuf::from)
            && drawn.is_some()
            && capabilities.video_memory.is_none()
        {
            crate::snapshot::snapshot(
                &folder.join("settings-graphics-memory.png"),
                egui::vec2(900.0, 420.0),
                vec![Vec::new(); 3],
                &mut draw,
            );
        }
        drawn_texts(&crate::theme::run(&ctx, input, draw).shapes)
    };
    // A card that doesn't say its memory: the preset's 1,024 MB.
    let unknown = Capabilities::default();
    let text = page(&unknown, Some(&drawn));
    assert!(text.contains("Preset (1024 MB)"), "{text}");
    assert!(
        text.contains("Textures here take 1579 MB at full size")
            && text.contains("Raise Texture memory"),
        "{text}"
    );
    // Nothing drawn yet, nothing to say.
    assert!(!page(&unknown, None).contains("Textures here take"));
    // A 16 GB card: 4,096 MB without being asked, and it all fits.
    let card = Capabilities {
        video_memory: Some(16 << 30),
        ..Capabilities::default()
    };
    let text = page(&card, Some(&drawn));
    assert!(text.contains("Automatic (4096 MB)"), "{text}");
    assert!(text.contains("Textures 4096 MB · meshes 2048 MB"), "{text}");
    assert!(!text.contains("Textures here take"), "{text}");
}

/// Every control on the Graphics page has a cost tag beside it, every
/// searchable Graphics row is one of them, and the page fits a 260-point
/// column with Advanced open.
#[test]
fn every_graphics_row_has_a_cost_tag_and_fits_a_narrow_column() {
    use super::graphics::{DRAWN_COSTS, DRAWN_ROWS, graphics_page};
    let ctx = theme::test_context();
    let mut settings = GraphicsSettings::preset(Quality::High);
    let capabilities = Capabilities::default();
    // A row under Advanced, so the fold is drawn open.
    find_setting(&ctx, "Sun rays");
    let mut width = 0.0;
    let _ = crate::theme::run(&ctx, Default::default(), |ui| {
        ui.set_max_width(260.0);
        ui.vertical(|ui| {
            ui.set_max_width(260.0);
            graphics_page(ui, &mut settings, &capabilities, None);
            width = ui.min_rect().width();
        });
    });
    let drawn = |key: &str| {
        ctx.data(|data| data.get_temp::<Vec<String>>(egui::Id::new(key)))
            .unwrap_or_default()
    };
    let rows = drawn(DRAWN_ROWS);
    assert_eq!(rows, drawn(DRAWN_COSTS), "each row drawn has its cost tag");
    for row in SEARCHABLE
        .iter()
        .filter(|row| row.page == SettingsPage::Graphics)
    {
        if matches!(row.name, "Quality" | "Ray tracing" | "In use") {
            continue;
        }
        assert!(
            rows.iter().any(|drawn| drawn == row.name),
            "`{}` is a row with a cost tag: {rows:?}",
            row.name
        );
    }
    assert!(rows.iter().any(|row| row == "Vignette"), "Advanced is open");
    assert!(width <= 261.0, "the page is {width} points wide");
    if let Some(folder) = std::env::var_os("AXCENITY_SNAPSHOTS").map(PathBuf::from) {
        let mut asked = false;
        crate::snapshot::snapshot(
            &folder.join("settings-graphics-narrow.png"),
            egui::vec2(260.0, 2600.0),
            vec![Vec::new(); 3],
            |ui| {
                if !std::mem::replace(&mut asked, true) {
                    find_setting(ui.ctx(), "Sun rays");
                }
                graphics_page(ui, &mut settings, &capabilities, None);
            },
        );
    }
}

#[test]
fn the_in_use_line_lists_each_ray_traced_effect() {
    use super::graphics::ray_tracing_in_use;
    use axcenity_render::{EffectiveGraphics, RayTracedAmbientOcclusion};
    let mut effective = EffectiveGraphics::default();
    assert!(ray_tracing_in_use(&effective).contains("off"));
    effective.ray_traced_global_illumination = axcenity_render::RayTracedGlobalIllumination::Low;
    effective.ray_traced_gi_in_software = true;
    let line = ray_tracing_in_use(&effective);
    assert!(
        line.contains("off") && line.contains("GI Low traced in software"),
        "{line}"
    );
    effective.ray_traced_global_illumination = axcenity_render::RayTracedGlobalIllumination::Off;
    effective.ray_traced_gi_in_software = false;
    effective.ray_queries = true;
    effective.ray_tracing_distance = 400.0;
    assert!(ray_tracing_in_use(&effective).contains("no ray traced effects"));
    effective.ray_traced_shadows = RayTracedShadows::Low;
    effective.ray_traced_ambient_occlusion = RayTracedAmbientOcclusion::High;
    let line = ray_tracing_in_use(&effective);
    assert!(line.contains("shadows Low"), "{line}");
    assert!(line.contains("ambient occlusion High (4 rays"), "{line}");
    effective.ray_traced_reflections = axcenity_render::RayTracedReflections::Low;
    let line = ray_tracing_in_use(&effective);
    assert!(
        line.contains("reflections Low (smoother than 0.15 roughness, half size)"),
        "{line}"
    );
}

/// Layout & ribbon has no checkbox per panel (those are the ribbon's Panels
/// menu and View › Panels): its one checkbox is the start screen's. It
/// lists the custom ribbon tabs, and Edit opens one in the tab editor.
#[test]
fn the_layout_page_lists_ribbon_tabs_and_has_no_panel_checkboxes() {
    use crate::ribbon_tabs::RibbonGroup;
    use egui::accesskit::Role;
    let ctx = theme::test_context();
    ctx.enable_accesskit();
    let mut app = EditorApp::new(None);
    app.ribbon_tabs.add_custom(
        "Building",
        "HAMMER",
        &[RibbonGroup::Parts, RibbonGroup::Tools],
    );
    app.settings.open = true;
    app.settings.page = SettingsPage::Layout;
    let mut clock = 0.0;
    let mut frame = |app: &mut EditorApp, events: Vec<egui::Event>| {
        clock += 0.05;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 640.0),
            )),
            time: Some(clock),
            events,
            ..Default::default()
        };
        crate::theme::run(&ctx, input, |ui| {
            let ctx = &ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |_| {});
            app.settings_window(ctx);
        })
    };
    frame(&mut app, Vec::new());
    let output = frame(&mut app, Vec::new());
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is on")
        .nodes;
    let labelled = |role: Role| -> Vec<(String, egui::Pos2)> {
        nodes
            .iter()
            .filter(|(_, node)| node.role() == role)
            .filter_map(|(_, node)| {
                let bounds = node.bounds()?;
                let middle = egui::pos2(
                    ((bounds.x0 + bounds.x1) * 0.5) as f32,
                    ((bounds.y0 + bounds.y1) * 0.5) as f32,
                );
                Some((node.label()?.to_owned(), middle))
            })
            .collect()
    };
    let checkboxes: Vec<String> = labelled(Role::CheckBox)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    assert_eq!(checkboxes, ["Fold the sidebar to its icons"]);
    let edit = labelled(Role::Button)
        .into_iter()
        .find(|(label, _)| label == "Edit its name, icon and groups")
        .expect("each custom tab has Edit")
        .1;
    for events in [
        vec![egui::Event::PointerMoved(edit)],
        vec![super::super::ui_designer_tests::press(edit, true)],
        vec![super::super::ui_designer_tests::press(edit, false)],
    ] {
        frame(&mut app, events);
    }
    assert!(app.ribbon_tabs.is_editing(), "Edit opens the tab editor");
}

/// With `AXCENITY_SNAPSHOTS` set: Layout & ribbon in both themes.
#[test]
fn the_layout_page_draws_in_both_themes() {
    use crate::ribbon_tabs::RibbonGroup;
    let Some(folder) = std::env::var_os("AXCENITY_SNAPSHOTS").map(PathBuf::from) else {
        return;
    };
    for (name, appearance) in [
        ("settings-layout-dark", Appearance::default()),
        (
            "settings-layout-light",
            Appearance {
                theme: ThemeChoice::Light,
                ..Appearance::default()
            },
        ),
    ] {
        let mut app = EditorApp::new(None);
        app.ribbon_tabs.add_custom(
            "Building",
            "HAMMER",
            &[
                RibbonGroup::Parts,
                RibbonGroup::Tools,
                RibbonGroup::Snapping,
            ],
        );
        app.ribbon_tabs
            .add_custom("Lights", "LIGHTBULB", &[RibbonGroup::Lights]);
        app.settings.open = true;
        app.settings.page = SettingsPage::Layout;
        crate::snapshot::snapshot(
            &folder.join(format!("{name}.png")),
            egui::vec2(900.0, 640.0),
            vec![Vec::new(); 3],
            |ui| {
                let ctx = &ui.ctx().clone();
                theme::apply(ctx, &appearance);
                egui::CentralPanel::default().show(ui, |_| {});
                app.settings_window(ctx);
            },
        );
    }
}

/// Regression check for the Shortcuts window glitch: on every Settings page
/// (especially Shortcuts), the rendered window width stays within WINDOW_SIZE
/// and never stretches past the top bar.
#[test]
fn settings_window_fits_its_top_bar_on_every_page() {
    let ctx = theme::test_context();
    let mut app = EditorApp::new(None);
    app.settings.open = true;
    for page in SettingsPage::ALL {
        app.settings.page = page;
        let mut output = None;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            output = Some(crate::theme::run(&ctx, input, |ui| {
                let ctx = &ui.ctx().clone();
                egui::CentralPanel::default().show(ui, |_| {});
                app.settings_window(ctx);
            }));
        }
        let window_id = egui::Id::new("settings window");
        let area = ctx
            .memory(|mem| mem.area_rect(window_id))
            .expect("settings window has an area rect");
        // Margins around WINDOW_SIZE add at most 2 * window_margin (~32 pt);
        // the old Shortcuts Grid bug pushed the body >80 pt past the top bar.
        let max_expected = WINDOW_SIZE.x + 40.0;
        assert!(
            area.width() <= max_expected,
            "Settings page {page:?} overflowed its top bar: width {:.1} > {max_expected:.1}",
            area.width()
        );
        let _ = output;
    }
}

/// Brand kit supports importing external images into `assets/branding/` and
/// handling drag-and-drop onto the branding section.
#[test]
fn branding_imports_external_files_and_handles_drops() {
    let folder = crate::app::tests::copy_of_example("part", "branding-drop");
    let external =
        std::env::temp_dir().join(format!("axcenity-brand-drop-{}.png", std::process::id()));
    image::RgbaImage::from_pixel(256, 256, image::Rgba([40, 140, 220, 255]))
        .save(&external)
        .unwrap();
    let rel = super::super::branding::import_branding_file(&folder, &external).unwrap();
    assert_eq!(
        rel,
        format!(
            "assets/branding/{}",
            external.file_name().unwrap().to_string_lossy()
        )
    );
    assert!(folder.join(&rel).exists());

    // Dropping an image over the Icon drop zone in Settings > This project assigns it.
    let mut app = EditorApp::new(Some(&folder));
    app.settings.open = true;
    app.settings.page = SettingsPage::Project;
    let ctx = theme::test_context();
    for _ in 0..2 {
        let _ = crate::theme::run(&ctx, Default::default(), |ui| {
            let ctx = &ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |_| {});
            app.settings_window(ctx);
        });
    }
    let drop_pos = app
        .branding_previews
        .icon_drop_rect
        .expect("icon drop rect recorded")
        .center();
    let handled = {
        let mut got = false;
        let input = egui::RawInput {
            events: vec![egui::Event::PointerMoved(drop_pos)],
            ..Default::default()
        };
        let _ = crate::theme::run(&ctx, input, |ui| {
            got = app.try_drop_branding(ui.ctx(), std::slice::from_ref(&external));
        });
        got
    };
    assert!(handled, "branding drop zone accepted dropped icon");
    assert_eq!(
        app.project.as_ref().unwrap().branding.icon.as_deref(),
        Some(rel.as_str())
    );
    std::fs::remove_file(&external).ok();
}

/// Every AI client guide generates a valid snippet with the project path.
#[test]
fn ai_connections_guides_cover_all_clients() {
    use super::super::mcp::check::AiClientGuide;
    let folder = PathBuf::from("/tmp/example-game");
    for client in AiClientGuide::ALL {
        let snippet = client.snippet(&folder, axcenity_mcp::Profile::Standard);
        assert!(
            snippet.contains("/tmp/example-game"),
            "{client:?} missing project path: {snippet}"
        );
        assert!(!client.steps().is_empty(), "{client:?} needs steps");
    }
    let antigravity = AiClientGuide::Antigravity.snippet(&folder, axcenity_mcp::Profile::Standard);
    assert!(antigravity.contains("\"mcpServers\""));
    let codex = AiClientGuide::Codex.snippet(&folder, axcenity_mcp::Profile::Standard);
    assert!(codex.contains("codex mcp add") && codex.contains("[mcp_servers.axcenity]"));
    let local = AiClientGuide::LocalModels.snippet(&folder, axcenity_mcp::Profile::Lite);
    assert!(local.contains("\"--profile\", \"lite\""));
}

/// Viewport HUD overlays (FPS bar and ray tracing badge) stay behind Settings
/// and theme cards stay inside Settings even at 125% UI scale.
#[test]
fn viewport_overlays_stay_behind_settings_and_theme_cards_stay_inside() {
    let mut app = EditorApp::new(None);
    app.settings.open = true;
    app.settings.page = SettingsPage::Appearance;
    let ctx = theme::test_context();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1024.0, 720.0),
        )),
        ..Default::default()
    };
    let badge = crate::viewport::ray_badge::Badge {
        text: "Ray traced (software)".to_owned(),
        detail: "Software BVH".to_owned(),
        warning: false,
        restart: false,
        compare: Some(false),
    };
    for _ in 0..2 {
        let _ = crate::theme::run(&ctx, input.clone(), |ui| {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                let rect = ui.max_rect();
                app.viewport_bar(
                    &ctx,
                    rect,
                    ui.layer_id().order,
                    None,
                    std::time::Duration::ZERO,
                );
                crate::viewport::ray_badge::paint(ui, rect, &badge);
            });
            app.settings_window(&ctx);
        });
    }
    let settings_layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new("settings window"));
    let bar_layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("viewport bar"));
    let badge_layer =
        egui::LayerId::new(egui::Order::Background, egui::Id::new("ray tracing badge"));
    let (settings_rect, bar_rect, badge_rect) = ctx.memory(|memory| {
        (
            memory.area_rect(settings_layer.id),
            memory.area_rect(bar_layer.id),
            memory.area_rect(badge_layer.id),
        )
    });
    assert!(settings_rect.is_some(), "settings window was drawn");
    assert!(bar_rect.is_some(), "viewport bar was drawn");
    assert!(badge_rect.is_some(), "ray tracing badge was drawn");
    assert!(
        bar_layer.order < settings_layer.order && badge_layer.order < settings_layer.order,
        "viewport overlays must paint below Settings"
    );

    // At 125% UI scale on a compact screen, every theme preview stays within Settings.
    app.appearance.ui_scale = 1.25;
    theme::apply(&ctx, &app.appearance);
    let output = crate::theme::run(&ctx, input, |ui| {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |_| {});
        app.settings_window(&ctx);
    });
    let win_rect = ctx
        .memory(|m| m.area_rect(settings_layer.id))
        .expect("settings rect at 125%");
    for clipped in &output.shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            let bounds = egui::Rect::from_min_size(text.pos, text.galley.size());
            assert!(
                bounds.max.x <= win_rect.max.x + 2.0,
                "settings text {:?} at {:?} extended past settings window {:?}",
                text.galley.text(),
                bounds,
                win_rect
            );
        }
    }
}

/// A settings file eframe couldn't read (cut short by a crash) is kept as
/// `app.ron.damaged`, and the Console says the settings were reset; a file
/// that was read, or none at all, says nothing (QA-020).
#[test]
fn a_damaged_settings_file_is_kept_and_reported() {
    let folder = std::env::temp_dir().join(format!("axcenity-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("app.ron");
    let warnings = |app: &EditorApp| {
        app.output
            .lines()
            .iter()
            .filter(|line| line.text.contains("settings file"))
            .count()
    };

    // No file yet (a first launch): nothing to say.
    let mut app = EditorApp::new(None);
    app.check_settings_file(&file, Some(&MemoryStorage::default()));
    assert_eq!(warnings(&app), 0);

    // Read whole: its appearance is there.
    std::fs::write(&file, "{\"appearance\": \"{}\"}").unwrap();
    let mut read = MemoryStorage::default();
    eframe::Storage::set_string(&mut read, "appearance", "{}".to_owned());
    app.check_settings_file(&file, Some(&read));
    assert_eq!(warnings(&app), 0);

    // Cut short: eframe gave nothing back.
    std::fs::write(&file, "{\n    \"appearance\": \"{\\\"theme").unwrap();
    app.check_settings_file(&file, Some(&MemoryStorage::default()));
    assert_eq!(warnings(&app), 1);
    let line = &app.output.lines().last().unwrap().text;
    assert!(line.contains("back to their defaults"), "{line}");
    assert!(line.contains("app.ron.damaged"), "{line}");
    assert_eq!(
        std::fs::read_to_string(folder.join("app.ron.damaged")).unwrap(),
        std::fs::read_to_string(&file).unwrap()
    );
    let _ = std::fs::remove_dir_all(&folder);
}
