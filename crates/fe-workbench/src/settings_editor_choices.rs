//! The lists a [`SettingKind::Choice`](fe_config::SettingKind::Choice)
//! setting is one of, which only the app knows: the themes, as Change
//! Color Theme lists them ([`theme_picker::rows`]), the Claude models it
//! knows ([`chat_models::KNOWN`]) and the fonts the text system has.
//!
//! A slot, `theme.dark` or `theme.light`, takes a theme of its appearance
//! by the ID kiln selects it by, as the picker writes an installed one;
//! `theme.name` takes a bundled one by title, as the picker writes it.
//!
//! A model or font not listed is still one the setting takes: a model
//! newer than the app, or a font the text system finds by another name.
//! It shows with no hint, so a closed dropdown of fonts never lists them.

use fe_config::{ChoiceSource, EnumVariant, chat_models, themes};
use gpui::App;
use kiln::core::theme::Appearance;
use kiln_l10n::tr;

use crate::theme_picker::{self, Source};

/// The values `source` lists now, each described by its title, or by its
/// appearance for one that is a title.
pub(super) fn variants(source: ChoiceSource, cx: &App) -> Vec<EnumVariant> {
    match source {
        ChoiceSource::Themes { appearance } => themes(appearance, cx),
        ChoiceSource::ChatModels => (chat_models::KNOWN.iter())
            .map(|model| variant(model))
            .collect(),
        // Sorted and deduplicated by the text system.
        ChoiceSource::Fonts => (cx.text_system().all_font_names().iter())
            .map(|font| variant(font))
            .collect(),
    }
}

/// Whether a value `source` doesn't list is one the setting takes all the
/// same, shown with no hint.
pub(super) fn takes_any(source: ChoiceSource) -> bool {
    match source {
        ChoiceSource::Themes { .. } => false,
        ChoiceSource::ChatModels | ChoiceSource::Fonts => true,
    }
}

/// Whether `source`'s dropdown ends in Other…, which has the row take a
/// value typed in: a model id newer than the app.
pub(super) fn other(source: ChoiceSource) -> bool {
    source == ChoiceSource::ChatModels
}

fn variant(value: &str) -> EnumVariant {
    EnumVariant {
        value: value.to_owned(),
        description: String::new(),
    }
}

/// The value `source` lists `value` as: a bundled theme's title for the
/// ID, short title or title in another case `theme.name` also takes it by
/// ([`themes::find`]); else `value`, as a slot takes only an ID, and a
/// model or font only as listed.
pub(super) fn listed_as(source: ChoiceSource, value: &str) -> String {
    match source {
        ChoiceSource::Themes { appearance: None } => themes::find(value)
            .map_or(value, |theme| theme.title)
            .to_owned(),
        ChoiceSource::Themes {
            appearance: Some(_),
        }
        | ChoiceSource::ChatModels
        | ChoiceSource::Fonts => value.to_owned(),
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
                    description: appearance_name(theme.appearance).to_string(),
                }),
                Source::Installed(_) => None,
            })
            .collect(),
    }
}

/// A bundled theme's appearance, as `theme.name`'s list describes it.
fn appearance_name(appearance: Appearance) -> gpui::SharedString {
    match appearance {
        Appearance::Dark => tr!("workbench-settings-appearance-dark"),
        Appearance::Light => tr!("workbench-settings-appearance-light"),
    }
}
