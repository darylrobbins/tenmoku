//! The colour parser: each literal form to sRGB, and finding them in text.

use super::*;

/// `rgba` as it is written: `0xrrggbb` and an alpha.
fn rgb(hex: u32, alpha: f32) -> [f32; 4] {
    let channel = |shift: u32| ((hex >> shift) & 0xff) as f32 / 255.;
    [channel(16), channel(8), channel(0), alpha]
}

/// Asserts `literal` parses to within `tolerance` of `expected`, each
/// channel.
#[track_caller]
fn assert_near(literal: &str, expected: [f32; 4], tolerance: f32) {
    let Some(got) = parse_color(literal) else {
        panic!("{literal} isn't a colour");
    };
    for (got, expected) in got.iter().zip(expected) {
        assert!(
            (got - expected).abs() <= tolerance,
            "{literal}: {got:?} isn't {expected:?}",
        );
    }
}

/// Within half a step of 8 bits: the same colour written in hex.
const EXACT: f32 = 0.5 / 255.;

/// Within a step of 8 bits.
const STEP: f32 = 1. / 255. + 1e-6;

#[test]
fn hex_forms() {
    assert_near("#6c6976", rgb(0x6c6976, 1.), EXACT);
    assert_near("#ffffff12", rgb(0xffffff, 0x12 as f32 / 255.), EXACT);
    assert_near("#f80", rgb(0xff8800, 1.), EXACT);
    assert_near("#F808", rgb(0xff8800, 0x88 as f32 / 255.), EXACT);
    assert_near("#AbCdEf", rgb(0xabcdef, 1.), EXACT);
    assert_eq!(parse_color("#000"), Some([0., 0., 0., 1.]));
    assert_eq!(parse_color("#ffffffff"), Some([1.; 4]));
}

#[test]
fn hex_boundaries() {
    let starts = |text: &str| -> Vec<usize> {
        let mut starts: Vec<usize> = (css_colors(text).iter())
            .map(|found| found.range.start)
            .collect();
        let hex: Vec<usize> = (hex_colors(text).iter())
            .map(|found| found.range.start)
            .collect();
        // Six and eight digits are found by both.
        if !hex.is_empty() {
            assert_eq!(hex, starts, "{text}");
        }
        starts.dedup();
        starts
    };
    for rejected in [
        "&#123456;",
        "a#fff",
        "#1234567",
        "#fffg",
        "_#abcdef",
        "1#abc",
        "#abc-d",
    ] {
        assert_eq!(starts(rejected), Vec::<usize>::new(), "{rejected}");
    }
    assert_eq!(css_colors("#fff;")[0].range, 0..4);
    assert_eq!(css_colors("(#fff)")[0].range, 1..5);
    assert_eq!(hex_colors("x = \"#abcdef\"")[0].range, 5..12);
    assert_eq!(parse_color("#12345"), None);
    // Only six and eight digits outside CSS.
    assert_eq!(hex_colors("#abc #abcd #abcdef #abcdef12").len(), 2);
}

#[test]
fn rgb_comma_and_space_syntax() {
    assert_near("rgba(0, 0, 0, 0.62)", [0., 0., 0., 0.62], 1e-6);
    assert_near("rgb(255 0 0 / 50%)", [1., 0., 0., 0.5], 1e-6);
    assert_near("rgb(100%, 0%, 0%)", [1., 0., 0., 1.], 1e-6);
    assert_near("rgb(none 255 0)", [0., 1., 0., 1.], 1e-6);
    assert_near("RGB(255,128,0)", rgb(0xff8000, 1.), EXACT);
    assert_near("rgba(255 0 0 / 0.25)", [1., 0., 0., 0.25], 1e-6);
    assert_near("rgb(300 -5 0 / 2)", [1., 0., 0., 1.], 1e-6);
    assert_near("rgb(  1e2  0  0  )", [100. / 255., 0., 0., 1.], 1e-6);
    // The comma syntax doesn't mix numbers and percentages, nor have `none`.
    assert_eq!(parse_color("rgb(100%, 0, 0)"), None);
    assert_eq!(parse_color("rgb(none, 0, 0)"), None);
    assert_eq!(parse_color("rgb(1, 2, 3,)"), None);
    assert_eq!(parse_color("rgb(1, 2, 3 / 0.5)"), None);
    assert_eq!(parse_color("rgb(1 2 3 / 0.5 / 1)"), None);
}

#[test]
fn hsl_hwb_and_hue_units() {
    let green = rgb(0x00ff00, 1.);
    for literal in [
        "hsl(120 100% 50%)",
        "hsl(120deg 100% 50%)",
        "hsl(0.33333333turn 100% 50%)",
        "hsl(2.0943951rad 100% 50%)",
        "hsl(133.333333grad 100% 50%)",
        "hsl(120, 100%, 50%)",
        "hsl(480 100 50)",
        "HSL(-240DEG 100% 50%)",
    ] {
        assert_near(literal, green, STEP);
    }
    assert_near("hsla(120, 100%, 50%, 0.5)", rgb(0x00ff00, 0.5), STEP);
    assert_near("hsla(0 100% 50% / 25%)", rgb(0xff0000, 0.25), STEP);
    assert_near("hsl(210 50% 40%)", rgb(0x336699, 1.), STEP);
    // The comma syntax has percentages only.
    assert_eq!(parse_color("hsl(120, 100, 50)"), None);

    assert_near("hwb(0 0% 0%)", rgb(0xff0000, 1.), STEP);
    assert_near("hwb(120 20% 30%)", rgb(0x33b333, 1.), STEP);
    // White and black past 100% scale to add up to it: a grey.
    assert_near("hwb(90 60% 60%)", rgb(0x808080, 1.), STEP);
    assert_near("hwb(0 100% 300% / 0.5)", [0.25, 0.25, 0.25, 0.5], 1e-6);
    assert_eq!(parse_color("hwb(0, 0%, 0%)"), None);
}

