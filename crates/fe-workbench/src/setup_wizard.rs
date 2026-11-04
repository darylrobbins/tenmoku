//! The first-run setup wizard's sheet, over the model in
//! [`crate::setup_wizard_model`]. It's a sheet in the window, not a tab or a
//! palette: kiln's modal layer gives up its modal when focus leaves or the
//! window deactivates, and a window of its own can fall behind the workbench.
//! [`Workbench::show_setup`] puts it up, and nothing else does yet.
//!
//! - A choice is written at once, off the main thread and after the one
//!   before, so the config watcher's reload applies it to the whole window,
//!   this sheet included. One that fails says so in a toast, and the control
//!   goes back to what the file has.
//! - Skip setup keeps what's been chosen; it and Start Editing close the sheet
//!   and record how it ended, once the writes are done.
//! - One focus handle, on the card: its key context holds every key from the
//!   keymap, and [`SetupWizard::on_key`] reads them. Tab stops are
//!   [`Stop`]s, of which the one current shows a ring and, for VoiceOver, is
//!   the active descendant.
//! - The Appearance and Font pages draw their choices with
//!   [`crate::setup_wizard_preview`]: tiles in each theme's own colours, and a
//!   code sample that follows the theme and font as the choices reload.

use std::{collections::HashMap, io, path::PathBuf};

use fe_command::{ContextFrame, ContextStack, Invocation, Source, Value};
use fe_config::first_run::{self, Outcome};
use fe_ui::{ActiveTheme as _, Button, Switch, Tooltip};
use gpui::{
    AnyElement, App, AppContext as _, Context, FocusHandle, Global, InteractiveElement as _,
    IntoElement, KeyContext, KeyDownEvent, ParentElement as _, Pixels, Render, Role, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, Toggled, WeakEntity,
    WeakFocusHandle, Window, div, point, prelude::FluentBuilder as _, px,
};
use kiln::{
    core::theme::Appearance,
    platform::{
        command_actions::dispatch_invocation,
        keys::{self, HOLDS_ALL_KEYS},
    },
    prelude::AppExt as _,
};

use crate::{
    KEY_CONTEXT as WORKBENCH_CONTEXT, Workbench,
    setup_wizard_model::{Choices, Control, ControlId, Key, Step, steps},
    setup_wizard_preview::{
        Sample, Swatch, Swatches, TILE_WIDTH, TILES_WIDTH, TileArt, installed_font_families,
        label_family,
    },
    workbench::toast::{self, Toast},
};

/// The key context of the sheet's focus, with [`HOLDS_ALL_KEYS`].
pub(crate) const KEY_CONTEXT: &str = "setup_wizard";

/// The card's size, unless the window is too small for it.
const CARD_WIDTH: Pixels = px(720.);
const CARD_HEIGHT: Pixels = px(560.);

/// What's left of the window around a card too big for it.
const CARD_MARGIN: Pixels = px(48.);

/// How tall the list of fonts is.
const LIST_HEIGHT: Pixels = px(104.);

const SKIP: &str = "Skip setup";
const SKIP_TOOLTIP: &str = "Keeps what you've chosen so far";
const BACK: &str = "Back";
const CONTINUE: &str = "Continue";
const FINISH: &str = "Start Editing";

/// What's around a tile's art across its width: the focus ring's padding and
/// border and the 2 px chosen ring, on both sides.
const TILE_FRAME: Pixels = px(8.);

/// What VoiceOver calls the code sample.
const SAMPLE_LABEL: &str = "Code sample";

/// The least the sample shows, in a window too small for the rest.
const SAMPLE_MIN_HEIGHT: Pixels = px(120.);

/// The Mode control's options that show one theme, in the model's order, and
/// then Match System's.
const MODE_DARK: usize = 0;
const MODE_LIGHT: usize = 1;
const MODE_SYSTEM: usize = 2;

/// The Ready page's hints: what to call a command, and which it is.
const SETTINGS: (&str, &str) = ("Settings", "workbench.open_settings_ui");
const HINTS: [(&str, &str); 3] = [
    ("Command Palette", "command_palette.toggle"),
    ("Go to File", "quick_open.toggle"),
    ("Open Folder", "workbench.open_folder"),
];

/// The files the sheet writes, when not fe-config's: under test, none unless
/// set, so no test touches the user's.
#[derive(Clone, Default)]
pub(crate) struct SetupPaths {
    pub(crate) settings: Option<PathBuf>,
    pub(crate) state: Option<PathBuf>,
}

impl Global for SetupPaths {}

fn setup_paths(cx: &App) -> SetupPaths {
    match cx.try_global::<SetupPaths>() {
        Some(paths) => paths.clone(),
        None if cfg!(test) => SetupPaths::default(),
        None => SetupPaths {
            settings: fe_config::user_settings_path(),
            state: fe_config::paths::state_dir(),
        },
    }
}

