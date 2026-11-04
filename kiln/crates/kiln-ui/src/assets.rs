// SPDX-License-Identifier: LicenseRef-Proprietary

//! kiln-ui's bundled assets, served to GPUI.

use std::{
    borrow::Cow,
    collections::BTreeMap,
    sync::{
        LazyLock, PoisonError, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

use gpui::{AssetSource, SharedString};

use crate::{Icon, file_icons_coloured};

/// Coloured icons registered with [`Assets::register_coloured`], by path.
/// Process-wide, not per app: GPUI loads assets on background threads.
static COLOURED: LazyLock<RwLock<BTreeMap<&'static str, &'static [u8]>>> =
    LazyLock::new(Default::default);
/// Bumped each time a registration adds a path or changes one's file, so
/// images drawn from the old files are drawn again.
static COLOURED_REVISION: AtomicU64 = AtomicU64::new(0);

/// Serves kiln-ui's icons at `kiln-ui/icons/<name>.svg` (see [`Icon::path`]),
/// [`ColouredIcons`](crate::ColouredIcons)' at `kiln-ui/file-icons/<stem>.svg`,
/// and the coloured icons registered with [`Assets::register_coloured`] at
/// their paths. The files are compiled in, so nothing is read from disk.
///
/// Register it with `Application::with_assets(kiln_ui::Assets)`. A host that
/// already has its own `AssetSource` must delegate the paths it doesn't know
/// to `kiln_ui::Assets`, or `svg()` elements using these icons draw nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl Assets {
    /// Serves each `(path, svg)` from now on, for
    /// [`IconRef::Coloured`](crate::IconRef::Coloured) icons to draw. A path
    /// registered again takes the later file; a bundled icon's path, an
    /// [`Icon`]'s or a [`ColouredIcons`](crate::ColouredIcons) one's, still
    /// serves the bundled one. An icon drawn from an earlier file,
    /// or missing until now, draws from the new one the next time it's
    /// drawn.
    pub fn register_coloured(icons: &[(&'static str, &'static [u8])]) {
        let mut coloured = COLOURED.write().unwrap_or_else(PoisonError::into_inner);
        let mut changed = false;
        for &(path, svg) in icons {
            changed |= coloured.insert(path, svg).is_none_or(|old| old != svg);
        }
        if changed {
            COLOURED_REVISION.fetch_add(1, Ordering::Release);
        }
    }

    /// Changes with each registration that adds or changes a coloured icon.
    pub(crate) fn coloured_revision() -> u64 {
        COLOURED_REVISION.load(Ordering::Acquire)
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if let Some(icon) = Icon::ALL.iter().find(|icon| icon.path() == path) {
            return Ok(Some(Cow::Borrowed(icon.svg())));
        }
        if let Some(svg) = file_icons_coloured::bundled(path) {
            return Ok(Some(Cow::Borrowed(svg)));
        }
        let coloured = COLOURED.read().unwrap_or_else(PoisonError::into_inner);
        Ok(coloured.get(path).map(|svg| Cow::Borrowed(*svg)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let coloured = COLOURED.read().unwrap_or_else(PoisonError::into_inner);
        Ok(Icon::ALL
            .iter()
            .map(|icon| icon.path())
            .chain(file_icons_coloured::FILES.iter().map(|&(_, path, _)| path))
            .chain(coloured.keys().copied())
            .filter(|p| p.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}
