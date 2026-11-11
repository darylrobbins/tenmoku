//! Search TODO Comments (`todo.list`, LazyVim's `<leader>st` and
//! `<leader>xt`, todo-comments.nvim's `:TodoTelescope`): a [`Picker`] over
//! the project's `TODO:`-style comments, each row `KEYWORD  text  path:row`,
//! the keyword as written in its kind's colour ([`Kind::tint`]), the text
//! what follows the tag.
//!
//! The rows come from a search of the root ([`fe_project::search_filtered`],
//! so `.gitignore` and the rest of the walk's rules apply) for [`PATTERN`],
//! run off the main thread once the picker shows; until it lands the status
//! line says it's searching. A search finds keywords in strings as well as
//! comments, as todo-comments' own ripgrep search does. A file open in an
//! editor whose parse is up to date has each of its hits checked against
//! that parse ([`crate::todo_comments::comment_ranges`] and
//! [`crate::todo_comments::scan`]
//! over the chunk of rows of the buffer the hit is in): a hit the parse doesn't put in a
//! comment is left out. A hit whose row of the buffer doesn't have the
//! keyword at the same byte column (the buffer edited since, or decoded from
//! another encoding), or whose row's highlighting didn't finish, can't be
//! checked, and stays, as do the hits of every other file.
//!
//! Every match is a row, sorted by path, row and column. The search stops at
//! [`MAX_ROWS`] matches, and the status line says so, with an empty list
//! too. A line the search's preview cuts (one over 512 bytes) is read again,
//! once, for the tag, its text and its column in chars: a second keyword far
//! along, or a long `(user)`, still lists. A line that can't be read again
//! (gone, changed, over a MiB, or of a UTF-16 file, whose offsets are of its
//! text transcoded) lists what its preview holds of it. A row's text is
//! listed and matched up to [`MAX_TEXT_CHARS`] chars.
//!
//! Rows match the query by subsequence over the text and the path, ignoring
//! case. A query starting with a keyword in capitals and a space (`FIX `,
//! `TODO `) lists only that keyword's kind (`FIX ` lists `FIXME:` and `BUG:`
//! too), the rest of the query matching as before.
//!
//! Moving the selection onto a row in the active editor's file previews it
//! there as the symbol picker previews a symbol ([`RowPreview`]); one in
//! another file previews nothing, as previewing it would open that file.
//! Dismissing puts the scroll and the folds back. Enter opens the file at
//! the keyword ([`Workbench::open_document_at`]), a jump that back returns
//! from.

use std::{
    collections::HashSet,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_gpui::{EditorTheme, EditorView};
use fe_project::{FileMatches, LineMatch, PathFilter, SearchOptions, SearchQuery};
use fe_ui::{HighlightedLabel, char_ranges};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Hsla, InteractiveElement as _,
    IntoElement as _, ParentElement as _, SharedString, Styled as _, Subscription, Task,
    WeakEntity, Window, div,
};

use super::{
    buffer_lines_picker::RowPreview,
    todo_commands::{chunk_of, keyword_starts_in},
};
use crate::{
    Workbench,
    picker::{Picker, PickerDelegate},
    todo_comments::Kind,
    toolbar_picker::subsequence,
    workbench::Palette,
};

/// The search's pattern: a keyword in capitals at a word boundary, then a
/// colon, with an optional `(user)` between.
pub(crate) const PATTERN: &str =
    r"\b(TODO|FIX(ME|IT)?|BUG|HACK|XXX|WARN(ING)?|PERF|OPTIMIZE|NOTE|INFO|TEST(ING)?)(\([^)]*\))?:";

/// The matches listed at most.
pub(crate) const MAX_ROWS: usize = 5_000;

