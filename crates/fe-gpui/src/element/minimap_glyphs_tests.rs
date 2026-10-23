//! The minimap's char masks: one for every printable ASCII char at every
//! scale and display, drawn into tiles in the run's colour, other chars as
//! half-weight blocks, and blocks with characters off; toggling characters
//! rebuilds every tile and other options none.

use fe_buffer::Buffer;
use fe_display::minimap::{MinimapOptions, MinimapShowSlider, MinimapSide};
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, px, size};

use super::{device_scale, mask};
use crate::{EditorTheme, EditorView, init};

#[test]
fn every_printable_ascii_char_has_a_mask_of_its_scales_size() {
    for scale in 1..=3 {
        for scale_factor in [1., 2.] {
            let device = device_scale(scale as f32, scale_factor);
            assert_eq!(device, scale * scale_factor as u32);
            for ch in b'!'..=b'~' {
                let mask = mask(ch, device).unwrap_or_else(|| panic!("{}", ch as char));
                assert_eq!(
                    (mask.width, mask.height),
                    (device as usize, 2 * device as usize)
                );
                assert_eq!(mask.alpha.len(), mask.width * mask.height);
                // Every char but `.`-like specks shows at the smallest size.
                assert!(mask.alpha.iter().any(|&alpha| alpha > 0), "{}", ch as char);
            }
        }
    }
}

#[test]
fn a_space_and_chars_past_ascii_have_no_mask() {
    for device in 1..=6 {
        assert!(mask(b' ', device).is_none());
        assert!(mask(b'\t', device).is_none());
        assert!(mask(0x7f, device).is_none());
        assert!(mask(0xc3, device).is_none());
    }
}

#[test]
fn masks_past_the_samples_spread_them_evenly() {
    // `l` at 4 device pixels a column is its 2 by 4 sample doubled.
    let (two, four) = (mask(b'l', 2).expect("l"), mask(b'l', 4).expect("l"));
    for y in 0..8 {
        for x in 0..4 {
            assert_eq!(four.alpha[y * 4 + x], two.alpha[y / 2 * 2 + x / 2]);
        }
    }
    // At 3, a pixel over two of the sample's is the mean of what it covers.
    let three = mask(b'l', 3).expect("l");
    let sample = |x: usize, y: usize| two.alpha[y * 2 + x] as f32;
    let middle = (sample(0, 0) + sample(1, 0)) / 2.;
    assert!((three.alpha[1] as f32 - middle).abs() <= 1.);
}

/// An editor on plain `text`, `height` tall, with the minimap on with
/// `characters`, drawn.
fn editor<'a>(
    text: &str,
    height: f32,
    characters: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some("notes.txt".into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.simulate_resize(size(px(800.), px(height)));
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    set_options(&view, characters_on(characters), cx);
    (view, cx)
}

fn characters_on(characters: bool) -> MinimapOptions {
    MinimapOptions {
        render_characters: characters,
        ..MinimapOptions::default()
    }
}

fn set_options(view: &Entity<EditorView>, options: MinimapOptions, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| view.set_minimap(Some(options), cx));
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Tile 0's width and its pixels, blue, green, red and alpha.
fn tile(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (usize, Vec<[u8; 4]>) {
    view.read_with(cx, |view, _| {
        let cache = view.minimap_cache.borrow();
        let image = &cache.tiles.get(&0).expect("tile 0").image;
        let width = image.size(0).width.0 as usize;
        let bytes = image.as_bytes(0).expect("a frame");
        (width, bytes.as_chunks::<4>().0.to_vec())
    })
}

/// The foreground's blue, green and red.
fn foreground(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> [u8; 3] {
    view.read_with(cx, |view, _| {
        let gpui::Rgba { r, g, b, a } = view.theme.foreground.into();
        assert_eq!(a, 1., "an opaque foreground");
        [b, g, r].map(|channel| (channel * 255.).round() as u8)
    })
}

/// The alphas of the tile's first `columns` device pixels on its first
/// `lines` lines, each in the foreground's colour or clear.
fn alphas(
    view: &Entity<EditorView>,
    columns: usize,
    lines: usize,
    cx: &mut VisualTestContext,
) -> Vec<Vec<u8>> {
    let ([b, g, r], (width, pixels)) = (foreground(view, cx), tile(view, cx));
    (0..lines)
        .map(|y| {
            (0..columns)
                .map(|x| {
                    let [pb, pg, pr, alpha] = pixels[y * width + x];
                    assert!(alpha == 0 || [pb, pg, pr] == [b, g, r], "({x}, {y})");
                    alpha
                })
                .collect()
        })
        .collect()
}

#[gpui::test]
fn a_row_of_ab_draws_their_masks_and_blocks_without_characters(cx: &mut TestAppContext) {
    // Scale 1 on the test window's 2x display: 2 by 4 device pixels a char,
    // VS Code's samples of `a` and `b` softened to 12/15.
    let (view, cx) = editor("ab\n", 400., true, cx);
    let characters = alphas(&view, 5, 5, cx);
    assert_eq!(
        characters,
        [
            [37, 27, 137, 30, 0],
            [132, 161, 154, 145, 0],
            [162, 159, 161, 144, 0],
            [5, 0, 0, 5, 0],
            [0, 0, 0, 0, 0],
        ]
    );

    // Off, a block over both columns three rows' pixels tall, at 0.6.
    set_options(&view, characters_on(false), cx);
    let blocks = alphas(&view, 5, 7, cx);
    assert_ne!(blocks[..5], characters[..]);
    let row = [153, 153, 153, 153, 0];
    assert_eq!(blocks, [row, row, row, row, row, row, [0; 5]]);
}

#[gpui::test]
fn chars_without_masks_draw_half_weight_blocks_over_their_columns(cx: &mut TestAppContext) {
    // `é` a column, `中` two, then `a`'s mask.
    let (view, cx) = editor("é中a\n", 400., true, cx);
    let alphas = alphas(&view, 9, 4, cx);
    for line in &alphas {
        assert_eq!(line[..6], [128; 6]);
        assert_eq!(line[8], 0);
    }
    assert_eq!(alphas[0][6..8], [37, 27], "a");
}

/// The tiles built since the last call, by index.
fn built(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        let mut built = std::mem::take(&mut view.minimap_cache.borrow_mut().built);
        built.sort();
        built.dedup();
        built
    })
}

#[gpui::test]
fn toggling_characters_rebuilds_every_tile_and_other_options_none(cx: &mut TestAppContext) {
    let text: String = (0..300).map(|n| format!("value {n}\n")).collect();
    let (view, cx) = editor(&text, 1400., true, cx);
    let all = [0, 1, 2, 3, 4];
    assert_eq!(built(&view, cx), all);

    set_options(&view, characters_on(false), cx);
    assert_eq!(built(&view, cx), all, "blocks");
    set_options(&view, characters_on(true), cx);
    assert_eq!(built(&view, cx), all, "characters again");

    let left = MinimapOptions {
        side: MinimapSide::Left,
        ..characters_on(true)
    };
    set_options(&view, left, cx);
    assert_eq!(built(&view, cx), [0u32; 0], "the side");
    let always = MinimapOptions {
        show_slider: MinimapShowSlider::Always,
        ..left
    };
    set_options(&view, always, cx);
    assert_eq!(built(&view, cx), [0u32; 0], "the slider");
}
