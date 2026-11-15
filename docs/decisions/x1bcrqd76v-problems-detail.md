# T1587: Problems detail pane

Spec: `docs/specs/x1bcrqd76v-problems-detail-pane-code-and-source-a.md`;
design: `docs/design/design-system.md#problems` ("The selected problem opens
a detail pane: code and source, a three-line snippet with the suggested fix
as a green line, then Apply fix ⌥↵, Ask an agent and Explain"). Ask an agent
and Explain are a later card's.

## What changed

- **`Problem.code`** (`crates/fe-editor/src/diagnostics.rs`): the
  diagnostic's code, a number as its decimal digits, from
  `EditorModel::problems`. fe-lsp already decoded `code` for open buffers.
  Its `FileDiagnostic` (a file no editor shows) now also keeps
  `code_description`, and `decode_file_diagnostics` carries it, so the
  panel's rows for such files have `code` and `code_href` too.
- **`crates/fe-workbench/src/problems_detail.rs`** (new, a child module of
  `problems_panel.rs`, so it reaches the panel's selection). For the selected
  problem, or the problem of a selected related row:
  - `code · source` in code small. The code is in `text.accent`, underlined,
    and opens `code_href` on click when the server gave one; else it's muted.
    The source is faint.
  - The whole message, wrapped. Its row in the list shows only the first line.
  - A snippet: the problem's row and one row either side, in the code family
    at code small (12/18, so the pane fits the bottom dock), with faint
    one-based row numbers, on the editor's ground (`surface.background`) in a
    bordered box. The leading blanks every shown row (fix lines included)
    shares are left out, since the pane is narrow. The problem's
    range is underlined, wavy, in its severity's colour. The text comes from
    the open buffer when an editor shows the file, else from the file on
    disk, read off the main thread with `problems_panel::read_text` (gone,
    binary or too large shows no snippet). A task's problem has no range, so
    nothing is underlined.
  - The suggested fix, when the problem is one of an open buffer's
    diagnostics and its server answers. As the problem is selected, the
    server is asked (`CodeActioner::ask`) for the `quickfix` actions over
    the problem's range with that diagnostic, automatically (`triggerKind`
    2). The enabled quick fixes are taken preferred (`isPreferred`) first,
    then those fixing a diagnostic, in fe-lsp's order. The first is resolved.
    If its edit changes the buffer, at the version asked about, the rows it
    changes show as they'd read once applied. Those are the rows of the
    edits on the problem's row, else of the first edit, with any edit
    sharing them. Each is a `status.success_soft` (celadon-soft) line with a
    `Plus` glyph in `status.success`. The fix's title follows on the Apply
    fix row, muted. The first capture had it at the green line's right,
    where it squeezed the code to an ellipsis. A fix with no edit (a bare command) or editing
    only other files shows nothing. That leaves the "fixes needing a command
    or a choice" to the card the spec defers them to.
  - **Apply fix** (`problems.apply_fix`), a small primary button with its key
    printed beside it (⌥↵ on macOS, from the keymap, so a rebinding shows),
    then the fix's title. It shows only with a fix.
  - The pane's parts keep their height, and the pane scrolls if the panel is
    shorter than they are. The first capture squashed the snippet's rows
    into each other.
- **Applying.** `problems.apply_fix` is a view command of the panel
  (`#[kiln::command(context = "problems_panel")] impl ProblemsPanel`,
  attached in its render), so it adds no `ProblemsOp` variant to
  `workbench_commands.rs`' exhaustive match. Bound to `alt-enter` in the
  `problems_panel` context in `fastedit.toml`. It applies the fix only if
  it's still for the selected problem and the buffer is at the version it
  was asked about. It goes through `Workbench::apply_code_action`
  (`lsp_code_actions.rs`, new), which runs the picker's existing
  `apply_alone` path: resolve, edits (previewed in AI Changes if they reach
  past the file), then the action's command. Without a fix the command does
  nothing. `lsp_code_actions.rs` also gained `sent_diagnostic`, the
  diagnostic-to-request conversion the picker already did inline, so both
  send the same fields.
- **Following.** The pane works out what it shows as the panel draws: the
  selected problem, its file's editor, and that buffer's version. When any
  of those changes, the fix is dropped and asked for again. An answer for
  what's no longer shown is dropped. The panel isn't drawn while hidden, so
  nothing is asked for then.
- **Layout.** The spec says "below the list". Since T1548, Problems lives in
  the bottom dock, a few rows tall and the window's width. There, a pane
  under the list left the list two or three rows, and two existing tests
  (`problems_panel_tests::open`,
  `task_problems_panel_tests::clicking_a_task_problem_opens_the_location`)
  clicked rows the pane then covered. So the pane goes **beside** the list,
  45% of the width, while the panel is drawn 600 px wide or more
  (`BESIDE_WIDTH`, measured by a canvas as `runs_panel.rs` measures for a
  test run's detail). In a narrower panel it goes under the list, at most
  half the panel's height.
- **Carried T1547 F1** (`problems_panel.rs`): the panel observes
  `ActiveLocale` and remakes both status items (`show_problem_counts`), so
  their tooltips follow a locale switch with the counts unchanged.
  `tests/problems_b3.rs::the_counts_are_plural_messages` checks both items'
  tooltips right after switching each way. `ProblemsPanel::new` now takes
  its `Context` for the subscription; its one caller already passed one.
- **Carried T1561 F3** (`lsp_code_actions.rs:192`): already fixed on main by
  T1728 (`kind_key` and the `workbench-lsp-code-actions-none-of-kind`
  selector); nothing left to do.
- **i18n.** New keys in `l10n/en-US/problems.ftl`:
  `workbench-problems-detail-apply-fix` and the command title
  `command-problems-apply_fix` (matching the method's doc title, "Apply
  Fix"). `l10n-allow.toml` already lints every file (`all = true`). It gains
  one entry: the button's element id, which R2 reads as `Button::new`'s text.
  The code, source, message and code lines are data, not catalogue text.

## Scope

Respecced once to add the files the change forces:
`task_run.rs`, `diagnostics_picker_tests.rs` and fe-editor's
`diagnostics_nav_tests.rs` (`Problem` struct literals);
`src/tests/mod.rs` (the `tests::problems_detail` module);
`examples/screenshots/audit_scenes.rs` (the scene's `mod` and `SURFACES`
lines); `examples/screenshots/audit/problems.rs` (in case the existing scene's
probes moved: they didn't need a change); `l10n-allow.toml`; and
`src/tests/problems_b3.rs` (the carried finding's test).

## Tests

- C1, `tests::problems_detail`:
  - `shows_code_source_message_and_a_three_line_snippet`: no server. Shows
    the code (linked), the source, the whole two-line message and the three
    rows with `beta` marked, inside the panel. Then a task's problem in a
    file no editor shows, read from disk, with nothing marked.
  - `the_preferred_quickfix_shows_as_a_celadon_line_with_a_plus_glyph`:
    the fake server offers a refactor, a non-preferred import quick fix and
    the preferred rename. Checks the request (`only: ["quickfix"]`,
    automatic, the diagnostic, its range), and that the line reads
    `    fixed();` on `status.success_soft` with the `Plus` glyph and the
    rename's title, with Apply fix below it. Nothing runs from showing it.
  - `apply_fix_applies_the_fixs_edit`: ⌥↵ in the panel applies the rename,
    then the server runs the action's `fix.after` command.
  - `no_fix_means_no_button_and_alt_enter_does_nothing`: a refactor and a
    disabled quick fix. No fix line, no button, and ⌥↵ changes nothing,
    runs nothing and prompts nothing; the panel keeps focus and selection.
- C2: `fe-editor diagnostics::nav_tests::problems_carry_the_code_and_its_page`,
  `fe-lsp diagnostics::code_tests::file_diagnostics_keep_the_code_and_its_page`
  (with the existing fe-lsp diagnostics tests), and
  `fe-keymap --test problems_detail_keys` (macOS and Linux, vim on and off;
  Enter still opens; not the panel's in an editor).
- C3: `fe-workbench --test l10n` (`ui_text_is_in_the_catalogue`,
  `keys_used_here_exist`).

## Audit scene (C4)

`examples/screenshots/audit/problems_detail.rs` puts an E0308 from rustc,
with its page, on `line.clone()` in the demo's `src/main.rs`
(`set_diagnostics_for_audit`), then runs `problems.show` and
`problems.select_first`. The audit runs no language server, so it shows a
fix the way a server's preferred quick fix would show
(`Workbench::show_problem_fix_for_audit`, `#[doc(hidden)]` like
`set_diagnostics_for_audit`; applying it does nothing). The fix borrows `line`,
titled "Consider borrowing here: `&line`". Its six probes are the pane's
ground (`surface.panel`), the snippet's (`surface.background`), the fix line
(`status.success_soft`), Apply fix (`accent.base`), the list beside the pane
(`surface.panel`) and the editor.

Recorded on the Mac, 2026-11-15, with
`cargo run -p fe-workbench --example screenshots -- audit target/ui-audit`:
it wrote `problems_detail_dark.png` (Yohen) and `problems_detail_light.png`
(Hakuji) and printed `ok problems_detail_dark probes (6)` and
`ok problems_detail_light probes (6)`.

Both captures show the bottom dock's Problems panel, with `src/main.rs` and
its one error selected in the list. The pane is at the right:
- "E0308" underlined in the accent, then "· rustc".
- The message "mismatched types: expected `&str`, found `String`".
- Rows 8–10 dedented by four, with `line.clone()` wavy-underlined in the
  error colour.
- The green line "+ `*counts.entry(&line).or_defaul…`".
- Apply fix (rust), "⌥↩", and the title.

The run's other failures are in scenes T1576's decision file records as
already failing on main (among them chrome, settings, shortcuts_editor,
changes, git_log, problems_panel, problems and terminal_session). None is in
this card's pane. `problems` (T1547's scene, which selects a problem, so
it now shows this pane too) fails the same three probes as on main. Those
are at sidebar coordinates from before T1548 moved Problems to the bottom
dock. `audit/problems.rs` was left unchanged.
