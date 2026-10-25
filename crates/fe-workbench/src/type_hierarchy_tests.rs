//! The Hierarchy panel in Types mode over a fake server.

use std::time::Duration;

use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
use gpui::{Focusable as _, TestAppContext};
use lsp_types::{CallHierarchyServerCapability, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use super::Mode;
use super::hierarchy_panel_tests::{
    INCOMING, MAIN, PREPARE as PREPARE_CALLS, asked_about, item, panel, press, range, reply,
    requests, selection, texts, toasts,
};
use crate::{DockPanel, tests::lsp::Lsp};

const PREPARE: &str = "textDocument/prepareTypeHierarchy";
const SUPERTYPES: &str = "typeHierarchy/supertypes";
const SUBTYPES: &str = "typeHierarchy/subtypes";

fn script(provider: bool) -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF16));
    init.capabilities = ServerCapabilities {
        call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
        experimental: provider.then(|| json!({ "typeHierarchyProvider": true })),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

/// `src/z.rs` open and focused with `a` (line 0) the type at the caret; `b`
/// is on line 1 and `c` on line 2.
fn setup(cx: &mut TestAppContext, provider: bool) -> Lsp {
    let mut s = Lsp::with_fake_clock(cx, script(provider), Script::default());
    s.set_trust(true);
    std::fs::write(s.path("src/z.rs"), MAIN).unwrap();
    s.open("src/z.rs");
    let uri = s.uri("src/z.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("z.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    let a = item(&s, "a", 0);
    reply(&s, PREPARE, json!([a]));
    s
}

fn show(s: &mut Lsp, rows: usize) {
    s.run("nav.show_type_hierarchy");
    wait_for_rows(s, rows);
}

fn wait_for_rows(s: &mut Lsp, rows: usize) {
    s.until("the rows", |s| {
        s.cx.executor().advance_clock(Duration::from_millis(20));
        s.workbench.read_with(&s.cx, |w, cx| {
            w.dock()
                .read(cx)
                .hierarchy()
                .is_some_and(|p| p.read(cx).texts().len() >= rows)
        })
    });
}

fn mode(s: &Lsp) -> Mode {
    panel(s).read_with(&s.cx, |p, _| p.mode())
}

fn title(s: &Lsp) -> String {
    panel(s).read_with(&s.cx, |p, cx| p.title(cx).to_string())
}

fn with(item: Value, uri_of: &Lsp, name: &str) -> Value {
    let mut item = item;
    item["uri"] = json!(uri_of.uri(name).to_string());
    item
}

#[gpui::test]
fn show_type_hierarchy_lists_supertypes(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let (b, c) = (item(&s, "b", 1), item(&s, "c", 2));
    reply(&s, SUPERTYPES, json!([b, c]));
    show(&mut s, 3);
    assert_eq!(texts(&s), ["a", "  b", "  c"]);
    assert_eq!(title(&s), "Type Hierarchy: a");
    assert_eq!(mode(&s), Mode::Supertypes);
    // Lazy: only the root's supertypes, and no subtypes.
    assert_eq!(asked_about(&s, SUPERTYPES), ["a"]);
    assert!(requests(&s, SUBTYPES).is_empty());
    let active = (s.workbench).read_with(&s.cx, |w, cx| w.dock().read(cx).active());
    assert_eq!(active, DockPanel::CallHierarchy);

    // Expanding a row asks for just that row's supertypes.
    reply(&s, SUPERTYPES, json!([]));
    press(&mut s, "down");
    press(&mut s, "right");
    s.until("no supertypes", |s| texts(s).len() == 4);
    assert_eq!(texts(&s), ["a", "  b", "    No supertypes", "  c"]);
    assert_eq!(asked_about(&s, SUPERTYPES), ["a", "b"]);
}

#[gpui::test]
fn toggle_shows_subtypes_lazily(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let b = item(&s, "b", 1);
    reply(&s, SUPERTYPES, json!([b]));
    show(&mut s, 2);

    let c = item(&s, "c", 2);
    reply(&s, SUBTYPES, json!([c]));
    s.run("hierarchy.toggle_direction");
    s.until("subtypes", |s| texts(s) == ["a", "  c"]);
    assert_eq!(mode(&s), Mode::Subtypes);
    assert_eq!(asked_about(&s, SUBTYPES), ["a"]);

    reply(&s, SUBTYPES, json!([]));
    press(&mut s, "down");
    press(&mut s, "right");
    s.until("no subtypes", |s| texts(s).len() == 3);
    assert_eq!(texts(&s), ["a", "  c", "    No subtypes"]);
    assert_eq!(asked_about(&s, SUBTYPES), ["a", "c"]);

    // And back.
    s.run("hierarchy.toggle_direction");
    s.until("supertypes", |s| texts(s) == ["a", "  b"]);
    assert_eq!(mode(&s), Mode::Supertypes);
}

#[gpui::test]
fn toggling_during_prepare_restarts_it(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let a = item(&s, "a", 0);
    s.fake.set_reply(
        PREPARE,
        Scripted {
            reply: Reply::Result(json!([a])),
            delay: Duration::from_millis(500),
        },
    );
    let c = item(&s, "c", 2);
    reply(&s, SUBTYPES, json!([c]));
    s.run("nav.show_type_hierarchy");
    s.run("hierarchy.toggle_direction");
    s.until("the root with its subtypes", |s| {
        s.cx.executor().advance_clock(Duration::from_millis(100));
        s.clock.advance(Duration::from_millis(100));
        texts(s) == ["a", "  c"]
    });
    assert_eq!(mode(&s), Mode::Subtypes);
}

#[gpui::test]
fn a_cyclic_hierarchy_is_marked(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let (a, b) = (item(&s, "a", 0), item(&s, "b", 1));
    reply(&s, SUPERTYPES, json!([b]));
    show(&mut s, 2);
    reply(&s, SUPERTYPES, json!([a]));
    press(&mut s, "down");
    press(&mut s, "right");
    s.until("the cycle", |s| texts(s).len() == 3);
    assert_eq!(texts(&s), ["a", "  b", "    a (recursive)"]);
    // Not expandable.
    press(&mut s, "down");
    press(&mut s, "right");
    assert_eq!(texts(&s).len(), 3);
    assert_eq!(asked_about(&s, SUPERTYPES), ["a", "b"]);
}

#[gpui::test]
fn enter_opens_the_type_at_its_selection(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    std::fs::write(s.path("src/y.rs"), "struct Y;\n").unwrap();
    let mut y = with(item(&s, "Y", 0), &s, "src/y.rs");
    y["selectionRange"] = range(0, 7, 8);
    reply(&s, SUPERTYPES, json!([y]));
    show(&mut s, 2);
    s.cx.simulate_keystrokes("down enter");
    s.draw();
    s.until("Y selected", |s| selection(s) == (7..8));
    // The root opens its own name.
    s.cx.simulate_keystrokes("up enter");
    s.draw();
    s.until("a selected", |s| selection(s) == (3..4));
}

#[gpui::test]
fn no_provider_toasts(cx: &mut TestAppContext) {
    let mut s = setup(cx, false);
    s.run("nav.show_type_hierarchy");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["No type hierarchy available here"]);
    assert!(requests(&s, PREPARE).is_empty());
}

#[gpui::test]
fn no_item_toasts(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    reply(&s, PREPARE, json!([]));
    s.run("nav.show_type_hierarchy");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["No type hierarchy available here"]);
}