#[test]
fn lab_lch_oklab_oklch_convert() {
    assert_near("oklch(0.62 0.08 195)", rgb(0x429595, 1.), STEP);
    assert_near("oklab(0.5 0.1 -0.1)", rgb(0x81459a, 1.), STEP);
    assert_near("lab(50 40 -20)", rgb(0xab5a9a, 1.), STEP);
    assert_near("lch(50 40 300)", rgb(0x816cb2, 1.), STEP);
    assert_near("lab(100 0 0)", rgb(0xffffff, 1.), STEP);
    assert_near("oklch(1 0 0)", rgb(0xffffff, 1.), STEP);
    // Percentages: L 100% is 100 and 1, a and b 100% 125 and 0.4, C 150 and 0.4.
    assert_near(
        "lab(50% 32% -16%)",
        parse_color("lab(50 40 -20)").unwrap(),
        1e-6,
    );
    assert_near(
        "lch(50% 26.6666667% 300)",
        parse_color("lch(50 40 300)").unwrap(),
        1e-5,
    );
    assert_near(
        "oklab(50% 25% -25%)",
        parse_color("oklab(0.5 0.1 -0.1)").unwrap(),
        1e-6,
    );
    assert_near("oklch(62% 20% 195deg / 50%)", rgb(0x429595, 0.5), STEP);
    assert_eq!(parse_color("lab(50, 40, -20)"), None);
}

#[test]
fn out_of_gamut_oklch_clips() {
    let clipped = parse_color("oklch(0.7 0.4 150)").unwrap();
    assert_near("oklch(0.7 0.4 150)", rgb(0x00d600, 1.), STEP);
    assert!(
        clipped.iter().all(|channel| (0. ..=1.).contains(channel)),
        "{clipped:?}"
    );
    for literal in [
        "lab(100 -128 127)",
        "lch(30 150 280)",
        "oklab(0.3 -0.4 0.4)",
    ] {
        let rgba = parse_color(literal).unwrap();
        assert!(
            rgba.iter().all(|channel| (0. ..=1.).contains(channel)),
            "{literal}: {rgba:?}"
        );
    }
}

#[test]
fn color_srgb_and_named_colours() {
    assert_near("color(srgb 1 0.5 0 / 0.5)", [1., 0.5, 0., 0.5], 1e-6);
    assert_near("color(SRGB 100% 50% none)", [1., 0.5, 0., 1.], 1e-6);
    assert_near("rebeccapurple", rgb(0x663399, 1.), EXACT);
    assert_near("RED", rgb(0xff0000, 1.), EXACT);
    assert_eq!(parse_color("grey"), parse_color("gray"));
    assert_eq!(parse_color("lightslategrey"), parse_color("lightslategray"));
    assert_eq!(parse_color("transparent"), Some([0.; 4]));
    assert_eq!(parse_color("currentColor"), None);
    assert_eq!(parse_color("canvas"), None);
    assert!(NAMED.windows(2).all(|pair| pair[0].0 < pair[1].0), "sorted");
    for (name, _) in NAMED {
        assert!(parse_color(name).is_some(), "{name}");
    }
}

#[test]
fn invalid_forms_rejected() {
    for literal in [
        "var(--c)",
        "rgb(var(--r) 0 0)",
        "rgb(calc(1) 0 0)",
        "rgb(from red r g b)",
        "rgb(1, 2)",
        "rgb(1 2, 3)",
        "hsl(10px 50% 50%)",
        "color(display-p3 1 0 0)",
        "color(srgb-linear 1 0 0)",
        "rgb(1 2 3",
        "#12345",
        "rgb(1px 2 3)",
        "rgb(1 2 3 4)",
        "rgb()",
        "notacolour",
    ] {
        assert_eq!(parse_color(literal), None, "{literal}");
        let found = css_colors(literal);
        assert!(
            found.iter().all(|found| found.range.start != 0),
            "{literal}: {found:?}"
        );
    }
    assert_eq!(parse_color("rgb(1 2 3) "), None);
    // A hex literal inside a function that isn't a colour is one of its own.
    let fallback = css_colors("var(--x, #fff)");
    assert_eq!(fallback.len(), 1);
    assert_eq!(fallback[0].range, 9..13);
    let darken = css_colors("darken(#336699, 10%)");
    assert_eq!(darken.len(), 1);
    assert_eq!(darken[0].range, 7..14);
    let broken = css_colors("rgb(#fff 0 0)");
    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].range, 4..8);
}

#[test]
fn css_colors_finds_each_literal_in_order() {
    let text = "#6c6976; #ffffff12; oklch(0.62 0.08 195); rgba(0, 0, 0, 0.62)";
    let found = css_colors(text);
    let ranges: Vec<_> = found.iter().map(|found| found.range.clone()).collect();
    assert_eq!(ranges, [0..7, 9..18, 20..40, 42..61]);
    for found in &found {
        assert_eq!(Some(found.rgba), parse_color(&text[found.range.clone()]));
    }

    // Names only as words of their own, not functions, variables or
    // custom properties.
    let text = "border: 1px solid red; --red: blue; $tan: x; red-ish: a.red; Navy";
    let names: Vec<_> = (css_colors(text).into_iter())
        .map(|found| &text[found.range])
        .collect();
    assert_eq!(names, ["red", "blue", "Navy"]);
}
