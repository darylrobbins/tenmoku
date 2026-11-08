// SPDX-License-Identifier: Apache-2.0

//! Layer 2: no hard-coded user-facing literal in a crate's non-test source.
//!
//! - **R1, anywhere.** A literal with whitespace and a letter, or that starts
//!   with an uppercase letter followed by a lowercase one.
//! - **R2, UI sinks.** A literal with any letter passed to a UI sink:
//!   `.child(`, `.children(`, `.tooltip(`, `.aria_label(`, `.placeholder(`,
//!   `.prompt(`, `register(`, `Toast::*`, `Button::new`, `PromptButton::*`,
//!   `Picker::new`, `SharedString::from`/`new`/`new_static`, and the fields of
//!   a `StatusItem`. A sink argument that names a `const` or `static` of the
//!   crate, or a local bound to a literal in the same function, is checked as
//!   if the literal were there.
//! - **R3, format strings.** A `format!`, `format_args!`, `write!` or
//!   `writeln!` string, or an `#[error(…)]`, with a word of two or more
//!   letters outside its `{…}` placeholders.
//! - **R4, output.** The format string of `print!`, `println!`, `eprint!` and
//!   `eprintln!`, as R3: command-line text is user-facing.
//!
//! Never reported: `log::*` and `tracing::*`, `panic!`, `assert*!`,
//! `unreachable!`, `todo!`, `unimplemented!`, `.expect(`, `concat!`, `env!`,
//! `include_*!`, `matches!`, `resources!`; anything in an `impl Debug`; patterns, and
//! `==`/`!=` operands; arguments of `.contains(`, `.starts_with(`,
//! `.ends_with(`, `.strip_prefix(`, `.strip_suffix(`, `.find(`, `.rfind(`;
//! `Command::new`, `.arg(`, `.args(` and `.env(`; attributes other than
//! `#[error]`; the key of `tr!`, `text!` and `tr_or`; and `verbatim!`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Expr, ExprLit, Lit, LitStr, Macro, Meta, Token};

use super::allow::AllowList;
use super::source::{SourceFile, crate_sources, literal_str};

/// Which rule reported a literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// Spaced or capitalised text, anywhere.
    R1,
    /// Any word in a UI sink.
    R2,
    /// A word in a format string.
    R3,
    /// A word printed to the terminal.
    R4,
}

/// One reported literal.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    /// The file, relative to the crate, such as `src/picker.rs`.
    pub file: String,
    pub line: usize,
    pub column: usize,
    /// The literal's text.
    pub text: String,
    pub rules: BTreeSet<Rule>,
    /// The `const`, `static` or local the sink argument named, if any.
    pub via: Option<String>,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rules: Vec<String> = self.rules.iter().map(|r| format!("{r:?}")).collect();
        write!(
            f,
            "{}:{}:{}: {} {:?}",
            self.file,
            self.line,
            self.column,
            rules.join("+"),
            self.text
        )?;
        if let Some(via) = &self.via {
            write!(f, " (through {via})")?;
        }
        Ok(())
    }
}

/// What a run of the literal lint found.
#[derive(Clone, Debug, Default)]
pub struct LiteralReport {
    /// The files it checked, relative to the crate.
    pub checked: Vec<String>,
    pub findings: Vec<Finding>,
    /// Problems with the source or the allow-list, such as an entry without a
    /// reason.
    pub errors: Vec<String>,
}

impl LiteralReport {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty() && self.errors.is_empty()
    }
}

impl fmt::Display for LiteralReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for error in &self.errors {
            writeln!(f, "error: {error}")?;
        }
        if !self.findings.is_empty() {
            writeln!(
                f,
                "{} hard-coded user-facing literal(s): move each to the crate's l10n/en-US/*.ftl and use tr!/text!, \
                 or mark it verbatim!(…) or list it in l10n-allow.toml with a reason:",
                self.findings.len()
            )?;
            for finding in &self.findings {
                writeln!(f, "  {finding}")?;
            }
        }
        Ok(())
    }
}

/// The literal lint over the crate at `manifest_dir`:
///
/// ```ignore
/// kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
///     .allow_list("l10n-allow.toml")
///     .check();
/// ```
pub fn literals(manifest_dir: impl AsRef<Path>) -> Literals {
    Literals {
        dir: manifest_dir.as_ref().to_path_buf(),
        allow_list: None,
    }
}

/// A configured literal lint; see [`literals`].
pub struct Literals {
    dir: PathBuf,
    allow_list: Option<String>,
}

