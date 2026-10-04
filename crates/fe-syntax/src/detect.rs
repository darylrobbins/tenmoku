//! Choosing a language from a buffer's content when its path doesn't decide.

use std::collections::VecDeque;
use std::path::Path;

use crate::language::{language_name_for_path, language_names};

/// Interpreter names and the language each one runs.
const INTERPRETERS: &[(&str, &str)] = &[
    ("python", "python"),
    ("sh", "bash"),
    ("bash", "bash"),
    ("zsh", "bash"),
    ("dash", "bash"),
    ("ksh", "bash"),
    ("node", "javascript"),
    ("nodejs", "javascript"),
];

/// The language a `#!` line runs, if its interpreter is known and compiled in.
///
/// Only the first line of `first_line` is read. `env` is looked through to the
/// command it runs, past its options and `NAME=value` assignments, so
/// `#!/usr/bin/env -S node --flag` is JavaScript. A version suffix is ignored:
/// `python3.12` is `python`.
pub fn language_name_for_shebang(first_line: &str) -> Option<&'static str> {
    let line = first_line.lines().next()?.strip_prefix("#!")?.trim_start();
    let (interpreter, args) = line.split_at(line.find(char::is_whitespace).unwrap_or(line.len()));
    let program = match base_name(interpreter) {
        "" => return None,
        "env" => env_command(args)?,
        _ => interpreter.to_owned(),
    };
    let name = base_name(&program);
    let language = INTERPRETERS.iter().find_map(|&(interpreter, language)| {
        let version = name.strip_prefix(interpreter)?;
        is_version_suffix(version).then_some(language)
    })?;
    language_names().find(|&compiled| compiled == language)
}

/// The language for a buffer: the path's if it names one, else the shebang on
/// the first line of `text_start`. Pass the first few hundred bytes, not the
/// whole buffer.
pub fn detect_language_name(path: Option<&Path>, text_start: &str) -> Option<&'static str> {
    path.and_then(language_name_for_path)
        .or_else(|| language_name_for_shebang(text_start))
}

/// What an `env` option takes after it.
#[derive(Clone, Copy)]
enum Takes {
    Nothing,
    /// The rest of its word, or else the next argument.
    Operand,
    /// Only a value attached with `=`; long options only.
    OptionalValue,
    /// An operand that `env` splits into further arguments (`-S`).
    SplitString,
    /// Nothing, and `env` then runs no command: `-0` refuses one, and
    /// `--help` and `--version` exit.
    NoCommand,
}

/// GNU `env`'s long options. Like `getopt_long`, any unambiguous prefix of a
/// name selects it.
const ENV_LONG_OPTIONS: &[(&str, Takes)] = &[
    ("argv0", Takes::Operand),
    ("block-signal", Takes::OptionalValue),
    ("chdir", Takes::Operand),
    ("debug", Takes::Nothing),
    ("default-signal", Takes::OptionalValue),
    ("help", Takes::NoCommand),
    ("ignore-environment", Takes::Nothing),
    ("ignore-signal", Takes::OptionalValue),
    ("list-signal-handling", Takes::Nothing),
    ("null", Takes::NoCommand),
    ("split-string", Takes::SplitString),
    ("unset", Takes::Operand),
    ("version", Takes::NoCommand),
];

fn env_long_option(name: &str) -> Option<Takes> {
    if let Some(&(_, takes)) = ENV_LONG_OPTIONS.iter().find(|&&(n, _)| n == name) {
        return Some(takes);
    }
    let mut matches = ENV_LONG_OPTIONS
        .iter()
        .filter(|&&(n, _)| n.starts_with(name));
    let &(_, takes) = matches.next()?;
    matches.next().is_none().then_some(takes)
}

/// `env`'s short options: GNU's, plus BSD's `-P altpath`.
fn env_short_option(c: char) -> Option<Takes> {
    match c {
        'i' | 'v' => Some(Takes::Nothing),
        '0' => Some(Takes::NoCommand),
        'a' | 'C' | 'P' | 'u' => Some(Takes::Operand),
        'S' => Some(Takes::SplitString),
        _ => None,
    }
}

/// `env`'s arguments on a shebang line: the line's words, until `-S` takes the
/// rest of the line and splits it into the arguments that follow.
struct EnvArgs<'a> {
    line: &'a str,
    split: VecDeque<String>,
}

