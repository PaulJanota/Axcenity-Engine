---
title: "ADR-0031: Character Studio is a workspace of its own, outside the Game Editor"
type: adr
adr: 31
adr_status: Accepted
date: 2026-11-14
supersedes:
superseded_by:
affects: [editor, project, anim, scene]
source: [crates/editor/src/character_workspace.rs, crates/editor/src/app/character_workspace.rs, crates/editor/src/start/characters.rs]
docs: [docs/research/character-studio.md, docs/editor/character-studio.md]
tags: [adr, editor, characters]
---

# ADR-0031: Character Studio is a workspace of its own, outside the Game Editor

- **Status:** Accepted
- **Date:** 2026-11-14
- **Affected systems:** the editor's start screen and window, Character Studio

## Context and problem

Paul's brief of 2026-11-14 (`docs/research/character-studio.md`) asks for character creation that a beginner can reach from the launch screen without opening a project or learning the Game Editor, and that isn't "the Game Editor with a few panels hidden". Until then Character Studio was a document tab in an open project, following the selection (facial animation F6, decision D9 of `docs/research/facial-animation.md`).

## Considered alternatives

| Option | For | Against |
|---|---|---|
| Keep it a Game Editor tab | No new window states | Needs a project and a selected character; surrounded by the editor's panels and menus |
| A separate program | Fully focused | A second app to build, ship and update; can't share the editor's assets, theme or settings |
| A third state of the editor window: the workspace, in place of the start screen | One program; shares `assets`, `anim`, `scene`, `render`, the theme and the preview canvas; no project needed | The window has three states (start screen, project, workspace) to keep apart |

## Decision

The editor window shows the Character Studio workspace in place of the start screen when it's opened from the start screen's Character Studio page (or `--character-studio`). It owns a preview World posed by the engine's `Animators`, its own undo, and its files in the editor's data folder (`Character Studio/`); the Game Editor's menus aren't shown over it. The face tab stays in the Game Editor for characters placed in a project.

## Reasons

- A beginner gets one click from launch to a live character, with no project and no editor chrome.
- Reusing the engine's systems keeps one implementation of posing, morphing and drawing (the preview is what a game draws, plus the look's shape weights until `HumanoidDescription` lands).

## Consequences

- Characters made here live outside projects until CS5 brings them into one (the global library or Insert ▸ Character).
- The window has a third state; code that assumes "no project means start screen" must check the workspace (the title and the header menus already do).
- Model Studio or an Animation Studio could become workspaces the same way.

## Implementation references

- `crates/editor/src/character_workspace.rs`, `character_workspace/ui.rs`, `stage.rs`
- `crates/editor/src/app/character_workspace.rs`, `start/characters.rs`, the `--character-studio` flag in `main.rs`
- Tests: `character_workspace::tests`, `app::character_workspace::tests`
- Plan: `docs/research/character-studio.md` D9, stage CS2
