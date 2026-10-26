//! `af if ac ic`: a function or class and its body, from the functions and
//! classes the host's syntax lists around a caret, as
//! nvim-treesitter-textobjects' `@function.outer/inner` and
//! `@class.outer/inner` select them. A host here gives canned ones.

use std::ops::Range;

use fe_buffer::{Buffer, ResolvedSelection};
use fe_command::{Invocation, Registry};
use fe_editor::{
    EditorModel,
    commands::{EditorHost, SyntaxObject},
};
use fe_vim::{
    Mode, Pending, Registers, Step, VimCommands, VimKey, VimState, VimTarget, context, parse_keys,
    registry,
};

/// The host's syntax: the functions and classes it knows, in any order,
/// listed around an offset innermost first.
struct Host {
    objects: Vec<SyntaxObject>,
    clipboard: Option<String>,
}

impl EditorHost for Host {
    fn page_rows(&self) -> u32 {
        20
    }

    fn read_clipboard(&mut self) -> Option<String> {
        self.clipboard.clone()
    }

    fn write_clipboard(&mut self, text: String) {
        self.clipboard = Some(text);
    }

    fn fold_range_at(&self, _row: u32) -> Option<std::ops::RangeInclusive<u32>> {
        None
    }

    fn fold_ranges(&self, _rows: Range<u32>) -> Vec<std::ops::RangeInclusive<u32>> {
        Vec::new()
    }

    fn definition_objects(&self, offset: usize) -> Vec<SyntaxObject> {
        let mut around: Vec<SyntaxObject> = (self.objects.iter())
            .filter(|object| object.outer.contains(&offset))
            .cloned()
            .collect();
        around.sort_by_key(|object| object.outer.len());
        around
    }
}

/// An editor fed keys as the GPUI host routes them: through the reducer
/// outside insert mode and the command line, as typed text in them.
struct Vim {
    model: EditorModel,
    state: VimState,
    host: Host,
    registers: Registers,
    registry: Registry<VimCommands>,
    /// The command line's text, in `cmdline` mode.
    line: String,
}

impl Vim {
    fn new(text: &str, objects: Vec<SyntaxObject>) -> Self {
        Self {
            model: EditorModel::new(Buffer::new(text), None),
            state: VimState::new(),
            host: Host {
                objects,
                clipboard: None,
            },
            registers: Registers::new(),
            registry: registry(),
            line: String::new(),
        }
    }

    /// Carets at each of `offsets`.
    fn carets(&mut self, offsets: &[usize]) {
        self.model
            .set_selections(offsets.iter().map(|&offset| ResolvedSelection {
                range: offset..offset,
                reversed: false,
            }));
    }

    fn dispatch(&mut self, invocation: &Invocation) {
        let invocation = invocation.clone().context(context(&self.state));
        let mut target = VimTarget {
            model: &mut self.model,
            host: &mut self.host,
            state: &mut self.state,
            registers: &mut self.registers,
        };
        fe_vim::dispatch(&self.registry, &mut target, &invocation).unwrap();
    }

    fn feed(&mut self, keys: &str) {
        for key in parse_keys(keys).unwrap() {
            if self.state.mode() == Mode::Insert {
                match key {
                    VimKey::Char(ch) => self.model.insert(ch.encode_utf8(&mut [0; 4])),
                    VimKey::Esc => self.dispatch(&Invocation::of("vim.normal_mode").unwrap()),
                    _ => panic!("{key:?} in insert mode"),
                }
                continue;
            }
            if let Mode::CmdLine(_) = self.state.mode() {
                match key {
                    VimKey::Char(ch) => self.line.push(ch),
                    VimKey::Enter => {
                        let text = std::mem::take(&mut self.line);
                        let submit = Invocation::of("vim.command_line_submit")
                            .unwrap()
                            .args(serde_json::json!({ "text": text }));
                        self.dispatch(&submit);
                    }
                    _ => panic!("{key:?} on the command line"),
                }
                continue;
            }
            self.state.sync_visual(&mut self.model);
            match self.state.feed(key) {
                Step::Commit(invocation) => {
                    self.dispatch(&invocation);
                    // A command line starts from what vim puts on it.
                    self.line = self.state.command_line_prefill().to_string();
                }
                Step::Wait(_) | Step::Cancel => {}
                Step::Forward => panic!("{key:?} was forwarded"),
            }
        }
    }