/// The kind of keyword `word`, one of [`PATTERN`]'s, in capitals.
fn kind_of(word: &str) -> Option<Kind> {
    Some(match word {
        "TODO" => Kind::Todo,
        "FIX" | "FIXME" | "FIXIT" | "BUG" => Kind::Fix,
        "HACK" | "XXX" => Kind::Hack,
        "WARN" | "WARNING" => Kind::Warn,
        "PERF" | "OPTIMIZE" => Kind::Perf,
        "NOTE" | "INFO" => Kind::Note,
        "TEST" | "TESTING" => Kind::Test,
        _ => return None,
    })
}

/// A keyword found.
#[derive(Clone, Debug)]
struct Row {
    path: PathBuf,
    /// Its file as the row shows it: from the root.
    file: String,
    /// Zero-based.
    row: u32,
    /// The keyword's zero-based column, in chars.
    column: u32,
    /// Its byte offset in the line, to check it against a parse.
    byte_column: usize,
    /// The keyword alone, as written.
    keyword: String,
    kind: Kind,
    /// What follows the tag, trimmed.
    text: String,
    /// What the query matches: the text, then the file.
    matched: String,
}

/// What a search found: the rows, and whether the cap cut it short. None if
/// it was cancelled.
type Found = Option<(Vec<Row>, bool)>;

/// Searches `root` for [`PATTERN`]: a row per match.
fn search(root: &Path, cancel: Arc<AtomicBool>) -> Found {
    let query = SearchQuery {
        pattern: PATTERN.to_owned(),
        regex: true,
        case_sensitive: true,
        whole_word: false,
    };
    let options = SearchOptions {
        max_matches: MAX_ROWS,
        ..SearchOptions::default()
    };
    let mut files: Vec<FileMatches> = Vec::new();
    let summary = fe_project::search_filtered(
        root,
        &query,
        &options,
        &PathFilter::default(),
        cancel.clone(),
        |batch| files.extend(batch),
    );
    // The pattern is a constant one: a search fails only if it's invalid.
    let summary = summary.ok()?;
    if summary.cancelled || cancel.load(Ordering::Relaxed) {
        return None;
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    let mut rows = Vec::new();
    for file in &files {
        let mut reader = LineReader::new(&file.path);
        for line in &file.lines {
            // Read at most once, and only for a line the preview cuts.
            let mut full: Option<Option<Vec<u8>>> = None;
            for range in &line.ranges {
                if let Some(row) = row_in_preview(file, line, range.clone()) {
                    rows.push(row);
                    continue;
                }
                let full = full.get_or_insert_with(|| reader.line_at(line.line_offset));
                rows.extend((full.as_deref()).and_then(|full| row_in(file, line, range, full)));
            }
        }
    }
    Some((rows, summary.truncated))
}

/// The row of the match at `range` of `line` from its preview, if the
/// preview starts at the line's start and holds the whole tag.
fn row_in_preview(file: &FileMatches, line: &LineMatch, range: Range<usize>) -> Option<Row> {
    if line.preview_start != 0 {
        return None;
    }
    let tag = line.preview.get(range.clone())?;
    let before = line.preview.get(..range.start)?;
    let after = line.preview.get(range.end..)?;
    row_of(file, line, range.start, before, tag, after)
}

/// The row of the match at `range` of `line`, read again in `full`, if it
/// still has the tag there.
fn row_in(file: &FileMatches, line: &LineMatch, range: &Range<usize>, full: &[u8]) -> Option<Row> {
    let tag = std::str::from_utf8(full.get(range.clone())?).ok()?;
    let before = String::from_utf8_lossy(full.get(..range.start)?);
    let after = String::from_utf8_lossy(full.get(range.end..)?);
    row_of(file, line, range.start, &before, tag, &after)
}

/// A row for `tag`, `start` bytes into `line` after `before`, with `after`
/// following it, if it's a tag. A UTF-8 byte order mark the search counts
/// as part of the first line isn't counted in its columns, as the editor
/// doesn't show one.
fn row_of(
    file: &FileMatches,
    line: &LineMatch,
    start: usize,
    before: &str,
    tag: &str,
    after: &str,
) -> Option<Row> {
    if !tag.ends_with(':') {
        return None;
    }
    let bom = line.line_offset == 0 && before.starts_with('\u{feff}');
    let column = before.chars().count() - usize::from(bom);
    let start = start - if bom { '\u{feff}'.len_utf8() } else { 0 };
    let keyword = tag.split(['(', ':']).next()?.to_owned();
    let kind = kind_of(&keyword)?;
    let text: String = after.trim().chars().take(MAX_TEXT_CHARS).collect();
    let matched = format!("{text} {}", file.rel);
    Some(Row {
        path: file.path.clone(),
        file: file.rel.clone(),
        row: u32::try_from(line.line_number.saturating_sub(1)).unwrap_or(u32::MAX),
        column: u32::try_from(column).unwrap_or(u32::MAX),
        byte_column: start,
        keyword,
        kind,
        text,
        matched,
    })
}

/// The chars of a row's text listed and matched at most.
const MAX_TEXT_CHARS: usize = 500;

/// The bytes of a line read again at most: a longer one isn't.
const MAX_LINE_BYTES: usize = 1 << 20;

/// Reads lines of a file the search cut, opening it once.
struct LineReader<'a> {
    path: &'a Path,
    /// The file, once opened: `None` inside if it couldn't be, or is UTF-16,
    /// which the search reads transcoded, so its offsets aren't the file's.
    file: Option<Option<std::fs::File>>,
}

