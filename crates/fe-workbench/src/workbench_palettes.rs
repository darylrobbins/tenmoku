//! The palettes shown over the window, one at a time
//! ([`Workbench::show_palette`]): quick open and the language, theme,
//! go-to-line, recent and toolbar pickers (and those made afresh each
//! time: the buffer, git files, config files, keymaps, diagnostics and
//! buffer lines pickers), and the breadcrumbs and
//! navigation buttons they read from.

use std::path::PathBuf;

use fe_gpui::EditorView;
use gpui::{AppContext as _, Context, Entity, Focusable, WeakEntity, Window};
use kiln_shell::ModalId;

use super::recent_picker::RecentOp;
#[cfg(test)]
use super::recent_picker::RecentPicker;
use super::{Workbench, recent_picker};
use crate::{
    Pane, Picker, QuickOpen, open,
    pane_toolbar::{self, NavButtons, SymbolChain, ToolbarRequest},
};
#[cfg(test)]
use crate::{
    extensions_picker::ExtensionsPicker, go_to_line::GoToLine, language_picker::LanguagePicker,
    theme_picker::ThemePicker, toolbar_picker::ToolbarPicker,
};

impl Workbench {
    /// Shows `picker` as `palette` in the modal layer, in place of any
    /// other. The layer hides it once it's picked from, dismissed or left.
    pub(crate) fn show_palette<D: crate::PickerDelegate>(
        &mut self,
        palette: Palette,
        picker: Entity<Picker<D>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.modals.update(cx, |layer, cx| {
            layer.show_modal(palette.id(), picker, window, cx)
        });
        self.palette = Some(palette);
        cx.notify();
    }

    /// The quick-open palette for `root`, opening files in `pane`.
    pub(super) fn new_quick_open(
        root: Option<PathBuf>,
        pane: WeakEntity<Pane>,
        cx: &mut Context<Self>,
    ) -> Entity<Picker<QuickOpen>> {
        cx.new(|cx| {
            let delegate = QuickOpen::new(root, pane);
            Picker::new(delegate, "Go to file…", fe_ui::ui_theme(cx), cx)
        })
    }

    /// The quick-open palette, shown while [`Workbench::palette_open`].
    pub fn quick_open(&self) -> &Entity<Picker<QuickOpen>> {
        &self.quick_open
    }

    /// Whether the quick-open palette shows.
    pub fn palette_open(&self) -> bool {
        self.palette == Some(Palette::QuickOpen)
    }

