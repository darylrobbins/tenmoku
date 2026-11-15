//! The Problems panel's detail pane (`docs/design/design-system.md#problems`):
//! for the selected problem (or the problem of a selected related row),
//! below the list, or beside it while the panel is [`BESIDE_WIDTH`] or wider,
//! as in the bottom dock (as the Runs panel places a test run's detail),
//! where a pane below would leave the list a row or two.
//!
//! - **Code and source**, in code small: `code · source`, the code a link
//!   to the page describing it (`codeDescription`) when the server gave one.
//! - **The message**, whole, wrapped.
//! - **A snippet**: the problem's row and one row either side, in the
//!   editor's font, with row numbers, the problem's range underlined in its
//!   severity's colour. The text is the open buffer's, else the file's as
//!   it is on disk, read off the main thread ([`super::read_text`]); a file
//!   that can't be read shows no snippet.
//! - **The suggested fix**, when the language server has one for the
//!   problem: it's asked for the `quickfix` actions over the problem's range
//!   with the problem's diagnostic, automatically, as the problem is selected
//!   in an open buffer. The enabled quick fixes are taken preferred
//!   (`isPreferred`) first, then those fixing the diagnostic, in fe-lsp's
//!   order; the first is resolved, and if its edit changes the buffer, the
//!   rows it changes show as they'd read once fixed, a celadon-soft line
//!   each with a + glyph, the first with the fix's title. A fix that edits
//!   only other files, or has no edit (a bare command), shows nothing.
//! - **Apply fix** (`problems.apply_fix`, ⌥↵ in the panel's key context)
//!   applies that action as the code-action picker would
//!   ([`Workbench::apply_code_action`]): its edit, then its command. With no
//!   fix there's no button and the command does nothing.
//!
//! The pane follows what it shows as the panel draws: a problem selected,
//! moved or edited around (its buffer's version) is looked up again, and an
//! answer for what's no longer shown is dropped. Ask an agent and Explain
//! are a later card's.

use std::{
    cell::Cell,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::{BufferId, Version};
use fe_command::{CommandId, ContextFrame, ContextStack, Value};
use fe_editor::diagnostics::Problem;
use fe_gpui::EditorView;
use fe_lsp::{CodeAction, EditTarget, FileEdits, code_action::CodeActionQuery};
use fe_text::Text;
use fe_ui::{ActiveTheme as _, Button, CODE_SMALL, FamilySlot, FontFamilies, UI_SMALL};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EntityId, InteractiveElement as _,
    IntoElement as _, ParentElement as _, Pixels, StatefulInteractiveElement as _, Styled as _,
    Task, WeakEntity, Window, canvas, div, prelude::FluentBuilder as _, px, relative, svg,
};
use kiln::Cx;
use kiln_l10n::text;

use super::{KEY_CONTEXT, ProblemsPanel, read_text};

/// The command applying the suggested fix.
pub(crate) const APPLY_FIX: &str = "problems.apply_fix";

/// The kind of the actions a fix is one of.
const QUICKFIX: &str = "quickfix";

/// The pane's test selector.
pub(crate) const DETAIL_SELECTOR: &str = "problems-detail";

/// The fix's first line's test selector.
pub(crate) const FIX_SELECTOR: &str = "problems-detail-fix";

/// The Apply fix button's element id and test selector.
pub(crate) const APPLY_SELECTOR: &str = "problems-detail-apply";

/// The code link's element id.
const CODE_LINK: &str = "problems-detail-code";

/// Between the code and the source.
const SEPARATOR: &str = "·";

/// What the pane shows, and what it found for it.
pub(super) struct Detail {
    /// Whether the panel was last drawn [`BESIDE_WIDTH`] or wider, for the
    /// pane beside the list.
    wide: Rc<Cell<bool>>,
    /// The problem shown, as it was when its fix was looked up.
    shown: Option<Shown>,
    /// A file no editor shows, read for the snippet: its path, and its text
    /// once read (`None` inside if it can't be), with the read.
    read: Option<(PathBuf, Option<Option<Text>>, Task<()>)>,
    fix: Lookup,
}

