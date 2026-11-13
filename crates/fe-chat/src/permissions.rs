// SPDX-License-Identifier: Apache-2.0

//! The agent permission model (decision record
//! `docs/decisions/50t8wt9we2-ai-agents.md` § 7): classes, levels, command
//! patterns, shell-syntax detection and per-chat session memory.
//!
//! [`Policy::decide`] gives a [`Level`] for a [`Request`]. The most
//! restrictive applicable rule wins: deny, then ask, then allow. Allow
//! patterns and remembered answers only turn an `ask` into an `allow`, and
//! only for a bare program name that resolves (API chat) or a bare first
//! word without shell syntax (Claude Agent).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ChatId;

/// What a request is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    Read,
    Search,
    ProposeEdit,
    RunCommand,
    Fetch,
    Other,
}

impl Class {
    /// Every class, in the settings' order.
    pub const ALL: [Class; 6] = [
        Class::Read,
        Class::Search,
        Class::ProposeEdit,
        Class::RunCommand,
        Class::Fetch,
        Class::Other,
    ];
}

/// What may happen. Ordered by restriction: `Allow < Ask < Deny`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Allow,
    Ask,
    Deny,
}

/// A command to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// The API chat's `run_command`: no shell, so argv is exact.
    /// `resolved` is the real path the program resolved to on the login
    /// `PATH`, or `None` if it did not resolve (or was given with a path).
    Argv {
        argv: Vec<String>,
        resolved: Option<PathBuf>,
    },
    /// Claude Agent's command string, resolved by Claude Code's shell.
    Shell(String),
}

/// What a request targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A path, a search pattern, a URL or a tool title: no pattern applies.
    Plain(String),
    Command(Command),
    /// Claude Code's `SandboxNetworkAccess` for a host. Never class `other`.
    SandboxNetworkAccess {
        host: String,
    },
}

/// One thing an agent wants to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub chat: ChatId,
    pub class: Class,
    pub target: Target,
}

impl Request {
    pub fn plain(chat: ChatId, class: Class, target: impl Into<String>) -> Request {
        Request {
            chat,
            class,
            target: Target::Plain(target.into()),
        }
    }

    /// An API `run_command`, with the program's resolved real path.
    pub fn argv(chat: ChatId, argv: Vec<String>, resolved: Option<PathBuf>) -> Request {
        Request {
            chat,
            class: Class::RunCommand,
            target: Target::Command(Command::Argv { argv, resolved }),
        }
    }

    /// A Claude Agent `Bash` command string.
    pub fn shell(chat: ChatId, command: impl Into<String>) -> Request {
        Request {
            chat,
            class: Class::RunCommand,
            target: Target::Command(Command::Shell(command.into())),
        }
    }

    pub fn network(chat: ChatId, host: impl Into<String>) -> Request {
        Request {
            chat,
            class: Class::Other,
            target: Target::SandboxNetworkAccess { host: host.into() },
        }
    }
}

/// Whether a command string uses shell syntax: `;`, `&&`, `||`, `|`,
/// backticks, `$(` and other expansion, redirections, subshells, newlines.
pub fn has_shell_syntax(command: &str) -> bool {
    command.chars().any(|c| {
        matches!(
            c,
            ';' | '&'
                | '|'
                | '`'
                | '$'
                | '<'
                | '>'
                | '('
                | ')'
                | '\n'
                | '\r'
                | '\''
                | '"'
                | '\\'
                | '*'
                | '?'
                | '['
                | ']'
                | '{'
                | '}'
                | '~'
                | '!'
                | '#'
        )
    })
}

/// Whether a host is a loopback name or address.
pub fn is_loopback_host(host: &str) -> bool {
    let host = host.trim().trim_matches(['[', ']']).to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let v6_mapped = match ip {
            std::net::IpAddr::V6(v6) => v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()),
            std::net::IpAddr::V4(_) => false,
        };
        return ip.is_loopback() || ip.is_unspecified() || v6_mapped;
    }
    host == "localhost"
        || host.ends_with(".localhost")
        || host.starts_with("127.")
        || host == "0.0.0.0"
        || host == "*"
}

fn file_name(word: &str) -> &str {
    Path::new(word)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(word)
}

