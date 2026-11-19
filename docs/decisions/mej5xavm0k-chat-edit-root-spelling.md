# Agent edits under a root spelled another way (T695, Mac T1972)

## The cause

GH CI 44405849017 (8ba6f008c, windows Test) refuses `propose_edit` of
`notes.txt`, directly under the root, with "it's outside the folder", in
`chat_tool_loop::tests::edits_run_in_the_calls_order_whatever_is_answered_first`;
`a_hand_in_under_way_ends_only_its_own_turn` goes through the same flow.

Read side by side, the two paths `chat_apply::locate` compares are resolved
differently:

- `real_path(root.join(rel))` walks the path a part at a time with
  `symlink_metadata`. A part that isn't a link is kept **as written**
  (`real = real.join(name)`): an 8.3 short name (`RUNNER~1`), a part in
  another letter case, a `C:` drive letter in lower case, all stay.
- `dunce::canonicalize(root)` is `GetFinalPathNameByHandleW` on Windows (and
  `realpath` on macOS): it reports the folder as the file system names it, the
  long name in its stored case.

`real.starts_with(&real_root)` then compares, say,
`C:\Users\RUNNER~1\AppData\Local\Temp\.tmpX\notes.txt` with
`C:\Users\runneradmin\AppData\Local\Temp\.tmpX`, and fails. That is the GH
Windows runner's `TEMP` exactly: it is spelled with the short name
`RUNNER~1`, and these tests pass the raw `tempfile::tempdir()` path as the
root. The chat tests that pass on Windows (`chat_apply_tests.rs`,
`agent_tools_tests.rs`) `dunce::canonicalize(temp_dir())` first, so their
root is already in its canonical spelling and the two sides agree.

It isn't test-only: the workbench keeps its root as it was given
(`Launch::root`; only `..` parts are resolved, `workbench.rs`
`resolved_parents`), so a folder opened as `C:\Users\RUNNER~1\proj` or
`c:\users\me\proj` gets every agent edit, read and link refused. The same
compare sits in `agent_tools::resolve` (it calls `locate`) and so in
`read_file`, `list_files` and `propose_edit`.

There's no Windows box here to log `real` and `real_root` on, and a lane
can't push a branch to run GH CI, so the GH CI windows run after landing
checks C2. The evidence there is:

- **The failing job's log** (job 135160725781 of run 44405849017) prints no
  raw temp path. Every temp path in it is already canonical: a `\\?\C:\...`
  from `std::fs::canonicalize`, or `C:\Users\runneradmin\...` from tests that
  `dunce::canonicalize` their tempdir first (`fe-git` `worktree.rs`,
  `tests/debug_session.rs`, `tests/project_multi_select.rs`). It can't show
  what `TEMP` itself is.
- **Which tests fail splits on that.** The chat tests that canonicalize the
  temp dir first pass. The `chat_tool_loop` tests that pass a raw
  `tempfile::tempdir()` fail with "it's outside the folder" (call_1, call_2).
- **GH's Windows runners do set `TEMP` to the short name**,
  `C:\Users\RUNNER~1\AppData\Local\Temp`. Several projects hit it this way:
  R's `tempdir()` on GH Actions (r-devel, Aug 2025), pypa/build#413, and
  GreptimeDB#4518, whose logs sat under that path. Windows makes `TEMP` and
  `TMP` short names when they're set as user environment variables
  (Raymond Chen, *Why does the system convert TEMP to a short file name?*).

One other Windows spelling could hit the same compare, and the fix covers it
too: a junction or symlink on the way. Its `read_link` target comes back
verbatim (`\\?\C:\...`), which `real_path` would keep, against
`dunce::canonicalize`'s plain `C:\...`.

### What Linux can make

None of the aliases Linux can make reach `locate`'s compare (a symlinked root
does reach the chat context's, below):

- **A symlinked root or ancestor**: `real_path` follows every link, as
  `realpath` does, so both sides land on the target.
- **`.`, `//`, a trailing `/`**: `Path::components` drops them before either
  side sees them.
- **`..`**: `real_path` takes it from what's resolved so far, as the kernel
  does; `resolved_parents` has already removed it from the root anyway.
- **Bind mounts**: neither side resolves them, so they agree.
- **Case-folded folders** (ext4 `+F`, tmpfs `casefold`): glibc `realpath`
  keeps each name as given, as `real_path` does, so they agree; and they need
  a mount or a casefold-enabled file system besides.

So the edit half of the regression test can't fail at base on Linux. It makes the spellings
the platform has: a part in another letter case (Windows, and macOS on
case-insensitive APFS, where `realpath` reports the stored case) and, on
Windows, the 8.3 short name; where a spelling doesn't name the same folder
(Linux, a case-sensitive volume, short names turned off) it's skipped. A
root reached through a symlink runs on every unix, and its context checks fail
at base.

## The fix

`locate` puts what `real_path` resolved into its canonical spelling before the
compare: the deepest part of it that's there goes through
`dunce::canonicalize`, and the rest, which isn't there yet, is joined on as
named (`canonical_spelling`). Both sides are then spelled by the file system.

The guarantees hold:

- `real_path` still resolves every link and `..` first, so the walk out of
  the root is caught as before.
- Canonicalizing the existing part follows links too; if one were swapped in
  between, the result is still what's compared against the root, so it can
  only refuse more.
- The path returned is the canonical one, which is what
  `crate::open::file_identity` (via `lsp::resolve`) keys open editors by and
  what `chat_context::walk` reports on Windows, so the proposals' keys, the
  open-editor match and `open_vetted`'s deny-list check all see one spelling.

### The chat's context, too (review nrwmtt F1)

The file an edit opens is now keyed by its canonical path. But
`chat_context::open_vetted`, which vets the active file for automatic
context and an @file, compared that path lexically against the root as it
was opened ("it's outside the folder"). `walk`, which keeps names as
written, then walked the two from different spellings. **Linux reproduces
this one:** with a root opened through a symlink, the editor's path is the
link's target, which the written root doesn't prefix. The link case of
`tests::chat_edit_root_spelling` fails without the change and passes with it.
So does the card's regression test,
`tests::open_folder::chat_root_spelling::context_under_a_root_opened_through_a_link_is_inside_it`.
It's a submodule of a test module base already has, so the regression check
runs it at base.

`open_vetted` now spells the two alike first (`spelled_alike`):

- the root becomes `dunce::canonicalize(root)`;
- a path under the root as given is moved under that canonical spelling, the
  rest kept as written;
- a path already in the canonical spelling is left alone.

Both checks then run as before: the deny list on the names as written, and
`walk`'s real path against the walked root. `ChatPanel::attach_file`'s quick
check on the main thread used to refuse a path not lexically under the root.
Now it leaves such a path to that vetting, which runs off the main thread
anyway. A secret under the root is still refused by either spelling, and the
test checks that too.

### Not chosen

The alternative, putting the root through `real_path` as well, keeps the
written spelling on both sides but leaves the located path in a spelling no
editor is keyed by, so an editor already open on the file wouldn't be found.
