//! Expand and shrink selection with vim on: the syntax-node keys grow vim's
//! visual selection and step back to normal mode. A host here grows through
//! a fixed list of nested ranges.

use std::ops::Range;

use fe_buffer::{Buffer, ResolvedSelection};
use fe_command::{Effect, Invocation, Registry};
use fe_editor::{EditorModel, commands::EditorHost};
use fe_vim::{
    Mode, Registers, Step, VimCommands, VimState, VimTarget, context, parse_keys, registry,
};

/// Nested ranges, innermost first.
struct Host(Vec<Range<usize>>);

impl EditorHost for Host {
    fn page_rows(&self) -> u32 {
        20
    }

    fn read_clipboard(&mut self) -> Option<String> {
        None
    }

    fn write_clipboard(&mut self, _: String) {}

    fn fold_range_at(&self, _: u32) -> Option<std::ops::RangeInclusive<u32>> {
        None
    }

    fn fold_ranges(&self, _: Range<u32>) -> Vec<std::ops::RangeInclusive<u32>> {
        Vec::new()
    }

    fn larger_syntax_node(&self, range: Range<usize>) -> Option<Range<usize>> {
        (self.0.iter())
            .find(|node| node.start <= range.start && range.end <= node.end && **node != range)
            .cloned()
    }
}

struct Vim {
    model: EditorModel,
    state: VimState,
    host: Host,
    registers: Registers,
    registry: Registry<VimCommands>,
}

impl Vim {
    /// `abcdefghijkl` with the caret at 4 and nodes 3..6, 2..8 and 0..12.
    fn new() -> Self {
        let mut model = EditorModel::new(Buffer::new("abcdefghijkl"), None);
        model.set_selections([ResolvedSelection {
            range: 4..4,
            reversed: false,
        }]);
        Self {
            model,
            state: VimState::new(),
            host: Host(vec![3..6, 2..8, 0..12]),
            registers: Registers::new(),
            registry: registry(),
        }
    }

    fn dispatch(&mut self, invocation: &Invocation) -> Effect {
        let invocation = invocation.clone().context(context(&self.state));
        let mut target = VimTarget {
            model: &mut self.model,
            host: &mut self.host,
            state: &mut self.state,
            registers: &mut self.registers,
        };
        fe_vim::dispatch(&self.registry, &mut target, &invocation)
            .unwrap()
            .effect
    }

    fn command(&mut self, id: &str) -> Effect {
        self.dispatch(&Invocation::of(id).unwrap())
    }

    fn feed(&mut self, keys: &str) {
        for key in parse_keys(keys).unwrap() {
            self.state.sync_visual(&mut self.model);
            if let Step::Commit(invocation) = self.state.feed(key) {
                self.dispatch(&invocation);
            }
        }
    }

    fn expand(&mut self) -> Effect {
        self.command("vim.expand_selection")
    }

    fn shrink(&mut self) -> Effect {
        self.command("vim.shrink_selection")
    }

    fn selected(&self) -> Range<usize> {
        self.model.primary_selection().range
    }

    fn visual(&self) -> bool {
        matches!(self.state.mode(), Mode::Visual(_))
    }

    fn text(&self) -> String {
        let text = self.model.text();
        text.slice_to_string(0..text.len())
    }
}

#[test]
fn expand_from_normal_enters_visual_and_grows() {
    let mut vim = Vim::new();
    assert_eq!(vim.expand(), Effect::Changed);
    assert!(vim.visual());
    assert_eq!(vim.selected(), 3..6);
    assert_eq!(vim.expand(), Effect::Changed);
    assert!(vim.visual());
    assert_eq!(vim.selected(), 2..8);
    // Nothing larger: nothing changes.
    vim.expand();
    assert_eq!(vim.selected(), 0..12);
    assert_eq!(vim.expand(), Effect::Nothing);
    assert_eq!(vim.selected(), 0..12);
}

#[test]
fn shrink_steps_back_to_normal_mode() {
    let mut vim = Vim::new();
    for _ in 0..3 {
        vim.expand();
    }
    assert_eq!(vim.selected(), 0..12);
    assert_eq!(vim.shrink(), Effect::Changed);
    assert!(vim.visual());
    assert_eq!(vim.selected(), 2..8);
    assert_eq!(vim.shrink(), Effect::Changed);
    assert!(vim.visual());
    assert_eq!(vim.selected(), 3..6);
    assert_eq!(vim.shrink(), Effect::Changed);
    assert_eq!(vim.state.mode(), Mode::Normal);
    assert_eq!(vim.selected(), 4..4);
    assert_eq!(vim.shrink(), Effect::Nothing);
    assert_eq!(vim.state.mode(), Mode::Normal);
    assert_eq!(vim.selected(), 4..4);
}

#[test]
fn a_motion_forgets_the_history() {
    let mut vim = Vim::new();
    vim.expand();
    vim.expand();
    vim.feed("l");
    assert!(vim.visual());
    assert_eq!(vim.shrink(), Effect::Nothing);
}

#[test]
fn then_d_deletes_the_node() {
    let mut vim = Vim::new();
    vim.expand();
    vim.expand();
    vim.feed("d");
    assert_eq!(vim.text(), "abijkl");
    assert_eq!(vim.state.mode(), Mode::Normal);
}

#[test]
fn shrinking_back_restores_a_caret_at_the_end_of_the_row() {
    let mut vim = Vim::new();
    vim.model.set_selections([ResolvedSelection {
        range: 12..12,
        reversed: false,
    }]);
    let nodes: Vec<Range<usize>> = std::iter::once(0..12).collect();
    vim.host = Host(nodes);
    assert_eq!(vim.expand(), Effect::Changed);
    assert_eq!(vim.selected(), 0..12);
    assert_eq!(vim.shrink(), Effect::Changed);
    assert_eq!(vim.state.mode(), Mode::Normal);
    assert_eq!(vim.selected(), 12..12);
}