impl Default for Detail {
    fn default() -> Self {
        Self {
            wide: Rc::new(Cell::new(true)),
            shown: None,
            read: None,
            fix: Lookup::None,
        }
    }
}

/// Under this width the pane goes under the list, not beside it.
pub(crate) const BESIDE_WIDTH: Pixels = px(600.);

/// The share of the panel's width the pane takes beside the list.
const BESIDE_FRACTION: f32 = 0.45;

/// The share of the panel's height the pane takes at most under the list.
const UNDER_FRACTION: f32 = 0.5;

/// The problem shown: its file, the problem, and the editor showing its
/// file, if one does, with its buffer's version.
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    path: PathBuf,
    problem: Problem,
    editor: Option<(EntityId, Version)>,
}

/// The suggested fix: being looked up, found, or not.
#[derive(Default)]
enum Lookup {
    #[default]
    None,
    Asking(#[allow(dead_code)] Task<()>),
    Found(Box<Fix>),
}

/// The language server's preferred quick fix for the problem shown.
struct Fix {
    title: String,
    /// The rows it changes, as they'd read once it's applied.
    lines: Vec<String>,
    /// Resolved; `None` for the UI audit's, which no server offered.
    action: Option<CodeAction>,
    editor: WeakEntity<EditorView>,
    /// The buffer's version it was asked for at.
    version: Version,
}

#[kiln::command(context = "problems_panel")]
impl ProblemsPanel {
    /// Apply Fix
    ///
    /// Applies the language server's preferred quick fix for the selected
    /// problem, as the detail pane shows it.
    #[command(id = "problems.apply_fix")]
    fn apply_fix_command(&mut self, cx: &mut Cx<Self>) {
        self.apply_fix(cx.window, cx.cx);
    }
}

impl ProblemsPanel {
    /// The problem the pane is for, and the editor showing its file, if any.
    fn detail_target(&self, cx: &App) -> Option<(Shown, Option<Entity<EditorView>>)> {
        let workbench = self.workbench.upgrade()?;
        self.detail_target_in(workbench.read(cx).open_editors(cx), cx)
    }

    /// [`Self::detail_target`], of `editors`, the workbench's open ones.
    fn detail_target_in(
        &self,
        editors: Vec<Entity<EditorView>>,
        cx: &App,
    ) -> Option<(Shown, Option<Entity<EditorView>>)> {
        let (path, problem) = self.selected_problem()?;
        let editor =
            (editors.into_iter()).find(|editor| editor.read(cx).model().path() == Some(path));
        let at = (editor.as_ref()).map(|editor| {
            (
                editor.entity_id(),
                editor.read(cx).model().buffer().version(),
            )
        });
        let shown = Shown {
            path: path.to_owned(),
            problem: problem.clone(),
            editor: at,
        };
        Some((shown, editor))
    }

