---
task_uid: "01m69vtapd6r7bspsvav9n28n5"
task: "T389"
rev: 1
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/accessibility-audit.md exists with sections Baseline (window, project tree, tabs, editor, terminal, chat, find bar, settings, toasts, each saying what the tree exposes today or why the dump cannot reach it), API surface (gpui-pre 0.3.9 a11y builders/hooks and what a code editor lacks), Gap list per surface, Human checks (VoiceOver and Accessibility Inspector steps, marked not done), and Proposed follow-up cards (ordered, each with title, type, size, files, dependencies)"
    required: true
    manual: false
  - id: "C2"
    text: "The doc names the four custom elements needing text-input a11y by exact file: crates/fe-gpui/src/element.rs, crates/fe-gpui/src/file_viewer/element.rs, crates/fe-terminal-view/src/element.rs, kiln/crates/kiln-ui/src/text_field.rs; and states whether Window::debug_a11y_tree_json yields a tree in a headless test window"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-workbench/tests/a11y_tree_dump.rs (new file) builds a Workbench window with an open file, draws, parses window.debug_a11y_tree_json() with serde_json and asserts a root node (or documents in its doc comment why headless gives none); an #[ignore]d dump test prints the JSON under --nocapture; cargo test -p fe-workbench --test a11y_tree_dump passes and clippy -p fe-workbench --all-targets -D warnings is clean"
    required: true
    manual: false
  - id: "C4"
    text: "git diff --stat against the base lists only docs/accessibility-audit.md and crates/fe-workbench/tests/a11y_tree_dump.rs (no production or Cargo.toml change)"
    required: true
    manual: false
---

# Audit accessibility: baseline what screen readers see today (gpui-pre a11y tree dump), the usable API, per-surface gaps and an ordered list of follow-up cards (Mac T1266)

## Goal

A spike: record what assistive technology can see in the app today and plan the accessibility work.
Deliverable is one doc, `docs/accessibility-audit.md`, plus one headless test that makes the baseline
reproducible. **No production code changes.**

## Facts found

- GPUI is `gpui-pre =0.3.9` (Cargo.toml, Cargo.lock). It ships AccessKit (guide: `_accessibility.rs` in the
  crate's `src/`, under `~/.cargo/registry/src/*/gpui-pre-0.3.9/`): `.role()` and `.aria_*()` builders on
  divs (`elements/div.rs` ~1310-1490, incl. `aria_label`, `aria_selected`, `aria_expanded`, `aria_value`,
  `aria_active_descendant`), the `Element` hooks `a11y_role`, `write_a11y_info` and
  `a11y_synthetic_children` (`element.rs` ~113-135, used for text runs), `Window::on_a11y_action`,
  `Window::is_a11y_active`, and `Window::debug_a11y_tree_json` (retains the last `TreeUpdate`; `window/a11y/debug.rs`).
- fastedit uses none of it: `rg -n 'a11y|accesskit|\.role\(|aria_' --glob '*.rs' crates kiln` is empty.
- Four custom elements will each need text-input a11y (value, selection, caret, text runs):
  `crates/fe-gpui/src/element.rs` (`EditorElement`), `crates/fe-gpui/src/file_viewer/element.rs`
  (`FileViewerElement`), `crates/fe-terminal-view/src/element.rs` (`TerminalElement`),
  `kiln/crates/kiln-ui/src/text_field.rs` (`TextLine`).
- Headless windows (`cx.add_window_view`, as in `crates/fe-workbench/tests/palette_ghost_text.rs`) may never
  report a11y as active (`A11y::is_active` needs the platform's active flag), in which case
  `debug_a11y_tree_json` returns `None` or an empty tree. Finding out is part of the job.

## Changes

1. New test `crates/fe-workbench/tests/a11y_tree_dump.rs` (a new test binary, like its siblings): build a
   Workbench window as `palette_ghost_text.rs` does, open a file, draw, and call
   `window.debug_a11y_tree_json()`. Print the JSON (an `#[ignore]`d `dump` test run with `--nocapture`, so
   the default gate stays fast) and assert only what is true today and cheap: that the dump parses
   (`serde_json` is already a dev-dependency) and has a root node. If headless windows cannot produce a tree,
   say so in the test's doc comment and the audit doc, and propose the smallest production dump hook
   (for example an env var read in `main.rs`, which T1257 is editing) as the first follow-up card; do not add it here.
2. Write `docs/accessibility-audit.md` with:
   - **Baseline** per surface: window, project tree, tabs, editor, terminal, chat, find bar, settings,
     toasts. Say what the tree exposes today (from the dump, where the headless dump reaches the surface) and
     mark surfaces the dump cannot reach, with the reason.
   - **API surface**: the gpui-pre a11y API we can use, as a short table (builder or hook, what it is for,
     which surface needs it), noting anything missing for a code editor (text selection ranges, live
     regions for toasts and terminal output, caret) and what AccessKit 's `TextRun`/`TextSelection` give.
   - **Gap list** per surface: what a screen reader user cannot do or hear today.
   - **Human checks**: the steps for VoiceOver and Accessibility Inspector (macOS) on the running app, listed
     as steps for the user. Mark them as not done; this card does not require them.
   - **Proposed follow-up cards**, ordered, each with title, type, size (xs-xl), files it would touch and
     dependencies. Expect roughly: dump hook (if needed), window/landmark roles and labels for chrome,
     tabs, project tree and find bar; settings and toasts (live regions); `TextLine` and kiln text fields;
     `EditorElement` and `FileViewerElement` text-input a11y; `TerminalElement`; chat transcript. Size
     the editor and terminal cards honestly (probably l).

## Out of scope

Any production change, any `rgt` or card filing (the lead files the follow-ups from the doc), VoiceOver or
GUI driving (no GUI permissions on this Mac).