    fn text(&self) -> String {
        let text = self.model.text();
        text.slice_to_string(0..text.len())
    }

    fn register(&self) -> String {
        (self.registers.get('"').map(|r| r.text.clone())).unwrap_or_default()
    }

    /// The text a visual selection from `keys` covers, by yanking it.
    fn selected(&mut self, keys: &str) -> String {
        self.feed(keys);
        self.feed("y");
        self.register()
    }
}

const CODE: &str = "\
struct S {
    x: u32,
}

impl S {
    fn a() {
        let y = 1;
    }

    fn b() {
        fn c() { inner(); }
        call();
    }

    fn e() {}
}
";

/// The bytes of `text` from the first `from` to the end of the first `to`
/// after it.
fn between(text: &str, from: &str, to: &str) -> Range<usize> {
    let start = text.find(from).unwrap();
    let end = start + text[start..].find(to).unwrap() + to.len();
    start..end
}

fn at(text: &str, needle: &str) -> usize {
    text.find(needle).unwrap()
}

/// What a syntax would list in `text` (a shape of [`CODE`]'s): each item
/// to the end of the first `end` after its start, its body what's between
/// its braces without the white space at its edges.
fn objects(text: &str) -> Vec<SyntaxObject> {
    let items = [
        ("struct S", "\n}", false),
        ("impl S", "\n}", false),
        ("fn a", "\n    }", true),
        ("fn b", "call();\n    }", true),
        ("fn c", "}", true),
        ("fn e", "}", true),
    ];
    (items.into_iter())
        .filter(|(start, _, _)| text.contains(start))
        .map(|(start, end, function)| {
            let outer = between(text, start, end);
            let open = outer.start + text[outer.clone()].find('{').unwrap() + 1;
            let body = &text[open..outer.end - 1];
            let lead = body.len() - body.trim_start().len();
            let inner = open + lead..open + lead + body.trim().len();
            SyntaxObject {
                function,
                outer,
                inner: Some(inner),
            }
        })
        .collect()
}

fn vim_at(needle: &str) -> Vim {
    let mut vim = Vim::new(CODE, objects(CODE));
    vim.carets(&[at(CODE, needle)]);
    vim
}

fn after(needle: &str, keys: &str) -> Vim {
    let mut vim = vim_at(needle);
    vim.feed(keys);
    vim
}

#[test]
fn definition_objects_delete_and_yank() {
    // `daf` takes the function's rows, as a delete over whole rows does.
    let fn_a = "    fn a() {\n        let y = 1;\n    }\n";
    let vim = after("let y", "daf");
    assert_eq!(vim.text(), CODE.replace(fn_a, ""));
    assert_eq!(vim.register(), fn_a);
    // From its name too.
    assert_eq!(after("a()", "daf").text(), CODE.replace(fn_a, ""));
    // `dif` the body, linewise: its rows are whole inside the braces.
    let vim = after("y = 1", "dif");
    assert_eq!(
        vim.text(),
        CODE.replace("    fn a() {\n        let y = 1;\n", "    fn a() {\n")
    );
    assert_eq!(vim.register(), "        let y = 1;\n");
    // A body on the braces' row: charwise.
    let vim = after("inner()", "dif");
    assert_eq!(vim.text(), CODE.replace("{ inner(); }", "{  }"));
    assert_eq!(vim.register(), "inner();");
    let vim = after("inner()", "cifdone();<Esc>");
    assert_eq!(vim.text(), CODE.replace("inner();", "done();"));
    // `yac` the class, charwise, leaving the text.
    let vim = after("x: u32", "yac");
    assert_eq!(vim.text(), CODE);
    assert_eq!(vim.register(), "struct S {\n    x: u32,\n}");
    // `cic` its body, linewise.
    let vim = after("u32", "cicy: i8,<Esc>");
    assert_eq!(vim.text(), CODE.replace("    x: u32,\n", "    y: i8,\n"));
    // A method's class is its impl.
    let vim = after("let y", "yic");
    let body = &CODE[at(CODE, "    fn a")..at(CODE, "}\n}") + 2];
    assert_eq!(vim.register(), body);
    // An empty body: nothing to delete, and `cif` types between the
    // braces.
    let vim = after("e()", "dif");
    assert_eq!(vim.text(), CODE);
    let vim = after("e()", "cifgo();<Esc>");
    assert_eq!(vim.text(), CODE.replace("fn e() {}", "fn e() {go();}"));
}