/// A command pattern: words matched against argv. A trailing `*` word
/// matches anything after the words before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    words: Vec<String>,
    /// A lone `*`: matches every command.
    any: bool,
}

impl Pattern {
    pub fn new(text: &str) -> Pattern {
        let mut words: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
        let any = words.len() == 1 && words[0] == "*";
        if words.last().is_some_and(|word| word == "*") {
            words.pop();
        }
        if text.trim().is_empty() {
            // An empty pattern matches nothing.
            words = vec![String::new()];
        }
        Pattern { words, any }
    }

    /// Whether argv starts with this pattern's words, the program compared
    /// by file name.
    fn matches_argv(&self, argv: &[&str]) -> bool {
        if self.any {
            return true;
        }
        if self.words.len() > argv.len() {
            return false;
        }
        self.words.iter().zip(argv).enumerate().all(
            |(i, (w, a))| {
                if i == 0 { w == file_name(a) } else { w == a }
            },
        )
    }

    fn matches_first_word(&self, first: &str) -> bool {
        self.any
            || self
                .words
                .first()
                .is_some_and(|w| !w.is_empty() && w == file_name(first))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Remembered {
    program: String,
    first_arg: Option<String>,
    /// The resolved real path the answer was given for; `None` for a shell
    /// command, whose program Tenmoku cannot resolve.
    resolved: Option<PathBuf>,
}

#[derive(Default)]
struct Memory {
    classes: HashSet<Class>,
    commands: HashSet<Remembered>,
}

/// The policy: class levels, command patterns and per-chat memory.
pub struct Policy {
    levels: HashMap<Class, Level>,
    project_levels: HashMap<Class, Level>,
    allow: Vec<Pattern>,
    ask: Vec<Pattern>,
    deny: Vec<Pattern>,
    memory: HashMap<ChatId, Memory>,
}

impl Default for Policy {
    /// The defaults of `[agents.permissions]`.
    fn default() -> Policy {
        let mut levels = HashMap::new();
        for class in Class::ALL {
            let level = match class {
                Class::Read | Class::Search | Class::ProposeEdit => Level::Allow,
                _ => Level::Ask,
            };
            levels.insert(class, level);
        }
        Policy {
            levels,
            project_levels: HashMap::new(),
            allow: Vec::new(),
            ask: Vec::new(),
            deny: Vec::new(),
            memory: HashMap::new(),
        }
    }
}

impl Policy {
    pub fn new() -> Policy {
        Policy::default()
    }

    pub fn set_level(&mut self, class: Class, level: Level) -> &mut Policy {
        self.levels.insert(class, level);
        self
    }

    /// A project's level for a class: it can only tighten.
    pub fn set_project_level(&mut self, class: Class, level: Level) -> &mut Policy {
        self.project_levels.insert(class, level);
        self
    }

    pub fn allow_commands<S: AsRef<str>>(&mut self, patterns: &[S]) -> &mut Policy {
        self.allow = patterns.iter().map(|p| Pattern::new(p.as_ref())).collect();
        self
    }

    pub fn ask_commands<S: AsRef<str>>(&mut self, patterns: &[S]) -> &mut Policy {
        self.ask = patterns.iter().map(|p| Pattern::new(p.as_ref())).collect();
        self
    }

    pub fn deny_commands<S: AsRef<str>>(&mut self, patterns: &[S]) -> &mut Policy {
        self.deny = patterns.iter().map(|p| Pattern::new(p.as_ref())).collect();
        self
    }

    /// The class's level: the stricter of the user's and the project's.
    pub fn class_level(&self, class: Class) -> Level {
        let user = self.levels.get(&class).copied().unwrap_or(Level::Ask);
        match self.project_levels.get(&class) {
            Some(project) => user.max(*project),
            None => user,
        }
    }

    /// Decides a request. Never `Allow` for something that must prompt.
    pub fn decide(&self, request: &Request) -> Level {
        match &request.target {
            Target::SandboxNetworkAccess { host } => {
                if is_loopback_host(host) {
                    Level::Deny
                } else {
                    Level::Ask
                }
            }
            Target::Plain(_) => {
                let level = self.class_level(request.class);
                if level == Level::Ask && self.class_remembered(request) {
                    Level::Allow
                } else {
                    level
                }
            }
            Target::Command(command) => self.decide_command(request, command),
        }
    }

    fn class_remembered(&self, request: &Request) -> bool {
        request.class != Class::RunCommand
            && self
                .memory
                .get(&request.chat)
                .is_some_and(|m| m.classes.contains(&request.class))
    }

    fn decide_command(&self, request: &Request, command: &Command) -> Level {
        let class_level = self.class_level(Class::RunCommand);
        // A request's own class is run_command whatever the caller set.
        let _ = request.class;
        let (words, shaped, resolved): (Vec<&str>, bool, Option<&Path>) = match command {
            Command::Argv { argv, resolved } => (
                argv.iter().map(String::as_str).collect(),
                false,
                resolved.as_deref(),
            ),
            Command::Shell(text) => (
                text.split_whitespace().collect(),
                has_shell_syntax(text),
                None,
            ),
        };
        let Some(&first) = words.first() else {
            return class_level.max(Level::Ask);
        };
        // Deny and ask patterns: for shell syntax only the first word is
        // known, so a pattern matches on its first word alone.
        let hits = |patterns: &[Pattern]| {
            patterns.iter().any(|p| {
                if shaped {
                    p.matches_first_word(first)
                } else {
                    p.matches_argv(&words)
                }
            })
        };
        if class_level == Level::Deny || hits(&self.deny) {
            return Level::Deny;
        }
        // Shell syntax, a path-given program or an unresolved one must
        // prompt, whatever the class level; and so must an ask pattern.
        if hits(&self.ask) || !allow_eligible(command, first, shaped, resolved) {
            return Level::Ask;
        }
        if class_level == Level::Allow {
            return Level::Allow;
        }
        let by_pattern = self.allow.iter().any(|p| p.matches_argv(&words));
        let by_memory = self.memory.get(&request.chat).is_some_and(|m| {
            m.commands.contains(&Remembered {
                program: first.to_owned(),
                first_arg: words.get(1).map(|w| (*w).to_owned()),
                resolved: resolved.map(Path::to_path_buf),
            })
        });
        if by_pattern || by_memory {
            Level::Allow
        } else {
            Level::Ask
        }
    }

    /// "Allow for this chat". Remembers the class for non-command classes,
    /// or the command's program plus first argument (the program bound to
    /// its resolved path for the API chat). Returns whether anything was
    /// remembered: a request that must prompt, or that never matches an
    /// allow pattern, is not. It never overrides a deny.
    pub fn remember(&mut self, request: &Request) -> bool {
        if self.decide(request) == Level::Deny {
            return false;
        }
        match &request.target {
            Target::SandboxNetworkAccess { .. } => false,
            Target::Plain(_) => {
                self.memory
                    .entry(request.chat)
                    .or_default()
                    .classes
                    .insert(request.class);
                true
            }
            Target::Command(command) => {
                let (words, shaped, resolved): (Vec<&str>, bool, Option<&Path>) = match command {
                    Command::Argv { argv, resolved } => (
                        argv.iter().map(String::as_str).collect(),
                        false,
                        resolved.as_deref(),
                    ),
                    Command::Shell(text) => (
                        text.split_whitespace().collect(),
                        has_shell_syntax(text),
                        None,
                    ),
                };
                let Some(&first) = words.first() else {
                    return false;
                };
                if !allow_eligible(command, first, shaped, resolved) {
                    return false;
                }
                let entry = Remembered {
                    program: first.to_owned(),
                    first_arg: words.get(1).map(|w| (*w).to_owned()),
                    resolved: resolved.map(Path::to_path_buf),
                };
                self.memory
                    .entry(request.chat)
                    .or_default()
                    .commands
                    .insert(entry);
                true
            }
        }
    }

    /// Forgets a chat's memory (the chat closed).
    pub fn forget_chat(&mut self, chat: ChatId) {
        self.memory.remove(&chat);
    }
}

/// Whether a command may match an allow pattern or memory at all: a bare
/// program name, no shell syntax, and for argv a resolved path.
fn allow_eligible(command: &Command, first: &str, shaped: bool, resolved: Option<&Path>) -> bool {
    if shaped || first.contains('/') {
        return false;
    }
    match command {
        Command::Argv { .. } => resolved.is_some(),
        Command::Shell(_) => true,
    }
}