impl Literals {
    /// Reads the crate's allow-list, `name` relative to the crate. Without
    /// one, every file is checked and nothing is allowed.
    pub fn allow_list(mut self, name: &str) -> Self {
        self.allow_list = Some(name.to_string());
        self
    }

    /// Runs the lint.
    pub fn report(&self) -> LiteralReport {
        let (files, mut errors) = crate_sources(&self.dir);
        if files.is_empty() && errors.is_empty() {
            errors.push(format!(
                "{}: no src/lib.rs, src/main.rs or src/bin",
                self.dir.display()
            ));
        }
        let names: BTreeSet<String> = files.iter().map(|f| f.rel.clone()).collect();
        let allow = match &self.allow_list {
            None => AllowList::everything(),
            Some(name) => match AllowList::read(&self.dir.join(name), name, &names) {
                Ok(allow) => allow,
                Err(mut list_errors) => {
                    errors.append(&mut list_errors);
                    return LiteralReport {
                        checked: Vec::new(),
                        findings: Vec::new(),
                        errors,
                    };
                }
            },
        };
        let mut report = scan(&files, &allow);
        errors.append(&mut report.errors);
        report.errors = errors;
        report
    }

    /// Runs the lint and panics with every finding and error.
    #[track_caller]
    pub fn check(&self) {
        let report = self.report();
        assert!(report.is_clean(), "{report}");
    }
}

/// The lint over one file's source, as `src/lib.rs` of a crate with every
/// file checked.
pub fn scan_source(source: &str) -> Result<Vec<Finding>, String> {
    let mut syntax = syn::parse_file(source).map_err(|e| e.to_string())?;
    syn::visit_mut::VisitMut::visit_file_mut(&mut super::source::StripTests, &mut syntax);
    let files = vec![SourceFile {
        rel: "src/lib.rs".to_string(),
        syntax,
    }];
    let report = scan(&files, &AllowList::everything());
    Ok(report.findings)
}

fn scan(files: &[SourceFile], allow: &AllowList) -> LiteralReport {
    let mut consts = HashMap::new();
    for file in files {
        ConstTable(&mut consts).visit_file(&file.syntax);
    }
    let mut report = LiteralReport::default();
    for file in files {
        if !allow.checks(&file.rel) {
            continue;
        }
        report.checked.push(file.rel.clone());
        let mut scanner = Scanner {
            file: &file.rel,
            consts: &consts,
            locals: Vec::new(),
            found: BTreeMap::new(),
        };
        scanner.visit_file(&file.syntax);
        report.findings.extend(scanner.found.into_values());
    }
    let mut used = vec![false; allow.literals.len()];
    report.findings.retain(|finding| {
        let allowed = allow
            .literals
            .iter()
            .position(|(file, text)| *file == finding.file && *text == finding.text);
        if let Some(index) = allowed {
            used[index] = true;
        }
        allowed.is_none()
    });
    for ((file, text), used) in allow.literals.iter().zip(used) {
        if !used && report.checked.contains(file) {
            report.errors.push(format!(
                "l10n-allow.toml: [[literal]] {text:?} in `{file}` matches nothing; remove it"
            ));
        }
    }
    report
}

fn r1(text: &str) -> bool {
    let spaced = text.chars().any(char::is_whitespace) && text.chars().any(char::is_alphabetic);
    let mut chars = text.chars();
    let capitalised = matches!(
        (chars.next(), chars.next()),
        (Some(a), Some(b)) if a.is_uppercase() && b.is_lowercase()
    );
    spaced || capitalised
}

/// Whether a format string has a word of two or more letters outside its
/// placeholders.
fn r3(text: &str) -> bool {
    let mut outside = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                outside.push(' ');
            }
            '{' => {
                for inner in chars.by_ref() {
                    if inner == '}' {
                        break;
                    }
                }
                outside.push(' ');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                outside.push(' ');
            }
            c => outside.push(c),
        }
    }
    let mut run = 0;
    for c in outside.chars() {
        run = if c.is_alphabetic() { run + 1 } else { 0 };
        if run >= 2 {
            return true;
        }
    }
    false
}

const EXEMPT_MACROS: &[&str] = &[
    "trace",
    "debug",
    "info",
    "warn",
    "error",
    "log",
    "event",
    "span",
    "trace_span",
    "debug_span",
    "info_span",
    "warn_span",
    "error_span",
    "panic",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "unreachable",
    "todo",
    "unimplemented",
    "concat",
    "env",
    "option_env",
    "include",
    "include_str",
    "include_bytes",
    "matches",
    "verbatim",
    "resources",
    "stringify",
    "compile_error",
    "cfg",
    "line",
    "column",
    "file",
    "module_path",
];

