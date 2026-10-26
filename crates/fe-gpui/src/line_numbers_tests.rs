//! `editor.line_numbers` reaching open editors on a settings reload, and an
//! editor's own `:set nu rnu` surviving a reload of something else, in a
//! headless window.

use std::sync::Arc;

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_config::{Settings, settings::LineNumbersSetting};
use fe_display::{LineNumbers, layout::MeasuredLine as _};
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{commands::RunCommand, *};

/// A focused vim editor on twelve rows, `l00` to `l11`, its caret on the
/// last.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let text = (0..12).map(|k| format!("l{k:02}")).collect::<Vec<_>>();
    let text = text.join("\n");
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    view.update(cx, |view, cx| view.select_and_reveal(44..44, cx));
    cx.run_until_parked();
    (view, cx)
}

fn next_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();
}

/// A reload from `previous` to `settings`.
fn event(previous: &EditorSettings, settings: &EditorSettings) -> SettingsEvent {
    let wrap = |editor: &EditorSettings| {
        Arc::new(Settings {
            editor: editor.clone(),
            ..Settings::default()
        })
    };
    SettingsEvent {
        settings: wrap(settings),
        previous: wrap(previous),
        diagnostics: Vec::new(),
    }
}

/// Opens vim's command line and submits `line`, as Enter in it does, and
/// returns the vim message.
fn ex(line: &str, view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    cx.simulate_keystrokes(":");
    let invocation = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": line }));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| view.vim_state()?.message().map(str::to_owned))
}

fn flags(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (bool, bool) {
    view.read_with(cx, |view, _| {
        let numbers = view.model().line_numbers();
        (numbers.number, numbers.relative)
    })
}

/// How many digits each drawn line number has, top to bottom: enough to
/// tell the modes apart with the caret on the last of twelve rows.
fn digits(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<usize> {
    next_frame(cx);
    view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        (frame.line_numbers.iter())
            // The boundary at the end: the number's length.
            .map(|(_, line)| line.index_for_x(f32::MAX))
            .collect()
    })
}

#[gpui::test]
fn reload_applies_and_set_survives(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    // 1 to 12.
    let absolute = [1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2];
    // 11 to 0.
    let relative = [2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
    // 11 to 1, then 12.
    let hybrid = [2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2];
    assert_eq!(flags(&view, cx), (true, false));
    assert_eq!(digits(&view, cx), absolute);

    // A reload that changes editor.line_numbers applies it.
    let defaults = EditorSettings::default();
    let relative_settings = EditorSettings {
        line_numbers: LineNumbersSetting::Relative,
        ..defaults.clone()
    };
    view.update(cx, |view, cx| {
        view.reload_settings(&event(&defaults, &relative_settings), cx)
    });
    assert_eq!(flags(&view, cx), (false, true));
    assert_eq!(digits(&view, cx), relative);

    // The editor's own :set nu survives a reload of something else.
    assert_eq!(ex("set nu", &view, cx), None);
    assert_eq!(flags(&view, cx), (true, true));
    assert_eq!(digits(&view, cx), hybrid);
    let bigger = EditorSettings {
        font_size: 20.,
        ..relative_settings.clone()
    };
    view.update(cx, |view, cx| {
        view.reload_settings(&event(&relative_settings, &bigger), cx)
    });
    assert_eq!(flags(&view, cx), (true, true));
    assert_eq!(digits(&view, cx), hybrid);

    // Until a reload changes editor.line_numbers again.
    let off = EditorSettings {
        line_numbers: LineNumbersSetting::Off,
        ..bigger.clone()
    };
    view.update(cx, |view, cx| {
        view.reload_settings(&event(&bigger, &off), cx)
    });
    assert_eq!(flags(&view, cx), (false, false));
    assert_eq!(digits(&view, cx), Vec::<usize>::new());
    let hybrid_settings = EditorSettings {
        line_numbers: LineNumbersSetting::Hybrid,
        ..off.clone()
    };
    view.update(cx, |view, cx| {
        view.reload_settings(&event(&off, &hybrid_settings), cx)
    });
    assert_eq!(flags(&view, cx), (true, true));
    assert_eq!(digits(&view, cx), hybrid);

    // Each value is the vim options it names.
    for (setting, number, relative) in [
        (LineNumbersSetting::On, true, false),
        (LineNumbersSetting::Off, false, false),
        (LineNumbersSetting::Relative, false, true),
        (LineNumbersSetting::Hybrid, true, true),
    ] {
        assert_eq!(
            line_numbers(setting),
            LineNumbers { number, relative },
            "{setting:?}"
        );
    }
}
