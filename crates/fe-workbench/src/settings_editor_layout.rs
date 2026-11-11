//! How the Settings tab lays its list out (`docs/design/design-system.md`,
//! "Settings"):
//!
//! - The rows sit in a column at most [`COLUMN_WIDTH`] wide, centred in
//!   the list with [`GUTTER`] either side; a row's hover and selected
//!   shade and its hairline under it are inside it.
//! - A group's heading is a short row, its title (section heading, 15/20
//!   600) and the schema's description of it, not a setting's height: the
//!   list is a gpui `list`, its rows as tall as they draw. With nothing
//!   searched for, the Common section comes first: under its heading, the
//!   theme cards, Yohen, Hakuji and Match system, each drawn in its theme's
//!   own colours, which set `theme.mode`; then its settings
//!   ([`COMMON`](super::COMMON)).
//! - A setting's row has on the left its label (UI body), its key in
//!   settings.toml (code small, faint), and under them one line of help
//!   (UI small, muted: fe-config's catalogue's message for the key) and
//!   anything wrong; its control, 26 px tall, on the right; where the
//!   column has no room for both, the control goes under. A list editor
//!   always goes under, taking the column's width. A setting the selected
//!   scope's file sets has an accent dot before its label and Reset after
//!   its key.
//! - Beside the list, the section index lists the top-level groups the
//!   search finds settings in, the Common section first, with how many,
//!   and an accent dot on each with a setting the selected scope's file
//!   sets: a click scrolls one's heading to the top, and the one in view is
//!   highlighted. A tab narrower than [`NAV_MIN_TAB_WIDTH`] leaves it out.

use fe_command::Value as Json;
use fe_config::{SettingEntry, SettingsModel, ThemeModeSetting, themes};
use fe_ui::{
    ActiveTheme as _, FamilySlot, FontFamilies, SECTION_HEADING, TextRole, UI_BODY, UI_SMALL,
    height, ui_theme,
};
use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ListOffset, ParentElement as _,
    Pixels, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use kiln::core::theme::Appearance;
use kiln_l10n::tr;

use super::{
    LINE_HEIGHT, Row, SettingsEditor, THEME_DARK, THEME_LIGHT, THEME_MODE, THEME_NAME,
    controls::{Change, Value},
    selector, title,
};
use crate::setup_wizard_preview::{TILE_WIDTH, TileArt};

/// The widest the rows' column gets.
pub(super) const COLUMN_WIDTH: Pixels = px(760.);

/// The space either side of the column.
pub(super) const GUTTER: Pixels = px(24.);

/// The space between a row's shade's edge and its text.
const ROW_INSET: Pixels = px(12.);

const ROW_PADDING: Pixels = px(12.);

const ROW_GAP: Pixels = px(4.);

/// The narrowest a setting's title block gets beside its control: in a
/// narrower column the control goes under it.
const TITLE_WIDTH: Pixels = px(280.);

/// The space between a setting's title block and its control.
const CONTROL_GAP: Pixels = px(24.);

/// A setting's help line's style: UI small, one line.
pub(crate) const HELP_ROLE: TextRole = UI_SMALL;

/// A setting's label's style: UI body.
pub(crate) const LABEL_ROLE: TextRole = UI_BODY;

/// The height of the line under a group's title: its description.
const DESCRIPTION_LINE_HEIGHT: Pixels = px(16.);

/// The height of the line saying why a typed value wasn't written.
const MESSAGE_HEIGHT: Pixels = px(14.);

/// The tallest a group's heading is: its title line and its description's.
/// The Common section's is taller, the theme cards under it.
pub(super) const HEADING_HEIGHT: Pixels = px(46.);

/// The tallest the Common section's heading is, unmeasured: its title and
/// description, and the theme cards wrapped one to a line, each 74 px with
/// 12 between, under 12 px of padding.
pub(super) const COMMON_HEADING_HEIGHT: Pixels = px(46. + 12. + 3. * 74. + 2. * 12.);

/// The size of the accent dot that marks a changed setting or section.
pub(crate) const DOT: Pixels = px(6.);

/// The space under a group's last setting, before the next heading.
const SECTION_GAP: Pixels = px(16.);