/// Methods whose arguments match external text, configure a subprocess, or
/// explain a panic.
const EXEMPT_METHODS: &[&str] = &[
    "contains",
    "starts_with",
    "ends_with",
    "strip_prefix",
    "strip_suffix",
    "find",
    "rfind",
    "expect",
    "arg",
    "args",
    "env",
];

const SINK_METHODS: &[&str] = &[
    "child",
    "children",
    "tooltip",
    "aria_label",
    "placeholder",
    "prompt",
    "register",
];

/// Methods that pass their receiver's text on, looked through in a sink
/// argument.
const PASS_METHODS: &[&str] = &[
    "into",
    "to_string",
    "to_owned",
    "clone",
    "as_str",
    "as_ref",
    "into_owned",
];

fn segments(path: &syn::Path) -> Vec<String> {
    path.segments.iter().map(|s| s.ident.to_string()).collect()
}

fn ends_with(segments: &[String], tail: &[&str]) -> bool {
    segments.len() >= tail.len()
        && segments[segments.len() - tail.len()..]
            .iter()
            .zip(tail)
            .all(|(a, b)| a == b)
}

fn is_sink_path(segments: &[String]) -> bool {
    let n = segments.len();
    let owner = if n >= 2 { segments[n - 2].as_str() } else { "" };
    let last = segments.last().map(String::as_str).unwrap_or("");
    matches!(owner, "Toast" | "PromptButton")
        || (owner == "Button" && last == "new")
        || (owner == "Picker" && last == "new")
        || (owner == "SharedString" && matches!(last, "from" | "new" | "new_static"))
        || last == "register"
}

fn is_pass_call(segments: &[String]) -> bool {
    (segments.len() == 1 && segments[0] == "Some")
        || ends_with(segments, &["String", "from"])
        || ends_with(segments, &["Cow", "Borrowed"])
        || ends_with(segments, &["Cow", "Owned"])
        || ends_with(segments, &["Into", "into"])
        || ends_with(segments, &["From", "from"])
}

fn is_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(_) => true,
        Expr::Reference(r) => is_literal(&r.expr),
        Expr::Paren(p) => is_literal(&p.expr),
        Expr::Group(g) => is_literal(&g.expr),
        _ => false,
    }
}

fn parse_args(mac: &Macro) -> Option<Punctuated<Expr, Token![,]>> {
    mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated)
        .ok()
}

fn block_tail(block: &syn::Block) -> Option<&Expr> {
    match block.stmts.last() {
        Some(syn::Stmt::Expr(expr, None)) => Some(expr),
        _ => None,
    }
}

/// The string literals an expression evaluates to, looking through
/// references, parentheses, arrays, tuples, branches, block tails and
/// conversions, but not through names.
fn literal_values(expr: &Expr, out: &mut Vec<String>) {
    terminals(expr, &mut |found| {
        if let Terminal::Literal(lit) = found {
            out.push(lit.value());
        }
    });
}

enum Terminal<'a> {
    Literal(&'a LitStr),
    Name(&'a syn::ExprPath),
}

fn terminals<'a>(expr: &'a Expr, found: &mut dyn FnMut(Terminal<'a>)) {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => found(Terminal::Literal(s)),
        Expr::Paren(e) => terminals(&e.expr, found),
        Expr::Group(e) => terminals(&e.expr, found),
        Expr::Reference(e) => terminals(&e.expr, found),
        Expr::Cast(e) => terminals(&e.expr, found),
        Expr::Array(a) => a.elems.iter().for_each(|e| terminals(e, found)),
        Expr::Tuple(t) => t.elems.iter().for_each(|e| terminals(e, found)),
        Expr::If(i) => {
            if let Some(tail) = block_tail(&i.then_branch) {
                terminals(tail, found);
            }
            if let Some((_, otherwise)) = &i.else_branch {
                terminals(otherwise, found);
            }
        }
        Expr::Match(m) => m.arms.iter().for_each(|arm| terminals(&arm.body, found)),
        Expr::Block(b) => {
            if let Some(tail) = block_tail(&b.block) {
                terminals(tail, found);
            }
        }
        Expr::Unsafe(b) => {
            if let Some(tail) = block_tail(&b.block) {
                terminals(tail, found);
            }
        }
        Expr::MethodCall(m)
            if m.args.is_empty() && PASS_METHODS.contains(&m.method.to_string().as_str()) =>
        {
            terminals(&m.receiver, found)
        }
        Expr::Call(c) => {
            if let Expr::Path(p) = &*c.func
                && is_pass_call(&segments(&p.path))
            {
                c.args.iter().for_each(|e| terminals(e, found));
            }
        }
        Expr::Path(p) if p.qself.is_none() => found(Terminal::Name(p)),
        _ => {}
    }
}

