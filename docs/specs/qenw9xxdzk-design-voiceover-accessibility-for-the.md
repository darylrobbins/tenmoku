---
task_uid: "01m6smz3smfagdqmqenw9xxdzk"
task: "T294"
rev: 1
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/decisions/\u003clane\u003e-accessibility-design.md exists with the twelve sections the spec names (Status, What was read and spiked, Platform path options, Editor text area, Announcements, Chrome semantics, Keyboard reachability, System settings, Testing strategy, Recommended first slice, Proposed card breakdown, Human checks), each ending in a decision or a named open question"
    required: true
    manual: false
  - id: "C2"
    text: "Editor text area says, for each NSAccessibility text method the spec lists, whether accesskit_macos 0.26.3 answers it, citing the file and line read, and decides how much text is exposed and what happens to lines outside it"
    required: true
    manual: false
  - id: "C3"
    text: "System settings covers Reduce Motion, Increase Contrast, Reduce Transparency, Differentiate Without Colour and larger text, each with the AppKit API, how a live change is observed and where the code goes"
    required: true
    manual: false
  - id: "C4"
    text: "Proposed card breakdown lists each implementation card with size, dependencies, Mac-only or Linux-OK and exact scope globs (no crates/*/src/lib.rs), and says for each of docs/accessibility-audit.md's ten follow-up cards whether it is kept, merged, reordered or dropped"
    required: true
    manual: false
  - id: "C5"
    text: "The doc's status line says 'proposed' and names the user as the approver; no implementation card is filed by this card"
    required: true
    manual: false
  - id: "C6"
    text: "git diff --stat against the base lists only the decision doc and docs/accessibility-audit.md, the latter with at most one added line; no .rs, Cargo or prototype file"
    required: true
    manual: false
  - id: "C7"
    text: "rgt ci run --fast passes"
    required: true
    manual: false
---

# Design VoiceOver accessibility for the first release: platform path (gpui-pre AccessKit vs patch vs direct NSAccessibility), the editor text area, announcements, keyboard reach, system settings and a first slice (Mac T1430)

## Problem

The user (2026-11-06) approved accessibility work: a **VoiceOver first pass** for the first public release,
which is macOS-only and editor-first. Before any implementation card, decide how Tenmoku exposes itself to
VoiceOver, starting with the editor's text area. Also decide how it respects the system's accessibility
settings, and how every control is reached by keyboard. The user approves the design before implementation
cards are readied.

Linux-OK for the doc itself. The design must say what is Mac-only, and must keep kiln-level code
platform-neutral, because AccessKit also has `accesskit_unix` (AT-SPI) for the Linux port on neon.

## What exists, to build on (do not redo)

- **`docs/accessibility-audit.md` (T389, Mac T1266, landed 622272cb)**. Read it first. It already has:
  - the baseline: nothing below the root `Window` node
  - gpui-pre's AccessKit API surface (`.id()`+`.role()`, `aria_*` builders, `Element::a11y_role`,
    `write_a11y_info`, `a11y_synthetic_children` + `A11ySubtreeBuilder`, `Window::on_a11y_action`,
    `is_a11y_active`, `debug_a11y_tree_json`)
  - a per-surface gap list
  - a 10-card follow-up list. **None of those cards was filed.**

  It also found that a headless test window builds **no** tree (`crates/fe-workbench/tests/a11y_tree_dump.rs`).
- **One surface already uses the API**: the setup wizard, `crates/fe-workbench/src/setup_wizard.rs` (T1339,
  T1341). It has roles, `aria_label`, `aria_selected`, `aria_position_in_set`/`size_of_set`,
  `aria_toggled`, `aria_numeric_value*` and `aria_active_descendant`, and tests that check the labels it
  passes. That is the in-repo pattern to generalise. T1344 (inbox) is the human VoiceOver check of it.
