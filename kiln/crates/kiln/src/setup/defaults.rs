// SPDX-License-Identifier: Apache-2.0

//! kiln's bundled keymap and menus, under an app's own, and the check that
//! makes a mistake in the app's own fail fast: [`Config::default_keys`],
//! [`Config::default_menus`] and [`Config::fail_fast`].

use std::{borrow::Cow, fmt::Write as _, path::Path};

use kiln_core::{CommandCatalog, Diagnostic, ItemKind, KeymapLayer, Menu, MenuBar, MenuItem, Os};

use super::Config;

/// The path the bundled keymap's diagnostics and bindings name.
const KEYMAP_PATH: &str = "<kiln keymap>";

/// The path the bundled menus' diagnostics name.
const MENUS_PATH: &str = "<kiln menus>";

const KEYMAP: &str = include_str!("keymap.toml");

/// Its titles are keys in kiln's catalogue, `$app` in them the app's name;
/// its first menu, the app menu, is named for the app by [`name_app_menu`].
const MENUS: &str = include_str!("menus.toml");

impl Config {
    /// Lays kiln's own keys under the app's keymap: the command palette
    /// (`secondary-shift-p`, anywhere), Quit (`secondary-q`), F10 for the
    /// menus drawn in the window (not on macOS), and the keys of
    /// kiln-shell's picker and its text field, which the shell leaves to
    /// the app. The app's keymap overrides them key by key, and the
    /// user's overrides both. Off by default, so the keymap is only the
    /// app's and the user's; `kiln::app` turns it on.
    pub fn default_keys(mut self, on: bool) -> Self {
        self.default_keys = on;
        self
    }

    /// Installs kiln's own menus when the app gives no menu table
    /// ([`Config::menus`]): an app menu named after the app (Hide, Hide
    /// Others and Show All on macOS, then Quit), a View menu (Command
    /// Palette, Toggle Full Screen) and a Window menu (Minimize, Zoom,
    /// Close Window). An app's table replaces them whole. Off by default,
    /// so an app without a table has no menus; `kiln::app` turns it on.
    pub fn default_menus(mut self, on: bool) -> Self {
        self.default_menus = on;
        self
    }

    /// Makes [`crate::init`] panic when the app's own keymap or menus, or
    /// kiln's bundled ones, have any diagnostic, an error or a warning
    /// (such as an unknown command), listing each with its file and line:
    /// a mistake in what the app ships shows the first time it runs. The
    /// user's files never panic; one with errors keeps its last good
    /// version, as always. Off by default, so `init` returns the
    /// diagnostics and logs them; `kiln::app` turns it on.
    pub fn fail_fast(mut self, on: bool) -> Self {
        self.fail_fast = on;
        self
    }

    /// The keymap layers the app ships, as on `os`, their bindings checked
    /// against `catalog` if given: kiln's bundled keys if
    /// [`Config::default_keys`] is on, then `contributed` (the contributed
    /// commands' default keys), then the app's. With their diagnostics,
    /// which a layer with errors is left out over.
    pub(crate) fn own_keymap(
        &self,
        os: Os,
        catalog: Option<&dyn CommandCatalog>,
        contributed: Option<KeymapLayer>,
    ) -> (Vec<KeymapLayer>, Vec<Diagnostic>) {
        let mut layers = Vec::new();
        let mut diagnostics = Vec::new();
        let parse = |path: &Path, text: &str| KeymapLayer::parse_for(path, text, catalog, os);
        let bundled = self
            .default_keys
            .then(|| parse(Path::new(KEYMAP_PATH), KEYMAP));
        let app = self
            .keymap
            .as_ref()
            .map(|source| parse(&source.path, &source.text));
        let contributed = contributed.map(|layer| (Some(layer), Vec::new()));
        for (layer, found) in bundled.into_iter().chain(contributed).chain(app) {
            layers.extend(layer);
            diagnostics.extend(found);
        }
        (layers, diagnostics)
    }

    /// Gives keyed menu titles the app's name as `$app`
    /// ([`kiln_core::menu::set_label_arg`]): before any menu file is parsed,
    /// the app's, kiln's bundled one or an extension's, so the en-US titles
    /// it fills in, and every label shown later, name the app. Every path
    /// to the menus calls it first: [`Config::menu_source`], and the shipped
    /// menus' whatever their table.
    pub(crate) fn name_the_app_in_menus(&self) {
        kiln_core::menu::set_label_arg("app", self.app_name.clone());
    }

