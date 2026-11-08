//! The first-run setup wizard's model: what it asks, in which order, and
//! what each answer writes to the user's `settings.toml`. No GPUI here: the
//! sheet shows [`Step::controls`] and passes what [`Choices::choose`]
//! returns to [`fe_config::write_settings`].
//!
//! A control offers a curated few of its setting's values. A value it
//! doesn't offer, set by hand or an earlier run, is listed too while it's the
//! current one, so nothing is misreported and the choice can be left alone:
//! a theme, font or size as it is, another as "Custom". Leaving it drops it
//! from the list, as nothing could write it back.
//!
//! Spell check, session restore, telemetry, update checks, the AI chat and UI
//! density are left out on purpose, to keep the wizard five short pages: each
//! has no setting yet or its own first-use flow.

use std::collections::BTreeMap;

use fe_config::{SettingValue, Settings, themes};
use kiln::{
    core::{Os, theme::Appearance},
    platform::fonts,
};
use serde::Serialize;

/// A setting's path, table names then its name: `["theme", "mode"]`.
pub(crate) type Key = &'static [&'static str];

/// One write for [`fe_config::write_settings`]: `None` removes the key.
pub(crate) type Write<'a> = (Key, Option<SettingValue<'a>>);

const THEME_MODE: Key = &["theme", "mode"];
const THEME_DARK: Key = &["theme", "dark"];
const THEME_LIGHT: Key = &["theme", "light"];
const THEME_NAME: Key = &["theme", "name"];
const FONT_FAMILY: Key = &["editor", "font_family"];
const FONT_SIZE: Key = &["editor", "font_size"];
const FONT_LIGATURES: Key = &["editor", "font_ligatures"];
const VIM_ENABLED: Key = &["vim", "enabled"];
const VIM_PRESET: Key = &["vim", "preset"];
const LINE_NUMBERS: Key = &["editor", "line_numbers"];
const FORMAT_ON_SAVE: Key = &["editor", "format_on_save"];
const AUTO_SAVE: Key = &["editor", "auto_save"];
const INLINE_DIAGNOSTICS: Key = &["editor", "inline_diagnostics"];
const MINIMAP: Key = &["editor", "minimap", "enabled"];
const INLAY_HINTS: Key = &["editor", "inlay_hints"];
const SOFT_WRAP: Key = &["editor", "soft_wrap"];

/// The font sizes offered: the schema's 6 to 72 is more than a list wants.
const FONT_SIZES: std::ops::RangeInclusive<u8> = 10..=24;

/// What a value not offered shows as, where showing it would mean nothing.
const CUSTOM: &str = "Custom";

/// A page of the wizard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Appearance,
    Font,
    Keyboard,
    Editing,
    Ready,
}

/// The pages, in order.
pub(crate) fn steps() -> [Step; 5] {
    [
        Step::Appearance,
        Step::Font,
        Step::Keyboard,
        Step::Editing,
        Step::Ready,
    ]
}

impl Step {
    /// The page's heading.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Step::Appearance => "Appearance",
            Step::Font => "Font",
            Step::Keyboard => "Keyboard",
            Step::Editing => "Editing",
            Step::Ready => "Ready",
        }
    }

    /// The line under the heading.
    pub(crate) fn subtitle(self) -> &'static str {
        match self {
            Step::Appearance => "Choose the colors Tenmoku shows.",
            Step::Font => "Choose how text looks in the editor.",
            Step::Keyboard => "Choose the keys you edit with.",
            Step::Editing => "Choose what the editor does for you.",
            Step::Ready => "That's everything Tenmoku needs to start.",
        }
    }

    /// What the page asks, given the choices so far: another choice can hide
    /// a control.
    pub(crate) fn controls(self, choices: &Choices) -> Vec<Control> {
        let ids: &[ControlId] = match self {
            Step::Appearance => &[
                ControlId::ThemeMode,
                ControlId::DarkTheme,
                ControlId::LightTheme,
            ],
            Step::Font => &[
                ControlId::FontFamily,
                ControlId::FontSize,
                ControlId::FontLigatures,
            ],
            Step::Keyboard => &[
                ControlId::Keyboard,
                ControlId::VimPreset,
                ControlId::LineNumbers,
            ],
            Step::Editing => &[
                ControlId::FormatOnSave,
                ControlId::AutoSave,
                ControlId::InlineDiagnostics,
                ControlId::Minimap,
                ControlId::InlayHints,
                ControlId::SoftWrap,
            ],
            Step::Ready => &[],
        };
        (ids.iter().copied())
            .filter(|id| choices.is_listed(*id))
            .map(|id| choices.control(id))
            .collect()
    }
}

