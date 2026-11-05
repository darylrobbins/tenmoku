//! The editor's commands. Every key binding dispatches one GPUI action,
//! kiln's [`RunCommand`], carrying an [`Invocation`] (see
//! [`crate::keymap`]). The focused [`EditorView`] runs it through the
//! editor's registry and lets any other ID propagate to the app's registry,
//! which kiln owns; [`init`] adds vim's commands to it.

use std::{
    cell::{Cell, RefCell},
    ops::Range,
    ops::RangeInclusive,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_buffer::{BufferSnapshot, Version};
use fe_command::{
    Actor, CommandError, CommandId, CommandMeta, ContextPredicate, DispatchError, Dispatched,
    Effect, Grants, Invocation, PluginPolicy, Registry, Source, Value,
};
use fe_editor::{
    CommentTokens, IndentSuggestion,
    commands::{Editor, EditorHost, EditorTarget, ScrollRequest, SyntaxObject, Viewport},
};
use fe_keymap::{CatalogError, CommandCatalog};
use fe_syntax::{Syntax, SyntaxDriver};
use gpui::{App, ClipboardItem, Context, Task, Window};

use crate::{
    EditorView, completion_menu, signature_help,
    view::{ghost_text, synced_syntax},
    vim,
};

// The app's command plumbing is kiln's (docs/decisions/app-framework.md);
// Tenmoku keeps its paths to it.
pub(crate) use kiln_platform::commands::notify_observers;
pub use kiln_platform::commands::{AppCommands, RunCommand, observe_invocations};

/// What a host such as a diff view sets on an editor and hears from it
/// (`EditorView::set_read_only`, `set_blocks` and `set_scroll_top`): their
/// types are reachable here, as the crate root doesn't re-export them.
pub use crate::view::{EditorViewEvent, HostBlock, HostBlockKind};

/// The name app commands are titled with (`Hide Tenmoku`).
const APP_NAME: &str = fe_config::APP_NAME;

thread_local! {
    /// Built once: GPUI runs on one thread, and handlers aren't `Sync`.
    static EDITOR_COMMANDS: Rc<Registry<Editor>> = Rc::new(editor_registry());
    /// What plugins' invocations of the editor's commands are checked
    /// against ([`set_plugin_policy`]).
    static PLUGIN_POLICY: RefCell<Option<Arc<dyn PluginPolicy>>> = const { RefCell::new(None) };
}

/// Checks plugins' invocations of the editor's commands against `policy`,
/// the app's plugin host. Until it's set every plugin is refused, as one
/// the host doesn't know.
pub fn set_plugin_policy(policy: Arc<dyn PluginPolicy>) {
    PLUGIN_POLICY.with(|installed| *installed.borrow_mut() = Some(policy));
}

/// The editor registry's [`PluginPolicy`]: whatever [`set_plugin_policy`]
/// installed, as the registry is built once, before the plugin host is.
struct InstalledPolicy;

impl PluginPolicy for InstalledPolicy {
    fn grants(&self, plugin: &str) -> Option<Grants> {
        PLUGIN_POLICY.with(|installed| installed.borrow().as_ref()?.grants(plugin))
    }

    fn workspace_roots(&self) -> Vec<PathBuf> {
        PLUGIN_POLICY.with(|installed| {
            (installed.borrow().as_ref())
                .map(|policy| policy.workspace_roots())
                .unwrap_or_default()
        })
    }
}

/// Shows the hover popover at the caret ([`crate::hover`]).
pub const SHOW_HOVER: &str = "editor.show_hover";

/// Moves each caret to its bracket's partner, or to the close of the pair
/// around it ([`EditorView::go_to_bracket`]).
pub const GO_TO_BRACKET: &str = "editor.go_to_bracket";

/// Saves the buffer: fe-editor's command, which the view runs off the main
/// thread ([`EditorView::save_from_command`]).
const SAVE: &str = "editor.save";

/// fe-editor's commands, and the view's own: in the editor's context, and
/// the completion menu's and the ghost text's in their own
/// ([`completion_menu`], [`ghost_text`]). The
/// view's do nothing to the model: once one has dispatched, the view does
/// its work ([`EditorView::run_command`]). Plugins are checked against
/// the [`InstalledPolicy`].
fn editor_registry() -> Registry<Editor> {
    let mut registry = fe_editor::commands::registry();
    registry.set_plugin_policy(Arc::new(InstalledPolicy));
    for (id, title) in [
        (SHOW_HOVER, "Show Hover"),
        (signature_help::SHOW, "Show Signature Help"),
        (GO_TO_BRACKET, "Go to Bracket"),
    ] {
        registry
            .register(id, title, |_: &mut EditorTarget<'_>, _: &Actor| {
                Ok(Dispatched::new(Effect::Nothing))
            })
            .expect("view command IDs are valid and distinct")
            .context(ContextPredicate::identifier(fe_editor::commands::CONTEXT));
    }
    let menu = [
        (
            completion_menu::TRIGGER,
            "Trigger Completion",
            fe_editor::commands::CONTEXT,
        ),
        (
            completion_menu::SELECT_PREV,
            "Select Previous Completion",
            completion_menu::KEY_CONTEXT,
        ),
        (
            completion_menu::SELECT_NEXT,
            "Select Next Completion",
            completion_menu::KEY_CONTEXT,
        ),
        (
            completion_menu::ACCEPT,
            "Accept Completion",
            completion_menu::KEY_CONTEXT,
        ),
        (
            completion_menu::DISMISS,
            "Dismiss Completion",
            completion_menu::KEY_CONTEXT,
        ),
        (
            ghost_text::ACCEPT,
            "Accept Suggestion",
            ghost_text::KEY_CONTEXT,
        ),
        (
            ghost_text::ACCEPT_WORD,
            "Accept Next Word of Suggestion",
            ghost_text::KEY_CONTEXT,
        ),
        (
            ghost_text::ACCEPT_LINE,
            "Accept Next Line of Suggestion",
            ghost_text::KEY_CONTEXT,
        ),
        (
            ghost_text::DISMISS,
            "Dismiss Suggestion",
            ghost_text::KEY_CONTEXT,
        ),
    ];
    for (id, title, context) in menu {
        registry
            .register(id, title, |_: &mut EditorTarget<'_>, _: &Actor| {
                Ok(Dispatched::new(Effect::Nothing))
            })
            .expect("view command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    registry
}

type AppCommand = fn(&mut App);

/// vim's app commands: `vim.toggle`, and `vim.key`, which does nothing
/// when dispatched: a binding to it hands its keystroke to vim instead
/// ([`crate::vim`]).
fn register_vim_commands(app: &mut Registry<AppCommands>) {
    let commands: [(&str, &str, AppCommand); 2] = [
        (vim::TOGGLE, "Toggle Vim Mode", |cx| {
            let enabled = vim::vim_enabled(cx);
            vim::set_vim_enabled(cx, !enabled)
        }),
        (vim::KEY, "Vim Key", |_| {}),
    ];
    for (id, title, run) in commands {
        app.register(id, title, move |cx: &mut &mut App, _: &Actor| {
            run(cx);
            Ok(Dispatched::new(Effect::Nothing))
        })
        .expect("vim's app command IDs are valid and distinct");
    }
}

/// kiln's app commands and vim's, as [`init`] installs them.
fn app_registry() -> Registry<AppCommands> {
    let mut app = kiln_platform::commands::app_commands(APP_NAME);
    register_vim_commands(&mut app);
    app
}

/// The editor's, vim's, the diff view's and the app's commands, to check
/// keymap bindings against: pass it to [`fe_keymap::KeymapStore::load`].
pub fn command_catalog() -> Box<dyn CommandCatalog> {
    Box::new(Catalog {
        editor: EDITOR_COMMANDS.with(Rc::clone),
        vim: vim::registry(),
        proposal: crate::view::proposal_registry(),
        diff: crate::diff_view::registry(),
        image: crate::image_viewer::registry(),
        app: app_registry(),
    })
}

struct Catalog {
    editor: Rc<Registry<Editor>>,
    vim: Rc<Registry<fe_vim::VimCommands>>,
    proposal: Registry<crate::view::ProposalCommands>,
    diff: Registry<crate::diff_view::DiffCommands>,
    image: Registry<crate::image_viewer::ImageCommands>,
    app: Registry<AppCommands>,
}

/// A command the editor doesn't know is checked against vim's, then the
/// proposal commands', then the diff view's, then the app's.
impl CommandCatalog for Catalog {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        let checks: [&dyn CommandCatalog; 5] = [
            &*self.editor,
            &*self.vim,
            &self.proposal,
            &self.diff,
            &self.image,
        ];
        for catalog in checks {
            match catalog.check(command, args) {
                Err(CatalogError::UnknownCommand) => {}
                result => return result,
            }
        }
        self.app.check(command, args)
    }
}

