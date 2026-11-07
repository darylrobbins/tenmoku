---
task_uid: "01m6smzd4ya8vxf7xte5e7hqsw"
task: "T1432"
rev: 2
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/decisions/\u003clane\u003e-i18n-design.md exists with sections Status, Library, Catalogue layout, Fetching strings per surface, Plurals and arguments, Locale selection, Bundle, Keeping the catalogue complete, Migration plan, Cost to every future card, Out of scope and open questions; each ends in a decision or a named open question"
    required: true
    manual: false
  - id: "C2"
    text: "The Library section gives the licence of every crate each option pulls in and whether it passes deny.toml's [licenses] allow list unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "The Migration plan lists each card with size (s or m only), scope globs, dependencies, Linux-OK or Mac-only and a string count, covering crates/fe-workbench, crates/fe-gpui, crates/fe-editor, crates/fe-vim, crates/fe-config, crates/fe-chat-view, crates/fastedit and kiln/crates/*. Documented limit: the plan covers CommandMeta readers only; the typed-command path (CommandSpec, CommandInfo, TypedCommand and their consumers), the per-locale-bundle reference check (F10) and the live-locale settings-template writer (F14) are known gaps, named in the doc's Out of scope and open questions section and tracked by follow-up card T1506"
    required: true
    manual: false
  - id: "C4"
    text: "The doc's status line says 'proposed' and names the user as the approver; no string is moved or translated and no implementation card is filed by this card"
    required: true
    manual: false
  - id: "C5"
    text: "git diff --stat against the base lists only the new doc"
    required: true
    manual: false
---

# Design the UI string catalogue: library, catalogue layout, how each surface fetches strings, plurals, locale selection, bundle localisations, a completeness lint and the per-crate migration plan

Decide and document how Tenmoku's user-facing strings move into a translatable catalogue, before any string is moved or translated. A design card: one doc, no code. User-approved 2026-11-06. It waits for the UI polish audit (T1429) so strings are not moved twice; when this lands the lead brings the doc to the user before any implementation card is filed.

## Problem

Every user-facing string is a Rust literal today: about 930 capitalised literals in crates/fe-workbench/src (non-test), ~190 in crates/fe-vim/src, ~110 each in crates/fe-gpui/src and crates/fe-editor/src (command titles such as "Column Select Left" in column_select.rs), ~180 across kiln/crates/{kiln,kiln-core,kiln-shell,kiln-platform}/src (menus: kiln-core/src/menu.rs, kiln-platform/src/menus.rs, kiln-shell/src/menu_bar.rs, kiln/src/setup/defaults.rs), plus crates/fe-config (settings.schema.json and settings-template.toml descriptions), crates/fe-chat-view, crates/fastedit/src/main.rs, and accessibility labels the a11y follow-ups will add (docs/accessibility-audit.md, T389; Mac follow-up T1266). Nothing is in a catalogue and nothing selects a locale. Count again with `rg -o '"[A-Z][a-z][^"]{2,}"' <dir> --type rust -g '!*tests*'` and say how many are user-facing versus ids, log text and test data.

## Deliverable

docs/decisions/<lane>-i18n-design.md (the same `<lane>-` prefix convention as the other files in docs/decisions/), with these sections, each ending in a decision or a named open question for the user:

1. **Status**: 'proposed', names the user as approver, and says no string was translated or moved.
2. **Library**: compare Mozilla Fluent (fluent-bundle, fluent-templates, with unic-langid / intl-memoizer), rust-i18n, and gettext (gettext-rs links GNU gettext, LGPL: say what that means here). For each: licence of every crate it pulls in, checked against deny.toml's `[licenses] allow` list (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, ...; MPL-2.0 only as a listed exception, and widening deny.toml needs a human); compile-time versus runtime loading; plural and gender support (CLDR rules) and number/date formatting (ICU4X `icu_*` crates are already in Cargo.lock via icu_collator: check which); binary size and startup cost (PERF.md startup budget); whether the crate must stay GPUI-free so headless crates (fe-vim, fe-editor, fe-config; scripts/check-no-gpui.sh) can use it; and kiln's boundary (kiln/ stands alone, Apache-2.0, may not name fastedit: scripts/check_kiln_boundary.py) so kiln needs its own catalogue or a host-supplied lookup. Recommend one.
3. **Catalogue layout**: where files live (per crate, or one app catalogue plus kiln's own), file naming per locale, how they are embedded (include_str!/build.rs/macro) and how a missing key or locale falls back (to en-US, never a panic, logged once).
4. **Fetching strings, per surface**: GPUI views and kiln widgets (a `t!`-style macro or a function taking `&App`; how a view re-renders when the locale changes), native menus (kiln-platform, rebuilt on change), command palette titles and categories (kiln-command registration: a key, not a literal), settings descriptions (settings.schema.json and settings-template.toml: generated from the catalogue, or keyed), notifications and toasts, dialogs, vim messages (decide: keep nvim's English E-numbered messages verbatim, as vim users search them, or translate the text but keep `E492:` prefixes), which-key group names and the LazyVim preset (T1395), accessibility labels, plugin and VS Code extension contributions (extensions' package.nls.<locale>.json and `%key%` titles: docs/decisions/vmtnmkay62-extensions-and-plugins.md), and CLI `--help`/errors in crates/fastedit. Show each with a before/after snippet.
5. **Plurals and arguments**: how counts ("3 files changed"), names and paths are passed and formatted; no string concatenation of fragments; bidi isolation of user-supplied arguments (Fluent's FSI/PDI marks) and how that interacts with the editor's own text rendering.
6. **Locale selection**: follow macOS preferred languages (NSLocale preferredLanguages, through kiln-platform; Linux: LANG/LC_MESSAGES) negotiated against the shipped locales, with a setting to override (name it, e.g. `workbench.locale`, its schema entry and whether it applies live or on restart); what happens to keybinding labels, number formats and the vim keyboard layout.
7. **Bundle**: CFBundleLocalizations and CFBundleDevelopmentRegion in crates/fastedit/macos/Info.plist (scripts/bundle-macos.sh copies and edits it), whether .lproj folders are needed so AppKit's own menu items (Services, Hide, Quit) and Sparkle's UI localise, and what the Sparkle updater shows.
8. **Keeping the catalogue complete**: a check that rejects a new hard-coded user-facing string, with an allow-list (file or attribute), where it runs (a test in the affected crate, or a script beside check_kiln_boundary.py run by the no_gpui stage; say which and why), how it tells a user-facing literal from an id, key or log string with few false positives, and a second check that every key used exists in en-US and no key is unused. Note that regatta.toml and the gate stages are human-approved paths if the check needs a new stage.
9. **Migration plan**: split by crate into s and m cards (never l), each with scope globs, dependencies, Linux-OK or Mac-only, and an estimate of strings moved; order them so the infrastructure card lands first and the lint is switched on per crate as each crate is migrated. Name which strings T1429's polish follow-ups are likely to rewrite, and order those crates after them.
10. **Cost to every future card**: what a card that adds UI text must now do (add a key, an en-US entry, maybe a test), how the card template and the codex reviewer should check it, and the expected overhead per card.
11. **Out of scope and open questions**: translating anything, choosing launch locales, a translation service or community workflow.

## Criteria notes

No implementation card is filed by this card. Do not touch any file but the new doc; do not edit deny.toml, Cargo.toml, Info.plist or scripts.
