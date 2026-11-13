// SPDX-License-Identifier: Apache-2.0

//! The B3 controls (`docs/design/design-system.md`): 26 and 22 px buttons
//! with a 4 px radius, the primary one filled with the accent; disabled
//! controls at half opacity; the 2 px accent focus ring, 1 px out from a
//! focused input; a toggle's accent tint; a badge's corner and cut-out
//! ring; icon-only buttons' names; and the focused selection's accent edge.

use gpui::{
    AppContext as _, Bounds, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    Modifiers, ParentElement as _, Pixels, Render, Styled as _, TestAppContext, VisualTestContext,
    Window, div, px,
};
use kiln_l10n::Text;

use crate::{
    ActiveTheme as _, Badge, BadgeTone, Button, ButtonVariant, DISABLED_OPACITY, FOCUS_RING, Icon,
    IconButton, ListRow, RowState, SegmentedControl, TOGGLE_ON_ALPHA, TextField, TextInput, Theme,
    TreeRow, UiTheme, badge::BADGE_OVERHANG, radius, segmented::segment_colors,
    toggle_button::toggle_colors, tree_row::SELECTION_EDGE,
};

/// The themes every colour rule is checked in.
fn themes() -> [Theme; 2] {
    [Theme::dark(), Theme::light()]
}

fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(selector).expect(selector)
}

/// Draws `render` in a window of its own.
fn open<V: Render + 'static>(
    view: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
    cx: &mut TestAppContext,
) -> (Entity<V>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(view);
    cx.run_until_parked();
    (view, cx)
}

struct Buttons;

impl Render for Buttons {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_start()
            .child(
                Button::new("medium", Text::verbatim("Save"))
                    .primary()
                    .debug_selector(|| "medium".into()),
            )
            .child(
                Button::new("small", Text::verbatim("Save"))
                    .small()
                    .debug_selector(|| "small".into()),
            )
            .child(
                Button::icon_only("search", Icon::Search)
                    .name("Search")
                    .ghost()
                    .debug_selector(|| "icon".into()),
            )
            .child(IconButton::new(
                "close",
                Icon::X,
                "Close",
                &UiTheme::default(),
            ))
    }
}

#[gpui::test]
fn buttons_are_26_and_22_tall_with_a_4_px_radius(cx: &mut TestAppContext) {
    let (_, cx) = open(|_, _| Buttons, cx);
    assert_eq!(bounds("medium", cx).size.height, px(26.));
    assert_eq!(bounds("small", cx).size.height, px(22.));
    let icon = bounds("icon", cx).size;
    assert_eq!((icon.width, icon.height), (px(26.), px(26.)), "square");
    for theme in themes() {
        for variant in [
            ButtonVariant::Primary,
            ButtonVariant::Secondary,
            ButtonVariant::Ghost,
        ] {
            assert_eq!(variant.look(false, &theme.colors).radius, px(4.));
        }
    }
    assert_eq!(radius::CONTROL, px(4.));
}

#[test]
fn primary_buttons_fill_with_the_accent() {
    for theme in themes() {
        let c = &theme.colors;
        let primary = ButtonVariant::Primary.look(false, c);
        assert_eq!(primary.background, c.accent.base);
        assert_eq!(primary.text, c.text.on_accent);
        let secondary = ButtonVariant::Secondary.look(false, c);
        assert_eq!(secondary.border, c.border.control, "outlined");
        assert_eq!(secondary.background, gpui::transparent_black());
        let ghost = ButtonVariant::Ghost.look(false, c);
        assert_eq!(
            (ghost.background, ghost.border),
            (gpui::transparent_black(), gpui::transparent_black()),
            "a ghost button shows only its hover"
        );
        assert_eq!(ghost.hover, c.state.hover);
    }
}

#[test]
fn a_disabled_control_is_drawn_at_half_opacity() {
    assert_eq!(DISABLED_OPACITY, 0.5);
    for theme in themes() {
        let c = &theme.colors;
        for variant in [
            ButtonVariant::Primary,
            ButtonVariant::Secondary,
            ButtonVariant::Ghost,
        ] {
            let (enabled, disabled) = (variant.look(false, c), variant.look(true, c));
            assert_eq!(enabled.opacity, 1.);
            assert_eq!(disabled.opacity, 0.5, "{variant:?}");
            assert_eq!(
                (disabled.background, disabled.text, disabled.border),
                (enabled.background, enabled.text, enabled.border),
                "{variant:?}: the enabled look, faded"
            );
        }
    }
}

struct Inputs {
    field: Entity<TextField>,
    input: Entity<TextInput>,
}

impl Render for Inputs {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .w(px(300.))
            .child(
                div()
                    .debug_selector(|| "field".into())
                    .child(self.field.clone()),
            )
            .child(
                div()
                    .debug_selector(|| "input".into())
                    .child(self.input.clone()),
            )
    }
}

