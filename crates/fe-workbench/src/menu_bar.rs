//! The menu bar drawn in the window, where the platform has no native one
//! (Linux): the menus [`fe_gpui::menu::install`] set, as a row of names
//! above the panes, each opening a panel of its items with their
//! shortcuts. The design is `docs/decisions/linux-menu-bar.md`.
//!
//! Opening a menu takes focus, so its keys reach it (Left and Right between
//! menus, Up and Down between items, Enter, Escape), and closing gives
//! focus back to what had it. A chosen item runs after that, in that
//! element's context, as a native menu's does (`fe_gpui::menu::run`).
//! `menu_bar.open`, F10 by default, opens the first menu.

use std::rc::Rc;

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use fe_keymap::{ItemKind, MenuBar};
use fe_ui::{MenuEntry, MenuPanel, MenuStrip, UiTheme};
use gpui::{
    App, Context, DispatchPhase, FocusHandle, Focusable, Global, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseDownEvent, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Window, canvas, div,
};

/// The key context of the bar, while a menu is open.
pub(crate) const KEY_CONTEXT: &str = "menu_bar";

/// The command that opens the first menu.
const OPEN: &str = "menu_bar.open";

pub(crate) struct MenuBarCommands;

impl CommandTarget for MenuBarCommands {
    type Tx = ();
    type Target<'a> = bool;
}

struct Commands(Rc<Registry<MenuBarCommands>>);

impl Global for Commands {}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<MenuBarCommands> {
    let mut registry = Registry::new();
    registry
        .register(OPEN, "Open Menu Bar", |opened: &mut bool, _: &Actor| {
            *opened = true;
            Ok(Dispatched::new(Effect::Notify))
        })
        .expect("the menu bar command ID is valid")
        .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    registry
}

/// Whether `run` is `menu_bar.open`, in a context where it applies.
pub(crate) fn opens(run: &RunCommand, cx: &App) -> bool {
    let Some(commands) = cx.try_global::<Commands>() else {
        return false;
    };
    let registry = commands.0.clone();
    if registry.get(&run.0.command).is_none() {
        return false;
    }
    let mut opened = false;
    if let Err(error) = registry.dispatch(&mut opened, &run.0) {
        log::warn!("{error}");
    }
    opened
}

/// The open menu, and the item highlighted in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Open {
    menu: usize,
    /// An index into the menu's items; never a separator's.
    item: Option<usize>,
}

pub struct MenuBarView {
    open: Option<Open>,
    focus: FocusHandle,
    /// What had focus before a menu opened, given it back when it closes.
    restore: Option<FocusHandle>,
    /// Closes the menu when focus leaves the bar, as a click elsewhere
    /// moves it.
    _focus_out: Option<Subscription>,
    ui: UiTheme,
}

impl MenuBarView {
    pub(crate) fn new(ui: UiTheme, cx: &mut Context<Self>) -> Self {
        Self {
            open: None,
            focus: cx.focus_handle(),
            restore: None,
            _focus_out: None,
            ui,
        }
    }

    /// The open menu's index, and its highlighted item's, if any.
    pub fn open_menu(&self) -> Option<(usize, Option<usize>)> {
        self.open.map(|open| (open.menu, open.item))
    }

    fn bar(cx: &App) -> Option<MenuBar> {
        fe_gpui::menu::installed(cx).cloned()
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
        self.open = Some(Open { menu, item });
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
        let Some((name, id)) = bar.menus.get(menu).and_then(|m| {
            let id = m.items.get(item)?.command()?.id.clone();
            Some((m.name.clone(), id))
        }) else {
            return;
        };
        self.close(window, cx);
        window.defer(cx, move |window, cx| {
            fe_gpui::menu::run(&name, &id, window, cx);
        });
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(open), Some(bar)) = (self.open, Self::bar(cx)) else {
            return;
        };
        if event.keystroke.modifiers.modified() {
            return;
        }
        let menus = bar.menus.len();
        match event.keystroke.key.as_str() {
            "escape" => self.close(window, cx),
            "left" => self.open((open.menu + menus - 1) % menus, true, window, cx),
            "right" => self.open((open.menu + 1) % menus, true, window, cx),
            step @ ("up" | "down") => {
                let step = if step == "up" { -1 } else { 1 };
                self.open = Some(Open {
                    item: next_item(&bar, open.menu, open.item, step),
                    ..open
                });
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = self.ui;
        let bar = Self::bar(cx).unwrap_or_default();
        let names = bar
            .menus
            .iter()
            .map(|menu| SharedString::from(menu.name.clone()));
        let entity = cx.entity();
        let mut strip = MenuStrip::new(names, &ui)
            .on_click({
                let entity = entity.clone();
                move |ix, window, cx| {
                    entity.update(cx, |bar, cx| match bar.open {
                        Some(open) if open.menu == ix => bar.close(window, cx),
                        _ => bar.open(ix, false, window, cx),
                    })
                }
            })
            .on_hover({
                let entity = entity.clone();
                move |ix, window, cx| {
                    entity.update(cx, |bar, cx| {
                        if bar.open.is_some_and(|open| open.menu != ix) {
                            bar.open(ix, false, window, cx);
                        }
                    })
                }
            });
        if let Some(open) = self.open
            && let Some(menu) = bar.menus.get(open.menu)
        {
            let keymap = fe_gpui::current_keymap(cx);
            let entries = menu
                .items
                .iter()
                .map(|item| match &item.kind {
                    ItemKind::Command(command) => MenuEntry::Item {
                        title: command.title.clone().into(),
                        shortcut: command
                            .shortcut(&keymap)
                            .map(|keys| fe_keymap::shortcut_label(&keys).into()),
                    },
                    _ => MenuEntry::Separator,
                })
                .collect();
            let menu_ix = open.menu;
            let panel = MenuPanel::new(entries, &ui)
                .highlighted(open.item)
                .on_click({
                    let entity = entity.clone();
                    move |item, window, cx| {
                        entity.update(cx, |bar, cx| bar.choose(menu_ix, item, window, cx))
                    }
                })
                .on_hover(move |item, _, cx| {
                    entity.update(cx, |bar, cx| {
                        if let Some(open) = &mut bar.open
                            && open.item != Some(item)
                        {
                            open.item = Some(item);
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
            .flex_none()
            .w_full()
            .bg(ui.panel_background)
            .border_b_1()
            .border_color(ui.border)
            .children(closer)
            .child(strip)
    }
}