/// The editor's, vim's and the diff view's commands, for the command
/// palette to list: it lists each where its context holds.
pub fn command_list() -> Vec<CommandMeta> {
    let editor = EDITOR_COMMANDS.with(Rc::clone);
    let vim = vim::registry();
    let diff = crate::diff_view::registry();
    let image = crate::image_viewer::registry();
    (editor.commands().cloned())
        .chain(vim.commands().cloned())
        .chain(diff.commands().cloned())
        .chain(image.commands().cloned())
        .collect()
}

/// Registers the app commands, vim's among them, and the handler that runs
/// them.
pub(crate) fn init(cx: &mut App) {
    kiln_platform::commands::init(cx, APP_NAME);
    kiln_platform::commands::register_app_commands(cx, register_vim_commands);
}

thread_local! {
    /// What the last frame of the view about to dispatch showed: set by
    /// [`EditorView::page_rows`], which every dispatch asks first.
    pub(crate) static VIEWPORT: Cell<Option<Viewport>> = const { Cell::new(None) };
    /// The display columns [`VIEWPORT`] shows of an unwrapped row, set with it.
    pub(crate) static VIEW_COLUMNS: Cell<Option<std::ops::Range<u32>>> = const { Cell::new(None) };
    /// The display row [`VIEWPORT`] starts at, set with it.
    pub(crate) static TOP_DISPLAY_ROW: Cell<Option<u32>> = const { Cell::new(None) };
    /// The first row a command asked to show, which the view takes in
    /// [`EditorView::dispatched`].
    static SCROLLED: Cell<Option<u32>> = const { Cell::new(None) };
    /// Whether a command asked for the viewport, as every scroll does,
    /// even one that leaves the top row alone.
    static VIEWPORT_ASKED: Cell<bool> = const { Cell::new(false) };
}