/// Where Tab can rest on a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    Control(ControlId),
    Skip,
    Back,
    /// Continue, or on the last page Start Editing.
    Primary,
}

/// How a control shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Segmented,
    List,
    Tiles,
    Switch,
    Stepper,
}

fn shape(id: ControlId) -> Shape {
    match id {
        ControlId::ThemeMode | ControlId::DarkTheme | ControlId::LightTheme => Shape::Tiles,
        ControlId::Keyboard
        | ControlId::VimPreset
        | ControlId::LineNumbers
        | ControlId::AutoSave
        | ControlId::InlineDiagnostics => Shape::Segmented,
        ControlId::FontFamily => Shape::List,
        ControlId::FontSize => Shape::Stepper,
        ControlId::FontLigatures
        | ControlId::FormatOnSave
        | ControlId::Minimap
        | ControlId::InlayHints
        | ControlId::SoftWrap => Shape::Switch,
    }
}

/// The element ID of a control's part: unique across the sheet.
fn part(id: ControlId, name: &'static str) -> gpui::ElementId {
    (name, id as u64).into()
}

/// The sheet: see the module docs.
pub(crate) struct SetupWizard {
    workbench: WeakEntity<Workbench>,
    focus: FocusHandle,
    /// What had focus before the sheet, for it to go back to.
    previous: Option<WeakFocusHandle>,
    /// The index of the page showing, in [`steps`].
    page: usize,
    stop: Stop,
    choices: Choices,
    /// What the file holds, as far as the writes have told: what a key goes
    /// back to when a write fails.
    saved: Choices,
    /// The number of the last write of each key: a write that failed doesn't
    /// undo a later one to its key.
    latest: HashMap<Key, u64>,
    next_write: u64,
    /// The number of the last write that's over: they end in order.
    finished: u64,
    /// The last of the writes, which the next one waits for.
    writes: Option<Task<()>>,
    /// The page's body, or on Appearance the column of tiles beside the sample.
    body_scroll: ScrollHandle,
    font_scroll: ScrollHandle,
    swatches: Swatches,
    /// Once the workbench that's opening the sheet can be read for its pane.
    sample: Option<Sample>,
    /// Every (role, label) passed in the last render.
    #[cfg(test)]
    nodes: std::cell::RefCell<Vec<(Role, String)>>,
    /// Every tile passed in the last render, by its label.
    #[cfg(test)]
    tiles: std::cell::RefCell<Vec<(String, TileArt)>>,
    /// Holds focus here: anything else that takes it, an open from another
    /// process, say, would have keys typed under the sheet.
    _focus_out: gpui::Subscription,
}

impl SetupWizard {
    pub(crate) fn new(
        workbench: WeakEntity<Workbench>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let fonts = installed_font_families(cx);
        let settings = cx.settings::<fe_config::Settings>();
        let choices = Choices::from_settings(&settings, &fonts);
        let focus = cx.focus_handle();
        let focus_out = cx.on_focus_out(&focus, window, |this, _, window, cx| {
            // A prompt in the window has its own keys to read.
            if !window.has_active_prompt() {
                window.focus(&this.focus, cx);
            }
        });
        // A window closed under the sheet takes no choice with it: dropped,
        // the task would cancel the writes still waiting.
        cx.on_release(|this: &mut Self, _| {
            if let Some(writes) = this.writes.take() {
                writes.detach();
            }
        })
        .detach();
        // The workbench is opening the sheet, so it can't be read yet.
        cx.defer_in(window, |this, _, cx| {
            let pane = (this.workbench.upgrade()).map(|bench| bench.read(cx).pane().clone());
            this.sample = pane.map(|pane| Sample::new(&pane, cx));
            cx.notify();
        });
        let mut this = Self {
            workbench,
            previous: window.focused(cx).map(|focus| focus.downgrade()),
            focus,
            page: 0,
            stop: Stop::Primary,
            saved: choices.clone(),
            choices,
            latest: HashMap::new(),
            next_write: 0,
            finished: 0,
            writes: None,
            body_scroll: ScrollHandle::new(),
            font_scroll: ScrollHandle::new(),
            swatches: Swatches::load(),
            sample: None,
            #[cfg(test)]
            nodes: Default::default(),
            #[cfg(test)]
            tiles: Default::default(),
            _focus_out: focus_out,
        };
        this.enter_page(0);
        this
    }

    pub(crate) fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    fn step(&self) -> Step {
        steps()[self.page]
    }

    fn last_page(&self) -> bool {
        self.page + 1 == steps().len()
    }

    fn controls(&self) -> Vec<Control> {
        self.step().controls(&self.choices)
    }

