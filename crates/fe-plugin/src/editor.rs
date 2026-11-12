//! What plugins may do to buffers: read their text, edit it, or propose a change.
//!
//! The app hands each plugin its buffers as a [`BufferAccess`]. A plugin reads with
//! `buffer.read`, and edits or proposes with `buffer.write`. Its edits are checked here against
//! the version it names, which has to be the buffer's version now, and then go in as one
//! transaction under [`Origin::Host`] with the plugin's id, so undo scoped to that origin reverts
//! the plugin's edits and nobody else's. A proposal is a [`ProposedChange`] under the same origin,
//! which the buffer doesn't see until the user accepts it.

use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use fe_buffer::{BufferSnapshot, Origin, ProposedChange};
use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::Error;
use kiln_plugin::limits::Limits;
use kiln_plugin::paths::{self, Rules};

use crate::HostState;
use crate::fastedit::editor::buffer::{self as wit, Host, Snapshot, TextEdit};

/// A buffer as it is now, as [`BufferAccess::snapshot`] gives it.
pub struct BufferText {
    pub snapshot: BufferSnapshot,
    /// The version plugins see: it increases with every change to the buffer.
    pub version: u64,
    /// The host's path, which the plugin is given as its guest path ([`kiln_plugin::paths`]).
    pub path: Option<String>,
    pub language: Option<String>,
}

/// A change a plugin offers, for the user to accept or reject.
pub struct Proposal {
    /// Shown with the change.
    pub title: String,
    pub change: ProposedChange,
}

/// The app's buffers, as one plugin reaches them. Buffers are named by the ids plugins see.
pub trait BufferAccess: Send {
    /// `buffer` as it is now; none when no open buffer has that id.
    fn snapshot(&mut self, buffer: u64) -> Option<BufferText>;

    /// Applies `edits` to `buffer` as one transaction under `origin`, through
    /// [`Buffer::edit_from`](fe_buffer::Buffer::edit_from) against `version`, and returns the
    /// version after. The edits are sorted, apart and on char boundaries in the text
    /// [`BufferAccess::snapshot`] gave at `version`. A buffer that has moved past `version`
    /// applies nothing and fails with [`Error::Conflict`].
    fn edit(
        &mut self,
        buffer: u64,
        version: u64,
        edits: Vec<(Range<usize>, String)>,
        origin: Origin,
    ) -> Result<u64, Error>;

    /// Holds `proposal`, made against `buffer`, for the user to accept or reject.
    fn propose(&mut self, buffer: u64, proposal: Proposal) -> Result<(), Error>;
}

/// Buffers the app also holds.
impl<T: BufferAccess> BufferAccess for Arc<Mutex<T>> {
    fn snapshot(&mut self, buffer: u64) -> Option<BufferText> {
        self.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot(buffer)
    }

    fn edit(
        &mut self,
        buffer: u64,
        version: u64,
        edits: Vec<(Range<usize>, String)>,
        origin: Origin,
    ) -> Result<u64, Error> {
        self.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .edit(buffer, version, edits, origin)
    }

    fn propose(&mut self, buffer: u64, proposal: Proposal) -> Result<(), Error> {
        self.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .propose(buffer, proposal)
    }
}

/// The buffer slice of [`HostState`]. The default reaches no buffers: every call is denied.
#[derive(Default)]
pub struct State {
    attached: Option<Attached>,
}

struct Attached {
    /// `Origin::Host` with the plugin's id.
    origin: Origin,
    grants: Grants,
    max_read_bytes: usize,
    /// How the host's paths are spelled.
    rules: Rules,
    access: Box<dyn BufferAccess>,
}

impl State {
    /// Plugin `plugin`'s slice over `access`, as far as `grants` allow, reading at most
    /// `limits.max_read_bytes` at once.
    pub fn new(
        plugin: &str,
        grants: &Grants,
        limits: &Limits,
        access: Box<dyn BufferAccess>,
    ) -> Self {
        Self {
            attached: Some(Attached {
                origin: Origin::Host(plugin.into()),
                grants: grants.clone(),
                max_read_bytes: limits.max_read_bytes,
                rules: Rules::HOST,
                access,
            }),
        }
    }

    /// The slice, reading `access`'s paths as `rules` spell them rather than as this host does:
    /// for tests of another OS's paths.
    pub fn with_path_rules(mut self, rules: Rules) -> Self {
        if let Some(attached) = &mut self.attached {
            attached.rules = rules;
        }
        self
    }

