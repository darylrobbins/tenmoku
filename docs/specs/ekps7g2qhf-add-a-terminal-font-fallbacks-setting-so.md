---
task_uid: "01m4sj3rrgrja4h3ekps7g2qhf"
task: "T71"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config tests/terminal_font_fallbacks.rs: terminal.font_fallbacks defaults to empty, takes a list of strings from user and project files, and a non-list value is a config problem that keeps the default, as the other terminal settings do; the schema and the commented template carry it and tests/template.rs passes"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal-view tests/font_fallbacks.rs: with font_fallbacks [\"Symbols Nerd Font Mono\"], every Font the element asks the text system for (the cell measure, plain, bold and italic cells, marked text) has those fallbacks; with an empty list Font::fallbacks is None"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench terminal_font_fallbacks_tests.rs: a settings reload changing terminal.font_fallbacks restyles the running session (its TerminalStyle has the list) without starting a new one"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test and cargo clippy --all-targets -- -D warnings pass for fe-config, fe-terminal-view and fe-workbench"
    required: true
    manual: false
---

# Add a terminal.font_fallbacks setting so Nerd Font icons can come from a symbol font, such as Symbols Nerd Font Mono, beside any terminal font (Mac T607)

Filed on argon from Mac T607 (card ~/coordination/argon-cards/T607.json rev 1). Mac task uid 01m4sj29mr6xcd01kksy267384. Depends on argon T70 (both change fe-terminal-view element.rs). Its other dependencies, Mac T578 (argon T62) and Mac T585 (argon T64), are on argon main. Title the host commit "... (Mac T607)".

## Problem

The user (2026-10-13): "We should support a custom font for the terminal so I can specify a nerd font."

T578 (argon dbd1bfb, stack 6) lets them set `terminal.font_family` to a patched Nerd Font. Someone who wants to
keep their own font (SF Mono, Berkeley Mono) and only take the icons from Nerd Fonts' symbol font
("Symbols Nerd Font Mono") can't: on macOS GPUI's CoreText shaping falls back only through the fonts in
`Font::fallbacks` and the system's default cascade list
(`gpui-pre-macos` `open_type::apply_features_and_fallbacks` / `append_system_fallbacks`), which doesn't
include user-installed symbol fonts, so private-use icons draw as missing-glyph boxes. The terminal builds
its font with `gpui::font(style.font_family.clone())` (`crates/fe-terminal-view/src/element.rs`:
`cell_size`, `prepaint`, the marked text), so it never sets fallbacks. Zed and WezTerm offer a fallback font
list for this.

## Change

- **Setting.** `crates/fe-config/src/settings.rs`: `TerminalSettings` gains `font_fallbacks: Vec<String>`,
  default empty, settable from user and project files. Add it to `crates/fe-config/settings.schema.json`
  (array of strings) and to the `[terminal]` table of `crates/fe-config/settings-template.toml`, commented:

  ```toml
  # Fonts to take a character from when the terminal's font lacks it, in
  # order: "Symbols Nerd Font Mono" gives Nerd Font icons with any font.
  # Or set font_family to a Nerd Font, such as "JetBrainsMono Nerd Font Mono".
  # font_fallbacks = []
  ```
- **Style.** `crates/fe-terminal-view/src/style.rs`: `TerminalStyle` gains `font_fallbacks:
  Vec<SharedString>` and a `TerminalStyle::font(&self) -> gpui::Font` that sets `Font::fallbacks`
  (`FontFallbacks::from_fonts`) when the list isn't empty. `element.rs` builds every font through it (bold
  and italic keep the fallbacks).
- **Panel.** `crates/fe-workbench/src/terminal_panel.rs`'s `style(theme, settings)` copies the list; a
  reload changing it restyles the running session, as T578 does for `font_family`.

New tests in `crates/fe-config/tests/terminal_font_fallbacks.rs`, `crates/fe-terminal-view/tests/font_fallbacks.rs`
(a fake `PlatformTextSystem` whose `font_id` records each `Font` descriptor asked for) and
`crates/fe-workbench/src/terminal_font_fallbacks_tests.rs` (declared from `terminal_panel.rs` with `#[path]`,
as `terminal_settings_tests.rs` is).

Needs Mac stack 6 (T578's `TerminalSettings` and `terminal_panel.rs`; T585's `element.rs` and `tests/find.rs`).
