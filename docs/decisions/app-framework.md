# App framework: `kiln`, a GPUI-idiomatic layer that replaces `fe-shell`

Status: **build**. Decided 2026-10-10. Supersedes `app-shell.md` (see §6).
The name `kiln` is a placeholder; the final name is chosen before the
framework is open-sourced, and renamed in one change (card K6.4).
`kiln/docs/` holds the framework's standalone copy of this record and of
the parts of `command-model.md` it implements, written without fastedit,
so kiln's code can cite them once it leaves this repository; this record
stays the source of truth here (card K6.3).

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
  keys kiln doesn't match fall through to GPUI's keymap. That keymap holds
  only what kiln doesn't match: the menus' display-only bindings, which
  kiln replaces on each keymap swap, and a widget's own GPUI bindings (such
  as `TextField`'s), which it leaves alone.
- **One keymap and one menu table per app.** `secondary-` is cmd on macOS
  and ctrl elsewhere; a binding can name keys per platform
  (`keys = { macos = "alt-left", default = "ctrl-left" }`) or be limited to
  some (`platform = "macos"`); menu items carry roles (`settings`, `quit`,
  `about`) that each platform places by its conventions.
- **The shell.** A window's `Shell` view lays out the app's centre view,
  the docks beside and below it, the status bar and the modal layer under
  the chrome, with the app's root key context, which the app passes in
  (fastedit's `workbench`). Where an app keeps a piece's state in a view
  of its own (to read it without a context, or to add to what it does),
  kiln gives the state and a view trait as well as a ready view:
  `DockState` with `DockView` and `Dock`, and a `RootTrust` that any
  `TrustHost` view holds. The modal layer (`ModalLayer`, `ModalId`) shows
  any `ModalView`: a GPUI `ManagedView` that can be dismissed from
  outside, as `Picker` can.
- **Widgets.** Stateless ones are `RenderOnce` builders over a `Div`, so
  GPUI's `Styled`, `InteractiveElement` and `FluentBuilder` methods work on
  them; values are controlled. Stateful ones are `Entity<T>` views that emit
  events (`cx.subscribe_in`). Widgets read `cx.theme()` while rendering.
- **GPUI internals kiln relies on.** gpui-pre is pinned (`=0.3.7`), and a
  bump is its own card. `kiln::commands!` implements `gpui::Action` by hand
  rather than through GPUI's derive, so that an app needs no `gpui`
  dependency of its own: the derive writes bare `gpui::` paths. That impl
  names `gpui::MacroActionData`, `MacroActionBuilder` and
  `gpui::private::inventory`, which are public but not stable. A bump that
  changes them fails to compile in kiln-platform, and
  `the_macro_makes_gpui_actions` and `the_macro_needs_no_gpui_of_its_own`
  test the result.

## 4. Compatibility for fastedit

- `fe-command` and `fe-ui` become package aliases of `kiln-command` and
  `kiln-ui`; `fe-config` and `fe-keymap` become facades over `kiln-core`
  holding fastedit's own content (its settings, keymaps, menus, theme, and
  paths pinned to `fastedit` in the XDG layout, so no user's files move).
  `fe-gpui` and `fe-workbench` re-export what moves out of them. Existing
  tests stay unedited.
- No command ID or key context is renamed: fastedit keeps `workbench.*` and
  the `workbench` context, which the shell takes as a parameter. A renamed
  context would silently stop matching users' keymap rows. The one rename
  is internal: the menus' display-only GPUI binding context and action,
  `fastedit_menu` and `fastedit::MenuCommand`, become `kiln_menu` and
  `kiln::MenuCommand`. No element sets that context and no user keymap can
  name it, since users' rows go to kiln's matcher, not GPUI's.

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

## 7. Where kiln-shell departs from `app-shell.md`

K5's batch C (K5.7–K5.10) follows `app-shell.md` §3.6 and §4 as they read
after T275 (its F4 claims and F5 close transactions), except as follows.

**Close vetoes (K5.7, §3.6 and F5).**

- `Shell::confirm_close(items) -> Task<bool>` is instead
  `kiln_shell::close::confirm(CloseRequest, commit, window, cx) ->
  Closing`. A tab closes from its pane's update and a pane from the host's,
  outside any `Context<Shell>`, and the close must happen in the caller's
  update when nothing is asked (`Closing::Now`), or else in the update
  that revalidates the transaction (the `commit` closure, `Closing::Later`).
  A `CloseRequest` names its kind (items, pane, window, quit) and, per
  window, the items it closes and every item open there, read now and
  again on each later pass (`LiveItems`), so a pane or window that gains
  an item while it waits asks about it. Hosts give each window's items
  with `close::set_window_items`.
- `app.quit` and `window.close` stay kiln-platform's app commands; a
  hook (`kiln_platform::commands::set_close_handlers`) has them run
  kiln-shell's `close::quit` and `close::close_window`, which
  `kiln_shell::init` installs. `close::set_quit` replaces `cx.quit()` for
  a test or a host that does more as it quits.
- A request that has nothing to ask (each item it closes has its document
  open in an item that stays) closes at once even while another close
  waits on an answer, rather than waiting behind it: fastedit's
  `closing_panes_and_tabs` test closes such a pane while another pane's
  close asks.
- Within one transaction an item isn't asked if an item it closes that
  shows the same document has agreed at an unchanged revision, so closing
  a window with two split views of one dirty file asks once.
- `PaneItem::close_revision`'s default follows `is_dirty` alone;
  `EditorView`'s also follows its buffer's version.
- Cancel is the default on `EditorView`'s Discard/Cancel prompt, on every
  platform (decided 2026-10-11): Return and Escape keep the editor. GPUI
  gives Return to the first button and Escape to a cancel button, taking
  Return from it, so Cancel comes first as an ok button and Escape finds
  it by its title, as AppKit's alerts and kiln-ui's in-window prompt do.

**Command palette (K5.8, §4.3).**

- It's kiln-shell's `command_palette`: `Shell` handles
  `command_palette.toggle` (a typed command) and shows a
  `Picker<CommandPalette>` as the modal `command_palette`. Registries
  aren't enumerable from a global, so it lists the app registry's commands
  (`app.*`, `window.*`), every typed command, and what an app adds with
  `command_palette::add_commands` (fastedit adds each of its registries'
  `commands()`, the editor's and vim's), each once, by title. Commands
  with args, and `command_palette.toggle` itself, are left out.
- Keys are labelled with `kiln_core::shortcuts` and `shortcut_label`
  (this platform's label rather than `macos_label` everywhere), looked up
  as each row first draws.
- fastedit binds it to secondary-shift-p in the `workbench` context, the
  last binding of `fastedit.toml` (and of the per-platform files kept in
  step with it), so the old table's order, which fe-gpui's keymap tests
  zip against, is unchanged.

**Layout persistence (K5.9, §4.6 and F4).**

- The file, its merging writes and the claims are kiln-core's
  `layout::LayoutStore` (GPUI-free, so two stores stand in for two
  processes in its tests); `AppPaths::layouts_dir` is
  `<state>/layouts`. Claims lock `<key>.<id>.lock` with std's
  `File::try_lock` (`flock` on Unix, `LockFileEx` on Windows), so kiln
  branches on no OS for them. A claim whose entry was deleted also
  removes its lock file as it's released; others leave theirs, as the
  trust store leaves its lock.
- The GPUI side is kiln-shell's `layout`: `register_item::<T>` (and
  `register_kind`, a restore closure, for a host that restores a kind its
  own way), `save_panes`/`restore_panes`, `DockState::save`/`restore`
  (and `joined`), `save_bounds`/`restore_bounds`, and `WindowLayout`, the
  window's claim and its saves through one background `persist::Writer`.
  The entry is `{bounds, panes: {tree, active}, dock, …}` plus the host's
  own keys (fastedit's `breadcrumbs`).
- "After each layout change" is when the entry's shape changes: the
  entry without the items' states (carets) and the bounds, compared as
  the window draws. The full entry is saved at quit and as the window
  closes, which also catches carets and bounds. Nothing is saved while a
  restore is still reading files.
- A restored window's items land together, once its documents are read,
  rather than each tab showing the pane's placeholder until its own does;
  the panes are empty until then.
- The window's close is heard both through `on_window_closed` and the
  host view's release, whichever comes first: fastedit's workbench can
  outlive its window (its language servers retire the same way).
- `PaneItem::restore` can't reach a host's theme, so fe-gpui's
  `set_restore_style` gives restored editors the workbench's theme and
  editor settings.
- A file launch restores nothing, as Q4 says, but its window still gets
  a fresh claimed id and saves its layout under the file's folder.

**Multiple windows (K5.10, §4.1).**

- kiln-shell's `layout::launch` (a launch's windows: every entry no live
  window holds, else one fresh id) and `layout::new_window` (the newest
  unclaimed entry, else a fresh id) are what a host opens windows with;
  `layout::mark_quitting` marks a quit that GPUI's quit observers don't
  start (and tests' quits, as GPUI's test platform can't quit with a
  window open).
- `window.new` is fastedit's command (in fe-workbench's open registry,
  bound to secondary-shift-n), not a kiln app command, and so is
  `workbench.open_folder_in_new_window` (Open Folder in New Window…), a
  command of its own rather than a choice in the open panel. Both open
  through the host's `fe_workbench::set_window_opener`. Neither is in the
  File menu yet, as fastedit's menu tests pin the File menu's items; the
  command palette lists both.
- Settings reloads fan out to every workbench window. Each window's
  settings are the user's file under its root's project file: the config
  service loads and watches the launch root's; another root's is read off
  the main thread when its window opens, when it's re-rooted, and when
  the user's file reloads, but its own project file isn't watched (its
  edits apply at the next of those).
- Window commands (`window.close` and the rest) act on the window they
  were dispatched in (kiln-platform's `command_actions::dispatch_window`),
  falling back to the platform's active window: an X server without a
  window manager has none, and `window.close` did nothing there.
