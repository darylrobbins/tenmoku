//! A code lens's `rust-analyzer.showReferences` and
//! `editor.action.showReferences` open the References panel with the
//! locations they carry, and send the server nothing.

use std::time::Duration;

use fe_command::{ContextFrame, ContextStack, Source};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{CodeLensOptions, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use crate::{
    DockPanel,
    references_panel::{ReferencesPanel, ShownRow},
    tests::lsp::Lsp,
};

const LENS: &str = "textDocument/codeLens";
const RESOLVE: &str = "codeLens/resolve";
const REST: Duration = Duration::from_millis(250);
const EXECUTE: &str = "workspace/executeCommand";
const MAIN: &str = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\n";
/// `d` is at column 8 after the emoji's two UTF-16 units.
const OTHER: &str = "// 😀 d()\n";

fn script() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF16));
    init.capabilities = ServerCapabilities {
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(true),
        }),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn location(name: &str, s: &Lsp, line: u32, from: u32, to: u32) -> Value {
    json!({ "uri": s.uri(name).to_string(), "range": {
        "start": { "line": line, "character": from },
        "end": { "line": line, "character": to } } })
}

/// Opens `main.rs` with a `2 references` lens on `d` that runs `command`.
fn setup(cx: &mut TestAppContext, command: &str) -> Lsp {
    setup_with(cx, command, true)
}

/// As `setup`; `well_formed` false gives the lens no arguments, and the
/// server lists `command`.
fn setup_with(cx: &mut TestAppContext, command: &str, well_formed: bool) -> Lsp {
    let mut script = script();
    if !well_formed {
        script.initialize = match script.initialize {
            InitializeReply::Result(mut init) => {
                init.capabilities.execute_command_provider =
                    Some(lsp_types::ExecuteCommandOptions {
                        commands: vec![command.into()],
                        ..Default::default()
                    });
                InitializeReply::Result(init)
            }
            other => other,
        };
    }
    let mut s = Lsp::with_fake_clock(cx, script, Script::default());
    s.set_trust(true);
    std::fs::write(s.path("src/z.rs"), MAIN).unwrap();
    std::fs::write(s.path("src/other.rs"), OTHER).unwrap();
    let main = s.uri("src/z.rs");
    let locations = json!([
        location("src/other.rs", &s, 0, 6, 7),
        location("src/z.rs", &s, 3, 3, 4),
    ]);
    s.fake.set_reply(
        LENS,
        Scripted::now(Reply::Result(json!([{
            "range": { "start": { "line": 3, "character": 3 }, "end": { "line": 3, "character": 4 } },
            "command": {
                "title": "2 references",
                "command": command,
                "arguments": if well_formed {
                    json!([main.to_string(), { "line": 3, "character": 3 }, locations])
                } else {
                    json!([main.to_string()])
                },
            },
        }]))),
    );
    // The unopened file of the locations is open too, so the server has its
    // text.
    s.open("src/other.rs");
    let other = s.uri("src/other.rs");
    s.fake
        .wait_for_document(&other, "didOpen", |r| r.is_some_and(|r| r.open));
    s.open("src/z.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor: Entity<EditorView> = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("z.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    s
}

/// The first lens row's first item's click, once it shows.
fn lens(s: &Lsp) -> Option<fe_command::Invocation> {
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    editor.read_with(&s.cx, |view, _| {
        let snapshot = view.model().snapshot().clone();
        (view.lens_providers().iter())
            .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
            .find_map(|lens| lens.items.first().map(|item| item.invocation.clone()))
    })
}

fn click(s: &mut Lsp) {
    s.until("the lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        lens(s).is_some()
    });
    let invocation = lens(s).unwrap();
    let context = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let run = RunCommand(invocation.context(context).source(Source::Mouse));
    s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    s.draw();
}

fn panel(s: &Lsp) -> Entity<ReferencesPanel> {
    (s.workbench).read_with(&s.cx, |w, cx| w.dock().read(cx).references().clone())
}