    /// Looks the shown problem's fix up again, or reads its file, if what
    /// the pane is for has changed since it last did. The problem's
    /// editor, if any.
    fn sync_detail(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(Shown, Option<Entity<EditorView>>)> {
        let target = self.detail_target(cx);
        let shown = target.as_ref().map(|(shown, _)| shown.clone());
        if shown != self.detail.shown {
            self.detail.fix = Lookup::None;
            self.detail.shown = shown;
            match &target {
                Some((shown, Some(editor))) => self.ask_fix(shown.clone(), editor.clone(), cx),
                Some((shown, None)) => self.read_for_snippet(&shown.path, cx),
                None => {}
            }
        }
        target
    }

    /// Reads `path`'s text off the main thread for the snippet, unless it's
    /// read or being read.
    fn read_for_snippet(&mut self, path: &Path, cx: &mut Context<Self>) {
        if (self.detail.read.as_ref()).is_some_and(|(read, ..)| read == path) {
            return;
        }
        let read = cx.background_spawn({
            let path = path.to_owned();
            async move { read_text(path) }
        });
        let at = path.to_owned();
        let task = cx.spawn(async move |panel, cx| {
            let text = read.await;
            panel
                .update(cx, |panel, cx| {
                    if let Some((path, read, _)) = &mut panel.detail.read
                        && *path == at
                    {
                        *read = Some(text);
                        cx.notify();
                    }
                })
                .ok();
        });
        self.detail.read = Some((path.to_owned(), None, task));
    }

    /// Asks `editor`'s language server for its quick fixes for `shown`'s
    /// problem, one of the buffer's diagnostics, and keeps the preferred one
    /// if its edit changes the buffer.
    fn ask_fix(&mut self, shown: Shown, editor: Entity<EditorView>, cx: &mut Context<Self>) {
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        let model = editor.read(cx).model();
        let (buffer, version) = (model.buffer().id(), model.buffer().version());
        let problem = &shown.problem;
        // A task's problem isn't a diagnostic the server could fix.
        let diagnostics: Vec<_> = (model.diagnostics().into_iter())
            .filter(|entry| {
                (&entry.range, entry.severity, &entry.message)
                    == (&problem.range, problem.severity, &problem.message)
            })
            .take(1)
            .map(|entry| crate::lsp_code_actions::sent_diagnostic(&entry))
            .collect();
        if diagnostics.is_empty() {
            return;
        }
        let Some(actioner) = workbench.read(cx).lsp_code_actioner(buffer) else {
            return;
        };
        let query = CodeActionQuery {
            only: Some(vec![QUICKFIX.to_owned()]),
            automatic: true,
        };
        let reply = actioner.ask(buffer, version, problem.range.clone(), diagnostics, query);
        let weak = editor.downgrade();
        let task = cx.spawn(async move |panel, cx| {
            let preferred = reply.await.ok().and_then(preferred_fix);
            let resolved = match preferred {
                Some(action) => actioner.resolve(&action).await.ok(),
                None => None,
            };
            let edits = (resolved.as_ref()).and_then(|action| actioner.edits(action).ok());
            panel
                .update(cx, |panel, cx| {
                    if panel.detail.shown.as_ref() != Some(&shown) {
                        return;
                    }
                    let lines = (weak.upgrade().zip(edits)).and_then(|(editor, edits)| {
                        let buffer_now = editor.read(cx).model().buffer();
                        if buffer_now.version() != version {
                            return None;
                        }
                        fixed_lines(
                            buffer_now.text(),
                            buffer,
                            version,
                            shown.problem.row,
                            &edits,
                        )
                    });
                    panel.detail.fix = match (resolved, lines) {
                        (Some(action), Some(lines)) => Lookup::Found(Box::new(Fix {
                            title: action.title.clone(),
                            lines,
                            action: Some(action),
                            editor: weak,
                            version,
                        })),
                        _ => Lookup::None,
                    };
                    cx.notify();
                })
                .ok();
        });
        self.detail.fix = Lookup::Asking(task);
    }

    /// The fix shown, if it's still for the problem selected and its
    /// buffer as it is.
    fn current_fix(&self, cx: &App) -> Option<&Fix> {
        let Lookup::Found(fix) = &self.detail.fix else {
            return None;
        };
        let (shown, _) = self.detail_target(cx)?;
        let editor = fix.editor.upgrade()?;
        let current = self.detail.shown.as_ref() == Some(&shown)
            && editor.read(cx).model().buffer().version() == fix.version;
        current.then_some(fix)
    }

    /// `problems.apply_fix`: applies the fix shown, its edit and then its
    /// command, as the code-action picker would. Nothing without one.
    pub(crate) fn apply_fix(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fix) = self.current_fix(cx) else {
            return;
        };
        let (Some(editor), Some(workbench)) = (fix.editor.upgrade(), self.workbench.upgrade())
        else {
            return;
        };
        let (Some(action), version) = (fix.action.clone(), fix.version) else {
            return;
        };
        // Outside the panel's update: applying the edit lists the problems
        // again.
        window.defer(cx, move |window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.apply_code_action(editor, version, action, window, cx)
            })
        });
    }

    /// The pane, for the selected problem; none without one.
    pub(super) fn render_detail(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (shown, editor) = self.sync_detail(cx)?;
        let ui = fe_ui::ui_theme(cx);
        let c = cx.theme().colors;
        let code = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Code, cx);
        let problem = &shown.problem;
        let severity = self.color(problem.severity);

        // `code · source`, the code a link to its page.
        let small = |element: gpui::Div| {
            element
                .flex_none()
                .font_family(code.clone())
                .text_size(CODE_SMALL.size)
                .line_height(CODE_SMALL.line_height)
        };
        let code_part = problem.code.clone().map(|name| {
            let element = small(div()).text_color(c.text.muted).child(name);
            match problem.code_href.clone() {
                Some(href) => element
                    .id(CODE_LINK)
                    .cursor_pointer()
                    .text_color(c.text.accent)
                    .underline()
                    .on_click(move |_, _, cx| cx.open_url(&href))
                    .into_any_element(),
                None => element.into_any_element(),
            }
        });
        let source_part = (problem.source.clone())
            .map(|source| small(div()).text_color(c.text.faint).child(source));
        let both = code_part.is_some() && source_part.is_some();
        let heading = (code_part.is_some() || source_part.is_some()).then(|| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .children(code_part)
                .when(both, |row| {
                    row.child(small(div()).text_color(c.text.faint).child(SEPARATOR))
                })
                .children(source_part)
        });

        // The snippet: the problem's row and one either side.
        let text = match &editor {
            Some(editor) => Some(editor.read(cx).model().buffer().text().clone()),
            None => match &self.detail.read {
                Some((path, Some(text), _)) if *path == shown.path => text.clone(),
                _ => None,
            },
        };
        let mut snippet = text.as_ref().map(|text| snippet_rows(text, problem));
        let mut fixed = (self.current_fix(cx)).map(|fix| (fix.lines.clone(), fix.title.clone()));
        // The indent every row shown shares, left out: the pane is narrow.
        dedent(
            snippet.as_deref_mut().unwrap_or_default(),
            fixed
                .as_mut()
                .map_or(&mut [][..], |(lines, _)| &mut lines[..]),
        );
        #[cfg(test)]
        let drawn_snippet: Vec<_> = (snippet.iter().flatten())
            .map(|row| {
                let (before, marked, after) = row.parts();
                (row.row, before, marked, after)
            })
            .collect();
        #[cfg(test)]
        let drawn_fix = fixed.clone();
        let row_number = |row: u32| {
            div()
                .flex_none()
                .w(px(32.))
                .pr_2()
                .text_right()
                .text_color(c.text.faint)
                .child((row + 1).to_string())
        };
        let code_row = || {
            div()
                .flex()
                .items_center()
                .whitespace_nowrap()
                .overflow_hidden()
                .font_family(code.clone())
                .text_size(CODE_SMALL.size)
                .line_height(CODE_SMALL.line_height)
        };
        let snippet = snippet.map(|rows| {
            let rows = rows.into_iter().map(|row| {
                let (before, marked, after) = row.parts();
                code_row()
                    .child(row_number(row.row))
                    .child(div().flex_none().text_color(c.text.primary).child(before))
                    .when(!marked.is_empty(), |line| {
                        line.child(
                            div()
                                .flex_none()
                                .text_color(c.text.primary)
                                .text_decoration_1()
                                .text_decoration_wavy()
                                .text_decoration_color(severity)
                                .child(marked),
                        )
                    })
                    .child(div().flex_none().text_color(c.text.primary).child(after))
            });
            div()
                .flex_none()
                .flex()
                .flex_col()
                .py_1()
                .rounded(fe_ui::radius::CONTROL)
                .border_1()
                .border_color(c.border.default)
                .bg(c.surface.background)
                .overflow_hidden()
                .children(rows)
        });

        // The fix: its rows on celadon-soft, each with a + glyph, then its
        // title.
        let fix = fixed.map(|(lines, title)| {
            let lines = lines.into_iter().enumerate().map(|(ix, line)| {
                code_row()
                    .when(ix == 0, |row| row.debug_selector(|| FIX_SELECTOR.into()))
                    .pr_2()
                    .bg(c.status.success_soft)
                    .child(
                        div()
                            .flex_none()
                            .w(px(32.))
                            .pr_2()
                            .flex()
                            .justify_end()
                            .child(
                                svg()
                                    .path(fe_ui::Icon::Plus.path())
                                    .size(px(12.))
                                    .text_color(c.status.success),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(c.text.primary)
                            .child(line),
                    )
            });
            let lines = div()
                .flex_none()
                .flex()
                .flex_col()
                .rounded(fe_ui::radius::CONTROL)
                .overflow_hidden()
                .children(lines);
            (lines, title)
        });
        let (fix, title) = fix.unzip();
        let actions = title.map(|title| {
            let key = apply_key(cx);
            let button = Button::new(APPLY_SELECTOR, text!("workbench-problems-detail-apply-fix"))
                .primary()
                .small()
                .on_click(cx.listener(|panel, _, window, cx| panel.apply_fix(window, cx)));
            let button = match key.clone() {
                Some(key) => button.shortcut(key),
                None => button,
            };
            // The button, its key, then what the fix does.
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .whitespace_nowrap()
                .child(
                    div()
                        .flex_none()
                        .debug_selector(|| APPLY_SELECTOR.into())
                        .child(button),
                )
                .children(key.map(|key| {
                    div()
                        .flex_none()
                        .text_size(UI_SMALL.size)
                        .text_color(c.text.faint)
                        .child(key)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_size(UI_SMALL.size)
                        .line_height(UI_SMALL.line_height)
                        .text_color(c.text.muted)
                        .child(title),
                )
        });

        #[cfg(test)]
        {
            self.drawn_detail = Some(DrawnDetail {
                code: problem.code.clone(),
                source: problem.source.clone(),
                linked: problem.code.is_some() && problem.code_href.is_some(),
                message: problem.message.clone(),
                snippet: drawn_snippet,
                fix: drawn_fix.map(|(lines, title)| DrawnFix {
                    lines,
                    title,
                    ground: c.status.success_soft,
                    glyph: fe_ui::Icon::Plus,
                }),
                apply: actions.is_some(),
            });
        }

        let pane = div()
            .id(DETAIL_SELECTOR)
            .debug_selector(|| DETAIL_SELECTOR.into())
            .flex_none()
            .min_w_0()
            // Its parts keep their height; it scrolls in a short panel.
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1p5()
            .px_3()
            .py_1p5()
            .border_color(ui.border)
            // Beside the list in a wide panel (the bottom dock); under it,
            // keeping it half the panel however long the message, in a
            // narrow one.
            .map(|pane| match self.detail.wide.get() {
                true => pane.h_full().w(relative(BESIDE_FRACTION)).border_l_1(),
                false => pane.w_full().max_h(relative(UNDER_FRACTION)).border_t_1(),
            })
            .children(heading)
            .child(
                div()
                    .flex_none()
                    .text_color(c.text.primary)
                    .whitespace_normal()
                    .child(problem.message.clone()),
            )
            .children(snippet)
            .children(fix)
            .children(actions);
        Some(pane.into_any_element())
    }
}

impl crate::Workbench {
    /// Shows a fix titled `title` changing the selected problem's rows to
    /// `lines` in the Problems panel's detail pane, as a language server's
    /// preferred quick fix would show, for the UI audit's captures, which
    /// run no server. Applying it does nothing. Whether a problem in an
    /// open file is selected, for it to show for; it shows until the
    /// selection or the buffer changes.
    #[doc(hidden)]
    pub fn show_problem_fix_for_audit(
        &mut self,
        title: &str,
        lines: Vec<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let editors = self.open_editors(cx);
        let panel = self.dock().read(cx).problems().clone();
        panel.update(cx, |panel, cx| {
            let Some((shown, Some(editor))) = panel.detail_target_in(editors, cx) else {
                return false;
            };
            let Some((_, version)) = shown.editor else {
                return false;
            };
            panel.detail.shown = Some(shown);
            panel.detail.fix = Lookup::Found(Box::new(Fix {
                title: title.to_owned(),
                lines,
                action: None,
                editor: editor.downgrade(),
                version,
            }));
            cx.notify();
            true
        })
    }
}

impl ProblemsPanel {
    /// `list` with `detail` beside it, if the panel was last drawn
    /// [`BESIDE_WIDTH`] or wider, else under it. A change of side redraws it.
    pub(super) fn with_detail(
        &self,
        list: AnyElement,
        detail: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(detail) = detail else {
            return list;
        };
        let (cell, panel) = (self.detail.wide.clone(), cx.weak_entity());
        let measure = canvas(
            move |bounds, _, cx| {
                let now = bounds.size.width >= BESIDE_WIDTH;
                if cell.replace(now) != now {
                    panel.update(cx, |_, cx| cx.notify()).ok();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .when(!self.detail.wide.get(), |both| both.flex_col())
            .child(measure)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(list),
            )
            .child(detail)
            .into_any_element()
    }
}

/// One of the snippet's rows: its number, its text, and the bytes of it the
/// problem's range covers.
struct SnippetRow {
    row: u32,
    text: String,
    marked: Range<usize>,
}

impl SnippetRow {
    /// Its text before the problem's range, in it, and after it.
    fn parts(&self) -> (String, String, String) {
        let Range { start, end } = self.marked.clone();
        (
            self.text[..start].to_owned(),
            self.text[start..end].to_owned(),
            self.text[end..].to_owned(),
        )
    }
}

/// `problem`'s row and one either side, of `text`, each with the part of
/// the problem's range on it. A row past the text's end isn't shown.
fn snippet_rows(text: &Text, problem: &Problem) -> Vec<SnippetRow> {
    let last = text.row_count().saturating_sub(1);
    if problem.row > last {
        return Vec::new();
    }
    let range = problem.range.start.min(text.len())..problem.range.end.min(text.len());
    (problem.row.saturating_sub(1)..=(problem.row + 1).min(last))
        .map(|row| {
            let line = text.row_to_string(row);
            let start = text.row_start(row);
            let from = range.start.clamp(start, start + line.len()) - start;
            let to = range.end.clamp(start, start + line.len()) - start;
            // An empty mark goes at the row's end, so its text is all before it.
            let marked = match from < to && line.is_char_boundary(from) && line.is_char_boundary(to)
            {
                true => from..to,
                false => line.len()..line.len(),
            };
            SnippetRow {
                row,
                text: line,
                marked,
            }
        })
        .collect()
}

/// Takes the leading spaces and tabs every non-blank row of `rows` and
/// `lines` shares off them, moving each row's mark with its text.
fn dedent(rows: &mut [SnippetRow], lines: &mut [String]) {
    let indent = |text: &str| {
        let rest = text.trim_start_matches([' ', '\t']);
        (!rest.is_empty()).then_some(text.len() - rest.len())
    };
    let shared = (rows.iter().map(|row| indent(&row.text)))
        .chain(lines.iter().map(|line| indent(line)))
        .flatten()
        .min()
        .unwrap_or(0);
    if shared == 0 {
        return;
    }
    // Every row has `shared` leading blanks but a blank one, which may be
    // shorter.
    let cut = |text: &str| shared.min(text.len() - text.trim_start_matches([' ', '\t']).len());
    for row in rows {
        let cut = cut(&row.text);
        row.text.drain(..cut);
        let Range { start, end } = row.marked.clone();
        row.marked = start.saturating_sub(cut)..end.saturating_sub(cut);
    }
    for line in lines {
        let cut = cut(line);
        line.drain(..cut);
    }
}

/// Of `actions`, the enabled quick fixes, preferred first, then those that
/// fix the diagnostic, in the order given: the first.
fn preferred_fix(actions: Vec<CodeAction>) -> Option<CodeAction> {
    let mut fixes: Vec<_> = (actions.into_iter())
        .filter(|action| action.is_quick_fix() && action.disabled.is_none())
        .collect();
    fixes.sort_by_key(|action| (!action.is_preferred, action.diagnostics_fixed == 0));
    fixes.into_iter().next()
}

/// The rows of `text` (`buffer`'s at `version`) that `edits` change, as
/// they'd read once applied: those of the edits on row `row`, else of the
/// first edit, with any edit sharing them. `None` if `edits` don't change
/// the buffer.
fn fixed_lines(
    text: &Text,
    buffer: BufferId,
    version: Version,
    row: u32,
    edits: &[FileEdits],
) -> Option<Vec<String>> {
    let mut ours: Vec<&(Range<usize>, String)> = (edits.iter())
        .filter_map(|edit| match &edit.target {
            EditTarget::Open {
                buffer: on,
                version: at,
                edits,
            } if *on == buffer && *at == version => Some(edits),
            _ => None,
        })
        .flatten()
        .collect();
    ours.sort_by_key(|(range, _)| (range.start, range.end));
    let rows = |range: &Range<usize>| {
        let end = range.end.min(text.len());
        (
            text.offset_to_point(range.start.min(end)).row,
            text.offset_to_point(end).row,
        )
    };
    let touching = (ours.iter())
        .map(|(range, _)| rows(range))
        .filter(|(a, b)| *a <= row && row <= *b);
    let (mut first, mut last) = touching
        .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
        .or_else(|| ours.first().map(|(range, _)| rows(range)))?;
    loop {
        let (was_first, was_last) = (first, last);
        for (range, _) in &ours {
            let (a, b) = rows(range);
            if a <= last && first <= b {
                (first, last) = (first.min(a), last.max(b));
            }
        }
        if (first, last) == (was_first, was_last) {
            break;
        }
    }
    let start = text.row_start(first);
    let end = text.row_start(last) + text.row_len(last) as usize;
    let mut fixed = String::new();
    let mut at = start;
    for (range, new_text) in &ours {
        if range.start < start || range.end > end || range.start < at {
            continue;
        }
        fixed.push_str(&text.slice_to_string(at..range.start));
        fixed.push_str(new_text);
        at = range.end;
    }
    fixed.push_str(&text.slice_to_string(at..end));
    Some(fixed.split('\n').map(str::to_owned).collect())
}

/// The key that applies the fix, as this platform prints it: the first
/// single key the keymap binds to [`APPLY_FIX`] in the panel, read now so
/// it follows reloads.
fn apply_key(cx: &App) -> Option<String> {
    let command = CommandId::parse(APPLY_FIX).ok()?;
    let stack: ContextStack = [crate::KEY_CONTEXT, KEY_CONTEXT]
        .into_iter()
        .map(ContextFrame::new)
        .collect();
    let keymap = fe_gpui::current_keymap(cx);
    fe_keymap::shortcuts(&keymap, &command, &Value::Null, &stack)
        .iter()
        .find(|keys| keys.keystrokes().len() == 1)
        .map(fe_keymap::shortcut_label)
}

/// What the pane drew last, for the tests.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawnDetail {
    pub(crate) code: Option<String>,
    pub(crate) source: Option<String>,
    /// Whether the code is a link to its page.
    pub(crate) linked: bool,
    pub(crate) message: String,
    /// Each row's number (zero-based), and its text before the underlined
    /// range, in it, and after it.
    pub(crate) snippet: Vec<(u32, String, String, String)>,
    pub(crate) fix: Option<DrawnFix>,
    /// Whether the Apply fix button shows.
    pub(crate) apply: bool,
}

/// The fix the pane drew.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawnFix {
    pub(crate) lines: Vec<String>,
    pub(crate) title: String,
    /// Its lines' background.
    pub(crate) ground: gpui::Hsla,
    pub(crate) glyph: fe_ui::Icon,
}

#[cfg(test)]
impl ProblemsPanel {
    /// What the detail pane drew last, while one shows.
    pub(crate) fn drawn_detail(&self) -> Option<DrawnDetail> {
        self.selected_problem()?;
        self.drawn_detail.clone()
    }
}
