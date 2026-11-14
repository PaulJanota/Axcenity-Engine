//! The editor's side of the Character Studio workspace
//! (`crate::character_workspace`): opening it from the start screen,
//! drawing it in place of the start screen, and leaving it.

use std::path::{Path, PathBuf};

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use super::EditorApp;
use crate::character_workspace::{CharacterWorkspace, WorkspaceAction, presets};
use crate::dialog;
use crate::theme::{self, icons};

impl EditorApp {
    /// Where Character Studio keeps its files; the start screen lists the
    /// saved characters there.
    pub fn set_characters_folder(&mut self, folder: Option<PathBuf>) {
        self.start.set_characters_folder(folder.clone());
        self.characters_folder = folder;
    }

    /// Opens the workspace on the preset at `preset` or on a saved `file`.
    /// A problem is told on the start screen.
    pub(super) fn open_character_workspace(&mut self, preset: Option<usize>, file: Option<&Path>) {
        let Some(folder) = self.characters_folder.clone() else {
            self.start.set_notice(
                "Character Studio has nowhere to keep its files on this computer.".into(),
            );
            return;
        };
        let opened = match file {
            Some(file) => CharacterWorkspace::open_file(&folder, file),
            None => {
                let look = presets()
                    .into_iter()
                    .nth(preset.unwrap_or(0))
                    .map(|(_, _, look)| look)
                    .unwrap_or_default();
                CharacterWorkspace::open(&folder, look, false)
            }
        };
        match opened {
            Ok(workspace) => self.character_workspace = Some(Box::new(workspace)),
            Err(error) => self.start.set_notice(error),
        }
    }

    /// Opens the workspace on the default character (`--character-studio`).
    pub fn open_character_studio(&mut self) {
        self.open_character_workspace(Some(0), None);
    }

    /// Draws the open workspace and does what it asks.
    pub(super) fn character_workspace_ui(
        &mut self,
        ui: &mut egui::Ui,
        render_state: Option<&RenderState>,
    ) {
        let Some(workspace) = self.character_workspace.as_mut() else {
            return;
        };
        let palette = theme::palette(ui.ctx());
        let action = egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(palette.app_bg))
            .show(ui, |ui| workspace.ui(ui, render_state))
            .inner;
        if action == Some(WorkspaceAction::Home) {
            if workspace.dirty() {
                workspace.leaving = true;
            } else {
                self.character_workspace = None;
                return;
            }
        }
        if workspace.leaving {
            let (icon, tone) = dialog::unsaved_style();
            let message = format!(
                "\"{}\" has unsaved changes. Save them before going back to the start screen?",
                workspace.look().name
            );
            let outcome = dialog::ask(
                ui.ctx(),
                &dialog::Question {
                    id: "leave character studio",
                    icon,
                    tone,
                    title: "Save changes?",
                    message: &message,
                    items: Vec::new(),
                },
                &[
                    dialog::Button::new(icons::X, "Cancel", dialog::Kind::Plain),
                    dialog::Button::new(icons::TRASH, "Don't Save", dialog::Kind::Plain),
                    dialog::Button::new(icons::FLOPPY_DISK, "Save", dialog::Kind::Main),
                ],
            );
            match outcome {
                Some(dialog::Outcome::Picked(2)) => match workspace.save() {
                    Ok(_) => self.character_workspace = None,
                    Err(error) => {
                        workspace.notice = Some(error);
                        workspace.leaving = false;
                    }
                },
                Some(dialog::Outcome::Picked(1)) => self.character_workspace = None,
                Some(_) => workspace.leaving = false,
                None => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::drawn_texts;
    use super::*;

    fn frame(app: &mut EditorApp, ctx: &egui::Context) -> Vec<String> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 900.0),
            )),
            ..Default::default()
        };
        let output = crate::theme::run(ctx, input, |ui| app.frame(ui, None));
        drawn_texts(&output)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    #[test]
    fn character_studio_opens_from_the_start_screen_as_a_workspace_of_its_own() {
        let ctx = crate::theme::test_context();
        let mut app = EditorApp::new(None);
        let folder = crate::character_studio::test_folder("workspace-app");
        app.set_characters_folder(Some(folder.clone()));
        let start = frame(&mut app, &ctx);
        assert!(
            start.iter().any(|text| text.contains("Character Studio")),
            "the start screen lists it: {start:?}"
        );
        assert!(start.iter().any(|text| text == "Debug"), "{start:?}");

        // The Athlete preset, from the start page.
        app.open_character_workspace(Some(2), None);
        let workspace = app.character_workspace.as_ref().expect("opened");
        assert_eq!(workspace.look().hair.as_deref(), Some("Hair_Buzz"));
        assert_eq!(app.title(), "New Character* - Character Studio");
        let drawn = frame(&mut app, &ctx);
        for text in ["Customize", "Presets", "Clothing", "Athlete"] {
            let found = drawn
                .iter()
                .any(|t| t.to_lowercase().contains(&text.to_lowercase()));
            assert!(found, "{text} in {drawn:?}");
        }
        // No Game Editor menus over it.
        assert!(!drawn.iter().any(|text| text == "Debug"), "{drawn:?}");

        // Leaving with unsaved changes asks first.
        app.character_workspace.as_mut().unwrap().leaving = true;
        // A modal is sized on its first frame and shown on the next.
        frame(&mut app, &ctx);
        let asked = frame(&mut app, &ctx);
        assert!(
            asked
                .iter()
                .any(|text| text.contains("\"New Character\" has unsaved changes")),
            "{asked:?}"
        );
        // A saved character opens from its file, not dirty.
        let workspace = app.character_workspace.as_mut().unwrap();
        workspace.leaving = false;
        let saved = workspace.save().unwrap();
        app.character_workspace = None;
        app.open_character_workspace(None, Some(&saved));
        assert!(!app.character_workspace.as_ref().unwrap().dirty());
        assert_eq!(app.title(), "New Character - Character Studio");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
