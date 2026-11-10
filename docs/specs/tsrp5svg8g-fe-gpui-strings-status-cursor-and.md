---
task_uid: "01m71sgd29zxdcv3tsrp5svg8g"
task: "T549"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-gpui --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with nothing pending: fe-gpui's lint covers the whole crate"
    required: true
    manual: false
  - id: "C2"
    text: "Status messages are whole messages: gpui-status-cursor and gpui-status-cursor-selections with a plural count (a fake-locale test); cargo nextest run -p fe-gpui passes"
    required: true
    manual: false
---

# fe-gpui strings: status cursor and selection messages with plurals, diff view, image and file viewers, proposals, the which-key reader on label() (i18n G1) (Mac T1532)

# fe-gpui strings: status messages, diff view, viewers, proposals, which-key reader (G1)

Spec: card **G1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for G1 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- The plan waits for P21 (popovers) and T1396. P21 is replaced by the B3 hover
  card, which now comes after this one; T1396 is abandoned.
- Scope is the files that carry text today (listed), not `src/**`:
  `element.rs` has no UI text (its literal is a font name) and is left to the
  editor cards that hold it (T1501–T1507). If the lint reports another file,
  ask for it to be added.

Platform: **Linux-OK**.

## Implementation notes

- **Scope added by respec.** The lint, switched on with `all = true`, also
  reported `vim.rs` (map and `fe:` command messages), `view/host.rs` (E21),
  `diff_fold.rs` (hidden-line counts) and `vim/which_key_layout.rs` (the
  footer); `lib.rs` and `Cargo.lock` carry the module and dependency. The
  public consts fe-workbench read became functions, so its readers moved:
  `diff_view::{no_changes, wrapped_to_first, wrapped_to_last}` (`git_diff.rs`,
  `compare_saved_tests.rs`), `pane_item::discard_buttons` (`close_tests.rs`)
  and `status::plain_text` (`status_bar.rs`, `lsp_format.rs`,
  `test_commands.rs`, `language_picker.rs`). `element.rs`'s and
  `element/decorations.rs`'s only literals are `#[cfg(test)]` paint notes,
  allow-listed without touching the files.
- **Status cursor.** `status::cursor_label(line, column, Selected)` picks one
  of `gpui-status-cursor`, `-selected` and `-selections`; the editor and the
  file viewer both call it. The file viewer now stores the numbers it works
  out off the main thread and formats at each read, so a locale change shows
  at the next frame. `tests/l10n_status.rs` reads an editor's status items in
  en-US and in a fake `ru` (one / few / many) fixture.
- **Command titles** register from `command-<id>` (`l10n::command_title`), so
  `label()` resolves them again wherever they show; `l10n.rs`'s
  `commands_and_command_keys_match` checks the 28 ids against the keys.
  Which-key's `command_title` is public as `fe_gpui::which_key::command_title`
  for Z1's cross-surface test.
- **Prompt buttons** are built as `PromptButton`s, never `&str`, so a
  translated Cancel keeps its role: GPUI's `From<&str>` only knows English
  "Cancel" and "OK". The discard prompt's Cancel is an ok button by design;
  kiln-ui's in-window prompt finds it for Escape by the English title
  "Cancel" (`Prompt::cancel_button`), so in another locale Escape there would
  need kiln-ui to compare with its own translated Cancel — a kiln card, not
  this one. en-US is unchanged.
- **Kept resolved when set**, as toasts are: the diff view's navigation
  message, the file viewer's banner and the image viewer's "can't display"
  message are set once per event and show until the next; a locale change
  shows in the next one.
- **Byte-equal en-US** keeps today's plurals where English had none:
  `1 changes`, `1 bytes`, `(1 selected)` have identical `[one]` and `*[other]`
  branches for translators to split.
