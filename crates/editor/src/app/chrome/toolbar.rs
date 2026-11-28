use super::ribbon_menus::{insert_hint, insert_items, insert_menu};
use super::*;

/// Why the scene tools are greyed out.
const SCENE_HIDDEN: &str = "These act on the Scene View: show it (Show Scene, before these \
                            tools) to use them.";

impl EditorApp {
    /// The ribbon under the menu bar: the tools, the snapping settings, then
    /// buttons that insert each primitive and each kind of script. The
    /// ribbon's tabs, on the app background like the header, over the
    /// ribbon itself, a shade lighter with a hairline above and below, so
    /// the tabs don't run into its buttons. Play, Run and Stop are pinned at
    /// the right of the tabs' row, so they're there whichever tab shows.
    pub(in crate::app) fn toolbar(&mut self, ui: &mut egui::Ui) {
        let palette = theme::palette(ui.ctx());
        ui.spacing_mut().item_spacing.y = 0.0;
        let centered = self.appearance.ribbon == theme::RibbonAlign::Centered;
        let mut clicks = RibbonClicks::default();
        let active_tab = egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: 2,
                bottom: 0,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let tab = self.ribbon_tabs.tab_bar(ui, centered);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.pinned_play(ui, &mut clicks);
                    });
                    tab
                })
                .inner
            })
            .inner;
        let groups = self.ribbon_tabs.groups();
        let ribbon = egui::Frame::new()
            .fill(palette.panel)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                // Every tab keeps the tallest tab's height (Home's snapping
                // grid), so the panels below don't jump when tabs change.
                let tallest_id = ribbon::tallest_id(ui.ctx());
                let tallest: f32 = ui.data(|d| d.get_temp(tallest_id)).unwrap_or(0.0);
                ui.set_min_height(tallest);
                let top = ui.min_rect().top();
                self.ribbon_row(ui, &groups, &mut clicks, "ribbon");
                let height = ui.min_rect().bottom() - top;
                if height > tallest {
                    ui.data_mut(|d| d.insert_temp(tallest_id, height));
                }
            })
            .response
            .rect;
        let line = egui::Stroke::new(1.0_f32, palette.border);
        let painter = ui.painter();
        painter.hline(ribbon.x_range(), ribbon.top() + 0.5, line);
        painter.hline(ribbon.x_range(), ribbon.bottom() - 0.5, line);
        // The active tab is filled like the ribbon and joins it, over the line.
        if let Some(tab) = active_tab {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(tab.left() + 1.0, ribbon.top() - 0.5),
                    egui::pos2(tab.right() - 1.0, ribbon.top() + 1.0),
                ),
                0,
                palette.panel,
            );
        }
        if let Some(what) = clicks.insert {
            let near = self.selected;
            self.act(Action::Insert { what, near });
        }
        if let Some(class) = clicks.insert_script {
            self.act(Action::InsertScript(class));
        }
        if clicks.insert_controls {
            self.insert_controls_ui();
        }
        if clicks.weld {
            self.weld_selected();
        }
        if let Some(action) = clicks.solid {
            self.solid_action(action);
        }
        if let Some(class) = clicks.constraint {
            self.pick_constraint_tool(class);
        }
        if clicks.bake_probes {
            self.bake_probes_asked = true;
        }
        if clicks.fill_probes {
            self.fill_with_probes();
        }
        if clicks.import {
            self.import_with_dialog(axcenity_assets::PROJECT_TEXTURES_FOLDER);
        }
        if clicks.check_assets {
            self.run_asset_check();
        }
        if clicks.play {
            if self.paused() {
                self.continue_play();
            } else if self.playing() {
                self.pause_play();
            } else {
                self.play();
            }
        }
        if clicks.run {
            self.run();
        }
        if clicks.stop {
            self.stop();
        }
    }

    /// Play (or Pause, Continue), Run and Stop at the right of the ribbon's
    /// tab row, styled like the navigation tabs, on every tab.
    fn pinned_play(&mut self, ui: &mut egui::Ui, clicks: &mut RibbonClicks) {
        let buttons = self.play_buttons(ui.ctx());
        // Drawn right to left, so Stop first.
        ui.spacing_mut().item_spacing.x = 2.0;
        for (index, button) in buttons.iter().enumerate().rev() {
            let clicked = ui
                .add_enabled(
                    button.enabled,
                    ribbon::PinnedButton::new(button.icon, button.label, button.color),
                )
                .on_hover_text(&button.hover)
                .on_disabled_hover_text(button.disabled_hover)
                .clicked();
            match index {
                0 => clicks.play |= clicked,
                1 => clicks.run |= clicked,
                _ => clicks.stop |= clicked,
            }
        }
    }

    /// Play (Pause or Continue while the game runs), Run and Stop, as the
    /// pinned buttons and the Game group show them.
    fn play_buttons(&self, ctx: &egui::Context) -> [PlayButton; 3] {
        let palette = theme::palette(ctx);
        let playing = self.playing();
        let paused = self.paused();
        let can_play = self.project.is_some();
        // Paused at a breakpoint, Play is Continue, as in Studio; while the
        // game runs, it's Pause (roadmap G6).
        let pausing = self.pausing();
        let (icon, label, hover) = if paused {
            (
                filled_icons::PLAY,
                "Continue",
                "Continue the game from the breakpoint",
            )
        } else if pausing {
            (
                filled_icons::PAUSE,
                "Pausing…",
                "Play pauses at the next line a script runs",
            )
        } else if playing {
            (
                filled_icons::PAUSE,
                "Pause",
                "Pause at the next line a script runs, to see its variables and step",
            )
        } else {
            (filled_icons::PLAY, "Play", "Save, then run the game here")
        };
        let shortcut = |action: crate::shortcuts::Action| action.menu_text(ctx);
        let open_first = "Open a project first.";
        [
            PlayButton {
                icon,
                label,
                hover: format!("{hover} ({})", shortcut(crate::shortcuts::Action::Play)),
                color: if playing && !paused {
                    palette.warning
                } else {
                    palette.success
                },
                enabled: (playing && !pausing || paused || !playing) && can_play,
                disabled_hover: if pausing {
                    "Waiting for a script to run a line."
                } else {
                    open_first
                },
            },
            PlayButton {
                icon: filled_icons::PERSON_SIMPLE_RUN,
                label: "Run",
                hover: format!(
                    "Save, then run the game with no player: Scripts and physics run, no \
                     character spawns, and the fly camera stays ({})",
                    shortcut(crate::shortcuts::Action::Run)
                ),
                color: palette.success,
                enabled: !playing && can_play,
                disabled_hover: if playing {
                    "The game is playing."
                } else {
                    open_first
                },
            },
            PlayButton {
                icon: filled_icons::STOP,
                label: "Stop",
                hover: format!(
                    "Stop the game and go back to editing ({})",
                    shortcut(crate::shortcuts::Action::Stop)
                ),
                color: palette.error,
                enabled: playing,
                disabled_hover: "The game isn't playing.",
            },
        ]
    }

    /// The ribbon's groups in a row: centred unless Settings > Appearance >
    /// Ribbon is Left, scrolling sideways when they don't fit, with Show
    /// Scene before the scene tools while no Scene View shows. The tab
    /// editor's preview draws a tab with this too; `salt` keeps the widths
    /// it remembers apart from the real ribbon's.
    pub(in crate::app) fn ribbon_row(
        &mut self,
        ui: &mut egui::Ui,
        groups: &[RibbonGroup],
        clicks: &mut RibbonClicks,
        salt: &str,
    ) {
        let scene = self.scene_shown(ui.ctx());
        // Centred, the groups start half the room they leave from the
        // left: their width is known from the last frame this tab
        // was drawn (the first frame of a tab is drawn again).
        let centered = self.appearance.ribbon == theme::RibbonAlign::Centered;
        let room = ui.available_width();
        let width_id = egui::Id::new(("ribbon width", salt, format!("{groups:?}"), scene));
        egui::ScrollArea::horizontal()
            .id_salt(("ribbon row", salt))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if centered {
                        match ui.data(|d| d.get_temp::<f32>(width_id)) {
                            Some(width) => ui.add_space(((room - width) / 2.0).max(0.0).floor()),
                            None => ui.ctx().request_discard("ribbon width"),
                        }
                    }
                    let start = ui.cursor().left();
                    // Show Scene goes before the first greyed-out group,
                    // where it's seen however narrow the window is.
                    let mut offered = scene;
                    for (index, &group) in groups.iter().enumerate() {
                        if index > 0 {
                            ui.add(egui::Separator::default().vertical().spacing(10.0));
                        }
                        if !offered && group.needs_scene() {
                            offered = true;
                            let show = ui
                                .add(RibbonButton::new(icons::CUBE_FOCUS, "Show Scene"))
                                .on_hover_text("Show the Scene View, to use its tools");
                            if show.clicked() {
                                self.layout.open(Pane::Scene);
                                ui.ctx().request_repaint();
                            }
                            ui.add(egui::Separator::default().vertical().spacing(10.0));
                        }
                        if scene || !group.needs_scene() {
                            self.ribbon_group(ui, group, clicks);
                        } else {
                            // Acting on a scene that isn't shown would
                            // change the world out of sight.
                            ui.add_enabled_ui(false, |ui| {
                                self.ribbon_group(ui, group, clicks);
                            })
                            .response
                            .on_disabled_hover_text(SCENE_HIDDEN);
                        }
                    }
                    let width = ui.min_rect().right() - start;
                    let known = ui.data(|d| d.get_temp::<f32>(width_id));
                    if known.is_none_or(|known| (known - width).abs() > 0.5) {
                        ui.data_mut(|d| d.insert_temp(width_id, width));
                        if known.is_some() {
                            ui.ctx().request_repaint();
                        }
                    }
                });
            });
    }

    /// The custom tab editor (`ribbon_tabs.rs`), with its preview drawn by
    /// [`ribbon_row`](Self::ribbon_row), the ribbon's own code. The
    /// preview's buttons are covered, and anything they'd do waits in a
    /// `RibbonClicks` that's dropped.
    pub(in crate::app) fn ribbon_tab_editor(&mut self, ctx: &egui::Context) {
        if !self.ribbon_tabs.is_editing() {
            return;
        }
        // Taken out while it's drawn, so the preview can borrow the app.
        let mut tabs = std::mem::take(&mut self.ribbon_tabs);
        tabs.editor_window(ctx, &mut |ui, groups| {
            let mut ignored = RibbonClicks::default();
            self.ribbon_row(ui, groups, &mut ignored, "tab preview");
        });
        self.ribbon_tabs = tabs;
    }

    /// The ribbon buttons for `tools`. Clicking one picks it.
    pub(super) fn tool_buttons(&mut self, ui: &mut egui::Ui, tools: &[Tool]) {
        for tool in tools {
            let key = tool.key();
            let hover = match key {
                Some(key) if *tool == Tool::Select => format!("Select tool ({})", key.name()),
                Some(key) => format!(
                    "{} tool ({}). Click again to go back to Select.",
                    tool.label(),
                    key.name()
                ),
                None => format!("{} tool", tool.label()),
            };
            let button = RibbonButton::new(theme::tool_icon(*tool), tool.label())
                .selected(self.tool == *tool);
            if ui.add(button).on_hover_text(hover).clicked() {
                self.toggle_tool(*tool);
            }
        }
    }

    /// Whether a Scene View showed last frame (docked, split or in its own
    /// window). The ribbon's scene tools and their keys need one.
    pub(in crate::app) fn scene_shown(&self, ctx: &egui::Context) -> bool {
        self.scene_drawn_pass
            .is_some_and(|frame| frame + 1 >= theme::frame_number(ctx))
    }

    /// Picks `tool`, or, if it's already picked, goes back to Select: a
    /// tool's button (or key) turns it off again, as well as on.
    pub(in crate::app) fn toggle_tool(&mut self, tool: Tool) {
        self.tool = if self.tool == tool {
            Tool::Select
        } else {
            tool
        };
        if !matches!(
            self.tool,
            Tool::Select | Tool::Move | Tool::Rotate | Tool::Transform
        ) {
            self.edit_pivot = false;
        }
    }

    /// Draws one of the ribbon's groups. Buttons that act on the whole app
    /// are noted in `clicks`, and the toolbar acts on them once it's drawn.
    pub(in crate::app) fn ribbon_group(
        &mut self,
        ui: &mut egui::Ui,
        group: RibbonGroup,
        clicks: &mut RibbonClicks,
    ) {
        let playing = self.playing();
        let can_play = self.project.is_some();
        match group {
            RibbonGroup::Game => ribbon::group_tinted(ui, "Game", Some(group.hue()), |ui| {
                let [play, run, stop] = self.play_buttons(ui.ctx());
                let mut add = |button: &PlayButton| {
                    let ribbon_button = RibbonButton::new(button.icon, button.label)
                        .family(theme::filled())
                        .color(button.color);
                    ui.add_enabled(button.enabled, ribbon_button)
                        .on_hover_text(&button.hover)
                        .on_disabled_hover_text(button.disabled_hover)
                        .clicked()
                };
                clicks.play = add(&play);
                clicks.run = add(&run);
                clicks.stop = add(&stop);
            }),
            RibbonGroup::Clients => ribbon::group_tinted(ui, "Clients", Some(group.hue()), |ui| {
                self.play_settings_controls(ui, playing);
            }),
            RibbonGroup::Check => ribbon::group_tinted(ui, "Check", Some(group.hue()), |ui| {
                let button = RibbonButton::new(icons::MAGNIFYING_GLASS, "Check Assets");
                clicks.check_assets = ui
                    .add_enabled(can_play, button)
                    .on_hover_text(
                        "Scan the open project for missing files, invalid references, \
                         import failures, and broken dependencies.",
                    )
                    .on_disabled_hover_text("Open a project first.")
                    .clicked();
                for (pane, hint) in [
                    (
                        Pane::Profiler,
                        "Show or hide the Profiler panel: frame time, GPU cost and memory during Play.",
                    ),
                    (
                        Pane::Mixer,
                        "Show or hide the Mixer panel: SoundGroups with live levels, mute and solo.",
                    ),
                    (
                        Pane::Tests,
                        "Show or hide the Tests panel: run the project's Luau game tests.",
                    ),
                    (
                        Pane::LodInspector,
                        "Show or hide the LOD Inspector for the selected MeshPart.",
                    ),
                ] {
                    let btn = RibbonButton::new(pane.icon(), pane.label())
                        .selected(self.layout.is_open(pane));
                    if ui.add(btn).on_hover_text(hint).clicked() {
                        self.layout.toggle(pane);
                    }
                }
            }),
            RibbonGroup::Plugins => self.plugin_ribbon_groups(ui),
            RibbonGroup::Tools => ribbon::group_tinted(ui, "Tools", Some(group.hue()), |ui| {
                self.tool_buttons(ui, &Tool::TRANSFORM);
                let has_target =
                    crate::viewport::pivot_target(&self.world, &self.selection, self.selected)
                        .is_some();
                let edit = RibbonButton::new(icons::CROSSHAIR_SIMPLE, "Edit Pivot")
                    .selected(self.edit_pivot);
                if ui
                    .add_enabled(!playing, edit)
                    .on_hover_text(
                        "Move or turn the selected Part's or Model's pivot point without moving \
                         the object, so Move and Rotate act around that point and angle.",
                    )
                    .on_disabled_hover_text("Press Stop to edit.")
                    .clicked()
                {
                    self.edit_pivot = !self.edit_pivot;
                }
                let reset = RibbonButton::new(icons::ARROW_COUNTER_CLOCKWISE, "Reset Pivot");
                if ui
                    .add_enabled(!playing && has_target, reset)
                    .on_hover_text("Put the selected object's pivot back at its center.")
                    .on_disabled_hover_text(if playing {
                        "Press Stop to edit."
                    } else {
                        "Select a Part or Model first."
                    })
                    .clicked()
                {
                    self.reset_pivot();
                }
            }),
            RibbonGroup::Build => ribbon::group_tinted(ui, "Build", Some(group.hue()), |ui| {
                self.tool_buttons(ui, &Tool::BUILD);
            }),
            RibbonGroup::Snapping => {
                ribbon::group_tinted(ui, "Snapping", Some(group.hue()), |ui| {
                    let snap = &mut self.snap;
                    let mut open_groups = false;
                    egui::Grid::new("snapping")
                        .num_columns(4)
                        .spacing([6.0, 4.0])
                        .show(ui, |ui| {
                            ui.checkbox(
                                &mut snap.move_on,
                                theme::menu_item(ui.ctx(), icons::GRID_FOUR, "Move"),
                            )
                            .on_hover_text("Move and scale in steps of this many studs.");
                            ui.add_enabled(
                                snap.move_on,
                                egui::DragValue::new(&mut snap.move_step)
                                    .range(Snap::MIN_MOVE_STEP..=1000.0)
                                    .speed(0.05)
                                    .max_decimals(3)
                                    .suffix(" studs"),
                            );
                            ui.checkbox(
                                &mut snap.surfaces,
                                theme::menu_item(ui.ctx(), icons::MAGNET, "Surfaces"),
                            )
                            .on_hover_text(
                                "Moved and resized parts snap flush against, or in line with, the \
                             faces of parts near them.",
                            );
                            ui.checkbox(
                                &mut snap.points,
                                theme::menu_item(ui.ctx(), icons::CROSSHAIR, "Snap Points"),
                            )
                            .on_hover_text(
                                "Show markers on the parts around a move, and click its corners, \
                             edge and face middles and Attachments to theirs.",
                            );
                            ui.checkbox(
                                &mut snap.join,
                                theme::menu_item(ui.ctx(), icons::LINK_SIMPLE, "Join Surfaces"),
                            )
                            .on_hover_text(
                                "When you let go of a part you moved, weld it to the parts it's \
                             touching, so they move as one in the game.",
                            );
                            ui.end_row();
                            ui.checkbox(
                                &mut snap.rotate_on,
                                theme::menu_item(ui.ctx(), icons::ANGLE, "Rotate"),
                            )
                            .on_hover_text("Rotate in steps of this many degrees.");
                            ui.add_enabled(
                                snap.rotate_on,
                                egui::DragValue::new(&mut snap.rotate_step)
                                    .range(Snap::MIN_ROTATE_STEP..=180.0)
                                    .speed(0.5)
                                    .max_decimals(2)
                                    .suffix("°"),
                            );
                            ui.checkbox(
                                &mut snap.collisions,
                                theme::menu_item(ui.ctx(), icons::CUBE, "Collisions"),
                            )
                            .on_hover_text(
                                "Moved, resized and turned parts stop at solid parts (CanCollide) \
                             instead of going into them.",
                            );
                            open_groups = ui
                                .button(theme::menu_item(
                                    ui.ctx(),
                                    icons::INTERSECT,
                                    "Collision Groups…",
                                ))
                                .on_hover_text(
                                    "Name collision groups and choose which of them collide: parts \
                                 in groups that don't pass through each other.",
                                )
                                .clicked();
                            ui.end_row();
                        });
                    if open_groups {
                        self.collision_groups.open = true;
                    }
                })
            }
            RibbonGroup::Parts => ribbon::group_tinted(ui, "Parts", Some(group.hue()), |ui| {
                for what in Insertable::PARTS {
                    let button = RibbonButton::new(theme::insert_icon(what), what.label());
                    if ui
                        .add_enabled(!playing, button)
                        .on_hover_text(insert_hint(what))
                        .on_disabled_hover_text("Press Stop to edit.")
                        .clicked()
                    {
                        clicks.insert = Some(what);
                    }
                }
            }),
            RibbonGroup::Effects => ribbon::group_tinted(ui, "Effects", Some(group.hue()), |ui| {
                insert_menu(
                    ui,
                    (icons::SPEAKER_HIGH, "Sound"),
                    "Insert a Sound, a SoundGroup or a Reverb Zone.",
                    &[&Insertable::AUDIO],
                    playing,
                    &mut clicks.insert,
                );
                let particles = Insertable::ParticleEmitter;
                let button = RibbonButton::new(theme::insert_icon(particles), "Particles");
                if ui
                    .add_enabled(!playing, button)
                    .on_hover_text(insert_hint(particles))
                    .on_disabled_hover_text("Press Stop to edit.")
                    .clicked()
                {
                    clicks.insert = Some(particles);
                }
                insert_menu(
                    ui,
                    (icons::APERTURE, "Post FX"),
                    "Insert a lens effect over the whole picture, a Graphics Volume that \
                     changes the look inside it, or a Weather Volume that keeps rain and snow \
                     out of a room.",
                    &[&Insertable::LENS, &Insertable::VOLUMES],
                    playing,
                    &mut clicks.insert,
                );
                let effect = RibbonButton::new(Pane::Effect.icon(), Pane::Effect.label())
                    .selected(self.layout.is_open(Pane::Effect));
                if ui
                    .add(effect)
                    .on_hover_text(
                        "Show or hide the Effect editor for the selected particle effect.",
                    )
                    .clicked()
                {
                    self.layout.toggle(Pane::Effect);
                }
            }),
            RibbonGroup::Lights => {
                ribbon::group_tinted(ui, "Lights & Sky", Some(group.hue()), |ui| {
                    let button = ui
                        .add_enabled(
                            !playing,
                            RibbonButton::new(icons::LIGHTBULB, "Light").dropdown(),
                        )
                        .on_hover_text("Insert a light or a probe, or bake the probes.")
                        .on_disabled_hover_text("Press Stop to edit.");
                    egui::Popup::menu(&button).show(|ui| {
                        insert_items(
                            ui,
                            &[&Insertable::LIGHTS, &Insertable::PROBES],
                            &mut clicks.insert,
                        );
                        ui.separator();
                        if ui
                            .button(theme::menu_item(
                                ui.ctx(),
                                icons::CAMERA_ROTATE,
                                "Bake Probes",
                            ))
                            .on_hover_text(
                                "Bake every LightProbeVolume and ReflectionProbe: the light around \
                             each probe is saved in assets/probes. Bake again after changing \
                             the world: an out-of-date volume has a warning in the Explorer.",
                            )
                            .clicked()
                        {
                            clicks.bake_probes = true;
                        }
                        if ui
                            .button(theme::menu_item(
                                ui.ctx(),
                                icons::GRID_FOUR,
                                "Fill with Probes",
                            ))
                            .on_hover_text(
                                "Put a LightProbeVolume around the selected parts or model, with \
                             its probes spaced to fit 512. Probes inside solid parts are left \
                             out of the bake. Then Bake Probes.",
                            )
                            .clicked()
                        {
                            clicks.fill_probes = true;
                        }
                    });
                    insert_menu(
                        ui,
                        (icons::CLOUD_SUN, "Sky"),
                        "Insert an Atmosphere, Clouds, Weather, outlines or color correction.",
                        &[&Insertable::SKY],
                        playing,
                        &mut clicks.insert,
                    );
                })
            }
            RibbonGroup::Gameplay => {
                ribbon::group_tinted(ui, "Gameplay", Some(group.hue()), |ui| {
                    insert_menu(
                        ui,
                        (icons::PERSON_SIMPLE, "Character"),
                        "Insert a ready character, or the pieces of one: a Humanoid, a controller, \
                     Health, Tools and an Inventory, or an agent that walks it.",
                        &[
                            &Insertable::HUMANOIDS,
                            &Insertable::CHARACTERS,
                            &Insertable::HEALTH,
                            &Insertable::TOOLS,
                            &Insertable::AGENTS,
                        ],
                        playing,
                        &mut clicks.insert,
                    );
                    insert_menu(
                        ui,
                        (icons::HAND_POINTING, "Interact"),
                        "Insert a Prompt, a Click Detector or a Seat.",
                        &[&Insertable::INTERACTION, &Insertable::SEATS],
                        playing,
                        &mut clicks.insert,
                    );
                    insert_menu(
                        ui,
                        (icons::FLAG, "Spawn"),
                        "Insert a SpawnLocation, a Team, a Checkpoint or a Camera viewpoint.",
                        &[
                            &Insertable::SPAWNING,
                            &Insertable::CHECKPOINTS,
                            &Insertable::CAMERAS,
                        ],
                        playing,
                        &mut clicks.insert,
                    );
                    insert_menu(
                        ui,
                        (icons::CAR, "Vehicle"),
                        "Insert a ready-to-drive car, a Vehicle Seat and Wheels to build one, or a \
                     Spline and a Spline Follower that carries a part along it.",
                        &[&Insertable::VEHICLES, &Insertable::SPLINES],
                        playing,
                        &mut clicks.insert,
                    );
                    insert_menu(
                        ui,
                        (icons::TREE, "Nature"),
                        "Insert a tree grown from a recipe, with its recipe and model written \
                     into the project.",
                        &[&Insertable::NATURE],
                        playing,
                        &mut clicks.insert,
                    );
                    let studios_open = [Pane::Animation, Pane::Sequencer, Pane::CharacterStudio]
                        .into_iter()
                        .any(|pane| self.layout.is_open(pane));
                    let studios = ui
                        .add(
                            RibbonButton::new(icons::FILM_SLATE, "Studios")
                                .selected(studios_open)
                                .dropdown(),
                        )
                        .on_hover_text("Open Animation, Sequencer or Character Studio panels.");
                    egui::Popup::menu(&studios)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            for pane in [Pane::Animation, Pane::Sequencer, Pane::CharacterStudio] {
                                let item = egui::Button::selectable(
                                    self.layout.is_open(pane),
                                    theme::menu_item(ui.ctx(), pane.icon(), pane.label()),
                                );
                                if ui.add(item).clicked() {
                                    self.layout.toggle(pane);
                                }
                            }
                        });
                    let scene_open = [Pane::Levels, Pane::Layers, Pane::Tags]
                        .into_iter()
                        .any(|pane| self.layout.is_open(pane));
                    let scene = ui
                        .add(
                            RibbonButton::new(icons::STACK, "Scene")
                                .selected(scene_open)
                                .dropdown(),
                        )
                        .on_hover_text("Open Levels, Layers or Tags panels.");
                    egui::Popup::menu(&scene)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            for pane in [Pane::Levels, Pane::Layers, Pane::Tags] {
                                let item = egui::Button::selectable(
                                    self.layout.is_open(pane),
                                    theme::menu_item(ui.ctx(), pane.icon(), pane.label()),
                                );
                                if ui.add(item).clicked() {
                                    self.layout.toggle(pane);
                                }
                            }
                        });
                })
            }
            RibbonGroup::Solid => {
                ribbon::group_tinted(ui, "Solid Modeling", Some(group.hue()), |ui| {
                    use crate::app::solid::SolidAction;
                    let parts = self.solid_parts();
                    for action in SolidAction::ALL {
                        let usable = action.usable(&self.world, &parts);
                        let button = RibbonButton::new(action.icon(), action.label());
                        if ui
                            .add_enabled(!playing && usable, button)
                            .on_hover_text(action.hover())
                            .on_disabled_hover_text(if playing {
                                "Press Stop to edit."
                            } else {
                                action.needs()
                            })
                            .clicked()
                        {
                            clicks.solid = Some(action);
                        }
                    }
                })
            }
            RibbonGroup::Constraints => {
                ribbon::group_tinted(ui, "Constraints", Some(group.hue()), |ui| {
                    let can_weld =
                        !crate::constraints::weld_pairs(&self.world, &self.tool_parts()).is_empty();
                    let button = RibbonButton::new(icons::LINK_SIMPLE, "Weld");
                    clicks.weld = ui
                        .add_enabled(!playing && can_weld, button)
                        .on_hover_text(
                            "Weld the selected parts to the largest of them, where they are now: \
                         unanchored, they move as one body in the game.",
                        )
                        .on_disabled_hover_text(if playing {
                            "Press Stop to edit."
                        } else {
                            "Select two or more parts to weld."
                        })
                        .clicked();
                    let active = self.constraint_tool.map(|tool| tool.class);
                    let icon = active.map_or(icons::DOOR_OPEN, theme::class_icon);
                    let button = RibbonButton::new(icon, "Constraint").selected(active.is_some());
                    let button = ui
                    .add_enabled(!playing, button)
                    .on_hover_text(
                        "Join two parts with a hinge, ball socket, slider, spring, rope or rod: \
                         pick one, click a surface on one part, then a surface on another. \
                         Or push and turn a part with a mover: click a surface on it (an \
                         align then takes a surface on the part it follows). The \
                         Attachments and the constraint are one undo step. Esc stops.",
                    )
                    .on_disabled_hover_text("Press Stop to edit.");
                    egui::Popup::menu(&button).show(|ui| {
                        let groups = [&crate::constraints::JOINTS[..], &crate::constraints::MOVERS];
                        for (i, classes) in groups.into_iter().enumerate() {
                            if i > 0 {
                                ui.separator();
                            }
                            for &class in classes {
                                let label = theme::menu_item(
                                    ui.ctx(),
                                    theme::class_icon(class),
                                    class.to_string(),
                                );
                                let item = egui::Button::selectable(active == Some(class), label);
                                if ui.add(item).clicked() {
                                    clicks.constraint = Some(Some(class));
                                }
                            }
                        }
                        if active.is_some() && ui.button("Stop").clicked() {
                            clicks.constraint = Some(None);
                        }
                    });
                    for what in Insertable::CONSTRAINTS {
                        let button = RibbonButton::new(theme::insert_icon(what), what.label());
                        if ui
                            .add_enabled(!playing, button)
                            .on_hover_text(insert_hint(what))
                            .on_disabled_hover_text("Press Stop to edit.")
                            .clicked()
                        {
                            clicks.insert = Some(what);
                        }
                    }
                })
            }
            RibbonGroup::Import => ribbon::group_tinted(ui, "Models", Some(group.hue()), |ui| {
                let button = RibbonButton::new(icons::FILE_ARROW_DOWN, "Import…");
                clicks.import = ui
                    .add_enabled(!playing, button)
                    .on_hover_text(IMPORT_HOVER)
                    .on_disabled_hover_text("Press Stop to edit.")
                    .clicked();
                let studio = RibbonButton::new(Pane::ModelStudio.icon(), "Model Studio")
                    .selected(self.layout.is_open(Pane::ModelStudio));
                if ui
                    .add_enabled(can_play, studio)
                    .on_hover_text(
                        "Open the Model Studio: make a shape and model it, unwrap, paint, sculpt and rig it, then save it as a .glb.",
                    )
                    .on_disabled_hover_text("Open a project first.")
                    .clicked()
                {
                    self.layout.open(Pane::ModelStudio);
                }
                for (pane, hint) in [
                    (
                        Pane::Material,
                        "Show or hide the Material editor panel for .material.json files.",
                    ),
                    (
                        Pane::Toolbox,
                        "Show or hide the Toolbox panel: browse online assets and the asset library.",
                    ),
                ] {
                    let btn = RibbonButton::new(pane.icon(), pane.label())
                        .selected(self.layout.is_open(pane));
                    if ui.add(btn).on_hover_text(hint).clicked() {
                        self.layout.toggle(pane);
                    }
                }
            }),
            RibbonGroup::Ui => ribbon::group_tinted(ui, "UI", Some(group.hue()), |ui| {
                let screen = Insertable::ScreenGui;
                let button = RibbonButton::new(theme::insert_icon(screen), "Screen");
                if ui
                    .add_enabled(!playing, button)
                    .on_hover_text(insert_hint(screen))
                    .on_disabled_hover_text("Press Stop to edit.")
                    .clicked()
                {
                    clicks.insert = Some(screen);
                }
                insert_menu(
                    ui,
                    (icons::SQUARES_FOUR, "Layout"),
                    "Insert a Frame, a scrolling frame, pages, a table or a BillboardGui.",
                    &[
                        &[
                            Insertable::Frame,
                            Insertable::ScrollingFrame,
                            Insertable::Table,
                        ],
                        &[Insertable::PageLayout, Insertable::Page],
                        &[Insertable::BillboardGui],
                    ],
                    playing,
                    &mut clicks.insert,
                );
                insert_menu(
                    ui,
                    (icons::TEXT_T, "Elements"),
                    "Insert text, buttons, images, inputs and bars.",
                    &[
                        &[
                            Insertable::TextLabel,
                            Insertable::TextButton,
                            Insertable::ImageLabel,
                        ],
                        &[
                            Insertable::TextBox,
                            Insertable::Slider,
                            Insertable::Toggle,
                            Insertable::Dropdown,
                        ],
                        &[Insertable::ProgressBar, Insertable::HealthBar],
                    ],
                    playing,
                    &mut clicks.insert,
                );
                let button = RibbonButton::new(icons::GAME_CONTROLLER, "Controls");
                clicks.insert_controls = ui
                    .add_enabled(!playing && can_play, button)
                    .on_hover_text(
                        "Insert a ready-made Controls menu: a ScreenGui listing the input \
                         map's actions, and a LocalScript that rebinds one to the next key \
                         or button pressed after its button is clicked. In the game, Menu \
                         (Escape, or Start on a controller) shows it.",
                    )
                    .on_disabled_hover_text("Open a project, and press Stop to edit.")
                    .clicked();
                let designer = RibbonButton::new(Pane::UiDesigner.icon(), Pane::UiDesigner.label())
                    .selected(self.layout.is_open(Pane::UiDesigner));
                if ui
                    .add(designer)
                    .on_hover_text("Show or hide the UI Designer artboard and inspector.")
                    .clicked()
                {
                    self.layout.toggle(Pane::UiDesigner);
                }
            }),
            RibbonGroup::Scripts => ribbon::group_tinted(ui, "Scripts", Some(group.hue()), |ui| {
                for class in explorer::SCRIPT_CLASSES {
                    let button = RibbonButton::new(theme::class_icon(class), class.as_str());
                    if ui
                        .add_enabled(!playing && can_play, button)
                        .on_hover_text(explorer::script_hover(class))
                        .on_disabled_hover_text(if playing {
                            "Press Stop to edit."
                        } else {
                            "Open a project first."
                        })
                        .clicked()
                    {
                        clicks.insert_script = Some(class);
                    }
                }
                for (pane, hint) in [
                    (
                        Pane::BehaviorTree,
                        "Show or hide the Behavior Tree graph editor.",
                    ),
                    (Pane::Dialogue, "Show or hide the Dialogue graph editor."),
                    (
                        Pane::DataTable,
                        "Show or hide the Data Table spreadsheet editor.",
                    ),
                ] {
                    let btn = RibbonButton::new(pane.icon(), pane.label())
                        .selected(self.layout.is_open(pane));
                    if ui.add(btn).on_hover_text(hint).clicked() {
                        self.layout.toggle(pane);
                    }
                }
            }),
            RibbonGroup::Terrain => ribbon::group_tinted(ui, "Terrain", Some(group.hue()), |ui| {
                use crate::terrain_studio::TerrainTool;
                let editor = RibbonButton::new(icons::MOUNTAINS, "Terrain")
                    .selected(self.layout.is_open(Pane::Terrain));
                if ui
                    .add_enabled(can_play, editor)
                    .on_hover_text(
                        "Open the Terrain Studio: make, sculpt, paint and generate terrain.",
                    )
                    .on_disabled_hover_text("Open a project first.")
                    .clicked()
                {
                    self.layout.open(Pane::Terrain);
                }
                let active = self.terrain.tool;
                let icon = active.map_or(icons::PAINT_BRUSH_BROAD, TerrainTool::icon);
                let button = RibbonButton::new(icon, "Brush")
                    .selected(active.is_some())
                    .dropdown();
                let button = ui
                    .add_enabled(!playing && can_play, button)
                    .on_hover_text("Pick a terrain tool: sculpt, paint or generate.")
                    .on_disabled_hover_text(if playing {
                        "Press Stop to edit."
                    } else {
                        "Open a project first."
                    });
                egui::Popup::menu(&button).show(|ui| {
                    for tool in [
                        TerrainTool::Raise,
                        TerrainTool::Paint,
                        TerrainTool::Water,
                        TerrainTool::Generate,
                    ] {
                        let item = egui::Button::selectable(
                            active == Some(tool),
                            theme::menu_item(ui.ctx(), tool.icon(), tool.label()),
                        );
                        if ui.add(item).on_hover_text(tool.hint()).clicked() {
                            self.pick_terrain_tool(tool);
                        }
                    }
                });
                let scatter = RibbonButton::new(Pane::Scatter.icon(), "Scatter")
                    .selected(self.layout.is_open(Pane::Scatter));
                if ui
                    .add_enabled(can_play, scatter)
                    .on_hover_text(
                        "Open the Scatter pane: choose props and models, how big each is, its share of the mix and whether it collides, then paint them onto terrain.",
                    )
                    .on_disabled_hover_text("Open a project first.")
                    .clicked()
                {
                    self.open_scatter();
                }
            }),
            RibbonGroup::Panels => ribbon::group_tinted(ui, "Panels", Some(group.hue()), |ui| {
                for pane in Pane::CORE {
                    let button = RibbonButton::new(pane.icon(), pane.label())
                        .selected(self.layout.is_open(pane));
                    if ui
                        .add(button)
                        .on_hover_text(format!(
                            "Show or hide the {} panel. Drag a panel by its tab to move it. \
                             More panels are on their matching ribbon tabs and under View ▸ Panels.",
                            pane.label()
                        ))
                        .clicked()
                    {
                        self.layout.toggle(pane);
                    }
                }
            }),
            RibbonGroup::Overlays => ribbon::group_tinted(ui, "View", Some(group.hue()), |ui| {
                let cube =
                    RibbonButton::new(icons::CUBE_FOCUS, "View Cube").selected(self.view_cube);
                if ui
                    .add(cube)
                    .on_hover_text(
                        "Show or hide the view cube in the viewport's corner: click a face, \
                         edge or corner to look from there, or the house for the starting view.",
                    )
                    .clicked()
                {
                    self.view_cube = !self.view_cube;
                }
                let grid = RibbonButton::new(icons::GRID_NINE, "Grid").selected(self.grid);
                if ui
                    .add(grid)
                    .on_hover_text(
                        "Show or hide the measuring grid on the baseplate, with its distances \
                         in studs.",
                    )
                    .clicked()
                {
                    self.grid = !self.grid;
                }
                let any = self.overlays.any() || self.light_cones;
                let button = RibbonButton::new(icons::STACK_SIMPLE, "Overlays")
                    .selected(any)
                    .dropdown();
                let button = ui.add(button).on_hover_text(
                    "Debug drawings over the viewport: colliders, raycasts, the nav mesh and more.",
                );
                egui::Popup::menu(&button)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| self.overlay_items(ui));
                let mode = RibbonButton::new(icons::EYE, "View Mode")
                    .selected(self.view_mode != ViewMode::Lit)
                    .dropdown();
                let mode = ui
                    .add(mode)
                    .on_hover_text("Show the scene lit, or one of the renderer's debug views.");
                egui::Popup::menu(&mode).show(|ui| {
                    for mode in ViewMode::all() {
                        let item = egui::Button::selectable(
                            self.view_mode == mode,
                            theme::menu_entry(ui.ctx(), None, mode.label()),
                        );
                        if ui.add(item).on_hover_text(mode.hint()).clicked() {
                            self.view_mode = mode;
                        }
                    }
                });
            }),
        }
    }
}

