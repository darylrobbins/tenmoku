---
task_uid: "01m55ef81zptj46f8qvq1wrarc"
task: "T174"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test tests::lsp_nav::goto_type_definition_and_implementation_jump_to_the_servers_answer passes: nav.goto_type_definition with one Location answered moves the active editor to it, nav.goto_implementation with two Locations answered lists both in the definitions picker, each request sent once at the caret; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test tests::lsp_nav::goto_declaration_without_a_server_says_none_found passes: with no server the status reads 'No declaration found for `\u003cword\u003e`' and no tab opens; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-keymap integration test goto_implementation_is_cmd_f12_on_macos_and_ctrl_f12_elsewhere (crates/fe-keymap/tests/nav_related_bindings.rs, top level) passes, and the Go menu lists Go to Declaration, Go to Type Definition and Go to Implementations right after Go to Definition; the three IDs are rows of typed_command_ids_tests.rs's table"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench -p fe-keymap -p fe-gpui passes with no existing test weakened: the only edits to existing tests are added rows or bumped counts for the one new default binding (typed_command_ids_tests.rs's table gains the three IDs; menu_go_nav.rs::go_menu_lists_navigation_commands lists the three new IDs after nav.goto_definition; the default keymap binding count asserted in fe-gpui command_tests.rs table_is_the_only_binding_source and keymap_tests.rs default_keymap_matches_the_old_table goes 260 -\u003e 261, with the day-one comment count in command_tests.rs); cargo clippy -p fe-workbench -p fe-keymap -p fe-gpui --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Go to Declaration, Go to Type Definition and Go to Implementations (cmd-F12 / ctrl-F12) in the Go menu and palette, jumping or listing as Go to Definition does (Mac T852)

## Problem

With fe-nav's `Navigation::related` in place (the card this depends on), the workbench still has no way to ask for
it. VS Code's Go menu has, after Go to Definition: Go to Declaration, Go to Type Definition and Go to
Implementations (cmd-F12 on macOS, ctrl-F12 elsewhere); fastedit's Go menu (`crates/fe-keymap/menus/fastedit.toml`
~line 340) and `NavOp` (`crates/fe-workbench/src/nav.rs:785`) have only Go to Definition. Gap audit 2026-10-17
(user-approved).

## Change

1. `crates/fe-workbench/src/nav.rs`: `NavOp::Related(fe_nav::Related)`; `run_nav_op` runs it as
   `goto_definition` (`nav.rs:678`) runs, with `navigation.related(kind, &request, cancel)` in place of
   `definitions`, landing in `definitions_landed` so an unambiguous answer jumps (recording the place for
   `nav.back`) and several are listed in the definitions picker. Its messages are `definitions_landed`'s
   (`nav.rs:715-737`) with the kind's noun for "definition": `No implementation found for `foo``, `Go to type
   definition failed: <error>`, `No declaration found for `foo``. Every provider answering `Unsupported` (search
   always does) is "none found", not a failure.
2. `crates/fe-workbench/src/workbench_commands.rs`, in the one `#[kiln::command(context = "workbench")]` block
   next to `go_to_definition`: `nav.goto_declaration` "Go to Declaration", `nav.goto_type_definition` "Go to Type
   Definition", `nav.goto_implementation` "Go to Implementations", each calling `self.nav_command(NavOp::Related(..))`.
   Add their rows to `crates/fe-workbench/src/typed_command_ids_tests.rs`'s table.
3. `crates/fe-keymap/keymaps/fastedit.toml`: `nav.goto_implementation` on `{ macos = "cmd-f12", default =
   "ctrl-f12" }`, context `"workbench && !terminal"` as f12's binding has. The other two get no key (VS Code binds
   none). `crates/fe-keymap/menus/fastedit.toml`: Go menu items "Go to Declaration", "Go to Type Definition", "Go to
   Implementations" right after "Go to Definition".
4. Tests:
   - In the existing `crates/fe-workbench/src/tests/lsp_nav.rs`, with its `Lsp` harness and helpers:
     `goto_type_definition_and_implementation_jump_to_the_servers_answer` (the fake server provides
     typeDefinition and implementation; `nav.goto_type_definition` with one Location answered jumps the active
     editor to it; `nav.goto_implementation` answered two Locations lists both in the definitions picker; each sent
     its request once, at the caret) and `goto_declaration_without_a_server_says_none_found` (the root untrusted, so no
     server: the definitions status reads `No declaration found for `<word>``, no tab opened).
   - New integration test `crates/fe-keymap/tests/nav_related_bindings.rs`, top-level fn
     `goto_implementation_is_cmd_f12_on_macos_and_ctrl_f12_elsewhere`, using `crates/fe-keymap/tests/lsp_bindings.rs`'s
     way of resolving keys (copy its helpers): cmd-f12 resolves to `nav.goto_implementation` in the editor stack on
     `Os::MacOs`, ctrl-f12 on `Os::Linux`, and neither key is bound to anything else in a vim normal-mode stack.
