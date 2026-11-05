// SPDX-License-Identifier: Apache-2.0

//! Plugin warnings: what combinations of capabilities say, and which sets
//! are full access.

use kiln_command::capability::{Capability, Grants};
use kiln_plugin::warnings::{
    Warning, combination_warnings, describe, is_full_access, is_interpreter,
};

fn grants(capabilities: &[&str]) -> Grants {
    capabilities
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

fn texts(capabilities: &[&str]) -> Vec<String> {
    combination_warnings(&grants(capabilities))
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn read_plus_net_warns_it_can_send_your_code() {
    assert_eq!(
        texts(&["buffer.read", "net:api.example.com"]),
        ["It can read your code and send it to api.example.com."]
    );
    // fs.read reads code too, and every host is named.
    assert_eq!(
        combination_warnings(&grants(&[
            "fs.read:workspace",
            "net:b.example",
            "net:a.example"
        ])),
        [Warning::SendsCode {
            hosts: vec!["a.example".into(), "b.example".into()]
        }]
    );
    assert_eq!(
        texts(&[
            "buffer.read",
            "net:a.example",
            "net:b.example",
            "net:c.example"
        ]),
        ["It can read your code and send it to a.example, b.example and c.example."]
    );
    // Without a read grant there's no code of yours to send.
    assert!(texts(&["buffer.write", "net:api.example.com"]).is_empty());
}

#[test]
fn spawn_names_the_programs_outside_the_sandbox() {
    assert_eq!(
        texts(&["process.spawn:rg"]),
        ["It runs rg outside the sandbox, with your permissions."]
    );
    assert_eq!(
        combination_warnings(&grants(&["process.spawn:rg", "process.spawn:git"])),
        [Warning::OutsideSandbox {
            programs: vec!["git".into(), "rg".into()]
        }]
    );
}

#[test]
fn interpreters_are_flagged_near_wildcard() {
    for program in ["sh", "python3"] {
        let warnings = texts(&[&format!("process.spawn:{program}")]);
        assert_eq!(
            warnings[1],
            format!(
                "{program} is near-wildcard: it can run any code, so granting it is almost \
                 process.spawn:*."
            ),
            "{warnings:?}"
        );
        assert!(warnings[0].contains(&format!("runs {program} outside the sandbox")));
    }
    // Each interpreter is named; a plain tool isn't one.
    assert_eq!(
        combination_warnings(&grants(&["process.spawn:rg", "process.spawn:bash"]))[1..],
        [Warning::NearWildcard {
            program: "bash".into()
        }]
    );
    for program in [
        "bash",
        "zsh",
        "fish",
        "dash",
        "node",
        "python",
        "deno",
        "bun",
        "ruby",
        "perl",
        "python3.12",
        "Python3",
        "node22",
    ] {
        assert!(is_interpreter(program), "{program}");
    }
    for program in ["rg", "git", "cargo", "shfmt"] {
        assert!(!is_interpreter(program), "{program}");
    }
}

#[test]
fn wildcards_are_extreme_and_full_access() {
    let net = combination_warnings(&grants(&["buffer.read", "net:*", "net:api.example.com"]));
    assert_eq!(net, [Warning::SendsAnywhere]);
    assert!(net[0].is_extreme());
    assert!(net[0].to_string().contains("can send your code anywhere"));
    assert!(is_full_access(&grants(&["net:*"])));

    let spawn = combination_warnings(&grants(&["process.spawn:*", "process.spawn:sh"]));
    assert_eq!(spawn, [Warning::RunsAnyProgram]);
    assert!(spawn[0].is_extreme());
    assert!(
        spawn[0]
            .to_string()
            .contains("can run any program as you, outside the sandbox")
    );
    assert!(is_full_access(&grants(&["process.spawn:*"])));

    // Both, extreme first.
    assert_eq!(
        combination_warnings(&grants(&["net:*", "process.spawn:*"])),
        [Warning::SendsAnywhere, Warning::RunsAnyProgram]
    );

    let scoped = grants(&[
        "buffer.read",
        "net:api.example.com",
        "process.spawn:sh",
        "fs.write:workspace",
    ]);
    assert!(!is_full_access(&scoped));
    assert!(
        !combination_warnings(&scoped)
            .iter()
            .any(Warning::is_extreme)
    );
}

#[test]
fn a_benign_set_has_no_warning() {
    assert!(texts(&["buffer.read", "commands", "status"]).is_empty());
    assert!(!is_full_access(&grants(&[
        "buffer.read",
        "commands",
        "status"
    ])));
    // Every capability still says what it does.
    assert_eq!(
        describe(&Capability::parse("buffer.read").unwrap()),
        "read the text of your open files"
    );
    assert_eq!(
        describe(&Capability::parse("net:api.example.com").unwrap()),
        "connect to api.example.com"
    );
}
