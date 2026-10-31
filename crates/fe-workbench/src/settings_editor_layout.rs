//! How the Settings tab lays its list out, as VS Code's does:
//!
//! - The rows sit in a column at most [`COLUMN_WIDTH`] wide, centred in
//!   the list with [`GUTTER`] either side; a row's hover and selected
//!   shade, its hairline under it and its modified bar are inside it.
//! - A group's heading is a short row, its title and the schema's
//!   description of it, not a setting's height: the list is a gpui `list`,
//!   its rows as tall as they draw.
//! - A setting's row has its title, its description (up to three lines)
//!   and anything wrong on the left, and its control on the right, at the
//!   top; where the column has no room for both, the control goes under.
//!   A list editor always goes under, taking the column's width.
//! - Beside the list, the category navigation lists the top-level groups
//!   the search finds settings in, with how many: a click scrolls one's
//!   heading to the top, and the one in view is highlighted. A tab
//!   narrower than [`NAV_MIN_TAB_WIDTH`] leaves it out.

use fe_command::Value as Json;
use fe_config::{SettingEntry, SettingsModel};
use fe_ui::ui_theme;
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ListOffset, ParentElement as _,
    Pixels, StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
    px,
};

use super::{LINE_HEIGHT, Row, SettingsEditor, WORKSPACE_RESTRICTED, title};

/// The widest the rows' column gets.
pub(super) const COLUMN_WIDTH: Pixels = px(760.);

/// The space either side of the column.
pub(super) const GUTTER: Pixels = px(24.);

/// The space between a row's shade's edge and its text.
const ROW_INSET: Pixels = px(12.);

const ROW_PADDING: Pixels = px(10.);

const ROW_GAP: Pixels = px(4.);

/// The narrowest a setting's title block gets beside its control: in a
/// narrower column the control goes under it.
const TITLE_WIDTH: Pixels = px(280.);

/// The space between a setting's title block and its control.
const CONTROL_GAP: Pixels = px(24.);

/// The height of a line of a setting's description, which shows up to
/// [`DESCRIPTION_LINES`].
const DESCRIPTION_LINE_HEIGHT: Pixels = px(16.);

const DESCRIPTION_LINES: usize = 3;

/// The height of the line saying why a typed value wasn't written.
const MESSAGE_HEIGHT: Pixels = px(14.);

/// The tallest a group's heading is: its title line and its description's.
pub(super) const HEADING_HEIGHT: Pixels = px(46.);

/// The space under a group's last setting, before the next heading.
const SECTION_GAP: Pixels = px(16.);

/// The category navigation's width.
pub(super) const NAV_WIDTH: Pixels = px(180.);

/// The narrowest tab that shows the category navigation.
pub(super) const NAV_MIN_TAB_WIDTH: Pixels = px(700.);

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
}

