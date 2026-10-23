---
task_uid: "01m5jyr6vxshf631xssq4rmcwh"
task: "T288"
rev: 5
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "extension_workspace::a_workspace_manifest_keeps_only_language_kinds: with only_kinds set, a manifest contributing themes, keymaps, menus and lsp loads with warnings for the first three and its lsp kept (kiln/crates/kiln-core/tests/extension_workspace.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "extension_workspace::an_untrusted_root_is_skipped_and_contributes_nothing: \u003croot\u003e/.fastedit/extensions with an extension, root untrusted: kiln::extensions::skipped lists it with the reason and on_extensions never sees it (kiln/crates/kiln/tests/extension_workspace.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "extension_workspace::trusting_loads_and_withdrawing_trust_drops_it: deciding trust loads the extension's contributions, revoking trust or changing the root withdraws them, an edit to its manifest while trusted reloads it"
    required: true
    manual: false
  - id: "C4"
    text: "extension_workspace::the_users_copy_of_an_id_wins: the same id in the user dir and the workspace loads the user's, with a warning naming the workspace one"
    required: true
    manual: false
  - id: "C5"
    text: "extension_workspace::a_trusted_workspace_server_needs_its_own_consent: a trusted folder's [contributes.lsp.*] is pending in pending_extension_consents until granted, keyed by the workspace extension's directory, and a user grant of a same-named server doesn't cover it (crates/fastedit/tests/extension_workspace.rs)"
    required: true
    manual: false
  - id: "C6"
    text: "extensions_picker_workspace_tests::the_list_shows_source_and_skipped: a workspace extension's row has source Workspace; an untrusted root's extension has state SkippedUntrusted"
    required: true
    manual: false
  - id: "C7"
    text: "scripts/check_kiln_boundary.py, cargo nextest run -p kiln-core -p kiln -p fe-workbench -p fastedit and cargo clippy --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Load a trusted folder's .fastedit/extensions for grammars, language servers and snippets only (extensions record 3) (Mac T1041)

## Problem

Extensions record §3 (user decision Q3, 2026-10-17): workspace extensions are allowed once the folder
is trusted, for grammars, LSP configs and snippets only. Stage 1 reads only the user's extensions
directory (`AppPaths::extensions_dir`). `AppPaths::project_extensions_dir(root)` already names
`<root>/.fastedit/extensions` (T840) but nothing reads it.

## Change

Record §3: `<root>/.fastedit/extensions/<dir>/extension.toml`, the launch root only (as the project
settings file is: see `launch_root` in `crates/fastedit/src/main.rs:105`, `follow_settings`), only
while the root is trusted (`kiln_core::trust::TrustStore`; `Workbench::root_trusted` and kiln-shell's
`TrustChanged`).

- `kiln-core/src/extension.rs`: `ExtensionApi` gains `only_kinds: Option<Vec<String>>`. With it set,
  `load_manifest` drops `themes`, `keymaps`, `menus` and any app kind not in the list, each with a
  warning ("workspace extensions can't contribute themes"), and loads the rest.
- kiln (`extensions.rs`, `setup.rs`, `config.rs`): a second, workspace extension set beside the user
  one, with its own root set at runtime and rescanned (watched, as the user's is) while trusted and
  dropped when trust is withdrawn or the root changes; a root that isn't trusted is listed in
  `kiln::extensions::skipped(cx)` with the reason and contributes nothing. An id also installed for
  the user loads the user's copy and the workspace's is dropped with a warning. The app's
  `on_extensions` callback gets the user's and the workspace's manifests, workspace after user,
  each manifest carrying its source (`Manifest` is unchanged; the callback receives the source beside
  it).
- fastedit (`crates/fastedit/src/config.rs`, new `crates/fastedit/src/workspace_extensions.rs`,
  `main.rs`): sets the workspace root (the launch root) and passes `grammars`, `lsp` and `snippets`
  as the allowed kinds. Language servers from a workspace extension go through the same consent
  gate as the user's (keyed by the workspace extension's directory, so they need their own consent).
- `crates/fe-workbench/src/extensions_picker.rs`: rows for workspace extensions show source
  `Workspace`, and an untrusted root's skipped extensions show state `SkippedUntrusted`.

## Out of scope

Workspace themes, keymaps or menus; extensions in a non-launch root or a nested folder; installing
from an archive; a project-level `[extensions]` setting.

Lend note (helium lead): spec, scope and criteria replaced with the Mac's full JSON (y51); earlier copies were truncated.

## Implementation notes

- Scope (revision 4) adds `kiln-core/tests/extension_manifest.rs`, `kiln-core/tests/extension_discovery.rs`
  and `kiln-plugin/tests/manifest.rs`: each builds an `ExtensionApi` literal, which the new
  `only_kinds` field breaks; each gains `only_kinds: None` and nothing else.
- `only_kinds` applies to every kind alike: a file kind (`themes`, `keymaps`, `menus`) or app kind
  not in the list is dropped, unchecked, with a warning at its key ("workspace extensions can't
  contribute themes, so `contributes.themes` is ignored"). fastedit's list is `grammars`, `lsp`,
  `snippets`, so themes, keymaps and menus are always dropped.
- `on_extensions` keeps its `&ExtensionSet` argument, so the callers outside the scope
  (`kiln/tests/extensions_reload.rs`, `extensions_fingerprint.rs`) are unchanged. The set kiln passes
  is `ExtensionSet::joined`: the user's extensions, then the workspace's, each with its source
  (`ExtensionSet::sourced`, `kiln_core::extension::ExtensionSource`); `Manifest` is unchanged.
  `joined` is where the user's copy of an ID wins: the workspace's is dropped with a warning at its
  `id` naming both directories.
- kiln follows the root's trust itself, through `kiln_shell::trust::decided` and `observe` on the
  shared decisions, so the app only sets the root (`kiln::extensions::set_workspace_root`). The
  directory is `<root>/.<app>/extensions`, `<app>` being the `[engine]` key, as
  `AppPaths::project_extensions_dir` has it.
- The config service watches a fixed set of trees, so a trusted workspace directory gets a
  `TreeWatcher` of its own, dropped with the set. When the app gives the batches (`watch_with`, in
  tests), they name the workspace directory instead, as they name the user's.
- Workspace extensions give no licence notices: the notice says the user installed it.
- `kiln::extensions::loaded` keeps its signature (user's then workspace's); `loaded_from` has the
  sources, `skipped` the untrusted root's extensions with the reason ("<root> isn't trusted").
- fastedit's managers run only the first server of a name (existing behaviour), so a granted
  workspace `zls` beside a user's `zls` doesn't run; the consent is still its own, keyed by its
  directory (C5's test checks the pending entry and the grant).
- Review round 1, F1: fe-syntax only ever added grammars, so a withdrawn workspace's stayed
  registered. Scope (revision 5) adds `crates/fe-syntax/src/runtime.rs` and `lib.rs` for
  `fe_syntax::unregister_language`, which waits for a registration under way. fastedit's grammar
  batches now always run, and each first withdraws the languages extensions registered that no
  grammar names now; batches run one at a time, so a stopped batch's registrations are withdrawn by
  the newer one. A `Syntax` already made keeps its language. Test:
  `extension_workspace::a_workspace_grammar_goes_with_trust` (the JSON wasm fixture), covering
  revocation and a root change.
- F2: a picker row's identity (`match_id`) is its extension's directory, so a skipped workspace row
  with a user extension's ID is its own row. Test:
  `extensions_picker_workspace_tests::a_skipped_row_with_a_users_id_is_its_own_row` clicks both.
- Review round 2, F1: registrations were kept by language name alone, so a refused grammar of the
  same name (a user's broken one, or another root's) kept a withdrawn workspace's registered. Each
  registration now records its extension's directory, and a batch keeps it only if that directory
  still lists a grammar of the name; a refused one keeps only its own extension's last good. Tests:
  `a_users_broken_grammar_of_the_name_doesnt_keep_the_workspaces`,
  `another_roots_broken_grammar_of_the_name_doesnt_keep_the_first`,
  `a_broken_edit_keeps_its_own_last_good_grammar` (crates/fastedit/tests/extension_workspace.rs).
  `withdrawing_trust_keeps_the_users_grammar_of_the_name`: a user's and a workspace's grammar of one
  language, both good; trusted, the workspace's registers after the user's; withdrawn, the
  workspace's goes and the user's stays.