    /// Shows the quick-open palette, focused, in place of any other, or
    /// dismisses it.
    pub(crate) fn toggle_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let show = !self.palette_open();
        if show {
            self.dismiss_palette(window, cx);
        }
        let previous = window.focused(cx);
        let picker = self.quick_open.clone();
        if show {
            let workbench = cx.entity().downgrade();
            picker.update(cx, |picker, cx| {
                picker
                    .delegate
                    .hand_symbols_to(workbench, window.window_handle());
                picker.delegate.opened(previous, cx);
                picker.show(window, cx);
            });
            self.show_palette(Palette::QuickOpen, picker, window, cx);
        } else {
            self.dismiss_palette(window, cx);
        }
        cx.notify();
    }

    /// The language picker, shown by `language.select`.
    #[cfg(test)]
    pub(crate) fn language_picker(&self) -> &Entity<Picker<LanguagePicker>> {
        &self.language_picker
    }

    /// Whether the language picker shows.
    #[cfg(test)]
    pub(crate) fn language_picker_open(&self) -> bool {
        self.palette == Some(Palette::Language)
    }

    /// Shows the language picker, focused, for the active editor, in place
    /// of any other palette, or afresh if it shows. Nothing without an
    /// active editor.
    pub(crate) fn select_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        // Dismissing it would hide it again once its dismissal lands.
        if self.palette != Some(Palette::Language) {
            self.dismiss_palette(window, cx);
        }
        let picker = self.language_picker.clone();
        picker.update(cx, |picker, cx| {
            picker.delegate.opened(&editor, cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::Language, picker, window, cx);
    }

    /// The theme picker, shown by `theme.select`.
    #[cfg(test)]
    pub(crate) fn theme_picker(&self) -> &Entity<Picker<ThemePicker>> {
        &self.theme_picker
    }

    /// Whether the theme picker shows.
    #[cfg(test)]
    pub(crate) fn theme_picker_open(&self) -> bool {
        self.palette == Some(Palette::Theme)
    }

    /// Shows the theme picker, focused, in place of any other palette, or
    /// afresh if it shows.
    pub(crate) fn select_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Dismissing it would hide it again once its dismissal lands.
        if self.palette != Some(Palette::Theme) {
            self.dismiss_palette(window, cx);
        }
        let picker = self.theme_picker.clone();
        let input = picker.focus_handle(cx);
        let previous = window.focused(cx).filter(|focused| *focused != input);
        let settings = open::settings_path(cx);
        picker.update(cx, |picker, cx| {
            picker.delegate.opened(previous, settings, cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::Theme, picker, window, cx);
    }

    /// The extensions picker, shown by `workbench.show_extensions`.
    #[cfg(test)]
    pub(crate) fn extensions_picker(&self) -> &Entity<Picker<ExtensionsPicker>> {
        &self.extensions_picker
    }

    /// Whether the extensions picker shows.
    #[cfg(test)]
    pub(crate) fn extensions_picker_open(&self) -> bool {
        self.palette == Some(Palette::Extensions)
    }

    /// Shows the extensions picker, focused, in place of any other
    /// palette, or afresh if it shows.
    pub(crate) fn show_extensions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Dismissing it would hide it again once its dismissal lands.
        if self.palette != Some(Palette::Extensions) {
            self.dismiss_palette(window, cx);
        }
        let picker = self.extensions_picker.clone();
        let input = picker.focus_handle(cx);
        let previous = window.focused(cx).filter(|focused| *focused != input);
        picker.update(cx, |picker, cx| {
            picker.delegate.opened(previous, cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::Extensions, picker, window, cx);
    }

    /// The go-to-line picker, shown by `go_to_line.toggle`.
    #[cfg(test)]
    pub(crate) fn go_to_line_picker(&self) -> &Entity<Picker<GoToLine>> {
        &self.go_to_line
    }

    /// Whether the go-to-line picker shows.
    #[cfg(test)]
    pub(crate) fn go_to_line_open(&self) -> bool {
        self.palette == Some(Palette::GoToLine)
    }

    /// Shows the go-to-line picker, focused, for the active editor, in
    /// place of any other palette, or dismisses it if it shows. Nothing
    /// without an active editor.
    pub(crate) fn toggle_go_to_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette == Some(Palette::GoToLine) {
            return self.dismiss_palette(window, cx);
        }
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        self.dismiss_palette(window, cx);
        let workbench = cx.entity().downgrade();
        let picker = self.go_to_line.clone();
        picker.update(cx, |picker, cx| {
            picker.delegate.opened(&editor, workbench, cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::GoToLine, picker, window, cx);
    }

    /// The recent folders and files picker, shown by `workbench.open_recent`.
    #[cfg(test)]
    pub(crate) fn recent_picker(&self) -> &Entity<Picker<RecentPicker>> {
        &self.recent_picker
    }

    /// Does a recent command: shows the picker, focused, in place of any
    /// other palette, or afresh if it shows; empties the list; or, while
    /// the picker shows, opens its selected folder in a new window, or its
    /// selected file as confirming it would.
    pub(crate) fn run_recent_op(
        &mut self,
        op: RecentOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picker = self.recent_picker.clone();
        match op {
            RecentOp::Open => {
                // Dismissing it would hide it again once its dismissal lands.
                if self.palette != Some(Palette::Recent) {
                    self.dismiss_palette(window, cx);
                }
                let input = picker.focus_handle(cx);
                let previous = window.focused(cx).filter(|focused| *focused != input);
                picker.update(cx, |picker, cx| {
                    picker.delegate.opened(previous, cx);
                    picker.show(window, cx);
                });
                self.show_palette(Palette::Recent, picker, window, cx);
            }
            RecentOp::Clear => {
                recent_picker::clear(cx);
                picker.update(cx, |picker, cx| picker.delegate.cleared(cx));
            }
            RecentOp::OpenInNewWindow if self.palette == Some(Palette::Recent) => {
                // After this update: confirming a file reads the workbench.
                window.defer(cx, move |window, cx| {
                    picker.update(cx, |picker, cx| {
                        let selected = picker.selected();
                        picker.delegate.open_in_new_window(selected, window, cx)
                    })
                });
            }
            RecentOp::OpenInNewWindow => cx.propagate(),
        }
    }

    /// The picker shown as `palette`, if it's the one showing: those made
    /// afresh each time they show, which the workbench doesn't keep.
    #[cfg(test)]
    pub(crate) fn shown_palette<D: crate::PickerDelegate>(
        &self,
        palette: Palette,
        cx: &gpui::App,
    ) -> Option<Entity<Picker<D>>> {
        (self.palette == Some(palette)).then_some(())?;
        let layer = self.shell().read(cx).modals().read(cx);
        layer.modal().filter(|id| *id == palette.id())?;
        layer.modal_view()?.clone().downcast().ok()
    }

    /// Dismisses the palette that shows, as Escape would.
    pub(crate) fn dismiss_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            (self.modals.clone()).update(cx, |layer, cx| layer.dismiss_modal(window, cx));
        }
    }

    /// Shows the toolbar picker, focused, for `request` from `pane`, in
    /// place of any other palette, or afresh if it shows.
    pub(super) fn open_toolbar_picker(
        &mut self,
        pane: &Entity<Pane>,
        request: ToolbarRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette != Some(Palette::Toolbar) {
            self.dismiss_palette(window, cx);
        }
        let input = self.toolbar_picker.focus_handle(cx);
        let previous = window.focused(cx).filter(|focused| *focused != input);
        let (pane, root) = (pane.downgrade(), self.root.clone());
        let workbench = cx.entity().downgrade();
        let picker = self.toolbar_picker.clone();
        picker.update(cx, |picker, cx| {
            picker
                .delegate
                .open(request, pane, workbench, root, previous, cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::Toolbar, picker, window, cx);
    }

    /// The toolbar picker, shown by a crumb or "…".
    #[cfg(test)]
    pub(crate) fn toolbar_picker(&self) -> Option<&Entity<Picker<ToolbarPicker>>> {
        (self.palette == Some(Palette::Toolbar)).then_some(&self.toolbar_picker)
    }

    /// Shows or hides every pane's toolbar, as `view.toggle_breadcrumbs`
    /// does.
    pub(crate) fn toggle_breadcrumbs(&mut self, cx: &mut Context<Self>) {
        self.breadcrumbs = !self.breadcrumbs;
        self.layout_keys.breadcrumbs.set(self.breadcrumbs);
        self.layout_keys_changed(cx);
        self.chain_key = None;
        let visible = self.breadcrumbs;
        for pane in self.panes(cx) {
            pane.update(cx, |pane, cx| {
                pane.update_toolbar(|toolbar| toolbar.visible = visible, cx)
            });
        }
        cx.notify();
    }

    /// Enables back and forward in every pane, as the navigation history
    /// allows.
    pub(crate) fn set_nav_buttons(&mut self, nav: NavButtons, cx: &mut Context<Self>) {
        self.nav = nav;
        #[cfg(test)]
        {
            self.nav_pushes += 1;
        }
        for pane in self.panes(cx) {
            pane.update(cx, |pane, cx| {
                pane.update_toolbar(|toolbar| toolbar.nav = nav, cx)
            });
        }
    }

    /// Pushes the symbols enclosing `editor`'s caret into the active pane,
    /// from the outline shown for it, if the toolbars show and the pane (a
    /// tab moved keeps its editor), the editor, its caret or the outline
    /// moved since the last time.
    pub(super) fn push_symbols(
        &mut self,
        editor: Option<&Entity<EditorView>>,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = editor.filter(|_| self.breadcrumbs) else {
            return;
        };
        let caret = pane_toolbar::caret(editor.read(cx));
        let outline = self.dock.read(cx).outline().read(cx);
        let followed = outline.shown_for(editor.entity_id());
        let shown = followed.flatten();
        let key = (
            self.active.entity_id(),
            editor.entity_id(),
            caret,
            followed.map(|shown| shown.map(|(_, generation)| generation)),
        );
        if self.chain_key == Some(key) {
            return;
        }
        self.chain_key = Some(key);
        // A pane made active again keeps its chain until the outline
        // following it there lands, unless its caret moved: so a click on
        // one of its crumbs, which activates it on the way down, still lands.
        let kept = &self.active.read(cx).toolbar().symbols;
        if key.3 == Some(None) && kept.editor == Some(key.1) && kept.caret == caret {
            return;
        }
        #[cfg(test)]
        {
            self.chains_computed += 1;
        }
        let chain = SymbolChain {
            editor: Some(editor.entity_id()),
            caret,
            generation: key.3.flatten().unwrap_or_default(),
            items: shown.map_or_else(Vec::new, |(outline, _)| {
                pane_toolbar::symbol_chain(outline, caret)
            }),
        };
        self.active.update(cx, |pane, cx| {
            pane.update_toolbar(|toolbar| toolbar.symbols = chain, cx)
        });
    }
}

/// The palette shown over the window, one at a time, in kiln-shell's
/// modal layer: fastedit's names for its [`ModalId`]s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Palette {
    QuickOpen,
    Language,
    Toolbar,
    Definitions,
    Symbols,
    ConfigProblems,
    GoToLine,
    /// The new name for `lsp.rename`.
    Rename,
    /// `lsp.code_actions`' list.
    CodeActions,
    Recent,
    /// `git.switch_branch`'s list, or the new branch's name.
    Branch,
    /// `theme.select`'s themes.
    Theme,
    /// `workbench.show_extensions`' list.
    Extensions,
    /// `nav.document_symbols`' list.
    DocumentSymbols,
    /// `buffers.show`'s open editors.
    Buffers,
    /// `git.show_files`' tracked and untracked files.
    GitFiles,
    /// `workbench.show_config_files`' files in the config folder.
    ConfigFiles,
    /// `keymaps.search`'s bindings and vim maps.
    Keymaps,
    /// `diagnostics.search_document`'s and `diagnostics.search_workspace`'s
    /// problems.
    Diagnostics,
    /// `search.buffer_lines`' rows of the active editor.
    BufferLines,
    /// `todo.list`'s TODO comments of the project.
    Todos,
}

impl Palette {
    const ALL: [Palette; 21] = [
        Palette::QuickOpen,
        Palette::Language,
        Palette::Toolbar,
        Palette::Definitions,
        Palette::Symbols,
        Palette::ConfigProblems,
        Palette::GoToLine,
        Palette::Rename,
        Palette::CodeActions,
        Palette::Recent,
        Palette::Branch,
        Palette::Theme,
        Palette::Extensions,
        Palette::DocumentSymbols,
        Palette::Buffers,
        Palette::GitFiles,
        Palette::ConfigFiles,
        Palette::Keymaps,
        Palette::Diagnostics,
        Palette::BufferLines,
        Palette::Todos,
    ];

    /// Its id in the modal layer.
    pub(crate) fn id(self) -> ModalId {
        ModalId(match self {
            Palette::QuickOpen => "quick_open",
            Palette::Language => "language",
            Palette::Toolbar => "toolbar",
            Palette::Definitions => "definitions",
            Palette::Symbols => "symbols",
            Palette::ConfigProblems => "config_problems",
            Palette::GoToLine => "go_to_line",
            Palette::Rename => "rename",
            Palette::CodeActions => "code_actions",
            Palette::Recent => "recent",
            Palette::Branch => "git_branch",
            Palette::Theme => "theme",
            Palette::Extensions => "extensions",
            Palette::DocumentSymbols => "document_symbols",
            Palette::Buffers => "buffers",
            Palette::GitFiles => "git_files",
            Palette::ConfigFiles => "config_files",
            Palette::Keymaps => "keymaps",
            Palette::Diagnostics => "diagnostics",
            Palette::BufferLines => "buffer_lines",
            Palette::Todos => "todos",
        })
    }

    /// The palette shown as `id`.
    pub(super) fn of(id: ModalId) -> Option<Self> {
        Self::ALL.into_iter().find(|palette| palette.id() == id)
    }
}
