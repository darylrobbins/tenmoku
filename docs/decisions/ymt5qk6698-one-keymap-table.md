# T120: one keymap table, the per-platform copies generated

`crates/fe-keymap/build.rs` generates `DEFAULT_MACOS`, `DEFAULT_LINUX`,
`DEFAULT_MACOS_MENUS` and `DEFAULT_LINUX_MENUS` from `keymaps/fastedit.toml`
and `menus/fastedit.toml`; the four hand-kept files are deleted. The tables
changed by comment lines only.

## C2: nothing lost

Method: before the deletion, a temporary test (not committed: after the
change it would compare two things derived from one source) took the base
commit's (65b3bc2) four files via `git show` and the generated text, and for
each of macOS, Linux and Windows compared, in `tests/one_keymap.rs`'s form,
the resolved bindings (`keys | command | context | args`, in order, more than
100 of them) and the `MenuBar` (`MenuBar::parse_for`) of the base copy
against the generated text: the macOS copies against macOS, the Linux copies
against Linux and Windows (no `platform` filter names Windows, so it resolves
as Linux).

Result: equal on all three, bindings and menu bar. A second check: no
`platform` filter or `keys` table in the tables names an OS other than
macos/linux.

The build also panics on an unknown platform or malformed `keys`, checked on
every entry, including those left out for every OS.
