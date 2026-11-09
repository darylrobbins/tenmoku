---
task_uid: "01m6xrpv9msybg5g0w1p3zhd9s"
task: "T531"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The three workbench.locale tests of section 6 pass (xx on system de with de shipped gives de and a diagnostic naming de; xx on system fr gives en-US and says en-US; a shipped override, no diagnostic)"
    required: true
    manual: false
  - id: "C2"
    text: "Test catalogue_loads_without_duplicates (cargo nextest run -p fastedit --test l10n_catalogue) passes"
    required: true
    manual: false
  - id: "C3"
    text: "On Linux, the FSI/PDI test of section 5 passes (the marks draw nothing)"
    required: true
    manual: false
  - id: "C4"
    text: "fastedit --perf before/after recorded in the decision file, with the main-thread work locale selection adds before the first frame (the warm() call and the first application of workbench.locale, as release --perf times them) under 1 ms at p90; the end-to-end bound on a quiet machine is T533's (Known limits)"
    required: true
    manual: false
---

# Locale selection: sys-locale preferred languages, ActiveLocale, workbench.locale, catalogue warm-up off the startup path (i18n I2) (Mac T1522)

# Locale selection: sys-locale, ActiveLocale, workbench.locale, warm-up (I2)

Spec: card **I2** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for I2 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- `crates/fastedit/src/main.rs` is also in T1484/T1485/T1411's scopes; the
  warm-up is a few added lines at the top of `main`.
- The Linux FSI/PDI check of § 5 is a test here, as planned.

Platform: **Linux-OK**.

## Known limits

- **C4 is the main thread's cost, not the end-to-end bound.** helium is
  shared with other lanes' builds (loads of 25–126 on 24 cores while T531
  ran), and release `tenmoku --perf` launches spread over hundreds of
  milliseconds there, so a before/after of main-to-first-frame can't
  resolve 1 ms (review F2, twice). C4 is narrowed to what `--perf` times in
  the release app: the main-thread work locale selection adds before the
  first frame. The warm-up thread's competition with the main thread, and
  the end-to-end before/after on a quiet machine, are T533's.

## Implementation notes

- **kiln-platform `locale.rs`**: `preferred()` (sys-locale, dropping
  `C`/`POSIX` and non-tags), `requested(choice, system)`, `negotiate`,
  the `ActiveLocale` global (chain, language, catalogue generation),
  `active(cx)`, `warm()` and `set_locale(cx, requested)`. `set_locale`
  rebuilds the bundles only when the negotiated chain differs from the one
  this module last set (or the catalogue was switched around it, which its
  generation shows); it refreshes menus and windows only when the text
  changed, so the startup call after the warm-up does neither. `warm()`
  and `set_locale` share one lock, so whichever comes first wins and the
  warm-up never overwrites the setting. `config.rs` needed no change.
