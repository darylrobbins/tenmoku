use std::path::PathBuf;

use fe_chat::ChatId;
use fe_chat::permissions::{Class, Level, Policy, Request};

fn chat() -> ChatId {
    ChatId::new()
}

fn p(path: &str) -> Option<PathBuf> {
    Some(PathBuf::from(path))
}

fn argv(chat: ChatId, words: &[&str], resolved: Option<&str>) -> Request {
    Request::argv(
        chat,
        words.iter().map(|w| (*w).to_owned()).collect(),
        resolved.and_then(p),
    )
}

#[test]
fn strictest_wins() {
    let c = chat();
    let mut policy = Policy::new();
    policy.set_level(Class::Read, Level::Allow);
    policy.set_project_level(Class::Read, Level::Ask);
    assert_eq!(
        policy.decide(&Request::plain(c, Class::Read, "a")),
        Level::Ask
    );
    policy.set_level(Class::Read, Level::Deny);
    policy.set_project_level(Class::Read, Level::Allow);
    assert_eq!(
        policy.decide(&Request::plain(c, Class::Read, "a")),
        Level::Deny
    );

    policy.set_level(Class::RunCommand, Level::Allow);
    policy.ask_commands(&["git push"]);
    policy.deny_commands(&["rm"]);
    assert_eq!(
        policy.decide(&argv(c, &["git", "status"], Some("/usr/bin/git"))),
        Level::Allow
    );
    assert_eq!(
        policy.decide(&argv(c, &["git", "push"], Some("/usr/bin/git"))),
        Level::Ask
    );
    assert_eq!(
        policy.decide(&argv(c, &["rm", "x"], Some("/bin/rm"))),
        Level::Deny
    );
}

#[test]
fn allow_patterns_never_beat_deny() {
    let c = chat();
    let mut policy = Policy::new();
    policy.allow_commands(&["git", "rm *"]);
    policy.deny_commands(&["rm"]);
    assert_eq!(
        policy.decide(&argv(c, &["git", "log"], Some("/usr/bin/git"))),
        Level::Allow
    );
    assert_eq!(
        policy.decide(&argv(c, &["rm", "-rf"], Some("/bin/rm"))),
        Level::Deny
    );
    assert_eq!(policy.decide(&Request::shell(c, "rm -rf x")), Level::Deny);

    policy.set_level(Class::RunCommand, Level::Deny);
    assert_eq!(
        policy.decide(&argv(c, &["git", "log"], Some("/usr/bin/git"))),
        Level::Deny
    );
    let mut other = Policy::new();
    other.allow_commands(&["git"]);
    let req = argv(c, &["git", "log"], Some("/usr/bin/git"));
    assert!(other.remember(&req));
    other.set_level(Class::RunCommand, Level::Deny);
    assert_eq!(other.decide(&req), Level::Deny);
    assert!(!other.remember(&req));
}

#[test]
fn shell_strings_and_slashed_words_match_no_allow_pattern() {
    let c = chat();
    let mut policy = Policy::new();
    policy.allow_commands(&["git", "cargo test"]);
    assert_eq!(
        policy.decide(&Request::shell(c, "git status")),
        Level::Allow
    );
    for text in [
        "git status; rm x",
        "git a && b",
        "git a || b",
        "git log | head",
        "git `x`",
        "git $(x)",
        "git log > out",
        "git $HOME",
        "./git status",
        "/usr/bin/git status",
    ] {
        assert_eq!(
            policy.decide(&Request::shell(c, text)),
            Level::Ask,
            "{text}"
        );
    }
    policy.deny_commands(&["git"]);
    assert_eq!(policy.decide(&Request::shell(c, "git a; b")), Level::Deny);
    assert_eq!(
        policy.decide(&Request::shell(c, "/usr/bin/git a")),
        Level::Deny
    );
    // A path given to the API chat matches no allow pattern either.
    let mut other = Policy::new();
    other.allow_commands(&["build.sh"]);
    assert_eq!(
        other.decide(&argv(c, &["./build.sh"], Some("/w/build.sh"))),
        Level::Ask
    );
}

#[test]
fn a_request_carries_its_resolved_path() {
    let c = chat();
    let mut policy = Policy::new();
    policy.allow_commands(&["git"]);
    // Unresolved: no allow pattern.
    assert_eq!(policy.decide(&argv(c, &["git", "log"], None)), Level::Ask);
    assert_eq!(
        policy.decide(&argv(c, &["git", "log"], Some("/usr/bin/git"))),
        Level::Allow
    );

    // Memory is bound to the resolved path.
    let mut policy = Policy::new();
    let first = argv(c, &["cargo", "test"], Some("/usr/bin/cargo"));
    assert_eq!(policy.decide(&first), Level::Ask);
    assert!(policy.remember(&first));
    assert_eq!(policy.decide(&first), Level::Allow);
    let moved = argv(c, &["cargo", "test"], Some("/w/target/cargo"));
    assert_eq!(policy.decide(&moved), Level::Ask);
    assert_eq!(
        policy.decide(&argv(c, &["cargo", "test"], None)),
        Level::Ask
    );
    // A path-given program is never remembered or matched.
    let slashed = argv(c, &["./x", "a"], Some("/w/x"));
    assert!(!policy.remember(&slashed));
    assert_eq!(policy.decide(&slashed), Level::Ask);
}