fn reference(line: u64, preview: &str, highlights: std::ops::Range<usize>) -> ShownRow {
    ShownRow::Ref {
        line,
        preview: preview.into(),
        highlights: vec![highlights],
        dimmed: false,
        definition: false,
    }
}

#[gpui::test]
fn reference_lenses_open_the_references_panel(cx: &mut TestAppContext) {
    for command in [
        "rust-analyzer.showReferences",
        "editor.action.showReferences",
    ] {
        let mut s = setup(cx, command);
        click(&mut s);
        let panel = panel(&s);
        s.until("the references", |s| {
            panel.read_with(&s.cx, |p, _| p.shown_rows().len() == 4)
        });
        let (rows, header) = panel.read_with(&s.cx, |p, _| (p.shown_rows(), p.header_text()));
        assert_eq!(
            rows,
            [
                ShownRow::File("src/z.rs".into(), 1),
                reference(4, "fn d() {}", 3..4),
                ShownRow::File("src/other.rs".into(), 1),
                reference(1, "// 😀 d()", 8..9),
            ],
            "{command}"
        );
        assert_eq!(header.as_deref(), Some("References to `d`"));
        let (visible, active) = s.workbench.read_with(&s.cx, |w, cx| {
            let dock = w.dock().read(cx);
            (dock.is_visible(), dock.active())
        });
        assert!(visible && active == DockPanel::References);
        s.draw();
        assert!(s.cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)));
        let executed = (s.fake.records().iter())
            .filter(|r| matches!(r, Record::Request { method, .. } if method == EXECUTE))
            .count();
        assert_eq!(executed, 0, "{command} sends nothing");
    }
}

/// The first lens row's item titles.
fn titles(s: &Lsp) -> Vec<String> {
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    editor.read_with(&s.cx, |view, _| {
        let snapshot = view.model().snapshot().clone();
        (view.lens_providers().iter())
            .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
            .flat_map(|lens| lens.items.into_iter().map(|item| item.title.to_string()))
            .collect()
    })
}

#[gpui::test]
fn a_restarted_server_is_asked_for_lenses_again(cx: &mut TestAppContext) {
    let mut s = setup(cx, "rust-analyzer.showReferences");
    s.until("the lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        titles(s) == ["2 references"]
    });
    // The restart reopens the unchanged document under a new session.
    s.fake.set_reply(
        LENS,
        Scripted::now(Reply::Result(json!([{
            "range": { "start": { "line": 3, "character": 3 }, "end": { "line": 3, "character": 4 } },
            "command": { "title": "after restart", "command": "x" },
        }]))),
    );
    s.run("lsp.restart");
    s.until("the new lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        titles(s) == ["after restart"]
    });
}

fn type_at_start(s: &mut Lsp, text: &str) {
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(0);
            view.model_mut().insert(text);
            cx.notify();
        })
    });
    s.draw();
}

#[gpui::test]
fn a_retained_lens_lists_the_references_of_the_text_it_was_asked_on(cx: &mut TestAppContext) {
    let mut s = setup(cx, "rust-analyzer.showReferences");
    s.until("the lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        lens(s).is_some()
    });
    // An unsaved line above `d`, and the lens asked again on that text:
    // its references are on rows the file on disk doesn't have.
    let main = s.uri("src/z.rs");
    let locations = json!([
        location("src/other.rs", &s, 0, 6, 7),
        location("src/z.rs", &s, 4, 3, 4),
    ]);
    s.fake.set_reply(
        LENS,
        Scripted::now(Reply::Result(json!([{
            "range": { "start": { "line": 4, "character": 3 }, "end": { "line": 4, "character": 4 } },
            "command": {
                "title": "moved",
                "command": "rust-analyzer.showReferences",
                "arguments": [main.to_string(), { "line": 4, "character": 3 }, locations],
            },
        }]))),
    );
    type_at_start(&mut s, "// x\n");
    s.cx.executor()
        .advance_clock(REST + Duration::from_millis(10));
    s.until("the new lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        titles(s) == ["moved"]
    });
    for _ in 0..5 {
        s.clock.advance(Duration::from_millis(20));
        s.draw();
    }
    // Typing on, the lens is retained while its replacement is asked for.
    type_at_start(&mut s, "// y\n");
    click(&mut s);
    let panel = panel(&s);
    s.until("the references", |s| {
        panel.read_with(&s.cx, |p, _| p.shown_rows().len() == 4)
    });
    let (rows, header) = panel.read_with(&s.cx, |p, _| (p.shown_rows(), p.header_text()));
    assert_eq!(
        rows,
        [
            ShownRow::File("src/z.rs".into(), 1),
            reference(5, "fn d() {}", 3..4),
            ShownRow::File("src/other.rs".into(), 1),
            reference(1, "// 😀 d()", 8..9),
        ]
    );
    assert_eq!(header.as_deref(), Some("References to `d`"));
    // Opened, the symbol is selected where it is now, below both unsaved lines.
    selected_at(&mut s, &panel, 0, |text| text.find("fn d").unwrap() + 3);
}