#[test]
fn definition_objects_count_and_nesting() {
    let fn_b = "    fn b() {\n        fn c() { inner(); }\n        call();\n    }\n";
    // `daf` in a nested function takes it, `2daf` the one around it. On
    // one row it's charwise, as a delete within a row is, leaving the
    // indent.
    let vim = after("inner()", "daf");
    assert_eq!(vim.text(), CODE.replace("fn c() { inner(); }", ""));
    let vim = after("inner()", "2daf");
    assert_eq!(vim.text(), CODE.replace(fn_b, ""));
    let vim = after("inner()", "d2af");
    assert_eq!(vim.text(), CODE.replace(fn_b, ""));
    // `2dif` the outer one's body, from the nested one's.
    let vim = after("inner()", "2yif");
    assert_eq!(
        vim.register(),
        "        fn c() { inner(); }\n        call();\n"
    );
    // A count past the outermost fails and changes nothing.
    for keys in ["3daf", "3dif", "9daf", "2dac", "2dic"] {
        let mut vim = vim_at("inner()");
        vim.feed("yl");
        vim.feed(keys);
        assert_eq!(vim.text(), CODE, "{keys}");
        assert_eq!(vim.register(), "i", "{keys}");
        assert_eq!(vim.state.mode(), Mode::Normal, "{keys}");
    }
    // On a field, in a class but no function: `ac` takes the class, `af`
    // fails.
    let vim = after("x: u32", "dac");
    assert_eq!(vim.text(), &CODE[at(CODE, "\nimpl")..]);
    let vim = after("x: u32", "daf");
    assert_eq!(vim.text(), CODE);
    let vim = after("x: u32", "dic");
    assert_eq!(vim.text(), CODE.replace("    x: u32,\n", ""));
    // `dac` in a method takes its impl, `2dac` none.
    let vim = after("call()", "dac");
    assert_eq!(vim.text(), &CODE[..at(CODE, "impl")]);
    // A function without a body: `af` takes it, `if` the body around it.
    let text = "impl T {\n    fn a() {\n        fn sig();\n    }\n}\n";
    let mut objects = vec![
        SyntaxObject {
            function: false,
            outer: between(text, "impl", "\n}"),
            inner: Some(between(text, "fn a", "\n    }")),
        },
        SyntaxObject {
            function: true,
            outer: between(text, "fn a", "\n    }"),
            inner: Some(between(text, "fn sig", ";")),
        },
    ];
    objects.push(SyntaxObject {
        function: true,
        outer: between(text, "fn sig", ";"),
        inner: None,
    });
    let mut vim = Vim::new(text, objects.clone());
    vim.carets(&[at(text, "sig")]);
    vim.feed("yaf");
    assert_eq!(vim.register(), "fn sig();");
    let mut vim = Vim::new(text, objects);
    vim.carets(&[at(text, "sig")]);
    vim.feed("yif");
    assert_eq!(vim.register(), "        fn sig();\n");
}

