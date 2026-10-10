// SPDX-License-Identifier: LicenseRef-Proprietary

//! kiln-ui's bundled assets, served to GPUI.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

use crate::Icon;

/// Serves kiln-ui's icons at `kiln-ui/icons/<name>.svg` (see [`Icon::path`]).
/// The files are compiled in, so nothing is read from disk.
///
/// Register it with `Application::with_assets(kiln_ui::Assets)`. A host that
/// already has its own `AssetSource` must delegate the paths it doesn't know
/// to `kiln_ui::Assets`, or `svg()` elements using these icons draw nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(Icon::ALL
            .iter()
            .find(|icon| icon.path() == path)
            .map(|icon| Cow::Borrowed(icon.svg())))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Icon::ALL
            .iter()
            .map(|icon| icon.path())
            .filter(|p| p.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}
