use fe_display::layout::{StyleRun, TextMeasure};
use gpui::{Hsla, TestAppContext, hsla, px};

use super::*;

fn run(len: usize, style: Hsla) -> StyleRun<Hsla> {
    StyleRun { len, style }
}

#[test]
fn colour_runs_become_decoration_runs() {
    let (red, blue) = (hsla(0.0, 1.0, 0.5, 1.0), hsla(0.6, 1.0, 0.5, 1.0));
    let runs = [
        run(2, red),
        run(3, red),
        run(4, blue),
        run(0, red),
        run(1, red),
    ];
    let decorations = decoration_runs(&runs);
    let summary: Vec<(u32, Hsla)> = decorations.iter().map(|d| (d.len, d.color)).collect();
    assert_eq!(summary, [(5, red), (4, blue), (1, red)]);
    let total: usize = runs.iter().map(|r| r.len).sum();
    assert_eq!(
        decorations.iter().map(|d| d.len as usize).sum::<usize>(),
        total
    );
    assert!(decorations.iter().all(|d| {
        d.background_color.is_none() && d.underline.is_none() && d.strikethrough.is_none()
    }));
    assert!(decoration_runs(&[]).is_empty());
}

#[gpui::test]
fn colours_do_not_change_the_shaped_layout(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| gpui::Empty);
    let (red, blue) = (hsla(0.0, 1.0, 0.5, 1.0), hsla(0.6, 1.0, 0.5, 1.0));
    cx.update(|window, _| {
        let mut shaper = Shaper {
            text_system: window.text_system().clone(),
            font: gpui::font("Menlo"),
            font_size: px(14.0),
        };
        let text = "let answer = 42;";
        let plain = shaper.shape(text, &[run(text.len(), red)]);
        let split = shaper.shape(text, &[run(3, red), run(text.len() - 3, blue)]);
        assert!(Arc::ptr_eq(&plain.layout, &split.layout));
        assert_eq!(split.decorations.len(), 2);
    });
}

#[test]
fn the_visible_columns_run_from_the_scroll_to_the_text_edge() {
    assert_eq!(visible_columns(0., 805., 10.), Some(0..80));
    assert_eq!(visible_columns(300., 805., 10.), Some(30..110));
    assert_eq!(visible_columns(0., 3., 10.), Some(0..1));
    assert_eq!(visible_columns(0., 800., 0.), None);
}
