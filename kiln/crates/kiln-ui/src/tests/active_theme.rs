// SPDX-License-Identifier: LicenseRef-Proprietary

//! The active chrome colours, as a GPUI global a host sets.

use std::{cell::Cell, rc::Rc};

use gpui::{TestAppContext, rgb};

use crate::{ActiveUiTheme, UiTheme, set_ui_theme, ui_theme};

#[gpui::test]
fn defaults_and_notifies(cx: &mut TestAppContext) {
    assert_eq!(cx.read(ui_theme), UiTheme::default_dark());

    let observed = Rc::new(Cell::new(0));
    let _subscription = cx.update(|cx| {
        let observed = observed.clone();
        cx.observe_global::<ActiveUiTheme>(move |_| observed.set(observed.get() + 1))
    });
    let paper = UiTheme {
        background: rgb(0xfafafa).into(),
        text: rgb(0x202020).into(),
        ..UiTheme::default_dark()
    };
    cx.update(|cx| set_ui_theme(cx, paper));
    cx.run_until_parked();

    assert_eq!(cx.read(ui_theme), paper);
    assert_ne!(paper, UiTheme::default_dark());
    assert_eq!(observed.get(), 1);
}
