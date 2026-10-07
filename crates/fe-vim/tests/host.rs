//! fe-vim driven as a host drives it, through its public API alone: an
//! editor's model, its vim state, the registers and vim's registry. In
//! insert mode the host types text into the model, and `Enter` through
//! `VimState::line_break`.

use fe_buffer::{Buffer, Origin};
use fe_command::{Invocation, Registry};
use fe_editor::{EditorModel, commands::HeadlessHost};
use fe_vim::{
    InsertArgs, InsertAt, Registers, VimCommands, VimState, VimTarget, context, registry,
};

struct Host {
    model: EditorModel,
    state: VimState,
    host: HeadlessHost,
    registers: Registers,
    registry: Registry<VimCommands>,
}

impl Host {
    /// `text` with a caret at byte `offset`.
    fn new(text: &str, offset: usize) -> Self {
        let mut model = EditorModel::new(Buffer::new(text), None);
        model.set_cursor(offset);
        Self {
            model,
            state: VimState::new(),
            host: HeadlessHost::new(),
            registers: Registers::new(),
            registry: registry(),
        }
    }

    fn dispatch(&mut self, invocation: Invocation) {
        let invocation = invocation.context(context(&self.state));
        let mut target = VimTarget {
            model: &mut self.model,
            host: &mut self.host,
            state: &mut self.state,
            registers: &mut self.registers,
        };
        self.registry.dispatch(&mut target, &invocation).unwrap();
    }

    /// `A`.
    fn append(&mut self) {
        let args = InsertArgs {
            at: InsertAt::LineEnd,
            count: None,
        };
        let invocation = Invocation::of("vim.insert")
            .unwrap()
            .args(serde_json::to_value(args).unwrap());
        self.dispatch(invocation);
    }

    /// `Esc` in insert mode.
    fn escape(&mut self) {
        self.dispatch(Invocation::of("vim.normal_mode").unwrap());
    }

    fn text(&self) -> String {
        let text = self.model.text();
        text.slice_to_string(0..text.len())
    }
}

fn ai() -> Origin {
    Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

#[test]
fn enter_through_line_break_is_autoindent() {
    // The indentation the line break copied goes if nothing is typed after
    // it, as Vim's 'autoindent' does.
    let mut host = Host::new("  abc", 0);
    host.append();
    host.state.line_break(&mut host.model);
    assert_eq!(host.text(), "  abc\n  ");
    host.escape();
    assert_eq!(host.text(), "  abc\n");
    // Typed after, it stays.
    let mut host = Host::new("  abc", 0);
    host.append();
    host.state.line_break(&mut host.model);
    host.model.insert("x");
    host.escape();
    assert_eq!(host.text(), "  abc\n  x");
    // Outside insert mode it's a line break, and nothing more.
    let mut host = Host::new("  abc", 5);
    host.state.line_break(&mut host.model);
    assert_eq!(host.text(), "  abc\n  ");

    // A line break the host types as input, not through `line_break`, is
    // typed text like any other: what it put in stays, and `.` keeps it.
    let mut host = Host::new("  abc", 0);
    host.append();
    host.model.newline();
    host.escape();
    assert_eq!(host.text(), "  abc\n  ");
    let last = host.state.last_change().unwrap();
    assert_eq!(last.inserted(), Some("\n  "));
}

/// Measuring text may edit the buffer through another model of it, inside
/// `EditorModel::newline` (see `EditorModel::set_char_widths`). Char widths
/// can't be set from here (`fe_display::CharWidth` isn't a dependency of
/// fe-vim), so edits right after the line break, before the session
/// follows them, stand in: to the session they're the same batches after
/// the line break's, and `VimState::line_break` notes the line break by
/// the version its batch starts from, read before it, not after.
#[test]
fn a_line_break_is_told_from_the_edits_after_it() {
    // The same origin's literal line break and blank stays...
    let mut host = Host::new("  abc", 0);
    host.append();
    host.state.line_break(&mut host.model);
    host.model.buffer_mut().edit([(8..8, "\n ")], Origin::User);
    assert_eq!(host.text(), "  abc\n  \n ");
    host.escape();
    assert_eq!(host.text(), "  abc\n\n ");
    // ...and another origin's edit leaves the line break the session's.
    let mut host = Host::new("  abc", 0);
    host.append();
    host.state.line_break(&mut host.model);
    host.model.buffer_mut().edit([(0..0, "!")], ai());
    host.escape();
    assert_eq!(host.text(), "!  abc\n");
    // So do edits made before it.
    let mut host = Host::new("  abc", 0);
    host.append();
    host.model.buffer_mut().edit([(0..0, "\n ")], Origin::User);
    host.state.line_break(&mut host.model);
    host.escape();
    assert_eq!(host.text(), "\n   abc\n");
}