    /// The page's tab stops, in order.
    fn stops(&self) -> Vec<Stop> {
        let controls = self.controls().into_iter();
        let mut stops: Vec<Stop> = controls.map(|control| Stop::Control(control.id)).collect();
        stops.push(Stop::Skip);
        if self.page > 0 {
            stops.push(Stop::Back);
        }
        stops.push(Stop::Primary);
        stops
    }

    /// Shows page `page`, at its first control, or its primary button with
    /// none, and each list at its choice.
    fn enter_page(&mut self, page: usize) {
        self.page = page;
        let controls = self.controls();
        self.stop = (controls.first()).map_or(Stop::Primary, |control| Stop::Control(control.id));
        self.body_scroll.set_offset(point(px(0.), px(0.)));
        if let Some(font) = controls.iter().find(|c| shape(c.id) == Shape::List) {
            self.font_scroll.scroll_to_item(font.selected);
        }
    }

    /// Scrolls `id`'s control into view. On Font the body's first child holds
    /// all three.
    fn reveal(&self, id: ControlId) {
        let row = (self.controls().iter()).position(|control| control.id == id);
        if let Some(row) = row {
            let row = if self.step() == Step::Font { 0 } else { row };
            self.body_scroll.scroll_to_item(row);
        }
    }

    /// Moves `by` tab stops, around the ends, and into view.
    fn move_stop(&mut self, by: isize, cx: &mut Context<Self>) {
        let stops = self.stops();
        let at = stops
            .iter()
            .position(|stop| *stop == self.stop)
            .unwrap_or(0);
        let count = stops.len() as isize;
        self.stop = stops[(at as isize + by).rem_euclid(count) as usize];
        if let Stop::Control(id) = self.stop {
            self.reveal(id);
        }
        cx.notify();
    }

    /// An arrow key: the next or the previous of the focused control's
    /// options, around the ends in a group; a stepper stops at its ends.
    fn arrow(&mut self, by: isize, cx: &mut Context<Self>) {
        let Stop::Control(id) = self.stop else {
            return;
        };
        let Some(control) = self.controls().into_iter().find(|c| c.id == id) else {
            return;
        };
        let count = control.options.len() as isize;
        let next = control.selected as isize + by;
        let option = match shape(id) {
            Shape::Switch => return,
            Shape::Stepper if !(0..count).contains(&next) => return,
            Shape::Stepper | Shape::Segmented | Shape::List | Shape::Tiles => {
                next.rem_euclid(count) as usize
            }
        };
        self.choose(&control, option, cx);
        match shape(id) {
            Shape::List => self.font_scroll.scroll_to_item(option),
            Shape::Tiles => self.reveal(id),
            _ => {}
        }
    }

