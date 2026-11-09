// SPDX-License-Identifier: Apache-2.0

//! The menu bar drawn in the window, where the platform has no native one
//! (Linux): the menus [`kiln_platform::menus::install`] set, as a row of
//! names above the app's content, each opening a panel of its items with
//! their shortcuts.
//!
//! Opening a menu takes focus, so its keys reach it (Left and Right between
//! menus, Up and Down between items, Enter, Escape), and closing gives
//! focus back to what had it. A chosen item runs after that, in that
//! element's context, as a native menu's does
//! ([`kiln_platform::menus::run`]).
//!
//! [`Open`] (`menu_bar.open`, which apps bind to F10 on Linux) opens the
//! first menu: whoever draws the bar handles it, as [`crate::Root`] does,
//! once [`register`] has recorded it.

use gpui::{
    App, Context, DispatchPhase, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseDownEvent, ParentElement as _, Render, Styled as _, Subscription, Window,
    canvas, div,
};
use kiln_command::{CommandId, Value};
use kiln_core::{CatalogError, CommandCatalog, ItemKind, Keymap, Menu, MenuBar, shortcut_label};
use kiln_platform::{keys, menus};
use kiln_ui::{MenuEntry, MenuPanel, MenuStrip, ui_theme};

/// The key context of the bar, while a menu is open.
pub const KEY_CONTEXT: &str = "menu_bar";

kiln_platform::commands!(
    menu_bar,
    [
        /// Open Menu Bar
        Open,
    ]
);

/// The menu bar's commands, to check keymap bindings against without an
/// app: `menu_bar.open`. An app on kiln's command registry checks them
/// once [`register`] has run.
pub fn catalog() -> Box<dyn CommandCatalog> {
    Box::new(Catalog)
}

struct Catalog;

impl CommandCatalog for Catalog {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        use kiln_platform::command_actions::Command as _;
        if *command != Open::id() {
            return Err(CatalogError::UnknownCommand);
        }
        <Open as gpui::Action>::build(args.clone())
            .map(drop)
            .map_err(|error| CatalogError::BadArgs(error.to_string()))
    }
}

/// The open menu, and the item highlighted in it. The indices are where
/// they were last found; the menu's ID and the command are what they are
/// found by when the installed menus change.
#[derive(Clone, Debug, PartialEq, Eq)]
struct OpenMenu {
    menu: usize,
    id: String,
    /// An index into the menu's items; never a separator's.
    item: Option<usize>,
    /// The command at `item`.
    command: Option<String>,
}

impl OpenMenu {
    /// Menu `menu` of `bar` open, with its item `item` highlighted if that
    /// is a command.
    fn at(bar: &MenuBar, menu: usize, item: Option<usize>) -> Option<Self> {
        let id = bar.menus.get(menu)?.id.clone();
        let command = item.and_then(|item| command_at(bar, menu, item));
        let item = item.filter(|_| command.is_some());
        Some(Self {
            menu,
            id,
            item,
            command,
        })
    }

    /// This, found again in `bar` by its menu's ID and its item's command:
    /// the first menu of that ID, and the first command item in it of that
    /// ID. `None` if the menu is gone; no highlight if the command is.
    fn locate(&self, bar: &MenuBar) -> Option<Self> {
        let menu = bar.menus.iter().position(|menu| menu.id == self.id)?;
        let item = self.command.as_ref().and_then(|id| {
            bar.menus[menu]
                .items
                .iter()
                .position(|item| item.command().is_some_and(|command| command.id == *id))
        });
        Some(Self {
            menu,
            id: self.id.clone(),
            item,
            command: item.and(self.command.clone()),
        })
    }
}

fn command_at(bar: &MenuBar, menu: usize, item: usize) -> Option<String> {
    let item = bar.menus.get(menu)?.items.get(item)?;
    Some(item.command()?.id.clone())
}

/// The in-window menu bar's view: the installed menus' names, and the open
/// one's panel. Its host lays it out in a row of
/// [`crate::window_frame::TopRow`], which draws the background, and opens
/// it on [`Open`].
pub struct MenuBarView {
    open: Option<OpenMenu>,
    focus: FocusHandle,
    /// What had focus before a menu opened, given it back when it closes.
    restore: Option<FocusHandle>,
    /// Closes the menu when focus leaves the bar, as a click elsewhere
    /// moves it.
    _focus_out: Option<Subscription>,
}