#[gpui::test]
fn a_focused_input_draws_a_2_px_accent_ring_1_px_out(cx: &mut TestAppContext) {
    let (inputs, cx) = open(
        |window, cx| Inputs {
            field: cx.new(|cx| TextField::new(window, cx).placeholder(Text::verbatim("Name"))),
            input: cx.new(|cx| {
                TextInput::new(Text::verbatim("Search"), &UiTheme::default(), cx).bordered()
            }),
        },
        cx,
    );
    let (field, input) =
        inputs.read_with(cx, |inputs, _| (inputs.field.clone(), inputs.input.clone()));
    for ring in ["text-field-focus-ring", "text-input-focus-ring"] {
        assert!(cx.debug_bounds(ring).is_none(), "{ring}: none before focus");
    }
    assert_eq!(FOCUS_RING.width, px(2.));
    assert_eq!(FOCUS_RING.offset, px(1.));
    let out = FOCUS_RING.width + FOCUS_RING.offset;

    for (host, ring, focus) in [
        (
            "field",
            "text-field-focus-ring",
            cx.update(|_, cx| field.focus_handle(cx)),
        ),
        (
            "input",
            "text-input-focus-ring",
            cx.update(|_, cx| input.focus_handle(cx)),
        ),
    ] {
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.run_until_parked();
        let (control, drawn) = (bounds(host, cx), bounds(ring, cx));
        assert_eq!(drawn.origin.x, control.origin.x - out, "{ring}: left");
        assert_eq!(drawn.origin.y, control.origin.y - out, "{ring}: top");
        assert_eq!(drawn.size.width, control.size.width + out * 2., "{ring}");
        assert_eq!(drawn.size.height, control.size.height + out * 2., "{ring}");
    }
    assert!(
        cx.debug_bounds("text-field-focus-ring").is_none(),
        "the field lost focus and its ring"
    );
    cx.update(|_, cx| {
        let c = cx.theme().colors;
        assert_eq!(crate::controls::focus_ring_color(&c), c.accent.base);
        let input = input.read(cx);
        assert_eq!(input.border_color(true, &c), Some(c.border.control));
    });
}

#[test]
fn a_toggle_is_on_in_a_tint_of_the_accent() {
    assert_eq!(TOGGLE_ON_ALPHA, 0x26);
    for theme in themes() {
        let c = &theme.colors;
        let (on, text) = toggle_colors(true, c);
        let on = on.expect("an on toggle has a fill");
        assert_eq!(
            (on.h, on.s, on.l),
            (c.accent.base.h, c.accent.base.s, c.accent.base.l)
        );
        assert_eq!(on.a, 38. / 255., "alpha 0x26, not a solid fill");
        assert_eq!(text, c.text.primary);
        assert_eq!(toggle_colors(false, c), (None, c.text.muted));
        // A segmented control's chosen segment is the design's pressed one.
        assert_eq!(
            segment_colors(true, c),
            (Some(c.state.selected_inactive), c.text.primary)
        );
    }
}

struct Badged;

impl Render for Badged {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.theme().colors.surface.panel;
        div().p_4().child(
            div()
                .debug_selector(|| "host".into())
                .relative()
                .size(px(16.))
                .child(Badge::count(3).tone(BadgeTone::Error).surface(panel)),
        )
    }
}

#[gpui::test]
fn a_badge_sits_in_the_bottom_right_in_a_2_px_surface_ring(cx: &mut TestAppContext) {
    let (_, cx) = open(|_, _| Badged, cx);
    let (host, badge) = (bounds("host", cx), bounds("badge", cx));
    assert_eq!(badge.right(), host.right() + BADGE_OVERHANG);
    assert_eq!(badge.bottom(), host.bottom() + BADGE_OVERHANG);
    assert!(
        badge.center().x > host.center().x && badge.center().y > host.center().y,
        "in the bottom-right quarter"
    );
    assert_eq!(crate::BADGE_RING, px(2.));
    cx.update(|_, cx| {
        let c = cx.theme().colors;
        let badge = Badge::count(3).surface(c.surface.panel);
        assert_eq!(badge.ring_color(&c), c.surface.panel, "the surface's");
        assert_eq!(Badge::dot().ring_color(&c), c.surface.chrome, "by default");
        assert_eq!(BadgeTone::Error.colors(&c).0, c.status.error, "by meaning");
        assert_eq!(Badge::count(120).text().as_deref(), Some("99+"));
        assert_eq!(Badge::dot().text(), None);
    });
}

struct Unnamed;

impl Render for Unnamed {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(Button::icon_only("close", Icon::X))
    }
}