    /// Takes `option` of `control` and writes what that changes, after the
    /// writes before it. If that fails a toast says why, sheet or no sheet,
    /// and the control goes back to what the file has, unless a later write
    /// of it is waiting.
    ///
    /// A write takes with it every key still on its way to the file, as each
    /// choice is made against what the ones before it should have left there:
    /// if the write that retires `theme.name` fails, the next one still has to.
    fn choose(&mut self, control: &Control, option: usize, cx: &mut Context<Self>) {
        let mut keys: Vec<Key> = (self.choices.choose(control.id, option).iter())
            .map(|(key, _)| *key)
            .collect();
        cx.notify();
        if keys.is_empty() {
            return;
        }
        let mut waiting: Vec<Key> = (self.latest.iter())
            .filter(|(key, write)| **write > self.finished && !keys.contains(*key))
            .map(|(key, _)| *key)
            .collect();
        waiting.sort_unstable();
        keys.extend(waiting);
        self.next_write += 1;
        let write = self.next_write;
        self.latest.extend(keys.iter().map(|key| (*key, write)));
        let (chosen, previous) = (self.choices.clone(), self.writes.take());
        let (path, dotted) = (setup_paths(cx).settings, control.dotted_key());
        self.writes = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let (written, keys, chosen) = cx
                .background_spawn(async move {
                    let written = match path {
                        Some(path) => fe_config::write_settings(&path, &chosen.writes_of(&keys)),
                        None => Err(io::Error::other("there's no settings file to write to")),
                    };
                    (written, keys, chosen)
                })
                .await;
            let written = written.map_err(|error| {
                log::warn!("can't set {dotted}: {error}");
                cx.update(|cx| {
                    let message = format!("Can't change {dotted}");
                    toast::show(cx, Toast::error(message).detail(error.to_string()))
                });
            });
            this.update(cx, |this, cx| {
                this.written(write, &keys, &chosen, written.is_ok(), cx)
            })
            .ok();
        }));
    }

    /// Write number `write`, of `keys` as `chosen` has them, is over.
    fn written(
        &mut self,
        write: u64,
        keys: &[Key],
        chosen: &Choices,
        ok: bool,
        cx: &mut Context<Self>,
    ) {
        self.finished = write;
        if ok {
            self.saved.adopt(chosen, keys);
            return;
        }
        let stale: Vec<Key> = (keys.iter())
            .filter(|key| self.latest.get(*key) == Some(&write))
            .copied()
            .collect();
        self.choices.adopt(&self.saved, &stale);
        // A control a value's going back hid or showed.
        if !self.stops().contains(&self.stop) {
            self.stop = Stop::Primary;
        }
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.page > 0 {
            self.enter_page(self.page - 1);
            cx.notify();
        }
    }

    /// Continue, or on the last page Start Editing.
    fn primary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.last_page() {
            self.close(Outcome::Completed, window, cx);
        } else {
            self.enter_page(self.page + 1);
            cx.notify();
        }
    }

    /// Closes the sheet and records `outcome`, after the writes waiting, which
    /// go on without it.
    fn close(&mut self, outcome: Outcome, window: &mut Window, cx: &mut Context<Self>) {
        let (state, writes) = (setup_paths(cx).state, self.writes.take());
        cx.spawn(async move |_, cx| {
            if let Some(writes) = writes {
                writes.await;
            }
            let Some(state) = state else {
                return;
            };
            let recorded = cx.background_spawn(async move { first_run::record(&state, outcome) });
            if let Err(error) = recorded.await {
                log::warn!("can't record how setup ended: {error}");
            }
        })
        .detach();
        let previous = self.previous.clone();
        (self.workbench)
            .update(cx, |workbench, cx| {
                workbench.close_setup(previous, window, cx)
            })
            .ok();
    }

    /// What Space and Enter do on a button.
    fn press(&mut self, stop: Stop, window: &mut Window, cx: &mut Context<Self>) {
        match stop {
            Stop::Skip => self.close(Outcome::Skipped, window, cx),
            Stop::Back => self.back(cx),
            Stop::Primary => self.primary(window, cx),
            Stop::Control(_) => {}
        }
    }

    fn space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Stop::Control(id) = self.stop else {
            return self.press(self.stop, window, cx);
        };
        if shape(id) == Shape::Switch
            && let Some(control) = self.controls().into_iter().find(|c| c.id == id)
        {
            self.choose(&control, 1 - control.selected, cx);
        }
    }

    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.stop {
            Stop::Control(_) => self.primary(window, cx),
            stop => self.press(stop, window, cx),
        }
    }

    /// The sheet's keys. None goes further, so a key with the platform
    /// modifier reaches no native menu: only secondary-q does anything.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let key = keystroke.key.as_str();
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.function {
            let plain = !modifiers.shift && !modifiers.alt && !modifiers.function;
            if modifiers.secondary() && plain && key == "q" && !event.is_held {
                quit(window, cx);
            }
            return;
        }
        match (key, modifiers.shift) {
            ("tab", shift) => self.move_stop(if shift { -1 } else { 1 }, cx),
            ("left" | "up", false) => self.arrow(-1, cx),
            ("right" | "down", false) => self.arrow(1, cx),
            // Not held: a hold of Enter isn't a way through every page.
            ("space", false) if !event.is_held => self.space(window, cx),
            ("enter", false) if !event.is_held => self.enter(window, cx),
            ("escape", false) if !event.is_held => self.close(Outcome::Skipped, window, cx),
            _ => {}
        }
    }

    /// Clicking `stop`'s control moves the ring there.
    fn click_stop(&mut self, stop: Stop, cx: &mut Context<Self>) {
        self.stop = stop;
        cx.notify();
    }

    /// `element` with its role and VoiceOver label, noted for the tests.
    fn node<E: gpui::StatefulInteractiveElement>(
        &self,
        element: E,
        role: Role,
        label: impl Into<SharedString>,
    ) -> E {
        let label = label.into();
        #[cfg(test)]
        self.nodes.borrow_mut().push((role, label.to_string()));
        element.role(role).aria_label(label)
    }

    /// Text VoiceOver can read: a bare string has no node.
    fn label(&self, id: impl Into<gpui::ElementId>, text: impl Into<SharedString>) -> AnyElement {
        let text = text.into();
        self.node(div().id(id), Role::Label, text.clone())
            .child(text)
            .into_any_element()
    }

    /// Around what holds a tab stop: a ring when `focused`, else the card's
    /// colour, so nothing moves.
    fn ring(&self, focused: bool, child: impl IntoElement, cx: &App) -> gpui::Div {
        let colors = cx.theme().colors;
        div()
            .p(px(1.))
            .rounded_md()
            .border_1()
            .border_color(if focused {
                colors.state.focus_ring
            } else {
                colors.surface.raised
            })
            .child(child)
    }

    /// A control's title and one-line description.
    fn heading(&self, control: &Control, cx: &App) -> gpui::Div {
        let colors = cx.theme().colors;
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(control.title),
            )
            .child(
                div()
                    .text_color(colors.text.muted)
                    .child(control.description),
            )
    }

    /// One option of a radio group, a button that writes when clicked.
    fn radio(
        &self,
        control: &Control,
        option: usize,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let colors = cx.theme().colors;
        let selected = option == control.selected;
        let label = control.options[option].label.clone();
        let id = control.id;
        let element_id =
            gpui::ElementId::NamedInteger(format!("setup-radio-{id:?}").into(), option as u64);
        let button = Button::new(element_id, label.clone())
            .debug_selector(move || format!("setup-radio-{id:?}-{option}"))
            .when(selected, |button| {
                button
                    .bg(colors.accent.subtle)
                    .border_color(colors.border.focused)
            })
            .when(selected && focused, |button| {
                button.aria_active_descendant()
            })
            .aria_selected(selected)
            .aria_position_in_set(option + 1)
            .aria_size_of_set(control.options.len())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.click_stop(Stop::Control(id), cx);
                if let Some(control) = this.controls().into_iter().find(|c| c.id == id) {
                    this.choose(&control, option, cx);
                }
            }));
        self.node(button, Role::RadioButton, label)
    }

    /// The swatch of the theme chosen for each of the dark and the light slot,
    /// shown or not: the model lists them both for Match System.
    fn chosen_swatches(&self, cx: &App) -> (Swatch, Swatch) {
        let mut both = self.choices.clone();
        let _ = both.choose(ControlId::ThemeMode, MODE_SYSTEM);
        let swatch = |id: ControlId, appearance| {
            let controls = Step::Appearance.controls(&both);
            let control = controls.iter().find(|control| control.id == id);
            let label = control.map_or("", |c| c.options[c.selected].label.as_str());
            self.swatches.of_option(label, appearance, cx)
        };
        (
            swatch(ControlId::DarkTheme, Appearance::Dark),
            swatch(ControlId::LightTheme, Appearance::Light),
        )
    }

    /// What the `option`th tile of `control` draws.
    fn tile_art(&self, control: &Control, option: usize, cx: &App) -> TileArt {
        let label = control.options[option].label.as_str();
        match control.id {
            ControlId::DarkTheme => {
                TileArt::Window(self.swatches.of_option(label, Appearance::Dark, cx))
            }
            ControlId::LightTheme => {
                TileArt::Window(self.swatches.of_option(label, Appearance::Light, cx))
            }
            _ => {
                let (dark, light) = self.chosen_swatches(cx);
                match option {
                    MODE_DARK => TileArt::Window(dark),
                    MODE_LIGHT => TileArt::Window(light),
                    _ => TileArt::Split { dark, light },
                }
            }
        }
    }

    /// One option of a group of tiles: its art in the card's frame, its title
    /// under it. A 2 px ring is on the chosen one and, around that, the focus
    /// ring while the group has the keyboard; the frames are the card's colour
    /// otherwise, so nothing moves.
    fn tile(
        &self,
        control: &Control,
        option: usize,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let id = control.id;
        let selected = option == control.selected;
        let label = control.options[option].label.clone();
        let art = self.tile_art(control, option, cx);
        #[cfg(test)]
        self.tiles.borrow_mut().push((label.clone(), art));
        // A hairline around the others, in the same room, for a light tile on
        // a light card.
        let chosen = div()
            .rounded_md()
            .overflow_hidden()
            .when(selected, |frame| {
                frame.border_2().border_color(colors.border.focused)
            })
            .when(!selected, |frame| {
                frame
                    .m(px(1.))
                    .border_1()
                    .border_color(colors.border.default)
            })
            .child(art.render(TILE_WIDTH - TILE_FRAME));
        let element_id =
            gpui::ElementId::NamedInteger(format!("setup-tile-{id:?}").into(), option as u64);
        let tile = div()
            .id(element_id)
            .debug_selector(move || format!("setup-radio-{id:?}-{option}"))
            .flex()
            .flex_col()
            .w(TILE_WIDTH)
            .gap(px(2.))
            .cursor_pointer()
            .when(selected && focused, |tile| tile.aria_active_descendant())
            .aria_selected(selected)
            .aria_position_in_set(option + 1)
            .aria_size_of_set(control.options.len())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.click_stop(Stop::Control(id), cx);
                if let Some(control) = this.controls().into_iter().find(|c| c.id == id) {
                    this.choose(&control, option, cx);
                }
            }))
            .child(self.ring(focused && selected, chosen, cx))
            .child(
                div()
                    .text_size(px(12.))
                    .text_center()
                    .truncate()
                    .child(label.clone()),
            );
        self.node(tile, Role::RadioButton, label).into_any_element()
    }

    fn render_group(&self, control: &Control, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let options = 0..control.options.len();
        let group = self.node(
            div().id(part(control.id, "group")),
            Role::RadioGroup,
            control.title,
        );
        let group = group.aria_description(control.description);
        if shape(control.id) == Shape::Tiles {
            let tiles: Vec<AnyElement> = options
                .map(|option| self.tile(control, option, focused, cx))
                .collect();
            return div()
                .flex()
                .flex_col()
                .flex_none()
                .gap_1()
                .child(self.heading(control, cx))
                .child(group.flex().flex_wrap().gap_x_2().gap_y_1().children(tiles))
                .into_any_element();
        }
        let body = match shape(control.id) {
            Shape::List => {
                let rows = options.map(|option| {
                    let family = label_family(&control.options[option].label);
                    let radio = self.radio(control, option, focused, cx);
                    let radio = radio.ghost().w_full().justify_start();
                    radio.font_family(family).into_any_element()
                });
                let rows: Vec<AnyElement> = rows.collect();
                group
                    .h(LIST_HEIGHT)
                    .overflow_y_scroll()
                    .track_scroll(&self.font_scroll)
                    .flex()
                    .flex_col()
                    .children(rows)
            }
            _ => {
                let buttons: Vec<AnyElement> = options
                    .map(|option| {
                        let radio = self.radio(control, option, focused, cx);
                        radio.into_any_element()
                    })
                    .collect();
                group.flex().flex_wrap().gap_1().children(buttons)
            }
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(self.heading(control, cx))
            .child(self.ring(focused, body, cx))
            .into_any_element()
    }

    fn render_switch(
        &self,
        control: &Control,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = control.id;
        let on = control.selected == 1;
        let switch = Switch::new(part(id, "switch"))
            .debug_selector(move || format!("setup-switch-{id:?}"))
            .on(on)
            .aria_toggled(if on { Toggled::True } else { Toggled::False })
            .aria_description(control.description)
            .when(focused, |switch| switch.aria_active_descendant())
            .on_change(cx.listener(move |this, on: &bool, _, cx| {
                this.click_stop(Stop::Control(id), cx);
                if let Some(control) = this.controls().into_iter().find(|c| c.id == id) {
                    this.choose(&control, usize::from(*on), cx);
                }
            }));
        let switch = self.node(switch, Role::Switch, control.title);
        let row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(self.heading(control, cx))
            .child(switch);
        self.ring(focused, row, cx).into_any_element()
    }

    fn render_stepper(
        &self,
        control: &Control,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = control.id;
        let last = control.options.len() - 1;
        let step = |name: &'static str, text: &'static str, by: isize, cx: &mut Context<Self>| {
            let label = format!("{} {}", text, control.title.to_lowercase());
            let shown = if by < 0 { "\u{2212}" } else { "+" };
            let enabled = match by < 0 {
                true => control.selected > 0,
                false => control.selected < last,
            };
            let button = Button::new(part(id, name), shown)
                .debug_selector(move || format!("setup-{name}-{id:?}"))
                .disabled(!enabled)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.click_stop(Stop::Control(id), cx);
                    this.arrow(by, cx);
                }));
            self.node(button, Role::Button, label)
        };
        let down = step("step-down", "Decrease", -1, cx);
        let up = step("step-up", "Increase", 1, cx);
        let value = control.options[control.selected].label.clone();
        let (min, max) = (
            control.options[0].label.parse().unwrap_or(0.),
            control.options[last].label.parse().unwrap_or(0.),
        );
        let stepper = self
            .node(
                div().id(part(id, "stepper")),
                Role::SpinButton,
                control.title,
            )
            .aria_value(value.clone())
            .aria_numeric_value(value.parse().unwrap_or(0.))
            .aria_min_numeric_value(min)
            .aria_max_numeric_value(max)
            .aria_description(control.description)
            .when(focused, |stepper| stepper.aria_active_descendant())
            .flex()
            .items_center()
            .gap_2()
            .child(down)
            .child(div().w(px(24.)).flex().justify_center().child(value))
            .child(up);
        let row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(self.heading(control, cx))
            .child(stepper);
        self.ring(focused, row, cx).into_any_element()
    }

    /// The code sample, for VoiceOver a picture: it can't be focused or read
    /// a line at a time.
    fn render_sample(&self, cx: &App) -> gpui::Stateful<gpui::Div> {
        let node = self.node(div().id("setup-sample"), Role::Figure, SAMPLE_LABEL);
        node.debug_selector(|| "setup-sample".into())
            .children(self.sample.as_ref().map(|sample| sample.render(cx)))
    }

    fn render_control(&self, control: &Control, cx: &mut Context<Self>) -> AnyElement {
        let focused = self.stop == Stop::Control(control.id);
        match shape(control.id) {
            Shape::Segmented | Shape::List | Shape::Tiles => {
                self.render_group(control, focused, cx)
            }
            Shape::Switch => self.render_switch(control, focused, cx),
            Shape::Stepper => self.render_stepper(control, focused, cx),
        }
    }

    /// The Appearance page: the tiles, which scroll if they must, beside the
    /// sample, which doesn't.
    fn render_appearance(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let groups: Vec<AnyElement> = (self.controls().iter())
            .map(|control| self.render_control(control, cx))
            .collect();
        div()
            .id("setup-body")
            .flex()
            .flex_1()
            .min_h_0()
            .gap_4()
            .child(
                div()
                    .id("setup-tiles")
                    .flex()
                    .flex_col()
                    .flex_none()
                    .w(TILES_WIDTH)
                    .gap_3()
                    .overflow_y_scroll()
                    .track_scroll(&self.body_scroll)
                    .children(groups),
            )
            .child(self.render_sample(cx).flex_1().min_w_0())
    }

    /// The Font page: the family beside the size and ligatures, and the sample
    /// under them with what's left.
    fn render_font(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let mut family = Vec::new();
        let mut rest = Vec::new();
        for control in self.controls() {
            let element = self.render_control(&control, cx);
            match shape(control.id) {
                Shape::List => family.push(element),
                _ => rest.push(element),
            }
        }
        let column = || div().flex().flex_col().flex_1().min_w_0().gap_3();
        div()
            .id("setup-body")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_3()
            .overflow_y_scroll()
            .track_scroll(&self.body_scroll)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap_4()
                    .child(column().children(family))
                    .child(column().children(rest)),
            )
            .child(self.render_sample(cx).flex_1().min_h(SAMPLE_MIN_HEIGHT))
    }

    /// Every other page: its controls, one under the other.
    fn render_plain(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let body: Vec<AnyElement> = match self.step() {
            Step::Ready => self.render_ready(cx),
            _ => (self.controls().iter())
                .map(|control| self.render_control(control, cx))
                .collect(),
        };
        div()
            .id("setup-body")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_3()
            .overflow_y_scroll()
            .track_scroll(&self.body_scroll)
            .children(body)
    }

    /// The Ready page: what the run changed, where to change it, and three
    /// hints with the keymap's keys.
    fn render_ready(&self, cx: &App) -> Vec<AnyElement> {
        let changed = match self.choices.changed_count() {
            0 => "Tenmoku's defaults are set".to_owned(),
            1 => "You changed 1 setting".to_owned(),
            count => format!("You changed {count} settings"),
        };
        let settings = match shortcut(SETTINGS.1, cx) {
            Some(keys) => format!("Change any of them in {} ({keys})", SETTINGS.0),
            None => format!("Change any of them in {}", SETTINGS.0),
        };
        let hints = HINTS.iter().enumerate().map(|(ix, (name, command))| {
            let text = match shortcut(command, cx) {
                Some(keys) => format!("{name} ({keys})"),
                None => (*name).to_owned(),
            };
            self.label(("setup-hint", ix as u64), text)
        });
        let colors = cx.theme().colors;
        let lead = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(self.label("setup-changed", changed))
            .child(
                div()
                    .text_color(colors.text.muted)
                    .child(self.label("setup-settings", settings)),
            );
        let hints = div().flex().flex_col().gap_1().children(hints);
        vec![lead.into_any_element(), hints.into_any_element()]
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> gpui::Div {
        let colors = cx.theme().colors;
        let ui = fe_ui::ui_theme(cx);
        let stop = self.stop;
        let skip = Button::new("setup-skip", SKIP)
            .ghost()
            .debug_selector(|| "setup-skip".into())
            .when(stop == Stop::Skip, |button| button.aria_active_descendant())
            .tooltip(move |_, cx| {
                cx.new(|_| Tooltip::new(SKIP_TOOLTIP, None::<&str>, &ui))
                    .into()
            })
            .on_click(cx.listener(|this, _, window, cx| this.close(Outcome::Skipped, window, cx)));
        let skip = self.node(skip, Role::Button, SKIP);
        let dots = (0..steps().len()).map(|page| {
            div().size(px(6.)).rounded_full().bg(if page == self.page {
                colors.accent.base
            } else {
                colors.border.default
            })
        });
        let back = (self.page > 0).then(|| {
            let back = Button::new("setup-back", BACK)
                .debug_selector(|| "setup-back".into())
                .large()
                .when(stop == Stop::Back, |button| button.aria_active_descendant())
                .on_click(cx.listener(|this, _, _, cx| this.back(cx)));
            self.ring(stop == Stop::Back, self.node(back, Role::Button, BACK), cx)
        });
        let label = if self.last_page() { FINISH } else { CONTINUE };
        let primary = Button::new("setup-primary", label)
            .primary()
            .large()
            .debug_selector(|| "setup-primary".into())
            .when(stop == Stop::Primary, |button| {
                button.aria_active_descendant()
            })
            .on_click(cx.listener(|this, _, window, cx| this.primary(window, cx)));
        let primary = self.node(primary, Role::Button, label);
        div()
            .flex()
            .flex_none()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .child(self.ring(stop == Stop::Skip, skip, cx)),
            )
            .child(div().flex().flex_none().gap_2().children(dots))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .children(back)
                    .child(self.ring(stop == Stop::Primary, primary, cx)),
            )
    }

    /// The label the card has on this page.
    fn dialog_label(&self) -> String {
        let (page, count) = (self.page + 1, steps().len());
        let title = self.step().title();
        format!("Set Up Tenmoku, step {page} of {count}, {title}")
    }

    /// Every (role, label) the last render passed.
    #[cfg(test)]
    pub(crate) fn nodes(&self) -> Vec<(Role, String)> {
        self.nodes.borrow().clone()
    }

    /// Every tile the last render passed, with its label.
    #[cfg(test)]
    pub(crate) fn tiles(&self) -> Vec<(String, TileArt)> {
        self.tiles.borrow().clone()
    }

    /// The code sample's editor, once it's there.
    #[cfg(test)]
    pub(crate) fn sample_editor(&self) -> Option<&gpui::Entity<fe_gpui::EditorView>> {
        self.sample.as_ref().map(Sample::editor)
    }

    /// `id`'s control as it's listed now.
    #[cfg(test)]
    pub(crate) fn control(&self, id: ControlId) -> Option<Control> {
        self.controls().into_iter().find(|control| control.id == id)
    }

    /// Takes the option labelled `label` of `id`, as a click on it does.
    #[cfg(test)]
    pub(crate) fn pick(&mut self, id: ControlId, label: &str, cx: &mut Context<Self>) {
        let control = self.control(id).expect("the control is listed");
        let option = (control.options.iter()).position(|option| option.label == label);
        self.choose(&control, option.expect("the option is listed"), cx);
    }

    #[cfg(test)]
    pub(crate) fn current_step(&self) -> Step {
        self.step()
    }

    #[cfg(test)]
    pub(crate) fn current_stop(&self) -> Stop {
        self.stop
    }
}

