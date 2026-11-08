---
task_uid: "01m6w2za6a8zb05dfya7tnzhgb"
task: "T521"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p kiln-l10n passes, including the section 5 isolation and selector tests and the section 8 lint fixtures (every row of the reported / not reported table)"
    required: true
    manual: false
  - id: "C2"
    text: "Lint fixtures for l10n-allow.toml: with migrated = [a.rs] only a.rs is checked; with all = true every file is; an entry without a reason is an error"
    required: true
    manual: false
  - id: "C3"
    text: "cargo deny check licenses passes with the Fluent crates (no deny.toml change)"
    required: true
    manual: false
---

# kiln-l10n: the Fluent catalogue, tr!/text!/Text/verbatim!, resources!, isolation, and the lint with an append-only migrated list (i18n I1) (Mac T1520)

# kiln-l10n: the Fluent wrapper, tr!/text!/Text/verbatim!, resources!, and the lint (I1)

Spec: card **I1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for I1 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- **The per-area switch is append-only.** § 8 "The allow-list file" switches the
  lint on area by area by *removing* files from a `pending` list. Make it a
  `migrated = [...]` list instead: while a crate's `l10n-allow.toml` has
  `migrated`, the lint checks only the listed files; a card that migrates
  files *adds* their lines; the crate's last card replaces the list with
  `all = true`. Adding lines lets area cards run in parallel (the lead adds the
  allow files to `regatta.toml`'s `scopes.shared_append_only`). Fixtures cover
  both forms. (Lead decision, to confirm with the user with T1432.)
- Root `Cargo.toml` / `Cargo.lock` are shared append-only: add the member and
  packages as added lines.

Platform: **Linux-OK**.

## Implementation notes

- **Isolation bypasses `format_pattern`.** `FluentBundle::format_pattern` ends
  in `FluentValue::into_string`, which runs the bundle's formatter over the
  *whole* result, so an RTL locale (or an en-US message whose text holds Hebrew)
  came back wrapped in FSI…PDI once more. `kiln-l10n` writes a message's
  pattern element by element through the public `resolver::Scope::maybe_track`
  instead (`catalogue::write_pattern`), so only placeables reach the formatter.
  The root check's formatting pass uses the same function.
- **`resources!`** defines `pub static RESOURCES: kiln_l10n::Resources` in the
  calling module (with `#[allow(unreachable_pub)]`) and submits it to
  `inventory`. Dynamic prefixes are declared first:
  `resources! { dynamic_prefixes = ["command-"]; "en-US" => [...] }`. It records
  `file!()` so `lint::keys` can resolve the listed paths and report a `.ftl`
  under `l10n/` that isn't listed.
- **`l10n-allow.toml`**: exactly one of `all = true` or `migrated = [...]`
  (neither, both, or `all = false` is an error); `[[file]] path, reason` and
  `[[literal]] file, text, reason`. A missing or blank reason, a path that
  isn't non-test source of the crate, a migrated file listed twice, and a
  `[[literal]]` that matches nothing in a checked file are errors.
- **Non-test source** is what the module trees of `src/lib.rs`, `src/main.rs`
  and `src/bin/*` reach (following `#[path]`), with `#[test]` items and items
  under a `cfg` that needs `test` (`test`, `all(test, …)`) removed.
- **Root checks** are `lint::catalogue()`, for I2's and I3's root tests:
  `.crates_under(dir)` reads each crate with a `Cargo.toml` (its `l10n/`, its
  non-test source's string literals, and its `resources!` dynamic prefixes);
  `.data_keys(..)`, `.dynamic_prefix(..)` and `.exempt_public()` cover bundled
  data and kiln's public keys. Its report keeps each check's errors apart
  (`parse`, `walk`, `format`, `duplicates`, `stale`, `unused`, and the
  non-fatal `missing` counts).
- **`tr_or(key, fallback)`** and `attribute(key, name, args)` miss silently,
  for I3's `command_label` / `command_detail`; `try_format` returns the
  formatted text with Fluent's errors and the locale that had the message.
- Both `deny.toml` and `kiln/deny.toml` pass `cargo deny check licenses`
  unchanged (the latter in a `kiln_standalone.py prepare` scratch workspace;
  cargo-deny 0.20 has no `--config`).