#[gpui::test]
fn calls_and_types_modes_replace_each_other_in_the_one_panel(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let b = item(&s, "b", 1);
    reply(&s, SUPERTYPES, json!([b.clone()]));
    show(&mut s, 2);
    assert_eq!(title(&s), "Type Hierarchy: a");

    // Calls take the panel: callers first, whatever the types direction was.
    s.run("hierarchy.toggle_direction");
    s.until("subtypes", |s| mode(s) == Mode::Subtypes);
    let a = item(&s, "a", 0);
    reply(&s, PREPARE_CALLS, json!([a]));
    reply(
        &s,
        INCOMING,
        json!([{ "from": b, "fromRanges": [range(1, 9, 10)] }]),
    );
    s.run("nav.show_call_hierarchy");
    s.until("callers", |s| {
        texts(s) == ["a", "  b"] && mode(s) == Mode::Incoming
    });
    assert_eq!(title(&s), "Call Hierarchy: a");

    // And types take it back, supertypes first.
    s.run("nav.show_type_hierarchy");
    s.until("supertypes", |s| {
        title(s) == "Type Hierarchy: a" && texts(s) == ["a", "  b"]
    });
    assert_eq!(mode(&s), Mode::Supertypes);
    let hierarchy_panels =
        (s.workbench).read_with(&s.cx, |w, cx| w.dock().read(cx).hierarchy().is_some());
    assert!(hierarchy_panels);
}