/// The label of what the active keymap runs `command` with where the
/// workbench's editor has focus, the usual place for it.
fn shortcut(command: &str, cx: &App) -> Option<String> {
    let command = fe_command::CommandId::parse(command).ok()?;
    let stack: ContextStack = [WORKBENCH_CONTEXT, fe_gpui::KEY_CONTEXT]
        .into_iter()
        .map(ContextFrame::new)
        .collect();
    let keymap = keys::current_keymap(cx);
    let found = fe_keymap::shortcuts(&keymap, &command, &Value::Null, &stack);
    found.first().map(fe_keymap::shortcut_label)
}

/// Runs `app.quit`, as secondary-q would with no sheet.
fn quit(window: &mut Window, cx: &mut App) {
    let stack = keys::context_stack(&window.context_stack(), cx);
    let Ok(invocation) = Invocation::of("app.quit") else {
        return;
    };
    let invocation = invocation.source(Source::Keyboard).context(stack);
    if let Err(error) = dispatch_invocation(invocation, window, cx) {
        log::warn!("{error}");
    }
}

impl Render for SetupWizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        self.nodes.borrow_mut().clear();
        #[cfg(test)]
        self.tiles.borrow_mut().clear();
        let colors = cx.theme().colors;
        let viewport = window.viewport_size();
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        context.add(HOLDS_ALL_KEYS);
        let step = self.step();
        let body = match step {
            Step::Appearance => self.render_appearance(cx),
            Step::Font => self.render_font(cx),
            _ => self.render_plain(cx),
        };
        let header = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap_1()
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(step.title()),
            )
            .child(
                div()
                    .text_color(colors.text.muted)
                    .child(self.label("setup-subtitle", step.subtitle())),
            );
        let footer = self.render_footer(cx);
        let card = self
            .node(div().id("setup-card"), Role::Dialog, self.dialog_label())
            .debug_selector(|| "setup-card".into())
            .key_context(context)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .w(CARD_WIDTH.min(viewport.width - CARD_MARGIN))
            .h(CARD_HEIGHT.min(viewport.height - CARD_MARGIN))
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .bg(colors.surface.raised)
            .border_1()
            .border_color(colors.border.default)
            .rounded_md()
            .shadow_lg()
            .text_color(colors.text.primary)
            .child(header)
            .child(body)
            .child(footer);
        div()
            .id("setup-scrim")
            .debug_selector(|| "setup-scrim".into())
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .bg(colors.surface.overlay)
            .child(card)
    }
}
