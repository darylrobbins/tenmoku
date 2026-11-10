// SPDX-License-Identifier: Apache-2.0

//! An open picker's placeholder, and its input's fallback accessible name,
//! follow a locale change (`ActiveLocale`) and back: the picker keeps the
//! keyed `Text` it was given. The process catalogue is global: this
//! binary's one test switches it and switches back.

use gpui::{
    AnyElement, App, Context, IntoElement as _, SharedString, TestAppContext,
    Window, div,
};
use kiln_l10n::{LanguageIdentifier, Registered, ResourceFile, Resources, text};
use kiln_shell::{Picker, PickerDelegate};
use kiln_ui::UiTheme;

struct Nothing;

impl PickerDelegate for Nothing {
    fn update_matches(&mut self, _: String, _: &mut Context<Picker<Self>>) {}

    fn match_count(&self) -> usize {
        0
    }

    fn match_id(&self, _: usize) -> SharedString {
        SharedString::default()
    }

    fn render_match(&self, _: usize, _: bool, _: &App) -> AnyElement {
        div().into_any_element()
    }

    fn confirm(&mut self, _: usize, _: &mut Window, _: &mut Context<Picker<Self>>) -> bool {
        true
    }

    fn dismissed(&mut self, _: &mut Window, _: &mut Context<Picker<Self>>) {}
}

/// A catalogue message in en-US and a fake locale, `qps-ploc`.
static MESSAGES: Resources = Resources {
    crate_name: "kiln-shell-picker-locale-change",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[
        ResourceFile {
            locale: "en-US",
            path: "en-US/picker_locale_change.ftl",
            source: "picker-locale-change-find = Find a word\n",
        },
        ResourceFile {
            locale: "qps-ploc",
            path: "qps-ploc/picker_locale_change.ftl",
            source: "picker-locale-change-find = ⟦Fïnd ä wörd⟧\n",
        },
    ],
    dynamic_prefixes: &[],
};

inventory::submit! { Registered(&MESSAGES) }

fn set(tag: &str, cx: &mut gpui::VisualTestContext) {
    let tag: LanguageIdentifier = tag.parse().unwrap();
    cx.update(|_, cx| kiln_platform::locale::set_locale(cx, &[tag]));
    cx.run_until_parked();
}

#[gpui::test]
fn an_open_picker_follows_a_locale_change(cx: &mut TestAppContext) {
    cx.update(kiln_shell::register);
    let (picker, cx) = cx.add_window_view(|_, cx| {
        Picker::new(
            Nothing,
            text!("picker-locale-change-find"),
            UiTheme::default_dark(),
            cx,
        )
    });
    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.show(window, cx)));
    set("en-US", cx);
    let shown = |cx: &mut gpui::VisualTestContext| {
        picker.read_with(cx, |picker, cx| {
            let input = picker.input().read(cx);
            (input.placeholder().to_string(), input.accessible_name().to_string())
        })
    };
    let english = ("Find a word".to_owned(), "Find a word".to_owned());
    assert_eq!(shown(cx), english);

    set("qps-ploc", cx);
    let fake = "⟦Fïnd ä wörd⟧".to_owned();
    assert_eq!(shown(cx), (fake.clone(), fake));

    set("en-US", cx);
    assert_eq!(shown(cx), english);
}
