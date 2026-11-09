//! Where a request goes, checked against the grants before anything is
//! looked up or opened.

use kiln_l10n::tr;
use std::collections::BTreeSet;

use http::Uri;
use kiln_command::Grants;

use crate::NetError;

/// The hosts a client may reach: the grants' `net:` hosts.
#[derive(Clone, Debug)]
pub(crate) struct Allowlist {
    hosts: BTreeSet<String>,
    /// `net:*`: every host.
    any: bool,
}

impl Allowlist {
    pub(crate) fn new(grants: &Grants) -> Allowlist {
        let net = grants.net_hosts();
        Allowlist {
            hosts: net
                .hosts
                .iter()
                .map(|host| host.to_ascii_lowercase())
                .collect(),
            any: net.any,
        }
    }

    fn allows(&self, host: &str) -> bool {
        self.any || self.hosts.contains(host)
    }
}

/// A URL that passed the checks.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub(crate) tls: bool,
    /// Lower-cased; an IPv6 address without its brackets.
    pub(crate) host: String,
    pub(crate) port: u16,
    /// The `Host` header: the URL's authority.
    pub(crate) authority: String,
    /// The request target: the path and query, `/` when the URL has none.
    pub(crate) path: String,
}

impl Target {
    /// Parses `url` and checks its scheme, then its host against `allowlist`.
    pub(crate) fn parse(url: &str, allowlist: &Allowlist) -> Result<Target, NetError> {
        let uri: Uri = url.parse().map_err(|error: http::uri::InvalidUri| {
            NetError::Http(
                tr!(
                    "net-invalid-url",
                    url = format!("{url:?}"),
                    error = error.to_string()
                )
                .to_string(),
            )
        })?;
        let authority = uri.authority().ok_or_else(|| {
            NetError::Http(tr!("net-url-no-host", url = format!("{url:?}")).to_string())
        })?;
        if authority.as_str().contains('@') {
            return Err(NetError::Http(
                tr!("net-url-user-name", url = format!("{url:?}")).to_string(),
            ));
        }
        // http::Uri takes `host:abc` and `host:65536`, giving no port for
        // them: they are refused rather than sent to the default port.
        let port = match &authority.as_str()[authority.host().len()..] {
            "" => None,
            port => Some(
                port.strip_prefix(':')
                    .and_then(|port| port.parse::<u16>().ok())
                    .filter(|&port| port != 0)
                    .ok_or_else(|| {
                        NetError::Http(
                            tr!("net-url-invalid-port", url = format!("{url:?}")).to_string(),
                        )
                    })?,
            ),
        };
        let bracketed = authority.host().to_ascii_lowercase();
        let host = bracketed
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(&bracketed)
            .to_owned();
        let tls = match uri.scheme_str() {
            Some(scheme) if scheme.eq_ignore_ascii_case("https") => true,
            Some(scheme) if scheme.eq_ignore_ascii_case("http") && is_loopback(&host) => false,
            _ => return Err(NetError::Scheme),
        };
        if !allowlist.allows(&bracketed) {
            return Err(NetError::HostNotAllowed(bracketed));
        }
        Ok(Target {
            tls,
            port: port.unwrap_or(if tls { 443 } else { 80 }),
            authority: authority.as_str().to_owned(),
            path: uri
                .path_and_query()
                .map_or_else(|| "/".to_owned(), |path| path.as_str().to_owned()),
            host,
        })
    }
}

/// The loopback addresses plain `http` may reach, for tests. Names (such
/// as `localhost`) aren't: they resolve through DNS.
fn is_loopback(host: &str) -> bool {
    host == "127.0.0.1" || host == "::1"
}
