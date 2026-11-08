// SPDX-License-Identifier: Apache-2.0

//! The role constants equal the design's tables.

use gpui::{TestAppContext, px};

use crate::{
    BOTTOM_PANEL_DEFAULT, CODE, CODE_SMALL, FOCUS_RING, FamilySlot, FontFamilies, PAGE_TITLE,
    SECTION_HEADING, SHADOW_FLOAT, TextRole, UI_BODY, UI_HEADING, UI_LABEL, UI_SMALL, height,
    radius, space, width,
};

fn role(family: FamilySlot, size: f32, line: f32, weight: u16) -> TextRole {
    TextRole {
        family,
        size: px(size),
        line_height: px(line),
        weight,
    }
}

#[test]
fn values_match_the_design_tables() {
    use FamilySlot::{Code, Title, Ui};
    assert_eq!(PAGE_TITLE, role(Title, 30., 36., 400));
    assert_eq!(SECTION_HEADING, role(Ui, 15., 20., 600));
    assert_eq!(UI_HEADING, role(Ui, 13., 18., 600));
    assert_eq!(UI_BODY, role(Ui, 13., 18., 400));
    assert_eq!(UI_LABEL, role(Ui, 12., 16., 500));
    assert_eq!(UI_SMALL, role(Ui, 11., 14., 500));
    assert_eq!(CODE, role(Code, 13., 20., 400));
    assert_eq!(CODE_SMALL, role(Code, 12., 18., 400));

    let spaces = [
        space::S2,
        space::S4,
        space::S6,
        space::S8,
        space::S12,
        space::S16,
        space::S24,
        space::S40,
    ];
    assert_eq!(spaces, [2., 4., 6., 8., 12., 16., 24., 40.].map(px));
    assert_eq!(
        [radius::FILL, radius::CONTROL, radius::MENU, radius::FLOAT],
        [2., 4., 6., 10.].map(px)
    );
    assert_eq!(
        [
            height::TITLE_BAR,
            height::TAB_STRIP,
            height::ROW,
            height::MENU_ROW,
            height::STATUS_BAR,
            height::CONTROL,
            height::CONTROL_SMALL,
            height::RAIL_ITEM,
        ],
        [38., 34., 24., 28., 24., 26., 22., 32.].map(px)
    );
    assert_eq!(
        [width::RAIL, width::SIDEBAR_MIN, width::SIDEBAR_DEFAULT],
        [42., 232., 248.].map(px)
    );
    assert_eq!(BOTTOM_PANEL_DEFAULT, px(250.));
    assert_eq!(
        (
            SHADOW_FLOAT.y,
            SHADOW_FLOAT.blur,
            SHADOW_FLOAT.strong,
            SHADOW_FLOAT.weak
        ),
        (px(12.), px(32.), 0.55, 0.14)
    );
    assert_eq!((FOCUS_RING.width, FOCUS_RING.offset), (px(2.), px(1.)));
}

#[gpui::test]
fn default_families_are_todays(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let families = FontFamilies::default();
        assert_eq!(
            families.family(FamilySlot::Ui, cx).as_ref(),
            ".SystemUIFont"
        );
        assert_eq!(families.family(FamilySlot::Title, cx), families.ui);
        // Whatever mono family `mono_family` picks, not a fixed fallback.
        assert_eq!(
            families.family(FamilySlot::Code, cx),
            kiln_platform::fonts::mono_family(cx)
        );
        let set = FontFamilies {
            code: Some("Foo Mono".into()),
            ..families
        };
        assert_eq!(set.family(FamilySlot::Code, cx).as_ref(), "Foo Mono");
    });
}
