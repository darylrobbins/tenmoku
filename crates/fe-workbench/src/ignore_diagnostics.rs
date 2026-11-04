//! An open ignore file's problems ([`fe_project::check_ignore_file`]) shown as
//! a language server's are: unused patterns dimmed, duplicate and shadowed ones
//! warned about, invalid ones marked as errors, each with a Problems row.
//!
//! An ignore file is checked when it opens, when it's saved or reloaded, when
//! its path or language changes, and when the file tree changes, [`DEBOUNCE`]
//! after the last of those. The check walks the project in the background; a
//! newer one cancels it, and only the newest one's result is shown. Edits don't
//! start a check: what the last one found moves with the text until a save.
//!
//! An editor's diagnostics are all keyed by one pair of types, and applying
//! under another pair replaces them. A language server for an ignore file would
//! take the file's diagnostics this way, so what it has published is left alone
//! here, and the check's are shown again once it has none.

use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use fe_buffer::{BufferId, DiskStamp, Version};
use fe_editor::{
    EditorModel,
    diagnostics::{Diagnostic, DiagnosticTag, Severity},
};
use fe_gpui::EditorView;
use fe_project::{
    FsChange, IgnoreFinding, IgnoreFindingKind, IgnoreReport, IgnoreSeverity, check_ignore_file,
};
use gpui::{App, AppContext as _, Context, Entity, Task};

use crate::Workbench;

/// How long after the last reason to check an ignore file it's checked.
pub(crate) const DEBOUNCE: Duration = Duration::from_millis(500);

/// What these diagnostics say produced them.
const SOURCE: &str = "ignore";

/// The language an ignore file is highlighted as.
const LANGUAGE: &str = "gitignore";

/// The "server" of these diagnostics, whose identity is a check's generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IgnoreSource;

/// What checks an ignore file: [`check_ignore_file`], but for tests.
pub(crate) type Check = dyn Fn(&Path, &str, &AtomicBool) -> IgnoreReport + Send + Sync;

/// The open ignore files' checks.
pub(crate) struct IgnoreDiagnostics {
    slots: HashMap<BufferId, Slot>,
    /// The generation last given to a check. Across buffers, so that one
    /// reopened can't be confused with an older check of it.
    generation: u64,
    check: Arc<Check>,
}

impl Default for IgnoreDiagnostics {
    fn default() -> Self {
        Self {
            slots: HashMap::new(),
            generation: 0,
            check: Arc::new(check_ignore_file),
        }
    }
}