/// The editor's side of a dispatch: the view's clipboard, viewport, syntax
/// folds, and its language's comment tokens and pairs.
pub(crate) struct ViewHost<'a> {
    pub(crate) cx: &'a mut App,
    pub(crate) page_rows: u32,
    /// The syntax tree, if it matches the text.
    pub(crate) syntax: Option<&'a Syntax>,
    /// The host's fold ranges, which replace the syntax's if it gave any.
    pub(crate) server_folds: Option<Rc<Vec<RangeInclusive<u32>>>>,
    /// The view's language, once loaded, whether or not its tree matches
    /// the text yet ([`loaded_language`]).
    pub(crate) language: Option<LoadedLanguage<'a>>,
    /// Vim is on: typing inserts exactly what was typed, so no pairs.
    pub(crate) vim: bool,
}

/// The language `syntax` parses, once it has loaded: known from the moment
/// an edit lands, unlike the tree, which matches the text only from the
/// next frame ([`synced_syntax`]).
pub(crate) fn loaded_language(
    syntax: &Option<SyntaxDriver<Task<()>>>,
) -> Option<LoadedLanguage<'_>> {
    let syntax = syntax.as_ref()?.syntax();
    Some(LoadedLanguage {
        name: syntax.language().name(),
        syntax,
        reparsed: RefCell::new(None),
        deadline: Cell::new(None),
    })
}

/// A view's loaded language, for one dispatch: its name, and its syntax,
/// whose tree may lag the text, which the indent hooks catch a copy of up
/// with the text they're asked about ([`reparsed`]).
pub(crate) struct LoadedLanguage<'a> {
    name: &'static str,
    syntax: &'a Syntax,
    /// The copy caught up with the last version asked about, or `None` in
    /// it if that ran out of time.
    reparsed: RefCell<Option<(Version, Option<Syntax>)>>,
    /// When the dispatch's parsing for indentation runs out of time: one
    /// [`INDENT_PARSE_BUDGET`] from its first.
    deadline: Cell<Option<Instant>>,
}

impl LoadedLanguage<'_> {
    /// `f` of the syntax caught up with `snapshot`, made once per version
    /// for the dispatch; `None` if it couldn't be in time.
    fn caught_up<R>(
        &self,
        snapshot: &BufferSnapshot,
        f: impl FnOnce(&Syntax) -> Option<R>,
    ) -> Option<R> {
        let mut reparsed = self.reparsed.borrow_mut();
        if !reparsed
            .as_ref()
            .is_some_and(|(version, _)| *version == snapshot.version())
        {
            let deadline =
                (self.deadline.get()).unwrap_or_else(|| Instant::now() + INDENT_PARSE_BUDGET);
            self.deadline.set(Some(deadline));
            let syntax = self::reparsed(self.syntax, snapshot, deadline);
            *reparsed = Some((snapshot.version(), syntax));
        }
        f(reparsed.as_ref()?.1.as_ref()?)
    }
}

