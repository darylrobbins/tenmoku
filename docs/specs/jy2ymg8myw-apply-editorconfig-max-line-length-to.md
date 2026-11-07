---
task_uid: "01m6sf8gd6z02kmwjy2ymg8myw"
task: "T1424"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "max_line_length_sets_the_wrap_column_in_column_mode passes: column mode with wrap_column 0 and max_line_length = 60 wraps at 60 columns, and viewport mode is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "max_line_length_off_or_unset_keeps_the_setting passes: off or no property leaves editor.wrap_column in force"
    required: true
    manual: false
  - id: "C3"
    text: "editing_the_editorconfig_updates_and_clears_the_wrap_column passes: a changed or removed value is followed after the reload"
    required: true
    manual: false
  - id: "C4"
    text: "element::rulers::max_line_length_ruler_tests::max_line_length_draws_a_ruler passes: the painted rulers include column N beside editor.rulers"
    required: true
    manual: false
---

# Apply .editorconfig max_line_length to the wrap column and draw a ruler at it

Wire .editorconfig `max_line_length` to the file's wrap column and a ruler (follow-up to Mac T1419 / 5834ccdc, which parses it, and Mac T1402 / b5106c9d, which added editor.wrap_column and editor.wrap_mode).

State today: `fe_config::EditorConfig::max_line_length: Option<MaxLineLength>` (`Columns(n)` | `Off`, crates/fe-config/src/editorconfig.rs ~44, ~62) is parsed and never read. The workbench applies a file's EditorConfig to its editor in `Configs::start` (crates/fe-workbench/src/pane.rs ~2150-2170: `apply_config`, `set_save_fixes`), re-run when the file or any .editorconfig changes. Soft wrap width comes from `EditorView::wrap_limit` (crates/fe-gpui/src/view.rs ~823) using the view's `wrap_column`/`wrap_mode`, which `apply_settings`/`set_wrap_settings` overwrite from settings. Rulers come from `view.theme.rulers` in crates/fe-gpui/src/element/rulers.rs `layout`.

Semantics (as VS Code's EditorConfig extension and Zed treat it: the per-file line length limit):
1. `max_line_length = N` is the file's wrap column, clamped to 20..=500 as `effective_wrap_column` does, replacing `editor.wrap_column` for that editor. It takes effect only where `editor.wrap_mode` is `column` or `bounded` and `editor.soft_wrap` is on; in `viewport` mode (the default) wrapping stays at the window edge, because the user chose that. So `bounded`/`column` with `wrap_column = 0` (none) and a file with `max_line_length = 80` wraps at 80.
2. `max_line_length = N` also draws one extra ruler at column N for that file, added to `editor.rulers` (deduped), in any wrap mode. N is not clamped for the ruler (only N >= 1).
3. `max_line_length = off`, or the property unset, adds no override: the editor's own `editor.wrap_column` and `editor.rulers` apply. (`off` does not turn the user's setting off; EditorConfig `off` just means this file declares no limit, e.g. to cancel a parent section's value.)
4. The override survives settings changes (`apply_settings` replaces the setting-derived value, not the override), is copied to a split of the editor, and is dropped when the file's .editorconfig stops giving a value (property removed, file deleted, Save As to another path) because the apply in `Configs::start` runs on every config change, including a change to `None`.

Implementation (keep it small):
- crates/fe-gpui/src/view.rs: a field `max_line_length: Option<usize>` on `EditorView` (default None) and `pub fn set_max_line_length(&mut self, columns: Option<usize>, cx)` (notify only on change). `wrap_limit` uses `self.max_line_length.map(clamp 20..=500).or(self.wrap_column)` for the `Column` and `Bounded` arms.
- crates/fe-gpui/src/element/rulers.rs: include `view.max_line_length` among the columns, after the `> 0` filter.
- crates/fe-gpui/src/pane_item.rs `split`: copy the new field next to `wrap_column`/`wrap_mode`.
- crates/fe-workbench/src/pane.rs: in the `editor.update` of `Configs::start`, after `apply_config`, call `editor.set_max_line_length(match config.max_line_length { Some(MaxLineLength::Columns(n)) => Some(n), _ => None }, cx)`. Do this unconditionally (so a removed value clears it). Not inside `apply_config`, which takes a model.
- No settings, docs or schema change; mention the behaviour in the doc comment of `set_max_line_length`.

Tests: a NEW integration test file crates/fe-workbench/tests/editorconfig_max_line_length.rs (host: the fe-workbench integration style of tests/hot_exit_rootless.rs, with `TENMOKU_CONFIG_DIR`, `XDG_CONFIG_HOME` and `HOME` cleared under a serial lock; a temp project containing `.editorconfig` and a file with one 200-column line, opened via `Launch { root, file }`, then a window resize so the viewport is wider than 120 columns and a draw). Read the result from `editor.model().display().wrap_width()` (as crates/fe-gpui/src/wrap_column_tests.rs does) and rulers by `cx.debug_bounds` or the existing ruler hook, whichever rulers_tests.rs uses; if neither is reachable from an integration test, assert the ruler in a lib test in a NEW file crates/fe-gpui/src/element/max_line_length_ruler_tests.rs declared from crates/fe-gpui/src/element/ (name its id in the criteria with its module path).
- `max_line_length_sets_the_wrap_column_in_column_mode`: wrap_mode column, wrap_column 0, `[*] max_line_length = 60` -> wrap width is 60 em and the line takes 4 rows; in the same binary with wrap_mode viewport -> 1 row (unchanged).
- `max_line_length_off_or_unset_keeps_the_setting`: wrap_mode column, wrap_column 40, `max_line_length = off` -> 40 columns; same with the property absent.
- `editing_the_editorconfig_updates_and_clears_the_wrap_column`: rewrite .editorconfig to 80, then to no property: the wrap width follows after the reload.
- `max_line_length_draws_a_ruler`: ruler at column N alongside `editor.rulers`.
Tests must fail at base (no override exists) and use only APIs present at base.

Scope: the four source files above, plus the new test file(s). Do not touch crates/*/src/lib.rs.

Lead note (rev 2, after codex round 1 F1): assert max_line_length_draws_a_ruler in a lib test, NEW file crates/fe-gpui/src/element/max_line_length_ruler_tests.rs, declared with #[cfg(test)] #[path] from rulers.rs as mod max_line_length_ruler_tests, reading rulers::PAINTED (as rulers_tests.rs does) so it observes the painted rulers. The other tests stay in crates/fe-workbench/tests/editorconfig_max_line_length.rs. Drop the EditorView::ruler_columns() accessor if nothing else needs it.