#[test]
fn definition_objects_visual_grows() {
    // `vaf`, then `af` again: the function around it.
    assert_eq!(vim_at("inner()").selected("vaf"), "fn c() { inner(); }");
    assert_eq!(
        vim_at("inner()").selected("vafaf"),
        "fn b() {\n        fn c() { inner(); }\n        call();\n    }"
    );
    // `v2af` at once; past the outermost it fails, keeping the selection.
    assert_eq!(
        vim_at("inner()").selected("v2af"),
        "fn b() {\n        fn c() { inner(); }\n        call();\n    }"
    );
    let mut vim = vim_at("inner()");
    vim.feed("vafafaf");
    assert!(vim.state.failed());
    assert_eq!(
        vim.selected(""),
        "fn b() {\n        fn c() { inner(); }\n        call();\n    }"
    );
    // `vif` then `af`: its function; `vif` then `if`: the one around's body.
    assert_eq!(vim_at("inner()").selected("vif"), "inner();");
    assert_eq!(vim_at("inner()").selected("vifaf"), "fn c() { inner(); }");
    assert_eq!(
        vim_at("inner()").selected("vifif"),
        "fn c() { inner(); }\n        call();"
    );
    // Over one position, the body of the item around it, as `yif` takes
    // it, though it doesn't hold the cursor: on a name, a brace or a class
    // header; counted, from a nested function's name.
    assert_eq!(vim_at("a()").selected("vif"), "let y = 1;");
    assert_eq!(vim_at("{\n        let").selected("vif"), "let y = 1;");
    assert_eq!(vim_at("c()").selected("vif"), "inner();");
    assert_eq!(
        vim_at("c()").selected("v2if"),
        "fn c() { inner(); }\n        call();"
    );
    assert_eq!(vim_at("S {\n    x").selected("vic"), "x: u32,");
    assert_eq!(
        vim_at("impl S").selected("vic"),
        &CODE[at(CODE, "fn a")..at(CODE, "{}\n}") + 2]
    );
    // Over one position of an object only as long as it: that object.
    assert_eq!(vim_at("let y").selected("vif"), "let y = 1;");
    // A wider selection takes the object holding it.
    assert_eq!(
        vim_at("inner()").selected("vjaf"),
        "fn b() {\n        fn c() { inner(); }\n        call();\n    }"
    );
    // `ac` and `ic` as well.
    assert_eq!(vim_at("x: u32").selected("vic"), "x: u32,");
    assert_eq!(
        vim_at("x: u32").selected("vicac"),
        "struct S {\n    x: u32,\n}"
    );
    assert_eq!(
        vim_at("x: u32").selected("vac"),
        "struct S {\n    x: u32,\n}"
    );
    let mut vim = vim_at("x: u32");
    vim.feed("vacac");
    assert!(vim.state.failed());
    let impl_s = &CODE[at(CODE, "impl")..CODE.len() - 1];
    assert_eq!(vim_at("let y").selected("vac"), impl_s);
    assert_eq!(
        vim_at("let y").selected("vic"),
        &CODE[at(CODE, "fn a")..at(CODE, "{}\n}") + 2]
    );
    // From linewise visual too, the mode becoming charwise.
    let mut vim = vim_at("let y");
    vim.feed("Vaf");
    assert_eq!(vim.state.mode(), Mode::Visual(fe_vim::VisualKind::Char));
    assert_eq!(vim.selected(""), "fn a() {\n        let y = 1;\n    }");
    // An empty body selects nothing: it fails.
    let mut vim = vim_at("e()");
    vim.feed("vif");
    assert!(vim.state.failed());
}

