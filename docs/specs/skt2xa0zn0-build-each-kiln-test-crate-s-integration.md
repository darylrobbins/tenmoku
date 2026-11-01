---
task_uid: "01m62w0dmzebvjbyskt2xa0zn0"
task: "T1234"
rev: 2
type: "chore"
complexity: "l"
criteria:
  - id: "C1"
    text: "kiln-shell builds exactly one integration-test executable: `cargo test -p kiln-shell --no-run` (run in kiln/) prints one `tests/it.rs` Executable line and none for the old per-file names"
    required: true
    manual: false
  - id: "C2"
    text: "Every kiln-shell test that ran before still runs: the sorted list of `cargo nextest list -p kiln-shell` test names, with the file stem prefixed, equals the list taken at the base (counts identical)"
    required: true
    manual: false
  - id: "C3"
    text: "`cargo nextest run -p kiln-shell` passes (each test in its own process), and clippy -D warnings and fmt are clean inside kiln/"
    required: true
    manual: false
  - id: "C4"
    text: "Adding kiln/crates/kiln-shell/tests/zz_new.rs (not edited into any other file) makes its test appear in `cargo nextest list -p kiln-shell`; removing it removes it; the guard test fails if the generated list and the directory disagree"
    required: true
    manual: false
  - id: "C5"
    text: "No reference to a bare kiln-shell integration test name remains in .config/nextest.toml, regatta.toml or TESTING.md; TESTING.md documents the layout for kiln-shell: new tests are new files in tests/, no list to edit, ids are \u003cfile\u003e::\u003ctest\u003e"
    required: true
    manual: false
  - id: "C6"
    text: "The report states kiln/ target/debug size and deps executable count before and after, and the extra time a test-file edit now costs (kiln-shell lib rebuild). The kiln crate is out of scope (lead, 10-29): its #[kiln::command] inventory records are per binary and collide in one binary, which needs kiln/kiln-macros src changes"
    required: true
    manual: false
---

# Build kiln-shell's integration tests as one binary, generated module list, so a lane's debug target stops filling the disk