impl EditorHost for ViewHost<'_> {
    fn page_rows(&self) -> u32 {
        self.page_rows
    }

    fn read_clipboard(&mut self) -> Option<String> {
        self.cx.read_from_clipboard().and_then(|item| item.text())
    }

    fn write_clipboard(&mut self, text: String) {
        self.cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn fold_range_at(&self, row: u32) -> Option<RangeInclusive<u32>> {
        if let Some(ranges) = &self.server_folds {
            return crate::server_folds::range_at(ranges, row);
        }
        self.syntax
            .and_then(|syntax| fe_syntax::fold_range_at(syntax, row))
    }

    fn fold_ranges(&self, rows: Range<u32>) -> Vec<RangeInclusive<u32>> {
        if let Some(ranges) = &self.server_folds {
            return crate::server_folds::ranges_in(ranges, rows);
        }
        self.syntax
            .map(|syntax| fe_syntax::fold_ranges(syntax, rows))
            .unwrap_or_default()
    }

    fn folds_exclusive(&self) -> bool {
        self.server_folds.is_some()
    }

    fn larger_syntax_node(&self, range: Range<usize>) -> Option<Range<usize>> {
        fe_syntax::larger_syntax_node(self.syntax?, range)
    }

    fn definition_objects(&self, offset: usize) -> Vec<SyntaxObject> {
        let Some(syntax) = self.syntax else {
            return Vec::new();
        };
        (fe_syntax::definition_objects(syntax, offset).into_iter())
            .map(|object| SyntaxObject {
                function: object.kind == fe_syntax::DefinitionKind::Function,
                outer: object.outer,
                inner: object.inner,
            })
            .collect()
    }

    fn function_bounds(&self) -> Vec<Range<usize>> {
        self.syntax
            .map(fe_syntax::function_bounds)
            .unwrap_or_default()
    }

    fn comment_tokens(&self) -> Option<CommentTokens> {
        let tokens = fe_syntax::comment_tokens(self.language.as_ref()?.name)?;
        Some(CommentTokens {
            line: tokens.line,
            block: tokens.block,
        })
    }

    fn language_name(&self) -> Option<&str> {
        Some(self.language.as_ref()?.name)
    }

    fn auto_pairs(&self) -> &[(char, char)] {
        if self.vim {
            return &[];
        }
        (self.language.as_ref()).map_or(&[], |language| fe_syntax::auto_pairs(language.name))
    }

    fn suggest_indent(&self, snapshot: &BufferSnapshot, row: u32) -> Option<IndentSuggestion> {
        let language = self.language.as_ref()?;
        let suggestion = language.caught_up(snapshot, |syntax| {
            fe_syntax::suggest_indent(syntax, snapshot.text(), row)
        })?;
        Some(match suggestion {
            fe_syntax::IndentSuggestion::Relative { basis_row, delta } => {
                IndentSuggestion::Relative { basis_row, delta }
            }
            fe_syntax::IndentSuggestion::Zero => IndentSuggestion::Zero,
        })
    }

    fn closer_outdents(&self, snapshot: &BufferSnapshot, row: u32) -> Option<u32> {
        // Vim outdents too, as nvim's 'indentkeys' do.
        (self.language.as_ref()?).caught_up(snapshot, |syntax| {
            fe_syntax::closer_outdents(syntax, snapshot.text(), row)
        })
    }

    fn viewport(&self) -> Option<Viewport> {
        VIEWPORT_ASKED.set(true);
        VIEWPORT.get()
    }

    fn view_columns(&self) -> Option<std::ops::Range<u32>> {
        let columns = VIEW_COLUMNS.take();
        VIEW_COLUMNS.set(columns.clone());
        columns
    }

    fn top_display_row(&self) -> Option<u32> {
        TOP_DISPLAY_ROW.get()
    }

    fn scroll(&mut self, request: ScrollRequest) {
        match request {
            ScrollRequest::TopRow(row) => SCROLLED.set(Some(row)),
        }
    }
}

/// Whether `invocation` places the view as nvim does: one of vim's scrolls
/// (`zt`, `<C-e>`…), even one that leaves the top row where it is, or `H`,
/// `M` or `L`, which move within the window nvim shows.
fn keeps_nvim_view(invocation: &Invocation) -> bool {
    let motion = invocation
        .args
        .get("motion")
        .and_then(|motion| motion.as_str());
    match invocation.command.as_str() {
        "vim.scroll" => true,
        "vim.move" => matches!(
            motion,
            Some("window_top" | "window_middle" | "window_bottom")
        ),
        _ => false,
    }
}

