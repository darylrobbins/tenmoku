//! The Keyboard Shortcuts editor's model: every command with its bindings
//! from the effective keymap, as VS Code's editor lists them (keys, when,
//! source), searched by command or by keystroke, with the bindings that
//! clash marked. Headless: the host passes in the commands, as it knows
//! them from its registry, and draws the rows.

use std::collections::HashMap;

use kiln_core::Os;

use crate::{
    Binding, KeySequence, Keymap, KeymapIssue, KeymapLayer, Location, macos_label, pc_label,
};

/// A command as the editor lists it, from the host's registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandInfo {
    pub id: String,
    pub title: String,
    /// Where it applies, as its predicate prints; `None` is everywhere.
    pub context: Option<String>,
}

/// Which keymap a binding comes from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ShortcutSource {
    /// The built-in keymap, layer 0.
    Default,
    /// Any later layer: the user's keymap.
    User,
}

/// Why a binding doesn't simply run its command when its keys are pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShortcutIssue {
    /// `command`'s binding has the same keys, in a context where both hold:
    /// there this one runs if it `wins`, else `command` does.
    Conflict { command: String, wins: bool },
    /// A later binding with the same keys and context replaces it, for
    /// `by`, or an unbind entry (`command = ""`) removes it, `by` `None`:
    /// it never runs.
    Hidden { by: Option<String> },
    /// Its keys start `keys`, `by`'s longer binding, so it runs only when
    /// the chord times out.
    Shadowed { by: String, keys: KeySequence },
}

/// One key sequence bound to a row's command.
#[derive(Clone, Debug, PartialEq)]
pub struct ShortcutBinding {
    pub keys: KeySequence,
    /// `keys` as the platform prints them, such as `Ctrl+K Ctrl+0` or `⌘K ⌘0`.
    pub label: String,
    /// The context it's bound in, as its predicate prints; `None` is
    /// everywhere.
    pub when: Option<String>,
    /// Its args as JSON, or `None` if it has none.
    pub args: Option<String>,
    pub source: ShortcutSource,
    pub location: Location,
    pub issues: Vec<ShortcutIssue>,
}

/// A command and every binding of it, in the order they were read. A
/// command bound to two key sequences has two bindings here, where VS Code
/// lists two rows of it.
#[derive(Clone, Debug, PartialEq)]
pub struct ShortcutRow {
    pub command: String,
    /// The command's title, or its ID if the host doesn't know it.
    pub title: String,
    /// Where the command applies, as [`CommandInfo::context`].
    pub context: Option<String>,
    /// Whether the host passed the command in. A binding for a command it
    /// doesn't know still gets a row, titled with the ID.
    pub known: bool,
    pub bindings: Vec<ShortcutBinding>,
}

/// Every command with its bindings, in title order: the Keyboard Shortcuts
/// editor's rows.
#[derive(Clone, Debug)]
pub struct ShortcutsModel {
    rows: Vec<ShortcutRow>,
    /// The keymap's bindings, to find those [`KeymapIssue`]s name.
    bindings: Vec<Binding>,
    /// Where each of `bindings` is listed: its row and its index there.
    places: Vec<(usize, usize)>,
    os: Os,
}

impl ShortcutsModel {
    /// One row per command in `keymap` or `commands`, with the bindings
    /// marked by [`Keymap::report`]. Bindings an unbind entry removed are
    /// gone from a keymap: [`ShortcutsModel::from_layers`] lists them too.
    /// Keys are labelled as on `os`.
    pub fn new(keymap: &Keymap, commands: &[CommandInfo], os: Os) -> Self {
        let listed: Vec<(&Binding, Option<usize>)> = keymap
            .bindings()
            .iter()
            .enumerate()
            .map(|(ix, binding)| (binding, Some(ix)))
            .collect();
        Self::build(keymap, &listed, commands, os)
    }

    /// [`ShortcutsModel::new`] for the keymap layered from `layers`, the
    /// defaults first, also listing the bindings an unbind entry removed,
    /// marked [`ShortcutIssue::Hidden`].
    pub fn from_layers(layers: &[KeymapLayer], commands: &[CommandInfo], os: Os) -> Self {
        let keymap = Keymap::from_layers(layers);
        // Every binding read, as if no entry unbound any.
        let without_unbinds: Vec<KeymapLayer> = layers
            .iter()
            .map(|layer| KeymapLayer {
                path: layer.path.clone(),
                entries: layer
                    .entries
                    .iter()
                    .filter(|entry| entry.command.is_some())
                    .cloned()
                    .collect(),
            })
            .collect();
        let read = Keymap::from_layers(&without_unbinds);
        // `read` keeps `keymap`'s bindings in their order, with the
        // removed ones among them.
        let listed: Vec<(&Binding, Option<usize>)> = read
            .bindings()
            .iter()
            .map(|binding| {
                let live = keymap.bindings().iter().position(|kept| kept == binding);
                (binding, live)
            })
            .collect();
        Self::build(&keymap, &listed, commands, os)
    }

