---
task_uid: "01m4vbj6fw0v7kdz4q8wqz0s4a"
task: "T675"
rev: 1
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "The host example's Rust (kiln/crates/kiln/examples/host/*.rs), counted without blank and comment-only lines, is at most 400 lines (929 before), and starts from kiln::app without using Shell, PaneGroup, Peers, LayoutStore or close::set_window_items directly"
    required: true
    manual: false
  - id: "C2"
    text: "The host keymap.toml has no picker.*, text_input.*, menu_bar.open or app.quit rows"
    required: true
    manual: false
  - id: "C3"
    text: "Conversation implements ItemState with a typed state and the threads persist through a serde type in the workspace entry, with no hand Value parsing"
    required: true
    manual: false
  - id: "C4"
    text: "tests/host.rs keeps every test function by name and what each asserts, drives the app through kiln::testing, and no example item keeps an allow(dead_code) accessor for tests/host.rs"
    required: true
    manual: false
---

# Rewrite the Kiln Chat host example on kiln::app and Workspace, under 400 Rust lines, with tests/host.rs checking the same things

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 1's exit; design record
`docs/decisions/kiln-app-layer.md`). Phase 1's exit criterion: the host example rewritten on `kiln::app` and
`Workspace` comes in under ~400 Rust lines, from 929 (566 of them in `examples/host/app.rs`), with no picker keys
in its keymap, while `tests/host.rs` checks the same things. It is kiln's proof that the defaults carry a real
app.

## Change

Rewrite `kiln/crates/kiln/examples/host/` (Kiln Chat) on what phase 1 added:

- `main.rs` is `kiln::app("dev.kiln.Chat", "Kiln Chat")` with `.key_context("chat")`,
  `.item::<Conversation>()`, `.panel(Left, ..)` for the conversation list, `.status(..)` for the conversation
  count, `.theme_mode(initial_mode())`, `.theme_extensions(&[THEME_TABLE])`, the commands, keymap and menus, and
  a `.workspace(..)` hook that opens the first conversation when nothing was restored. `setup.rs` keeps only what
  isn't a convention.
- `app.rs` keeps only chat behaviour (new and close conversation, split right and down through
  `Workspace::split`, toggle the panel and the theme, open a thread or activate its tab, the panel selection
  following `WorkspaceEvent::ActiveChanged`). `ChatWindow`'s pane, dock, status bar and layout glue go; the
  `Panes` and `StatusBar` views go.
- `Conversation` implements `kiln_shell::item::ItemState` with a typed state; the threads persist through a
  `serde` type in the workspace entry's app keys (`Workspace::on_save`), without hand `Value` parsing.
- `keymap.toml` loses the rows kiln now bundles (`picker.*`, `text_input.*`, `menu_bar.open`, `app.quit`,
  `command_palette.toggle` unless kept in `chat` deliberately).
- `tests/host.rs` drives the app through `kiln::testing::TestApp`. Keep every test function, by name, and what
  each asserts; only how it reaches the app changes. No example item keeps an
  `#[allow(dead_code, reason = "for tests/host.rs")]` accessor.
