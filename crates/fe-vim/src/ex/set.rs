//! `:se[t] {arg} …` (`:help :set`) for the options fastedit's editor
//! holds: `wrap` (soft wrap), `tabstop`/`ts` (1 to 16), `shiftwidth`/`sw`
//! (0 meaning tabstop), `expandtab`/`et` and `clipboard`/`cb` (`unnamedplus`
//! or empty), and the search options `ignorecase`/`ic`, `smartcase`/`scs`
//! and `incsearch`/`is` (bools, all held by the shared `Registers`; each
//! editor's vim state copies `incsearch` before a preview). `shiftwidth`
//! and `expandtab` are the editor's indent style:
//! spaces of that width, or tabs (`noexpandtab`) when shiftwidth is
//! tabstop.
//!
//! Each blank-separated argument is applied in turn, stopping at the first
//! that fails with its error; those before it stay set. A bool is `opt`,
//! `noopt`, `invopt`, `opt!` or `opt?`; a number or string `opt` or `opt?`
//! (its value is the message, as nvim's `  tabstop=4`), `opt=V` or `opt:V`.
//! Refused, changing nothing: `noexpandtab` (or `sw=N` under it) when
//! shiftwidth isn't tabstop, `opt&`, `opt+=`, `opt-=`, `opt^=`, `:set`
//! alone and `:set all`, `clipboard=unnamed`. Any other name is
//! "not supported yet". A range is E481. A number is decimal, `0x` hex, `0b`
//! binary, `0o` or leading-`0` octal, as nvim reads it. These change the
//! editor, not the saved settings.

use fe_command::{Actor, Dispatched, Effect};
use fe_editor::IndentStyle;

use super::super::{Address, ExError, Handled, uncommented};
use super::{Check, Entry};
use crate::VimTarget;

pub(super) const COMMAND: Entry = Entry {
    name: "set",
    min: 2,
    edits: false,
    parse,
    run,
};

/// The most `tabstop` the editor's settings allow.
const MAX_TABSTOP: usize = 16;

#[derive(Clone, Copy, PartialEq)]
enum Opt {
    Wrap,
    Tabstop,
    Shiftwidth,
    Expandtab,
    Clipboard,
    Ignorecase,
    Smartcase,
    Incsearch,
}

/// The option `name` names, with its full name.
fn option(name: &str) -> Option<(Opt, &'static str)> {
    Some(match name {
        "wrap" => (Opt::Wrap, "wrap"),
        "tabstop" | "ts" => (Opt::Tabstop, "tabstop"),
        "shiftwidth" | "sw" => (Opt::Shiftwidth, "shiftwidth"),
        "expandtab" | "et" => (Opt::Expandtab, "expandtab"),
        "clipboard" | "cb" => (Opt::Clipboard, "clipboard"),
        "ignorecase" | "ic" => (Opt::Ignorecase, "ignorecase"),
        "smartcase" | "scs" => (Opt::Smartcase, "smartcase"),
        "incsearch" | "is" => (Opt::Incsearch, "incsearch"),
        _ => return None,
    })
}

fn parse(after: &str, _check: &mut Check<'_>) -> Result<String, ExError> {
    if after.starts_with('!') {
        return Err(ExError::NoBang);
    }
    Ok(uncommented(after).trim_matches([' ', '\t']).to_string())
}

fn run(
    target: &mut VimTarget<'_>,
    _actor: &Actor,
    _lines: Option<(i64, i64)>,
    range: &[Address],
    args: &str,
) -> Result<Handled, ExError> {
    if !range.is_empty() {
        return Err(ExError::NoRange);
    }
    if args.is_empty() || args == "all" {
        return Err(ExError::Pattern(format!(
            ":set {args}: not supported, name the options"
        )));
    }
    let mut report = Vec::new();
    let done = args
        .split([' ', '\t'])
        .filter(|arg| !arg.is_empty())
        .try_for_each(|arg| apply(target, arg, &mut report));
    // What was asked for before a failure is still shown, as nvim does.
    if !report.is_empty() {
        target.state.message = Some(report.join("\n"));
    }
    done?;
    Ok(Ok(Dispatched::new(Effect::Notify)))
}