/// An open ignore file.
struct Slot {
    path: PathBuf,
    /// The file on disk as last seen, for telling that it was saved.
    stamp: Option<DiskStamp>,
    dirty: bool,
    /// The newest check asked for: only its result is shown.
    generation: u64,
    /// Stops that check's walk, which dropping `task` can't.
    cancel: Arc<AtomicBool>,
    /// The wait, then the check, until its result is shown.
    task: Option<Task<()>>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Workbench {
    /// Follows the tabs opening and closing: run with each change to them.
    pub(crate) fn sync_ignore_files(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let buffers: HashSet<BufferId> = (editors.iter())
            .map(|editor| editor.read(cx).model().buffer().id())
            .collect();
        self.ignore_diagnostics
            .slots
            .retain(|id, _| buffers.contains(id));
        for editor in &editors {
            self.sync_ignore_editor(editor, cx);
        }
    }

    /// Follows a change to `editor`: run with each of its notifications, so
    /// most are for a file that has nothing to do with this.
    pub(crate) fn sync_ignore_editor(
        &mut self,
        editor: &Entity<EditorView>,
        cx: &mut Context<Self>,
    ) {
        let view = editor.read(cx);
        let id = view.model().buffer().id();
        if self.ignore_diagnostics.slots.contains_key(&id) || is_ignore_file(view) {
            self.sync_ignore_buffer(id, cx);
        }
    }

    /// Brings buffer `id`'s editors, slot and check up to date.
    fn sync_ignore_buffer(&mut self, id: BufferId, cx: &mut Context<Self>) {
        let (wanted, others): (Vec<_>, Vec<_>) = (self.editors_of(id, cx).into_iter())
            .partition(|editor| is_ignore_file(editor.read(cx)));
        for editor in others {
            forget_diagnostics(&editor, cx);
        }
        let Some(first) = wanted.first() else {
            self.ignore_diagnostics.slots.remove(&id);
            return;
        };
        let model = first.read(cx).model();
        let Some(path) = model.path().map(Path::to_owned) else {
            return;
        };
        let stamp = model.buffer().disk_stamp();
        let dirty = model.buffer().is_dirty();
        let lost = (wanted.iter()).any(|editor| lost_diagnostics(editor.read(cx).model()));
        let check = match self.ignore_diagnostics.slots.get_mut(&id) {
            None => {
                let slot = Slot {
                    path,
                    stamp,
                    dirty,
                    generation: 0,
                    cancel: Arc::default(),
                    task: None,
                };
                self.ignore_diagnostics.slots.insert(id, slot);
                true
            }
            Some(slot) => {
                let saved = slot.stamp != stamp || (slot.dirty && !dirty);
                let moved = slot.path != path;
                // One that's under way or waiting will show its result on them.
                let lost = lost && slot.task.is_none();
                slot.path = path;
                slot.stamp = stamp;
                slot.dirty = dirty;
                saved || moved || lost
            }
        };
        if check {
            self.schedule_ignore_check(id, cx);
        }
    }

    /// The file tree changed: every open ignore file is checked again.
    fn recheck_ignore_files(&mut self, cx: &mut Context<Self>) {
        self.forget_ignore_matches();
        let ids: Vec<BufferId> = self.ignore_diagnostics.slots.keys().copied().collect();
        for id in ids {
            self.schedule_ignore_check(id, cx);
        }
    }

    /// Replaces any check of buffer `id` with one [`DEBOUNCE`] from now.
    fn schedule_ignore_check(&mut self, id: BufferId, cx: &mut Context<Self>) {
        let state = &mut self.ignore_diagnostics;
        state.generation += 1;
        let generation = state.generation;
        let Some(slot) = state.slots.get_mut(&id) else {
            return;
        };
        slot.cancel.store(true, Ordering::Relaxed);
        slot.cancel = Arc::default();
        slot.generation = generation;
        slot.task = Some(cx.spawn(async move |workbench, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let Ok(Some((version, check))) =
                workbench.update(cx, |workbench, cx| workbench.start_ignore_check(id, cx))
            else {
                return;
            };
            let report = cx.background_spawn(check).await;
            workbench
                .update(cx, |workbench, cx| {
                    workbench.ignore_check_landed(id, generation, version, report, cx)
                })
                .ok();
        }));
    }

    /// The wait is over: the buffer's text now, and the check of it to run off
    /// this thread.
    fn start_ignore_check(
        &mut self,
        id: BufferId,
        cx: &App,
    ) -> Option<(Version, impl Future<Output = IgnoreReport> + Send + use<>)> {
        let state = &self.ignore_diagnostics;
        let slot = state.slots.get(&id)?;
        let editors = self.editors_of(id, cx);
        // The buffer's: the model's snapshot trails it in a hidden tab.
        let buffer = editors.first()?.read(cx).model().buffer();
        let version = buffer.version();
        let text = buffer.text().clone();
        let path = slot.path.clone();
        let cancel = slot.cancel.clone();
        let check = state.check.clone();
        Some((
            version,
            async move { check(&path, &text.to_string(), &cancel) },
        ))
    }

    /// Shows what the check `generation` of buffer `id` found in its text at
    /// `version`, unless a newer one has been asked for. Replaces what its
    /// editors show, with nothing if it found nothing.
    pub(crate) fn ignore_check_landed(
        &mut self,
        id: BufferId,
        generation: u64,
        version: Version,
        report: IgnoreReport,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.ignore_diagnostics.slots.get_mut(&id) else {
            return;
        };
        if slot.generation != generation {
            return;
        }
        slot.task = None;
        if report.cancelled {
            return;
        }
        let items: Vec<_> = (report.findings.iter()).map(diagnostic).collect();
        for editor in self.editors_of(id, cx) {
            let view = editor.read(cx);
            if !is_ignore_file(view) || holds_others_diagnostics(view.model()) {
                continue;
            }
            editor.update(cx, |view, cx| {
                let shown = view.model_mut().apply_diagnostics(
                    IgnoreSource,
                    generation,
                    version,
                    items.clone(),
                    Some(generation),
                );
                if shown {
                    cx.notify();
                }
            });
        }
    }

    /// What checks an ignore file.
    pub(crate) fn ignore_check(&self) -> Arc<Check> {
        self.ignore_diagnostics.check.clone()
    }

    /// Checks with `check` from now on.
    #[cfg(test)]
    pub(crate) fn set_ignore_check(&mut self, check: Arc<Check>) {
        self.ignore_diagnostics.check = check;
    }

