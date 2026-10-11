//! Tests of the scroll feature.

use std::ops::{Range, RangeInclusive};

use fe_buffer::{Buffer, Point};
use fe_command::Source;
use fe_editor::{
    EditorModel,
    commands::{EditorHost, HeadlessHost, ScrollRequest, Viewport},
};

use super::{commit, run};
use crate::{
    HeadlessVim, Registers, Step, VimState, VimTarget, context, registry, scroll::ScrollArgs,
};

/// A host with only what `EditorHost` requires.
struct Bare;

impl EditorHost for Bare {
    fn page_rows(&self) -> u32 {
        20
    }

    fn read_clipboard(&mut self) -> Option<String> {
        None
    }

    fn write_clipboard(&mut self, _: String) {}

    fn fold_range_at(&self, _: u32) -> Option<RangeInclusive<u32>> {
        None
    }

    fn fold_ranges(&self, _: Range<u32>) -> Vec<RangeInclusive<u32>> {
        Vec::new()
    }
}

#[test]
fn default_viewport_methods() {
    let mut bare = Bare;
    assert_eq!(bare.viewport(), None);
    bare.scroll(ScrollRequest::TopRow(7));
    assert_eq!(bare.viewport(), None);

    let mut host = HeadlessHost::new();
    host.clipboard = Some("x".into());
    let before = host.clone();
    assert_eq!(host.viewport(), None);
    host.scroll(ScrollRequest::TopRow(7));
    assert_eq!(host, before);
    assert_eq!(host.viewport(), None);
}