/// UTF-16LE with a BOM, as the editor loads it.
fn utf16(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

#[gpui::test]
fn an_unopened_utf16_or_bom_target_opens_at_its_selection(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    std::fs::write(s.path("src/y.rs"), utf16("// é\nstruct Y;\n")).unwrap();
    std::fs::write(s.path("src/x.rs"), b"\xEF\xBB\xBF// \xC3\xA9\nstruct X;\n").unwrap();
    let mut y = with(item(&s, "Y", 1), &s, "src/y.rs");
    y["selectionRange"] = range(1, 7, 8);
    let mut x = with(item(&s, "X", 1), &s, "src/x.rs");
    x["selectionRange"] = range(1, 7, 8);
    reply(&s, SUPERTYPES, json!([y, x]));
    show(&mut s, 3);
    // "// é\n" is 6 bytes in UTF-8, and the BOM isn't text.
    s.cx.simulate_keystrokes("down enter");
    s.draw();
    s.until("Y selected", |s| selection(s) == (13..14));
    s.cx.simulate_keystrokes("down enter");
    s.draw();
    s.until("X selected", |s| selection(s) == (13..14));
}

#[gpui::test]
fn showing_again_starts_at_the_supertypes(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let (b, c) = (item(&s, "b", 1), item(&s, "c", 2));
    reply(&s, SUPERTYPES, json!([b]));
    reply(&s, SUBTYPES, json!([c]));
    show(&mut s, 2);
    s.run("hierarchy.toggle_direction");
    s.until("subtypes", |s| texts(s) == ["a", "  c"]);
    s.run("nav.show_type_hierarchy");
    s.until("supertypes", |s| texts(s) == ["a", "  b"]);
    assert_eq!(mode(&s), Mode::Supertypes);
}

#[gpui::test]
fn a_cancelled_type_prepare_is_not_restarted_by_a_toggle(cx: &mut TestAppContext) {
    cancelled_prepare_stays_cancelled(cx, true);
}

#[gpui::test]
fn a_cancelled_call_prepare_is_not_restarted_by_a_toggle(cx: &mut TestAppContext) {
    cancelled_prepare_stays_cancelled(cx, false);
}

fn cancelled_prepare_stays_cancelled(cx: &mut TestAppContext, types: bool) {
    {
        let mut s = setup(cx, true);
        let old = item(&s, "old", 1);
        for method in [PREPARE, PREPARE_CALLS] {
            s.fake.set_reply(
                method,
                Scripted {
                    reply: Reply::Result(json!([old.clone()])),
                    delay: Duration::from_millis(500),
                },
            );
        }
        let command = if types {
            "nav.show_type_hierarchy"
        } else {
            "nav.show_call_hierarchy"
        };
        s.run(command);
        // A file no server has cancels it.
        std::fs::write(s.path("notes.txt"), "text\n").unwrap();
        s.open("notes.txt");
        s.run(command);
        s.until("the toast", |s| !toasts(s).is_empty());
        s.run("hierarchy.toggle_direction");
        for _ in 0..3 {
            s.cx.executor().advance_clock(Duration::from_secs(1));
            s.clock.advance(Duration::from_secs(1));
            s.draw();
        }
        assert_eq!(texts(&s), Vec::<String>::new());
    }
}