impl<'a> LineReader<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path, file: None }
    }

    /// The line at byte `offset`, without its line break: None if it can't
    /// be read, or is over [`MAX_LINE_BYTES`].
    fn line_at(&mut self, offset: u64) -> Option<Vec<u8>> {
        use std::io::{Read as _, Seek as _, SeekFrom};
        let path = self.path;
        let file = self.file.get_or_insert_with(|| {
            let file = std::fs::File::open(path).ok()?;
            let mut bom = [0; 2];
            let utf16 =
                (&file).read_exact(&mut bom).is_ok() && matches!(bom, [0xFF, 0xFE] | [0xFE, 0xFF]);
            (!utf16).then_some(file)
        });
        let mut file = file.as_ref()?;
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut line = Vec::new();
        let mut chunk = [0; 8192];
        loop {
            let read = file.read(&mut chunk).ok()?;
            let chunk = &chunk[..read];
            if let Some(end) = chunk.iter().position(|&byte| byte == b'\n') {
                line.extend_from_slice(&chunk[..end]);
                break;
            }
            line.extend_from_slice(chunk);
            if read == 0 {
                break;
            }
            if line.len() > MAX_LINE_BYTES {
                return None;
            }
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        Some(line)
    }
}

/// Leaves out of `rows`, sorted by path, the hits that an open editor's
/// parse of their file doesn't put in a comment: see the module docs.
fn confirm_with_parses(rows: Vec<Row>, workbench: &Workbench, cx: &App) -> Vec<Row> {
    let editors = workbench.open_editors(cx);
    let mut kept = Vec::with_capacity(rows.len());
    let mut rows = rows.into_iter().peekable();
    while let Some(first) = rows.next() {
        let mut file = vec![first];
        while let Some(next) = rows.next_if(|next| next.path == file[0].path) {
            file.push(next);
        }
        let path = file[0].path.as_path();
        let view = (editors.iter())
            .map(|editor| editor.read(cx))
            .find(|view| view.model().path() == Some(path) && parse_is_current(view));
        if let Some(view) = view {
            keep_in_comments(view, &mut file);
        }
        kept.extend(file);
    }
    kept
}

/// Whether `view`'s parse is of its buffer as it is, so that its comments
/// can be read.
fn parse_is_current(view: &EditorView) -> bool {
    let Some(syntax) = view.syntax() else {
        return false;
    };
    let buffer = view.model().buffer();
    !(view.is_loading_language()
        || syntax.language().name() == "markdown"
        || syntax.failed()
        || syntax.tree().is_none()
        || syntax.needs_parse()
        || syntax.version() != buffer.version()
        || buffer.text().len() > crate::spell_ranges::MAX_SPELLABLE_BYTES)
}