/// Applies one argument, pushing what it reports onto `report`.
fn apply(target: &mut VimTarget<'_>, arg: &str, report: &mut Vec<String>) -> Result<(), ExError> {
    let split = arg
        .find(|ch: char| !ch.is_ascii_alphabetic())
        .unwrap_or(arg.len());
    let (name, tail) = arg.split_at(split);
    // `no` and `inv` only prefix a bool.
    let (found, prefix) = match option(name) {
        Some(found) => (found, ""),
        None => {
            let prefixed = ["no", "inv"]
                .into_iter()
                .find_map(|prefix| Some((option(name.strip_prefix(prefix)?)?, prefix)));
            match prefixed {
                Some((found, prefix)) => (found, prefix),
                None => return Err(ExError::Unsupported(name.to_string())),
            }
        }
    };
    let (which, full) = found;
    let is_bool = matches!(
        which,
        Opt::Wrap | Opt::Expandtab | Opt::Ignorecase | Opt::Smartcase | Opt::Incsearch
    );
    if !prefix.is_empty() && !is_bool {
        return Err(ExError::InvalidArgument(arg.to_string()));
    }
    if matches!(tail, "&" | "&vi" | "&vim" | "+=" | "-=" | "^=")
        || tail.starts_with("+=")
        || tail.starts_with("-=")
        || tail.starts_with("^=")
    {
        return Err(ExError::Pattern(format!("{arg}: not supported")));
    }
    match tail {
        "?" => {
            report.push(show(target, which, full));
            Ok(())
        }
        "" if prefix.is_empty() && !is_bool => {
            report.push(show(target, which, full));
            Ok(())
        }
        "" | "!" if is_bool => {
            let new = match (prefix, tail) {
                ("no", "") => false,
                ("inv", "") | ("", "!") => !value_of(target, which),
                ("", "") => true,
                _ => return Err(ExError::Trailing(arg.to_string())),
            };
            set_bool(target, which, new)
        }
        _ => {
            let value = tail
                .strip_prefix('=')
                .or_else(|| tail.strip_prefix(':'))
                .ok_or_else(|| ExError::Trailing(arg.to_string()))?;
            if is_bool {
                return Err(ExError::InvalidArgument(arg.to_string()));
            }
            match which {
                Opt::Clipboard => set_clipboard(target, arg, value),
                _ => set_number(target, which, arg, value),
            }
        }
    }
}

fn value_of(target: &VimTarget<'_>, which: Opt) -> bool {
    match which {
        Opt::Wrap => target.model.soft_wrap(),
        Opt::Ignorecase => target.registers.search_case().0,
        Opt::Smartcase => target.registers.search_case().1,
        Opt::Incsearch => target.registers.incsearch(),
        _ => target.model.indent_style() != Some(IndentStyle::Tabs),
    }
}

/// nvim's message for `opt?`.
fn show(target: &VimTarget<'_>, which: Opt, full: &str) -> String {
    match which {
        Opt::Wrap | Opt::Expandtab | Opt::Ignorecase | Opt::Smartcase | Opt::Incsearch => {
            match value_of(target, which) {
                true => format!("  {full}"),
                false => format!("no{full}"),
            }
        }
        Opt::Tabstop => format!("  tabstop={}", target.model.tab_size()),
        Opt::Shiftwidth => format!("  shiftwidth={}", shiftwidth(target)),
        Opt::Clipboard => match target.registers.clipboard_unnamed() {
            true => "  clipboard=unnamedplus".to_string(),
            false => "  clipboard=".to_string(),
        },
    }
}

/// The indent unit's width in columns.
fn shiftwidth(target: &VimTarget<'_>) -> usize {
    match target.model.indent_style() {
        Some(IndentStyle::Spaces(width)) => usize::from(width),
        Some(IndentStyle::Tabs) | None => target.model.tab_size(),
    }
}