    /// How many open files have a slot.
    #[cfg(test)]
    pub(crate) fn ignore_slot_count(&self) -> usize {
        self.ignore_diagnostics.slots.len()
    }

    /// The newest check of buffer `id`: its generation, and what stops it.
    #[cfg(test)]
    pub(crate) fn newest_ignore_check(&self, id: BufferId) -> Option<(u64, Arc<AtomicBool>)> {
        let slot = self.ignore_diagnostics.slots.get(&id)?;
        Some((slot.generation, slot.cancel.clone()))
    }
}

/// A watcher batch reached the workbench, which is being updated. Git's own
/// files change all the time and say nothing about the patterns' matches.
pub(crate) fn tree_changed(changes: &[FsChange], cx: &mut Context<Workbench>) {
    let relevant = changes.iter().any(|change| match change {
        FsChange::Path(path) => !in_git_dir(path),
        FsChange::Rescan => true,
    });
    if !relevant {
        return;
    }
    let workbench = cx.weak_entity();
    cx.defer(move |cx| {
        workbench
            .update(cx, |workbench, cx| workbench.recheck_ignore_files(cx))
            .ok();
    });
}

fn in_git_dir(path: &Path) -> bool {
    path.components()
        .any(|part| part.as_os_str() == OsStr::new(".git"))
}

/// Whether `view` shows a file the check applies to: one in the ignore-file
/// language, as picked by hand or else by its path, which is known without its
/// grammar loading, and which Docker or Podman don't read by other rules.
pub(crate) fn is_ignore_file(view: &EditorView) -> bool {
    let Some(path) = view.model().path() else {
        return false;
    };
    let language = match view.is_language_chosen() {
        true => view.syntax().map(|syntax| syntax.language().name()),
        false => fe_syntax::language_name_for_path(path),
    };
    language == Some(LANGUAGE) && !has_container_syntax(path)
}

/// Whether `path` is named as Docker's or Podman's files are, in any case, as
/// its language is found.
fn has_container_syntax(path: &Path) -> bool {
    let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
    let name = name.to_ascii_lowercase();
    name.ends_with(".dockerignore") || name.ends_with(".containerignore")
}

/// Whether `model` has diagnostics that aren't this module's.
fn holds_others_diagnostics(model: &EditorModel) -> bool {
    model.diagnostic_set::<IgnoreSource, u64>().is_none() && !model.diagnostics().is_empty()
}

/// Whether `model` has lost what a check found, or never had it, with nothing
/// in its place: the diagnostics were reset for a new root, or it's a new
/// editor of the buffer.
fn lost_diagnostics(model: &EditorModel) -> bool {
    model.diagnostic_set::<IgnoreSource, u64>().is_none() && model.diagnostics().is_empty()
}

/// Takes this module's diagnostics from `editor`, if it has them. The set is
/// all of its diagnostics, as it's keyed by [`IgnoreSource`].
fn forget_diagnostics(editor: &Entity<EditorView>, cx: &mut App) {
    if editor
        .read(cx)
        .model()
        .diagnostic_set::<IgnoreSource, u64>()
        .is_some()
    {
        editor.update(cx, |view, cx| {
            view.model_mut().reset_diagnostics();
            cx.notify();
        });
    }
}

/// What a line's finding shows as, over its pattern.
fn diagnostic(finding: &IgnoreFinding) -> (std::ops::Range<usize>, Diagnostic) {
    let severity = match finding.severity {
        IgnoreSeverity::Hint => Severity::Hint,
        IgnoreSeverity::Warning => Severity::Warning,
        IgnoreSeverity::Error => Severity::Error,
    };
    let (message, tags) = match &finding.kind {
        IgnoreFindingKind::Unused => (
            "This pattern matches nothing in the project".to_owned(),
            vec![DiagnosticTag::Unnecessary],
        ),
        IgnoreFindingKind::Duplicate { first_line } => {
            (format!("Duplicate of line {first_line}"), Vec::new())
        }
        IgnoreFindingKind::Shadowed { by_line } => (
            format!("Everything this pattern matches is decided the other way by line {by_line}"),
            Vec::new(),
        ),
        IgnoreFindingKind::Invalid(reason) => (reason.clone(), Vec::new()),
    };
    let mut diagnostic = Diagnostic::new(severity, message);
    diagnostic.source = Some(SOURCE.to_owned());
    diagnostic.tags = tags;
    (finding.range.clone(), diagnostic)
}

#[cfg(test)]
#[path = "ignore_diagnostics_tests.rs"]
mod ignore_diagnostics_tests;