impl EnvArgs<'_> {
    fn next(&mut self) -> Option<String> {
        if let Some(arg) = self.split.pop_front() {
            return Some(arg);
        }
        let line = self.line.trim_start();
        let (word, rest) = line.split_at(line.find(char::is_whitespace).unwrap_or(line.len()));
        self.line = rest;
        (!word.is_empty()).then(|| word.to_owned())
    }

    /// Splits a `-S` operand into the next arguments. `attached` is the text
    /// joined to the option in its word; from the line, the operand runs on to
    /// the end of it. `None` where `env` would reject the string.
    fn split_string(&mut self, attached: Option<&str>) -> Option<()> {
        let operand = match attached {
            Some(attached) => format!("{attached}{}", self.line),
            None if self.split.is_empty() => self.line.to_owned(),
            None => self.split.pop_front()?,
        };
        self.line = "";
        for word in split_string(&operand)?.into_iter().rev() {
            self.split.push_front(word);
        }
        Some(())
    }
}

/// The command `env` runs, given its arguments on a shebang line: the first
/// one after its options and `NAME=value` assignments. `None` if it runs
/// nothing, or if it would reject its arguments.
fn env_command(args: &str) -> Option<String> {
    let mut args = EnvArgs {
        line: args,
        split: VecDeque::new(),
    };
    let mut command = loop {
        let arg = args.next()?;
        if arg == "--" {
            break args.next()?;
        } else if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (long, None),
            };
            match (env_long_option(name)?, value) {
                (Takes::NoCommand, _) | (Takes::Nothing, Some(_)) => return None,
                (Takes::Operand, None) => {
                    args.next()?;
                }
                (Takes::SplitString, value) => args.split_string(value)?,
                _ => {}
            }
        } else if let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.is_empty()) {
            for (i, c) in cluster.char_indices() {
                let attached = &cluster[i + c.len_utf8()..];
                match env_short_option(c)? {
                    Takes::NoCommand => return None,
                    Takes::Nothing | Takes::OptionalValue => continue,
                    Takes::Operand if attached.is_empty() => {
                        args.next()?;
                    }
                    Takes::Operand => {}
                    Takes::SplitString => {
                        args.split_string(Some(attached).filter(|a| !a.is_empty()))?
                    }
                }
                break;
            }
        } else {
            break arg;
        }
    };
    // A lone `-` after the options means `-i`.
    if command == "-" {
        command = args.next()?;
    }
    while command.contains('=') {
        command = args.next()?;
    }
    Some(command)
}

/// Splits an `env -S` string by GNU `env`'s rules. Whitespace outside quotes
/// separates words. Single quotes keep their text as is but for `\\` and `\'`.
/// Elsewhere a backslash escapes one character: `\_` separates words outside
/// double quotes and is a space inside them, and `\c` ends the string, as does
/// an unquoted `#` that starts a word. `${NAME}` is kept unexpanded, since the
/// script's environment is unknown. `None` where `env` rejects the string.
fn split_string(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let (mut single, mut double) = (false, false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let quoted = single || double;
        let literal = match c {
            '\'' if !double => {
                single = !single;
                word.get_or_insert_default();
                continue;
            }
            '"' if !single => {
                double = !double;
                word.get_or_insert_default();
                continue;
            }
            ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' if !quoted => {
                words.extend(word.take());
                continue;
            }
            '#' if !quoted && word.is_none() => break,
            '\\' if single && !matches!(chars.peek(), Some('\\' | '\'')) => '\\',
            '\\' => match chars.next()? {
                '_' if double => ' ',
                '_' => {
                    words.extend(word.take());
                    continue;
                }
                'c' if double => return None,
                'c' => break,
                'f' => '\x0c',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'v' => '\x0b',
                escaped @ ('"' | '#' | '$' | '\'' | '\\') => escaped,
                _ => return None,
            },
            '$' if !single => {
                if chars.next()? != '{' {
                    return None;
                }
                let word = word.get_or_insert_default();
                word.push_str("${");
                let mut name_len = 0;
                loop {
                    match chars.next()? {
                        '}' if name_len > 0 => break,
                        c if c == '_'
                            || c.is_ascii_alphabetic()
                            || (c.is_ascii_digit() && name_len > 0) =>
                        {
                            word.push(c);
                            name_len += 1;
                        }
                        _ => return None,
                    }
                }
                word.push('}');
                continue;
            }
            c => c,
        };
        word.get_or_insert_default().push(literal);
    }
    if single || double {
        return None;
    }
    words.extend(word);
    Some(words)
}

