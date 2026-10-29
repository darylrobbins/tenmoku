//! A commit's web page from a git remote URL, through a list of providers so
//! other hosts can join GitHub later. Pure string parsing; nothing touches the
//! network.

use gix::remote::Direction;

use crate::Repo;

/// A remote URL split into its host and repository path, whichever form it
/// was written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRemote {
    /// The host name alone: no user, port or scheme.
    pub host: String,
    /// The repository path, without surrounding `/` or a trailing `.git`.
    pub path: String,
}

/// A hosting service that can turn a remote it recognises into a commit page.
pub trait RemoteLinkProvider: Send + Sync {
    /// Whether this provider hosts `remote`.
    fn matches(&self, remote: &ParsedRemote) -> bool;
    /// The web page of commit `id` in `remote`, which [`matches`](Self::matches).
    fn commit_url(&self, remote: &ParsedRemote, id: &str) -> String;
}

/// github.com, for `owner/repo` paths.
pub struct GitHub;

impl RemoteLinkProvider for GitHub {
    fn matches(&self, remote: &ParsedRemote) -> bool {
        remote.host.eq_ignore_ascii_case("github.com")
            && remote.path.split('/').count() == 2
            && remote.path.split('/').all(|part| !part.is_empty())
    }

    fn commit_url(&self, remote: &ParsedRemote, id: &str) -> String {
        format!("https://github.com/{}/commit/{id}", remote.path)
    }
}

/// The providers [`Repo::commit_web_url`] consults: GitHub only.
pub static DEFAULT_PROVIDERS: &[&dyn RemoteLinkProvider] = &[&GitHub];

/// Split a remote URL: `https://`, `http://`, `ssh://`, `git://` or scp-style
/// (`user@host:path`). `None` for a local path, an empty string or anything
/// without a host and path.
pub fn parse_remote(url: &str) -> Option<ParsedRemote> {
    let url = url.trim();
    let (authority, path) = if let Some((scheme, rest)) = url.split_once("://") {
        if !matches!(scheme, "https" | "http" | "ssh" | "git") {
            return None;
        }
        rest.split_once('/')?
    } else {
        let (authority, path) = url.split_once(':')?;
        if authority.contains('/') {
            return None;
        }
        (authority, path)
    };
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if url.contains("://") {
        host.split_once(':').map_or(host, |(host, _)| host)
    } else {
        host
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(ParsedRemote {
        host: host.to_owned(),
        path: path.to_owned(),
    })
}

/// The commit page of `id` for `url`, from the first of `providers` that
/// matches it.
pub fn commit_url_with(
    providers: &[&dyn RemoteLinkProvider],
    url: &str,
    id: &str,
) -> Option<String> {
    let remote = parse_remote(url)?;
    providers
        .iter()
        .find(|provider| provider.matches(&remote))
        .map(|provider| provider.commit_url(&remote, id))
}

/// The commit page of `id` for `url` at a registered provider.
pub fn commit_url(url: &str, id: &str) -> Option<String> {
    commit_url_with(DEFAULT_PROVIDERS, url, id)
}

impl Repo {
    /// The web page of commit `id` at the host of `origin`, or of the only
    /// remote when there is no `origin`. `None` with several remotes and no
    /// `origin`, with no remote, or when no provider knows the host.
    pub fn commit_web_url(&self, id: &str) -> Option<String> {
        // Reopen: the held repository keeps the config it was discovered with,
        // and remotes change under it.
        let repo = gix::open(self.repo.git_dir()).ok()?;
        let names: Vec<String> = repo
            .remote_names()
            .iter()
            .map(|name| name.to_string())
            .collect();
        let name = if names.iter().any(|name| name == "origin") {
            "origin"
        } else if let [only] = names.as_slice() {
            only
        } else {
            return None;
        };
        let remote = repo.find_remote(name).ok()?;
        let url = remote.url(Direction::Fetch)?.to_bstring();
        commit_url(&url.to_string(), id)
    }
}
