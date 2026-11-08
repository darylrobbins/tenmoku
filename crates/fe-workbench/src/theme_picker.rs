//! Change Color Theme (`theme.select`): a [`Picker`] over fastedit's
//! bundled themes ([`fe_config::themes`]) and the installed extensions'
//! ([`kiln::extensions::themes`]), which keeps the one picked in the user's
//! `settings.toml`. The settings reload that write brings applies it, as
//! any other setting's.
//!
//! A bundled theme is kept as `theme.name`, its title; an installed one as
//! the setting of its appearance's slot, `theme.dark` or `theme.light`, its
//! ID. Either way, the setting that would win the slot over it is removed,
//! the other slot keeps the theme it had, and if the other appearance
//! shows, whether `theme.mode` sets it or follows the system's, the mode
//! turns to the picked one's, so the pick shows.

use std::path::PathBuf;

use fe_config::{
    SettingValue, Settings, ThemeSettings,
    themes::{self, BundledTheme},
};
use gpui::{
    AnyElement, App, Context, FocusHandle, IntoElement as _, ParentElement as _, SharedString,
    Styled as _, Window, div,
};
use kiln::{
    core::theme::Appearance,
    extensions::{BUILTIN_THEMES, ThemeInfo},
    ui::ActiveTheme as _,
};

use crate::{
    picker::{Picker, PickerDelegate},
    workbench::toast::{self, Toast},
};

/// Where a row's theme comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Bundled(&'static BundledTheme),
    /// An extension's, by ID.
    Installed(String),
}

#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub(crate) title: SharedString,
    pub(crate) appearance: Appearance,
    pub(crate) source: Source,
}

impl Row {
    /// The ID kiln selects the row's theme by.
    pub(crate) fn id(&self) -> String {
        match &self.source {
            Source::Bundled(theme) => kiln::extensions::builtin_theme_id(theme.id),
            Source::Installed(id) => id.clone(),
        }
    }
}

pub(crate) struct ThemePicker {
    /// The bundled themes, then the installed ones by title, A–Z.
    rows: Vec<Row>,
    /// The rows matching the query, as indices into `rows`.
    matches: Vec<usize>,
    /// The row of the theme showing when it opened.
    current: Option<usize>,
    /// The user's `settings.toml`, where a pick goes; `None` with no config
    /// directory.
    settings_path: Option<PathBuf>,
    /// What was focused before it opened, focused again once it closes.
    previous_focus: Option<FocusHandle>,
}

impl ThemePicker {
    pub(crate) fn new() -> Self {
        Self {
            rows: Vec::new(),
            matches: Vec::new(),
            current: None,
            settings_path: None,
            previous_focus: None,
        }
    }

    /// The picker is opening, with `previous_focus` focused until now, or
    /// opening afresh, keeping what was focused before, if that's `None`:
    /// it lists the themes there are now, the one showing marked, for a
    /// pick to go to `settings_path`.
    pub(crate) fn opened(
        &mut self,
        previous_focus: Option<FocusHandle>,
        settings_path: Option<PathBuf>,
        cx: &App,
    ) {
        self.previous_focus = previous_focus.or(self.previous_focus.take());
        self.settings_path = settings_path;
        let listed = kiln::extensions::themes(cx);
        self.rows = rows(&listed);
        self.matches = (0..self.rows.len()).collect();
        let settings = theme_settings(cx);
        let showing = showing(&settings, cx.theme().appearance, &listed);
        self.current =
            showing.and_then(|showing| self.rows.iter().position(|row| row.id() == showing));
    }

    fn row(&self, ix: usize) -> &Row {
        &self.rows[self.matches[ix]]
    }

    /// Whether match `ix` is the theme showing when it opened.
    fn is_current(&self, ix: usize) -> bool {
        self.current == Some(self.matches[ix])
    }

    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }
}

#[cfg(test)]
impl ThemePicker {
    /// The matches' titles and appearances, each with whether it's marked
    /// current.
    pub(crate) fn labels(&self) -> Vec<(SharedString, Appearance, bool)> {
        (0..self.matches.len())
            .map(|ix| {
                let row = self.row(ix);
                (row.title.clone(), row.appearance, self.is_current(ix))
            })
            .collect()
    }
}

/// The bundled themes, then the `installed` ones, which
/// [`kiln::extensions::themes`] lists, by title, A–Z: what the picker
/// lists, as the Settings tab's theme dropdowns do.
pub(crate) fn rows(installed: &[ThemeInfo]) -> Vec<Row> {
    let bundled = (themes::bundled_themes().iter()).map(|theme| Row {
        title: theme.title.into(),
        appearance: theme.appearance,
        source: Source::Bundled(theme),
    });
    let mut installed: Vec<Row> = (installed.iter())
        .filter(|theme| !is_builtin(&theme.id))
        .map(|theme| Row {
            title: theme.title.clone().into(),
            appearance: theme.appearance,
            source: Source::Installed(theme.id.clone()),
        })
        .collect();
    installed.sort_by_key(|row| row.title.to_lowercase());
    bundled.chain(installed).collect()
}

/// Whether `id` is one of the app's own themes, listed as bundled.
fn is_builtin(id: &str) -> bool {
    id.split_once('/')
        .is_some_and(|(namespace, _)| namespace == BUILTIN_THEMES)
}

/// The theme settings now: the defaults with no settings service.
fn theme_settings(cx: &App) -> ThemeSettings {
    (cx.try_global::<kiln::GlobalSettings<Settings>>())
        .map(|global| global.settings().theme.clone())
        .unwrap_or_default()
}

