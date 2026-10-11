// SPDX-License-Identifier: LicenseRef-Proprietary

//! The user's theme file, as a source for kiln-platform's config service:
//! the facade's, as only it sees both the service and kiln-ui's themes.

use std::path::PathBuf;

use gpui::App;
use kiln_core::{
    Diagnostic,
    theme::{Appearance, ThemeData, ThemeFile},
};
use kiln_platform::config::{ConfigSource, log_target};
use kiln_ui::{GlobalTheme, Theme, ThemeMode, set_themes};

/// The user's theme file ([`crate::Config::user_theme`]). Laid over the
/// bundled base for its appearance, it takes that appearance's place in
/// the dark and light pair, so the theme mode still picks between them.
/// A file with errors keeps the last good version; a missing file puts
/// back the theme it replaced.
pub(crate) struct UserTheme {
    path: PathBuf,
    /// The theme last read without errors, while the file exists.
    theme: Option<Theme>,
    /// The slot the theme went into, and the theme that was there.
    replaced: Option<(Appearance, Theme)>,
    diagnostics: Vec<Diagnostic>,
}

impl UserTheme {
    pub(crate) fn load(path: PathBuf) -> Self {
        let mut theme = Self {
            path,
            theme: None,
            replaced: None,
            diagnostics: Vec::new(),
        };
        theme.read();
        theme
    }

    /// Re-reads the file. Returns whether the theme changed.
    fn read(&mut self) -> bool {
        // Only a file that's there is the user's theme: no file is none,
        // with nothing to report.
        if !self.path.exists() {
            self.diagnostics.clear();
            return self.theme.take().is_some();
        }
        let (file, diagnostics) = ThemeFile::load(&self.path, &[]);
        self.diagnostics = diagnostics;
        let Some(file) = file else {
            return false;
        };
        let theme = Theme::new(ThemeData::from_file(&file));
        if self.theme.as_ref() == Some(&theme) {
            return false;
        }
        self.theme = Some(theme);
        true
    }

    /// Puts the theme into its slot of the pair in use, in the mode in
    /// use, after putting back whatever an earlier version replaced.
    fn apply(&mut self, cx: &mut App) {
        let global = cx.try_global::<GlobalTheme>();
        let mode = global
            .and_then(GlobalTheme::mode)
            .unwrap_or(ThemeMode::System);
        let (mut dark, mut light) = global
            .and_then(GlobalTheme::themes)
            .map(|(dark, light)| (dark.clone(), light.clone()))
            .unwrap_or_else(|| (Theme::dark(), Theme::light()));
        if let Some((appearance, replaced)) = self.replaced.take() {
            *slot(appearance, &mut dark, &mut light) = replaced;
        }
        if let Some(theme) = &self.theme {
            let slot = slot(theme.appearance, &mut dark, &mut light);
            let previous = std::mem::replace(slot, theme.clone());
            self.replaced = Some((theme.appearance, previous));
        }
        set_themes(cx, mode, dark, light);
    }
}

/// The pair's theme for `appearance`.
fn slot<'a>(appearance: Appearance, dark: &'a mut Theme, light: &'a mut Theme) -> &'a mut Theme {
    match appearance {
        Appearance::Dark => dark,
        Appearance::Light => light,
    }
}

impl ConfigSource for UserTheme {
    fn paths(&self) -> Vec<PathBuf> {
        vec![self.path.clone()]
    }

    fn user_files(&self) -> Vec<PathBuf> {
        self.paths()
    }

    fn install(&mut self, cx: &mut App) {
        if self.theme.is_some() {
            self.apply(cx);
        }
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        if changed.contains(&self.path) && self.read() {
            log::info!(target: log_target(cx), "theme reloaded");
            self.apply(cx);
        }
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.clone()
    }
}