/// `count` rows, `row N` each.
fn rows(count: u32) -> String {
    (0..count)
        .map(|row| format!("{}row {row}", "  ".repeat(row as usize % 3)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn vim(cursor: (u32, usize)) -> HeadlessVim {
    let mut vim = HeadlessVim::new(&rows(60), cursor);
    vim.set_window_rows(10);
    vim
}

/// The view's top row and the cursor after `keys`.
fn after(vim: &mut HeadlessVim, keys: &str) -> (u32, (u32, usize)) {
    vim.feed(keys).unwrap();
    (vim.top_row(), vim.cursor())
}

#[test]
fn keys_move_view_and_cursor() {
    let mut v = vim((0, 0));
    assert_eq!(v.top_row(), 0);
    assert_eq!(after(&mut v, "<C-d>"), (5, (5, 0)));
    assert_eq!(after(&mut v, "<C-u>"), (0, (0, 0)));

    // A count sets 'scroll' for later uses.
    let mut v = vim((0, 0));
    assert_eq!(after(&mut v, "3<C-d>"), (3, (3, 0)));
    assert_eq!(after(&mut v, "<C-d>"), (6, (6, 0)));
    assert_eq!(after(&mut v, "<C-u>"), (3, (3, 0)));
    // A view of another height goes back to half of it.
    v.set_window_rows(4);
    assert_eq!(after(&mut v, "<C-d>"), (5, (5, 0)));

    // At the top, `<C-u>` does nothing and fails.
    let mut v = vim((0, 0));
    assert_eq!(after(&mut v, "<C-u>"), (0, (0, 0)));
    assert!(v.state().failed());

    let mut v = vim((0, 0));
    assert_eq!(after(&mut v, "<C-e>"), (1, (1, 0)));
    assert_eq!(after(&mut v, "<C-y>"), (0, (1, 0)));
    assert_eq!(after(&mut v, "<C-f>"), (8, (8, 0)));
    assert_eq!(after(&mut v, "<C-b>"), (0, (9, 0)));

    // The z keys put the cursor's row at the top, centre or bottom without
    // moving it; `z<CR>`, `z.` and `z-` go to its first non-blank too.
    let mut v = vim((20, 1));
    let top = v.top_row();
    assert_eq!(after(&mut v, "zt"), (20, (20, 1)));
    assert_eq!(after(&mut v, "zz"), (16, (20, 1)));
    assert_eq!(after(&mut v, "zb"), (11, (20, 1)));
    assert_ne!(top, 20);
    assert_eq!(after(&mut v, "z<CR>"), (20, (20, 4)));
    assert_eq!(after(&mut v, "jz."), (17, (21, 0)));
    assert_eq!(after(&mut v, "z-"), (12, (21, 0)));

    // In a visual mode the vim cursor moves, keeping the anchor.
    let mut v = vim((2, 5));
    assert_eq!(after(&mut v, "v<C-d>"), (5, (7, 5)));
    assert_eq!(v.mode(), "visual");
    assert_eq!(v.visual_anchor(), Some((2, 5)));
    assert_eq!(after(&mut v, "zt"), (7, (7, 5)));

    // After an operator they cancel.
    let mut v = vim((0, 0));
    assert_eq!(after(&mut v, "d<C-d>"), (0, (0, 0)));
    assert_eq!(v.mode(), "normal");
    assert_eq!(v.text(), rows(60));
}

#[test]
fn keys_commit_vim_scroll() {
    for (keys, action, count) in [
        ("<C-d>", "half_page_down", None),
        ("3<C-u>", "half_page_up", Some(3)),
        ("<C-e>", "line_down", None),
        ("2<C-y>", "line_up", Some(2)),
        ("<C-f>", "page_down", None),
        ("<C-b>", "page_up", None),
        ("zt", "top", None),
        ("zz", "center", None),
        ("zb", "bottom", None),
        ("5z<CR>", "top_first_non_blank", Some(5)),
        ("z.", "center_first_non_blank", None),
        ("z-", "bottom_first_non_blank", None),
    ] {
        let mut args = serde_json::json!({ "action": action });
        if let Some(count) = count {
            args["count"] = count.into();
        }
        let invocation = commit(keys);
        assert_eq!(invocation.command.as_str(), "vim.scroll", "{keys}");
        assert_eq!(invocation.args, args, "{keys}");
        assert_eq!(invocation.source, Source::Keyboard, "{keys}");
        let again: ScrollArgs = serde_json::from_value(args.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(again).unwrap(),
            args,
            "{keys} round-trips"
        );
    }
    // Other keys after `z` aren't the scroll feature's, and an operator
    // cancels.
    assert_eq!(run("zQ"), Step::Cancel);
    assert_eq!(run("d<C-d>"), Step::Cancel);
}

/// A host that knows no view: <C-d> acts on `page_rows() + 2` rows from
/// the first.
#[test]
fn no_viewport_is_a_view_from_the_first_row() {
    let registry = registry();
    let mut model = EditorModel::new(Buffer::new(&rows(60)), None);
    let mut host = HeadlessHost::new();
    host.page_rows = 8;
    let mut state = VimState::new();
    let mut registers = Registers::new();
    let mut keys_run =
        |model: &mut EditorModel, host: &mut HeadlessHost, state: &mut VimState, keys: &str| {
            let invocation = commit(keys).context(context(state));
            let mut target = VimTarget {
                model,
                host,
                state,
                registers: &mut registers,
            };
            registry.dispatch(&mut target, &invocation).unwrap();
            model
                .text()
                .offset_to_point(model.primary_selection().head())
        };
    assert_eq!(
        keys_run(&mut model, &mut host, &mut state, "<C-d>"),
        Point::new(5, 0)
    );
    // The view stays where it is, at the first row.
    assert_eq!(
        keys_run(&mut model, &mut host, &mut state, "<C-d>"),
        Point::new(10, 0)
    );
    assert_eq!(
        keys_run(&mut model, &mut host, &mut state, "<C-f>"),
        Point::new(8, 0)
    );
    assert_eq!(
        keys_run(&mut model, &mut host, &mut state, "<C-b>"),
        Point::new(8, 0)
    );
    assert!(state.failed());

    // A host can't overflow the view's arithmetic.
    host.page_rows = u32::MAX;
    for keys in [
        "<C-d>", "<C-u>", "<C-e>", "<C-y>", "<C-f>", "<C-b>", "zz", "zb",
    ] {
        keys_run(&mut model, &mut host, &mut state, keys);
    }
    let mut v = vim((30, 0));
    v.set_window_rows(u32::MAX);
    assert_eq!(after(&mut v, "G<C-y>zt"), (59, (59, 0)));
}

/// A host's view as `vim.scroll` reads and asks for it.
#[test]
fn scroll_asks_the_host() {
    struct Recording {
        view: Viewport,
        asked: Vec<ScrollRequest>,
    }
    impl EditorHost for Recording {
        fn page_rows(&self) -> u32 {
            0
        }
        fn read_clipboard(&mut self) -> Option<String> {
            None
        }
        fn write_clipboard(&mut self, _: String) {}
        fn fold_range_at(&self, _: u32) -> Option<RangeInclusive<u32>> {
            None
        }
        fn fold_ranges(&self, _: Range<u32>) -> Vec<RangeInclusive<u32>> {
            Vec::new()
        }
        fn viewport(&self) -> Option<Viewport> {
            Some(self.view)
        }
        fn scroll(&mut self, request: ScrollRequest) {
            self.asked.push(request);
        }
    }
    let registry = registry();
    let mut model = EditorModel::new(Buffer::new(&rows(60)), None);
    model.set_cursor(model.text().point_to_offset(Point::new(30, 0)));
    let mut host = Recording {
        view: Viewport {
            top_row: 25,
            rows: 6,
        },
        asked: Vec::new(),
    };
    let mut state = VimState::new();
    let mut registers = Registers::new();
    // The host's view stays at row 25: `<C-y>` pushes the cursor up into it.
    for (keys, asked, row) in [("<C-d>", 28, 33), ("zt", 33, 33), ("<C-y>", 24, 29)] {
        let invocation = commit(keys).context(context(&state));
        let mut target = VimTarget {
            model: &mut model,
            host: &mut host,
            state: &mut state,
            registers: &mut registers,
        };
        registry.dispatch(&mut target, &invocation).unwrap();
        assert_eq!(
            host.asked.pop(),
            Some(ScrollRequest::TopRow(asked)),
            "{keys}"
        );
        let head = model.primary_selection().head();
        assert_eq!(model.text().offset_to_point(head).row, row, "{keys}");
    }
}

#[test]
fn resizing_drops_counted_scroll() {
    // Away and back: half of the view again, not the stale 3.
    for plain in ["<C-d>", "<C-u>"] {
        let mut v = vim((20, 0));
        v.feed("3<C-d>").unwrap();
        v.set_window_rows(4);
        v.set_window_rows(10);
        let row = v.cursor().0;
        v.feed(plain).unwrap();
        assert_eq!(v.cursor().0.abs_diff(row), 5, "{plain}");
    }
}

#[test]
fn resize_and_return_with_scroll_keys_between() {
    let mut v = vim((0, 0));
    assert_eq!(after(&mut v, "3<C-d>"), (3, (3, 0)));
    v.set_window_rows(4);
    assert_eq!(after(&mut v, "<C-d>").1, (5, 0));
    v.set_window_rows(10);
    let row = v.cursor().0;
    assert_eq!(after(&mut v, "<C-d>").1, (row + 5, 0));
}

#[test]
fn visual_count_z_keeps_byte_and_goal_column() {
    let text = (0..60)
        .map(|row| {
            if row == 2 {
                "x"
            } else {
                "abcdefghijklmnopqrstuvwxyz"
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    for (keys, row) in [("v2j20zt", 19), ("v2j20zz", 19), ("v2j20zb", 19)] {
        let mut v = HeadlessVim::new(&text, (0, 20));
        v.set_window_rows(10);
        v.feed(keys).unwrap();
        assert_eq!(v.cursor(), (row, 1), "{keys}");
        v.feed("j").unwrap();
        assert_eq!(v.cursor(), (row + 1, 20), "{keys}j");
    }
}
