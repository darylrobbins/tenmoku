---
task_uid: "01m6rmdb8jqnqf586hz0xt7qcf"
task: "T480"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test (tests/user_snippets.rs) parses_the_vscode_format: prefix and body as string or array, description, scope list, comments and trailing commas; a file named rust.json scopes to rust, a .code-snippets file uses its scope key, none means all languages"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test (tests/user_snippets.rs) bad_entries_are_dropped_with_a_diagnostic: an empty prefix, an unparsable body or a non-object entry is dropped with a message naming file and snippet, and the rest load"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test user_snippets_tests::matching_user_snippets_are_offered_and_expand: typing a user prefix in a buffer of its language offers a snippet item (detail 'User snippet'), accepting it expands with tabstops; another language sees none; a global one is offered everywhere"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test user_snippets_tests::editing_the_file_reloads_without_restart: rewriting snippets/rust.json changes the offers on the next completion, deleting it removes them"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test snippet_picker_tests::insert_snippet_lists_valid_snippets_and_inserts: the picker lists user and extension snippets for the active language only; Enter inserts the body at the caret via the snippet session"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test snippet_picker_tests::configure_creates_and_opens_the_language_file: choosing a language with no file creates snippets/\u003clang\u003e.json with a commented example and opens it in a tab; an existing file is not overwritten"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# User snippet files per language in the VS Code JSON format with hot reload, an Insert Snippet picker and Configure User Snippets (Mac T1403)

