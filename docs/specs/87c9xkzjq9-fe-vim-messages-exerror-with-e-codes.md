---
task_uid: "01m6w2zcs0a0avsb87c9xkzjq9"
task: "T524"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-vim passes, the nvim oracle tests included (en-US text equals nvim's)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim --test l10n passes for every fe-vim file but which_key.rs and remap.rs (V2's)"
    required: true
    manual: false
---

# fe-vim messages: ExError with E-codes kept, summaries as plurals, en-US equal to nvim's English (i18n V1) (Mac T1524)

# fe-vim messages, E-codes kept, en-US equal to nvim's English (V1)

Spec: card **V1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for V1 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- Waits for T1496 (and through it T1495, T1490), which change
  `crates/fe-vim/src/ex/quickfix.rs`.

Platform: **Linux-OK**.

## Implementation notes

- **Catalogue.** `crates/fe-vim/src/l10n.rs` lists `l10n/en-US/vim.ftl`
  (messages) and `l10n/en-US/commands.ftl` (command titles). `l10n-allow.toml`
  has `migrated = [...]` with every non-test file but `which_key.rs` and
  `remap.rs`, which V2 adds before switching to `all = true`.
- **E-codes.** `ExError`'s `Display` is written by hand: each variant is
  `messages::coded("E492", tr!("vim-e492", …))`, the code outside the
  catalogue. `fe_vim::messages` exports what other crates say
  (`not_an_editor_command`, `no_range_allowed`, `shell_refused`, …):
  `fe-gpui/src/vim.rs` and `fe-workbench/src/git_commands.rs` call them, and
  so does `fe-workbench/src/vim_filter.rs`, in place of the removed
  `SHELL_REFUSED` const.
- **Plurals.** `:s`'s report is two whole messages, `vim-substitute-matches`
  and `vim-substitute-substitutions`, each selecting on `$lines` and then on
  the count, as nvim's `NGETTEXT` pairs do; `{N} lines filtered` selects on
  `$count`. Size-limit errors (copies, carets, rows, bytes, keys, composing
  characters) select on each count too; en-US keeps today's invariant
  English ("putting 1 copies…") in every branch.
- **Whole messages.** `ExError::Unsupported` and every `PatternError`
  variant hold their whole message: a built-in reason is one catalogue
  message with its wording around it (`vim-unsupported-*`,
  `vim-pattern-*`), and only text as typed or `regex`'s own message goes
  through a generic wrapper (`ExError::unsupported`,
  `PatternError::unsupported`, `vim-pattern-invalid-regex`,
  `vim-pattern-too-large-regex`).
- **Command titles.** The lint covers `commands.rs` and every feature's
  registration, so fe-vim's 59 command titles and descriptions moved to
  `command-<id>` with a `.description` attribute, the key I3's
  `command_label`/`command_detail` will read. Until I3 lands the registry is
  built from them (`l10n::command_title`, `command_description`);
  `tests/l10n.rs` checks every command has its key and every key a command,
  and the registered text was compared byte for byte against the old
  literals.
- **API changes.** `Pick`'s `title` and `header` are `String` (were
  `&'static str`), `PatternError::Invalid` holds a `String`, and
  `SHELL_REFUSED` became `messages::shell_refused()`; scope widened to
  `fe-workbench/src/vim_pick.rs`, `vim_filter.rs`, `vim_filter_tests.rs`,
  `fe-gpui/src/vim/macro_tests.rs` and `crates/fe-vim/tests/**` for their
  readers.
- **Numbers.** Counts and sizes go to Fluent as numbers; the `:undo {N}`
  change number goes as a string, since it can be 2^64 - 1, past an f64's
  exact integers.
- **Allow-listed.** `messages::coded`'s E-code join, buffer text (`"{payload}\n"`-style joins), `:set`'s
  option listings, regex translations and cache keys, Vim syntax as typed,
  and `HeadlessVim`'s `Forwarded` error. Ids (context predicates, ex
  command names, matchit words) are `verbatim!`.
- **Carried findings.** T393, T407 and T468's mediums are in files this card
  touched only to move text (smart_select.rs, line_undo.rs and
  insert/keys.rs); their behaviour is unchanged and the findings stand for
  follow-up cards.
