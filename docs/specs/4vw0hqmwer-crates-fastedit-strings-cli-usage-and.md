---
task_uid: "01m79fa18am1kwmp4vw0hqmwer"
task: "T600"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fastedit --test l10n passes with crates/fastedit's lint on for the whole crate (macOS-only sources linted from source)"
    required: true
    manual: false
  - id: "C2"
    text: "tenmoku --help prints today's text byte for byte in en-US, with no FSI/PDI marks (a test)"
    required: true
    manual: false
---

# crates/fastedit strings: CLI usage and errors, updater and speller messages, theme and grammar install messages (i18n F1) (Mac T1535)

# crates/fastedit: CLI usage and errors, updater and speller messages (F1)

Spec: card **F1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for F1 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- Scope is the files that carry text today, not `src/**`, because T1484 and
  T1485 hold `grammar_registry.rs`, `grammar_install.rs` and `main.rs`; waits
  for T1485 for `grammar_install.rs`.

Platform: **Linux-OK** (the macOS-only sources are linted from source).

## Implementation notes

- Migrated: `--help` and the unknown-flag error (system locale, isolation marks stripped), the shell command's reports, theme.name warnings, grammar install errors and toast.
- Allow-listed with reasons (log-only or dev tooling, or another task's files): perf, velopack_update, update_signature, sparkle, ipc, log_file, config, vim_remaps.
- `mod l10n` is declared from cli.rs (non-test) and theme.rs (test builds), since main.rs belongs to T1485; test binaries that include those files declare it too (respec added l10n_catalogue, instance_handoff, wait_close).
