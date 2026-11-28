//! The Graphics page: this computer's preset, its overrides grouped by what
//! they draw, what each costs a frame, and what's in use once the GPU's
//! limits are applied (`docs/rendering/settings.md`).

use super::*;
use crate::responsive;

/// What a setting costs a frame, as its tag beside the control says.
/// Measured on an RX 9070 XT (the hover note says where); the tags inform
/// and never change a preset's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cost {
    /// No frame time measured (it may take video memory).
    None,
    /// Under about half a millisecond.
    Low,
    /// About half a millisecond to two.
    Medium,
    /// Over two milliseconds, or heavy work on the CPU.
    High,
}

impl Cost {
    fn label(self) -> &'static str {
        match self {
            Cost::None => "Free",
            Cost::Low => "Low",
            Cost::Medium => "Medium",
            Cost::High => "High",
        }
    }

    fn meaning(self) -> &'static str {
        match self {
            Cost::None => "No frame time measured",
            Cost::Low => "Low cost: under about 0.5 ms a frame",
            Cost::Medium => "Medium cost: about 0.5 to 2 ms a frame",
            Cost::High => "High cost: over 2 ms a frame, or heavy CPU work",
        }
    }

    fn color(self, palette: &Palette) -> egui::Color32 {
        match self {
            Cost::None => palette.text_weak,
            Cost::Low => palette.success,
            Cost::Medium => palette.warning,
            Cost::High => palette.error,
        }
    }
}

/// The rows in the Advanced fold, which search opens to show one.
const ADVANCED: &[&str] = &[
    "Shadow distance",
    "Far shadow detail",
    "Contact shadows",
    "Texture quality",
    "Texture memory",
    "Ray tracing distance",
    "Ray traced shadow resolution",
    "Real-time GI (raster)",
    "Vignette",
    "Film grain",
    "Chromatic aberration",
    "Sharpen",
    "Sun rays",
];

