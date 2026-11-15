# Module and source-root folder icons from build files (T386, Mac T1773)

## Status

Done on Linux, 2026-11-15. The two `file_icons` goldens are left for the Mac
lead (below).

## Where the context comes from

Build files only, never a language server: no LSP request lists modules, the
private ones (jdtls, Roslyn, rust-analyzer) need the server running and
indexed, and differ per server. A folder's own entries and its parent's
answer for every language at once, with no server.

## The seam

`kiln-ui` gains `FolderContext` (the parent's and grandparent's names below
the project root, `own` build files and `beside`, the parent's) and
`BuildFiles` (one flag per tool's file, plus `main_or_test`, a `main` or
`test` folder). `IconSet::folder_in(name, context, expanded)` is a new
method whose default is `folder_named`, so `LucideIcons` and `ColouredIcons`
are unchanged and every existing caller of `folder_named` and
`folder_icon_ref_named` keeps working; `folder_icon_ref_in` is the free
form. Adding a method rather than changing `folder_named`'s signature keeps
`file_icons_coloured.rs` and the existing tests that call `folder_named`
untouched. `TintedIcons::context_role` decides, and returns "no say" where
the context says nothing, so the name role stays the fallback.

`FolderRole` gains `Module` (tint `folder_source`) and `Solution` (tint
`folder`). They are left out of `FolderRole::ALL`, which now documents itself
as the roles a name gives: `tinted_project_icons.rs` checks `ALL` against its
table of named roles, and no name gives these two.

## Roles

| Role | Rule (in this order) |
| --- | --- |
| Solution | own `settings.gradle(.kts)`, `*.sln`/`*.slnx`, or a `Cargo.toml` with a `[workspace]` table (or subtable) at a line's start |
| Module | own `build.gradle(.kts)`, `pom.xml`, `*.csproj`/`*.fsproj`/`*.vbproj`, `Cargo.toml`, `package.json`, `go.mod` or `pyproject.toml`, whatever the folder's name |
| Source (`folder-code`) | `java`, `kotlin`, `scala`, `groovy` in `src/main`; `src` beside a `Cargo.toml`; a module's `src` with no `main`/`test` folder |
| Tests (`folder-check`) | `java`, `kotlin`, `scala` in `src/test`; `tests`, `benches` beside a `Cargo.toml` |
| Assets (`folder-bookmark`) | `resources` in `src/main` or `src/test` |
| Output (`folder-output`) | `bin`, `obj` beside a .NET project; `build` beside a Gradle build; `target` beside a `Cargo.toml` |
| plain folder | a module's `src` holding `main` or `test`: the container, not a root |

The project root has no row, so it is never marked; its build files count
only as `beside` for its children. Names compare ASCII case-insensitively,
as the name lookup does; marker file names match exactly (the tools read them
so), extensions case-insensitively. Maven's and Gradle's roots are judged from
the row's own path components (`parent`, `grandparent`), never by stat.

## Glyph

Lucide 1.52.0 (`500620a2e8123f8d1db191538886dc0c223f69a9`) `folder-root`,
for both Module and Solution: a folder glyph, like every other role's
(`FolderRole`'s rule that every folder keeps one shape), where `blocks`, the
spec's alternative, is not one, and already the outline's symbol-kind glyph.
Copied with the stroke at 1.75 as every Lucide icon is; its contributor is
`danielbayley`, not Feather's, so ISC. Its row is added to both
`THIRD_PARTY.md` tables. The root table also lacked `plus.svg` (T1769's New
menu icon, recorded only in `kiln/THIRD_PARTY.md`); it is added there with its
note, so both tables now list the 140 SVGs on disk.

## Cost

No scan of the project, and no frame waits on IO.

- Each drawn folder row's folder, and its parent (the root included), gets
  one `MarkerProbe`, run on the background executor: the folder opened from
  `/` a component at a time without following a symlink
  (`listing::open_dirs`, the tree's own), its names read once through that
  handle (`listing::read_dir`, no stat per entry), and nothing below it.
  Metadata calls are at most `MAX_STATS` = 10 in all: an entry whose kind the
  file system doesn't report (`DT_UNKNOWN`) and whose name matters is
  looked up through the handle while nine last (past them it counts as a
  file), and a `Cargo.toml`'s open file is checked once. That `Cargo.toml`
  is opened by `open_file_under` (no-follow, non-blocking, so a link isn't
  read through and a FIFO isn't waited on), read if it's a regular file of
  at most 1 MiB, and parsed with `toml` for a top-level `workspace` key
  (`[workspace]`, `[ "workspace" ]`, `workspace.members = …`; a multiline
  string or comment that only looks like one doesn't count).
- Reading the names, rather than stat'ing ten fixed ones, is what finds
  every `*.csproj`/`*.fsproj`/`*.vbproj`/`*.sln`/`*.slnx` and a `src`
  folder's own build files beside its `main`/`test` (review F4, F5); it's
  one directory read per folder, the read the tree itself makes to list it.
- The probe wins over the tree's listing once it lands, so a folder's role
  doesn't change as it's expanded or ignored entries are shown: the listing
  hides ignored entries (review F7). Until it lands, a listed folder has its
  listing's markers (`Markers::of_listing`, no IO, worked out once per tree
  version per folder in the panel, `ListedMarkers`), an unlisted one none,
  so the row draws its name role.
- `MarkerCache` (in the panel) holds each result by absolute path. A folder
  is requested once however many frames draw it. A watcher batch (the
  existing `apply_fs_changes`, no new watch) forgets each folder it names,
  with everything below it, and each folder holding an entry it names whose
  name can count (a marker name, a .NET extension, `main` or `test`,
  ASCII case-insensitively), so saving a source file probes nothing; a
  rescan forgets all, as does opening the tree again. `apply` says whether
  it forgot anything, and the panel then redraws, so a change that leaves
  the tree's rows as they were (a build file in a collapsed folder, a
  `Cargo.toml`'s text) is shown and probed again (review F2). A result for a
  folder forgotten since its request is dropped (its serial no longer
  matches), and the folder probed again on its next frame.

## Tests

- `kiln-ui` `tests::module_folder_icons` (C1): Gradle and Maven modules, a
  Gradle solution, the `src` container, `src/main|test/<lang>` and
  `resources`, Gradle's `build`; .NET's project, solution, `bin` and `obj`;
  Cargo's crate, workspace, `src`, `tests`, `benches` and `target`; and that
  with nothing known, or no build files about, every folder keeps its name's
  icon (and `LucideIcons` ignores the context).
- `fe-project` `tests::folder_markers::probe_is_bounded_cached_and_invalidated_by_watcher_batches`
  (C2): every probe call recorded (test-only `CALLS`: opens, the one
  directory read, every metadata call): at most 10 metadata calls, one read
  of the folder's names, only its own entries touched (none in
  `core/nested`, whose `Cargo.toml` and `package.json` aren't `core`'s), only
  a `Cargo.toml` opened; one probe per folder over four frames; every .NET
  spelling; a `src` with a `Cargo.toml`; an ignored `pyproject.toml`
  counting, and a listed folder's role kept once its probe lands; batches
  naming an unrelated entry forget nothing, ones naming a build file, the
  folder or a `main`/`test` forget exactly those; a `Cargo.toml` edit read
  again; a stale result dropped; a rescan forgets all.
- `fe-project` `tests::folder_markers::probe_follows_no_link_and_reads_manifests_as_toml`:
  the workspace spellings and look-alikes, a manifest past the limit, a
  linked `Cargo.toml` and a FIFO, and a folder (or a folder above it)
  replaced by a link after its probe was requested, which reads nothing.
- `fe-workbench` `tests::project_folder_icons::build_file_changes_redraw_folder_icons`:
  in the panel, collapsed folders take their roles from their probes; a
  `build.gradle` made and removed in a collapsed folder, and a `Cargo.toml`
  made a workspace by its text, redraw the rows; a module's `src` holding
  `main` is a plain folder, and a source root again once the module goes.

## Goldens that move (Metal-only; the Mac lead regenerates them)

`crates/fe-workbench/examples/screenshots/icons_scene.rs` gains a project
roles row: `core/`, closed, holding `build.gradle`, drawn as a Module
(`folder-root` in `file_icon.folder_source`). The two goldens it renders move:

- `file_icons_dark.png`
- `file_icons_light.png`

Every other golden is unchanged by design: no other scene's tree has a folder
with a build file of its own, and the context rules that apply there give the
role the name already gave (`src` and `target` beside the scenes' root
`Cargo.toml` are Source and Output by name too). The `setup_wizard_*` goldens
draw no project folders.