/// Leaves out of `rows`, one file's in row order, those `view`'s parse,
/// which is current, doesn't put in a comment, reading its comments a chunk
/// of rows at a time ([`keyword_starts_in`]). A row whose buffer row doesn't
/// have the keyword at the same byte column stays, as do those of a chunk
/// whose highlighting didn't finish.
fn keep_in_comments(view: &EditorView, rows: &mut Vec<Row>) {
    let buffer = view.model().buffer();
    let text = buffer.text();
    let count = text.row_count();
    let on_chars = |at: usize| text.clip_offset(at, fe_text::Bias::Left) == at;
    // The chunk last read, and the keywords in its comments.
    let mut read: Option<(Range<u32>, Option<HashSet<usize>>)> = None;
    rows.retain(|row| {
        if row.row >= count {
            return true;
        }
        let row_start = text.row_start(row.row);
        let row_end = row_start + text.row_len(row.row) as usize;
        let start = row_start + row.byte_column;
        let end = start + row.keyword.len();
        let aligned = end <= row_end
            && on_chars(start)
            && on_chars(end)
            && text.slice_to_string(start..end) == row.keyword;
        if !aligned {
            return true;
        }
        if read
            .as_ref()
            .is_none_or(|(chunk, _)| !chunk.contains(&row.row))
        {
            let chunk = chunk_of(row.row, count);
            let starts = keyword_starts_in(view.syntax(), text, chunk.clone());
            read = Some((chunk, starts.map(|starts| starts.into_iter().collect())));
        }
        match &read {
            Some((_, Some(starts))) => starts.contains(&start),
            _ => true,
        }
    });
}

/// A running search. Dropping it cancels it.
struct Search {
    cancel: Arc<AtomicBool>,
    _task: Task<()>,
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Each kind's badge colour, from the editor's theme.
#[derive(Clone, Copy)]
struct Colors {
    error: Hsla,
    warning: Hsla,
    information: Hsla,
    hint: Hsla,
    accent: Hsla,
}

impl Colors {
    fn new(theme: &EditorTheme) -> Self {
        Self {
            error: theme.error,
            warning: theme.warning,
            information: theme.information,
            hint: theme.hint,
            accent: theme.cursor,
        }
    }

    /// As the editor draws [`Kind::tint`].
    fn of(&self, kind: Kind) -> Hsla {
        use fe_editor::decorations::Tint;
        match kind.tint() {
            Tint::Error => self.error,
            Tint::Warning => self.warning,
            Tint::Information => self.information,
            Tint::Hint => self.hint,
            _ => self.accent,
        }
    }
}

pub(crate) struct TodoPicker {
    workbench: WeakEntity<Workbench>,
    /// No folder is open: nothing to search.
    no_root: bool,
    /// The search, while it runs.
    search: Option<Search>,
    preview: RowPreview,
    /// The active editor's file as it opened, the one previews show in.
    previewed_path: Option<PathBuf>,
    rows: Vec<Row>,
    /// The search stopped at [`MAX_ROWS`].
    capped: bool,
    colors: Colors,
    query: String,
    /// The rows matching the query, each with the chars of `matched` it
    /// matched.
    matches: Vec<(usize, Vec<usize>)>,
    /// Focused before it opened, and again once it's dismissed.
    previous_focus: Option<FocusHandle>,
    _follows: Option<Subscription>,
}

/// `query`'s leading keyword's kind, and the rest of the query, if it
/// starts with a keyword in capitals and a space.
fn leading_kind(query: &str) -> (Option<Kind>, &str) {
    if let Some((word, rest)) = query.split_once(' ')
        && let Some(kind) = kind_of(word)
    {
        return (Some(kind), rest.trim_start());
    }
    (None, query)
}

impl TodoPicker {
    fn new(
        workbench: WeakEntity<Workbench>,
        editor: Option<&Entity<EditorView>>,
        colors: Colors,
        previous_focus: Option<FocusHandle>,
        cx: &App,
    ) -> Self {
        let previewed_path =
            editor.and_then(|editor| editor.read(cx).model().path().map(Path::to_owned));
        Self {
            workbench,
            no_root: false,
            search: None,
            preview: RowPreview::new(editor.map(Entity::downgrade)),
            previewed_path,
            rows: Vec::new(),
            capped: false,
            colors,
            query: String::new(),
            matches: Vec::new(),
            previous_focus,
            _follows: None,
        }
    }

