// SPDX-License-Identifier: Apache-2.0

//! What each platform does natively, and the window options, fonts and
//! dialog messages that follow from it, checked for every platform on any
//! host.

use gpui::{TestAppContext, point, px, size};
use kiln_platform::{
    Capabilities, Os, WindowSpec, dialogs::open_panel_failed, fonts, window_options,
};

#[test]
fn per_platform() {
    let mac = Capabilities::for_os(Os::MacOs);
    assert!(mac.native_menus && mac.native_prompts && mac.unified_title_bar);
    for os in [Os::Linux, Os::Windows] {
        let caps = Capabilities::for_os(os);
        assert!(
            !caps.native_menus && !caps.native_prompts && !caps.unified_title_bar,
            "{os:?}"
        );
    }
    assert_eq!(Capabilities::current(), Capabilities::for_os(Os::current()));
}

#[gpui::test]
fn window_options_follow_the_platform(cx: &mut TestAppContext) {
    let spec = WindowSpec::main("Notes")
        .size(size(px(900.), px(600.)))
        .app_id("com.example.notes");
    let mac = cx.update(|cx| window_options(&spec, &Capabilities::for_os(Os::MacOs), cx));
    let titlebar = mac.titlebar.unwrap();
    assert_eq!(titlebar.title.as_deref(), Some("Notes"));
    assert!(titlebar.appears_transparent);
    assert_eq!(
        titlebar.traffic_light_position,
        Some(point(px(14.), px(12.)))
    );
    assert!(mac.app_owns_titlebar_drag);
    assert_eq!(mac.app_id.as_deref(), Some("com.example.notes"));

    let linux = cx.update(|cx| window_options(&spec, &Capabilities::for_os(Os::Linux), cx));
    let titlebar = linux.titlebar.unwrap();
    assert_eq!(titlebar.title.as_deref(), Some("Notes"));
    assert!(!titlebar.appears_transparent);
    assert_eq!(titlebar.traffic_light_position, None);
    assert!(!linux.app_owns_titlebar_drag);
    assert_eq!(linux.app_id.as_deref(), Some("com.example.notes"));
    let bounds = linux.window_bounds.unwrap().get_bounds();
    assert_eq!(bounds.size, size(px(900.), px(600.)));
}

/// The last resort per platform is among the families tried, so a
/// platform that has it picks it before anything else fails. (The test
/// platform's text system takes any family as installed, so which are
/// installed is checked in the running app, not here.)
#[test]
fn mono_family_falls_back() {
    assert_eq!(fonts::mono_last_resort(Os::MacOs), "Menlo");
    assert_eq!(fonts::mono_last_resort(Os::Linux), "DejaVu Sans Mono");
    for os in [Os::MacOs, Os::Linux] {
        assert!(
            fonts::MONO_FAMILIES.contains(&fonts::mono_last_resort(os)),
            "{os:?}"
        );
    }
}

#[test]
fn dialog_errors_say_what_is_missing() {
    let (title, detail) = open_panel_failed("Portal request failed", Os::Linux);
    assert_eq!(title, "The open panel isn't available");
    assert_eq!(
        detail,
        "Portal request failed\n\nThe open panel comes from xdg-desktop-portal: install it \
         with a backend for your desktop, such as xdg-desktop-portal-gtk."
    );
    let (_, detail) = open_panel_failed("Portal request failed", Os::MacOs);
    assert_eq!(detail, "Portal request failed");
}

/// The design's code family is tried first: an app that registers it, as
/// Tenmoku does, gets it whatever else is installed.
#[test]
fn spline_sans_mono_is_tried_first() {
    assert_eq!(fonts::MONO_FAMILIES[0], "Spline Sans Mono");
}
