// SPDX-License-Identifier: Apache-2.0

//! Button, Checkbox and Switch, used the GPUI way: styled and identified
//! with GPUI's own methods, handled with `cx.listener`, values kept by the
//! caller.

use gpui::{
    Context, InteractiveElement as _, IntoElement, Modifiers, ParentElement as _, Render,
    Styled as _, TestAppContext, VisualTestContext, Window, div, prelude::FluentBuilder as _, px,
};
use kiln_l10n::Text;

use crate::{Button, Checkbox, Switch};

/// A form with a button, a checkbox and a switch, and what they did.
#[derive(Default)]
struct Form {
    saves: u32,
    disabled_clicks: u32,
    wrap: bool,
    sync: bool,
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                Button::new("save", Text::verbatim("Save"))
                    .primary()
                    .debug_selector(|| "save".into())
                    // The caller's styles win over the button's own.
                    .w(px(200.))
                    .h(px(40.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.saves += 1;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("off", Text::verbatim("Off"))
                    .disabled(true)
                    .debug_selector(|| "off".into())
                    .on_click(cx.listener(|this, _, _, _| this.disabled_clicks += 1)),
            )
            .child(
                Checkbox::new("wrap")
                    .label("Wrap lines")
                    .checked(self.wrap)
                    .debug_selector(|| "wrap".into())
                    .on_change(cx.listener(|this, checked: &bool, _, cx| {
                        this.wrap = *checked;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("sync")
                    .label("Sync")
                    .on(self.sync)
                    .debug_selector(|| "sync".into())
                    // GPUI's FluentBuilder works on it as on any element.
                    .when(self.sync, |switch| switch.opacity(0.9))
                    .on_change(cx.listener(|this, on: &bool, _, cx| {
                        this.sync = *on;
                        cx.notify();
                    })),
            )
    }
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn controls_take_gpui_styles_and_listeners(cx: &mut TestAppContext) {
    let (form, cx) = cx.add_window_view(|_, _| Form::default());
    cx.run_until_parked();

    let save = cx.debug_bounds("save").unwrap();
    assert_eq!(save.size.width, px(200.), "the caller's width");
    assert_eq!(
        save.size.height,
        px(40.),
        "the caller's height over the medium size's"
    );

    click("save", cx);
    click("save", cx);
    click("off", cx);
    form.read_with(cx, |form, _| {
        assert_eq!(form.saves, 2);
        assert_eq!(form.disabled_clicks, 0, "a disabled button runs nothing");
    });

    click("wrap", cx);
    assert!(form.read_with(cx, |form, _| form.wrap), "checked");
    click("wrap", cx);
    assert!(!form.read_with(cx, |form, _| form.wrap), "unchecked again");

    click("sync", cx);
    assert!(form.read_with(cx, |form, _| form.sync));
}

#[gpui::test]
fn disabled_toggles_report_nothing(cx: &mut TestAppContext) {
    struct Disabled(u32);
    impl Render for Disabled {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .child(
                    Checkbox::new("c")
                        .disabled(true)
                        .debug_selector(|| "c".into())
                        .on_change(cx.listener(|this, _, _, _| this.0 += 1)),
                )
                .child(
                    Switch::new("s")
                        .disabled(true)
                        .debug_selector(|| "s".into())
                        .on_change(cx.listener(|this, _, _, _| this.0 += 1)),
                )
        }
    }
    let (view, cx) = cx.add_window_view(|_, _| Disabled(0));
    cx.run_until_parked();
    click("c", cx);
    click("s", cx);
    assert_eq!(view.read_with(cx, |view, _| view.0), 0);
}

#[test]
fn disabled_buttons_keep_their_look_at_half_opacity() {
    use crate::{ButtonVariant, DISABLED_OPACITY, Theme};
    for theme in [Theme::dark(), Theme::light()] {
        let c = &theme.colors;
        for variant in [
            ButtonVariant::Primary,
            ButtonVariant::Secondary,
            ButtonVariant::Ghost,
            ButtonVariant::Danger,
        ] {
            let (enabled, disabled) = (variant.look(false, c), variant.look(true, c));
            assert_eq!(enabled.opacity, 1.);
            assert_eq!(DISABLED_OPACITY, 0.5);
            assert_eq!(
                disabled,
                crate::ButtonLook {
                    opacity: DISABLED_OPACITY,
                    ..enabled
                },
                "{variant:?} in {}: the enabled look, faded",
                theme.name
            );
        }
    }
}