#[gpui::test]
#[should_panic(expected = "an icon-only button needs a name")]
fn icon_only_buttons_need_a_name(cx: &mut TestAppContext) {
    // Named, it draws, and its name is its accessible name.
    let named = Button::icon_only("close", Icon::X).name("Close");
    assert_eq!(named.accessible_name().map(|name| &**name), Some("Close"));
    let labelled = Button::new("save", Text::verbatim("Save"));
    assert_eq!(labelled.accessible_name().map(|name| &**name), Some("Save"));
    let (_, cx) = open(|_, _| Buttons, cx);
    assert!(cx.debug_bounds("icon").is_some());
    assert!(
        cx.debug_bounds("icon-button:close").is_some(),
        "a named IconButton"
    );

    let _ = cx.add_empty_window();
    cx.update(|window, cx| {
        window.replace_root(cx, |_, _| Unnamed);
        window.draw(cx).clear(cx);
    });
}

struct UnnamedIcon;

impl Render for UnnamedIcon {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(IconButton::new("close", Icon::X, "", &UiTheme::default()))
    }
}

/// [`IconButton`] is icon-only by nature, so it always needs a name.
#[gpui::test]
#[should_panic(expected = "an icon-only button needs a name: IconButton::new")]
fn icon_buttons_need_a_name_too(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        window.replace_root(cx, |_, _| UnnamedIcon);
        window.draw(cx).clear(cx);
    });
}

/// A focused or unfocused list's selected tree row and list row.
struct Rows {
    focused: bool,
}

impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let state = RowState {
            selected: true,
            hovered: false,
            focused: self.focused,
        };
        div()
            .w(px(200.))
            .child(
                div().debug_selector(|| "tree-row".into()).child(
                    TreeRow::new("row", "main.rs", &UiTheme::default())
                        .selected(true)
                        .focused(self.focused),
                ),
            )
            .child(
                div()
                    .debug_selector(|| "list-row".into())
                    .child(ListRow::new(state).child("main.rs")),
            )
    }
}

#[gpui::test]
fn selected_row_with_focus_has_the_accent_edge(cx: &mut TestAppContext) {
    for theme in themes() {
        let c = &theme.colors;
        let row = || TreeRow::new("row", "main.rs", &UiTheme::default()).selected(true);
        let focused = row().focused(true).colors(c);
        assert_eq!(focused.background, Some(c.state.selected), "the rust tint");
        assert_eq!(focused.edge, Some(c.accent.base), "and the accent edge");
        let unfocused = row().colors(c);
        assert_eq!(unfocused.background, Some(c.state.selected_inactive));
        assert_eq!(unfocused.edge, None);

        let state = |focused| RowState {
            selected: true,
            hovered: false,
            focused,
        };
        assert_eq!(
            state(true).colors(c),
            (Some(c.state.selected), Some(c.accent.base))
        );
        assert_eq!(
            state(false).colors(c),
            (Some(c.state.selected_inactive), None)
        );
    }

    // Drawn: a 2 px bar down the selected row's left side, the tree row's
    // on its selection box (inset from the list's sides).
    let (rows, cx) = open(|_, _| Rows { focused: true }, cx);
    assert_eq!(SELECTION_EDGE, px(2.));
    for (row, edge) in [
        ("tree-row", "tree-row-edge:main.rs"),
        ("list-row", "list-row-edge"),
    ] {
        let (row, edge) = (bounds(row, cx), bounds(edge, cx));
        assert_eq!(edge.size.width, SELECTION_EDGE);
        assert_eq!((edge.top(), edge.bottom()), (row.top(), row.bottom()));
        assert!(edge.left() >= row.left() && edge.left() <= row.left() + px(4.));
    }

    rows.update(cx, |rows, cx| {
        rows.focused = false;
        cx.notify();
    });
    cx.run_until_parked();
    for edge in ["tree-row-edge:main.rs", "list-row-edge"] {
        assert!(
            cx.debug_bounds(edge).is_none(),
            "{edge}: none without focus"
        );
    }
}

struct Segments {
    scope: usize,
}

impl Render for Segments {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().child(
            SegmentedControl::new("scope", "Settings scope")
                .segments(["You", "This project"])
                .selected(self.scope)
                .on_change(cx.listener(|this, ix: &usize, _, cx| {
                    this.scope = *ix;
                    cx.notify();
                })),
        )
    }
}

#[gpui::test]
fn a_segmented_control_reports_the_segment_clicked(cx: &mut TestAppContext) {
    let (view, cx) = open(|_, _| Segments { scope: 0 }, cx);
    let control = bounds("segmented:scope", cx);
    assert_eq!(control.size.height, px(26.));
    let project = bounds("segment:This project", cx);
    cx.simulate_click(project.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.scope), 1);
}