/// This computer's graphics settings: a preset, its overrides, and what's
/// in use once the GPU's limits are applied, with why anything was lowered.
/// True when Restart now was clicked.
///
/// `drawn` is what the viewport's last frame held on the graphics card: a
/// world that needs more than the memory budgets is said so under the
/// quality, where it's seen without scrolling
/// (`RenderStats::memory_warnings`).
pub(super) fn graphics_page(
    ui: &mut egui::Ui,
    settings: &mut GraphicsSettings,
    capabilities: &Capabilities,
    drawn: Option<&axcenity_render::RenderStats>,
) -> bool {
    #[cfg(test)]
    for key in [DRAWN_ROWS, DRAWN_COSTS] {
        ui.data_mut(|data| data.remove::<Vec<String>>(egui::Id::new(key)));
    }
    page_title(
        ui,
        "Graphics",
        "This computer's quality, for the viewport and Play. Not saved in projects.",
    );
    caption(
        ui,
        "Each tag says what a setting costs a frame, measured on an RX 9070 XT; hover it \
         for the numbers.",
    );
    ui.add_space(space::S);
    let palette = theme::palette(ui.ctx());
    section(ui, "Quality");
    widgets::segmented(
        ui,
        &mut settings.quality,
        &Quality::ALL.map(|quality| (quality, None, quality.label())),
    );
    let preset = GraphicsSettings::preset(settings.quality);
    let effective = settings.effective(capabilities);
    // What the world in the viewport needs past the memory budgets.
    let memory = drawn
        .map(|stats| stats.memory_warnings(&effective))
        .unwrap_or_default();
    if !memory.is_empty() {
        ui.add_space(space::S);
        for warning in &memory {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(theme::with_icon(icons::WARNING, warning))
                        .size(type_scale::SMALL)
                        .color(palette.warning),
                )
                .wrap(),
            );
        }
    }

    ui.add_space(space::L);
    section(ui, "Resolution");
    responsive::form(ui, "graphics resolution", |ui| {
        row(
            ui,
            "Render scale",
            Cost::High,
            "The pixels drawn grow with the square of the scale. Desert Homestead's GPU frame \
             was 3.75 ms plus 1.09 ms for each million pixels (2560 × 1405 is 3.6 million).",
            |ui| {
                custom_value(
                    ui,
                    &mut settings.render_scale,
                    preset.render_scale(),
                    "Off, the preset's scale is used.",
                    (
                        format!("Preset ({}%)", (preset.render_scale() * 100.0).round()),
                        None,
                    ),
                    |ui, scale| {
                        let mut percent = (*scale * 100.0).round();
                        let range = RENDER_SCALE_RANGE;
                        if ui
                            .add(
                                egui::DragValue::new(&mut percent)
                                    .range(range.start() * 100.0..=range.end() * 100.0)
                                    .speed(1.0)
                                    .suffix("%"),
                            )
                            .changed()
                        {
                            *scale = percent / 100.0;
                        }
                    },
                );
            },
        );
        row(
            ui,
            "Anti-aliasing",
            Cost::Medium,
            "MSAA 4× cost 0.7 to 1.5 ms at 2560 × 1440 (Dry Patch); FXAA and TAA are one \
             cheap pass. TAA suits ray tracing, which it smooths over frames.",
            |ui| {
                choice(
                    ui,
                    "anti-aliasing",
                    &mut settings.anti_aliasing,
                    preset.anti_aliasing(),
                    &AntiAliasing::ALL,
                    named(AntiAliasing::label),
                );
            },
        );
    });

    ui.add_space(space::L);
    section(ui, "Lighting");
    responsive::form(ui, "graphics lighting", |ui| {
        row(
            ui,
            "Shadow quality",
            Cost::High,
            "The largest raster pass. At 2560 × 1440 (Dry Patch) no shadows at all saved 4.4 \
             to 5.0 ms, and High's maps instead of Ultra's saved 2.6 to 2.9 ms.",
            |ui| {
                choice(
                    ui,
                    "shadow quality",
                    &mut settings.shadow_quality,
                    preset.shadow_quality(),
                    &ShadowQuality::ALL,
                    named(ShadowQuality::label),
                );
            },
        );
        row(
            ui,
            "Ambient occlusion",
            Cost::Low,
            "Screen-space ambient occlusion took 0.1 to 0.2 ms at 2560 × 1440 (Dry Patch).",
            |ui| {
                choice(
                    ui,
                    "ambient occlusion",
                    &mut settings.ambient_occlusion,
                    preset.ambient_occlusion(),
                    &AmbientOcclusion::ALL,
                    named(AmbientOcclusion::label),
                );
            },
        );
        row(
            ui,
            "Global illumination",
            Cost::Low,
            "Baked probes cost nothing a frame. Ray traced GI added 0 to 2 fps on top of \
             traced shadows and ambient occlusion in Desert Homestead; not measured in other \
             worlds, nor traced in software.",
            |ui| {
                let realtime = settings.realtime_gi();
                let label = |gi: RayTracedGlobalIllumination| match gi {
                    RayTracedGlobalIllumination::Off if realtime => "Real-time probes".to_owned(),
                    RayTracedGlobalIllumination::Off => "Baked probes".to_owned(),
                    gi => format!("Ray traced {}", gi.label()),
                };
                // Without ray queries its rays are traced in software (RL3).
                let how = if capabilities.ray_queries {
                    "Traced with hardware ray tracing."
                } else if capabilities.compute_shaders {
                    "This GPU traces it in software (compute shaders), without hardware ray \
                     tracing."
                } else {
                    "Ray traced GI needs compute shaders, which this GPU lacks."
                };
                ui.add_enabled_ui(
                    capabilities.ray_queries || capabilities.compute_shaders,
                    |ui| {
                        choice(
                            ui,
                            "global illumination",
                            &mut settings.ray_traced_global_illumination,
                            preset.ray_traced_global_illumination(),
                            &RayTracedGlobalIllumination::ALL,
                            label,
                        )
                        .on_hover_text(format!(
                            "Bounced light from light probes around the camera and in \
                             LightProbeVolumes. Baked keeps each volume's bake; ray traced \
                             traces them again as the light changes. {how}"
                        ));
                    },
                );
            },
        );
    });

    ui.add_space(space::L);
    section(ui, "Reflections");
    responsive::form(ui, "graphics reflections", |ui| {
        row(
            ui,
            "Sky and probe reflections",
            Cost::None,
            "No frame time measured: the maps are made again only when the sky or a probe \
             changes. Higher qualities take more video memory (Ultra's are 256² a face).",
            |ui| {
                choice(
                    ui,
                    "reflection quality",
                    &mut settings.reflection_quality,
                    preset.reflection_quality(),
                    &ReflectionQuality::ALL,
                    named(ReflectionQuality::label),
                )
                .on_hover_text(
                    "The sky's environment maps and reflection probes. Low reflects the sky's \
                     gradient only.",
                );
            },
        );
        row(
            ui,
            "Screen-space reflections",
            Cost::Medium,
            "About 1 ms at 2560 × 1440 in Overgrown Street.",
            |ui| {
                let replaced = effective.ray_traced_reflections != RayTracedReflections::Off;
                ui.add_enabled_ui(!replaced, |ui| {
                    choice(
                        ui,
                        "screen space reflections",
                        &mut settings.screen_space_reflections,
                        preset.screen_space_reflections(),
                        &[true, false],
                        on_off,
                    )
                    .on_hover_text("Smooth and wet surfaces reflect what's on the screen.");
                });
                if replaced {
                    note(ui, "Replaced by ray traced reflections");
                }
            },
        );
    });

    ui.add_space(space::L);
    section(ui, "Atmosphere");
    responsive::form(ui, "graphics atmosphere", |ui| {
        row(
            ui,
            "Clouds",
            Cost::Low,
            "0.14 to 0.30 ms at 1920 × 1080 on Ultra, traced at half size first.",
            |ui| {
                choice(
                    ui,
                    "cloud quality",
                    &mut settings.cloud_quality,
                    preset.cloud_quality(),
                    &CloudQuality::ALL,
                    named(CloudQuality::label),
                )
                .on_hover_text(
                    "How detailed a world's volumetric clouds are. Low is a flat sheet.",
                );
            },
        );
        row(
            ui,
            "Volumetric fog",
            Cost::Low,
            "0.38 ms in Desert Homestead at 1920 × 1080.",
            |ui| {
                choice(
                    ui,
                    "volumetric fog",
                    &mut settings.volumetric_fog,
                    preset.volumetric_fog(),
                    &[true, false],
                    on_off,
                )
                .on_hover_text("Sun shafts and lamp light in an Atmosphere's fog.");
            },
        );
    });

    ui.add_space(space::L);
    let restart = ray_tracing_section(ui, settings, capabilities, &preset);

    ui.add_space(space::L);
    section(ui, "Camera effects");
    caption(
        ui,
        "Only drawn when the world uses them (its Lighting, or an effect such as \
         DepthOfFieldEffect).",
    );
    responsive::form(ui, "graphics camera effects", |ui| {
        row(
            ui,
            "Bloom and tone mapping",
            Cost::Low,
            "Post-processing took 0.1 ms at 2560 × 1440 (Dry Patch).",
            |ui| {
                choice(
                    ui,
                    "post-processing",
                    &mut settings.post_processing,
                    preset.post_processing(),
                    &[true, false],
                    on_off,
                )
                .on_hover_text(
                    "Off draws no bloom or tone mapping, whatever the world's Lighting asks for.",
                );
            },
        );
        row(
            ui,
            "Depth of field",
            Cost::Low,
            "A blur pass over the screen while a DepthOfFieldEffect is on; not measured on its \
             own.",
            |ui| {
                choice(
                    ui,
                    "depth of field",
                    &mut settings.depth_of_field,
                    preset.depth_of_field(),
                    &DepthOfFieldQuality::ALL,
                    named(DepthOfFieldQuality::label),
                );
            },
        );
        row(
            ui,
            "Motion blur",
            Cost::Low,
            "A blur pass over the screen while a MotionBlurEffect is on; not measured on its \
             own.",
            |ui| {
                choice(
                    ui,
                    "motion blur",
                    &mut settings.motion_blur,
                    preset.motion_blur(),
                    &MotionBlurQuality::ALL,
                    named(MotionBlurQuality::label),
                );
            },
        );
        row(
            ui,
            "Lens effects",
            Cost::None,
            "Vignette, film grain, chromatic aberration, sharpen and sun rays are drawn in the \
             post-processing pass: nothing measurable.",
            |ui| lens_effects(ui, settings, &preset),
        );
    });

    ui.add_space(space::L);
    advanced_section(ui, settings, capabilities, &preset, &effective);
    ui.add_space(space::L);
    in_use_section(ui, settings, capabilities);
    restart
}