    /// Starts the search of `root`, or says there's no folder, and previews
    /// the selected row from now on as the selection moves.
    fn start(&mut self, root: Option<PathBuf>, cx: &mut Context<Picker<Self>>) {
        // `Picker::select` is the only way the selection moves, and it
        // notifies; the delegate has no hook for it.
        self._follows = Some(cx.observe_self(|picker, cx| {
            let selected = picker.selected();
            picker.delegate.preview(selected, cx);
        }));
        let Some(root) = root else {
            self.no_root = true;
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let searching = cx.background_spawn(async move { search(&root, flag) });
        let task = cx.spawn(async move |picker, cx| {
            let found = searching.await;
            let landed = picker.update(cx, |picker, cx| picker.delegate.landed(found, cx));
            landed.ok();
        });
        self.search = Some(Search {
            cancel,
            _task: task,
        });
    }

    /// The search is done: lists what it found.
    fn landed(&mut self, found: Found, cx: &mut Context<Picker<Self>>) {
        self.search = None;
        let Some((mut rows, capped)) = found else {
            return;
        };
        if let Some(workbench) = self.workbench.upgrade() {
            rows = confirm_with_parses(rows, workbench.read(cx), cx);
        }
        self.rows = rows;
        self.capped = capped;
        self.refilter(cx);
        // The first row of the list that just landed isn't one moved to.
        let first = self.matches.first().map(|&(row, _)| &self.rows[row]);
        let here = first.filter(|row| Some(&row.path) == self.previewed_path.as_ref());
        self.preview.opened_at(here.map(|row| row.row));
    }

    /// Previews match `ix`, if it's in the previewed editor's file.
    fn preview(&mut self, ix: usize, cx: &mut App) {
        let Some(&(row, _)) = self.matches.get(ix) else {
            return;
        };
        let row = &self.rows[row];
        if Some(&row.path) != self.previewed_path.as_ref() {
            return;
        }
        self.preview.show(row.row, cx);
    }

    fn refilter(&mut self, cx: &mut Context<Picker<Self>>) {
        let (kind, rest) = leading_kind(&self.query);
        let query: Vec<char> = rest.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.rows.iter().enumerate())
            .filter(|(_, row)| kind.is_none_or(|kind| row.kind == kind))
            .filter_map(|(ix, row)| Some((ix, subsequence(&query, &row.matched)?)))
            .collect();
        let picker = cx.entity();
        // The picker is being updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }
}

#[cfg(test)]
impl TodoPicker {
    /// The matches: each kind, keyword, text and `file:row`.
    pub(crate) fn labels(&self) -> Vec<(Kind, String, String, String)> {
        (self.matches.iter())
            .map(|(row, _)| {
                let row = &self.rows[*row];
                let at = format!("{}:{}", row.file, row.row + 1);
                (row.kind, row.keyword.clone(), row.text.clone(), at)
            })
            .collect()
    }

    /// Whether the search is still running.
    pub(crate) fn searching(&self) -> bool {
        self.search.is_some()
    }
}

impl PickerDelegate for TodoPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.matches[ix].0.to_string().into()
    }