impl MenuBarView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            open: None,
            focus: cx.focus_handle(),
            restore: None,
            _focus_out: None,
        }
    }

    /// The open menu's index, and its highlighted item's, if any.
    pub fn open_menu(&self) -> Option<(usize, Option<usize>)> {
        self.open.as_ref().map(|open| (open.menu, open.item))
    }

    fn bar(cx: &App) -> Option<MenuBar> {
        menus::installed(cx).cloned()
    }

    /// Opens menu `menu`, highlighting its first item if `highlight`.
    pub fn open(
        &mut self,
        menu: usize,
        highlight: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bar) = Self::bar(cx) else {
            return;
        };
        if menu >= bar.menus.len() {
            return;
        }
        if self.open.is_none() {
            self.restore = window.focused(cx).filter(|focused| *focused != self.focus);
            window.focus(&self.focus, cx);
            self._focus_out = Some(cx.on_focus_out(&self.focus, window, |bar, _, _, cx| {
                bar.open = None;
                bar.restore = None;
                cx.notify();
            }));
        }
        let item = highlight.then(|| next_item(&bar, menu, None, 1)).flatten();
        self.open = OpenMenu::at(&bar, menu, item);
        cx.notify();
    }

    /// Closes the open menu and gives focus back.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.take().is_none() {
            return;
        }
        if let Some(restore) = self.restore.take() {
            window.focus(&restore, cx);
        }
        cx.notify();
    }

    /// Closes the menu, then runs its item `item` where focus went back to.
    fn choose(&mut self, menu: usize, item: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bar) = Self::bar(cx) else {
            return;
        };
        let Some((menu, id)) = bar.menus.get(menu).and_then(|m| {
            let id = m.items.get(item)?.command()?.id.clone();
            Some((m.id.clone(), id))
        }) else {
            return;
        };
        self.close(window, cx);
        window.defer(cx, move |window, cx| {
            menus::run(&menu, &id, window, cx);
        });
    }

    /// The open menu, reconciled with `bar`, which may have changed since
    /// it opened (as when a command is contributed): found again by its
    /// menu's ID and its highlight's command, closed, giving focus back,
    /// if `bar` has no such menu; its highlight dropped if the menu has no
    /// such command.
    fn reconcile(
        &mut self,
        bar: &MenuBar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<OpenMenu> {
        let open = self.open.as_ref()?;
        let Some(located) = open.locate(bar) else {
            self.close(window, cx);
            return None;
        };
        if located != *open {
            self.open = Some(located);
            cx.notify();
        }
        self.open.clone()
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let (true, Some(bar)) = (self.open.is_some(), Self::bar(cx)) else {
            return;
        };
        if event.keystroke.modifiers.modified() {
            return;
        }
        let Some(open) = self.reconcile(&bar, window, cx) else {
            // Its menu is gone: the key closed it.
            cx.stop_propagation();
            return;
        };
        let menus = bar.menus.len();
        match event.keystroke.key.as_str() {
            "escape" => self.close(window, cx),
            "left" => self.open((open.menu + menus - 1) % menus, true, window, cx),
            "right" => self.open((open.menu + 1) % menus, true, window, cx),
            step @ ("up" | "down") => {
                let step = if step == "up" { -1 } else { 1 };
                self.open =
                    OpenMenu::at(&bar, open.menu, next_item(&bar, open.menu, open.item, step));
                cx.notify();
            }
            "enter" => match open.item {
                Some(item) => self.choose(open.menu, item, window, cx),
                None => self.close(window, cx),
            },
            _ => return,
        }
        cx.stop_propagation();
    }
}

/// What the panel of `menu` draws, an entry for each item, so the panel's
/// indices are the menu's: each title in the active locale, a command's
/// with its shortcut in `keymap`. A system item, which only macOS fills,
/// shows its title and runs nothing; it is never highlighted.
fn entries(menu: &Menu, keymap: &Keymap) -> Vec<MenuEntry> {
    menu.items
        .iter()
        .map(|item| match &item.kind {
            ItemKind::Command(command) => MenuEntry::Item {
                title: command.label().resolve(),
                shortcut: command
                    .shortcut(keymap)
                    .map(|keys| shortcut_label(&keys).into()),
            },
            ItemKind::System(system) => MenuEntry::Item {
                title: system.label().resolve(),
                shortcut: None,
            },
            // A `remove` is only for extending a bar.
            ItemKind::Separator | ItemKind::Remove(_) => MenuEntry::Separator,
        })
        .collect()
}

