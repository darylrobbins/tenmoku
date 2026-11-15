---
task_uid: "01m7e4wp5rx04y8741av1ajh4e"
task: "T386"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests tests::module_folder_icons::gradle_module_and_maven_roots_get_their_roles, tests::module_folder_icons::dotnet_and_cargo_roots_get_their_roles and tests::module_folder_icons::a_plain_src_keeps_its_name_role pass"
    required: true
    manual: false
  - id: "C2"
    text: "tests::folder_markers::probe_is_bounded_cached_and_invalidated_by_watcher_batches passes (at most 10 stats per folder, one per folder across repeated frames, no recursion)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p kiln-ui -p fe-project -p fe-workbench passes, including tests::icon_stroke::every_bundled_icon_strokes_at_1_75"
    required: true
    manual: false
  - id: "C4"
    text: "Known limit: the screenshot goldens are Metal-only and the Mac lead regenerates them in the stack (its lend note says so): the decision file names the two file_icons goldens (file_icons_dark.png, file_icons_light.png) that move and states every other golden is unchanged by design"
    required: true
    manual: false
---

# Module and source-root folder icons in the project tree, derived from build files (Gradle, Maven, .NET, Cargo, npm, Go, Python) (Mac T1773)

# Module and source-root folder icons derived from build files

Today a folder's icon comes from its NAME alone: `folder_icon_ref_named(&name,
expanded)` (project_panel.rs) to `folder_named` to `FOLDER_NAMES`/`FolderRole`
in `kiln-ui/src/file_icons_tinted.rs`. So `src` is a source folder in every
project and a Gradle submodule `core/` looks like any folder. This card adds
context from build files, JetBrains-style, as new `FolderRole` variants.
Linux-OK (pure filesystem and icon logic).

## Where the context comes from: build files, not the language server

LSP has no standard project/module list (only `workspaceFolders`). jdtls
(`java.project.getAll`), Roslyn and rust-analyzer each have private requests,
need the server running and indexed (slow, absent for unopened projects) and
differ per server. Build files give the same answer for every language,
instantly, and work with no server. This card uses build files only.

## Roles (new `FolderRole` variants; Lucide glyphs, worker picks from Lucide
1.52.0 and notes it in `docs/decisions/<lane>-*.md`)

| Role | Detected by | Glyph (proposed) |
| --- | --- | --- |
| Module | folder directly contains `build.gradle(.kts)`, `pom.xml`, `*.csproj`, `*.fsproj`, `*.vbproj`, `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`; the project root itself is not marked | `folder-root` (or `blocks`), tint `folder_source` |
| Solution | folder with `*.sln`/`*.slnx` or `settings.gradle(.kts)`, or a root `Cargo.toml` with `[workspace]` | same glyph, tint `folder` |
| Source root | `src/main/java`, `src/main/kotlin`, `src/main/scala`, `src/main/groovy`, and a module's `src` when it has no `main`/`test` child; Cargo `src` | existing `folder-code` (`Source`) |
| Test root | `src/test/**` (java, kotlin, scala), Cargo `tests`, `benches` | existing `folder-check` (`Tests`) |
| Resources | `src/main/resources`, `src/test/resources` | existing `folder-bookmark` (`Assets`) |
| Excluded-looking | `bin`, `obj` next to a `*.csproj`; `build` next to Gradle; `target` next to Cargo | existing `folder-output` (`Output`) |

Name-only lookup stays the fallback; a context role wins over the name role
only for these folders (`src` under a Gradle `src/main` is the container, not
a source root: only `java`/`kotlin`/... children are marked).

## Cost (the hard requirement)

No scan of the project. A folder's role needs at most the folder's own listing
plus its parent's: for an expanded folder both come from the entries the tree
already loaded (zero extra IO). A collapsed folder gets its module marker from
a bounded probe, at most 10 marker names per folder, run once per folder off
the render path (background executor), cached in a map keyed by path and
invalidated by the watcher batch that names the folder or its children (the
existing fe-project watcher events; no new watch). Until the probe lands the
row draws its name role, so no frame waits on IO. No recursion: Maven's
`src/main/java` is judged from the `java` row's own ancestors (path
components), not by stat.

## Scope hints

`kiln/crates/kiln-ui/src/file_icons_tinted.rs`, `file_icons.rs` (the
`folder_named` seam gains a context argument: a small `FolderContext` of
parent names plus marker flags; existing callers pass the empty context),
`crates/fe-workbench/src/project_panel.rs` and a NEW `crates/fe-project/src/
folder_markers.rs` (the probe and cache), new Lucide SVGs if needed, tests in
NEW files declared from existing mods: `kiln/crates/kiln-ui/src/tests/
module_folder_icons.rs` (from `tests/mod.rs`) and `crates/fe-project/src/tests/
folder_markers.rs` (from `tests/mod.rs`).

## Goldens

Tree-only icon cards never move the `setup_wizard_*` goldens (they do not draw
project folders). `file_icons_{dark,light}.png` (icons scene, now 640 tall) move
if the scene gains a "Project roles" row: add it to `icons_scene.rs` and
regenerate those two only.

## Criteria (bare in-binary test ids)

C1  tests::module_folder_icons::gradle_module_and_maven_roots_get_their_roles,
    tests::module_folder_icons::dotnet_and_cargo_roots_get_their_roles and
    tests::module_folder_icons::a_plain_src_keeps_its_name_role pass.
C2  tests::folder_markers::probe_is_bounded_cached_and_invalidated_by_watcher_batches
    pass (asserts at most 10 stat calls per folder, one per folder across
    repeated frames, and no recursion into children).
C3  `cargo nextest run -p kiln-ui -p fe-project -p fe-workbench` passes,
    including tests::icon_stroke::every_bundled_icon_strokes_at_1_75.
C4  On the Mac: `cargo run -p fe-workbench --example screenshots` matches every
    golden except the two file_icons ones this card regenerates.
