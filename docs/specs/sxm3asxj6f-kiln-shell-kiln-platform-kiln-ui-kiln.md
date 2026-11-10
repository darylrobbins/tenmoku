---
task_uid: "01m71y9e18rsdp6zsxm3asxj6f"
task: "T550"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p kiln-shell -p kiln-platform -p kiln-ui -p kiln-plugin -p kiln passes with each crate's l10n tests on (ui_text_is_in_the_catalogue, keys_used_here_exist) and no migrated list left in those crates"
    required: true
    manual: false
  - id: "C2"
    text: "en-US text byte-equal: no kiln golden changes (on helium, every gallery page renders byte-identical before and after, dark and light; the Mac compares its goldens after the sync)"
    required: true
    manual: false
---

# kiln-shell, kiln-platform, kiln-ui, kiln-plugin and kiln strings; lint on for those crates (i18n K2) (Mac T1528)

# kiln-shell, kiln-platform, kiln-ui, kiln-plugin and kiln strings (K2)

Spec: card **K2** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for K2 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- The plan waits for the audit's P2, P7, P13 and P18. P2 and P18 (palette and
  pickers) are still to design and not filed; P7 and P13 are replaced by the
  B3 surface cards, which now come **after** this card and add their new text
  as keys. Migrate today's text as it is.
- The B3 controls card (kiln-ui) and the command-label resolver
  (kiln/src/contributions.rs) land first.
- `kiln-platform/src/screenshots/goldens.rs` is developer tooling: allow-list
  the file.

Platform: **Linux-OK**.

## Implementation notes

- **Lint on, `all = true`, in all five crates.** Each has `l10n-allow.toml`
  with no `migrated` list and its `tests/l10n.rs` (kiln-shell's in its one
  `it` binary) with `ui_text_is_in_the_catalogue` and
  `keys_used_here_exist`. kiln-shell's and kiln's earlier
  `*_keys_used_here_exist` moved there. kiln-platform, kiln-ui and
  kiln-plugin gain `src/l10n.rs`; kiln-plugin gains the `kiln-l10n`
  dependency, which is GPUI-free as kiln-plugin must be.
- **Messages moved.** kiln-shell: the palette's placeholder and no-match
  status; the trust prompt's buttons. kiln-ui: the pinned tab's and the
  shortcut tooltips as whole messages. kiln-platform: the open panel's
  failure prompt. kiln: theme, contributed key and menu, and extension
  diagnostics. kiln-plugin: capability descriptions and grant warnings
  (the "a, b and c" join as `kiln-plugin-and-list`), the grants file's
  header, and its plugin, command, process, network, manifest and secrets
  errors. `thiserror` stays for `source()`; each `Display` is written by
  hand, in the active locale. Fragments became whole messages with
  selectors (the unload reason, the capability's action, the secrets
  index's action, a theme's appearance).
- **Stored text.** `TrustPrompt`'s question and buttons are `Text`,
  resolved when the prompt shows, as a host builds the prompt once.
  `TrustPrompt::new` takes `impl Into<Text>`, so the app's `&str` question
  still builds. A plugin `Diagnostic`'s message is resolved when the
  plugin unloads, as a toast's is.
- **Escape on a translated Cancel.** `Prompt::cancel_button` matched only
  the English title "Cancel" (T549's carried F1). It now also matches
  `kiln-ui-cancel` in the active locale, a `## public` message an app's
  Cancel buttons can use; tested with a fake locale.
- **Allowed, with reasons:** element IDs and debug selectors, key names,
  key paths, file and thread names, temporary file names, doc comments
  that macros see as string tokens, matchers on wasmtime's text, the
  developer-facing `kiln::module!` warning and `fail_fast` panic,
  `kiln::testing`, font family names, and the screenshot harness (all of
  `kiln-platform/src/screenshots/`, not only `goldens.rs`: it's one
  developer tool). The platform's app command titles (`Quit` and the rest)
  stay as their registered English fallbacks; `label()` reads kiln's
  `command-app-*` and `command-window-*` keys.
- **Outside the crates:** `kiln_platform::dialogs::open_panel_failed`
  returns its title as a `String` now, so fe-workbench's one-line wrapper
  in `workbench.rs` changes its return type (respecced in, with
  `Cargo.lock` and `kiln-ui/tests/**`).
- **en-US byte-equal.** Each crate's l10n test formats every moved
  message, or the error that picks it, against the text it replaced, and
  the existing tests that assert English text pass unchanged. No golden
  changed.
- **C2 on helium (respecced).** The gallery's goldens are Metal's, compared
  only on macOS. On helium, `gallery-screenshots render` drew every page
  (controls, text, lists, layout, chrome, prompts, theme), dark and light,
  at the base and at this branch: all 14 PNGs are byte-identical. No file
  under `examples/gallery-screenshots/golden` changed. The Mac compares
  its goldens after the sync.