/// Nothing, or dot-separated numbers such as `3` or `3.12`.
fn is_version_suffix(suffix: &str) -> bool {
    suffix.is_empty()
        || suffix
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

fn base_name(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

#[cfg(all(test, feature = "python", feature = "bash", feature = "javascript"))]
mod tests {
    use super::*;

    #[test]
    fn shebangs_pick_the_interpreters_language() {
        let cases = [
            ("#!/usr/bin/python", Some("python")),
            ("#!/usr/bin/env python3", Some("python")),
            ("#!/usr/bin/python3.12 -u", Some("python")),
            ("#!/bin/sh", Some("bash")),
            ("#!/usr/bin/env bash\nset -e", Some("bash")),
            ("#! /bin/zsh -f", Some("bash")),
            ("#!/usr/bin/env node", Some("javascript")),
            ("#!/usr/bin/env -S node --flag", Some("javascript")),
            ("#!/usr/bin/env -S -i FOO=1 python3 -u", Some("python")),
            ("#!/usr/bin/nodejs", Some("javascript")),
            ("#!/usr/bin/env -S \"python3\" -u", Some("python")),
            ("#!/usr/bin/env -S -u FOO python3", Some("python")),
            ("#!/usr/bin/env -S 'node' --flag", Some("javascript")),
            // Signal options take a value only when attached with `=`.
            ("#!/usr/bin/env -S --default-signal python3", Some("python")),
            ("#!/usr/bin/env -S --ignore-signal python3", Some("python")),
            ("#!/usr/bin/env -S --block-signal python3", Some("python")),
            (
                "#!/usr/bin/env -S --default-signal=INT python3",
                Some("python"),
            ),
            // Operands attached, separate, or after `=`; clusters; abbreviations.
            ("#!/usr/bin/env -uFOO python3", Some("python")),
            ("#!/usr/bin/env --unset FOO python3", Some("python")),
            ("#!/usr/bin/env --uns=FOO -C /tmp python3", Some("python")),
            ("#!/usr/bin/env -iS python3 -u", Some("python")),
            ("#!/usr/bin/env -Spython3", Some("python")),
            ("#!/usr/bin/env --split-string=python3 -u", Some("python")),
            ("#!/usr/bin/env -i - FOO=1 python3", Some("python")),
            ("#!/usr/bin/env -- FOO=1 python3", Some("python")),
            // `-S` escapes, quotes, comments and variables.
            ("#!/usr/bin/env -S FOO=\"a\\\"b\" python3", Some("python")),
            ("#!/usr/bin/env -S FOO='a\\'b' python3", Some("python")),
            ("#!/usr/bin/env -S python3\\_-u", Some("python")),
            ("#!/usr/bin/env -S\\_python3", Some("python")),
            ("#!/usr/bin/env -S \"FOO=a\\_b\" python3", Some("python")),
            ("#!/usr/bin/env -S FOO=a\\tb python3", Some("python")),
            ("#!/usr/bin/env -S /opt/${V}/bin/python3 -u", Some("python")),
            ("#!/usr/bin/env -S node # comment", Some("javascript")),
            ("#!/usr/bin/env -S # python3", None),
            ("#!/usr/bin/env -S \\c python3", None),
            ("#!/usr/bin/env -S ${PY} -u", None),
            // What env rejects runs nothing.
            ("#!/usr/bin/env -S 'python3", None),
            ("#!/usr/bin/env -S python3\\q", None),
            ("#!/usr/bin/env -S python3\\", None),
            ("#!/usr/bin/env -S \"a\\cb\" python3", None),
            ("#!/usr/bin/env -S $PY python3", None),
            ("#!/usr/bin/env --ignore python3", None),
            ("#!/usr/bin/env --debug=1 python3", None),
            ("#!/usr/bin/env -x python3", None),
            ("#!/usr/bin/env -i0 python3", None),
            ("#!/usr/bin/env --null python3", None),
            ("#!/usr/bin/env --version python3", None),
            ("#!/usr/bin/env -u", None),
            ("#!/usr/bin/env FOO=1", None),
            // The kernel splits the line on whitespace only.
            ("#!/usr/bin/env \"python3\"", None),
            ("#!/usr/bin/perl", None),
            ("#!/usr/bin/python.", None),
            ("#!/usr/bin/python3..12", None),
            ("#!/usr/bin/python-config", None),
            ("#!/usr/bin/env", None),
            ("#!", None),
        ];
        for (text, want) in cases {
            assert_eq!(language_name_for_shebang(text), want, "{text:?}");
        }
    }

    #[test]
    fn the_path_wins_over_the_shebang() {
        let shebang = "#!/usr/bin/env python3\n";
        assert_eq!(
            detect_language_name(Some(Path::new("run.js")), shebang),
            Some("javascript")
        );
        assert_eq!(
            detect_language_name(Some(Path::new("bin/deploy")), shebang),
            Some("python")
        );
        assert_eq!(detect_language_name(None, shebang), Some("python"));
        assert_eq!(detect_language_name(None, "print(1)\n#!/bin/sh"), None);
        assert_eq!(detect_language_name(None, ""), None);
    }
}