/// One row of a [`responsive::form`]: the name search finds it by, the
/// control, and its cost tag (`note` says what was measured).
fn row<R>(
    ui: &mut egui::Ui,
    name: &str,
    cost: Cost,
    note: &str,
    control: impl FnOnce(&mut egui::Ui) -> R,
) {
    #[cfg(test)]
    remember(ui, DRAWN_ROWS, name);
    found_label(ui, name);
    responsive::toolbar(ui, |ui| {
        control(ui);
        cost_tag(ui, name, cost, note);
    });
    ui.end_row();
}

/// The tag saying what `name` costs a frame, with `note` on hover.
fn cost_tag(ui: &mut egui::Ui, name: &str, cost: Cost, note: &str) {
    #[cfg(test)]
    remember(ui, DRAWN_COSTS, name);
    #[cfg(not(test))]
    let _ = name;
    let palette = theme::palette(ui.ctx());
    pill(
        ui,
        &theme::with_icon(icons::GAUGE, cost.label()),
        cost.color(&palette),
    )
    .on_hover_text(format!("{}. RX 9070 XT: {note}", cost.meaning()));
}

/// Where the tests read which rows the page drew, and which had a tag.
#[cfg(test)]
pub(super) const DRAWN_ROWS: &str = "graphics rows drawn";
#[cfg(test)]
pub(super) const DRAWN_COSTS: &str = "graphics costs drawn";

#[cfg(test)]
fn remember(ui: &egui::Ui, key: &str, name: &str) {
    ui.data_mut(|data| {
        data.get_temp_mut_or_default::<Vec<String>>(egui::Id::new(key))
            .push(name.to_owned())
    });
}

/// A small rounded tag of `text` tinted with `color`, one widget so a
/// [`responsive::toolbar`] can wrap it.
fn pill(ui: &mut egui::Ui, text: &str, color: egui::Color32) -> egui::Response {
    let palette = theme::palette(ui.ctx());
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::new(type_scale::CAPTION, theme::medium_family()),
        color,
    );
    let size = galley.size() + egui::vec2(14.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    if ui.is_rect_visible(rect) {
        ui.painter().rect(
            rect,
            theme::radius::CONTROL,
            theme::mix(palette.surface, color, 0.16),
            egui::Stroke::new(1.0_f32, theme::mix(palette.surface, color, 0.32)),
            egui::StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    response
}

/// A quiet line of explanation under a section.
fn caption(ui: &mut egui::Ui, text: &str) {
    let palette = theme::palette(ui.ctx());
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(type_scale::CAPTION)
                .color(palette.text_weak),
        )
        .wrap(),
    );
}

