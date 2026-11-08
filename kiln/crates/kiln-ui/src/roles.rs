// SPDX-License-Identifier: Apache-2.0

//! The design's type, size, spacing and radius roles as named constants
//! (`docs/design/design-system.md#type-spacing-and-sizes`). Nothing adopts
//! them yet; widgets switch over one card at a time.

use gpui::{App, Global, Pixels, SharedString, px};

/// Which of [`FontFamilies`]' families a [`TextRole`] is set in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FamilySlot {
    Ui,
    Code,
    Title,
}

/// One row of the design's type table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextRole {
    pub family: FamilySlot,
    pub size: Pixels,
    pub line_height: Pixels,
    pub weight: u16,
}

impl TextRole {
    const fn new(family: FamilySlot, size: f32, line_height: f32, weight: u16) -> Self {
        Self {
            family,
            size: px(size),
            line_height: px(line_height),
            weight,
        }
    }
}

/// Type table: page title, 30/36, weight 400, in the title family.
pub const PAGE_TITLE: TextRole = TextRole::new(FamilySlot::Title, 30., 36., 400);
/// Type table: section heading, 15/20, weight 600.
pub const SECTION_HEADING: TextRole = TextRole::new(FamilySlot::Ui, 15., 20., 600);
/// Type table: UI heading, 13/18, weight 600.
pub const UI_HEADING: TextRole = TextRole::new(FamilySlot::Ui, 13., 18., 600);
/// Type table: UI body, 13/18, weight 400.
pub const UI_BODY: TextRole = TextRole::new(FamilySlot::Ui, 13., 18., 400);
/// Type table: UI label, 12/16, weight 500.
pub const UI_LABEL: TextRole = TextRole::new(FamilySlot::Ui, 12., 16., 500);
/// Type table: UI small, 11/14, weight 500.
pub const UI_SMALL: TextRole = TextRole::new(FamilySlot::Ui, 11., 14., 500);
/// Type table: code, 13/20, weight 400, in the code family.
pub const CODE: TextRole = TextRole::new(FamilySlot::Code, 13., 20., 400);
/// Type table: small code, 12/18, weight 400, in the code family.
pub const CODE_SMALL: TextRole = TextRole::new(FamilySlot::Code, 12., 18., 400);

/// The families the [`FamilySlot`]s resolve to. A host sets it as a GPUI
/// global. `None` for `code` means today's choice, `kiln_platform`'s
/// `mono_family(cx)` (the first installed monospace family), which needs an
/// `App` to look up, so it's resolved by [`FontFamilies::family`]. The
/// default is today's: the system UI font for `ui` and `title`, and that
/// monospace choice for `code`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFamilies {
    pub ui: SharedString,
    pub code: Option<SharedString>,
    pub title: SharedString,
}

impl FontFamilies {
    /// The family `slot` names, resolving an unset `code` on `cx`.
    pub fn family(&self, slot: FamilySlot, cx: &App) -> SharedString {
        match slot {
            FamilySlot::Ui => self.ui.clone(),
            FamilySlot::Code => self
                .code
                .clone()
                .unwrap_or_else(|| kiln_platform::fonts::mono_family(cx)),
            FamilySlot::Title => self.title.clone(),
        }
    }
}

impl Default for FontFamilies {
    fn default() -> Self {
        Self {
            ui: SYSTEM_UI_FONT.into(),
            code: None,
            title: SYSTEM_UI_FONT.into(),
        }
    }
}

impl Global for FontFamilies {}

/// GPUI's name for the platform's UI font, the family widgets inherit today.
const SYSTEM_UI_FONT: &str = ".SystemUIFont";

/// Spacing scale: the design's eight steps.
pub mod space {
    use gpui::{Pixels, px};
    /// Spacing scale: 2.
    pub const S2: Pixels = px(2.);
    /// Spacing scale: 4.
    pub const S4: Pixels = px(4.);
    /// Spacing scale: 6.
    pub const S6: Pixels = px(6.);
    /// Spacing scale: 8.
    pub const S8: Pixels = px(8.);
    /// Spacing scale: 12.
    pub const S12: Pixels = px(12.);
    /// Spacing scale: 16.
    pub const S16: Pixels = px(16.);
    /// Spacing scale: 24.
    pub const S24: Pixels = px(24.);
    /// Spacing scale: 40.
    pub const S40: Pixels = px(40.);
}

/// Radius table.
pub mod radius {
    use gpui::{Pixels, px};
    /// Radius table: fills, 2.
    pub const FILL: Pixels = px(2.);
    /// Radius table: controls, 4.
    pub const CONTROL: Pixels = px(4.);
    /// Radius table: menus, 6.
    pub const MENU: Pixels = px(6.);
    /// Radius table: floating surfaces, 10.
    pub const FLOAT: Pixels = px(10.);
}

/// Heights table.
pub mod height {
    use gpui::{Pixels, px};
    /// Heights table: title bar, 38.
    pub const TITLE_BAR: Pixels = px(38.);
    /// Heights table: tab strip, 34.
    pub const TAB_STRIP: Pixels = px(34.);
    /// Heights table: list and tree row, 24.
    pub const ROW: Pixels = px(24.);
    /// Heights table: menu row, 28.
    pub const MENU_ROW: Pixels = px(28.);
    /// Heights table: status bar, 24.
    pub const STATUS_BAR: Pixels = px(24.);
    /// Heights table: control, 26.
    pub const CONTROL: Pixels = px(26.);
    /// Heights table: small control, 22.
    pub const CONTROL_SMALL: Pixels = px(22.);
    /// Heights table: activity rail item, 32.
    pub const RAIL_ITEM: Pixels = px(32.);
}

/// Widths table.
pub mod width {
    use gpui::{Pixels, px};
    /// Widths table: activity rail, 42.
    pub const RAIL: Pixels = px(42.);
    /// Widths table: sidebar minimum, 232.
    pub const SIDEBAR_MIN: Pixels = px(232.);
    /// Widths table: sidebar default, 248.
    pub const SIDEBAR_DEFAULT: Pixels = px(248.);
}

/// Widths table: bottom panel default height, 250.
pub const BOTTOM_PANEL_DEFAULT: Pixels = px(250.);

/// A drop shadow: offset, blur and the two black layers' opacities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub y: Pixels,
    pub blur: Pixels,
    /// Opacity of the wide, soft layer's black.
    pub strong: f32,
    /// Opacity of the second layer's black.
    pub weak: f32,
}

/// Shadow row: floating surfaces, 0 12 32 at 55% / 14% black.
pub const SHADOW_FLOAT: Shadow = Shadow {
    y: px(12.),
    blur: px(32.),
    strong: 0.55,
    weak: 0.14,
};

/// A focus ring: stroke width and gap from the element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusRing {
    pub width: Pixels,
    pub offset: Pixels,
}

/// Focus row: a 2 px ring, offset 1 px.
pub const FOCUS_RING: FocusRing = FocusRing {
    width: px(2.),
    offset: px(1.),
};
