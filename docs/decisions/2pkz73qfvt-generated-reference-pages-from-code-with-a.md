# Generated reference pages (T716, help 2)

The user guide's three reference pages, `docs/user/src/reference/settings.md`,
`keyboard-shortcuts.md` and `commands.md`, are generated from the code they
describe by `crates/fastedit/examples/gen_user_docs.rs`, and a test fails when
one is stale.

```zsh
cargo run -p fastedit --example gen_user_docs              # rewrites the stale pages
cargo run -p fastedit --example gen_user_docs -- --check   # writes nothing; exits 1 if one is stale
```

## Consequence for other cards

`crates/fastedit/tests/user_docs_fresh.rs` (`reference_pages_are_up_to_date`)
runs the check in `cargo test`. So **a card that adds, removes or changes a
setting (or its doc comment, type or default), a built-in binding, or a
command or its title must regenerate the pages** with the command above, and
needs `docs/user/src/reference/**` in its scope. The failure prints the
command and the lines that changed.

The check runs inside `cargo nextest run --workspace`, which CONTRIBUTING.md's
"Tests and checks" already lists; the section now also names the regeneration
command, for when that test fails.

## Sources

- **Settings**: `fe_config::settings_schema()`, the JSON schema schemars
  derives from the settings types, whose doc comments are its descriptions. A
  struct is a table (`## editor.completion`); a map of structs is a table of
  tables (`lsp.servers.<name>`); a list of structs is a list of tables
  (`[[vim.remap]]`); anything else is a key (`### editor.tab_size`) with its
  type and bounds, its default as TOML, its enum values with their doc
  comments, and its own doc comment. An `Option` with no default value is
  "unset".
- **Keyboard shortcuts**: `fe_keymap::DEFAULT`, resolved for macOS, Linux and
  Windows by name with `KeymapLayer::parse_for`, never for the host, then
  applied as the app applies it (`Keymap::from_layers`, `bindings_for`): an
  unbind (`command = ""`) removes the keys bound before it, and a later
  binding of the same keys and context overrides an earlier one. A row is
  a command, its args and its context; its keys are printed as the platform's
  menus print them (`macos_label`, `pc_label`). Linux and Windows share a
  column while their keys are the same on every row.
- **Commands**: `fe_workbench::command_catalog()` checks IDs but can't list
  them, so the catalog is now built from parts that also list their commands.
  `fe_workbench::RECORDED_COMMAND_CRATES` names the crates whose
  `#[kiln::command]`s it checks (kiln's `recorded_catalog`), and the generator
  lists those crates' `inventory` records. `fe_workbench::registry_commands()`
  lists every registry part: picker, search panel, commit box, menu bar,
  command palette, project panel and status bar, plus fe-gpui's
  `catalog_commands()`, which covers the editor, vim, proposal, diff, image,
  hex and app registries. It destructures fe-gpui's `Catalog`, so a registry
  added there can't be left out. Commands are listed whether or not anything
  binds them, and no GPUI app is started. Titles are `command-<id>` messages
  in en-US, else the title the command was declared with.
  `every_command_in_the_catalog_has_a_row` checks that the rows are exactly
  that set, that every command a keymap, menu or palette list names is among
  them, and that some unbound registry commands have rows.
- **Defaults**: a key's default is the one the tables holding it give
  (`editor`'s default for `editor.completion.*`), else its type's own. An
  explicit null default reads "unset". A key that may be null (an `Option`,
  including an unconstrained `Option<serde_json::Value>`) with no default
  reads "unset (the schema gives no default)". A map's or list's default is
  shown whole: empty (`{}`, `[]`), null and absent each read differently, and
  a null entry stays in it as `unset`. So a change from one form to another
  changes the page.

## Determinism

The text is the same on every host: the locale is set to en-US and `app` to
`Tenmoku` before anything is titled; rows are sorted (keymap and command rows
by ID, then args and context; settings keys by name, tables depth first); key
labels follow the keymap's order within a row; lines end in LF; and nothing
names a path on disk or a time. `shortcuts_are_the_same_on_every_host` builds
the shortcuts page from fe-keymap's macOS and Linux texts (`DEFAULT_MACOS`,
`DEFAULT_LINUX`) on one host and compares it byte for byte with the page built
from `DEFAULT`, and checks each text binds the same whichever OS parses it.

## Markdown

Doc comments keep their code spans; everything else Markdown or HTML would
read as syntax is escaped, so a `<name>` or `snake_case` prints as written.
Keys are `<kbd>` with ASCII punctuation as entities, so a `|` or backtick key
can't break a table. Pages keep `scripts/check-user-docs.sh` passing: an H1,
and no relative links (reference pages need no status line).