    fn rows_fill_width(&self) -> bool {
        true
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (row, matched) = &self.matches[ix];
        let row = &self.rows[*row];
        // The chars matched in the text: those before the file.
        let length = row.text.chars().count();
        let in_text: Vec<usize> = (matched.iter().copied())
            .filter(|&ix| ix < length)
            .collect();
        let label = HighlightedLabel::new(row.text.clone(), char_ranges(&row.text, &in_text), &ui);
        let color = self.colors.of(row.kind);
        let at = format!("{}:{}", row.file, row.row + 1);
        let keyword = row.keyword.clone();
        let text = row.text.clone();
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_2()
            .child(
                div()
                    .debug_selector(|| format!("todo-picker-badge:{keyword}:{text}"))
                    .flex_none()
                    .px_1()
                    .rounded_sm()
                    .bg(color.opacity(0.15))
                    .text_color(color)
                    .child(row.keyword.clone()),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(label),
            )
            .child(
                div()
                    .ml_auto()
                    .flex_none()
                    .whitespace_nowrap()
                    .text_color(ui.muted_text)
                    .child(at),
            )
            .into_any_element()
    }

    /// The keyword, the text after it and where it is, as the row shows them.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        let row = &self.rows[self.matches[ix].0];
        format!(
            "{}, {}, {}:{}",
            row.keyword,
            row.text,
            row.file,
            row.row + 1
        )
        .into()
    }

    /// Opens the row's file at the keyword.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(&(row, _)) = self.matches.get(ix) else {
            return false;
        };
        let Some(workbench) = self.workbench.upgrade() else {
            return true;
        };
        let row = self.rows[row].clone();
        match Some(&row.path) == self.previewed_path.as_ref() {
            true => {
                if let Some(editor) = self.preview.editor() {
                    editor.update(cx, |editor, _| self.preview.close_folds(editor));
                }
                self.preview.keep();
            }
            // Another file: what the previews moved in this one goes back.
            false => self.preview.restore(cx),
        }
        self.previous_focus = None;
        // Opening at a line is a jump, recorded there.
        workbench.update(cx, |workbench, cx| {
            let line = Some(row.row.saturating_add(1));
            let column = Some(row.column.saturating_add(1));
            workbench
                .open_document_at(row.path, line, column, window, cx)
                .detach();
        });
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.search = None;
        self.preview.restore(cx);
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        if self.no_root {
            Some("No folder open".into())
        } else if self.search.is_some() {
            Some("Searching…".into())
        } else if self.rows.is_empty() {
            Some(self.capped_note("No TODO comments").into())
        } else if self.matches.is_empty() {
            Some(self.capped_note("No matching TODO comments").into())
        } else if self.capped {
            Some(capped_text())
        } else {
            None
        }
    }
}

/// What the status line says of a search the cap stopped.
fn capped_text() -> SharedString {
    format!("Stopped at the first {} matches", group(MAX_ROWS)).into()
}

impl TodoPicker {
    /// `text`, with what the cap stopped said too if it stopped the search:
    /// there may be more beyond it.
    fn capped_note(&self, text: &str) -> String {
        match self.capped {
            true => format!("{text}: {}", capped_text().to_lowercase()),
            false => text.to_owned(),
        }
    }
}

/// `n` with its thousands grouped by commas.
fn group(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (ix, ch) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

impl Workbench {
    /// The TODO picker, while it shows.
    #[cfg(test)]
    pub(crate) fn todo_picker(&self, cx: &App) -> Option<Entity<Picker<TodoPicker>>> {
        self.shown_palette(Palette::Todos, cx)
    }

    /// Shows the TODO picker, focused, in place of any other palette, or
    /// afresh if it shows, and starts searching the root.
    pub(crate) fn show_todos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_palette(window, cx);
        let previous = window.focused(cx);
        let editor = self.active_editor(cx).cloned();
        let colors = Colors::new(self.pane().read(cx).theme());
        let delegate = TodoPicker::new(
            cx.entity().downgrade(),
            editor.as_ref(),
            colors,
            previous,
            cx,
        );
        let root = self.root().map(Path::to_owned);
        let picker =
            cx.new(|cx| Picker::new(delegate, "Search TODO comments…", fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| {
            picker.delegate.start(root, cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::Todos, picker, window, cx);
    }
}

#[cfg(test)]
#[path = "todo_picker_tests.rs"]
mod todo_picker_tests;
