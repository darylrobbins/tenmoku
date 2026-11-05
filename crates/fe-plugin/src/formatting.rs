//! Formatters plugins offer for Format Document.
//!
//! A plugin with `buffer.read` and `buffer.write` registers one of its own commands as the
//! formatter for languages. The [`Formatters`] keep them, language to plugin and command, and
//! [`Formatters::request`] asks one: it runs the command on the plugin's thread, under the
//! plugin's call deadline, with the request's id, buffer, version and language. The command
//! reads the text and replies through `answer`, whose edits are checked against the text at the
//! requested version. An overrun unloads the plugin, as for any call, and fails the request.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::{Error, unimplemented};
use kiln_plugin::commands::{CommandHost, PluginHandle};
use kiln_plugin::runner::{Call, CallError, Reason, Reply};

use crate::HostState;
use crate::decorations::BufferVersions;
pub use crate::fastedit::editor::buffer::TextEdit;
use crate::fastedit::editor::formatting::Host;

/// The formatter for a language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formatter {
    pub language: String,
    pub plugin: String,
    /// The plugin's own command that formats.
    pub command: String,
}

/// Why a format request failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatError {
    NoFormatter(String),
    /// The formatter's plugin is unloaded.
    Unloaded(String),
    /// The command ran past the plugin's call deadline, which unloaded the plugin.
    Timeout(String),
    /// The command failed, or its plugin was unloaded while it ran.
    Failed(String),
    /// The plugin answered with edits that are out of order, overlap or are not in the text.
    BadAnswer(String),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFormatter(language) => write!(f, "no plugin formats {language}"),
            Self::Unloaded(plugin) => write!(f, "plugin {plugin} is unloaded"),
            Self::Timeout(why) | Self::Failed(why) | Self::BadAnswer(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for FormatError {}

/// Every plugin's formatters, which the app shares with every plugin's formatting slice. Clones
/// share it.
#[derive(Clone, Default)]
pub struct Formatters {
    shared: Arc<Mutex<Shared>>,
}

#[derive(Default)]
struct Shared {
    /// Each loaded plugin's attachment: the number of its load.
    attached: BTreeMap<String, u64>,
    loads: u64,
    /// Each plugin's handle, with the load it was bound to.
    handles: BTreeMap<String, (u64, PluginHandle)>,
    /// By language.
    entries: BTreeMap<String, (String, String)>,
    requests: u64,
    pending: HashMap<u64, Pending>,
}

struct Pending {
    plugin: String,
    buffer: u64,
    version: u64,
    answer: Answer,
}

enum Answer {
    Waiting,
    Edits(Vec<TextEdit>),
    Rejected(String),
}

impl Formatters {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every formatter, by language.
    pub fn formatters(&self) -> Vec<Formatter> {
        let shared = self.lock();
        let entries = shared.entries.iter();
        entries
            .map(|(language, (plugin, command))| Formatter {
                language: language.clone(),
                plugin: plugin.clone(),
                command: command.clone(),
            })
            .collect()
    }

    /// The formatter for `language`.
    pub fn for_language(&self, language: &str) -> Option<Formatter> {
        self.formatters()
            .into_iter()
            .find(|formatter| formatter.language == language)
    }

    /// Sets `handle`, the loaded plugin `plugin`, as what [`request`](Self::request) calls.
    pub fn bind(&self, plugin: &str, handle: PluginHandle) {
        let mut shared = self.lock();
        if let Some(&load) = shared.attached.get(plugin) {
            shared.handles.insert(plugin.to_owned(), (load, handle));
        }
    }

    /// Forgets plugin `plugin`: its formatters go, and its slice registers and answers nothing
    /// more.
    pub fn unload(&self, plugin: &str) {
        self.lock().forget(plugin, true);
    }

    /// Asks the formatter for `language` to format `buffer` at `version`: runs its command on
    /// the plugin's thread under its call deadline, and returns the edits it answered, none if
    /// it returned without answering. A call that overruns its deadline, or otherwise unloads
    /// the plugin, unloads its formatters too.
    pub fn request(
        &self,
        buffer: u64,
        version: u64,
        language: &str,
    ) -> Result<Option<Vec<TextEdit>>, FormatError> {
        let (plugin, command, handle, request, load) = {
            let mut shared = self.lock();
            let (plugin, command) = (shared.entries.get(language))
                .cloned()
                .ok_or_else(|| FormatError::NoFormatter(language.to_owned()))?;
            let (load, handle) = (shared.handles.get(&plugin))
                .cloned()
                .filter(|(load, _)| shared.attached.get(&plugin) == Some(load))
                .ok_or_else(|| FormatError::Unloaded(plugin.clone()))?;
            shared.requests += 1;
            let request = shared.requests;
            shared.pending.insert(
                request,
                Pending {
                    plugin: plugin.clone(),
                    buffer,
                    version,
                    answer: Answer::Waiting,
                },
            );
            (plugin, command, handle, request, load)
        };
        let args = format!(
            r#"{{"request":{request},"buffer":{buffer},"version":{version},"language":{}}}"#,
            json_string(language)
        );
        // The plugin answers on its own thread while this one waits, so the lock is not held.
        let reply =
            (handle.lock().unwrap_or_else(PoisonError::into_inner)).call(Call::RunCommand {
                id: command.clone(),
                args,
            });
        let answer = self
            .lock()
            .pending
            .remove(&request)
            .map(|pending| pending.answer);
        // An unload or a reload meanwhile drops the request; only its own load's failure
        // unloads the plugin.
        let unload = |this: &Self| this.lock().forget_load(&plugin, load);
        let mut failed = match reply {
            Ok(Reply::RunCommand(Ok(()))) => None,
            Ok(Reply::RunCommand(Err(error))) => Some(FormatError::Failed(format!(
                "{command}: plugin {plugin} returned {error:?}"
            ))),
            Ok(reply) => Some(FormatError::Failed(format!(
                "{command}: plugin {plugin} answered {reply:?}"
            ))),
            Err(CallError::Failed(diagnostic)) => {
                unload(self);
                Some(if diagnostic.reason == Reason::Timeout {
                    FormatError::Timeout(format!("{command}: {diagnostic}"))
                } else {
                    FormatError::Failed(format!("{command}: {diagnostic}"))
                })
            }
            Err(CallError::Unloaded) => {
                unload(self);
                Some(FormatError::Unloaded(plugin.clone()))
            }
        };
        if answer.is_none() && failed.is_none() {
            failed = Some(FormatError::Unloaded(plugin));
        }
        match (answer, failed) {
            // An overrun comes first: it is what unloaded the plugin.
            (_, Some(timeout @ FormatError::Timeout(_))) => Err(timeout),
            (Some(Answer::Rejected(why)), _) => Err(FormatError::BadAnswer(why)),
            (_, Some(failed)) => Err(failed),
            (Some(Answer::Edits(edits)), None) => Ok(Some(edits)),
            (_, None) => Ok(None),
        }
    }

    /// Attaches a new load of `plugin`, which starts with no formatters.
    fn attach(&self, plugin: &str) -> u64 {
        let mut shared = self.lock();
        shared.loads += 1;
        let load = shared.loads;
        shared.forget(plugin, true);
        shared.attached.insert(plugin.to_owned(), load);
        load
    }

    fn register(
        &self,
        plugin: &str,
        load: u64,
        languages: Vec<String>,
        command: &str,
    ) -> Result<(), Error> {
        let mut shared = self.lock();
        shared.current(plugin, load)?;
        for language in languages {
            let entry = (plugin.to_owned(), command.to_owned());
            shared.entries.insert(language, entry);
        }
        Ok(())
    }

    /// The buffer and version `request` of plugin `plugin`'s asks about.
    fn asked(&self, plugin: &str, load: u64, request: u64) -> Result<(u64, u64), Error> {
        let mut shared = self.lock();
        shared.current(plugin, load)?;
        match shared.pending.get_mut(&request) {
            Some(pending) if pending.plugin == plugin => match pending.answer {
                Answer::Waiting => Ok((pending.buffer, pending.version)),
                _ => {
                    // A second answer fails the request, whatever the first was.
                    let why = format!("request {request} was answered already");
                    pending.answer = Answer::Rejected(why.clone());
                    Err(Error::InvalidArgument(why))
                }
            },
            _ => Err(Error::NotFound(format!(
                "plugin {plugin} has no request {request} waiting"
            ))),
        }
    }

    fn answered(&self, request: u64, answer: Answer) {
        if let Some(pending) = self.lock().pending.get_mut(&request) {
            pending.answer = answer;
        }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Shared {
    /// Drops what plugin `plugin` registered, and with `detach` its attachment and handle.
    fn forget(&mut self, plugin: &str, detach: bool) {
        self.entries.retain(|_, (owner, _)| owner != plugin);
        self.pending.retain(|_, pending| pending.plugin != plugin);
        if detach {
            self.attached.remove(plugin);
            self.handles.remove(plugin);
        }
    }

    /// Unloads `plugin` if `load` is still its current load.
    fn forget_load(&mut self, plugin: &str, load: u64) {
        if self.attached.get(plugin) == Some(&load) {
            self.forget(plugin, true);
        }
    }

    /// Refuses `plugin` unless `load` is its current one.
    fn current(&self, plugin: &str, load: u64) -> Result<(), Error> {
        if self.attached.get(plugin) == Some(&load) {
            Ok(())
        } else {
            Err(Error::Denied(format!("plugin {plugin} was unloaded")))
        }
    }
}

/// `text` as a JSON string.
fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The formatting slice of [`HostState`]. The default has no [`Formatters`] and answers every
/// import with [`unimplemented`].
#[derive(Default)]
pub struct State {
    attached: Option<Attached>,
}

struct Attached {
    plugin: String,
    grants: Grants,
    formatters: Formatters,
    load: u64,
    commands: CommandHost,
    buffers: Box<dyn BufferVersions>,
}

impl State {
    /// Plugin `plugin`'s slice on `formatters`, as far as `grants` allow; `commands` says which
    /// commands it registered, and `buffers` holds the text its answers are checked against. It
    /// is a new load of the plugin: an earlier load's formatters are dropped, and that load's
    /// slice registers nothing more.
    pub fn new(
        plugin: &str,
        grants: &Grants,
        formatters: &Formatters,
        commands: &CommandHost,
        buffers: Box<dyn BufferVersions>,
    ) -> Self {
        Self {
            attached: Some(Attached {
                plugin: plugin.to_owned(),
                grants: grants.clone(),
                load: formatters.attach(plugin),
                formatters: formatters.clone(),
                commands: commands.clone(),
                buffers,
            }),
        }
    }

    fn attached(&mut self, import: &str) -> Result<&mut Attached, Error> {
        match self.attached.as_mut() {
            Some(attached) => Ok(attached),
            None => unimplemented(import),
        }
    }
}

impl Host for HostState {
    fn register(&mut self, languages: Vec<String>, command: String) -> Result<(), Error> {
        let attached = self
            .formatting
            .attached("fastedit:editor/formatting.register")?;
        for capability in [Capability::BufferRead, Capability::BufferWrite] {
            if !attached.grants.allows(&capability) {
                return Err(Error::Denied(format!(
                    "fastedit:editor/formatting.register needs the {capability} capability"
                )));
            }
        }
        let own = (attached.commands.registered(&attached.plugin))
            .iter()
            .any(|id| id.as_str() == command);
        if !own {
            return Err(Error::InvalidArgument(format!(
                "{command} is not a command plugin {} registered",
                attached.plugin
            )));
        }
        (attached.formatters).register(&attached.plugin, attached.load, languages, &command)
    }

    fn answer(&mut self, request: u64, edits: Vec<TextEdit>) -> Result<(), Error> {
        let attached = self
            .formatting
            .attached("fastedit:editor/formatting.answer")?;
        let (buffer, version) =
            (attached.formatters).asked(&attached.plugin, attached.load, request)?;
        let Some(text) = attached.buffers.at(buffer, version) else {
            let error = format!("buffer {buffer}'s version {version} is no longer held");
            (attached.formatters).answered(request, Answer::Rejected(error.clone()));
            return Err(Error::InvalidArgument(error));
        };
        match check(text.text(), &edits) {
            Ok(()) => attached.formatters.answered(request, Answer::Edits(edits)),
            Err(why) => {
                let error = format!("request {request}: {why}");
                (attached.formatters).answered(request, Answer::Rejected(error.clone()));
                return Err(Error::InvalidArgument(error));
            }
        }
        Ok(())
    }
}

/// Whether `edits` are in `text`, on char boundaries, in order and apart: a range may touch the
/// next, but two insertions can't share an offset.
fn check(text: &fe_buffer::Text, edits: &[TextEdit]) -> Result<(), String> {
    let len = text.len();
    let mut previous: Option<Range<usize>> = None;
    for (index, edit) in edits.iter().enumerate() {
        let offset = |offset: u64| usize::try_from(offset).ok().filter(|&offset| offset <= len);
        let (start, end) = (edit.range.start, edit.range.end);
        let (Some(start), Some(end)) = (offset(start), offset(end)) else {
            return Err(format!(
                "edit {index}'s range {}..{} is past the end of the text, at {len}",
                edit.range.start, edit.range.end
            ));
        };
        if start > end {
            return Err(format!(
                "edit {index}'s range {start}..{end} ends before it starts"
            ));
        }
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Err(format!(
                "edit {index}'s range {start}..{end} splits a character"
            ));
        }
        if let Some(previous) = &previous
            && (previous.end > start
                || previous.is_empty() && start == end && previous.start == start)
        {
            return Err(format!(
                "edit {index}'s range {start}..{end} overlaps or comes before edit {}'s, {previous:?}",
                index - 1
            ));
        }
        previous = Some(start..end);
    }
    Ok(())
}