- **fe-config**: `WorkbenchSettings::locale: LocaleSetting` (`Auto` or
  `Tag(String)`, any string accepted), with a hand-written `JsonSchema`
  whose `enum` is `auto` plus `kiln_l10n::shipped_locales()` (kiln-core's
  schema check doesn't check `enum`s, so an unshipped tag still loads).
  `LocaleSetting::choose(system, shipped)` gives the requested languages,
  the chain and the problem; `LocaleSetting::diagnostic` places it as a
  warning at `workbench.locale` in the user's file. A tag counts as used
  when it negotiates to a shipped locale on its own (`fluent-langneg`'s
  filtering, as kiln-l10n's): `de-CH` takes a shipped `de` silently, but
  `zh-Hant` on a `zh-Hans` system with `zh-Hans` shipped warns (review F1;
  a check on the whole chain's language subtags hid it). User-only: the `workbench` project filter now drops
  `locale` too, and `SettingsModel::workspace_allowed`/`applied_project`
  list it. Two messages in `l10n/en-US/config.ftl`:
  `config-locale-unshipped` and `config-project-locale-ignored`; fe-config's
  `keys_used_here_exist` test is on, the literal lint is left to C1.
- **C1's tests** are `fe-config tests::locale::*` (the three named cases,
  plus `auto`/bad tags and the user-only/schema/template checks).
- **crates/fastedit**: `warm()` right after logging starts in `main`; a
  `Locale` config source in `config::shipped` applies the setting at the
  service's start and on reloads, and publishes the warning in the config
  diagnostics. `tests/l10n_catalogue.rs` has
  `catalogue_loads_without_duplicates` (every crate with an `l10n/` is
  linked, one catalogue per shipped locale builds without errors) and
  `app_catalogue_is_complete` (`lint::catalogue()` over both trees plus the
  bundled menus' `title_key`s; the preset's group names wait for V2, which
  keys them). No crate other than fe-config has a catalogue yet.
- **FSI/PDI on Linux** (`kiln-platform tests/l10n.rs`
  `bidi_isolates_draw_nothing`): with cosmic-text the marks shape to a
  zero-advance glyph with no raster bounds; the line is as wide as without
  them and the other glyphs, Hebrew reordered, sit where they would.
- **Scope**: respecced to add `Cargo.lock`, `crates/fe-config/src/lib.rs`,
  `crates/fe-config/src/settings_model.rs` and the decision doc (C4).
- **C4**: recorded in the decision doc after § 6's decision. Under heavy
  shared load the end-to-end runs spread over 200 ms, which can't resolve
  1 ms (review F2), so `--perf` now prints
  `open: l10n on the main thread: …`: the release app's own time in
  `warm()` and in applying `workbench.locale` at the config service's
  start, waiting on the warm-up included (`perf.rs`, scope added by
  respec). Those are the only main-thread additions before the first
  frame: median 99 µs, p90 123 µs over 30 release launches at load
  101–126 (one outlier, a thread spawn, at 1.2 ms). The second review
  still asked for the end-to-end bound, which this machine can't give:
  C4 is respecced to the main thread's cost (Known limits) and T533
  measures end to end on a quiet machine.

## Implementation notes

- **kiln-platform `locale.rs`**: `preferred()` (sys-locale, dropping
  `C`/`POSIX` and non-tags), `requested(choice, system)`, `negotiate`,
  the `ActiveLocale` global (chain, language, catalogue generation),
  `active(cx)`, `warm()` and `set_locale(cx, requested)`. `set_locale`
  rebuilds the bundles only when the negotiated chain differs from the one
  this module last set (or the catalogue was switched around it, which its
  generation shows); it refreshes menus and windows only when the text
  changed, so the startup call after the warm-up does neither. `warm()`
  and `set_locale` share one lock, so whichever comes first wins and the
  warm-up never overwrites the setting. `config.rs` needed no change.
- **fe-config**: `WorkbenchSettings::locale: LocaleSetting` (`Auto` or
  `Tag(String)`, any string accepted), with a hand-written `JsonSchema`
  whose `enum` is `auto` plus `kiln_l10n::shipped_locales()` (kiln-core's
  schema check doesn't check `enum`s, so an unshipped tag still loads).
  `LocaleSetting::choose(system, shipped)` gives the requested languages,
  the chain and the problem; `LocaleSetting::diagnostic` places it as a
  warning at `workbench.locale` in the user's file. A tag counts as used
  when it negotiates to a shipped locale on its own (`fluent-langneg`'s
  filtering, as kiln-l10n's): `de-CH` takes a shipped `de` silently, but
  `zh-Hant` on a `zh-Hans` system with `zh-Hans` shipped warns (review F1;
  a check on the whole chain's language subtags hid it). User-only: the `workbench` project filter now drops
  `locale` too, and `SettingsModel::workspace_allowed`/`applied_project`
  list it. Two messages in `l10n/en-US/config.ftl`:
  `config-locale-unshipped` and `config-project-locale-ignored`; fe-config's
  `keys_used_here_exist` test is on, the literal lint is left to C1.
- **C1's tests** are `fe-config tests::locale::*` (the three named cases,
  plus `auto`/bad tags and the user-only/schema/template checks).
- **crates/fastedit**: `warm()` right after logging starts in `main`; a
  `Locale` config source in `config::shipped` applies the setting at the
  service's start and on reloads, and publishes the warning in the config
  diagnostics. `tests/l10n_catalogue.rs` has
  `catalogue_loads_without_duplicates` (every crate with an `l10n/` is
  linked, one catalogue per shipped locale builds without errors) and
  `app_catalogue_is_complete` (`lint::catalogue()` over both trees plus the
  bundled menus' `title_key`s; the preset's group names wait for V2, which
  keys them). No crate other than fe-config has a catalogue yet.
- **FSI/PDI on Linux** (`kiln-platform tests/l10n.rs`
  `bidi_isolates_draw_nothing`): with cosmic-text the marks shape to a
  zero-advance glyph with no raster bounds; the line is as wide as without
  them and the other glyphs, Hebrew reordered, sit where they would.
- **Scope**: respecced to add `Cargo.lock`, `crates/fe-config/src/lib.rs`,
  `crates/fe-config/src/settings_model.rs` and the decision doc (C4).
- **C4**: recorded in the decision doc after § 6's decision. Under heavy
  shared load the end-to-end runs spread over 200 ms, which can't resolve
  1 ms (review F2), so `--perf` now prints
  `open: l10n on the main thread: …`: the release app's own time in
  `warm()` and in applying `workbench.locale` at the config service's
  start, waiting on the warm-up included (`perf.rs`, scope added by
  respec). Those are the only main-thread additions before the first
  frame: median 99 µs, p90 123 µs over 30 release launches at load
  101–126 (one outlier, a thread spawn, at 1.2 ms).
