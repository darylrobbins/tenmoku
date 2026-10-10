// SPDX-License-Identifier: LicenseRef-Proprietary

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CommandId, ContextStack};

/// One record of "this was asked for". Plain data with no process-local
/// handles, so it can be logged, recorded as a macro, sent as an AI tool
/// call, saved as a repro test, kept in an audit log or replicated to a
/// remote host. As a JSON line:
///
/// ```json
/// {"command":"editor.move","args":{"motion":"word_left","extend":true,"count":3},"actor":"user","source":"keyboard","target":"focused","context":[{"name":"editor"}]}
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invocation {
    pub command: CommandId,
    /// `Null` for commands without args, and then left out when
    /// serialized. Otherwise a table, typed by the command at dispatch.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub args: Value,
    #[serde(default)]
    pub actor: Actor,
    #[serde(default)]
    pub source: Source,
    #[serde(default)]
    pub target: Target,
    /// The context it was dispatched in; the host stamps it, so a replay
    /// checks predicates the same way.
    #[serde(default)]
    pub context: ContextStack,
}

impl Invocation {
    /// An invocation of `command` with no args, by the user, from a test,
    /// on the focused target, in no context.
    pub fn new(command: CommandId) -> Self {
        Self {
            command,
            args: Value::Null,
            actor: Actor::default(),
            source: Source::default(),
            target: Target::default(),
            context: ContextStack::default(),
        }
    }

    /// Parses `command` as a [`CommandId`] for [`Invocation::new`].
    pub fn of(command: &str) -> Result<Self, crate::InvalidCommandId> {
        Ok(Self::new(CommandId::parse(command)?))
    }

    pub fn args(mut self, args: Value) -> Self {
        self.args = args;
        self
    }

    pub fn actor(mut self, actor: Actor) -> Self {
        self.actor = actor;
        self
    }

    pub fn source(mut self, source: Source) -> Self {
        self.source = source;
        self
    }

    pub fn target(mut self, target: Target) -> Self {
        self.target = target;
        self
    }

    pub fn context(mut self, context: ContextStack) -> Self {
        self.context = context;
        self
    }
}

/// Who asked. A document model can map it to its own record of who made
/// each edit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    #[default]
    User,
    Ai {
        agent: String,
        session: String,
    },
    Lsp,
    Format,
    Host(String),
}

/// How it was triggered.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Keyboard,
    Mouse,
    Menu,
    Palette,
    Script,
    Replay,
    #[default]
    Test,
}

/// What it applies to: a name the host resolves, never a pointer or handle.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// Whatever has focus when it's dispatched.
    #[default]
    Focused,
    /// A view by the host's ID for it.
    View(u64),
    Buffer {
        path: PathBuf,
    },
}