#[test]
fn definition_objects_repeat_and_carets() {
    let fn_a = "    fn a() {\n        let y = 1;\n    }\n";
    let fn_c = "fn c() { inner(); }";
    // `.` repeats `daf` where the caret is now, with the chain there.
    let mut vim = vim_at("let y");
    vim.feed("daf");
    let text = vim.text();
    vim.host.objects = objects(&text);
    vim.carets(&[at(&text, "inner()")]);
    vim.feed(".");
    assert_eq!(vim.text(), CODE.replace(fn_a, "").replace(fn_c, ""));
    // And `cif`, with what it typed.
    let mut vim = vim_at("let y");
    vim.feed("cifz();<Esc>");
    let text = vim.text();
    vim.host.objects = objects(&text);
    vim.carets(&[at(&text, "inner()")]);
    vim.feed(".");
    assert_eq!(
        vim.text(),
        CODE.replace("        let y = 1;\n", "        z();\n")
            .replace("inner();", "z();")
    );
    // Several carets: each takes its own function.
    let mut vim = Vim::new(CODE, objects(CODE));
    vim.carets(&[at(CODE, "let y"), at(CODE, "inner()")]);
    vim.feed("daf");
    assert_eq!(vim.text(), CODE.replace(fn_a, "").replace(fn_c, ""));
    let mut vim = Vim::new(CODE, objects(CODE));
    vim.carets(&[at(CODE, "let y"), at(CODE, "inner()")]);
    vim.feed("cifq<Esc>");
    assert_eq!(
        vim.text(),
        CODE.replace("        let y = 1;\n", "        q\n")
            .replace("inner();", "q")
    );
    // One with no function leaves the others to act.
    let mut vim = Vim::new(CODE, objects(CODE));
    vim.carets(&[at(CODE, "x: u32"), at(CODE, "inner()")]);
    vim.feed("daf");
    assert_eq!(vim.text(), CODE.replace(fn_c, ""));
}

#[test]
fn definition_objects_without_syntax_cancel() {
    // As an object nvim doesn't know (`q`): no edit, nothing pending, no
    // message, and no change for `.`.
    for keys in ["daf", "dif", "dac", "dic", "yaf", "cif", "dq"] {
        let mut vim = Vim::new(CODE, Vec::new());
        vim.carets(&[at(CODE, "let y")]);
        vim.feed(keys);
        assert_eq!(vim.text(), CODE, "{keys}");
        assert_eq!(vim.state.mode(), Mode::Normal, "{keys}");
        assert_eq!(*vim.state.pending(), Pending::default(), "{keys}");
        assert_eq!(vim.state.message(), None, "{keys}");
        assert!(vim.state.last_change().is_none(), "{keys}");
        assert!(vim.registers.get('"').is_none(), "{keys}");
        // What comes next is a key like any other.
        vim.feed("x");
        assert_eq!(vim.text(), CODE.replacen("let y", "et y", 1), "{keys}");
    }
    // In a visual mode it fails, keeping the selection.
    let mut vim = Vim::new(CODE, Vec::new());
    vim.carets(&[at(CODE, "let y")]);
    vim.feed("vlaf");
    assert!(vim.state.failed());
    assert_eq!(vim.selected(""), "le");
}

#[test]
fn definition_objects_surround_and_filter() {
    // `ysaf)` wraps the function, and `.` the one at another caret.
    let mut vim = vim_at("inner()");
    vim.feed("ysaf)");
    let wrapped = CODE.replace("fn c() { inner(); }", "(fn c() { inner(); })");
    assert_eq!(vim.text(), wrapped);
    vim.host.objects = objects(&wrapped);
    vim.carets(&[at(&wrapped, "let y")]);
    vim.feed(".");
    assert_eq!(
        vim.text(),
        wrapped
            .replace("fn a() {", "(fn a() {")
            .replace("let y = 1;\n    }", "let y = 1;\n    })")
    );
    // `!af` filters the function's rows, and `.` another function's.
    let rows = |vim: &mut Vim| match vim.state.take_host_command() {
        Some(fe_vim::HostCommand::Filter { rows, .. }) => rows,
        other => panic!("{other:?}"),
    };
    let mut vim = vim_at("let y");
    vim.feed("!af");
    assert_eq!(vim.state.mode(), Mode::CmdLine(fe_vim::CommandLineKind::Ex));
    vim.feed("sort<CR>");
    assert_eq!(rows(&mut vim), 5..8);
    vim.carets(&[at(CODE, "call()")]);
    vim.feed(".");
    assert_eq!(rows(&mut vim), 9..13);
}
