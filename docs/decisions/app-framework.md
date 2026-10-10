# App framework: `kiln`, a GPUI-idiomatic layer that replaces `fe-shell`

Status: **build**. Decided 2026-10-10. Supersedes `app-shell.md` (see §6).
The name `kiln` is a placeholder; the final name is chosen before the
framework is open-sourced, and renamed in one change (card K6.4).

## 1. Decision

A reusable application framework on top of GPUI (`gpui-pre` =0.3.7) that
hides per-platform differences and gives apps high-level widgets, written in
GPUI's own idioms. It replaces the planned `fe-shell`: it covers everything
`app-shell.md` planned for it (panes, docks, the modal layer, the palette,
close vetoes, layout persistence, multiple windows), plus a platform layer
and a widget kit (app chrome, basic controls, data views, layout and forms).

- **Where.** In this repository for now, under a top-level `kiln/` subtree
  that can be split out with its history. It is to be open-sourced, so no
  kiln crate may depend on a fastedit crate, kiln code is ours or
  permissively licensed (`CLEAN_ROOM.md`), and every crate's licence is set
  explicitly so the flip is one line per crate.
- **Platforms.** macOS and Linux (X11 and Wayland) now. Windows is an
  addition: only `kiln-core/src/platform/` and `kiln-platform/src/` branch on
  the operating system, and CI checks the GPUI-free crates for Windows.
- **Why not keep the platform switches in fastedit.** The Linux port put
  them in three places: `fastedit/src/main.rs` (`UNIFIED_TITLE_BAR`,
  `NATIVE_MENUS`, the prompt builder), `fe-workbench` (`title_bar.rs`,
  `window_frame.rs`, `menu_bar.rs`) and `fe-gpui` (`commands.rs`,
  `keymap.rs`, `menu.rs`), with two keymaps and two menu tables to keep in
  step. Every GPUI app needs the same answers.

## 2. Crates

| Crate | GPUI | Holds |
|---|---|---|
| `kiln-command` | no | command IDs, invocations, context predicates, registries (from `fe-command`) |
| `kiln-core` | no | config layers and settings stores, app paths, platform conventions, keymaps and menu tables resolved per platform, theme data, the trust store, a headless text-edit model |
| `kiln-platform` | yes | platform capabilities, window options, prompts, fonts, dialogs, the key interceptor and its `KeyOwner` hook, command dispatch, native menus |
| `kiln-ui` | yes | theme resolution, icons and the widget kit (from `fe-ui`) |
| `kiln-shell` | yes | the `Root` view (window frame, in-window menu bar, modal layer) and the shell: panes, docks, palette, status, persistence, windows |
| `kiln` | facade | re-exports, the prelude, `kiln::gpui`, `application()` |

`kiln-command` and `kiln-core` stay GPUI-free, as the core crates do today
(`scripts/check-no-gpui.sh`), so headless tools and the AI backend can load
the same commands, config and keymaps.

## 3. API: extend GPUI, never wrap it

- **Setup** is plain GPUI: `kiln::application().run(|cx| …)`, then
  `kiln::init(cx, Config::new(app_id, name))`, then `cx.open_window(
  kiln::window_options(cx, spec), |window, cx| cx.new(|cx| Root::new(content,
  window, cx)))`. There is no kiln `App` type; `&mut App` and `Window` are
  reachable everywhere.
- **Extension traits** in `kiln::prelude`: `cx.theme()`, `cx.settings::<S>()`,
  `cx.set_platform_menus(..)`, `cx.bind_commands(..)`, `window.run_command(..)`.
  `window.prompt(..)` stays GPUI's, made the same everywhere by `init`.