fn unusable_arguments_are_not_sent(cx: &mut TestAppContext, command: &str) {
    let mut s = setup_with(cx, command, false);
    click(&mut s);
    s.until("the toast", |s| {
        s.clock.advance(Duration::from_millis(20));
        !toasts(s).is_empty()
    });
    assert_eq!(toasts(&s), ["2 references: this command isn't supported"]);
    let executed = (s.fake.records().iter())
        .filter(|r| matches!(r, Record::Request { method, .. } if method == EXECUTE))
        .count();
    assert_eq!(executed, 0, "{command}");
}

#[gpui::test]
fn rust_analyzer_references_with_unusable_arguments_are_not_sent(cx: &mut TestAppContext) {
    unusable_arguments_are_not_sent(cx, "rust-analyzer.showReferences");
}

#[gpui::test]
fn vscode_references_with_unusable_arguments_are_not_sent(cx: &mut TestAppContext) {
    unusable_arguments_are_not_sent(cx, "editor.action.showReferences");
}

fn toasts(s: &Lsp) -> Vec<String> {
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    layer.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

fn tick(s: &mut Lsp, by: Duration) {
    s.cx.executor().advance_clock(by);
    s.clock.advance(by);
    s.draw();
}

fn lens_reply(title: Option<&str>) -> Value {
    let range =
        json!({ "start": { "line": 3, "character": 3 }, "end": { "line": 3, "character": 4 } });
    match title {
        Some(title) => json!([{ "range": range, "command": { "title": title, "command": "x" } }]),
        None => json!([{ "range": range, "data": { "id": 1 } }]),
    }
}

/// Opens the `n`th reference row of the panel (from the first), and the
/// text the active editor then selects, with the editor's whole text.
fn open_reference(s: &mut Lsp, panel: &Entity<ReferencesPanel>, n: isize) -> (String, String) {
    s.cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.move_selection(n, cx);
            p.open_selected(window, cx)
        })
    });
    s.draw();
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    editor.read_with(&s.cx, |view, _| {
        let model = view.model();
        let text = model.text().to_string();
        let range = model.primary_selection().range;
        (text[range].to_string(), text)
    })
}

/// Whether `text`'s `at` is the symbol that was selected: `want` is its
/// start.
fn selected_at(
    s: &mut Lsp,
    panel: &Entity<ReferencesPanel>,
    n: isize,
    want: impl Fn(&str) -> usize,
) {
    let (selected, text) = open_reference(s, panel, n);
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    let range = editor.read_with(&s.cx, |view, _| view.model().primary_selection().range);
    assert_eq!(selected, "d");
    assert_eq!(range.start, want(&text), "the symbol where it is now");
}

