//! The start screen's Character Studio page (`docs/research/character-studio.md`
//! CS2): make a new character from a preset or open a saved one, in the
//! Character Studio workspace, with no project open.

use super::*;
use crate::character_workspace::{presets, saved_characters};

impl StartScreen {
    /// Where Character Studio keeps its files (the page lists the saved
    /// characters there).
    pub fn set_characters_folder(&mut self, folder: Option<PathBuf>) {
        self.characters_folder = folder;
    }

    pub(super) fn characters_page(&mut self, ui: &mut egui::Ui) -> Option<StartAction> {
        let palette = theme::palette(ui.ctx());
        let mut action = None;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Character Studio")
                        .family(theme::display_family())
                        .size(type_scale::DISPLAY)
                        .extra_letter_spacing(theme::DISPLAY_TRACKING)
                        .color(palette.text_strong),
                );
                ui.label(
                    RichText::new(
                        "Create a character, dress it and save it, then use it in any game.",
                    )
                    .color(palette.text_secondary),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let new =
                    widgets::button(Role::Primary, Some(icons::PLUS), "New Character", &palette)
                        .min_size(vec2(0.0, 32.0));
                if ui
                    .add(new)
                    .on_hover_text("Open Character Studio on the default character")
                    .clicked()
                {
                    action = Some(StartAction::NewCharacter(0));
                }
            });
        });
        ui.add_space(28.0);
        widgets::section_label(ui, "Start from a preset");
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(CARD_SPACING, CARD_SPACING);
            for (index, (name, about, _)) in presets().into_iter().enumerate() {
                let response = egui::Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0_f32, palette.border))
                    .corner_radius(theme::radius::CARD)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.set_width(200.0);
                        ui.label(
                            RichText::new(theme::with_icon(icons::USER_FOCUS, name))
                                .family(theme::medium_family())
                                .color(palette.text_strong),
                        );
                        ui.add(
                            egui::Label::new(RichText::new(about).color(palette.text_weak).small())
                                .wrap(),
                        );
                    })
                    .response
                    .interact(Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    action = Some(StartAction::NewCharacter(index));
                }
            }
        });
        ui.add_space(28.0);
        widgets::section_label(ui, "Your characters");
        ui.add_space(6.0);
        let saved = self
            .characters_folder
            .as_deref()
            .map(saved_characters)
            .unwrap_or_default();
        if saved.is_empty() {
            ui.label(
                RichText::new("Characters you save in Character Studio appear here.")
                    .color(palette.text_weak),
            );
        }
        for (name, path) in saved {
            ui.horizontal(|ui| {
                ui.label(RichText::new(theme::with_icon(icons::USER, &name)).color(palette.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(widgets::button(Role::Secondary, None, "Open", &palette))
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        action = Some(StartAction::OpenCharacter(path.clone()));
                    }
                });
            });
        }
        action
    }
}