/// A question the wizard asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlId {
    ThemeMode,
    DarkTheme,
    LightTheme,
    FontFamily,
    FontSize,
    FontLigatures,
    Keyboard,
    VimPreset,
    LineNumbers,
    FormatOnSave,
    AutoSave,
    InlineDiagnostics,
    Minimap,
    InlayHints,
    SoftWrap,
}

impl ControlId {
    /// The setting it writes, and its title and one-line description.
    fn describe(self) -> (Key, &'static str, &'static str) {
        match self {
            ControlId::ThemeMode => (
                THEME_MODE,
                "Mode",
                "Stay dark or light, or follow the system's appearance.",
            ),
            ControlId::DarkTheme => (
                THEME_DARK,
                "Dark theme",
                "The colors to use when the appearance is dark.",
            ),
            ControlId::LightTheme => (
                THEME_LIGHT,
                "Light theme",
                "The colors to use when the appearance is light.",
            ),
            ControlId::FontFamily => (
                FONT_FAMILY,
                "Font family",
                "The font text is shown in, in the editor and the terminal.",
            ),
            ControlId::FontSize => (FONT_SIZE, "Font size", "The size of text, in points."),
            ControlId::FontLigatures => (
                FONT_LIGATURES,
                "Ligatures",
                "Join character sequences such as -> into one shape, if the font has them.",
            ),
            ControlId::Keyboard => (
                VIM_ENABLED,
                "Keys",
                "Tenmoku's standard VS Code-style keys, or Vim's modes and motions.",
            ),
            ControlId::VimPreset => (
                VIM_PRESET,
                "Vim keys",
                "Which maps Vim starts with, on top of your own.",
            ),
            ControlId::LineNumbers => (
                LINE_NUMBERS,
                "Line numbers",
                "Number rows absolutely, or by their distance from the caret.",
            ),
            ControlId::FormatOnSave => (
                FORMAT_ON_SAVE,
                "Format on save",
                "Have the language server format a file as it's saved.",
            ),
            ControlId::AutoSave => (
                AUTO_SAVE,
                "Auto save",
                "Save changed files without being asked.",
            ),
            ControlId::InlineDiagnostics => (
                INLINE_DIAGNOSTICS,
                "Inline diagnostics",
                "Which problems show their message at the end of their row.",
            ),
            ControlId::Minimap => (
                MINIMAP,
                "Minimap",
                "Draw the whole file small beside the text.",
            ),
            ControlId::InlayHints => (
                INLAY_HINTS,
                "Inlay hints",
                "Show the types and parameter names a language server infers.",
            ),
            ControlId::SoftWrap => (
                SOFT_WRAP,
                "Soft wrap",
                "Wrap long rows at the edge of the editor rather than scrolling.",
            ),
        }
    }

    /// The appearance whose theme it chooses, if it does.
    fn theme_appearance(self) -> Option<Appearance> {
        match self {
            ControlId::DarkTheme => Some(Appearance::Dark),
            ControlId::LightTheme => Some(Appearance::Light),
            _ => None,
        }
    }
}

/// A question as it stands now, for the sheet to show. Its title, description
/// and labels are also its VoiceOver labels.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Control {
    pub(crate) id: ControlId,
    pub(crate) key: Key,
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
    pub(crate) options: Vec<ControlOption>,
    /// The option showing now, as an index into `options`.
    pub(crate) selected: usize,
}

impl Control {
    /// The setting it writes as the docs spell it: `theme.mode`.
    pub(crate) fn dotted_key(&self) -> String {
        self.key.join(".")
    }
}

/// One answer to a [`Control`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ControlOption {
    pub(crate) label: String,
    value: Value,
}

impl ControlOption {
    fn new(label: impl Into<String>, value: Value) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }
}

/// What a key holds, as far as the wizard cares.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Unset,
    Bool(bool),
    /// A string, or an enum as the file spells it.
    Text(String),
    Number(f64),
}