- **Versions** (Cargo.lock): `gpui-pre =0.3.9`, `accesskit 0.24.1`, `accesskit_macos 0.26.3`,
  `accesskit_unix 0.22.1`. GPUI is **not vendored or patched**: `[patch.crates-io]` patches only
  tree-sitter and tree-sitter-yaml. Source is under `~/.cargo/registry/src/*/gpui-pre-0.3.9/` (guide
  `src/_accessibility.rs`, `examples/a11y`) and `gpui-pre-macos-0.3.9/src/window.rs`. The macOS window
  creates an `accesskit_macos::SubclassingAdapter::for_window` (`a11y_init`, `a11y_tree_update`, activation
  and action handlers, around line 2367).
- `accesskit_macos 0.26.3`'s `node.rs` implements the NSAccessibility text methods, including
  `accessibilityRangeForLine:` and `accessibilityLineForIndex:` (around lines 782 and 960). Which of the
  others it implements is for this card to establish.
- **System settings**: gpui-pre has `App::reduce_motion` / `set_reduce_motion` (`app.rs` around line 1293;
  `img.rs` honours it for animated images). **Nothing in `gpui-pre-macos` sets it from the system**, and
  Tenmoku never calls it. Tenmoku has no motion today (docs/design-language.md, Motion), but the polish
  card filed beside this one may add some. There is no UI font-size or zoom setting: only the editor's and
  the terminal's `font_size` (`crates/fe-config/src/settings.rs`). Chrome text is literal 11–15 px.

## Deliverable: `docs/decisions/<lane>-accessibility-design.md`

Name it with the `<lane>-` prefix the other files in `docs/decisions/` use. Each section ends in a decision
or a named open question.

1. **Status**: `proposed`. The user approves. No implementation card is filed by this card.
2. **What was read and spiked**: versions, files, and what the prototype showed (below).
   **Zed upstream since the 0.3.9 snapshot**: its accessibility commits and PRs (editor text area,
   AccessKit version bumps, macOS fixes) and later `gpui-pre` releases on crates.io. Would upgrading bring
   any of it, and at what cost? Licences must be Apache-2.0-compatible.
3. **Platform path options**, with effort, risks, upgrade cost and Linux impact for each:
   - (a) gpui-pre's AccessKit as shipped
   - (b) vendoring gpui-pre and/or accesskit_macos under `vendor/` with patches, as tree-sitter is
     (`vendor/tree-sitter/README.md`), for gaps found
   - (c) direct NSAccessibility on GPUI's NSView for the editor only, bypassing AccessKit
   - (d) waiting for upstream

   Pick one, and say what would make you switch.
4. **Editor text area**: exact NSAccessibility text-area semantics through AccessKit (`Role::TextInput` /
   multiline, `TextRun`, `TextSelection`):
   - the value
   - `accessibilityNumberOfCharacters`
   - `accessibilitySelectedTextRange`
   - `accessibilityInsertionPointLineNumber`
   - line-for-index and range-for-line
   - string- and bounds-for-range
   - `accessibilityVisibleCharacterRange`
   - which of these accesskit_macos 0.26.3 answers, and which it can't

   Decide:
   - **How much text to expose** (viewport only, viewport ± N rows, or whole file under a size cap), and
     what VoiceOver does when it asks for a line outside that window (does a line move scroll it into the
     tree?)
   - How folds, soft wrap, inlay hints, ghost text, sticky scroll and the minimap map to positions or are
     hidden
   - Multi-cursor (one AccessKit selection: primary only)
   - Vim normal mode's block caret
   - Typing echo and caret-move speech (it follows from selection changes)
   - Diagnostics at the caret
   - The read-only file viewer
   - Performance: build runs only while `is_a11y_active()`, and give a budget per keystroke against PERF.md

   Name the elements: `crates/fe-gpui/src/element.rs` `EditorElement`,
   `crates/fe-gpui/src/file_viewer/element.rs`, and `kiln/crates/kiln-ui/src/text_field.rs` `TextLine` as
   the small template. The terminal and chat come later.