#[test]
fn session_memory_is_scoped_to_a_chat() {
    let (a, b) = (chat(), chat());
    let mut policy = Policy::new();
    let fetch = Request::plain(a, Class::Fetch, "https://x.test");
    assert_eq!(policy.decide(&fetch), Level::Ask);
    assert!(policy.remember(&fetch));
    assert_eq!(policy.decide(&fetch), Level::Allow);
    assert_eq!(
        policy.decide(&Request::plain(b, Class::Fetch, "https://x.test")),
        Level::Ask
    );
    let cmd = argv(a, &["make", "all"], Some("/usr/bin/make"));
    assert!(policy.remember(&cmd));
    assert_eq!(policy.decide(&cmd), Level::Allow);
    assert_eq!(
        policy.decide(&argv(b, &["make", "all"], Some("/usr/bin/make"))),
        Level::Ask
    );
    // Program plus first argument, not the whole program.
    assert_eq!(
        policy.decide(&argv(a, &["make", "clean"], Some("/usr/bin/make"))),
        Level::Ask
    );
    policy.forget_chat(a);
    assert_eq!(policy.decide(&fetch), Level::Ask);
    assert_eq!(policy.decide(&cmd), Level::Ask);
}

#[test]
fn sandbox_network_access_is_its_own_request() {
    let c = chat();
    let mut policy = Policy::new();
    policy.set_level(Class::Other, Level::Allow);
    let other = Request::plain(c, Class::Other, "tool");
    assert!(policy.remember(&other));
    let net = Request::network(c, "example.com");
    assert_eq!(policy.decide(&net), Level::Ask);
    assert!(!policy.remember(&net));
    assert_eq!(
        policy.decide(&Request::network(c, "localhost")),
        Level::Deny
    );
    assert_eq!(
        policy.decide(&Request::network(c, "127.0.0.1")),
        Level::Deny
    );
    assert_eq!(policy.decide(&Request::network(c, "[::1]")), Level::Deny);
}

#[test]
fn a_class_allow_still_prompts_for_unverifiable_commands() {
    let c = chat();
    let mut policy = Policy::new();
    policy.set_level(Class::RunCommand, Level::Allow);
    assert_eq!(
        policy.decide(&Request::shell(c, "git status")),
        Level::Allow
    );
    assert_eq!(
        policy.decide(&Request::shell(c, "git status; other")),
        Level::Ask
    );
    assert_eq!(
        policy.decide(&argv(c, &["./build.sh"], Some("/w/build.sh"))),
        Level::Ask
    );
    assert_eq!(policy.decide(&argv(c, &["git"], None)), Level::Ask);
}

#[test]
fn quoting_and_escapes_defeat_matching() {
    let c = chat();
    let mut policy = Policy::new();
    policy.allow_commands(&["git"]);
    policy.deny_commands(&["git push"]);
    for text in [
        "git pu\\sh",
        "git 'push'",
        "git \"push\"",
        "git pu*",
        "git {push,x}",
    ] {
        let req = Request::shell(c, text);
        assert_ne!(policy.decide(&req), Level::Allow, "{text}");
        assert!(!policy.remember(&req), "{text}");
    }
}

#[test]
fn a_lone_star_matches_every_command() {
    let c = chat();
    let mut policy = Policy::new();
    policy.set_level(Class::RunCommand, Level::Allow);
    policy.ask_commands(&["*"]);
    assert_eq!(policy.decide(&Request::shell(c, "git a; b")), Level::Ask);
    assert_eq!(
        policy.decide(&argv(c, &["git"], Some("/usr/bin/git"))),
        Level::Ask
    );
    policy.deny_commands(&["*"]);
    assert_eq!(policy.decide(&Request::shell(c, "git a; b")), Level::Deny);
    assert_eq!(
        policy.decide(&argv(c, &["git"], Some("/usr/bin/git"))),
        Level::Deny
    );
}

#[test]
fn loopback_in_any_spelling_is_denied() {
    let c = chat();
    let policy = Policy::new();
    for host in [
        "0:0:0:0:0:0:0:1",
        "[::0001]",
        "::ffff:127.0.0.1",
        "LOCALHOST.",
        "a.localhost",
        "127.1.2.3",
    ] {
        assert_eq!(
            policy.decide(&Request::network(c, host)),
            Level::Deny,
            "{host}"
        );
    }
    assert_eq!(
        policy.decide(&Request::network(c, "example.com")),
        Level::Ask
    );
}