impl Value {
    fn text(text: &str) -> Self {
        Value::Text(text.to_owned())
    }

    /// `value`, one of the settings' enums, as the file spells it.
    fn of_enum(value: impl Serialize) -> Self {
        let text = serde_json::to_value(value).ok();
        Value::text(text.as_ref().and_then(|text| text.as_str()).unwrap_or(""))
    }

    fn of_option(value: Option<&String>) -> Self {
        value.map_or(Value::Unset, |text| Value::Text(text.clone()))
    }

    /// As [`fe_config::write_settings`] takes it. A number stays a float, as
    /// `settings-template.toml` spells it, so it takes the place of the
    /// commented-out default there.
    fn setting(&self) -> Option<SettingValue<'_>> {
        match self {
            Value::Unset => None,
            Value::Bool(value) => Some(SettingValue::Bool(*value)),
            Value::Text(text) => Some(SettingValue::Str(text)),
            Value::Number(value) => Some(SettingValue::Float(*value)),
        }
    }
}

/// Every key the wizard can write, with what `settings` holds there.
fn values(settings: &Settings) -> BTreeMap<Key, Value> {
    let (theme, vim, editor) = (&settings.theme, &settings.vim, &settings.editor);
    BTreeMap::from([
        (THEME_MODE, Value::of_enum(theme.mode)),
        (THEME_DARK, Value::of_option(theme.dark.as_ref())),
        (THEME_LIGHT, Value::of_option(theme.light.as_ref())),
        (THEME_NAME, Value::of_option(theme.name.as_ref())),
        (FONT_FAMILY, Value::of_option(editor.font_family.as_ref())),
        (FONT_SIZE, Value::Number(editor.font_size)),
        (FONT_LIGATURES, Value::Bool(editor.font_ligatures)),
        (VIM_ENABLED, Value::Bool(vim.enabled)),
        (VIM_PRESET, Value::of_enum(vim.preset)),
        (LINE_NUMBERS, Value::of_enum(editor.line_numbers)),
        (FORMAT_ON_SAVE, Value::Bool(editor.format_on_save)),
        (AUTO_SAVE, Value::of_enum(editor.auto_save)),
        (
            INLINE_DIAGNOSTICS,
            Value::of_enum(editor.inline_diagnostics),
        ),
        (MINIMAP, Value::Bool(editor.minimap.enabled)),
        (INLAY_HINTS, Value::Bool(editor.inlay_hints)),
        (SOFT_WRAP, Value::Bool(editor.soft_wrap)),
    ])
}

/// The ID of the theme an unset slot is: Yohen or Hakuji.
fn own_theme_id(appearance: Appearance) -> String {
    kiln::extensions::builtin_theme_id(match appearance {
        Appearance::Dark => themes::DEFAULT_DARK,
        Appearance::Light => themes::DEFAULT_LIGHT,
    })
}

/// The options that spell each of `values` as the file does.
fn listed(values: &[(&str, &str)]) -> Vec<ControlOption> {
    (values.iter())
        .map(|(value, label)| ControlOption::new(*label, Value::text(value)))
        .collect()
}

fn toggle(off: &str, on: &str) -> Vec<ControlOption> {
    vec![
        ControlOption::new(off, Value::Bool(false)),
        ControlOption::new(on, Value::Bool(true)),
    ]
}

/// The list for `appearance`: its bundled themes, by the ID kiln selects them
/// by, the one an unset slot is by being unset.
fn theme_options(appearance: Appearance) -> Vec<ControlOption> {
    let own = own_theme_id(appearance);
    (themes::bundled_themes().iter())
        .filter(|theme| theme.appearance == appearance)
        .map(|theme| {
            let id = kiln::extensions::builtin_theme_id(theme.id);
            let value = if id == own {
                Value::Unset
            } else {
                Value::Text(id)
            };
            ControlOption::new(theme.title, value)
        })
        .collect()
}

/// [`FONT_SIZES`], with `shown` among them in order if it's outside them.
fn size_options(shown: &Value) -> Vec<ControlOption> {
    let mut sizes: Vec<f64> = FONT_SIZES.map(f64::from).collect();
    if let Value::Number(shown) = *shown
        && !sizes.contains(&shown)
    {
        let at = sizes.partition_point(|size| *size < shown);
        sizes.insert(at, shown);
    }
    (sizes.into_iter())
        .map(|size| ControlOption::new(size.to_string(), Value::Number(size)))
        .collect()
}

