// SPDX-License-Identifier: LicenseRef-Proprietary

use std::{fmt, str::FromStr, sync::Arc};

use serde::{Deserialize, Serialize};

/// A stable, namespaced command name: two or more `.`-separated segments,
/// each `[a-z][a-z0-9_]*`, such as `editor.move_left` or `app.quit`.
/// Serializes as the plain string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CommandId(Arc<str>);

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid command ID {id:?}: {reason}")]
pub struct InvalidCommandId {
    pub id: String,
    pub reason: &'static str,
}

impl CommandId {
    pub fn parse(id: &str) -> Result<Self, InvalidCommandId> {
        let invalid = |reason| InvalidCommandId {
            id: id.to_string(),
            reason,
        };
        let mut segments = 0;
        for segment in id.split('.') {
            let mut chars = segment.chars();
            match chars.next() {
                None => return Err(invalid("a segment is empty")),
                Some('a'..='z') => {}
                Some(_) => return Err(invalid("a segment must start with a-z")),
            }
            if !chars.all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '_')) {
                return Err(invalid("segments may only contain a-z, 0-9 and _"));
            }
            segments += 1;
        }
        if segments < 2 {
            return Err(invalid("needs a namespace, as in `editor.move_left`"));
        }
        Ok(Self(id.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The first segment: `editor` for `editor.move_left`.
    pub fn namespace(&self) -> &str {
        self.0.split('.').next().unwrap_or_default()
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for CommandId {
    type Err = InvalidCommandId;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::parse(id)
    }
}

impl TryFrom<String> for CommandId {
    type Error = InvalidCommandId;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::parse(&id)
    }
}

impl TryFrom<&str> for CommandId {
    type Error = InvalidCommandId;

    fn try_from(id: &str) -> Result<Self, Self::Error> {
        Self::parse(id)
    }
}

impl From<CommandId> for String {
    fn from(id: CommandId) -> Self {
        id.0.to_string()
    }
}

impl AsRef<str> for CommandId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