    /// `listed` is each binding to list, in the order read, with its index
    /// in `keymap` or `None` if an unbind entry removed it.
    fn build(
        keymap: &Keymap,
        listed: &[(&Binding, Option<usize>)],
        commands: &[CommandInfo],
        os: Os,
    ) -> Self {
        let mut rows = Vec::new();
        let mut row_of: HashMap<String, usize> = HashMap::new();
        for command in commands {
            row_of.entry(command.id.clone()).or_insert_with(|| {
                rows.push(ShortcutRow {
                    command: command.id.clone(),
                    title: command.title.clone(),
                    context: command.context.clone(),
                    known: true,
                    bindings: Vec::new(),
                });
                rows.len() - 1
            });
        }
        let mut places = vec![(0, 0); keymap.bindings().len()];
        for &(binding, live) in listed {
            let id = binding.command.as_str();
            let row = *row_of.entry(id.to_owned()).or_insert_with(|| {
                rows.push(ShortcutRow {
                    command: id.to_owned(),
                    title: id.to_owned(),
                    context: None,
                    known: false,
                    bindings: Vec::new(),
                });
                rows.len() - 1
            });
            let issues = match live {
                Some(ix) => {
                    places[ix] = (row, rows[row].bindings.len());
                    Vec::new()
                }
                None => vec![ShortcutIssue::Hidden { by: None }],
            };
            rows[row].bindings.push(ShortcutBinding {
                keys: binding.keys.clone(),
                label: label(&binding.keys, os),
                when: binding.context.as_ref().map(ToString::to_string),
                args: (!binding.args.is_null()).then(|| binding.args.to_string()),
                source: match binding.layer {
                    0 => ShortcutSource::Default,
                    _ => ShortcutSource::User,
                },
                location: binding.location.clone(),
                issues,
            });
        }
        // Title order, then by ID for equal titles.
        let mut order: Vec<usize> = (0..rows.len()).collect();
        order.sort_by_cached_key(|&ix| (rows[ix].title.to_lowercase(), rows[ix].command.clone()));
        let mut new_ix = vec![0; rows.len()];
        for (new, &old) in order.iter().enumerate() {
            new_ix[old] = new;
        }
        let mut slots: Vec<Option<ShortcutRow>> = rows.into_iter().map(Some).collect();
        let rows = order
            .iter()
            .map(|&old| slots[old].take().expect("each row is taken once"))
            .collect();
        for (row, _) in &mut places {
            *row = new_ix[*row];
        }
        let mut model = Self {
            rows,
            bindings: keymap.bindings().to_vec(),
            places,
            os,
        };
        model.mark(&keymap.report());
        model
    }

    /// The model with the bindings marked by `issues` instead of by
    /// [`Keymap::report`]'s: for a host that has more contexts than
    /// `[editor]`, from [`Keymap::report_with`] on the same keymap.
    pub fn with_issues(mut self, issues: &[KeymapIssue]) -> Self {
        for &(row, ix) in &self.places {
            self.rows[row].bindings[ix].issues.clear();
        }
        self.mark(issues);
        self
    }

    fn mark(&mut self, issues: &[KeymapIssue]) {
        for issue in issues {
            match issue {
                KeymapIssue::Overridden { winner, loser } => {
                    let by = Some(winner.command.to_string());
                    self.push(loser, ShortcutIssue::Hidden { by });
                    if !same_action(winner, loser) {
                        let command = loser.command.to_string();
                        self.push(
                            winner,
                            ShortcutIssue::Conflict {
                                command,
                                wins: true,
                            },
                        );
                    }
                }
                KeymapIssue::Shadowed { winner, loser, .. } => {
                    if !same_action(winner, loser) {
                        let command = loser.command.to_string();
                        self.push(
                            winner,
                            ShortcutIssue::Conflict {
                                command,
                                wins: true,
                            },
                        );
                        let command = winner.command.to_string();
                        self.push(
                            loser,
                            ShortcutIssue::Conflict {
                                command,
                                wins: false,
                            },
                        );
                    }
                }
                KeymapIssue::PrefixDelay { prefix, longer, .. } => {
                    let issue = ShortcutIssue::Shadowed {
                        by: longer.command.to_string(),
                        keys: longer.keys.clone(),
                    };
                    self.push(prefix, issue);
                }
            }
        }
    }