/// The category navigation's width.
pub(super) const NAV_WIDTH: Pixels = px(180.);

/// The narrowest tab that shows the category navigation.
pub(super) const NAV_MIN_TAB_WIDTH: Pixels = px(700.);

/// A theme card of the Common section's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThemeCard {
    /// Dark, in Yohen.
    Yohen,
    /// Light, in Hakuji.
    Hakuji,
    /// The system's appearance, in Yohen or Hakuji.
    System,
}

/// The theme cards, in the order they show.
pub(crate) const THEME_CARDS: [ThemeCard; 3] =
    [ThemeCard::Yohen, ThemeCard::Hakuji, ThemeCard::System];

impl ThemeCard {
    fn mode(self) -> ThemeModeSetting {
        match self {
            ThemeCard::Yohen => ThemeModeSetting::Dark,
            ThemeCard::Hakuji => ThemeModeSetting::Light,
            ThemeCard::System => ThemeModeSetting::System,
        }
    }

    /// Its `theme.mode`, as settings.toml spells it: `dark`, `light` or
    /// `system`.
    pub(crate) fn mode_value(self) -> String {
        (serde_json::to_value(self.mode()).ok())
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    /// The appearances whose theme it shows as Tenmoku's own.
    fn appearances(self) -> &'static [Appearance] {
        match self {
            ThemeCard::Yohen => &[Appearance::Dark],
            ThemeCard::Hakuji => &[Appearance::Light],
            ThemeCard::System => &[Appearance::Dark, Appearance::Light],
        }
    }
}

/// A top-level group, as the category navigation lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Section {
    /// As in `Editor`.
    pub(crate) title: String,
    /// How many of its settings the search finds, its nested groups'
    /// included.
    pub(crate) count: usize,
    /// Its first row, its first heading's.
    pub(crate) row: usize,
    /// Its settings, by their index in [`SettingsModel::entries`]; for the
    /// Common section, `theme.mode`'s too, which its cards set.
    pub(crate) entries: Vec<usize>,
}

/// The sections `rows` show settings of, in order: the Common section, its
/// first `common` rows, then the top-level groups. The settings at the top
/// level, under no group, are in none.
pub(crate) fn sections(model: &SettingsModel, rows: &[Row], common: usize) -> Vec<Section> {
    let mut sections: Vec<(Option<String>, Section)> = Vec::new();
    if let Some(Row::Group(heading)) = rows.first().filter(|_| common > 0) {
        let settings: Vec<usize> = (rows[..common].iter())
            .filter_map(|row| match row {
                Row::Setting(entry) => Some(*entry),
                Row::Group(_) => None,
            })
            .collect();
        let mode = (model.entries().iter()).position(|entry| entry.key_path() == THEME_MODE);
        let section = Section {
            title: heading.clone(),
            count: settings.len(),
            row: 0,
            entries: settings.into_iter().chain(mode).collect(),
        };
        sections.push((None, section));
    }
    for (ix, row) in rows.iter().enumerate().skip(common) {
        let Row::Setting(entry) = row else {
            continue;
        };
        let key = &model.entries()[*entry].key;
        let [top, _, ..] = key.as_slice() else {
            continue;
        };
        let found = sections
            .iter_mut()
            .find(|(key, _)| key.as_ref() == Some(top));
        if let Some((_, section)) = found {
            section.count += 1;
            section.entries.push(*entry);
            continue;
        }
        let heading = ix
            .checked_sub(1)
            .filter(|ix| matches!(rows[*ix], Row::Group(_)));
        let group = (model.groups().iter()).find(|group| group.key == [top.clone()]);
        let section = Section {
            title: group.map_or_else(|| title(top), |group| group.title.clone()),
            count: 1,
            row: heading.unwrap_or(ix),
            entries: vec![*entry],
        };
        sections.push((Some(top.clone()), section));
    }
    sections.into_iter().map(|(_, section)| section).collect()
}