impl EditorApp {
    /// The overlay toggles, for the ribbon's Overlays menu and the menu
    /// bar's Viewport menu.
    pub(in crate::app) fn overlay_items(&mut self, ui: &mut egui::Ui) {
        let overlays = &mut self.overlays;
        let items: [(&mut bool, &'static str, &'static str, &'static str); 13] = [
            (
                &mut overlays.colliders,
                icons::CUBE,
                "Colliders",
                "Draw the shape each part actually collides with, including sensors and terrain chunks.",
            ),
            (
                &mut overlays.raycasts,
                icons::LINE_SEGMENT,
                "Raycasts",
                "Draw recent workspace:Raycast calls during Play.",
            ),
            (
                &mut overlays.bounds,
                icons::SQUARE,
                "Bounds",
                "Draw each part's Size box.",
            ),
            (
                &mut overlays.emitters,
                icons::SPEAKER_HIGH,
                "Emitters",
                "Mark where each spatial Sound is heard from.",
            ),
            (
                &mut overlays.nav_mesh,
                icons::FOOTPRINTS,
                "Nav Mesh",
                "Show where characters can walk. Each colour is ground that connects; it rebuilds after you move parts or sculpt terrain.",
            ),
            (
                &mut overlays.agent_paths,
                icons::PATH,
                "Agent Paths",
                "During Play, draw the path each NavigationAgent is walking, from its feet to its Destination.",
            ),
            (
                &mut overlays.constraints,
                icons::LINK_SIMPLE,
                "Constraints",
                "Draw each WeldConstraint as a line between its parts (grey when disabled) and every Attachment as a small gizmo.",
            ),
            (
                &mut overlays.contacts,
                icons::ARROWS_IN_LINE_VERTICAL,
                "Contacts",
                "During Play, mark where parts touch, with a line the way each contact pushes, longer the harder it pushes.",
            ),
            (
                &mut overlays.bodies,
                icons::CROSSHAIR,
                "Bodies",
                "During Play, mark each moving part's (or welded group's) center of mass: yellow while it's simulated, blue with a z once it's asleep.",
            ),
            (
                &mut overlays.streaming,
                icons::BROADCAST,
                "Streaming",
                "With Workspace.StreamingEnabled on, show what streams around the camera, while editing too: each unit's box (green there at once, blue loading, grey kept if loaded), what's always there in orange, both radii, and how much a player standing here has.",
            ),
            (
                &mut overlays.light_probes,
                icons::LIGHTBULB,
                "Light Probes",
                "Show every light probe volume's probes as discs filled with the light their bake gives a surface facing you, so dark or leaking probes stand out. Volumes are outlined green when baked, orange when the world has changed since, grey before their first bake and red when the bake file is missing.",
            ),
            (
                &mut overlays.culling,
                icons::FRAME_CORNERS,
                "Culling",
                "Hold what's drawn to where the camera is now, then fly away to see it: each part's bounds in green where it's drawn and red where it's culled, and the view it was held at.",
            ),
            (
                &mut overlays.skeletons,
                icons::BONE,
                "Skeletons",
                "Draw each rigged MeshPart's bones: where the animation has them during Play and in the Sequencer's preview, at rest while editing. The selected one's are brighter.",
            ),
        ];
        for (on, icon, label, hint) in items {
            overlay_menu_item(ui, on, icon, label, hint);
        }
        ui.separator();
        overlay_menu_item(
            ui,
            &mut self.light_cones,
            icons::FLASHLIGHT,
            "Light Cones",
            "Show where every light shines. Off, only the selected light (or the part it's in) shows its cone.",
        );
        overlay_menu_item(
            ui,
            &mut overlays.stream_editing,
            icons::BROADCAST,
            "Stream While Editing",
            "With Workspace.StreamingEnabled on, leave out what's beyond StreamingTargetRadius of the camera while editing, as in Play, so a big world edits as light as it plays. What's selected always shows.",
        );
    }

    /// Beside Play and Stop (Phases 64 and 67): how many clients Play
    /// runs, the network emulated between them, and, while several play,
    /// which one the viewport shows.
    fn play_settings_controls(&mut self, ui: &mut egui::Ui, playing: bool) {
        let palette = theme::palette(ui.ctx());
        let density = theme::density(ui.ctx());
        let compact = density == theme::Density::Compact;
        let height = ribbon::button_height(density);

        // Players count card: matches the ribbon's button height and icon-over-caption rhythm.
        let mut clients = self.play_settings.clients;
        egui::Frame::new()
            .fill(palette.surface)
            .stroke(egui::Stroke::new(1.0_f32, palette.border))
            .corner_radius(theme::radius::CONTROL)
            .inner_margin(egui::Margin::symmetric(8, if compact { 2 } else { 4 }))
            .show(ui, |ui| {
                let inner_h = (height - if compact { 4.0 } else { 8.0 }).max(18.0);
                if compact {
                    ui.horizontal_centered(|ui| {
                        ui.set_height(inner_h);
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(
                            egui::RichText::new(icons::USERS)
                                .size(14.0)
                                .color(if playing {
                                    palette.text_muted
                                } else {
                                    palette.text_secondary
                                }),
                        );
                        ui.add_enabled(
                            !playing,
                            egui::DragValue::new(&mut clients)
                                .range(1..=crate::play::multi::MAX_CLIENTS),
                        )
                        .on_hover_text(
                            "How many players Play runs, each a client with its own world. The \
                             first plays on the server; the others join it, as players on other \
                             computers do.",
                        )
                        .on_disabled_hover_text("Stop the game to change how many clients play.");
                    });
                } else {
                    ui.allocate_ui_with_layout(
                        egui::vec2(62.0, inner_h),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.label(egui::RichText::new(icons::USERS).size(15.0).color(
                                    if playing {
                                        palette.text_muted
                                    } else {
                                        palette.text_secondary
                                    },
                                ));
                                ui.add_enabled(
                                    !playing,
                                    egui::DragValue::new(&mut clients)
                                        .range(1..=crate::play::multi::MAX_CLIENTS),
                                )
                                .on_hover_text(
                                    "How many players Play runs, each a client with its own \
                                     world. The first plays on the server; the others join it, as \
                                     players on other computers do.",
                                )
                                .on_disabled_hover_text(
                                    "Stop the game to change how many clients play.",
                                );
                            });
                            ui.label(
                                egui::RichText::new("Players")
                                    .size(theme::type_scale::CAPTION)
                                    .color(palette.text_secondary),
                            );
                        },
                    );
                }
            });
        self.play_settings.clients = clients;

        // While multiple clients play, show a matching Viewing selector card beside Players.
        if let Some(play) = &mut self.play
            && play.clients() > 1
        {
            let views = play.views();
            let shown = play.view();
            let name = views
                .iter()
                .find(|(view, _)| *view == shown)
                .map(|(_, name)| name.clone())
                .unwrap_or_default();
            egui::Frame::new()
                .fill(palette.surface)
                .stroke(egui::Stroke::new(1.0_f32, palette.border))
                .corner_radius(theme::radius::CONTROL)
                .inner_margin(egui::Margin::symmetric(8, if compact { 2 } else { 4 }))
                .show(ui, |ui| {
                    let inner_h = (height - if compact { 4.0 } else { 8.0 }).max(18.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(92.0, inner_h),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            egui::ComboBox::from_id_salt("play view")
                                .width(86.0)
                                .selected_text(name)
                                .show_ui(ui, |ui| {
                                    for (view, name) in views {
                                        if ui.selectable_label(view == shown, name).clicked() {
                                            play.set_view(view);
                                        }
                                    }
                                })
                                .response
                                .on_hover_text(
                                    "Which device the viewport, Explorer and Properties show: the \
                                     server, from the fly camera, or a client's game, which gets \
                                     the keys and mouse.",
                                );
                            if !compact {
                                ui.label(
                                    egui::RichText::new("Viewing")
                                        .size(theme::type_scale::CAPTION)
                                        .color(palette.text_secondary),
                                );
                            }
                        },
                    );
                });
        }

        // Network emulation dropdown matching the ribbon's dropdown buttons (Overlays, View Mode).
        let conditions = self.play_settings.conditions;
        let emulating =
            !conditions.latency.is_zero() || !conditions.jitter.is_zero() || conditions.loss > 0.0;
        let network_tip = if emulating {
            format!(
                "Network emulation active: {:.0} ms latency, {:.0} ms jitter, {:.0}% packet loss.",
                conditions.latency.as_secs_f64() * 1000.0,
                conditions.jitter.as_secs_f64() * 1000.0,
                conditions.loss * 100.0,
            )
        } else {
            "Emulate latency, jitter, and packet loss between Play's clients (2 or more)."
                .to_owned()
        };
        let network_btn = ui
            .add(
                RibbonButton::new(icons::WIFI_HIGH, "Network")
                    .selected(emulating)
                    .dropdown(),
            )
            .on_hover_text(network_tip);
        egui::Popup::menu(&network_btn)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(230.0);
                let mut conditions = self.play_settings.conditions;
                ui.label(widgets::micro("Presets", palette.text_weak));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    for (label, lat_ms, jit_ms, loss_pct) in [
                        ("Ideal", 0.0, 0.0, 0.0),
                        ("Wi-Fi", 40.0, 8.0, 0.5),
                        ("4G", 120.0, 25.0, 2.0),
                        ("Lossy", 250.0, 60.0, 8.0),
                    ] {
                        let active = (conditions.latency.as_secs_f64() * 1000.0 - lat_ms).abs()
                            < 0.5
                            && (conditions.jitter.as_secs_f64() * 1000.0 - jit_ms).abs() < 0.5
                            && (conditions.loss * 100.0 - loss_pct).abs() < 0.1;
                        if widgets::chip(ui, label, active).clicked() {
                            conditions.latency =
                                std::time::Duration::from_secs_f64(lat_ms / 1000.0);
                            conditions.jitter = std::time::Duration::from_secs_f64(jit_ms / 1000.0);
                            conditions.loss = loss_pct / 100.0;
                        }
                    }
                });
                ui.separator();
                let mut latency = conditions.latency.as_secs_f64() * 1000.0;
                let mut jitter = conditions.jitter.as_secs_f64() * 1000.0;
                let mut loss = conditions.loss * 100.0;
                crate::responsive::form(ui, "play network", |ui| {
                    ui.label("Latency");
                    ui.add(
                        egui::DragValue::new(&mut latency)
                            .range(0.0..=1000.0)
                            .suffix(" ms"),
                    )
                    .on_hover_text("Added to every packet each way between the clients.");
                    ui.end_row();
                    ui.label("Jitter");
                    ui.add(
                        egui::DragValue::new(&mut jitter)
                            .range(0.0..=500.0)
                            .suffix(" ms"),
                    )
                    .on_hover_text("Each packet is up to this much earlier or later.");
                    ui.end_row();
                    ui.label("Packet loss");
                    ui.add(
                        egui::DragValue::new(&mut loss)
                            .range(0.0..=50.0)
                            .suffix(" %"),
                    )
                    .on_hover_text("This share of packets never arrives.");
                    ui.end_row();
                });
                ui.weak("Applies between Play's clients when 2 or more run.");
                conditions.latency = std::time::Duration::from_secs_f64(latency / 1000.0);
                conditions.jitter = std::time::Duration::from_secs_f64(jitter / 1000.0);
                conditions.loss = loss / 100.0;
                if conditions != self.play_settings.conditions {
                    self.play_settings.conditions = conditions;
                    if let Some(play) = &mut self.play {
                        play.emulate(conditions);
                    }
                }
            });

        // Pause on errors toggle matching View Cube / Grid / Profiler ribbon toggles.
        let pause_on_error = self.play_settings.pause_on_error;
        if ui
            .add(RibbonButton::new(icons::BUG, "Pause on errors").selected(pause_on_error))
            .on_hover_text(
                "A script error pauses Play where it happened, with that script's variables, \
                 instead of only being shown in Output.",
            )
            .clicked()
        {
            self.set_pause_on_error(!pause_on_error);
        }
    }
}

/// One of Play, Run and Stop: how it looks, and whether it can be pressed.
struct PlayButton {
    icon: &'static str,
    label: &'static str,
    hover: String,
    color: egui::Color32,
    enabled: bool,
    disabled_hover: &'static str,
}
