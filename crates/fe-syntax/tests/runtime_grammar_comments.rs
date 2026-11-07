//! A registered grammar's comment tokens and shebang interpreters
//! ([`GrammarSpec::line_comment`], [`GrammarSpec::shebangs`]): the tokens
//! are what [`comment_tokens`] gives for it, its interpreters pick it for a
//! file no path decides, after the built-ins, and an interpreter a built-in
//! claims is refused. A grammar without them behaves as before. The JSON
//! fixture stands in for every grammar.

use std::path::{Path, PathBuf};

use fe_syntax::{
    CommentTokens, GrammarSpec, comment_tokens, detect_language_name, language_by_name,
    language_name_for_shebang, register_language, unregister_language,
};

fn fixture() -> PathBuf {
    // Read at run time: a path compiled in can name a deleted snapshot.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    Path::new(&manifest).join("tests/fixtures/tree-sitter-json.wasm")
}

fn spec(language: &str) -> GrammarSpec {
    GrammarSpec {
        language: language.into(),
        wasm: fixture(),
        ..GrammarSpec::default()
    }
}

#[test]
fn registered_tokens_are_the_languages_comment_tokens() {
    register_language(&GrammarSpec {
        line_comment: Some("//".into()),
        block_comment: Some(("/*".into(), "*/".into())),
        ..spec("cmt_both")
    })
    .unwrap();
    assert_eq!(
        comment_tokens("cmt_both"),
        Some(CommentTokens {
            line: Some("//"),
            block: Some(("/*", "*/")),
        })
    );

    register_language(&GrammarSpec {
        line_comment: Some("%".into()),
        ..spec("cmt_line")
    })
    .unwrap();
    assert_eq!(
        comment_tokens("cmt_line"),
        Some(CommentTokens {
            line: Some("%"),
            block: None,
        })
    );

    // Registering again with other tokens replaces them, though the
    // grammar's files are unchanged.
    register_language(&GrammarSpec {
        block_comment: Some(("(*".into(), "*)".into())),
        ..spec("cmt_line")
    })
    .unwrap();
    assert_eq!(
        comment_tokens("cmt_line"),
        Some(CommentTokens {
            line: None,
            block: Some(("(*", "*)")),
        })
    );

    assert!(unregister_language("cmt_both"));
    assert_eq!(comment_tokens("cmt_both"), None);
}

#[test]
fn a_grammar_without_the_keys_behaves_as_before() {
    register_language(&GrammarSpec {
        extensions: vec!["cmtnone".into()],
        ..spec("cmt_none")
    })
    .unwrap();
    assert!(language_by_name("cmt_none").is_some());
    assert_eq!(comment_tokens("cmt_none"), None);
    assert_eq!(language_name_for_shebang("#!/usr/bin/env cmt_none"), None);
    // Built-ins keep theirs.
    let rust = comment_tokens("rust").unwrap();
    assert_eq!((rust.line, rust.block), (Some("//"), Some(("/*", "*/"))));
}

#[test]
fn registered_shebangs_pick_the_language() {
    register_language(&GrammarSpec {
        shebangs: vec!["cmtsh".into(), "cmtrun".into()],
        ..spec("cmt_shebang")
    })
    .unwrap();
    for line in [
        "#!/usr/bin/env cmtsh",
        "#!/usr/bin/cmtsh3",
        "#!/usr/local/bin/cmtsh3.12 -x",
        "#!/usr/bin/env -S cmtrun --flag",
        "#!/usr/bin/env -S FOO=1 cmtrun",
    ] {
        assert_eq!(
            language_name_for_shebang(line),
            Some("cmt_shebang"),
            "{line}"
        );
    }
    for line in [
        "#!/usr/bin/cmtshx",
        "#!/usr/bin/cmtsh-config",
        "#!/usr/bin/env cmt",
        "#!/bin/cmtsh/other",
    ] {
        assert_eq!(language_name_for_shebang(line), None, "{line}");
    }

    let text = "#!/usr/bin/env cmtsh\n{}\n";
    assert_eq!(
        detect_language_name(Some(Path::new("bin/tool")), text),
        Some("cmt_shebang")
    );
    assert_eq!(detect_language_name(None, text), Some("cmt_shebang"));
    // A path that names a language wins.
    #[cfg(feature = "json")]
    assert_eq!(
        detect_language_name(Some(Path::new("data.json")), text),
        Some("json")
    );
}

#[cfg(all(feature = "python", feature = "bash", feature = "javascript"))]
#[test]
fn an_interpreter_a_built_in_claims_is_refused() {
    use fe_syntax::{Clash, GrammarError};

    let clash = |language: &str, interpreter: &str| {
        let spec = GrammarSpec {
            shebangs: vec!["cmtfine".into(), interpreter.into()],
            ..spec(language)
        };
        match register_language(&spec) {
            Err(GrammarError::Clash {
                built_in, clash, ..
            }) => {
                assert!(language_by_name(language).is_none(), "{language}");
                (built_in, clash)
            }
            other => panic!("{interpreter}: {other:?}"),
        }
    };
    for (interpreter, built_in) in [
        ("python", "python"),
        ("python3", "python"),
        ("python3.12", "python"),
        ("sh", "bash"),
        ("zsh5", "bash"),
        ("node", "javascript"),
        ("nodejs", "javascript"),
    ] {
        let language = format!("cmt_clash_{}", interpreter.replace('.', "_"));
        assert_eq!(
            clash(&language, interpreter),
            (built_in, Clash::Interpreter(interpreter.into()))
        );
    }

    // Nothing a built-in runs: `pythonista` and `sh.` run no built-in.
    register_language(&GrammarSpec {
        shebangs: vec!["pythonista".into(), "sh.".into()],
        ..spec("cmt_near")
    })
    .unwrap();
    assert_eq!(
        language_name_for_shebang("#!/usr/bin/env pythonista"),
        Some("cmt_near")
    );
    assert_eq!(
        language_name_for_shebang("#!/usr/bin/env python3"),
        Some("python")
    );
    let error = clash("cmt_message", "python3").1;
    assert_eq!(error, Clash::Interpreter("python3".into()));
    let message = register_language(&GrammarSpec {
        shebangs: vec!["python3".into()],
        ..spec("cmt_message")
    })
    .unwrap_err()
    .to_string();
    assert_eq!(
        message,
        "cmt_message: the interpreter \"python3\" is the built-in python's"
    );
}

/// A registered language named as a built-in this build leaves out is no
/// built-in: registering it again keeps its interpreters, and its
/// shebangs are what pick it.
#[cfg(not(feature = "ruby"))]
#[test]
fn a_language_named_as_an_absent_built_in_goes_by_its_shebangs() {
    let ruby = |shebangs: &[&str]| GrammarSpec {
        shebangs: shebangs.iter().map(|&name| name.into()).collect(),
        ..spec("ruby")
    };
    register_language(&ruby(&["ruby"])).unwrap();
    assert_eq!(
        language_name_for_shebang("#!/usr/bin/env ruby"),
        Some("ruby")
    );

    // Reloaded, as an extension's grammar is when its manifest changes.
    register_language(&ruby(&["ruby", "jruby"])).unwrap();
    assert_eq!(language_name_for_shebang("#!/usr/bin/jruby"), Some("ruby"));
    assert_eq!(
        language_name_for_shebang("#!/usr/bin/ruby3.3"),
        Some("ruby")
    );

    register_language(&ruby(&[])).unwrap();
    assert_eq!(language_name_for_shebang("#!/usr/bin/env ruby"), None);
}