- **Commands are `gpui::Action`s** with kiln metadata (title, context,
  description), declared with `kiln::commands!` or `#[derive(Action,
  Command)]`, handled with `.on_action(cx.listener(..))`. GPUI's
  `ns::snake` name maps to kiln's `ns.snake` command ID. Every dispatch, from
  a key, a menu, the palette or an agent, carries an `Invocation` record.
  `RunCommand(Invocation)` stays only as the fallback for headless
  registries (fe-editor's model commands).
- **Keys** keep kiln's own matcher (as `command-model.md` decided), for vim's
  priority (`KeyOwner`), chord replay, last-good hot reload with
  diagnostics, `.` and `==` in predicates, click bindings and headless
  per-platform tests. Bindings look like GPUI's (`KeyBinding::new
  ("secondary-s", Save, None)`), contexts come from `.key_context(..)`, and
  keys kiln doesn't match fall through to GPUI's keymap.
- **One keymap and one menu table per app.** `secondary-` is cmd on macOS
  and ctrl elsewhere; a binding can name keys per platform
  (`keys = { macos = "alt-left", default = "ctrl-left" }`) or be limited to
  some (`platform = "macos"`); menu items carry roles (`settings`, `quit`,
  `about`) that each platform places by its conventions.
- **Widgets.** Stateless ones are `RenderOnce` builders over a `Div`, so
  GPUI's `Styled`, `InteractiveElement` and `FluentBuilder` methods work on
  them; values are controlled. Stateful ones are `Entity<T>` views that emit
  events (`cx.subscribe_in`). Widgets read `cx.theme()` while rendering.

## 4. Compatibility for fastedit

- `fe-command` and `fe-ui` become package aliases of `kiln-command` and
  `kiln-ui`; `fe-config` and `fe-keymap` become facades over `kiln-core`
  holding fastedit's own content (its settings, keymaps, menus, theme, and
  paths pinned to `fastedit` in the XDG layout, so no user's files move).
  `fe-gpui` and `fe-workbench` re-export what moves out of them. Existing
  tests stay unedited.
- No command ID or key context is renamed: fastedit keeps `workbench.*` and
  the `workbench` context, which the shell takes as a parameter. A renamed
  context would silently stop matching users' keymap rows.

## 5. Milestones

M-K0 preconditions (this record; the Linux branch landing; 1× goldens so CI
compares pixels), M-K1 the GPUI-free core, M-K2 the platform layer, M-K3
theme tokens, M-K4 the `Root` chrome and the widget kit with its gallery,
M-K5 the shell (app-shell's S1–S11), M-K6 open-source readiness (a host
example, a standalone build check, licensing files, the rename). The core
and platform milestones come first: every shell file dispatches through
`RunCommand`, which can't leave `fe-gpui` until the core it depends on has.

## 6. What happens to `app-shell.md`

Its §1–§2 crate boundary is replaced by §2 here. Its §3–§4 (`PaneItem`,
`ItemHandle`, `DockItem`, `Shell`, the modal layer, close vetoes, the
palette, persistence, multiple windows) remain the reference design for
`kiln-shell`. Its cards map onto K-cards: S1 → K5.2, S2 → K5.3, S3 → K5.4,
S4 → K1.1, K2.1, K2.2 and K3.1, S5 → K5.1 (its title bar moves in K4.1),
S6 → K5.5, S7 → K5.6, S8 → K5.7, S9 → K5.8, S10 → K5.9, S11 → K5.10,
S12 → K6.1.

Its open questions:

- Q1 (where `impl PaneItem for EditorView` lives): unchanged, in fe-gpui.
- Q2 (may the shell use `log`): yes. A published library uses the `log`
  facade; a report handler is optional, for user-facing notices.
- Q3 (host icons): icons become an open `IconName` (an asset path), with the
  bundled Lucide set as constants, so apps add their own.
- Q5 (one dock or several): `DockPosition` exists from the start; fastedit
  registers only the right dock.
- Q9 (names): framework names are neutral; fastedit's stay as they are.
- Q4, Q6, Q7, Q8 and Q10 carry over unchanged; Q8's layouts live under the
  app's `AppPaths` state directory.