/// `text`, a schema description, as one paragraph a line: the doc
/// comments it comes from break its lines where the source wraps.
pub(crate) fn unwrap(text: &str) -> String {
    (text.split("\n\n"))
        .map(|paragraph| paragraph.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The catalogue key of the label of the setting at dotted `key`:
/// `workbench-settings-label-` and the key, dashes for dots.
pub(crate) fn label_key(key: &str) -> String {
    let mut label = String::from("workbench-settings-label-");
    label.push_str(&key.replace('.', "-"));
    label
}

/// `text`, a setting's description from the catalogue, as the one line of
/// help its row shows: its first paragraph, which the row cuts short if
/// it's too long.
pub(crate) fn help_line(text: &str) -> String {
    unwrap(text).lines().next().unwrap_or_default().to_owned()
}

impl SettingsEditor {
    /// Row `ix` of the list, in the column, with the space under it that
    /// ends a group.
    pub(super) fn render_row(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        #[cfg(test)]
        match self.built.last_mut() {
            Some(range) if range.end == ix => range.end += 1,
            _ => self.built.push(ix..ix + 1),
        }
        let Some(model) = &self.model else {
            return div().into_any_element();
        };
        let row = match self.rows.get(ix) {
            Some(Row::Group(title)) => Err(title.clone()),
            Some(Row::Setting(entry)) => Ok(model.entries()[*entry].clone()),
            // The spacer, after the last row.
            None => return div().h(self.spacer).into_any_element(),
        };
        let ends_group = !matches!(self.rows.get(ix + 1), Some(Row::Setting(_)));
        let (column, setting) = match row {
            Err(title) => (self.heading(ix, &title, window, cx), false),
            Ok(entry) => (self.setting_row(ix, &entry, ends_group, window, cx), true),
        };
        div()
            .w_full()
            .px(GUTTER)
            .flex()
            .justify_center()
            .when(setting && ends_group, |row| row.pb(SECTION_GAP))
            .child(column)
            .into_any_element()
    }

    /// The heading of the group titled `title`, row `ix`: its title, and
    /// under it the schema's description of it on one line; the Common
    /// section's, its description and the theme cards.
    fn heading(
        &self,
        ix: usize,
        title: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ui = ui_theme(cx);
        let common = ix == 0 && self.common > 0;
        let description = match common {
            true => tr!("workbench-settings-common-description").to_string(),
            false => (self.model.iter())
                .flat_map(|model| model.groups())
                .find(|group| group.title == title)
                .map(|group| unwrap(&group.description))
                .unwrap_or_default(),
        };
        let id = selector("settings-group", title);
        let role = SECTION_HEADING;
        let heading = div()
            .debug_selector(move || id)
            .w_full()
            .px(ROW_INSET)
            .pt(px(8.))
            .pb(px(2.))
            .child(
                div()
                    .h(role.line_height)
                    .line_height(role.line_height)
                    .text_size(role.size)
                    .font_weight(gpui::FontWeight(f32::from(role.weight)))
                    .text_color(ui.text)
                    .truncate()
                    .child(title.to_owned()),
            )
            .when(!description.is_empty(), |heading| {
                heading.child(
                    div()
                        .h(DESCRIPTION_LINE_HEIGHT)
                        .line_height(DESCRIPTION_LINE_HEIGHT)
                        .text_size(px(12.))
                        .text_color(ui.muted_text)
                        .truncate()
                        .child(description),
                )
            });
        div()
            .flex_1()
            .min_w_0()
            .max_w(COLUMN_WIDTH)
            .flex()
            .flex_col()
            .child(heading)
            .when(common, |column| column.child(self.theme_cards(window, cx)))
            .into_any_element()
    }

    /// The theme cards, Yohen, Hakuji and Match system, each drawn in its
    /// theme's own colours (the setup wizard's tiles), a click on one picks
    /// it ([`SettingsEditor::pick_theme_card`]). The one the settings show
    /// has an accent outline, and, while the cards' row is selected in the
    /// focused list, the focus ring, as does the first with none shown.
    /// They're off where the selected scope's file can't set the theme.
    fn theme_cards(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self.entry(THEME_MODE).cloned() else {
            return div().into_any_element();
        };
        let colors = cx.theme().colors;
        let restricted = self.restricted(&entry);
        let chosen = self.chosen_theme_card(cx);
        let focused = self.cards_selected() && self.focus.is_focused(window);
        let ringed = focused.then(|| self.ringed_card(cx));
        let swatch = |id: &str, appearance: Appearance, cx: &App| {
            let title = themes::find(id).map_or("", |theme| theme.title);
            self.swatches.of_option(title, appearance, cx)
        };
        let dark = swatch(themes::DEFAULT_DARK, Appearance::Dark, cx);
        let light = swatch(themes::DEFAULT_LIGHT, Appearance::Light, cx);
        let inset = px(4.);
        let drawn = self.card_bounds.clone();
        let cards = THEME_CARDS.into_iter().enumerate().map(|(ix, card)| {
            let (name, art) = match card {
                ThemeCard::Yohen => (tr!("workbench-settings-theme-yohen"), TileArt::Window(dark)),
                ThemeCard::Hakuji => (
                    tr!("workbench-settings-theme-hakuji"),
                    TileArt::Window(light),
                ),
                ThemeCard::System => (
                    tr!("workbench-settings-theme-system"),
                    TileArt::Split { dark, light },
                ),
            };
            let shown = chosen == Some(card);
            let id = selector("settings-theme", &card.mode_value());
            div()
                .id(SharedString::from(id.clone()))
                .debug_selector(move || id)
                .role(gpui::Role::RadioButton)
                .aria_label(name.clone())
                .aria_toggled(gpui::Toggled::from(shown))
                .relative()
                .flex_none()
                .w(TILE_WIDTH)
                .p(inset)
                .flex()
                .flex_col()
                .gap(px(6.))
                .rounded(fe_ui::radius::MENU)
                .border_1()
                .border_color(match shown {
                    true => colors.accent.base,
                    false => colors.border.default,
                })
                .when(restricted, |card| card.opacity(0.5))
                .when(!restricted, |element| {
                    element
                        .cursor_pointer()
                        .when(!shown, |element| {
                            element.hover(move |style| style.bg(colors.state.hover))
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.selected = Some(0);
                            window.focus(&this.focus, cx);
                            this.pick_theme_card(card, cx);
                        }))
                })
                .child(art.render(TILE_WIDTH - inset * 2. - px(2.)))
                .child(
                    div()
                        .px(px(2.))
                        .text_size(UI_BODY.size)
                        .line_height(UI_BODY.line_height)
                        .text_color(colors.text.primary)
                        .truncate()
                        .child(name),
                )
                // Where it's drawn, for the keys to scroll it into view.
                .child({
                    let drawn = drawn.clone();
                    gpui::canvas(
                        move |bounds, _, _| drawn.borrow_mut()[ix] = Some(bounds),
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0()
                })
                .when(ringed == Some(card), |element| {
                    let ring = selector("settings-theme-ring", &card.mode_value());
                    element.child(
                        fe_ui::focus_ring(fe_ui::radius::MENU, px(1.), &colors)
                            .debug_selector(move || ring),
                    )
                })
        });
        div()
            .id("settings-theme-cards")
            .debug_selector(|| "settings-theme-cards".into())
            .role(gpui::Role::RadioGroup)
            .aria_label(tr!("workbench-settings-theme-cards"))
            .px(ROW_INSET)
            .pt(px(8.))
            .pb(px(4.))
            .flex()
            .flex_wrap()
            .gap(px(12.))
            .children(cards)
            .into_any_element()
    }

    /// Whether the selected row is the Common section's heading, its theme
    /// cards': the only heading the keys select.
    pub(crate) fn cards_selected(&self) -> bool {
        self.common > 0 && self.selected == Some(0)
    }

    /// The text value of the theme setting at dotted `key` as the selected
    /// scope shows it, a change still being written included; `None` unset.
    fn theme_text(&self, key: &str, cx: &App) -> Option<String> {
        let (value, _) = self.value(self.entry(key)?, cx)?;
        value.as_str().map(str::to_owned)
    }

    /// The theme settings that have `appearance` show a theme other than
    /// Tenmoku's own, Yohen or Hakuji, as the app chooses the slots
    /// (`fastedit`'s `theme::choice`): the slot, `theme.dark` or
    /// `theme.light`, set to anything but that theme's installed ID
    /// (`builtin/yohen`, `builtin/hakuji`: a slot takes an ID, not a name,
    /// so `yohen` is no theme), and, unless the slot keeps Tenmoku's own,
    /// `theme.name` naming another bundled theme of that appearance, which
    /// the app puts in an unset slot. A name no bundled theme has applies to
    /// neither.
    fn theme_overrides(&self, appearance: Appearance, cx: &App) -> Vec<&'static str> {
        let (slot, own) = match appearance {
            Appearance::Dark => (THEME_DARK, themes::DEFAULT_DARK),
            Appearance::Light => (THEME_LIGHT, themes::DEFAULT_LIGHT),
        };
        let name = self.theme_text(THEME_NAME, cx).filter(|name| {
            themes::find(name)
                .is_some_and(|theme| theme.appearance == appearance && theme.id != own)
        });
        match self.theme_text(slot, cx) {
            Some(id) if id == kiln::extensions::builtin_theme_id(own) => Vec::new(),
            Some(_) => [slot].into_iter().chain(name.map(|_| THEME_NAME)).collect(),
            None => name.map(|_| THEME_NAME).into_iter().collect(),
        }
    }

    /// The theme card the settings show: its mode, and for each appearance
    /// it covers, Tenmoku's own theme; `None` when the settings show
    /// another theme, which no card stands for.
    pub(crate) fn chosen_theme_card(&self, cx: &App) -> Option<ThemeCard> {
        let mode = self.theme_text(THEME_MODE, cx)?;
        THEME_CARDS.into_iter().find(|card| {
            card.mode_value() == mode
                && (card.appearances().iter())
                    .all(|appearance| self.theme_overrides(*appearance, cx).is_empty())
        })
    }

    /// Picks `card`: `theme.mode` becomes its mode, removed for the default
    /// (dark), and the settings that would show another theme in its place
    /// are removed ([`SettingsEditor::theme_overrides`]), so what's chosen is
    /// what the card draws. Nothing where the selected scope's file can't set
    /// the theme, or the card is already the one shown.
    pub(crate) fn pick_theme_card(&mut self, card: ThemeCard, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(THEME_MODE).cloned() else {
            return;
        };
        if self.restricted(&entry) || self.chosen_theme_card(cx) == Some(card) {
            return;
        }
        let mut resets: Vec<&str> = (card.appearances().iter())
            .flat_map(|appearance| self.theme_overrides(*appearance, cx))
            .collect();
        resets.sort_unstable();
        resets.dedup();
        for key in resets {
            let key: Vec<String> = key.split('.').map(str::to_owned).collect();
            self.write(&key, Change::Reset, cx);
        }
        let value = card.mode_value();
        let change = match entry.default.as_str() == Some(value.as_str()) {
            true => Change::Reset,
            false => Change::Set(Value::Str(value)),
        };
        self.write(&entry.key, change, cx);
    }

    /// Left or Right (`forward`), Home or End (`end`) on the selected cards'
    /// row: picks the card before or after the one shown, or the first or
    /// the last, as a radio group's keys do.
    pub(crate) fn step_theme_card(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let at = (self.chosen_theme_card(cx))
            .and_then(|card| THEME_CARDS.iter().position(|c| *c == card));
        let last = THEME_CARDS.len() - 1;
        let next = match key {
            "left" => at.map_or(0, |at| at.saturating_sub(1)),
            "right" => at.map_or(0, |at| (at + 1).min(last)),
            "home" => 0,
            "end" => last,
            // Enter or Space: the one shown, or else the first.
            _ => at.unwrap_or(0),
        };
        self.pick_theme_card(THEME_CARDS[next], cx);
        self.pinned = false;
        self.reveal_cards(window, cx);
        cx.notify();
        true
    }

    /// The card the focus ring is on while the cards' row is selected: the
    /// one shown, else the first.
    fn ringed_card(&self, cx: &App) -> ThemeCard {
        self.chosen_theme_card(cx).unwrap_or(ThemeCard::Yohen)
    }

    /// Scrolls the list as little as it takes for the ringed card, its focus
    /// ring included, to show, from where it was last drawn; whether it was
    /// drawn in the last frame. A card taller than the list shows its top.
    fn reveal_card(&self, card: ThemeCard) -> bool {
        let Some(ix) = THEME_CARDS.iter().position(|c| *c == card) else {
            return false;
        };
        let Some(bounds) = self.card_bounds.borrow()[ix] else {
            return false;
        };
        let view = self.list.viewport_bounds();
        // The ring's 2 px, 1 px out, and a pixel more.
        let margin = px(4.);
        let (top, bottom) = (bounds.top() - margin, bounds.bottom() + margin);
        if top < view.top() {
            self.list.scroll_by(top - view.top());
        } else if bottom > view.bottom() {
            self.list
                .scroll_by((bottom - view.bottom()).min(top - view.top()));
        }
        true
    }

    /// Brings the ringed card into view: at once if it was drawn in the
    /// last frame, else once the list, scrolled to its top, has drawn it.
    pub(super) fn reveal_cards(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.reveal_card(self.ringed_card(cx)) {
            return;
        }
        self.list.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: Pixels::ZERO,
        });
        cx.on_next_frame(window, |this, _, cx| {
            if this.cards_selected() {
                this.reveal_card(this.ringed_card(cx));
            }
            cx.notify();
        });
    }

    /// The label of `entry`: its catalogue message by key
    /// ([`label_key`]), else, for a setting newer than the catalogue, the
    /// name its key ends in, as a title.
    pub(crate) fn label(entry: &SettingEntry) -> SharedString {
        let name = entry
            .key
            .last()
            .map_or_else(String::new, |name| title(name));
        kiln_l10n::tr_or(&label_key(&entry.key_path()), &name)
    }

    /// Row `ix`, `entry`'s: on the left its label and its key, with an
    /// accent dot before them and Reset after them if the selected scope's
    /// file sets it, and under them one line of help; its control on the
    /// right, or under them in a narrow column; and at the bottom why a
    /// typed value wasn't written, if it wasn't. It's dimmed with a note,
    /// its control off, if the selected scope's file can't set it. A
    /// hairline is under it unless it `ends_group`.
    fn setting_row(
        &mut self,
        ix: usize,
        entry: &SettingEntry,
        ends_group: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ui = ui_theme(cx);
        let colors = cx.theme().colors;
        let code = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Code, cx);
        let key = entry.key_path();
        let restricted = self.restricted(entry);
        let (value, modified) = self.value(entry, cx).unwrap_or((Json::Null, false));
        let control = self.control(ix, entry, &value, restricted, window, cx);
        let message = self.message(&key);
        let label = Self::label(entry);
        let help = self.help(entry);
        let selected = self.selected == Some(ix);
        let hovered = self.hovered == Some(ix);
        let shade = if selected && self.focus.is_focused(window) {
            Some(ui.row_selected)
        } else if selected {
            Some(ui.row_selected_inactive)
        } else if hovered {
            Some(ui.row_hover)
        } else {
            None
        };
        // A list editor's items scroll in the column's width.
        let fills = entry.kind == fe_config::SettingKind::TextList;
        let selector = |name: &'static str| {
            let id = selector(name, &key);
            move || id
        };
        div()
            .id(("settings-row", ix))
            .debug_selector(selector("settings-row"))
            .relative()
            .flex_1()
            .min_w_0()
            .max_w(COLUMN_WIDTH)
            .px(ROW_INSET)
            .py(ROW_PADDING)
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(CONTROL_GAP)
            .gap_y(px(6.))
            .when(restricted, |row| row.opacity(0.5))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered = Some(ix);
                } else if this.hovered == Some(ix) {
                    this.hovered = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected = Some(ix);
                window.focus(&this.focus, cx);
                cx.notify();
            }))
            .when_some(shade, |row, shade| {
                row.child(
                    div()
                        .debug_selector(selector("settings-shade"))
                        .absolute()
                        .inset_0()
                        .rounded_sm()
                        .bg(shade),
                )
            })
            .when(!ends_group, |row| {
                row.child(
                    div()
                        .absolute()
                        .left(ROW_INSET)
                        .right(ROW_INSET)
                        .bottom_0()
                        .h(px(1.))
                        .bg(ui.border),
                )
            })
            .child(
                div()
                    .flex_grow(1.)
                    .flex_basis(TITLE_WIDTH)
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(ROW_GAP)
                    .child(
                        div()
                            .flex_none()
                            .h(LINE_HEIGHT)
                            .flex()
                            .items_center()
                            .gap_2()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(modified, |line| {
                                line.child(
                                    div()
                                        .debug_selector(selector("settings-modified"))
                                        .flex_none()
                                        .size(DOT)
                                        .rounded_full()
                                        .bg(colors.accent.base),
                                )
                            })
                            // Short of room, the key is cut short first, then
                            // the label: Reset always shows.
                            .child(
                                div()
                                    .debug_selector(selector("settings-label"))
                                    .min_w_0()
                                    .truncate()
                                    .text_size(LABEL_ROLE.size)
                                    .line_height(LABEL_ROLE.line_height)
                                    .text_color(colors.text.primary)
                                    .child(label),
                            )
                            .child(
                                div()
                                    .debug_selector(selector("settings-key"))
                                    .flex_shrink(1000.)
                                    .min_w_0()
                                    .truncate()
                                    .font_family(code)
                                    .text_size(fe_ui::CODE_SMALL.size)
                                    .line_height(fe_ui::CODE_SMALL.line_height)
                                    .text_color(colors.text.faint)
                                    .child(key.clone()),
                            )
                            .when(modified && !restricted, |line| {
                                line.child(self.reset_button(ix, entry, cx))
                            }),
                    )
                    .when(!help.is_empty(), |block| {
                        block.child(
                            div()
                                .debug_selector(selector("settings-description"))
                                .h(HELP_ROLE.line_height)
                                .line_height(HELP_ROLE.line_height)
                                .text_size(HELP_ROLE.size)
                                .font_weight(gpui::FontWeight(f32::from(HELP_ROLE.weight)))
                                .text_color(colors.text.muted)
                                .truncate()
                                .child(help),
                        )
                    })
                    .when(restricted, |block| {
                        block.child(
                            div()
                                .debug_selector(selector("settings-restricted"))
                                .text_size(HELP_ROLE.size)
                                .line_height(HELP_ROLE.line_height)
                                .text_color(colors.text.muted)
                                .child(tr!("workbench-settings-restricted")),
                        )
                    }),
            )
            .child(
                div()
                    .when(fills, |slot| slot.w_full())
                    // Never wider than the column: under the title, its
                    // control takes no more than the column's width.
                    .when(!fills, |slot| slot.flex_none().max_w_full())
                    .flex()
                    .whitespace_nowrap()
                    .text_size(px(12.))
                    .child(control),
            )
            .when_some(message, |row, message| {
                row.child(
                    div()
                        .debug_selector(selector("setting-message"))
                        .w_full()
                        .line_height(MESSAGE_HEIGHT)
                        .text_size(px(11.))
                        .text_color(ui.error_text)
                        .child(message),
                )
            })
            .into_any_element()
    }

    /// The one line of help `entry`'s row shows: its description, the
    /// catalogue's message for its key, or, for one the schema doesn't
    /// describe, its table's.
    pub(crate) fn help(&self, entry: &SettingEntry) -> String {
        if !entry.description.is_empty() {
            return help_line(&entry.description);
        }
        let table = &entry.key[..entry.key.len().saturating_sub(1)];
        (self.model.iter())
            .flat_map(|model| model.groups())
            .find(|group| group.key == table)
            .map(|group| help_line(&group.description))
            .unwrap_or_default()
    }

    /// Whether section `section` has a setting the selected scope's file
    /// sets, a change still being written included.
    pub(crate) fn section_changed(&self, section: &Section, cx: &App) -> bool {
        let Some(model) = &self.model else {
            return false;
        };
        (section.entries.iter())
            .filter_map(|entry| model.entries().get(*entry))
            .any(|entry| self.value(entry, cx).is_some_and(|(_, set)| set))
    }

    /// The section the category navigation highlights: the one whose rows
    /// are at the top of the list.
    pub(crate) fn active_section(&self) -> Option<usize> {
        let top = self.list.logical_scroll_top().item_ix;
        (self.sections.iter())
            .rposition(|section| section.row <= top)
            .or((!self.sections.is_empty()).then_some(0))
    }

    /// Scrolls section `ix`'s heading to the top of the list, which takes
    /// focus.
    fn go_to_section(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(section) = self.sections.get(ix) else {
            return;
        };
        self.list.scroll_to(ListOffset {
            item_ix: section.row,
            offset_in_item: px(0.),
        });
        self.anchor = Some(section.row);
        self.pinned = true;
        self.repins = 0;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Once the list is laid out: makes the spacer after the last row what
    /// the rows from the anchor's heading on lack of the view's height,
    /// more or less as the view or the rows changed, and, while the anchor
    /// is pinned, scrolls its heading back to the top if the list filled
    /// the view from the rows above it instead. Either is drawn in a frame
    /// it asks for.
    pub(super) fn settle_anchor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(anchor), Some(last)) = (self.anchor, self.rows.len().checked_sub(1)) else {
            return;
        };
        let view = self.list.viewport_bounds();
        let heading = self.list.bounds_for_item(anchor);
        let mut again = false;
        // Known when the rows from the heading to the last are measured.
        if let (Some(heading), Some(end)) = (heading, self.list.bounds_for_item(last)) {
            let spacer = (view.size.height - (end.bottom() - heading.top())).max(Pixels::ZERO);
            if spacer != self.spacer {
                self.spacer = spacer;
                self.repins = 0;
                let spacer_row = self.rows.len();
                self.list.remeasure_items(spacer_row..spacer_row + 1);
                again = true;
            }
        } else if self.spacer != Pixels::ZERO {
            // A resize drops the measured heights, and a section taller
            // than the view leaves the last row unmeasured, or scrolling
            // leaves the heading out of the list: the old spacer is stale
            // and, scrolled to the end, could fill the view.
            self.spacer = Pixels::ZERO;
            let spacer_row = self.rows.len();
            self.list.remeasure_items(spacer_row..spacer_row + 1);
            again = true;
        }
        let at_top = heading.is_some_and(|heading| (heading.top() - view.top()).abs() < px(0.5));
        if self.pinned && !at_top && self.repins < 3 {
            self.repins += 1;
            self.list.scroll_to(ListOffset {
                item_ix: anchor,
                offset_in_item: Pixels::ZERO,
            });
            again = true;
        }
        if again {
            let tab = cx.weak_entity();
            window.on_next_frame(move |_, cx| {
                tab.update(cx, |_, cx| cx.notify()).ok();
            });
        }
    }

    /// The section index: a section per line, the Common section first,
    /// with an accent dot if it has a setting the selected scope's file
    /// sets, and how many of its settings the search finds.
    pub(super) fn nav(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = ui_theme(cx);
        let accent = cx.theme().colors.accent.base;
        let active = self.active_section();
        let entries = self.sections.iter().enumerate().map(|(ix, section)| {
            let current = active == Some(ix);
            let changed = self.section_changed(section, cx);
            let id = selector("settings-nav", &section.title);
            let mark = selector("settings-nav-mark", &section.title);
            let name = match changed {
                true => tr!(
                    "workbench-settings-section-changed",
                    section = section.title.as_str()
                ),
                false => SharedString::from(section.title.clone()),
            };
            div()
                .id(("settings-nav", ix))
                .debug_selector(move || id)
                .aria_label(name)
                .flex_none()
                .h(height::MENU_ROW)
                .px_2()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .rounded_sm()
                .cursor_pointer()
                .text_color(ui.text)
                .when(current, |entry| {
                    entry
                        .bg(ui.row_selected_inactive)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .when(!current, |entry| {
                    entry.hover(move |style| style.bg(ui.row_hover))
                })
                .on_click(
                    cx.listener(move |this, _, window, cx| this.go_to_section(ix, window, cx)),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().min_w_0().truncate().child(section.title.clone()))
                        .when(changed, |title| {
                            title.child(
                                div()
                                    .debug_selector(move || mark)
                                    .flex_none()
                                    .size(DOT)
                                    .rounded_full()
                                    .bg(accent),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(ui.muted_text)
                        .child(section.count.to_string()),
                )
        });
        div()
            .id("settings-nav")
            .debug_selector(|| "settings-nav".into())
            .flex_none()
            .w(NAV_WIDTH)
            .h_full()
            .overflow_y_scroll()
            .pt_2()
            .pl_3()
            .flex()
            .flex_col()
            .gap_px()
            .children(entries)
    }

    /// The sections the category navigation lists.
    #[cfg(test)]
    pub(crate) fn sections(&self) -> &[Section] {
        &self.sections
    }
}
