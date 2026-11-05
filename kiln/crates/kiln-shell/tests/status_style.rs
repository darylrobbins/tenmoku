// SPDX-License-Identifier: Apache-2.0

//! A status item's icon, tone, fill and tooltip: carried by its builders
//! through the host's set, picked into colours by the entry, and drawn, the
//! icon before the text and the tooltip on hover.

use std::time::Duration;

use gpui::{
    Context, IntoElement, Modifiers, ParentElement as _, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div, hsla, px,
};
use kiln_shell::{Icon, StatusFill, StatusItem, StatusItems, StatusSide, StatusTone, status};
use kiln_ui::{StatusEntryColors, Theme, status_entry_colors};

fn fill() -> StatusFill {
    StatusFill {
        background: hsla(0.3, 0.5, 0.4, 1.),
        foreground: hsla(0., 0., 0.1, 1.),
    }
}

fn styled(text: &str) -> StatusItem {
    StatusItem::new("mode", StatusSide::Left, 0, text.to_owned())
        .icon(Icon::Lock)
        .tone(StatusTone::Warning)
        .fill(fill())
        .tooltip("Read-only")
}

#[test]
fn builders_carry_style() {
    let plain = StatusItem::new("mode", StatusSide::Left, 0, "NORMAL");
    assert_eq!(plain.icon, None);
    assert_eq!(plain.tone, StatusTone::Default);
    assert_eq!(plain.fill, None);
    assert_eq!(plain.tooltip, None);

    let item = styled("NORMAL");
    assert_eq!(item.icon, Some(Icon::Lock));
    assert_eq!(item.tone, StatusTone::Warning);
    assert_eq!(item.fill, Some(fill()));
    assert_eq!(item.tooltip.as_deref(), Some("Read-only"));

    let mut items = StatusItems::default();
    items.set(plain.clone());
    items.set(item.clone());
    assert_eq!(items.host(), std::slice::from_ref(&item));
    let replaced = styled("INSERT")
        .icon(Icon::Server)
        .tone(StatusTone::Error)
        .tooltip("Language server");
    items.set(replaced.clone());
    assert_eq!(items.host(), std::slice::from_ref(&replaced));
    // Over the active item's own plain one with its id.
    assert_eq!(items.merged([plain]), [replaced]);
}

#[test]
fn entry_colors() {
    for theme in [Theme::dark(), Theme::light()] {
        let c = &theme.colors;
        for tone in [
            StatusTone::Default,
            StatusTone::Error,
            StatusTone::Warning,
            StatusTone::Info,
            StatusTone::Success,
        ] {
            // A fill wins over the tone.
            assert_eq!(
                status_entry_colors(tone, Some(fill()), c),
                StatusEntryColors {
                    background: Some(fill().background),
                    text: fill().foreground,
                    icon: fill().foreground,
                },
                "{} {tone:?}",
                theme.name
            );
            let icon = match tone {
                StatusTone::Default => c.text.primary,
                StatusTone::Error => c.status.error,
                StatusTone::Warning => c.status.warning,
                StatusTone::Info => c.status.info,
                StatusTone::Success => c.status.success,
            };
            assert_eq!(
                status_entry_colors(tone, None, c),
                StatusEntryColors {
                    background: None,
                    text: c.text.primary,
                    icon,
                },
                "{} {tone:?}",
                theme.name
            );
        }
        // The tones are told apart.
        assert_ne!(c.status.error, c.text.primary);
        assert_ne!(c.status.warning, c.status.error);
    }
}

/// A view with the bar along its bottom, drawing `items`.
struct Bench {
    items: Vec<StatusItem>,
}

impl Render for Bench {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_1())
            .child(status::status_bar(self.items.clone(), cx))
    }
}

fn bounds(selector: String, cx: &mut VisualTestContext) -> Option<gpui::Bounds<gpui::Pixels>> {
    // `debug_bounds` takes a `&'static str`.
    cx.debug_bounds(selector.leak())
}

#[gpui::test]
fn icon_and_tooltip_draw(cx: &mut TestAppContext) {
    let items = vec![
        StatusItem::new("errors", StatusSide::Left, 0, "2")
            .icon(Icon::CircleX)
            .tone(StatusTone::Error)
            .tooltip("2 errors"),
        StatusItem::new("plain", StatusSide::Left, 1, "Plain"),
        StatusItem::new("mode", StatusSide::Right, 0, "NORMAL")
            .icon(Icon::TriangleAlert)
            .fill(fill()),
    ];
    let (_, cx) = cx.add_window_view(|_, _| Bench { items });
    cx.run_until_parked();

    // After the side padding, 12 px for a fill's segment.
    for (id, padding) in [("errors", px(8.)), ("mode", px(12.))] {
        let item = bounds(format!("status-item:{id}"), cx).expect("drawn");
        let icon = bounds(format!("status-icon:{id}"), cx).expect("icon drawn");
        assert!(item.contains(&icon.origin), "{id}: {icon:?} in {item:?}");
        assert!(
            icon.bottom_right().x <= item.bottom_right().x
                && icon.bottom_right().y <= item.bottom_right().y,
            "{id}: {icon:?} in {item:?}"
        );
        assert_eq!(icon.size.width, px(14.), "{id}");
        // First, before the text.
        assert_eq!(icon.left() - item.left(), padding, "{id}");
        assert!(item.right() - icon.right() > padding + px(4.), "{id}");
    }
    assert!(bounds("status-item:plain".into(), cx).is_some());
    assert_eq!(bounds("status-icon:plain".into(), cx), None);

    // No tooltip until hovered past the delay.
    assert_eq!(bounds("status-tooltip:errors:2 errors".into(), cx), None);
    let errors = bounds("status-item:errors".into(), cx).expect("drawn");
    cx.simulate_mouse_move(errors.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
    let tooltip = bounds("status-tooltip:errors:2 errors".into(), cx).expect("tooltip shown");
    assert!(tooltip.size.width > px(0.));

    // Gone once the pointer leaves; an item without one shows none.
    let plain = bounds("status-item:plain".into(), cx).expect("drawn");
    cx.simulate_mouse_move(plain.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
    assert_eq!(bounds("status-tooltip:errors:2 errors".into(), cx), None);
}