/// A short remark beside a control, such as why it does nothing now.
fn note(ui: &mut egui::Ui, text: &str) {
    let palette = theme::palette(ui.ctx());
    ui.label(
        egui::RichText::new(text)
            .size(type_scale::CAPTION)
            .color(palette.text_weak),
    );
}

/// A `label` method as a function giving a `String`, for [`choice`].
fn named<T>(label: fn(T) -> &'static str) -> impl Fn(T) -> String {
    move |value| label(value).to_owned()
}

fn on_off(on: bool) -> String {
    if on { "On" } else { "Off" }.to_owned()
}

/// A preset-or-override menu: Preset (the preset's `preset` value), then
/// each of `choices`, named by `label`. Returns the menu button's response.
fn choice<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut Option<T>,
    preset: T,
    choices: &[T],
    label: impl Fn(T) -> String,
) -> egui::Response {
    let text = |choice: Option<T>| match choice {
        Some(choice) => label(choice),
        None => format!("Preset ({})", label(preset)),
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(text(*value))
        .show_ui(ui, |ui| {
            for choice in std::iter::once(None).chain(choices.iter().copied().map(Some)) {
                ui.selectable_value(value, choice, text(choice));
            }
        })
        .response
}

/// A number that's the preset's until its checkbox is ticked: then `edit`
/// draws its own field. While it's the preset's, `shown` is what's said
/// (and why, on hover).
fn custom_value<T: Copy>(
    ui: &mut egui::Ui,
    value: &mut Option<T>,
    preset: T,
    off_tip: &str,
    shown: (String, Option<String>),
    edit: impl FnOnce(&mut egui::Ui, &mut T),
) {
    let palette = theme::palette(ui.ctx());
    let mut custom = value.is_some();
    if ui
        .checkbox(&mut custom, "")
        .on_hover_text(off_tip)
        .changed()
    {
        *value = custom.then_some(preset);
    }
    match value {
        Some(value) => edit(ui, value),
        None => {
            let (text, why) = shown;
            let label = ui.label(egui::RichText::new(text).color(palette.text_secondary));
            if let Some(why) = why {
                label.on_hover_text(why);
            }
        }
    }
}

/// One menu for all five lens effects: Preset, On or Off sets every one;
/// it reads Mixed while they differ (set apart in Advanced).
fn lens_effects(ui: &mut egui::Ui, settings: &mut GraphicsSettings, preset: &GraphicsSettings) {
    let presets = [
        preset.vignette(),
        preset.film_grain(),
        preset.chromatic_aberration(),
        preset.sharpen(),
        preset.sun_rays(),
    ];
    let mut lens = [
        &mut settings.vignette,
        &mut settings.film_grain,
        &mut settings.chromatic_aberration,
        &mut settings.sharpen,
        &mut settings.sun_rays,
    ];
    let first = *lens[0];
    let same = lens.iter().all(|value| **value == first);
    let text = |choice: Option<bool>| match choice {
        Some(on) => on_off(on),
        None if presets.iter().all(|on| *on == presets[0]) => {
            format!("Preset ({})", on_off(presets[0]))
        }
        None => "Preset (mixed)".to_owned(),
    };
    egui::ComboBox::from_id_salt("lens effects")
        .selected_text(if same {
            text(first)
        } else {
            "Mixed".to_owned()
        })
        .show_ui(ui, |ui| {
            for choice in [None, Some(true), Some(false)] {
                if ui
                    .selectable_label(same && first == choice, text(choice))
                    .clicked()
                {
                    for value in lens.iter_mut() {
                        **value = choice;
                    }
                }
            }
        })
        .response
        .on_hover_text(
            "Vignette, film grain, chromatic aberration, sharpen and sun rays at once. Each \
             is set apart under Advanced.",
        );
}

/// Finer controls the preset sets, folded away unless search asks for one.
fn advanced_section(
    ui: &mut egui::Ui,
    settings: &mut GraphicsSettings,
    capabilities: &Capabilities,
    preset: &GraphicsSettings,
    effective: &EffectiveGraphics,
) {
    let sought = sought_setting(ui.ctx()).is_some_and(|name| ADVANCED.contains(&name));
    egui::CollapsingHeader::new("Advanced")
        .id_salt("graphics advanced")
        .default_open(false)
        .open(sought.then_some(true))
        .show(ui, |ui| {
            caption(
                ui,
                "Finer controls the preset sets. Most change little on their own.",
            );
            ui.add_space(space::XS);
            responsive::form(ui, "graphics advanced", |ui| {
                advanced_rows(ui, settings, capabilities, preset, effective);
            });
        });
}

fn advanced_rows(
    ui: &mut egui::Ui,
    settings: &mut GraphicsSettings,
    capabilities: &Capabilities,
    preset: &GraphicsSettings,
    effective: &EffectiveGraphics,
) {
    row(
        ui,
        "Shadow distance",
        Cost::Low,
        "Shorter isn't cheaper: halving it made Dry Patch's shadows 0.4 to 1.4 ms slower, \
         since the same casters land in denser cascades.",
        |ui| {
            custom_value(
                ui,
                &mut settings.shadow_distance,
                preset.shadow_distance(),
                "Off, the preset's distance is used.",
                (format!("Preset ({} studs)", preset.shadow_distance()), None),
                |ui, distance| {
                    ui.add(
                        egui::DragValue::new(distance)
                            .range(0.0..=5000.0)
                            .speed(1.0)
                            .suffix(" studs"),
                    );
                },
            );
        },
    );
    row(
        ui,
        "Far shadow detail",
        Cost::Low,
        "The farthest shadow cascade draws each tree or model at the level of detail its \
         size in that map calls for. On saves GPU time in dense scenes; off draws it as the \
         camera sees it.",
        |ui| {
            choice(
                ui,
                "far shadow detail",
                &mut settings.shadow_lod,
                preset.shadow_lod(),
                &[true, false],
                on_off,
            );
        },
    );
    row(
        ui,
        "Contact shadows",
        Cost::Low,
        "A short march through the screen's depth for the gaps the shadow maps miss; \
         skipped under ray traced shadows.",
        |ui| {
            choice(
                ui,
                "contact shadows",
                &mut settings.contact_shadows,
                preset.contact_shadows(),
                &[true, false],
                on_off,
            );
            if effective.ray_traced_shadows != RayTracedShadows::Off {
                note(ui, "No effect under ray traced shadows");
            }
        },
    );
    row(
        ui,
        "Texture quality",
        Cost::None,
        "No frame time measured; lower qualities take less video memory.",
        |ui| {
            choice(
                ui,
                "texture quality",
                &mut settings.texture_quality,
                preset.texture_quality(),
                &TextureQuality::ALL,
                named(TextureQuality::label),
            );
        },
    );
    row(
        ui,
        "Texture memory",
        Cost::None,
        "No frame time measured: how much video memory textures may hold before the least \
         recently drawn lose detail.",
        |ui| {
            let (text, why) = automatic_texture_budget(preset, capabilities);
            custom_value(
                ui,
                &mut settings.texture_budget,
                preset.texture_budget_on(capabilities),
                "Off, the budget follows this graphics card's memory, or is the preset's where \
                 that isn't known.",
                (text, Some(why)),
                |ui, budget| {
                    ui.add(
                        egui::DragValue::new(budget)
                            .range(TEXTURE_BUDGET_RANGE)
                            .speed(4.0)
                            .suffix(" MB"),
                    );
                },
            );
        },
    );
    row(
        ui,
        "Ray tracing distance",
        Cost::High,
        "Mostly the CPU's: Overgrown Street's ray traced scene took 8.6 ms of the frame's \
         thread. Farther keeps more of the world in it. Used only while a ray traced effect \
         is on.",
        |ui| {
            custom_value(
                ui,
                &mut settings.ray_tracing_distance,
                preset.ray_tracing_distance(),
                "Off, the preset's distance is used.",
                (
                    format!("Preset ({} studs)", preset.ray_tracing_distance()),
                    None,
                ),
                |ui, distance| {
                    ui.add(
                        egui::DragValue::new(distance)
                            .range(RAY_TRACING_DISTANCE_RANGE)
                            .speed(5.0)
                            .suffix(" studs"),
                    );
                },
            );
        },
    );
    row(
        ui,
        "Ray traced shadow resolution",
        Cost::Medium,
        "Half saves about 55% of traced shadows: 12.8 ms at Full and 5.5 ms at Half in \
         Overgrown Street.",
        |ui| {
            ui.add_enabled_ui(
                capabilities.ray_queries && settings.ray_traced_shadows() != RayTracedShadows::Off,
                |ui| {
                    choice(
                        ui,
                        "ray traced shadow resolution",
                        &mut settings.ray_traced_shadow_resolution,
                        preset.ray_traced_shadow_resolution(),
                        &RayShadowResolution::ALL,
                        named(RayShadowResolution::label),
                    )
                    .on_hover_text(
                        "Full traces the sun's rays from every pixel. Half traces one pixel of \
                         each 2 × 2 (a different one each frame under TAA) and fills the rest \
                         from those on the same surface. Half at every preset.",
                    );
                },
            );
        },
    );
    row(
        ui,
        "Real-time GI (raster)",
        Cost::High,
        "On the CPU: in the editor at Ultra it once took 434 ms a frame (light probes drawn \
         again on one thread in a 36,000-item scene).",
        |ui| {
            choice(
                ui,
                "realtime gi",
                &mut settings.realtime_gi,
                preset.realtime_gi(),
                &[true, false],
                on_off,
            )
            .on_hover_text(
                "Light probe volumes drawn again while the world's light changes, instead of \
                 keeping their bakes, without ray tracing. Off at every preset. On, the preset \
                 sets how much: Low one probe a frame at 8² a face, Medium two at 8², High two \
                 at 16² and Ultra four at 16².",
            );
            if effective.ray_traced_global_illumination != RayTracedGlobalIllumination::Off {
                note(ui, "Ray traced GI takes its place");
            }
        },
    );
    let switches = [
        ("Vignette", preset.vignette(), &mut settings.vignette),
        ("Film grain", preset.film_grain(), &mut settings.film_grain),
        (
            "Chromatic aberration",
            preset.chromatic_aberration(),
            &mut settings.chromatic_aberration,
        ),
        ("Sharpen", preset.sharpen(), &mut settings.sharpen),
        ("Sun rays", preset.sun_rays(), &mut settings.sun_rays),
    ];
    for (name, preset_on, value) in switches {
        row(
            ui,
            name,
            Cost::None,
            "Drawn in the post-processing pass: nothing measurable.",
            |ui| {
                choice(ui, name, value, preset_on, &[true, false], on_off);
            },
        );
    }
}

/// What the renderer is using after the preset, the overrides and the
/// GPU's limits: a line of tags, and why anything was lowered.
fn in_use_section(ui: &mut egui::Ui, settings: &GraphicsSettings, capabilities: &Capabilities) {
    let palette = theme::palette(ui.ctx());
    section(ui, "In use");
    let effective = settings.effective(capabilities);
    egui::Frame::new()
        .fill(palette.surface)
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .corner_radius(theme::radius::CARD)
        .inner_margin(space::margin(space::M, space::S))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            responsive::toolbar(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                pill(
                    ui,
                    &format!("Preset: {}", settings.quality.label()),
                    palette.accent_text,
                );
                let neutral = palette.text_secondary;
                pill(
                    ui,
                    &format!("Scale: {}%", (effective.render_scale * 100.0).round()),
                    neutral,
                );
                pill(
                    ui,
                    &format!("AA: {}", effective.anti_aliasing.label()),
                    neutral,
                );
                pill(
                    ui,
                    &format!("Shadows: {}", effective.shadow_quality.label()),
                    neutral,
                );
                pill(
                    ui,
                    &format!(
                        "Textures {} MB · meshes {} MB",
                        effective.texture_budget >> 20,
                        effective.mesh_budget >> 20
                    ),
                    neutral,
                )
                .on_hover_text("The memory budgets for textures and meshes.");
                let rt_any = effective.ray_traced_shadows != RayTracedShadows::Off
                    || effective.ray_traced_ambient_occlusion != RayTracedAmbientOcclusion::Off
                    || effective.ray_traced_global_illumination != RayTracedGlobalIllumination::Off
                    || effective.ray_traced_reflections != RayTracedReflections::Off;
                let (text, color) = if effective.ray_queries && rt_any {
                    ("Hardware RT active", palette.success)
                } else if effective.ray_traced_gi_in_software {
                    ("Software GI active", palette.accent_text)
                } else if effective.ray_queries {
                    ("Hardware RT ready", neutral)
                } else {
                    ("Raster only", palette.text_weak)
                };
                pill(ui, text, color).on_hover_text(ray_tracing_in_use(&effective));
            });
            for note in &effective.notes {
                ui.add_space(space::XXS);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(theme::with_icon(icons::WARNING, note))
                            .size(type_scale::SMALL)
                            .color(palette.warning),
                    )
                    .wrap(),
                );
            }
        });
    ui.add_space(space::XS);
    caption(
        ui,
        "A world's Lighting can ask for less (ShadowsEnabled, ShadowDistance, \
         PostProcessingEnabled), never more.",
    );
}

