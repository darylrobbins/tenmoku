---
task_uid: "01m763j8vex5djtahrbddszr55"
task: "T352"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Tests in tests::git_log_b3 (cargo nextest run -p fe-workbench) pass: the Git log's prose that was assembled from fragments is whole catalogue messages in git_log.ftl, counts (the signed line counts and the overflow ref count) are passed as numbers, and the messages render byte-equal in English; driving the production rendering paths in the regression test (review F1) and rendering styled spans from the complete formatted message (review F2) are a known limit, left to follow-up T354"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist)"
    required: true
    manual: false
---

# Git log: no user-visible text assembled outside the catalogue (Mac T1551 follow-up F3)

# Git log: no user-visible text assembled outside the catalogue (follow-up of Mac T1551, review F1)

Mac T1551 (argon T348) moved the Git log's text into `crates/fe-workbench/l10n/en-US/git_log.ftl`. Its last review (approve, medium finding F1, below blocking severity) found the migration incomplete: user-visible messages in `git_log.rs` (three places, first near the commit-trailer and message code and a second near the panel's state types, as the review cited lines 446 and 586 of the T348 version) are still assembled from fragments outside the catalogue instead of being whole messages. The Mac lead asked for it as a follow-up.

## Change (`crates/fe-workbench/src/git_log.rs`, `git_log_menu.rs`, `git_history.rs`, `l10n/en-US/git_log.ftl`)

Audit the three migrated files for any user-visible text built by `format!`, `push_str`, joins or concatenation of literals and translated pieces. Each such message becomes a whole catalogue message (`workbench-git-log-…`) through `tr!` / `text!`, with placeholders and plural or select selectors for counts and variants, as `docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4-5 require. Text it keeps stays byte-equal in English. Add a test in `tests::git_log_b3` that the messages render as before.

Platform: **Linux-OK**.
