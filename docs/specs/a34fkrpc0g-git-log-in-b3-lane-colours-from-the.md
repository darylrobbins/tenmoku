---
task_uid: "01m75b0mmgneew73a34fkrpc0g"
task: "T348"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::git_log_b3 (cargo nextest run -p fe-workbench) pass: GRAPH_COLORS and avatar_color are gone; lane colours come from the theme's signal roles whenever any signal role is eligible, and never equal accent.base or agent.base; a theme with no eligible signal colour is a known limit, closed by follow-up T350; the card is surface.raised with the float shadow and opens after 300 ms"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Git log in B3: lane colours from the theme, rows, the commit card's floating look (Mac T1551)

# Git log in B3

Spec: `docs/design/design-system.md#signals` (one meaning per colour) and
`#surfaces`. Audit findings GL1, GL3.

## Change (`crates/fe-workbench/src/git_log.rs`, `git_log_menu.rs`, `git_history.rs`)

- Graph lane colours from the theme (a list read from the signal roles, never
  the accent or agent colour, which mean you and agents); avatars from the
  same list; the `main` label in `surface.raised` with ink text.
- Rows 24 px; hash and date in faint code small, subject in ink, author in
  muted. An agent's commit (from provenance where known) gets the agent colour.
- The commit card: the floating-surface look, shown after 300 ms.
- An audit scene `audit/git_log.rs` with the card open (the audit card's hook).

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `git_log.rs`, `git_log_menu.rs` and `git_history.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/git_log.ftl` (keys `workbench-git-log-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Linux-OK**.

## Implementation notes

- Lane colours (`GraphColors`, `git_log.rs`): `status.modified`, `success`, `warning` and `error`, less any equal to `accent.base` or `agent.base`; avatars hash the email onto the same list.
- An agent's commit has no provenance in git: it's one with a `Co-authored-by` trailer whose email is `@anthropic.com` or `@openai.com`. Its author shows in the agent colour.
- The card delay is gpui's `tooltip_show_delay`; `git_log_card_tests` moved from 400 ms to 250 ms for "not yet".
- The audit scene `git_log` now opens the card; its probe point (250, 465) is unchecked: the goldens are Metal-only, the Mac verifies.