Lent from Mac T1403 (lend y42, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

## Problem

The snippet engine (`crates/fe-editor/src/snippet.rs`, T856) and extension-contributed snippets (`crates/fe-workbench/src/extension_snippets.rs`, spec 0e30kfp87r) exist, but a user cannot write their own. VS Code has `<config>/snippets/<language>.json` (VS Code JSON snippet format), `Snippets: Insert Snippet` and `Snippets: Configure User Snippets`. Verified missing: no user snippet source in fe-config or fe-workbench.

Linux-OK: headless parse and completion/picker tests; no screenshot.

## Change

- `crates/fe-config/src/user_snippets.rs` (new, GPUI-free): parse a VS Code snippet file: top-level object of `{ "Name": { "prefix": str|[str], "body": str|[str], "description": str, "scope": "rust,python" } }`. Files: `<config_dir>/snippets/<language-id>.json` (scope = that language) and `<config_dir>/snippets/global.code-snippets` / `*.code-snippets` (scope key decides, none = all languages). JSONC comments and trailing commas allowed (reuse the repo's JSONC reader if fe-config has one, else the one in `fe-syntax`/settings; do not add a new JSON5 crate). A bad entry is dropped with a diagnostic naming file and snippet; the rest load. Bodies are validated with `fe_editor::snippet` parse (the same check as `snippet_body_error` in `extension_snippets.rs`: reuse it, do not copy it).
- Hot reload: the config watcher that reloads settings/keymap/theme also reloads the snippets directory (add the directory to its watched set); changed files take effect without restart, a deleted file removes its snippets.
- `crates/fe-workbench/src/user_snippets.rs` (new): holds the loaded set per window; the completion provider (`lsp.rs`, next to the extension-snippets hook) offers user snippets exactly as extension snippets are offered, sorted before extension ones of equal match, detail "User snippet".
- `crates/fe-workbench/src/snippet_picker.rs` (new): `workbench.insert_snippet` (Insert Snippet) lists user and extension snippets valid for the active editor's language (name, prefix, description) and inserts the chosen one at the carets through the snippet session (multi-caret as the engine allows). `workbench.configure_snippets` (Configure User Snippets) lists languages (plus "New Global Snippets file"), creating `snippets/<lang>.json` with a commented example if missing, then opens it in a tab.
- Vim: no default map. The LazyVim preset is untouched here.

## Out of scope (later cards)

Emmet abbreviation expansion (HTML/CSS/JSX); snippet variables beyond what `snippet.rs` supports (`TM_FILENAME`, `CURRENT_YEAR` etc: if the engine lacks them, say so in the notes and leave them literal, do not widen the engine); snippets in project `.tenmoku/snippets`; cloud sync.

## Conflicts

`crates/fe-workbench/src/lsp.rs` (completion provider) is shared with the format-on-type card only in different regions; `lib.rs` and `workbench_commands.rs` mod/command lists are shared with other cards. Same area as T287's extension snippets, which have landed.


## Helium notes
- New bindings or commands have broken count tests before (fe-gpui binding counts / DEDICATED, the git_keys chord, crates/fastedit/src/vim_remaps.rs for lazyvim.vimrc). Run the affected crates' tests early and respec any count-pinning test file into scope before the first finish (`rgt task respec --scope`, which replaces the list).

## Implementation notes

- **Parsing** is `fe_config::user_snippets` (GPUI-free). The repo had no JSONC reader, so it blanks `//` and `/* */` comments and trailing commas in place (no new crate) and hands the text to `serde_json`. Lines stay where they were, so diagnostics carry the line of the snippet's key. A body is checked with the host's `body_error` callback. fe-config can't depend on `fe-workbench`, which owns `snippet_body_error`, so fe-workbench passes that function in rather than copying it. A snippet with no or empty prefix, no body, a body that isn't a string or list of strings, a bad body, a non-string description or scope, or one that isn't an object, is dropped with a warning keyed by its name (`file:line:col: warning: Name: snippet "Name" is dropped: …`). A file that isn't a JSON object is an error and loads nothing. A `<lang>.json` file ignores entries' `scope`, as VS Code does. Language names are fastedit's (`rust`, `go`, …), compared case-insensitively.
- **Hot reload** checks the files when they're used, not with a watcher: `DirStamp` (len, mtime, and on Unix dev/ino/ctime of each snippet file) is taken on each completion request and each Insert Snippet. The directory is read again only when the stamp changed, so a rewrite, a new file or a deletion takes effect at the next completion with no restart. The config service's extra watch set (`kiln::platform::config::watch_files`) only takes files: each file's parent directory is watched non-recursively and events are filtered by name, so a new file in `snippets/` would go unseen. A recursive tree watch needs a `ConfigSource` registered in `crates/fastedit/src/main.rs`, which is outside this card's scope. Checking costs one `read_dir` plus a stat per snippet file. The loaded set is app-wide (one config directory), not per window.
- **Completions** offer user snippets through the extension-snippet path (`extension_snippets::offers`), one item per prefix, with detail "User snippet" and the description as the label detail. A user item's sort text starts with NUL, so it sorts before an extension's of equal match: the menu breaks ties on sort text.
- **Insert Snippet** inserts at the primary selection only. `EditorModel::insert_snippet` expands one snippet per session, so other carets are left as they were ("multi-caret as the engine allows"). With vim on, the placeholder is selected but the vim mode isn't changed: fe-gpui has no public host API to start insert mode, and fe-gpui is outside scope. A follow-up card should add one.
- **Variables**: `snippet.rs` resolves none. A variable with a default inserts the default; one without (`$TM_FILENAME`, `$CURRENT_YEAR`) becomes a placeholder holding its name, as VS Code does with an unknown variable. The engine is not widened here.
- **Configure User Snippets** lists "New Global Snippets file" (`snippets/global.code-snippets`; there's no name prompt), then languages with an existing file, then the rest, A–Z. It creates the file with `create_new`, so an existing file is never overwritten, even in a race.
- **Payload bound** (review rounds 2–3): offers are built once per read or set, one per prefix, all sharing a single `Rc<SnippetText>` (body and description). Nothing is expanded at load. A completion request filters by language and word before copying a body into an item, and copies at most `SNIPPET_PAYLOAD_LIMIT` (4 MiB) of bodies and descriptions; the user's matches get the budget first. When matches are left out, the list is marked incomplete, so typing asks again and a left-out prefix can still come up and be accepted. Documentation, the engine's expansion, is rendered only for offered items, one at a time, up to `SNIPPET_DOCUMENTATION_LIMIT` (1 MiB) in all. The first item past that limit, and every item after it, gets none, so no further bodies are expanded. The engine already caps one expansion at `MAX_MIRROR_BYTES`. Insert Snippet builds its rows from one `UserSnippetSet` (snippets, their texts and offers from the same read), so a name is never paired with another snippet's body. Regression tests: `user_snippets_tests::a_large_snippet_with_many_prefixes_is_offered_within_a_bound`, `compact_snippets_that_expand_far_are_expanded_only_when_offered` and `each_read_is_one_consistent_set`.