fn set_bool(target: &mut VimTarget<'_>, which: Opt, on: bool) -> Result<(), ExError> {
    match which {
        Opt::Wrap => target.model.set_soft_wrap(on),
        Opt::Ignorecase => {
            let (_, smart) = target.registers.search_case();
            target.registers.set_search_case(on, smart);
        }
        Opt::Smartcase => {
            let (ignore, _) = target.registers.search_case();
            target.registers.set_search_case(ignore, on);
        }
        Opt::Incsearch => {
            target.registers.set_incsearch(on);
            target.state.set_incsearch(on);
        }
        _ => {
            let tab = target.model.tab_size();
            match (on, target.model.indent_style()) {
                (true, Some(IndentStyle::Tabs)) => target.model.set_indent_style(None),
                (false, Some(IndentStyle::Spaces(n))) if usize::from(n) != tab => {
                    return Err(ExError::Pattern(
                        "noexpandtab: not supported unless shiftwidth is tabstop".into(),
                    ));
                }
                (false, _) => target.model.set_indent_style(Some(IndentStyle::Tabs)),
                _ => {}
            }
        }
    }
    Ok(())
}

fn width(columns: usize) -> u8 {
    u8::try_from(columns).unwrap_or(u8::MAX)
}

fn set_clipboard(target: &mut VimTarget<'_>, arg: &str, value: &str) -> Result<(), ExError> {
    match value {
        "" => target.registers.set_clipboard_unnamed(false),
        "unnamedplus" => target.registers.set_clipboard_unnamed(true),
        "unnamed" | "unnamed,unnamedplus" | "unnamedplus,unnamed" => {
            return Err(ExError::Pattern(format!("{arg}: not supported")));
        }
        _ => return Err(ExError::InvalidArgument(arg.to_string())),
    }
    Ok(())
}

/// Reads a `:set` number as nvim does: an optional `-`, then `0x`/`0X` hex,
/// `0b`/`0B` binary, `0o`/`0O` octal, a leading `0` with only digits 0 to 7
/// (octal), or decimal (so `08` is 8). Returns the sign and magnitude (too
/// large saturates), or `None` for anything else or anything left over.
fn parse_number(value: &str) -> Option<(bool, usize)> {
    let (negative, rest) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    let bytes = rest.as_bytes();
    let (radix, digits) = match bytes {
        [b'0', b'x' | b'X', ..] => (16, &rest[2..]),
        [b'0', b'b' | b'B', ..] => (2, &rest[2..]),
        [b'0', b'o' | b'O', ..] => (8, &rest[2..]),
        [b'0', _, ..] if bytes.iter().all(|byte| (b'0'..=b'7').contains(byte)) => (8, &rest[1..]),
        _ => (10, rest),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    Some((
        negative,
        usize::from_str_radix(digits, radix).unwrap_or(usize::MAX),
    ))
}

fn set_number(
    target: &mut VimTarget<'_>,
    which: Opt,
    arg: &str,
    value: &str,
) -> Result<(), ExError> {
    let Some((negative, number)) = parse_number(value) else {
        return Err(ExError::Pattern(format!(
            "E521: Number required after =: {arg}"
        )));
    };
    if negative && number != 0 {
        return Err(ExError::Pattern(format!(
            "E487: Argument must be positive: {arg}"
        )));
    }
    let tab = target.model.tab_size();
    match which {
        Opt::Tabstop => {
            if number == 0 {
                return Err(ExError::Pattern(format!(
                    "E487: Argument must be positive: {arg}"
                )));
            }
            if number > MAX_TABSTOP {
                return Err(ExError::InvalidArgument(arg.to_string()));
            }
            // Tabs mean shiftwidth == tabstop: a new tabstop would leave a
            // shiftwidth fastedit can't tell from one set to the old value.
            if target.model.indent_style() == Some(IndentStyle::Tabs) && number != tab {
                return Err(ExError::Pattern(format!(
                    "{arg}: not supported under noexpandtab, set expandtab first"
                )));
            }
            target.model.set_tab_size(number);
        }
        _ => {
            // shiftwidth 0 follows tabstop: the editor's unset indent unit.
            if number == 0 {
                if target.model.indent_style() != Some(IndentStyle::Tabs) {
                    target.model.set_indent_style(None);
                }
                return Ok(());
            }
            if number > usize::from(u8::MAX) {
                return Err(ExError::InvalidArgument(arg.to_string()));
            }
            match target.model.indent_style() {
                Some(IndentStyle::Tabs) if number != tab => {
                    return Err(ExError::Pattern(format!(
                        "{arg}: not supported under noexpandtab unless it is tabstop"
                    )));
                }
                Some(IndentStyle::Tabs) => {}
                _ => target
                    .model
                    .set_indent_style(Some(IndentStyle::Spaces(width(number)))),
            }
        }
    }
    Ok(())
}