5. **Announcements**: live regions for toasts, the status bar, the find bar's match count, and vim mode and
   command-line messages. Does accesskit_macos turn `Live` into `NSAccessibilityAnnouncementRequested`? If
   not, what is the workaround?
6. **Chrome semantics**: landmarks, tabs, project tree, palette and pickers (active descendant),
   in-window prompts (`kiln/crates/kiln-ui/src/prompt.rs`), settings controls, find bar, completion and
   hover.
   - Prefer changes in shared kiln-ui widgets (`tab_bar.rs`, `tree_row.rs`, `palette.rs`, `controls.rs`,
     `icon_button.rs`, `status_bar.rs`) so one card covers many views.
   - Native NSOpenPanel and NSAlert are AppKit's, and already accessible: say which dialogs are native and
     which in-window.
7. **Keyboard reachability**: list every control that keyboard alone can't reach or operate today, by
   surface, with the source `path:symbol`. Cover tab order, Full Keyboard Access, the focus ring being
   visible (ties to the polish audit), and escape and return in every modal.
8. **System settings to respect**, each with the AppKit API, how it is observed live
   (`NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`), and what Tenmoku does:
   - Reduce Motion (set `App::set_reduce_motion` from `accessibilityDisplayShouldReduceMotion`)
   - Increase Contrast (`accessibilityDisplayShouldIncreaseContrast`: a high-contrast token rule or theme)
   - Reduce Transparency
   - Differentiate Without Colour (git status letters, diagnostics, diff colours)
   - larger text: AppKit has no dynamic type here, so propose a `workbench.ui_font_size` or zoom setting
     and the type-ramp dependency on the polish audit

   Say where the code lives (kiln-platform versus the app).
9. **Testing strategy**, given that headless has no tree:
   - Can the test platform be forced active (a test-support hook, `debug_a11y_tree_json`)?
   - the setup wizard's label-assertion pattern
   - the `FASTEDIT_A11Y_DUMP` running-app dump the audit proposed (`crates/fastedit/src/main.rs`)
   - which checks need the human

   Name the regression-test shape implementation cards should use.
10. **Recommended first slice** for the first public release, and its non-goals. Suggested: the dump hook,
    the chrome landmarks plus tabs, tree, palette and prompt, the `TextLine` template, then the editor text
    area, and Reduce Motion plus Increase Contrast.
11. **Proposed card breakdown**: ordered cards with size (xs..xl), dependencies, Mac-only or Linux-OK,
    and exact scope globs (no `crates/*/src/lib.rs`). Reconcile with `docs/accessibility-audit.md`'s ten
    follow-ups: keep, merge, reorder or drop each. Add human VoiceOver check action cards where needed.
12. **Human checks**: a short VoiceOver and Accessibility Inspector script for the user to run, both on
    today's build (baseline) and after the first slice.

## Prototype

A throwaway spike is welcome, for example forcing a11y active, or giving `TextLine` a `TextRun` to see what
VoiceOver or Accessibility Inspector reports. It must not land:
- do it in a separate `git worktree` or branch outside the lane's candidate, and don't push it
- record what it showed in section 2, with the commands to reproduce it
- don't commit the prototype code into this card's diff

Agents here can't drive VoiceOver or grant screen access, so anything only a person can observe goes into
section 12 as a check.

## Allowed edit outside the decision doc

One line at the top of `docs/accessibility-audit.md`, pointing to the new design doc as its follow-up
plan. Nothing else in that file.

## When this lands

The lead brings the doc to the user. **No implementation card is readied until the user approves the
platform path (section 3) and the first slice (section 10).**

## Out of scope

Any Rust, Cargo or settings change. Filing implementation cards. Terminal and chat a11y design, beyond
noting where they slot in.

## Conflicts

Docs only. `docs/accessibility-audit.md` is in no open card's scope. The polish audit card filed beside this
one shares one topic, Reduce Motion and the focus ring: this doc owns the system-setting plumbing, and the
polish audit owns the visual rules.
