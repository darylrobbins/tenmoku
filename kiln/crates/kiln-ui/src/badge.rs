// SPDX-License-Identifier: Apache-2.0

//! [`Badge`]: a count or a dot on an icon's bottom-right corner.

use gpui::{
    App, Div, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, Tokens, controls::Control};

/// The cut-out ring's width: the badge's outline in the colour of the
/// surface it sits on, so it reads as cut out of the icon under it.
pub const BADGE_RING: Pixels = px(2.);

/// A dot's diameter, inside its ring.
const DOT: Pixels = px(6.);
/// A count's height, inside its ring: the "small" text's line.
const COUNT_HEIGHT: Pixels = px(14.);
/// How far the badge's outer edge reaches past its host's right and bottom
/// edges.
pub const BADGE_OVERHANG: Pixels = px(4.);
/// The largest count shown as itself; more shows as `99+`.
const MAX_COUNT: usize = 99;

/// What a badge means, which picks its colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeTone {
    /// Something to look at: the accent.
    #[default]
    Accent,
    Error,
    Warning,
    Success,
    Info,
    /// Changed and not yet saved or committed.
    Modified,
    /// A count with no meaning of its own.
    Neutral,
}

impl BadgeTone {
    /// The (fill, text) a badge of this tone is drawn in.
    pub fn colors(self, c: &Tokens<Hsla>) -> (Hsla, Hsla) {
        let fill = match self {
            BadgeTone::Accent => c.accent.base,
            BadgeTone::Error => c.status.error,
            BadgeTone::Warning => c.status.warning,
            BadgeTone::Success => c.status.success,
            BadgeTone::Info => c.status.info,
            BadgeTone::Modified => c.status.modified,
            BadgeTone::Neutral => c.text.muted,
        };
        (fill, c.text.on_accent)
    }
}

/// A count or a dot, in a colour by its meaning ([`BadgeTone`]), in a 2 px
/// ring ([`BADGE_RING`]) of the surface it sits on, so it reads as cut out
/// of the icon it marks. It places itself on its parent's bottom-right
/// corner, reaching [`BADGE_OVERHANG`] past it: add it as the last child of
/// a `relative()` element, such as an icon's box.
///
/// ```ignore
/// div().relative().size(px(16.)).child(icon).child(Badge::count(3).tone(BadgeTone::Error))
/// ```
///
/// The ring is `surface.chrome`, where the rail's and the tab strip's
/// icons sit, unless [`Badge::surface`] names another.
///
/// A badge whose meaning isn't in its host's own accessible name gets one
/// of its own from [`Badge::name`], such as "3 errors" or "Unsaved
/// changes": a screen reader then hears it as a status. Without one it's
/// only drawn, and the host's name should say what it means.
#[derive(IntoElement)]
pub struct Badge {
    count: Option<usize>,
    tone: BadgeTone,
    surface: Option<Hsla>,
    /// Its element ID and accessible name.
    name: Option<(ElementId, SharedString)>,
}

impl Badge {
    /// A badge showing `count`, `99+` past 99.
    pub fn count(count: usize) -> Self {
        Self {
            count: Some(count),
            tone: BadgeTone::default(),
            surface: None,
            name: None,
        }
    }

    /// A dot: something's there, no count.
    pub fn dot() -> Self {
        Self {
            count: None,
            tone: BadgeTone::default(),
            surface: None,
            name: None,
        }
    }

    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self
    }

    /// Its accessible name, such as "3 errors", which a screen reader reads
    /// as a status, and `id`, which keeps it the same node from frame to
    /// frame.
    pub fn name(mut self, id: impl Into<ElementId>, name: impl Into<SharedString>) -> Self {
        self.name = Some((id.into(), name.into()));
        self
    }

    /// The colour of the surface its host sits on, for its ring.
    pub fn surface(mut self, surface: Hsla) -> Self {
        self.surface = Some(surface);
        self
    }

    /// The text it shows: its count, or none for a dot.
    pub fn text(&self) -> Option<SharedString> {
        self.count.map(|count| {
            if count > MAX_COUNT {
                format!("{MAX_COUNT}+").into()
            } else {
                count.to_string().into()
            }
        })
    }

    /// Its ring's colour in `c`.
    pub(crate) fn ring_color(&self, c: &Tokens<Hsla>) -> Hsla {
        self.surface.unwrap_or(c.surface.chrome)
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let drawn = self.drawn(cx);
        match self.name {
            Some(name) => status(drawn, name).into_any_element(),
            None => drawn.into_any_element(),
        }
    }
}

impl Badge {
    /// Its element as a status node, which a named badge renders as; `None`
    /// for one without a name, which renders as [`Badge::drawn`] alone.
    #[cfg(test)]
    pub(crate) fn status(self, cx: &App) -> Option<Control> {
        let drawn = self.drawn(cx);
        Some(status(drawn, self.name?))
    }

    /// What it draws.
    fn drawn(&self, cx: &App) -> Div {
        let c = &cx.theme().colors;
        let (fill, text_color) = self.tone.colors(c);
        let ring = self.ring_color(c);
        let text = self.text();
        let inner = if text.is_some() { COUNT_HEIGHT } else { DOT };
        div()
            .debug_selector(|| "badge".into())
            .absolute()
            .right(-BADGE_OVERHANG)
            .bottom(-BADGE_OVERHANG)
            .flex()
            .items_center()
            .justify_center()
            .h(inner + BADGE_RING * 2.)
            .min_w(inner + BADGE_RING * 2.)
            .rounded_full()
            .border(BADGE_RING)
            .border_color(ring)
            .bg(fill)
            .when_some(text, |badge, text| {
                badge
                    .px(px(3.))
                    .text_size(px(10.))
                    .line_height(COUNT_HEIGHT)
                    .text_color(text_color)
                    .child(text)
            })
    }
}

/// A drawn badge as a status node named `name`, with element ID `id`.
fn status(drawn: Div, (id, name): (ElementId, SharedString)) -> Control {
    Control::new(drawn.id(id).role(Role::Status).aria_label(name), false)
}