    /// Adds `issue` to `binding`'s, once. A binding not in the keymap, as
    /// from an issue of another, is skipped.
    fn push(&mut self, binding: &Binding, issue: ShortcutIssue) {
        let Some(ix) = self.bindings.iter().position(|ours| ours == binding) else {
            return;
        };
        let (row, ix) = self.places[ix];
        let issues = &mut self.rows[row].bindings[ix].issues;
        if !issues.contains(&issue) {
            issues.push(issue);
        }
    }

    /// Every row, in title order.
    pub fn rows(&self) -> &[ShortcutRow] {
        &self.rows
    }

    /// Each marked binding with its row and each of its issues, in row
    /// order: conflicts, hidden bindings and shadowed chord prefixes.
    pub fn conflicts(&self) -> Vec<(&ShortcutRow, &ShortcutBinding, &ShortcutIssue)> {
        self.rows
            .iter()
            .flat_map(|row| {
                row.bindings.iter().flat_map(move |binding| {
                    binding
                        .issues
                        .iter()
                        .map(move |issue| (row, binding, issue))
                })
            })
            .collect()
    }

    /// The rows matching `query`, each with only its matching bindings, as
    /// VS Code's editor shows a row per binding.
    ///
    /// - Plain words match the command's title or ID, or the binding's
    ///   when-text, ignoring case; every word must match. If the words also
    ///   parse as keys, such as `ctrl-k`, rows whose bindings start with
    ///   them, or equal them if `exact`, match too.
    /// - `@keybinding:ctrl-k`, to the end of the query, or keys in quotes,
    ///   `"ctrl-k ctrl-0"`, match only by keys: bindings that start with
    ///   them, or equal them if `exact` or quoted. Keys that don't parse are
    ///   matched as words.
    /// - `@source:user` or `@source:default` keeps only bindings from
    ///   there; `@command:<id>` only that command. Either may be anywhere
    ///   in the query, after `@keybinding:` too.
    ///
    /// Keys are written as in a keymap file, as on this model's platform,
    /// or as the labels print them, as in `Ctrl+Shift+K`, `Super+K` or
    /// `Esc`.
    pub fn search(&self, query: &str, exact: bool) -> Vec<ShortcutRow> {
        let query = Query::parse(query, exact, self.os);
        self.rows
            .iter()
            .filter_map(|row| {
                if query.command.as_ref().is_some_and(|id| *id != row.command) {
                    return None;
                }
                if row.bindings.is_empty() {
                    let unbound = query.source.is_none()
                        && query.keys.is_none()
                        && query.words_match(row, None);
                    return unbound.then(|| row.clone());
                }
                let bindings: Vec<ShortcutBinding> = row
                    .bindings
                    .iter()
                    .filter(|binding| query.matches(row, binding))
                    .cloned()
                    .collect();
                (!bindings.is_empty()).then(|| ShortcutRow {
                    bindings,
                    ..row.clone()
                })
            })
            .collect()
    }
}

/// Whether two bindings do the same thing: one command with the same args.
fn same_action(a: &Binding, b: &Binding) -> bool {
    a.command == b.command && a.args == b.args
}

/// `keys` as `os` prints them: [`crate::shortcut_label`] for that platform.
fn label(keys: &KeySequence, os: Os) -> String {
    match os {
        Os::MacOs => macos_label(keys),
        _ => pc_label(keys),
    }
}

/// A parsed [`ShortcutsModel::search`] query.
struct Query {
    /// Lowercase.
    words: Vec<String>,
    /// Keys the words also parse as, matched exactly if `exact`.
    words_as_keys: Option<KeySequence>,
    exact: bool,
    /// Keys asked for explicitly, and whether exactly.
    keys: Option<(KeySequence, bool)>,
    source: Option<ShortcutSource>,
    command: Option<String>,
}

