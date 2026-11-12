//! Plugins' formatters ([`fe_plugin::formatting`]) for Format Document and
//! format on save: the last source [`crate::lsp_format`] tries.
//!
//! - **Which.** The formatter a plugin registered for the document's
//!   language, as the language servers route it
//!   ([`routed_language`](crate::lsp::routed_language)), asked only when
//!   neither the language's external formatter nor its server formatted:
//!   extensions yield to the built-in sources and to the user's own config.
//!   A selection is never a plugin's.
//! - **The request.** Made for the buffer by the id and version plugins know
//!   it by ([`crate::plugin_bridge`]), the snapshot at that version held
//!   while the request runs ([`Asked`]), so the plugin's answer is checked
//!   against the text it was asked about, whatever it reads meanwhile. The
//!   request blocks on the plugin's thread under its call deadline, so it
//!   waits on a thread of its own, and is waited for at most that deadline
//!   ([`PluginFormatter::ask`]); an answer later than that is dropped.
//! - **The answer.** Its edits go through the same checks as the other
//!   sources' (the buffer's version first) and apply as one undo step; see
//!   [`crate::lsp_format`]. A failure names the plugin, on one line.
//! - **Loads.** Each load of a plugin registers its formatters anew on the
//!   app's [`PluginFormatting`] ([`PluginFormatting::state`]); unloading it
//!   drops them ([`unload`]).

use std::{
    collections::HashMap,
    ops::Range,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread,
    time::Duration,
};

use fe_buffer::{BufferSnapshot, Version};
use fe_gpui::EditorView;
use fe_plugin::{
    decorations::BufferVersions,
    formatting::{FormatError, Formatters, State, TextEdit},
};
use futures::channel::oneshot;
use gpui::{App, AsyncApp, Entity, FutureExt as _, Global};
use kiln_l10n::tr;
use kiln_plugin::commands::CommandHost;

use crate::{lsp::routed_language, plugin_bridge, plugin_host};

/// The app's plugin formatters, and the snapshots their requests are
/// about. Clones share them.
#[derive(Clone, Default)]
pub(crate) struct PluginFormatting {
    formatters: Formatters,
    asked: Asked,
    /// What the requests did, for the tests: `asked <language>` as one
    /// starts and `answered` as it returns.
    #[cfg(test)]
    pub(crate) record: Arc<Mutex<Vec<String>>>,
}

impl Global for PluginFormatting {}

impl PluginFormatting {
    /// A new load of plugin `plugin`'s `formatting` slice, as far as
    /// `grants` allow, its commands those `commands` says it registered: an
    /// earlier load's formatters go.
    pub(crate) fn state(
        &self,
        plugin: &str,
        grants: &fe_command::Grants,
        commands: &CommandHost,
    ) -> State {
        let asked = Box::new(self.asked.clone());
        State::new(plugin, grants, &self.formatters, commands, asked)
    }

    /// Drops plugin `plugin`'s formatters, from any thread.
    pub(crate) fn forget(&self, plugin: &str) {
        self.formatters.unload(plugin);
    }
}

/// The app's [`PluginFormatting`], made on first use.
pub(crate) fn formatting(cx: &mut App) -> PluginFormatting {
    cx.default_global::<PluginFormatting>().clone()
}

/// Plugin `plugin` is unloaded: its formatters go.
pub(crate) fn unload(plugin: &str, cx: &App) {
    if let Some(formatting) = cx.try_global::<PluginFormatting>() {
        formatting.forget(plugin);
    }
}

/// The snapshots requests were made about, by buffer and plugin version,
/// each with how many requests hold it. A plugin's answer is checked
/// against them, and only them: one for a version no request names is
/// refused. Clones share them.
#[derive(Clone, Default)]
struct Asked(Arc<Mutex<Snapshots>>);

/// By buffer and plugin version: the snapshot, and how many hold it.
type Snapshots = HashMap<(u64, u64), (BufferSnapshot, usize)>;