/// Every `const` and `static` of the crate, by name, with the literals it
/// holds.
struct ConstTable<'a>(&'a mut HashMap<String, Vec<String>>);

impl ConstTable<'_> {
    fn add(&mut self, name: &syn::Ident, expr: &Expr) {
        let mut values = Vec::new();
        literal_values(expr, &mut values);
        if !values.is_empty() {
            self.0.entry(name.to_string()).or_default().extend(values);
        }
    }
}

impl<'ast> Visit<'ast> for ConstTable<'_> {
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.add(&item.ident, &item.expr);
        visit::visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.add(&item.ident, &item.expr);
        visit::visit_item_static(self, item);
    }

    fn visit_impl_item_const(&mut self, item: &'ast syn::ImplItemConst) {
        self.add(&item.ident, &item.expr);
        visit::visit_impl_item_const(self, item);
    }

    fn visit_trait_item_const(&mut self, item: &'ast syn::TraitItemConst) {
        if let Some((_, expr)) = &item.default {
            self.add(&item.ident, expr);
        }
        visit::visit_trait_item_const(self, item);
    }
}

/// A function's locals bound directly to literals.
#[derive(Default)]
struct Locals(HashMap<String, Vec<String>>);

impl<'ast> Visit<'ast> for Locals {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        let mut pat = &local.pat;
        if let syn::Pat::Type(typed) = pat {
            pat = &typed.pat;
        }
        if let (syn::Pat::Ident(ident), Some(init)) = (pat, &local.init) {
            let mut values = Vec::new();
            literal_values(&init.expr, &mut values);
            if !values.is_empty() {
                self.0.insert(ident.ident.to_string(), values);
            }
        }
        visit::visit_local(self, local);
    }
}

struct Scanner<'a> {
    file: &'a str,
    consts: &'a HashMap<String, Vec<String>>,
    locals: Vec<HashMap<String, Vec<String>>>,
    found: BTreeMap<(usize, usize, String), Finding>,
}