/// The first of [`fonts::MONO_FAMILIES`] among `installed`, as an unset
/// `editor.font_family` is: else this platform's last resort.
fn default_font(installed: &[&str]) -> String {
    (fonts::MONO_FAMILIES.iter())
        .find(|family| installed.contains(family))
        .copied()
        .unwrap_or(fonts::mono_last_resort(Os::current()))
        .to_owned()
}

/// Every answer so far, from the settings the wizard started from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Choices {
    now: BTreeMap<Key, Value>,
    started: BTreeMap<Key, Value>,
    installed_fonts: Vec<String>,
    default_font: String,
}

impl Choices {
    /// What each control shows given `settings` and the font families the
    /// text system has.
    pub(crate) fn from_settings(settings: &Settings, installed_fonts: &[&str]) -> Self {
        let now = values(settings);
        Self {
            started: now.clone(),
            now,
            installed_fonts: (installed_fonts.iter())
                .map(|family| (*family).to_owned())
                .collect(),
            default_font: default_font(installed_fonts),
        }
    }

    fn value(&self, key: Key) -> &Value {
        self.now.get(key).unwrap_or(&Value::Unset)
    }

    /// Whether the control shows given the other choices: the Vim-only two,
    /// and the theme list of an appearance the mode never shows, don't.
    fn is_listed(&self, id: ControlId) -> bool {
        let mode_is = |mode: &str| *self.value(THEME_MODE) == Value::text(mode);
        match id {
            ControlId::DarkTheme => !mode_is("light"),
            ControlId::LightTheme => !mode_is("dark"),
            ControlId::VimPreset | ControlId::LineNumbers => {
                *self.value(VIM_ENABLED) == Value::Bool(true)
            }
            _ => true,
        }
    }