Problem. Cargo builds each tests/*.rs as its own statically linked binary. A lane's debug target is
about 37 GB, of which about 25 GB is 698 executables in target/debug/deps; every lane, gate and
regression build holds its own copy and the Mac disk fills (ENOSPC). The gpui-linked test crates
are 100-375 MB per binary: kiln-shell has 46 test files, kiln 36.

Mechanism (decided; no new dependency, so nothing for deny.toml / THIRD_PARTY.md). For each pilot
crate: Cargo.toml gets `autotests = false`, `build = "build.rs"` and
`[[test]] name = "it" path = "tests/it.rs"`. build.rs lists tests/*.rs (excluding it.rs; subdirectories
such as tests/support are NOT listed, they stay `mod support;` inside the files that use them), sorted,
and writes one `#[path = "<absolute path>"] mod <file stem>;` per file to $OUT_DIR/it_mods.rs, with
`cargo:rerun-if-changed=tests` and one rerun-if-changed per file. tests/it.rs is only
`include!(concat!(env!("OUT_DIR"), "/it_mods.rs"));`. Test files do not move, so history and
other tasks' scope globs stay valid, and adding a test is still adding one new file. The generator is
written once per workspace as kiln/scripts/it_mods.rs (include!d by each kiln build.rs, kiln/ is its own
workspace and is copied alone by TESTING.md's clean-copy check) so the crate build.rs files are 3 lines.
Pilot crates here: kiln-shell and kiln. Do not use `automod` or a proc macro: a proc macro cannot
track newly added files, so a stale build would silently skip a new test.

Things to verify and fix in these two crates (each test runs in its own process under nextest, which is
what CI uses, so process-global state is isolated; the risk is `cargo test`, which would share one process):
- Test ids become `<file_stem>::<test>` (was bare `<test>`). Update every reference: .config/nextest.toml
  filters, regatta.toml, TESTING.md, and any open task's regression_tests naming a kiln-shell or kiln
  integration test (list them with rgt and report; today none of the 4 open tasks with regression_tests
  name one).
- `mod support;` inside a file now resolves via #[path] relative to the file's directory (tests/support),
  one copy per including module: dead-code warnings may appear; fix with #![allow(dead_code)] in the
  support module, not per file. Inner attributes (#![...]) at the top of a test file must still compile as a module.
- Process-global state: kiln/tests/modules.rs (log::set_logger / OnceLock logger), module_before_init.rs
  (static counters), extensions_reload.rs (set_current_dir), macro_model.rs and picker_vim_g_prefix.rs
  (thread_local). Make them not depend on a fresh process or document in the file why they do, and add
  `cargo test -p kiln --test it` as a one-time manual check in the task report, not in CI.
- A guard so a new file can never be silently skipped: a test in each crate (tests/it_lists_every_file.rs is
  NOT allowed, it would itself be a file; put it in tests/it.rs) that compares the generated module list to
  fs::read_dir(tests) .rs files.
- Adding a file regenerates the list without touching any other file, and rebuilds the lib only if cargo
  does so anyway: show with `cargo build -p kiln-shell --tests` timings before/after (timestamps in the report).

Savings. Report `du -sh target/debug` and `ls target/debug/deps | wc -l` for kiln/ before and after
(clean build, CARGO_TARGET_DIR under the scratch dir, delete afterwards).

## Implementation notes

kiln-shell is done. Rev 2 narrowed the task to kiln-shell; why kiln can't be done in this scope is below.

- kiln-shell: `autotests = false`, `build = "build.rs"`, `[[test]] it`. build.rs is three lines
  (`include!("../../scripts/it_mods.rs")`); the generator also writes `IT_MODS`, the stems, which
  `it_lists_every_test_file` in tests/it.rs compares to `read_dir(tests)`.
- Deviation: `tests/support` is declared once, in tests/it.rs, and the 19 files that used it say
  `use crate::support;`. One copy per file fails every test at start: support's
  `kiln_platform::commands!(pad, ..)` registers GPUI actions, and GPUI panics on an action name
  registered twice in a binary (`Action with name pad::SelectAll already registered`).
- picker_vim.rs and picker_vim_g_prefix.rs: their `thread_local!` switch is per test thread, so
  `cargo test -p kiln-shell --test it` (one process) passes too: 181 passed.
- Cost: adding or editing any kiln-shell test file reruns the build script, and cargo then rebuilds
  the kiln-shell library and its unittests as well as `it` (and, in a workspace build, everything
  that depends on kiln-shell). At the base, adding tests/zz_new.rs compiled only `zz_new` (1.1 s).
  After the change, the same edit took 17.4 s.

kiln (not done), for two reasons, each needing files outside this task's scope:
1. tests/gallery.rs, host.rs and notes.rs `#[path]`-include the examples' sources, which use
   `crate::commands`, `crate::threads`, `crate::note_view` and similar: paths that assume the example
   file is the crate root. As modules of `it` they don't resolve (E0432, 9 errors). The examples
   would have to change (e.g. `super::`).
2. kiln's inventory records are per binary. `#[kiln::command]` IDs default to
   `CARGO_CRATE_NAME.<fn>`, so `new_note` in typed_tables.rs, macro_command.rs, recorded_catalog.rs
   and examples/notes/notes.rs would all become `it.new_note`, each on `secondary-n` in File. 8 files
   assert `"<stem>.…"` IDs. `kiln::app` adds every in-app record linked into the binary
   (`Contributions::add_recorded`, kiln/src/contributions.rs:878), so each test's app would get every
   other file's commands, keys, menus, panels, statuses, items, settings and models. Fixing this needs
   kiln and kiln-macros source changes, for example scoping test-binary records by module.

Savings, kiln-shell only (clean `cargo test --no-run` of the 8 kiln crates, CARGO_TARGET_DIR in a
scratch dir, deleted afterwards): `target/debug` went from 11G to 6.0G, `ls target/debug/deps | wc -l`
from 4125 to 2670, and the `Executable` lines from 128 to 83 (46 kiln-shell files became one `it`,
58 MB). C2 for kiln-shell: the base list with stems prefixed equals the new list, plus
`it_lists_every_test_file`. C6: open tasks with regression_tests are T564, T1125, T1230 and T1232,
and none of them names a kiln-shell or kiln integration test.