impl Query {
    fn parse(query: &str, exact: bool, os: Os) -> Self {
        let mut parsed = Query {
            words: Vec::new(),
            words_as_keys: None,
            exact,
            keys: None,
            source: None,
            command: None,
        };
        // The filters, wherever they are.
        let mut unfiltered = Vec::new();
        for word in query.split_whitespace() {
            if let Some(source) = word.strip_prefix("@source:") {
                match source.to_ascii_lowercase().as_str() {
                    "user" => parsed.source = Some(ShortcutSource::User),
                    "default" => parsed.source = Some(ShortcutSource::Default),
                    _ => unfiltered.push(word),
                }
            } else if let Some(id) = word.strip_prefix("@command:") {
                parsed.command = Some(id.to_owned());
            } else {
                unfiltered.push(word);
            }
        }
        let query = unfiltered.join(" ");
        // Explicit keys: to the end after `@keybinding:`, or in quotes.
        let (rest, keys_text) = if let Some(at) = query.find("@keybinding:") {
            let keys = query[at + "@keybinding:".len()..].trim();
            (query[..at].to_owned(), Some((keys, exact)))
        } else if let Some(open) = query.find('"') {
            let after = &query[open + 1..];
            let (keys, after) = after.split_once('"').unwrap_or((after, ""));
            let rest = format!("{} {after}", &query[..open]);
            (rest, Some((keys.trim(), true)))
        } else {
            (query.to_owned(), None)
        };
        let mut words: Vec<&str> = rest.split_whitespace().collect();
        match keys_text {
            Some((text, exact)) => match parse_keys(text, os) {
                Some(keys) => parsed.keys = Some((keys, exact)),
                None => words.extend(text.split_whitespace()),
            },
            None if !words.is_empty() => parsed.words_as_keys = parse_keys(&words.join(" "), os),
            None => {}
        }
        parsed.words = words.iter().map(|word| word.to_lowercase()).collect();
        parsed
    }

    fn matches(&self, row: &ShortcutRow, binding: &ShortcutBinding) -> bool {
        if self.source.is_some_and(|source| source != binding.source) {
            return false;
        }
        if let Some((keys, exact)) = &self.keys {
            return keys_match(binding, keys, *exact) && self.words_match(row, Some(binding));
        }
        self.words_match(row, Some(binding))
            || self
                .words_as_keys
                .as_ref()
                .is_some_and(|keys| keys_match(binding, keys, self.exact))
    }

    /// Whether every word is in the command's title or ID or the binding's
    /// when-text.
    fn words_match(&self, row: &ShortcutRow, binding: Option<&ShortcutBinding>) -> bool {
        let title = row.title.to_lowercase();
        let id = row.command.to_lowercase();
        let when = binding
            .and_then(|binding| binding.when.as_deref())
            .map(str::to_lowercase);
        self.words.iter().all(|word| {
            title.contains(word.as_str())
                || id.contains(word.as_str())
                || when
                    .as_ref()
                    .is_some_and(|when| when.contains(word.as_str()))
        })
    }
}

fn keys_match(binding: &ShortcutBinding, keys: &KeySequence, exact: bool) -> bool {
    if exact {
        binding.keys == *keys
    } else {
        binding.keys.keystrokes().starts_with(keys.keystrokes())
    }
}

/// `text` as keys, written as in a keymap file or with `+` between each
/// keystroke's parts, as [`pc_label`] prints them.
fn parse_keys(text: &str, os: Os) -> Option<KeySequence> {
    let written: Vec<String> = text.split_whitespace().map(dashed).collect();
    KeySequence::parse_for(&written.join(" "), os).ok()
}

/// A keystroke written `Ctrl+Shift+K` as `ctrl-shift-k`, with `Super` for
/// `cmd` and `Esc` for `escape`, however its parts are separated.
fn dashed(keystroke: &str) -> String {
    // The modifiers, and the key after the last separator unless that
    // separator is the key, as in `ctrl-+` or `ctrl--`.
    let split = |separator: char| {
        let doubled = [separator, separator];
        if keystroke.len() > 1 && keystroke.ends_with(doubled.as_slice()) {
            let modifiers = &keystroke[..keystroke.len() - 2];
            let key = &keystroke[keystroke.len() - 1..];
            return Some((modifiers, key));
        }
        keystroke
            .rsplit_once(separator)
            .filter(|(modifiers, key)| !modifiers.is_empty() && !key.is_empty())
    };
    let (modifiers, key) = match split('+')
        .map(|parts| (parts, '+'))
        .or_else(|| split('-').map(|parts| (parts, '-')))
    {
        Some(((modifiers, key), separator)) => (modifiers.split(separator).collect(), key),
        None => (Vec::new(), keystroke),
    };
    let mut parts: Vec<&str> = modifiers
        .into_iter()
        .map(|modifier| match modifier.to_ascii_lowercase().as_str() {
            "super" => "cmd",
            _ => modifier,
        })
        .collect();
    parts.push(if key.eq_ignore_ascii_case("esc") {
        "escape"
    } else {
        key
    });
    parts.join("-")
}