impl Asked {
    /// Keeps `snapshot`, buffer `buffer` at plugin version `version`, until
    /// what this returns is dropped.
    fn hold(&self, buffer: u64, version: u64, snapshot: BufferSnapshot) -> Held {
        let mut asked = self.lock();
        let (_, holders) = asked.entry((buffer, version)).or_insert((snapshot, 0));
        *holders += 1;
        Held {
            asked: self.clone(),
            key: (buffer, version),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Snapshots> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl BufferVersions for Asked {
    /// None: the formatting slice checks answers against the version asked
    /// about alone.
    fn now(&mut self, _: u64) -> Option<BufferSnapshot> {
        None
    }

    fn at(&mut self, buffer: u64, version: u64) -> Option<BufferSnapshot> {
        let asked = self.lock();
        asked
            .get(&(buffer, version))
            .map(|(snapshot, _)| snapshot.clone())
    }
}

/// A request's hold on its snapshot.
struct Held {
    asked: Asked,
    key: (u64, u64),
}

impl Drop for Held {
    fn drop(&mut self) {
        let mut asked = self.asked.lock();
        if let Some((_, holders)) = asked.get_mut(&self.key) {
            *holders -= 1;
            if *holders == 0 {
                asked.remove(&self.key);
            }
        }
    }
}

/// The plugin formatter for a document's language.
pub(crate) struct PluginFormatter {
    formatting: PluginFormatting,
    language: String,
    /// Its plugin's name, for what's shown.
    name: String,
    /// The plugin's call deadline: the longest a request is waited for.
    deadline: Duration,
}

/// The plugin formatter for `view`'s document, if a plugin registered one
/// for its language.
pub(crate) fn formatter(view: &EditorView, cx: &App) -> Option<PluginFormatter> {
    let formatting = cx.try_global::<PluginFormatting>()?;
    let language = routed_language(view)?;
    let formatter = formatting.formatters.for_language(&language)?;
    let deadline = match cx.try_global::<plugin_host::PluginHost>() {
        Some(host) => host.limits.call_deadline,
        None => kiln_plugin::limits::Limits::default().call_deadline,
    };
    let name = plugin_host::plugin_name(&formatter.plugin, cx).unwrap_or(formatter.plugin);
    Some(PluginFormatter {
        formatting: formatting.clone(),
        language: language.into_owned(),
        name,
        deadline,
    })
}

/// What a plugin's formatter answered.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PluginAnswer {
    /// Its edits to the text at the version asked about: in order, apart
    /// and in the text.
    Edits(Vec<(Range<usize>, String)>),
    /// Nothing to change, or the buffer moved on before it was asked.
    Unchanged,
    /// There's none now, its plugin unloaded or the buffer in no window.
    NoFormatter,
    /// It failed: this says why, on one line, naming the plugin.
    Failed(String),
}

impl PluginFormatter {
    /// Asks the formatter to format `editor`'s buffer at `version`, and
    /// waits for its answer, at most the plugin's call deadline.
    pub(crate) async fn ask(
        self,
        editor: &Entity<EditorView>,
        version: Version,
        cx: &mut AsyncApp,
    ) -> PluginAnswer {
        // By the ids plugins know, which an app-wide update sees in every
        // window, the one formatting included.
        let asked = cx.update(|cx| {
            let (buffer, plugin_version, _) = (plugin_bridge::open_editors(cx).into_iter())
                .find(|(_, _, open)| open == editor)?;
            let model = editor.read(cx).model();
            let snapshot = (model.buffer().version() == version).then(|| model.buffer().snapshot());
            Some((buffer, plugin_version, snapshot))
        });
        let Some((buffer, plugin_version, snapshot)) = asked else {
            return PluginAnswer::NoFormatter;
        };
        let Some(snapshot) = snapshot else {
            return PluginAnswer::Unchanged;
        };
        let held = (self.formatting.asked).hold(buffer, plugin_version, snapshot);
        let formatters = self.formatting.formatters.clone();
        let language = self.language.clone();
        #[cfg(test)]
        let record = self.formatting.record.clone();
        #[cfg(test)]
        lock(&record).push(format!("asked {language}"));
        let (send, answer) = oneshot::channel();
        // The request blocks until the plugin returns, which may wait on
        // this thread for the buffer: on a thread of its own.
        let spawned = thread::Builder::new()
            .name(format!("format with {}", self.name))
            .spawn(move || {
                let reply = formatters.request(buffer, plugin_version, &language);
                drop(held);
                #[cfg(test)]
                lock(&record).push("answered".into());
                drop(send.send(reply));
            });
        if let Err(error) = spawned {
            let error = error.to_string();
            return self.failed(&tr!(
                "workbench-workbench-plugin-format-no-thread",
                error = error
            ));
        }
        let executor = cx.background_executor().clone();
        let reply = match answer.with_timeout(self.deadline, &executor).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => return self.failed(&tr!("workbench-workbench-plugin-format-dropped")),
            Err(_) => {
                let waited = self.deadline.as_millis();
                return self.failed(&tr!(
                    "workbench-workbench-plugin-format-timed-out",
                    ms = waited
                ));
            }
        };
        match reply {
            Ok(None) => PluginAnswer::Unchanged,
            Ok(Some(edits)) => PluginAnswer::Edits(edits.into_iter().map(edit).collect()),
            Err(FormatError::NoFormatter(_) | FormatError::Unloaded(_)) => {
                PluginAnswer::NoFormatter
            }
            Err(error) => self.failed(&error.to_string()),
        }
    }

    /// A failure, on one line, naming the plugin.
    fn failed(&self, why: &str) -> PluginAnswer {
        let why = why.lines().next().unwrap_or_default();
        let plugin = self.name.as_str();
        let failed = tr!(
            "workbench-workbench-plugin-format-failed",
            plugin = plugin,
            why = why
        );
        PluginAnswer::Failed(failed.into())
    }
}

/// A plugin's edit, checked against the text, as a buffer's.
fn edit(edit: TextEdit) -> (Range<usize>, String) {
    // In the text it was checked against, so within `usize`.
    let offset = |offset: u64| usize::try_from(offset).unwrap_or(usize::MAX);
    (offset(edit.range.start)..offset(edit.range.end), edit.text)
}

#[cfg(test)]
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "plugin_format_tests.rs"]
mod plugin_format_tests;