    /// The bundled theme `theme.name` names, if it's of `appearance`.
    fn named(&self, appearance: Appearance) -> Option<&'static themes::BundledTheme> {
        match self.value(THEME_NAME) {
            Value::Text(name) => themes::find(name).filter(|theme| theme.appearance == appearance),
            _ => None,
        }
    }

    /// Whether `theme.name` has a say in the `appearance` slot, so choosing a
    /// theme for that slot retires it: it names a theme of that appearance,
    /// or none at all. One of the other appearance stays what shows there.
    fn name_gives_way(&self, appearance: Appearance) -> bool {
        match self.value(THEME_NAME) {
            Value::Text(name) => {
                themes::find(name).is_none_or(|theme| theme.appearance == appearance)
            }
            _ => false,
        }
    }

    /// What the control shows now, in terms of its options' values. A slot
    /// shows its setting, else the theme `theme.name` gives it, else the
    /// bundled one an unset slot is.
    fn shown(&self, key: Key, id: ControlId) -> Value {
        let Some(appearance) = id.theme_appearance() else {
            return self.value(key).clone();
        };
        let own = own_theme_id(appearance);
        let theme = match self.value(key) {
            Value::Text(theme) => theme.clone(),
            _ => match self.named(appearance) {
                Some(theme) => kiln::extensions::builtin_theme_id(theme.id),
                None => own.clone(),
            },
        };
        if theme == own {
            Value::Unset
        } else {
            Value::Text(theme)
        }
    }

    fn control(&self, id: ControlId) -> Control {
        let (key, title, description) = id.describe();
        let shown = self.shown(key, id);
        let mut options = match id {
            ControlId::ThemeMode => listed(&[
                ("dark", "Dark"),
                ("light", "Light"),
                ("system", "Match System"),
            ]),
            ControlId::DarkTheme => theme_options(Appearance::Dark),
            ControlId::LightTheme => theme_options(Appearance::Light),
            ControlId::FontFamily => {
                let default =
                    ControlOption::new(format!("Default ({})", self.default_font), Value::Unset);
                let installed = (self.installed_fonts.iter())
                    .map(|family| ControlOption::new(family, Value::text(family)));
                std::iter::once(default).chain(installed).collect()
            }
            ControlId::FontSize => size_options(&shown),
            ControlId::Keyboard => toggle("Standard", "Vim"),
            ControlId::VimPreset => listed(&[
                ("none", "Plain vim"),
                ("lazyvim", "LazyVim, Space as leader"),
            ]),
            ControlId::LineNumbers => {
                listed(&[("on", "On"), ("relative", "Relative"), ("hybrid", "Hybrid")])
            }
            ControlId::AutoSave => listed(&[
                ("off", "Off"),
                ("after_delay", "After a pause"),
                ("on_focus_change", "When focus leaves"),
            ]),
            ControlId::InlineDiagnostics => listed(&[
                ("off", "Off"),
                ("error", "Errors"),
                ("warning", "Errors and warnings"),
            ]),
            ControlId::FontLigatures
            | ControlId::FormatOnSave
            | ControlId::Minimap
            | ControlId::InlayHints
            | ControlId::SoftWrap => toggle("Off", "On"),
        };
        let selected =
            (options.iter().position(|option| option.value == shown)).unwrap_or_else(|| {
                // A theme or font not offered means something by its name.
                let label = match &shown {
                    Value::Text(text)
                        if id == ControlId::FontFamily || id.theme_appearance().is_some() =>
                    {
                        text.clone()
                    }
                    _ => CUSTOM.to_owned(),
                };
                options.push(ControlOption::new(label, shown));
                options.len() - 1
            });
        Control {
            id,
            key,
            title,
            description,
            options,
            selected,
        }
    }

    /// Takes the `option`th of the control's options as it's now listed, and
    /// returns the writes that makes: a value equal to the default's removes
    /// its key. Choosing what's already shown, or a control or option not
    /// listed now, writes nothing. Changing `theme.mode` leaves the themes
    /// be; a theme writes its slot and retires a `theme.name` it overrides,
    /// which would otherwise warn on every load.
    pub(crate) fn choose(&mut self, id: ControlId, option: usize) -> Vec<Write<'_>> {
        if !self.is_listed(id) {
            return Vec::new();
        }
        let control = self.control(id);
        if option == control.selected {
            return Vec::new();
        }
        let Some(option) = control.options.into_iter().nth(option) else {
            return Vec::new();
        };
        let mut changes = vec![(control.key, option.value)];
        if let Some(appearance) = id.theme_appearance()
            && self.name_gives_way(appearance)
        {
            changes.push((THEME_NAME, Value::Unset));
        }
        // The bundled tile for an unset slot `theme.name` chose: only the
        // name goes, as the slot has no key to remove.
        changes.retain(|(key, value)| self.value(key) != value);
        let keys: Vec<Key> = changes.iter().map(|(key, _)| *key).collect();
        self.now.extend(changes);
        self.writes_of(&keys)
    }

    /// The writes that put `keys` in the file as they are here: a value equal
    /// to the default's removes its key. On a clone, it's what a write made
    /// off the main thread takes, as [`Write`] borrows its text.
    pub(crate) fn writes_of(&self, keys: &[Key]) -> Vec<Write<'_>> {
        let defaults = values(&Settings::default());
        (keys.iter())
            .map(|key| {
                let value = self.value(key);
                let value = if defaults.get(key) == Some(value) {
                    None
                } else {
                    value.setting()
                };
                (*key, value)
            })
            .collect()
    }

    /// Takes what `keys` hold in `other`: a write that failed, back to what
    /// the file has.
    pub(crate) fn adopt(&mut self, other: &Choices, keys: &[Key]) {
        for key in keys {
            self.now.insert(*key, other.value(key).clone());
        }
    }

    /// How many keys hold something other than what the run started with.
    pub(crate) fn changed_count(&self) -> usize {
        (self.now.iter())
            .filter(|(key, value)| self.started.get(*key) != Some(*value))
            .count()
    }

    /// What the run changed, and where to change it, in a sentence. The Ready
    /// page has its own copy, with the shortcut.
    #[cfg(test)]
    pub(crate) fn summary(&self) -> String {
        let changed = match self.changed_count() {
            0 => "No settings changed.".to_owned(),
            1 => "1 setting changed.".to_owned(),
            count => format!("{count} settings changed."),
        };
        format!("{changed} You can change any of them later with Open Settings (UI).")
    }
}