/// Hardware ray tracing: whether this GPU can, and each ray traced effect,
/// off unless turned on (`docs/rendering/ray-tracing.md`). True when
/// Restart now was clicked.
fn ray_tracing_section(
    ui: &mut egui::Ui,
    settings: &mut GraphicsSettings,
    capabilities: &Capabilities,
    preset: &GraphicsSettings,
) -> bool {
    let palette = theme::palette(ui.ctx());
    section(ui, "Ray tracing");
    let can_rt = axcenity_render::adapters::adapter_can_ray_trace();
    let restart_needed = needs_restart(capabilities.ray_queries, can_rt, settings.ray_tracing());
    let mut restart = false;

    let (status, color) = match (capabilities.ray_queries, can_rt, settings.ray_tracing()) {
        (true, _, true) => ("Hardware RT active", palette.success),
        (true, _, false) | (false, true, true) => ("Restart required", palette.warning),
        (false, true, false) => ("Hardware RT available", palette.text_secondary),
        (false, false, _) => ("Software GI only", palette.text_weak),
    };
    pill(ui, status, color);
    ui.add_space(space::XXS);
    ui.add(
        egui::Label::new(
            egui::RichText::new(ray_tracing_status(
                capabilities.ray_queries,
                can_rt,
                settings.ray_tracing(),
            ))
            .size(type_scale::SMALL)
            .color(palette.text_secondary),
        )
        .wrap(),
    );
    ui.add_space(space::S);

    responsive::form(ui, "graphics ray tracing", |ui| {
        row(
            ui,
            "Hardware ray tracing",
            Cost::None,
            "Opening the GPU with ray tracing costs nothing a frame by itself; each effect \
             below has its own cost. Takes a restart.",
            |ui| {
                let mut on = settings.ray_tracing();
                if ui
                    .checkbox(&mut on, "At the next start")
                    .on_hover_text(
                        "Opens the GPU with hardware ray tracing the next time the editor \
                         starts, for ray traced shadows, ambient occlusion, reflections, \
                         hardware GI and View Mode > Ray Hits.",
                    )
                    .changed()
                {
                    settings.ray_tracing = on.then_some(true);
                }
            },
        );
    });

    if restart_needed {
        ui.add_space(space::XS);
        responsive::toolbar(ui, |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        if settings.ray_tracing()
                            && axcenity_render::adapters::ray_tracing_override() == Some(false)
                        {
                            "AXCENITY_RAY_TRACING=0 keeps ray tracing off. Restart now starts \
                             it without that."
                        } else if settings.ray_tracing() {
                            "Restart the editor to start hardware ray tracing."
                        } else {
                            "Restart the editor to turn hardware ray tracing off."
                        },
                    )
                    .size(type_scale::SMALL)
                    .color(palette.warning),
                )
                .wrap(),
            );
            restart = ui
                .add(widgets::button(
                    widgets::Role::Primary,
                    Some(icons::ARROW_CLOCKWISE),
                    "Restart now",
                    &palette,
                ))
                .on_hover_text(
                    "Saves these settings and starts the editor again with this project, \
                     carrying unsaved world and script edits.",
                )
                .clicked();
        });
    }

    ui.add_space(space::S);
    if !capabilities.ray_queries {
        caption(
            ui,
            "Needs hardware ray tracing: shadows, ambient occlusion and reflections below. \
             Global illumination, under Lighting, is traced in software without it.",
        );
        ui.add_space(space::XS);
    }
    let rt = capabilities.ray_queries;
    responsive::form(ui, "graphics ray traced effects", |ui| {
        row(
            ui,
            "Ray traced shadows",
            Cost::High,
            "4.7 ms in Desert Homestead at 2560 × 1405; in Overgrown Street 12.8 ms at Full \
             resolution and 5.5 ms at Half.",
            |ui| {
                ui.add_enabled_ui(rt, |ui| {
                    choice(
                        ui,
                        "ray traced shadows",
                        &mut settings.ray_traced_shadows,
                        preset.ray_traced_shadows(),
                        &RayTracedShadows::ALL,
                        named(RayTracedShadows::label),
                    )
                    .on_hover_text(
                        "The sun's and lamps' shadows traced as rays: exact up close, with soft \
                         edges by blocker distance.",
                    );
                });
            },
        );
        row(
            ui,
            "Ray traced ambient occlusion",
            Cost::High,
            "2.3 to 2.9 ms in Desert Homestead; 6.0 ms in Overgrown Street.",
            |ui| {
                ui.add_enabled_ui(rt, |ui| {
                    choice(
                        ui,
                        "ray traced ambient occlusion",
                        &mut settings.ray_traced_ambient_occlusion,
                        preset.ray_traced_ambient_occlusion(),
                        &RayTracedAmbientOcclusion::ALL,
                        named(RayTracedAmbientOcclusion::label),
                    )
                    .on_hover_text(
                        "Corners and creases darkened by short rays, including off-screen \
                         occluders. Low, Medium and High trace 2, 4 and 8 rays a pixel.",
                    );
                });
            },
        );
        row(
            ui,
            "Ray traced reflections",
            Cost::Low,
            "With ray traced GI, 0 to 2 fps on top of traced shadows and ambient occlusion in \
             Desert Homestead; not measured in other worlds.",
            |ui| {
                ui.add_enabled_ui(rt, |ui| {
                    choice(
                        ui,
                        "ray traced reflections",
                        &mut settings.ray_traced_reflections,
                        preset.ray_traced_reflections(),
                        &RayTracedReflections::ALL,
                        named(RayTracedReflections::label),
                    )
                    .on_hover_text(
                        "Glossy surfaces mirror off-screen geometry with a ray a pixel, in \
                         place of screen-space reflections. Low traces roughness under 0.15 at \
                         half size; Medium under 0.3 and High under 0.5 at full size.",
                    );
                });
            },
        );
    });
    ui.add_space(space::XS);
    let any_effect_on = settings.ray_traced_shadows() != RayTracedShadows::Off
        || settings.ray_traced_ambient_occlusion() != RayTracedAmbientOcclusion::Off
        || settings.ray_traced_global_illumination() != RayTracedGlobalIllumination::Off
        || settings.ray_traced_reflections() != RayTracedReflections::Off;
    if ui
        .add_enabled(any_effect_on, egui::Button::new("Turn all off"))
        .on_hover_text(
            "Turns every ray traced effect off, global illumination too, without changing \
             Hardware ray tracing.",
        )
        .clicked()
    {
        *settings = settings.without_ray_traced_effects();
    }
    restart
}

