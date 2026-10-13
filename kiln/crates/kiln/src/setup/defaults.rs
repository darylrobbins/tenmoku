// SPDX-License-Identifier: LicenseRef-Proprietary

//! kiln's bundled keymap and menus, under an app's own, and the check that
//! makes a mistake in the app's own fail fast: [`Config::default_keys`],
//! [`Config::default_menus`] and [`Config::fail_fast`].

use std::{borrow::Cow, fmt::Write as _, path::Path};

use kiln_core::{CommandCatalog, Diagnostic, ItemKind, KeymapLayer, MenuBar, MenuItem, Os};

use super::Config;

/// The path the bundled keymap's diagnostics and bindings name.
const KEYMAP_PATH: &str = "<kiln keymap>";

/// The path the bundled menus' diagnostics name.
const MENUS_PATH: &str = "<kiln menus>";

const KEYMAP: &str = include_str!("keymap.toml");

/// `{app}` in its titles is the app's name; its first menu, the app
/// menu, is named for the app by [`name_app_menu`].
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
    /// [`Config::default_keys`] is on, then the app's. With their
    /// diagnostics, which a layer with errors is left out over.
    pub(super) fn own_keymap(
        &self,
        os: Os,
        catalog: Option<&dyn CommandCatalog>,
    ) -> (Vec<KeymapLayer>, Vec<Diagnostic>) {
        let mut layers = Vec::new();
        let mut diagnostics = Vec::new();
        let bundled = self.default_keys.then(|| (Path::new(KEYMAP_PATH), KEYMAP));
        let app = self
            .keymap
            .as_ref()
            .map(|source| (source.path.as_path(), &*source.text));
        for (path, text) in bundled.into_iter().chain(app) {
            let (layer, found) = KeymapLayer::parse_for(path, text, catalog, os);
            layers.extend(layer);
            diagnostics.extend(found);
        }
        (layers, diagnostics)
    }

    /// The menu table the app shows and the path its diagnostics name: the
    /// app's, or else kiln's bundled one if [`Config::default_menus`] is
    /// on, its titles naming the app. The bundled bar, once parsed, is
    /// then [`name_app_menu`]'s to finish.
    pub(super) fn menu_source(&self) -> Option<(&Path, Cow<'_, str>)> {
        match &self.menus {
            Some(source) => Some((&source.path, Cow::Borrowed(&*source.text))),
            None if self.default_menus => {
                let name = toml_escape(&self.app_name);
                Some((
                    Path::new(MENUS_PATH),
                    Cow::Owned(MENUS.replace("{app}", &name)),
                ))
            }
            None => None,
        }
    }

    /// Panics with every one of `diagnostics` if [`Config::fail_fast`] is
    /// on and there are any: those of the keymap and menus the app ships.
    pub(super) fn check_fail_fast(&self, diagnostics: &[Diagnostic]) {
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

/// Names the bundled bar's app menu, its first, for the app. Menus are
/// known by their names, so a bundled menu the app shares its name with,
/// as an app called `Window` does, is folded into the app menu after a
/// separator rather than shown twice.
pub(super) fn name_app_menu(bar: &mut MenuBar, app_name: &str) {
    if let Some(ix) = bar
        .menus
        .iter()
        .skip(1)
        .position(|menu| menu.name == app_name)
    {
        let same = bar.menus.remove(ix + 1);
        let app = &mut bar.menus[0];
        app.items.push(MenuItem {
            kind: ItemKind::Separator,
            place: None,
        });
        app.items.extend(same.items);
    }
    if let Some(app) = bar.menus.first_mut() {
        app_name.clone_into(&mut app.name);
    }
}

/// `text` as the inside of a TOML basic string: `"` and `\` escaped, and
/// control characters as `\uXXXX`.
fn toml_escape(text: &str) -> String {
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
    use super::toml_escape;

    #[test]
    fn app_names_are_escaped_for_toml() {
        assert_eq!(toml_escape("Notes"), "Notes");
        assert_eq!(toml_escape(r#"A "B" \ C"#), r#"A \"B\" \\ C"#);
        assert_eq!(toml_escape("a\nb"), "a\\u000Ab");
    }
}