/// The command item `step` (1 or -1) from `from` in menu `menu`, wrapping
/// and skipping separators; from `None`, the first (or last) one.
fn next_item(bar: &MenuBar, menu: usize, from: Option<usize>, step: isize) -> Option<usize> {
    let items = &bar.menus.get(menu)?.items;
    let len = items.len() as isize;
    let start = from.map_or(if step > 0 { -1 } else { len }, |ix| ix as isize);
    (1..=len)
        .map(|offset| (start + step * offset).rem_euclid(len) as usize)
        .find(|&ix| items[ix].command().is_some())
}

impl Focusable for MenuBarView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for MenuBarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The strip and panel read `cx.theme()`; they still take the flat
        // colours, which they ignore.
        let ui = ui_theme(cx);
        let bar = Self::bar(cx).unwrap_or_default();
        let names = bar.menus.iter().map(|menu| menu.label().resolve());
        let entity = cx.entity();
        let mut strip = MenuStrip::new(names, &ui)
            .on_click({
                let entity = entity.clone();
                move |ix, window, cx| {
                    entity.update(cx, |bar, cx| match &bar.open {
                        Some(open) if open.menu == ix => bar.close(window, cx),
                        _ => bar.open(ix, false, window, cx),
                    })
                }
            })
            .on_hover({
                let entity = entity.clone();
                move |ix, window, cx| {
                    entity.update(cx, |bar, cx| {
                        if bar.open.as_ref().is_some_and(|open| open.menu != ix) {
                            bar.open(ix, false, window, cx);
                        }
                    })
                }
            });
        let located = self.open.as_ref().map(|open| open.locate(&bar));
        if located
            .as_ref()
            .is_some_and(|located| *located != self.open)
        {
            // The installed menus changed under it: reconcile it once
            // drawn, closing it if its menu went, when focus can move.
            cx.defer_in(window, |this, window, cx| {
                let bar = Self::bar(cx).unwrap_or_default();
                this.reconcile(&bar, window, cx);
            });
        }
        if let Some(open) = located.flatten()
            && let Some(menu) = bar.menus.get(open.menu)
        {
            let highlighted = open.item;
            let keymap = keys::current_keymap(cx);
            let entries = entries(menu, &keymap);
            let menu_ix = open.menu;
            let panel = MenuPanel::new(entries, &ui)
                .highlighted(highlighted)
                .on_click({
                    let entity = entity.clone();
                    move |item, window, cx| {
                        entity.update(cx, |bar, cx| bar.choose(menu_ix, item, window, cx))
                    }
                })
                .on_hover(move |item, _, cx| {
                    entity.update(cx, |bar, cx| {
                        let Some(open) = &bar.open else {
                            return;
                        };
                        if open.item != Some(item)
                            && let Some(open) = Self::bar(cx)
                                .and_then(|menus| OpenMenu::at(&menus, open.menu, Some(item)))
                        {
                            bar.open = Some(open);
                            cx.notify();
                        }
                    })
                });
            strip = strip.open(open.menu, panel);
        }
        // While a menu is open, any press that reaches this listener closes
        // it: the names and the panel stop theirs. Painted before the names,
        // so it hears a press after them.
        let closer = self.open.is_some().then(|| {
            let entity = cx.entity();
            canvas(
                |_, _, _| (),
                move |_, (), window, _| {
                    window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble {
                            entity.update(cx, |bar, cx| bar.close(window, cx));
                        }
                    });
                },
            )
            .absolute()
            .size_0()
        });
        div()
            .id("menu-bar")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .relative()
            // The row it's laid out in draws the background and border.
            .flex_none()
            .h_full()
            .children(closer)
            .child(strip)
    }
}

/// Menu titles in the active locale, in the GPUI menus kiln-platform
/// builds and in the bar drawn here: the bundled title of a Services item
/// (kiln-core's `kiln-core-menu-services`), a command item a table keys
/// with `title_key`, and a menu named in the table, which stays as
/// written. The process catalogue is global: this test switches it, and is
/// its own process under nextest.
#[cfg(test)]
mod tests {
    use std::path::Path;

    use gpui::{OwnedMenuItem, TestAppContext, VisualTestContext};
    use kiln_core::{ItemKind, Keymap, MenuBar};
    use kiln_l10n::{LanguageIdentifier, Registered, ResourceFile, Resources};
    use kiln_platform::{commands, keys, locale::set_locale, menus};

    use super::MenuBarView;

    /// A fake locale, `qps-ploc`, for this binary: kiln-core's Services
    /// title and kiln-shell's `menu_bar.open` title.
    static FAKE_LOCALE: Resources = Resources {
        crate_name: "kiln-shell-menu-fake-locale",
        manifest_dir: env!("CARGO_MANIFEST_DIR"),
        declared_in: file!(),
        files: &[ResourceFile {
            locale: "qps-ploc",
            path: "qps-ploc/menus.ftl",
            source: "kiln-core-menu-services = ⟦Sérvícés⟧\n\
                     command-menu_bar-open = ⟦Ópén Ménú Bár⟧\n",
        }],
        dynamic_prefixes: &[],
    };

