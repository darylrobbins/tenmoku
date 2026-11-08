// SPDX-License-Identifier: Apache-2.0

//! A crate's non-test source: the files its targets' module trees reach,
//! with `#[cfg(test)]` and `#[test]` items removed.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use quote::ToTokens;
use syn::visit_mut::VisitMut;
use syn::{Attribute, Item, Lit, Meta};

/// One parsed source file.
pub(crate) struct SourceFile {
    /// Its path relative to the crate, with `/`, such as `src/lib.rs`.
    pub(crate) rel: String,
    pub(crate) syntax: syn::File,
}

/// A string literal found in a file, and where.
#[derive(Clone, Debug)]
pub(crate) struct FoundLiteral {
    pub(crate) value: String,
    pub(crate) line: usize,
}

/// Every non-test source file of the crate at `dir`: the module trees of
/// `src/lib.rs`, `src/main.rs` and `src/bin/*`. Tests, benches, examples and
/// build scripts are never reached.
pub(crate) fn crate_sources(dir: &Path) -> (Vec<SourceFile>, Vec<String>) {
    let mut roots = Vec::new();
    for name in ["src/lib.rs", "src/main.rs"] {
        roots.push(dir.join(name));
    }
    if let Ok(entries) = fs::read_dir(dir.join("src/bin")) {
        let mut bins: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        bins.sort();
        for bin in bins {
            if bin.extension().is_some_and(|e| e == "rs") {
                roots.push(bin);
            } else if bin.join("main.rs").is_file() {
                roots.push(bin.join("main.rs"));
            }
        }
    }
    let mut walk = Walk {
        dir: dir.to_path_buf(),
        files: Vec::new(),
        errors: Vec::new(),
        seen: BTreeSet::new(),
    };
    for root in roots.into_iter().filter(|r| r.is_file()) {
        let module_dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
        walk.file(&root, module_dir);
    }
    (walk.files, walk.errors)
}

struct Walk {
    dir: PathBuf,
    files: Vec<SourceFile>,
    errors: Vec<String>,
    seen: BTreeSet<PathBuf>,
}

impl Walk {
    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Reads the module file `path`, whose child modules live in `module_dir`.
    fn file(&mut self, path: &Path, module_dir: PathBuf) {
        if !self.seen.insert(path.to_path_buf()) {
            return;
        }
        let rel = self.rel(path);
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                self.errors.push(format!("{rel}: {error}"));
                return;
            }
        };
        let mut syntax = match syn::parse_file(&text) {
            Ok(syntax) => syntax,
            Err(error) => {
                let at = error.span().start();
                self.errors
                    .push(format!("{rel}:{}: doesn't parse: {error}", at.line));
                return;
            }
        };
        StripTests.visit_file_mut(&mut syntax);
        let file_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let items = syntax.items.clone();
        self.files.push(SourceFile { rel, syntax });
        self.modules(&items, &file_dir, &module_dir, false);
    }

    fn modules(&mut self, items: &[Item], file_dir: &Path, module_dir: &Path, inline: bool) {
        for item in items {
            let Item::Mod(module) = item else {
                continue;
            };
            let name = module.ident.to_string();
            let explicit = path_attribute(&module.attrs);
            match &module.content {
                Some((_, inner)) => {
                    let inner_dir = match &explicit {
                        Some(path) => module_dir.join(path),
                        None => module_dir.join(&name),
                    };
                    self.modules(inner, file_dir, &inner_dir, true);
                }
                None => {
                    let found = match &explicit {
                        Some(path) => {
                            let base = if inline { module_dir } else { file_dir };
                            Some(base.join(path))
                        }
                        None => [
                            module_dir.join(format!("{name}.rs")),
                            module_dir.join(&name).join("mod.rs"),
                        ]
                        .into_iter()
                        .find(|candidate| candidate.is_file()),
                    };
                    match found {
                        Some(path) if path.is_file() => {
                            // A `mod.rs`, or a file named by `#[path]`, keeps
                            // its children beside it; `name.rs` in `name/`.
                            let child_dir = if explicit.is_some() || path.ends_with("mod.rs") {
                                path.parent().map(Path::to_path_buf).unwrap_or_default()
                            } else {
                                module_dir.join(&name)
                            };
                            self.file(&path, child_dir);
                        }
                        _ => self.errors.push(format!(
                            "{}: can't find the file of `mod {name};`",
                            self.rel(&module_dir.join(&name))
                        )),
                    }
                }
            }
        }
    }
}

fn path_attribute(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        Meta::NameValue(nv) if nv.path.is_ident("path") => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: Lit::Str(s), ..
            }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    })
}

/// Whether an item's attributes make it test-only: `#[test]`, or a `cfg`
/// that can only hold under `test`.
pub(crate) fn is_test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("test") {
            return true;
        }
        if !attr.path().is_ident("cfg") {
            return false;
        }
        attr.parse_args::<Meta>()
            .is_ok_and(|meta| cfg_needs_test(&meta))
    })
}

fn cfg_needs_test(meta: &Meta) -> bool {
    match meta {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) => {
            let Ok(nested) = list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return false;
            };
            if list.path.is_ident("all") {
                nested.iter().any(cfg_needs_test)
            } else if list.path.is_ident("any") {
                !nested.is_empty() && nested.iter().all(cfg_needs_test)
            } else {
                false
            }
        }
        Meta::NameValue(_) => false,
    }
}

/// Removes test-only items, wherever they are nested.
pub(crate) struct StripTests;

