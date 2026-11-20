//! The `[remote]` settings (`docs/decisions/j8wvgszah0-remote-development.md`
//! § 3 the grace period, § 4 deploy and version policy, § 8 port forwarding;
//! `docs/designs/remote-development.md` § 9): where a development server
//! binary is, which ports every connect forwards, what to do about a port
//! that appears, and how long a host keeps a dropped session.
//!
//! All four are the user's: a project file can't set them, because a cloned
//! repository shouldn't choose the program a connect uploads and runs, or how
//! long a host holds a session open. `settings::drop_remote` is what drops
//! them from a project layer.
//!
//! Nothing reads these yet. Their consumers are the connect card
//! (`dev_server`), the port cards (`ports`, `auto_forward`) and reconnect
//! (`reconnect_grace`).

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Settings for working on a remote host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RemoteSettings {
    /// A `tenmoku-server` built from this checkout, for a development build
    /// of Tenmoku, which has no server release to download. It is uploaded
    /// and installed like a release asset, and it must report exactly this
    /// build's version. Unset means the release download.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_server: Option<PathBuf>,
    /// Ports forwarded on every connect, each `3000` for the same port at
    /// both ends or `"5432:15432"` for the host's port and the local one it
    /// is offered on. The client listens on 127.0.0.1 only.
    pub ports: Vec<PortForward>,
    /// What happens when a program on the host starts listening on a port:
    /// a notification offering to forward it, forwarding it straight away,
    /// or nothing. A port a run or task configuration declares is forwarded
    /// whatever this says.
    pub auto_forward: AutoForward,
    /// How many minutes a host keeps a session whose link dropped, with its
    /// terminals, language servers and debuggers still running, before it
    /// tears them down.
    #[schemars(range(min = 1, max = 60))]
    pub reconnect_grace: u32,
}

impl Default for RemoteSettings {
    fn default() -> Self {
        Self {
            dev_server: None,
            ports: Vec::new(),
            auto_forward: AutoForward::default(),
            // Decision § 3: "a session whose link drops stays alive for 10
            // minutes (`remote.reconnect_grace`, 1-60)".
            reconnect_grace: 10,
        }
    }
}

/// One forwarded port: `3000`, or `"5432:15432"` for the host's port and the
/// local one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PortForward {
    /// The same port on the host and locally.
    Same(#[schemars(range(min = 1))] u16),
    /// `"<host>:<local>"`: the host's port, then the local one it is offered
    /// on.
    Mapped(
        #[schemars(regex(
            pattern = "^(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]):(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5])$"
        ))]
        String,
    ),
}

impl PortForward {
    /// The host's port and the local port it is offered on. `None` for a
    /// mapped form the schema didn't check, which is only reachable through
    /// [`PortForward::Mapped`] built in code.
    pub fn resolve(&self) -> Option<(u16, u16)> {
        match self {
            Self::Same(port) => Some((*port, *port)),
            Self::Mapped(text) => {
                let (host, local) = text.split_once(':')?;
                Some((host.parse().ok()?, local.parse().ok()?))
            }
        }
    }
}

/// What to do with a port a program on the host starts listening on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoForward {
    /// A notification offering to forward it.
    #[default]
    Notify,
    /// Forward it as it appears.
    Forward,
    /// Leave it alone.
    Off,
}