    inventory::submit! { Registered(&FAKE_LOCALE) }

    /// A bundled table: one keyed item, and a Services item with no title.
    const MENUS: &str = r#"
[[menus]]
name = "Probe"

[[menus.items]]
title_key = "command-menu_bar-open"
command = "menu_bar.open"

[[menus.items]]
separator = true

[[menus.items]]
system = "services"
"#;

    /// The installed GPUI menus' titles: each menu's, then its items'.
    fn gpui_titles(cx: &mut VisualTestContext) -> Vec<(String, Vec<String>)> {
        let menus = cx.update(|_, cx| cx.get_menus()).expect("menus are set");
        (menus.into_iter())
            .map(|menu| {
                let items = (menu.items.into_iter())
                    .filter_map(|item| match item {
                        OwnedMenuItem::Action { name, .. } => Some(name.to_string()),
                        OwnedMenuItem::SystemMenu(system) => Some(system.name.to_string()),
                        _ => None,
                    })
                    .collect();
                (menu.name.to_string(), items)
            })
            .collect()
    }

    /// Whether the drawn bar shows `selector` (`menu:<title>` or
    /// `menu-item:<title>`).
    fn drawn(selector: &str, cx: &mut VisualTestContext) -> bool {
        cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
            .is_some()
    }

    #[gpui::test]
    fn bundled_titles_follow_the_locale(cx: &mut TestAppContext) {
        let (bar, diagnostics) = MenuBar::parse(Path::new("menus.toml"), MENUS, None);
        assert_eq!(diagnostics, []);
        let bar = bar.unwrap();
        // Parsed in en-US: each keyed title is its en-US text.
        let items = &bar.menus[0].items;
        let ItemKind::Command(open) = &items[0].kind else {
            panic!("{items:?}")
        };
        assert_eq!(
            (open.title.as_str(), open.title_key.as_deref()),
            ("Open Menu Bar", Some("command-menu_bar-open"))
        );
        let ItemKind::System(services) = &items[2].kind else {
            panic!("{items:?}")
        };
        assert_eq!(
            (services.title.as_str(), services.title_key.as_deref()),
            ("Services", Some("kiln-core-menu-services"))
        );

        cx.update(|cx| {
            commands::init(cx, "Probe");
            keys::init(cx, Keymap::default());
            menus::install(cx, bar);
        });
        let (view, cx) = cx.add_window_view(|_, cx| MenuBarView::new(cx));
        view.update_in(cx, |view, window, cx| view.open(0, false, window, cx));
        cx.run_until_parked();
        let en_us = (
            "Probe".to_owned(),
            vec!["Open Menu Bar".to_owned(), "Services".to_owned()],
        );
        assert_eq!(gpui_titles(cx), std::slice::from_ref(&en_us));
        assert!(drawn("menu-item:Open Menu Bar", cx));
        assert!(drawn("menu-item:Services", cx));

        // The fake locale: GPUI's menus are built again, and the bar draws
        // the new titles; the table's own menu name stays as written.
        let fake: Vec<LanguageIdentifier> = vec!["qps-ploc".parse().unwrap()];
        cx.update(|_, cx| set_locale(cx, &fake));
        cx.run_until_parked();
        let titles = gpui_titles(cx);
        let drawn_fake = [
            drawn("menu:Probe", cx),
            drawn("menu-item:⟦Ópén Ménú Bár⟧", cx),
            drawn("menu-item:⟦Sérvícés⟧", cx),
            drawn("menu-item:Open Menu Bar", cx),
            drawn("menu-item:Services", cx),
        ];

        // Back to en-US: nothing kept a stale title.
        cx.update(|_, cx| set_locale(cx, &[]));
        cx.run_until_parked();
        let titles_after = gpui_titles(cx);
        let drawn_after = [
            drawn("menu-item:Open Menu Bar", cx),
            drawn("menu-item:Services", cx),
        ];

        assert_eq!(
            titles,
            [(
                "Probe".to_owned(),
                vec!["⟦Ópén Ménú Bár⟧".to_owned(), "⟦Sérvícés⟧".to_owned()]
            )]
        );
        assert_eq!(drawn_fake, [true, true, true, false, false]);
        assert_eq!(titles_after, [en_us]);
        assert_eq!(drawn_after, [true, true]);
    }
}