    /// The menu table the app shows and the path its diagnostics name: the
    /// app's, or else kiln's bundled one if [`Config::default_menus`] is
    /// on. The bundled bar, once parsed, is then [`name_app_menu`]'s to
    /// finish. Names the app in menu titles first
    /// ([`Config::name_the_app_in_menus`]), as the table is parsed next.
    pub(super) fn menu_source(&self) -> Option<(&Path, Cow<'_, str>)> {
        self.name_the_app_in_menus();
        match &self.menus {
            Some(source) => Some((&source.path, Cow::Borrowed(&*source.text))),
            None if self.default_menus => Some((Path::new(MENUS_PATH), Cow::Borrowed(MENUS))),
            None => None,
        }
    }

    /// Panics with every one of `diagnostics` if [`Config::fail_fast`] is
    /// on and there are any: those of the keymap and menus the app ships.
    pub(crate) fn check_fail_fast(&self, diagnostics: &[Diagnostic]) {
        if !self.fail_fast || diagnostics.is_empty() {
            return;
        }
        let mut message = format!(
            "{} has {} problem(s) in the keymap and menus it ships \
             (`kiln::Config::fail_fast` is on):",
            self.app_name,
            diagnostics.len()
        );
        for diagnostic in diagnostics {
            write!(message, "\n  {diagnostic}").expect("writing to a String");
        }
        panic!("{message}");
    }
}

/// Names the bundled bar's app menu, the one `menus.toml` calls `App`, for
/// the app, in every locale: its ID and title become the app's name. Menus
/// are known by their IDs, each its en-US name, so a bundled menu the app
/// shares its name with, as an app called `Window` does, is folded into the app
/// menu after a separator rather than shown twice. A catalog that knows none
/// of the app menu's commands drops it; the bar is then left as parsed, so
/// no other menu is renamed or folded.
pub(super) fn name_app_menu(bar: &mut MenuBar, app_name: &str) {
    let Some(app_ix) = bar.menus.iter().position(|menu| menu.id == "App") else {
        return;
    };
    if let Some(ix) = bar
        .menus
        .iter()
        .position(|menu| menu.id == app_name)
        .filter(|&ix| ix != app_ix)
    {
        let same = bar.menus.remove(ix);
        let app_ix = if ix < app_ix { app_ix - 1 } else { app_ix };
        let app = &mut bar.menus[app_ix];
        app.items.push(MenuItem {
            kind: ItemKind::Separator,
            place: None,
        });
        app.items.extend(same.items);
        name_for_app(app, app_name);
    } else {
        name_for_app(&mut bar.menus[app_ix], app_name);
    }
}

fn name_for_app(menu: &mut Menu, app_name: &str) {
    app_name.clone_into(&mut menu.id);
    app_name.clone_into(&mut menu.name);
    menu.title_key = None;
}

/// `text` as the inside of a TOML basic string: `"` and `\` escaped, and
/// control characters as `\uXXXX`.
pub(crate) fn toml_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            c if c.is_control() => {
                write!(escaped, "\\u{:04X}", u32::from(c)).expect("writing to a String");
            }
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kiln_core::{ItemKind, MenuBar, Os};

    use super::{MENUS, toml_escape};

    /// The bundled menus are all keyed, their keys in en-US: every item's
    /// title and every menu's but the app menu's, whose name is replaced by
    /// the app's. Parsed with no warning, as an unknown key would give.
    #[test]
    fn the_bundled_menus_are_keyed() {
        kiln_core::menu::set_label_arg("app", "Notes");
        for os in [Os::MacOs, Os::Linux, Os::Windows] {
            let (bar, diagnostics) = MenuBar::parse_for(Path::new("menus.toml"), MENUS, None, os);
            assert_eq!(diagnostics, [], "on {os:?}");
            let bar = bar.unwrap();
            assert_eq!(bar.menus[0].name, "App");
            for menu in &bar.menus[1..] {
                assert!(menu.title_key.is_some(), "{} on {os:?}", menu.name);
            }
            for item in bar.menus.iter().flat_map(|menu| &menu.items) {
                match &item.kind {
                    ItemKind::Command(command) => {
                        assert!(command.title_key.is_some(), "{} on {os:?}", command.title);
                    }
                    ItemKind::Separator => {}
                    kind => panic!("{kind:?} on {os:?}"),
                }
            }
        }
        for line in MENUS.lines() {
            assert!(!line.trim_start().starts_with("title "), "{line}");
        }
    }

    #[test]
    fn app_names_are_escaped_for_toml() {
        assert_eq!(toml_escape("Notes"), "Notes");
        assert_eq!(toml_escape(r#"A "B" \ C"#), r#"A \"B\" \\ C"#);
        assert_eq!(toml_escape("a\nb"), "a\\u000Ab");
    }
}