impl Scanner<'_> {
    fn report(&mut self, text: String, span: Span, rule: Rule, via: Option<String>) {
        let start = span.start();
        let (line, column) = (start.line, start.column + 1);
        self.found
            .entry((line, column, text.clone()))
            .or_insert_with(|| Finding {
                file: self.file.to_string(),
                line,
                column,
                text,
                rules: BTreeSet::new(),
                via,
            })
            .rules
            .insert(rule);
    }

    fn literal(&mut self, lit: &LitStr) {
        let text = lit.value();
        if r1(&text) {
            self.report(text, lit.span(), Rule::R1, None);
        }
    }

    fn format_string(&mut self, lit: &LitStr, rule: Rule) {
        self.literal(lit);
        let text = lit.value();
        if r3(&text) {
            self.report(text, lit.span(), rule, None);
        }
    }

    fn named(&self, path: &syn::ExprPath) -> Option<(String, Vec<String>)> {
        let name = path.path.segments.last()?.ident.to_string();
        if path.path.segments.len() == 1
            && let Some(values) = self.locals.iter().rev().find_map(|scope| scope.get(&name))
        {
            return Some((format!("local `{name}`"), values.clone()));
        }
        self.consts
            .get(&name)
            .map(|values| (format!("`{name}`"), values.clone()))
    }

    fn sink(&mut self, arg: &Expr) {
        let mut hits: Vec<(String, Span, Option<String>)> = Vec::new();
        terminals(arg, &mut |found| match found {
            Terminal::Literal(lit) => hits.push((lit.value(), lit.span(), None)),
            Terminal::Name(path) => {
                if let Some((via, values)) = self.named(path) {
                    let span = path
                        .path
                        .segments
                        .last()
                        .map(|s| s.ident.span())
                        .unwrap_or_else(Span::call_site);
                    for value in values {
                        hits.push((value, span, Some(via.clone())));
                    }
                }
            }
        });
        for (text, span, via) in hits {
            if text.chars().any(char::is_alphabetic) {
                self.report(text, span, Rule::R2, via);
            }
        }
    }

    fn tokens(&mut self, tokens: TokenStream) {
        for tree in tokens {
            match tree {
                TokenTree::Group(group) => self.tokens(group.stream()),
                TokenTree::Literal(literal) => {
                    if let Some(text) = literal_str(&literal)
                        && r1(&text)
                    {
                        self.report(text, literal.span(), Rule::R1, None);
                    }
                }
                _ => {}
            }
        }
    }

    fn format_macro(&mut self, mac: &Macro, index: usize, rule: Rule) {
        let Some(args) = parse_args(mac) else {
            self.tokens(mac.tokens.clone());
            return;
        };
        for (i, arg) in args.iter().enumerate() {
            if i == index
                && let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = arg
            {
                self.format_string(s, rule);
                continue;
            }
            self.visit_expr(arg);
        }
    }

    fn function(&mut self, block: &syn::Block) {
        let mut locals = Locals::default();
        locals.visit_block(block);
        self.locals.push(locals.0);
        self.visit_block(block);
        self.locals.pop();
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        // Attributes are ids (`serde(rename)`, `command(id)`), docs and
        // paths, except thiserror's message, which users see.
        if !attr.path().is_ident("error") {
            return;
        }
        if let Meta::List(list) = &attr.meta
            && let Ok(args) = list.parse_args_with(Punctuated::<Expr, Token![,]>::parse_terminated)
            && let Some(Expr::Lit(ExprLit {
                lit: Lit::Str(s), ..
            })) = args.first()
        {
            self.format_string(s, Rule::R3);
        }
    }

    fn visit_abi(&mut self, _: &'ast syn::Abi) {}

    fn visit_pat(&mut self, _: &'ast syn::Pat) {}

    fn visit_lit_str(&mut self, lit: &'ast LitStr) {
        self.literal(lit);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let debug = item
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .is_some_and(|segment| segment.ident == "Debug");
        if !debug {
            visit::visit_item_impl(self, item);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        for attr in &item.attrs {
            self.visit_attribute(attr);
        }
        self.function(&item.block);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.function(&item.block);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if let Some(block) = &item.default {
            self.function(block);
        }
    }

    fn visit_expr_binary(&mut self, expr: &'ast syn::ExprBinary) {
        if matches!(expr.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
            for side in [&expr.left, &expr.right] {
                if !is_literal(side) {
                    self.visit_expr(side);
                }
            }
            return;
        }
        visit::visit_expr_binary(self, expr);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        if EXEMPT_METHODS.contains(&method.as_str()) {
            self.visit_expr(&call.receiver);
            return;
        }
        if SINK_METHODS.contains(&method.as_str()) {
            for arg in &call.args {
                self.sink(arg);
            }
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(path) = &*call.func {
            let segments = segments(&path.path);
            if ends_with(&segments, &["Command", "new"]) {
                return;
            }
            if segments.last().is_some_and(|last| last == "tr_or") {
                for arg in call.args.iter().skip(1) {
                    self.visit_expr(arg);
                }
                return;
            }
            if is_sink_path(&segments) {
                for arg in &call.args {
                    self.sink(arg);
                }
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
        if expr
            .path
            .segments
            .last()
            .is_some_and(|last| last.ident == "StatusItem")
        {
            for field in &expr.fields {
                self.sink(&field.expr);
            }
        }
        visit::visit_expr_struct(self, expr);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        let segments = segments(&mac.path);
        if segments.len() > 1 && matches!(segments[0].as_str(), "log" | "tracing") {
            return;
        }
        let name = segments.last().map(String::as_str).unwrap_or("");
        match name {
            _ if EXEMPT_MACROS.contains(&name) => {}
            "tr" | "text" => {
                if let Some(args) = parse_args(mac) {
                    for arg in args.iter().skip(1) {
                        self.visit_expr(arg);
                    }
                }
            }
            "format" | "format_args" => self.format_macro(mac, 0, Rule::R3),
            "print" | "println" | "eprint" | "eprintln" => self.format_macro(mac, 0, Rule::R4),
            "write" | "writeln" => self.format_macro(mac, 1, Rule::R3),
            _ => match parse_args(mac) {
                Some(args) => args.iter().for_each(|arg| self.visit_expr(arg)),
                None => self.tokens(mac.tokens.clone()),
            },
        }
    }
}