#[cfg(test)]
thread_local! {
    /// How many times [`reparsed`] has seeded a copy from the view's tree on
    /// this thread, for tests to tell a seeded copy from one parsed afresh.
    pub(crate) static SEEDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How long one dispatch's [`reparsed`] calls may take altogether.
const INDENT_PARSE_BUDGET: Duration = Duration::from_millis(20);

/// A copy of `syntax`, whose tree may lag, with its tree caught up with
/// `snapshot`, a later version of its buffer, as an edit asking how to
/// indent has just left it; `None` once `deadline` passes, or for a file
/// too big to parse or that parsing gave up on. The view's own tree is left
/// as it was, to catch up on a frame.
///
/// A tree parsed at its own version seeds the copy: reparsing that reuses
/// what it can of the tree, a few milliseconds for half a megabyte of
/// well-formed code, though about a parse around errors, and it's edited
/// forward to `snapshot`, to reparse only what changed. A tree waiting on
/// a reparse could take as long as a parse to seed it, so then `snapshot`
/// is parsed afresh. Every parse stops at `deadline`, and only there, however long
/// the seed took, so nothing runs on past it.
fn reparsed(syntax: &Syntax, snapshot: &BufferSnapshot, deadline: Instant) -> Option<Syntax> {
    let out_of_time = || Instant::now() >= deadline;
    if syntax.failed() || out_of_time() {
        return None;
    }
    let mut caught_up = Syntax::new(syntax.language(), snapshot);
    if syntax.tree().is_some() && !syntax.needs_parse() {
        caught_up.finish(syntax.parse_job().parse(out_of_time)?);
        #[cfg(test)]
        SEEDS.with(|n| n.set(n.get() + 1));
    }
    // Not `parse_until`: its budget on the last reparse's time would give up
    // after a slow seed, whatever the deadline.
    if caught_up.needs_parse() {
        caught_up.finish(caught_up.parse_job().parse(out_of_time)?);
    }
    (caught_up.tree().is_some() && !caught_up.failed()).then_some(caught_up)
}

/// Whether a read-only view refuses `invocation`, one of the editor's: it
/// edits, asks for or accepts a completion, or accepts a suggestion, where
/// the registry would run it.
fn refused(registry: &Registry<Editor>, invocation: &Invocation) -> bool {
    let command = invocation.command.as_str();
    let completes = command == completion_menu::TRIGGER
        || command == completion_menu::ACCEPT
        || [
            ghost_text::ACCEPT,
            ghost_text::ACCEPT_WORD,
            ghost_text::ACCEPT_LINE,
        ]
        .contains(&command);
    registry
        .validate(invocation)
        .is_ok_and(|meta| meta.mutates_buffer || completes)
}

impl EditorView {
    pub(crate) fn on_run_command(
        &mut self,
        run: &RunCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_command(run.0.clone(), Some(window), cx);
    }

    /// Runs `invocation` on this editor, in its context: the stack it was
    /// dispatched in, such as `[workbench, editor]` from a key or a menu,
    /// if the editor's frame is in it, or else the editor's frame alone. An
    /// ID the editor doesn't know propagates, still in that context, to the
    /// app's commands. With vim on, vim's IDs run through its registry, the
    /// editor's frame stamped from its vim state, and in insert or replace
    /// mode an editor command other than text input splits the session
    /// ([`fe_vim::VimState::split_session`]). In replace mode typed text and
    /// `editor.backspace` overwrite and put back
    /// ([`fe_vim::VimState::replace_text`]), and `vim.normal_mode` is
    /// replace mode's `Esc`, `vim.replace_normal_mode`. While a macro
    /// records, what runs is recorded as the keys it was
    /// ([`EditorView::record_typing`]). A read-only view refuses a command
    /// that edits, accepting or asking for a completion, and accepting a
    /// suggestion
    /// ([`EditorView::refuse_edit`]).
    ///
    /// Returns whether it ran without failing (a refused edit, a command's
    /// own error); [`EditorView::command_failed`] keeps that for a command
    /// the keymap dispatches, as a remap's `<Cmd>fe:…<CR>` does.
    ///
    /// Given the window, the view takes [`SAVE`] over from fe-editor's
    /// synchronous command: it saves off the main thread
    /// ([`EditorView::save_from_command`]).
    pub(crate) fn run_command(
        &mut self,
        invocation: Invocation,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) -> bool {
        let ok = self.run_command_inner(invocation, window, cx);
        self.command_failed = !ok;
        ok
    }

    fn run_command_inner(
        &mut self,
        invocation: Invocation,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) -> bool {
        let in_editor = ContextPredicate::identifier(fe_editor::commands::CONTEXT);
        let mut invocation = if in_editor.eval(&invocation.context) {
            invocation
        } else {
            invocation.context(fe_editor::commands::context())
        };
        self.forget_blurred_ghost_text_unless_taken(&invocation);
        self.sync_vim(cx);
        if self.vim.is_some() && vim::registry().get(&invocation.command).is_some() {
            self.record_vim_command(&invocation);
            if invocation.command.as_str() == vim::NORMAL_MODE && self.typing_mode() == Some(true) {
                invocation.command = vim::REPLACE_NORMAL_MODE
                    .parse()
                    .expect("a valid command ID");
            }
            invocation.context = vim::stamp(invocation.context, self.vim.as_ref());
            return self.run_vim(&invocation, cx);
        }
        let registry = EDITOR_COMMANDS.with(Rc::clone);
        if registry.get(&invocation.command).is_none() {
            let proposals = crate::view::proposal_registry();
            if proposals.get(&invocation.command).is_some() {
                return self.run_proposal_command(&proposals, &invocation, cx);
            }
        }
        if self.read_only && refused(&registry, &invocation) {
            self.refuse_edit(cx);
            return false;
        }
        let replace = self.typing_mode();
        if replace.is_some() && registry.validate(&invocation).is_ok() {
            self.record_typing(&invocation);
        }
        let page_rows = self.page_rows();
        let syntax = synced_syntax(&self.syntax, &self.model);
        let language = loaded_language(&self.syntax);
        let mut host = ViewHost {
            cx,
            page_rows,
            syntax,
            server_folds: self.server_fold_ranges(),
            language,
            vim: self.vim.is_some(),
        };
        let meta = registry.get(&invocation.command);
        let mutates = meta.is_some_and(|meta| meta.mutates_buffer);
        let saves = window.is_some() && invocation.command.as_str() == SAVE;
        let mut dispatch = |model: &mut fe_editor::EditorModel| {
            if saves {
                // The view saves once it has dispatched.
                registry
                    .validate(&invocation)
                    .map(|_| Dispatched::new(Effect::Nothing))
            } else {
                registry.dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
            }
        };
        let mut split = false;
        let result = match &mut self.vim {
            // In insert mode, `Enter` is a typed line break, and any other
            // editor command but text input splits the session (§9.8). An
            // invocation the registry would refuse does neither. So in
            // replace mode, where typing and `<BS>` are fe-vim's.
            Some(state) if replace.is_some() && meta.is_some() => {
                match registry.validate(&invocation) {
                    Err(error) => Err(error),
                    Ok(_) => match invocation.command.as_str() {
                        vim::NEWLINE => vim::line_break(state, &mut self.model, &host, &invocation),
                        vim::INSERT
                            if replace == Some(true)
                                || vim::types_keys(
                                    invocation.args.get("text").and_then(Value::as_str),
                                ) =>
                        {
                            let text = invocation.args.get("text").and_then(Value::as_str);
                            let text = text.unwrap_or_default();
                            let overwrite = replace == Some(true);
                            let model = &mut self.model;
                            vim::type_as_keys(state, model, &host, &invocation, text, overwrite)
                        }
                        vim::BACKSPACE if replace == Some(true) => {
                            vim::replace_backspace(state, &mut self.model, &invocation)
                        }
                        // `Tab` in a virtual session is typed as spaces over
                        // the columns they take.
                        "editor.tab" if state.is_virtual_replace() => vim::type_as_keys(
                            state,
                            &mut self.model,
                            &host,
                            &invocation,
                            "\t",
                            true,
                        ),
                        // Edit nothing, or as typing does, so the session
                        // goes on.
                        SHOW_HOVER | signature_help::SHOW => dispatch(&mut self.model),
                        // Accepting a suggestion types it, into the session.
                        command
                            if command == completion_menu::TRIGGER
                                || command.starts_with("completion.")
                                || command.starts_with("ghost_text.") =>
                        {
                            dispatch(&mut self.model)
                        }
                        command if vim::TEXT_INPUT.contains(&command) => dispatch(&mut self.model),
                        command => {
                            split = true;
                            let splits = if mutates {
                                fe_vim::Splits::Edits
                            } else if vim::MOTION_PREFIXES
                                .iter()
                                .any(|prefix| command.starts_with(prefix))
                            {
                                fe_vim::Splits::Moves
                            } else {
                                fe_vim::Splits::Other
                            };
                            state.split_session(&mut self.model, splits, dispatch)
                        }
                    },
                }
            }
            _ => dispatch(&mut self.model),
        };
        if split {
            // Ending the session may have dropped indentation, whatever the
            // command did.
            self.changed(cx);
        }
        let failed = matches!(result, Err(DispatchError::Failed { .. }));
        match self.dispatched(result, &invocation, mutates, cx) {
            Ok(()) if invocation.command.as_str() == SHOW_HOVER => self.show_hover(cx),
            Ok(()) if invocation.command.as_str() == GO_TO_BRACKET => self.go_to_bracket(cx),
            Ok(()) if invocation.command.as_str() == signature_help::SHOW => {
                self.ask_signature_help(signature_help::SignatureTrigger::Invoked, cx)
            }
            Ok(()) if saves => {
                if let Some(window) = window {
                    self.save_from_command(window, cx);
                }
            }
            Ok(()) => {
                let command = invocation.command.as_str();
                self.run_ghost_text_command(command, invocation.source, cx);
                if !self.run_completion_command(command, cx) {
                    self.sync_completion(cx);
                }
            }
            Err(DispatchError::UnknownCommand(_)) => {
                kiln_platform::commands::propagate_with_context(invocation, cx);
            }
            Err(error) => log::error!("{error}"),
        }
        !failed
    }

    /// `editor.go_to_bracket`: a caret next to a bracket goes to its partner,
    /// on the same side of it ([`fe_syntax::bracket_pair`]); any other goes to
    /// the closing bracket of the innermost pair around it. Every selection
    /// moves on its own and collapses to a caret. Nothing moves without a
    /// tree for the current text. A move is a jump: the host hears where the
    /// primary selection was ([`EditorViewEvent::Jumped`]).
    fn go_to_bracket(&mut self, cx: &mut Context<Self>) {
        let Some(syntax) = synced_syntax(&self.syntax, &self.model) else {
            return;
        };
        if syntax.needs_parse() {
            return;
        }
        let selections = self.model.selections();
        let target = |head: usize| {
            if let Some(pair) = fe_syntax::bracket_pair(syntax, head) {
                let (open, close) = (&pair.open, &pair.close);
                return Some(if close.start == head {
                    open.end
                } else if open.start == head {
                    close.end
                } else if close.end == head {
                    open.start
                } else {
                    close.start
                });
            }
            fe_syntax::enclosing_bracket_pair(syntax, head).map(|pair| pair.close.start)
        };
        let heads: Vec<(usize, usize)> = (selections.iter())
            .map(|selection| {
                let head = if selection.reversed {
                    selection.range.start
                } else {
                    selection.range.end
                };
                (head, target(head).unwrap_or(head))
            })
            .collect();
        if heads.iter().all(|(head, to)| head == to) {
            return;
        }
        let primary = selections.len() - 1;
        let from = heads[primary].0;
        self.record_vim_jump(cx);
        // A destination a fold hides is opened, so no caret is invisible.
        let text = self.model.text().clone();
        for &(_, to) in &heads {
            let row = text.offset_to_point(to).row;
            self.model.unfold_rows(row..=row);
        }
        self.model
            .set_selections(heads.iter().map(|&(_, to)| fe_buffer::ResolvedSelection {
                range: to..to,
                reversed: false,
            }));
        self.changed(cx);
        cx.emit(EditorViewEvent::Jumped { from });
    }

    /// Runs `invocation`, one of the proposal commands: dispatched through
    /// `proposals`, then the view does the op. A read-only view refuses those
    /// that accept, unless it takes Accepts all the same
    /// ([`EditorView::set_accepts_when_read_only`]).
    fn run_proposal_command(
        &mut self,
        proposals: &Registry<crate::view::ProposalCommands>,
        invocation: &Invocation,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut op = None;
        let result = match proposals.dispatch(&mut op, invocation) {
            Ok(_) => {
                let mutates = proposals
                    .get(&invocation.command)
                    .is_some_and(|meta| meta.mutates_buffer);
                if self.read_only && mutates && !self.accepts_when_read_only {
                    self.refuse_edit(cx);
                    return false;
                }
                let op = op.expect("a dispatched proposal command sets its op");
                self.run_proposal_op(op, cx)
                    .map(Dispatched::<()>::new)
                    .map_err(|error| DispatchError::Failed {
                        command: invocation.command.clone(),
                        error,
                    })
            }
            Err(error) => Err(error),
        };
        let failed = matches!(result, Err(DispatchError::Failed { .. }));
        if let Err(error) = self.dispatched(result, invocation, true, cx) {
            log::error!("{error}");
        }
        !failed
    }

    /// Applies a dispatch's effect and tells the observers, or logs a
    /// failed command; `mutates` is whether the command can edit. Returns
    /// any other error.
    pub(crate) fn dispatched<Tx>(
        &mut self,
        result: Result<Dispatched<Tx>, DispatchError>,
        invocation: &Invocation,
        mutates: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), DispatchError> {
        let ok = result.is_ok();
        match result {
            Ok(dispatched) => {
                match dispatched.effect {
                    Effect::Changed => self.changed(cx),
                    Effect::Notify => cx.notify(),
                    Effect::Nothing => {}
                }
                notify_observers(invocation, cx);
                self.write_primary(cx);
            }
            Err(DispatchError::Failed {
                error: CommandError(message),
                ..
            }) => {
                log::error!("{message}");
                // A failed edit, such as undo, may have moved the carets;
                // a failed save only repaints.
                if mutates {
                    self.changed(cx);
                } else {
                    cx.notify();
                }
            }
            Err(error) => {
                SCROLLED.set(None);
                VIEWPORT_ASKED.set(false);
                return Err(error);
            }
        }
        let asked = VIEWPORT_ASKED.take();
        match SCROLLED.take() {
            Some(top_row) => self.scroll_to_top_row(top_row),
            // A scroll that kept the top row still moved the caret, which
            // mustn't scroll the view back; so a motion that asked (`H`)
            // and left it in view. One that took it out of view (`G`, `'a`)
            // scrolls it in. A vim scroll, and `H`, `M` and `L`, keep nvim's
            // view; sticky scroll's band then spares the caret's row.
            None if ok && asked && self.shows_caret() => {
                self.pin_top_row(keeps_nvim_view(invocation))
            }
            None => {}
        }
        Ok(())
    }

    /// Whether the primary caret's display row is among the rows the view
    /// shows, as many as fit whole: from the display row a vim scroll put
    /// first since the last frame, or else that frame's first row, partway
    /// down a wrapped row as it may be. A command that kept the view
    /// ([`EditorView::pin_top_row`]) leaves either.
    fn shows_caret(&self) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        let frame = &layout.frame;
        let top = self.scrolled_display_top.unwrap_or(frame.first_row);
        let display = self.model.display();
        let caret = display.row_for_offset(self.model.primary_selection().head());
        (top..top.saturating_add(frame.visible_rows())).contains(&caret)
    }

    /// Types `text` at every caret, as `editor.insert` from the keyboard.
    pub(crate) fn type_text(&mut self, text: &str, cx: &mut Context<Self>) {
        // Replace mode overwrites: nothing to complete or commit.
        let inserting = self.typing_mode() != Some(true);
        let accepted = if inserting && text.chars().count() == 1 {
            self.commit_completion(text, cx)
        } else {
            None
        };
        let invocation = Invocation::of("editor.insert")
            .expect("valid command ID")
            .args(Value::from_iter([("text", text)]))
            .source(Source::Keyboard);
        self.run_command(invocation, None, cx);
        if inserting {
            self.completion_typed(text, cx);
        }
        self.signature_help_typed(text, cx);
        // After the character, so a new menu is asked at the final caret.
        self.run_accepted_command(accepted, cx);
    }
}