/// The top-level groups `rows` show settings of, in order. The settings
/// at the top level, under no group, are in none.
pub(crate) fn sections(model: &SettingsModel, rows: &[Row]) -> Vec<Section> {
    let mut sections: Vec<(String, Section)> = Vec::new();
    for (ix, row) in rows.iter().enumerate() {
        let Row::Setting(entry) = row else {
            continue;
        };
        let key = &model.entries()[*entry].key;
        let [top, _, ..] = key.as_slice() else {
            continue;
        };
        if let Some((_, section)) = sections.iter_mut().find(|(key, _)| key == top) {
            section.count += 1;
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
        };
        sections.push((top.clone(), section));
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
            Err(title) => (self.heading(&title, cx), false),
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

    /// The heading of the group titled `title`: its title, and under it
    /// the schema's description of it on one line.
    fn heading(&self, title: &str, cx: &mut Context<Self>) -> AnyElement {
        let ui = ui_theme(cx);
        let description = (self.model.iter())
            .flat_map(|model| model.groups())
            .find(|group| group.title == title)
            .map(|group| group.description.clone())
            .unwrap_or_default();
        let selector = format!("settings-group-{title}");
        div()
            .debug_selector(move || selector)
            .flex_1()
            .min_w_0()
            .max_w(COLUMN_WIDTH)
            .px(ROW_INSET)
            .pt(px(8.))
            .pb(px(2.))
            .child(
                div()
                    .h(px(20.))
                    .line_height(px(20.))
                    .text_size(px(15.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
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
                        .child(unwrap(&description)),
                )
            })
            .into_any_element()
    }

    /// Row `ix`, `entry`'s: on the left its title, its key path while
    /// hovered or selected, Reset to Default if the selected scope's file
    /// sets it, and its description; its control on the right, or under
    /// them in a narrow column; and at the bottom why a typed value wasn't
    /// written, if it wasn't. It's dimmed with a note, its control off, if
    /// the selected scope's file can't set it. A hairline is under it
    /// unless it `ends_group`.
    fn setting_row(
        &mut self,
        ix: usize,
        entry: &SettingEntry,
        ends_group: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ui = ui_theme(cx);
        let key = entry.key_path();
        let restricted = self.restricted(entry);
        let (value, modified) = self.value(entry, cx).unwrap_or((Json::Null, false));
        let control = self.control(ix, entry, &value, restricted, window, cx);
        let message = self.message(&key);
        let name = entry
            .key
            .last()
            .map_or_else(String::new, |name| title(name));
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
        div()
            .id(("settings-row", ix))
            .debug_selector({
                let key = key.clone();
                move || format!("settings-row-{key}")
            })
            .relative()
            .flex_1()
            .min_w_0()
            .max_w(COLUMN_WIDTH)
            .px(ROW_INSET)
            .py(ROW_PADDING)
            .flex()
            .flex_wrap()
            .items_start()
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
                let key = key.clone();
                row.child(
                    div()
                        .debug_selector(move || format!("settings-shade-{key}"))
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
            .when(modified, |row| {
                row.child(
                    div()
                        .debug_selector({
                            let key = key.clone();
                            move || format!("settings-modified-{key}")
                        })
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(ui.accent),
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
                            .line_height(LINE_HEIGHT)
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            // Short of room, the key is cut short first, then
                            // the name: Reset always shows.
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(ui.text)
                                    .child(name),
                            )
                            .when(modified && !restricted, |line| {
                                line.child(self.reset_button(ix, entry, cx))
                            })
                            .child(
                                div()
                                    .flex_shrink(1000.)
                                    .min_w_0()
                                    .truncate()
                                    .text_color(ui.muted_text)
                                    .text_size(px(12.))
                                    .when(!selected && !hovered, |key| key.invisible())
                                    .child(key.clone()),
                            ),
                    )
                    .when(!entry.description.is_empty(), |block| {
                        block.child(
                            div()
                                .debug_selector({
                                    let key = key.clone();
                                    move || format!("settings-description-{key}")
                                })
                                .line_height(DESCRIPTION_LINE_HEIGHT)
                                .text_size(px(12.))
                                .text_color(ui.muted_text)
                                .max_h(DESCRIPTION_LINE_HEIGHT * DESCRIPTION_LINES as f32)
                                .overflow_hidden()
                                .line_clamp(DESCRIPTION_LINES)
                                .text_ellipsis()
                                .child(unwrap(&entry.description)),
                        )
                    })
                    .when(restricted, |block| {
                        let key = key.clone();
                        block.child(
                            div()
                                .debug_selector(move || format!("settings-restricted-{key}"))
                                .text_size(px(12.))
                                .text_color(ui.muted_text)
                                .child(WORKSPACE_RESTRICTED),
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
                        .debug_selector(move || format!("setting-message-{key}"))
                        .w_full()
                        .line_height(MESSAGE_HEIGHT)
                        .text_size(px(11.))
                        .text_color(ui.error_text)
                        .child(message),
                )
            })
            .into_any_element()
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

    /// The category navigation: a top-level group per line, with how many
    /// of its settings the search finds.
    pub(super) fn nav(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = ui_theme(cx);
        let active = self.active_section();
        let entries = self.sections.iter().enumerate().map(|(ix, section)| {
            let current = active == Some(ix);
            let selector = format!("settings-nav-{}", section.title);
            div()
                .id(("settings-nav", ix))
                .debug_selector(move || selector)
                .flex_none()
                .h(px(26.))
                .px_2()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .rounded_sm()
                .cursor_pointer()
                .text_color(if current { ui.text } else { ui.muted_text })
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
                .child(div().min_w_0().truncate().child(section.title.clone()))
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
