// SPDX-License-Identifier: Apache-2.0

//! A source breakpoint as the user set it (docs/decisions/r1b9dw3g0g-debugger.md
//! §5, card D5): the data a host's breakpoint store keeps, persists and,
//! through a session, sends in `setBreakpoints`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A breakpoint on a line of a file.
///
/// `condition`, `hit_condition` and `log_message` are carried and persisted
/// as they are; nothing edits or sends them yet (card D17).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Breakpoint {
    /// The file. A store keeps it absolute; its state file may hold it
    /// relative to the project's root.
    pub path: PathBuf,
    /// The line, from 1, as DAP's `SourceBreakpoint.line` counts with
    /// `linesStartAt1`.
    pub line: u32,
    /// Whether it's sent: a disabled one is kept and shown, not sent.
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
}

/// What a breakpoint read without `enabled` is.
fn enabled() -> bool {
    true
}

impl Breakpoint {
    /// An enabled breakpoint on `line` (from 1) of `path`, with no
    /// condition, hit condition or log message.
    pub fn new(path: impl Into<PathBuf>, line: u32) -> Self {
        Breakpoint {
            path: path.into(),
            line,
            enabled: true,
            condition: None,
            hit_condition: None,
            log_message: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_left_out_read_as_an_enabled_plain_breakpoint() {
        let read: Breakpoint = serde_json::from_str(r#"{"path": "src/a.rs", "line": 3}"#).unwrap();
        assert_eq!(read, Breakpoint::new("src/a.rs", 3));
    }

    #[test]
    fn the_carried_fields_round_trip() {
        let breakpoint = Breakpoint {
            enabled: false,
            condition: Some("x > 1".into()),
            hit_condition: Some("3".into()),
            log_message: Some("x is {x}".into()),
            ..Breakpoint::new("a.py", 7)
        };
        let json = serde_json::to_value(&breakpoint).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "path": "a.py",
                "line": 7,
                "enabled": false,
                "condition": "x > 1",
                "hitCondition": "3",
                "logMessage": "x is {x}",
            })
        );
        assert_eq!(
            serde_json::from_value::<Breakpoint>(json).unwrap(),
            breakpoint
        );
    }
}