#[gpui::test]
fn another_documents_edit_after_the_answer_doesnt_change_the_previews(cx: &mut TestAppContext) {
    let mut s = setup(cx, "rust-analyzer.showReferences");
    s.until("the lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        lens(s).is_some()
    });
    // The other file edited after the answer landed: the lens's locations
    // are still read in the text the server had when it answered.
    s.open("src/other.rs");
    type_at_start(&mut s, "// r\n");
    s.open("src/z.rs");
    click(&mut s);
    let panel = panel(&s);
    s.until("the references", |s| {
        panel.read_with(&s.cx, |p, _| p.shown_rows().len() == 4)
    });
    let rows = panel.read_with(&s.cx, |p, _| p.shown_rows());
    assert_eq!(
        rows,
        [
            ShownRow::File("src/z.rs".into(), 1),
            reference(4, "fn d() {}", 3..4),
            ShownRow::File("src/other.rs".into(), 1),
            reference(1, "// 😀 d()", 8..9),
        ]
    );
    // Opened, the other document's symbol is selected where it is now.
    selected_at(&mut s, &panel, 1, |text| text.find("d()").unwrap());
}

#[gpui::test]
fn an_answer_for_text_the_buffer_moved_on_from_is_dropped(cx: &mut TestAppContext) {
    let mut s = setup(cx, "rust-analyzer.showReferences");
    s.until("the lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        lens(s).is_some()
    });
    // The second ask is slow; the text moves on before it answers.
    s.fake.set_reply(
        LENS,
        Scripted {
            reply: Reply::Result(lens_reply(Some("stale"))),
            delay: Duration::from_secs(1),
        },
    );
    type_at_start(&mut s, "// x\n");
    tick(&mut s, REST + Duration::from_millis(10));
    s.until("the second request", |s| {
        s.clock.advance(Duration::from_millis(20));
        s.fake
            .records()
            .iter()
            .filter(|r| matches!(r, Record::Request { method, .. } if method == LENS))
            .count()
            >= 2
    });
    s.fake.set_reply(
        LENS,
        Scripted::now(Reply::Result(lens_reply(Some("fresh")))),
    );
    type_at_start(&mut s, "// y\n");
    tick(&mut s, Duration::from_secs(1));
    assert_ne!(titles(&s), ["stale"]);
    tick(&mut s, REST + Duration::from_millis(10));
    s.until("the fresh lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        titles(s) == ["fresh"]
    });
}

#[gpui::test]
fn a_lens_resolving_while_the_buffer_moves_on_is_dropped(cx: &mut TestAppContext) {
    let mut s = setup(cx, "rust-analyzer.showReferences");
    s.until("the lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        lens(s).is_some()
    });
    let resolved = |title: &str| {
        json!({ "range": lens_reply(None)[0]["range"].clone(), "data": { "id": 1 },
            "command": { "title": title, "command": "x" } })
    };
    s.fake
        .set_reply(LENS, Scripted::now(Reply::Result(lens_reply(None))));
    s.fake.set_reply(
        RESOLVE,
        Scripted {
            reply: Reply::Result(resolved("stale")),
            delay: Duration::from_secs(1),
        },
    );
    type_at_start(&mut s, "// x\n");
    tick(&mut s, REST + Duration::from_millis(10));
    s.until("the resolve", |s| {
        s.clock.advance(Duration::from_millis(20));
        s.fake
            .records()
            .iter()
            .any(|r| matches!(r, Record::Request { method, .. } if method == RESOLVE))
    });
    // The text moves on while the server resolves.
    s.fake
        .set_reply(RESOLVE, Scripted::now(Reply::Result(resolved("fresh"))));
    type_at_start(&mut s, "// y\n");
    tick(&mut s, Duration::from_secs(1));
    assert_ne!(titles(&s), ["stale"]);
    tick(&mut s, REST + Duration::from_millis(10));
    s.until("the fresh lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        titles(s) == ["fresh"]
    });
}