/// Whether the GPU must be opened again for Hardware ray tracing to be as
/// set: `wanted` but not `opened` on a GPU that `can`, or opened but no
/// longer wanted.
pub(super) fn needs_restart(opened: bool, can: bool, wanted: bool) -> bool {
    if wanted { !opened && can } else { opened }
}

/// What the Ray tracing section says about this GPU: whether the device
/// was `opened` with ray tracing, whether its adapter `can` ray trace, and
/// whether it's set to be opened with it (`wanted`) at the next start.
pub(super) fn ray_tracing_status(opened: bool, can: bool, wanted: bool) -> &'static str {
    match (opened, can, wanted) {
        (true, _, true) => {
            "Hardware ray tracing is on. Each ray traced effect is off until turned on; \
             View Mode > Ray Hits shows what rays hit."
        }
        (true, _, false) => "Hardware ray tracing turns off when the editor restarts.",
        (false, true, true) => "Hardware ray tracing turns on when the editor restarts.",
        (false, true, false) => {
            "This GPU can ray trace. Turn on Hardware ray tracing and restart the editor to \
             use it; until then the GPU is opened without it."
        }
        (false, false, _) => {
            "This GPU can't ray trace (it needs DirectX 12 ray tracing tier 1.1, Vulkan ray \
             queries or Metal on macOS 15). Everything is drawn without it."
        }
    }
}