    /// The attachment, if its grants include `capability`, which `import` needs.
    fn granted(&mut self, import: &str, capability: Capability) -> Result<&mut Attached, Error> {
        self.attached
            .as_mut()
            .filter(|attached| attached.grants.allows(&capability))
            .ok_or_else(|| Error::Denied(format!("{import} needs the {capability} capability")))
    }
}

impl Attached {
    fn snapshot(&mut self, buffer: u64) -> Result<BufferText, Error> {
        self.access
            .snapshot(buffer)
            .ok_or_else(|| Error::NotFound(format!("no open buffer has id {buffer}")))
    }

    /// `buffer`, if it is still at `version`.
    fn at(&mut self, buffer: u64, version: u64) -> Result<BufferText, Error> {
        let text = self.snapshot(buffer)?;
        if text.version != version {
            return Err(Error::Conflict(format!(
                "buffer {buffer} is at version {}, not {version}",
                text.version
            )));
        }
        Ok(text)
    }
}

impl Host for HostState {
    fn read(&mut self, buffer: u64, range: Option<wit::Range>) -> Result<Snapshot, Error> {
        let attached = self
            .editor
            .granted("fastedit:editor/buffer.read", Capability::BufferRead)?;
        let text = attached.snapshot(buffer)?;
        let range = match range {
            Some(range) => checked(&text.snapshot, &range).map_err(|why| {
                Error::InvalidArgument(format!("range {}..{} {why}", range.start, range.end))
            })?,
            None => 0..text.snapshot.len(),
        };
        if range.len() > attached.max_read_bytes {
            return Err(Error::Failed(format!(
                "reading {} bytes of buffer {buffer} is over the {}-byte limit",
                range.len(),
                attached.max_read_bytes
            )));
        }
        Ok(Snapshot {
            buffer,
            version: text.version,
            text: text.snapshot.text().slice_to_string(range),
            path: (text.path).and_then(|path| paths::guest(&path, attached.rules)),
            language: text.language,
        })
    }

    fn edit(&mut self, buffer: u64, version: u64, edits: Vec<TextEdit>) -> Result<u64, Error> {
        let attached = self
            .editor
            .granted("fastedit:editor/buffer.edit", Capability::BufferWrite)?;
        let text = attached.at(buffer, version)?;
        let edits = checked_edits(&text.snapshot, edits)?;
        let origin = attached.origin.clone();
        attached.access.edit(buffer, version, edits, origin)
    }

    fn propose(&mut self, change: wit::ProposedChange) -> Result<(), Error> {
        let attached = self
            .editor
            .granted("fastedit:editor/buffer.propose", Capability::BufferWrite)?;
        let text = attached.at(change.buffer, change.version)?;
        let edits = checked_edits(&text.snapshot, change.edits)?;
        let proposal = Proposal {
            title: change.title,
            change: ProposedChange::from_edits(&text.snapshot, edits, attached.origin.clone()),
        };
        attached.access.propose(change.buffer, proposal)
    }
}

/// `range` in `snapshot`'s text, or why it isn't one.
fn checked(snapshot: &BufferSnapshot, range: &wit::Range) -> Result<Range<usize>, String> {
    let len = snapshot.len();
    let offset = |offset: u64| usize::try_from(offset).ok().filter(|&offset| offset <= len);
    let (Some(start), Some(end)) = (offset(range.start), offset(range.end)) else {
        return Err(format!("is past the end of the text, at {len}"));
    };
    if start > end {
        return Err("ends before it starts".to_owned());
    }
    let text = snapshot.text();
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return Err("splits a character".to_owned());
    }
    Ok(start..end)
}

/// `edits` as ranges in `snapshot`'s text, if each range is one and they are in order and apart.
/// A range may touch the next, but two insertions can't share an offset: the buffer would merge
/// them, in no order the plugin chose.
fn checked_edits(
    snapshot: &BufferSnapshot,
    edits: Vec<TextEdit>,
) -> Result<Vec<(Range<usize>, String)>, Error> {
    let mut checked_edits: Vec<(Range<usize>, String)> = Vec::with_capacity(edits.len());
    for (index, edit) in edits.into_iter().enumerate() {
        let range = checked(snapshot, &edit.range).map_err(|why| {
            Error::InvalidArgument(format!(
                "edit {index}'s range {}..{} {why}",
                edit.range.start, edit.range.end
            ))
        })?;
        if let Some((previous, _)) = checked_edits.last()
            && (previous.end > range.start
                || previous.is_empty() && range.is_empty() && previous.start == range.start)
        {
            return Err(Error::InvalidArgument(format!(
                "edit {index}'s range {range:?} overlaps or comes before edit {}'s, {previous:?}",
                index - 1
            )));
        }
        checked_edits.push((range, edit.text));
    }
    Ok(checked_edits)
}
