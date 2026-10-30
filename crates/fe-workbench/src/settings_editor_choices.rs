//! The lists a [`SettingKind::Choice`](fe_config::SettingKind::Choice)
//! setting is one of, which only the app knows: the themes, as Change
//! Color Theme lists them ([`theme_picker::rows`]).
//!
//! A slot, `theme.dark` or `theme.light`, takes a theme of its appearance
//! by the ID kiln selects it by, as the picker writes an installed one;
//! `theme.name` takes a bundled one by title, as the picker writes it.

use fe_config::{ChoiceSource, EnumVariant, themes};
use gpui::App;
use kiln::core::theme::Appearance;

use crate::theme_picker::{self, Source};

/// The values `source` lists now, each described by its title, or by its
/// appearance for one that is a title.
pub(super) fn variants(source: ChoiceSource, cx: &App) -> Vec<EnumVariant> {
    match source {
        ChoiceSource::Themes { appearance } => themes(appearance, cx),
    }
}

/// The value `source` lists `value` as: a bundled theme's title for the
/// ID, short title or title in another case `theme.name` also takes it by
/// ([`themes::find`]); else `value`, as a slot takes only an ID.
pub(super) fn listed_as(source: ChoiceSource, value: &str) -> String {
    match source {
        ChoiceSource::Themes { appearance: None } => themes::find(value)
            .map_or(value, |theme| theme.title)
            .to_owned(),
        ChoiceSource::Themes {
            appearance: Some(_),
        } => value.to_owned(),
    }
}

/// The themes of `appearance` by ID, or with none the bundled ones by
/// title.
fn themes(appearance: Option<Appearance>, cx: &App) -> Vec<EnumVariant> {
    let rows = theme_picker::rows(&kiln::extensions::themes(cx));
    match appearance {
        Some(appearance) => (rows.iter())
            .filter(|row| row.appearance == appearance)
            .map(|row| EnumVariant {
                value: row.id(),
                description: row.title.to_string(),
            })
            .collect(),
        None => (rows.iter())
            .filter_map(|row| match row.source {
                Source::Bundled(theme) => Some(EnumVariant {
                    value: theme.title.to_owned(),
                    description: appearance_name(theme.appearance).to_owned(),
                }),
                Source::Installed(_) => None,
            })
            .collect(),
    }
}

fn appearance_name(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Dark => "dark",
        Appearance::Light => "light",
    }
}