#[cfg(test)]
#[path = "auto_indent_host_tests.rs"]
mod auto_indent_host_tests;
#[cfg(test)]
#[path = "comment_host_tests.rs"]
mod comment_host_tests;
#[cfg(test)]
#[path = "function_bounds_tests.rs"]
mod function_bounds_tests;
#[cfg(test)]
#[path = "pairs_host_tests.rs"]
mod pairs_host_tests;
#[cfg(test)]
#[path = "syntax_objects_tests.rs"]
mod syntax_objects_tests;

#[cfg(test)]
mod language_name_tests {
    use super::*;
    use crate::tests::editor;
    use gpui::TestAppContext;

    /// The host's [`EditorHost::language_name`] for `view` as it is now.
    fn named(view: &gpui::Entity<EditorView>, cx: &mut gpui::VisualTestContext) -> Option<String> {
        view.update(cx, |view, cx| {
            let host = ViewHost {
                cx,
                page_rows: 20,
                syntax: None,
                server_folds: None,
                language: loaded_language(&view.syntax),
                vim: true,
            };
            host.language_name().map(str::to_string)
        })
    }

    #[gpui::test]
    fn host_names_the_buffers_language(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx, "|if a then end");
        assert_eq!(named(&view, cx), None);
        for language in ["lua", "bash", "make", "rust"] {
            view.update(cx, |view, cx| view.set_language(Some(language), cx));
            cx.run_until_parked();
            assert_eq!(named(&view, cx).as_deref(), Some(language));
        }
        view.update(cx, |view, cx| view.set_language(None, cx));
        cx.run_until_parked();
        assert_eq!(named(&view, cx), None);
    }
}