impl VisitMut for StripTests {
    fn visit_file_mut(&mut self, file: &mut syn::File) {
        file.items.retain(|item| !item_is_test(item));
        syn::visit_mut::visit_file_mut(self, file);
    }

    fn visit_item_mod_mut(&mut self, module: &mut syn::ItemMod) {
        if let Some((_, items)) = &mut module.content {
            items.retain(|item| !item_is_test(item));
        }
        syn::visit_mut::visit_item_mod_mut(self, module);
    }

    fn visit_item_impl_mut(&mut self, item: &mut syn::ItemImpl) {
        item.items
            .retain(|item| !is_test_only(impl_item_attrs(item)));
        syn::visit_mut::visit_item_impl_mut(self, item);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        block.stmts.retain(|stmt| match stmt {
            syn::Stmt::Item(item) => !item_is_test(item),
            _ => true,
        });
        syn::visit_mut::visit_block_mut(self, block);
    }
}

fn item_is_test(item: &Item) -> bool {
    let attrs: &[Attribute] = match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    };
    is_test_only(attrs)
}

fn impl_item_attrs(item: &syn::ImplItem) -> &[Attribute] {
    match item {
        syn::ImplItem::Const(i) => &i.attrs,
        syn::ImplItem::Fn(i) => &i.attrs,
        syn::ImplItem::Type(i) => &i.attrs,
        syn::ImplItem::Macro(i) => &i.attrs,
        _ => &[],
    }
}

/// A string literal token's value.
pub(crate) fn literal_str(literal: &proc_macro2::Literal) -> Option<String> {
    match Lit::new(literal.clone()) {
        Lit::Str(s) => Some(s.value()),
        _ => None,
    }
}

/// Every string literal in `file`, macro arguments included, outside its
/// doc comments.
pub(crate) fn all_literals(file: &syn::File) -> Vec<FoundLiteral> {
    let mut out = Vec::new();
    let mut tokens = TokenStream::new();
    for item in &file.items {
        item.to_tokens(&mut tokens);
    }
    collect_literals(tokens, &mut out);
    out
}

fn collect_literals(tokens: TokenStream, out: &mut Vec<FoundLiteral>) {
    for tree in tokens {
        match tree {
            TokenTree::Group(group) => {
                // `#[doc = "…"]`: a doc comment isn't a use of a key.
                let stream = group.stream();
                let is_doc = group.delimiter() == Delimiter::Bracket
                    && stream.clone().into_iter().next().is_some_and(
                        |first| matches!(&first, TokenTree::Ident(ident) if ident == "doc"),
                    );
                if !is_doc {
                    collect_literals(stream, out);
                }
            }
            TokenTree::Literal(literal) => {
                if let Some(value) = literal_str(&literal) {
                    out.push(FoundLiteral {
                        value,
                        line: literal.span().start().line,
                    });
                }
            }
            _ => {}
        }
    }
}

/// The keys `file` passes as a literal to `tr!`, `text!` or `tr_or`.
pub(crate) fn key_literals(file: &syn::File) -> Vec<FoundLiteral> {
    let mut out = Vec::new();
    let mut tokens = TokenStream::new();
    for item in &file.items {
        item.to_tokens(&mut tokens);
    }
    collect_keys(tokens, &mut out);
    out
}

fn collect_keys(tokens: TokenStream, out: &mut Vec<FoundLiteral>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (i, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Group(group) => collect_keys(group.stream(), out),
            TokenTree::Ident(ident) => {
                let name = ident.to_string();
                let args = match name.as_str() {
                    "tr" | "text" => match (trees.get(i + 1), trees.get(i + 2)) {
                        (Some(TokenTree::Punct(bang)), Some(TokenTree::Group(group)))
                            if bang.as_char() == '!' =>
                        {
                            Some(group)
                        }
                        _ => None,
                    },
                    "tr_or" => match trees.get(i + 1) {
                        Some(TokenTree::Group(group))
                            if group.delimiter() == Delimiter::Parenthesis =>
                        {
                            Some(group)
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(group) = args
                    && let Some(TokenTree::Literal(literal)) = group.stream().into_iter().next()
                    && let Some(value) = literal_str(&literal)
                {
                    out.push(FoundLiteral {
                        value,
                        line: literal.span().start().line,
                    });
                }
            }
            _ => {}
        }
    }
}

/// The `dynamic_prefixes = [...]` a `resources!` call in `file` declares.
pub(crate) fn dynamic_prefixes(file: &syn::File) -> Vec<String> {
    struct Find(Vec<String>);
    impl<'ast> syn::visit::Visit<'ast> for Find {
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if !mac
                .path
                .segments
                .last()
                .is_some_and(|s| s.ident == "resources")
            {
                return;
            }
            let trees: Vec<TokenTree> = mac.tokens.clone().into_iter().collect();
            for window in trees.windows(3) {
                if let [
                    TokenTree::Ident(name),
                    TokenTree::Punct(eq),
                    TokenTree::Group(list),
                ] = window
                    && name == "dynamic_prefixes"
                    && eq.as_char() == '='
                {
                    for tree in list.stream() {
                        if let TokenTree::Literal(literal) = tree
                            && let Some(value) = literal_str(&literal)
                        {
                            self.0.push(value);
                        }
                    }
                }
            }
        }
    }
    let mut find = Find(Vec::new());
    syn::visit::Visit::visit_file(&mut find, file);
    find.0
}

/// Every `.ftl` file under `dir`, sorted.
pub(crate) fn ftl_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "ftl") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