/// The "In use" line for ray tracing: off, or each ray traced effect on.
pub(super) fn ray_tracing_in_use(effective: &EffectiveGraphics) -> String {
    if !effective.ray_queries {
        let off = "Ray tracing: off (Settings > Graphics > Hardware ray tracing) or not available";
        // GI's rays are then traced in software (RL3).
        if effective.ray_traced_gi_in_software {
            return format!(
                "{off}; GI {} traced in software ({} probes a frame, {} rays each)",
                effective.ray_traced_global_illumination.label(),
                effective.ray_traced_global_illumination.probes_per_frame(),
                effective.ray_traced_global_illumination.rays()
            );
        }
        return off.to_owned();
    }
    let mut effects = Vec::new();
    if effective.ray_traced_shadows != RayTracedShadows::Off {
        effects.push(format!(
            "shadows {} ({} rays a pixel where soft)",
            effective.ray_traced_shadows.label(),
            effective.ray_traced_shadows.rays()
        ));
    }
    if effective.ray_traced_ambient_occlusion != RayTracedAmbientOcclusion::Off {
        effects.push(format!(
            "ambient occlusion {} ({} rays a pixel)",
            effective.ray_traced_ambient_occlusion.label(),
            effective.ray_traced_ambient_occlusion.rays()
        ));
    }
    if effective.ray_traced_global_illumination != RayTracedGlobalIllumination::Off {
        effects.push(format!(
            "GI {} ({} probes a frame, {} rays each)",
            effective.ray_traced_global_illumination.label(),
            effective.ray_traced_global_illumination.probes_per_frame(),
            effective.ray_traced_global_illumination.rays()
        ));
    }
    if effective.ray_traced_reflections != RayTracedReflections::Off {
        effects.push(format!(
            "reflections {} (smoother than {} roughness{})",
            effective.ray_traced_reflections.label(),
            effective.ray_traced_reflections.roughness_cutoff(),
            if effective.ray_traced_reflections.downscale() > 1 {
                ", half size"
            } else {
                ""
            }
        ));
    }
    let reach = effective.ray_tracing_distance.round();
    if effects.is_empty() {
        format!("Ray tracing: available, up to {reach} studs (no ray traced effects on)")
    } else {
        format!("Ray tracing up to {reach} studs: {}", effects.join(", "))
    }
}

/// What the Texture memory row says while no budget is set, and why: the
/// budget the card's memory gives where that is more than the preset's,
/// otherwise the preset's.
pub(super) fn automatic_texture_budget(
    preset: &GraphicsSettings,
    capabilities: &Capabilities,
) -> (String, String) {
    let own = preset.texture_budget();
    let automatic = preset.texture_budget_on(capabilities);
    match capabilities.video_memory {
        Some(memory) if automatic > own => (
            format!("Automatic ({automatic} MB)"),
            format!(
                "A quarter of this graphics card's {} GB. The preset alone gives {own} MB.",
                axcenity_render::whole_gib(memory)
            ),
        ),
        Some(memory) => (
            format!("Preset ({own} MB)"),
            format!(
                "A quarter of this graphics card's {} GB is no more than the preset's budget.",
                axcenity_render::whole_gib(memory)
            ),
        ),
        None => (
            format!("Preset ({own} MB)"),
            "This graphics card doesn't say how much memory it has, so the preset's budget \
             is used. Set one by hand if the card has room."
                .to_owned(),
        ),
    }
}