/// The setting of the slot for `appearance`, and its name.
fn slot(settings: &ThemeSettings, appearance: Appearance) -> (&Option<String>, &'static str) {
    match appearance {
        Appearance::Dark => (&settings.dark, "dark"),
        Appearance::Light => (&settings.light, "light"),
    }
}

/// The bundled theme `theme.name` names, if any.
fn named(settings: &ThemeSettings) -> Option<&'static BundledTheme> {
    settings.name.as_deref().and_then(themes::find)
}

/// The ID of the theme `settings` show for `appearance`, as the host
/// chooses it and kiln takes the choice: the slot's setting, else the
/// bundled theme `theme.name` names if it's of that appearance, else, with
/// no choice, Tenmoku's default for the appearance, Yohen or Hakuji, which
/// the host selects for an unset slot. `None` for a choice that isn't one of
/// the `installed` themes of that appearance: kiln falls back to its own base
/// for the slot (Kiln Dark or Light chrome) under the default's editor
/// palette, which is no theme listed, so nothing is marked.
fn showing(
    settings: &ThemeSettings,
    appearance: Appearance,
    installed: &[ThemeInfo],
) -> Option<String> {
    let chosen = match slot(settings, appearance).0 {
        Some(id) => Some(id.clone()),
        None => named(settings)
            .filter(|theme| theme.appearance == appearance)
            .map(|theme| kiln::extensions::builtin_theme_id(theme.id)),
    };
    let Some(chosen) = chosen else {
        let own = match appearance {
            Appearance::Dark => themes::DEFAULT_DARK,
            Appearance::Light => themes::DEFAULT_LIGHT,
        };
        return Some(kiln::extensions::builtin_theme_id(own));
    };
    let taken =
        (installed.iter()).any(|theme| theme.id == chosen && theme.appearance == appearance);
    taken.then_some(chosen)
}

/// The changes to `settings.toml` that make `row` its appearance's theme
/// and show it, `appearance` showing now, keeping the other appearance's:
/// see the module docs.
fn changes(
    row: &Row,
    settings: &ThemeSettings,
    appearance: Appearance,
) -> Vec<(&'static [&'static str], Option<String>)> {
    let mut changes: Vec<(&'static [&'static str], Option<String>)> = Vec::new();
    let key = |appearance| match appearance {
        Appearance::Dark => &["theme", "dark"] as &'static [&'static str],
        Appearance::Light => &["theme", "light"],
    };
    let named = named(settings);
    match &row.source {
        Source::Bundled(theme) => {
            // The other slot's theme, if the name chose it, stays it.
            if let Some(before) = named.filter(|before| before.appearance != theme.appearance)
                && slot(settings, before.appearance).0.is_none()
            {
                let id = kiln::extensions::builtin_theme_id(before.id);
                changes.push((key(before.appearance), Some(id)));
            }
            changes.push((&["theme", "name"], Some(theme.title.to_owned())));
            if slot(settings, theme.appearance).0.is_some() {
                changes.push((key(theme.appearance), None));
            }
        }
        Source::Installed(id) => {
            changes.push((key(row.appearance), Some(id.clone())));
            if named.is_some_and(|before| before.appearance == row.appearance) {
                changes.push((&["theme", "name"], None));
            }
        }
    }
    // The mode, whether set or the system's, shows the other appearance.
    if appearance != row.appearance {
        let mode = slot(settings, row.appearance).1;
        changes.push((&["theme", "mode"], Some(mode.to_owned())));
    }
    changes
}

impl PickerDelegate for ThemePicker {
    /// Rows whose title holds `query`, ignoring case.
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query.to_lowercase();
        self.matches = (self.rows.iter().enumerate())
            .filter(|(_, row)| row.title.to_lowercase().contains(&query))
            .map(|(ix, _)| ix)
            .collect();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.row(ix).id().into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let row = self.row(ix);
        let appearance = match row.appearance {
            Appearance::Dark => "dark",
            Appearance::Light => "light",
        };
        let muted_text = fe_ui::ui_theme(cx).muted_text;
        let muted = |text: &'static str| div().ml_2().text_color(muted_text).child(text);
        let current = self.is_current(ix).then(|| muted("current"));
        div()
            .flex()
            .items_center()
            .child(row.title.clone())
            .child(muted(appearance))
            .children(current)
            .into_any_element()
    }

    /// Writes the row's theme to `settings.toml`, whose reload applies it,
    /// then focuses what was focused before. A write that fails is a toast.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let row = self.row(ix).clone();
        let changes = changes(&row, &theme_settings(cx), cx.theme().appearance);
        let changes: Vec<(&[&str], Option<SettingValue>)> = (changes.iter())
            .map(|(key, value)| (*key, value.as_deref().map(SettingValue::Str)))
            .collect();
        let written = match &self.settings_path {
            Some(path) => fe_config::write_settings(path, &changes)
                .map_err(|error| format!("{}: {error}", path.display())),
            None => Err("there's no config directory".to_owned()),
        };
        if let Err(error) = written {
            log::error!("can't keep the theme {}: {error}", row.title);
            toast::show(
                cx,
                Toast::error(format!("Can't keep the theme {}", row.title)).detail(error),
            );
        }
        self.restore_focus(window, cx);
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.restore_focus(window, cx);
    }
}

#[cfg(test)]
#[path = "theme_picker_tests.rs"]
mod theme_picker_tests;