#[gpui::test]
fn a_stale_unresolved_lens_is_not_sent_to_resolve(cx: &mut TestAppContext) {
    let mut s = setup(cx, "rust-analyzer.showReferences");
    s.until("the lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        lens(s).is_some()
    });
    let resolves = |s: &Lsp| {
        s.fake
            .records()
            .iter()
            .filter(|r| matches!(r, Record::Request { method, .. } if method == RESOLVE))
            .count()
    };
    // Let the first answer's own resolves finish.
    tick(&mut s, REST);
    let before = resolves(&s);
    // The next answer has no command, but the text moves on before it lands.
    s.fake.set_reply(
        LENS,
        Scripted {
            reply: Reply::Result(lens_reply(None)),
            delay: Duration::from_secs(1),
        },
    );
    type_at_start(&mut s, "// x\n");
    tick(&mut s, REST + Duration::from_millis(10));
    s.until("the second request", |s| {
        s.clock.advance(Duration::from_millis(20));
        s.fake
            .records()
            .iter()
            .filter(|r| matches!(r, Record::Request { method, .. } if method == LENS))
            .count()
            >= 2
    });
    // Later answers come with their command, so any resolve is the stale one's.
    s.fake.set_reply(
        LENS,
        Scripted::now(Reply::Result(lens_reply(Some("fresh")))),
    );
    type_at_start(&mut s, "// y\n");
    tick(&mut s, Duration::from_secs(1));
    tick(&mut s, REST + Duration::from_millis(10));
    s.until("the fresh lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        titles(s) == ["fresh"]
    });
    assert_eq!(resolves(&s), before, "a stale lens was sent to resolve");
}

#[gpui::test]
fn an_edit_drained_before_resolve_admission_sends_no_resolve(cx: &mut TestAppContext) {
    let mut s = setup(cx, "rust-analyzer.showReferences");
    s.until("the lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        lens(s).is_some()
    });
    s.fake
        .set_reply(LENS, Scripted::now(Reply::Result(lens_reply(None))));
    let (manager, buffer, version) = s.workbench.read_with(&s.cx, |w, cx| {
        let manager = w.lsp.manager.clone().expect("a manager");
        let editor = w.active_editor(cx).expect("an editor");
        let model = editor.read(cx).model();
        (manager, model.buffer().id(), model.snapshot().version())
    });
    // Asked straight to the manager, with no workbench in between.
    let lenses =
        futures::executor::block_on(super::super::lock(&manager).code_lenses(buffer, version))
            .unwrap()
            .unwrap();
    assert!(lenses[0].command.is_none());
    let resolves = |s: &Lsp| {
        s.fake
            .records()
            .iter()
            .filter(|r| matches!(r, Record::Request { method, .. } if method == RESOLVE))
            .count()
    };
    // The edit is queued for the server before the resolve, which the
    // server thread admits only after draining it.
    type_at_start(&mut s, "// x\n");
    let before = resolves(&s);
    let reply = super::super::lock(&manager).resolve_code_lens(lenses[0].clone());
    let lens = futures::executor::block_on(reply).unwrap();
    assert!(lens.command.is_none(), "resolved: {:?}", lens.command);
    assert_eq!(resolves(&s), before, "a stale lens was sent to resolve");
}

#[gpui::test]
fn an_edit_drained_before_request_admission_sends_no_request(cx: &mut TestAppContext) {
    let mut s = setup(cx, "rust-analyzer.showReferences");
    s.until("the lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        lens(s).is_some()
    });
    let (manager, buffer, version) = s.workbench.read_with(&s.cx, |w, cx| {
        let manager = w.lsp.manager.clone().expect("a manager");
        let editor = w.active_editor(cx).expect("an editor");
        let model = editor.read(cx).model();
        (manager, model.buffer().id(), model.snapshot().version())
    });
    let requests = |s: &Lsp| {
        s.fake
            .records()
            .iter()
            .filter(|r| matches!(r, Record::Request { method, .. } if method == LENS))
            .count()
    };
    type_at_start(&mut s, "// x\n");
    let before = requests(&s);
    // Asked on the text from before the edit, which the server drains first.
    let reply = super::super::lock(&manager).code_lenses(buffer, version);
    let lenses = futures::executor::block_on(reply);
    assert!(
        !matches!(&lenses, Ok(Some(lenses)) if !lenses.is_empty()),
        "{lenses:?}"
    );
    assert_eq!(requests(&s), before, "a stale request was sent");
}
