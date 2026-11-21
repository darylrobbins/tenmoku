use std::{ffi::OsStr, path::PathBuf};

use super::*;

fn remote(text: &str) -> (HostId, RemotePath) {
    match ProjectLocation::parse(text) {
        Ok(ProjectLocation::Remote { host, path }) => (host, path),
        other => panic!("{text}: {other:?}"),
    }
}

#[test]
fn an_alias_and_its_path_parse() {
    let (host, path) = remote("ssh://neon/home/me/src/app");
    assert_eq!(host.kind(), HostKind::Ssh);
    assert_eq!(
        (host.user(), host.host(), host.port()),
        (None, "neon", None)
    );
    assert_eq!(host.destination(), "neon");
    assert_eq!(path.as_bytes(), b"/home/me/src/app");
}

#[test]
fn a_user_and_port_are_the_destination() {
    let (host, path) = remote("ssh://me@box:2222/srv/app");
    assert_eq!(
        (host.user(), host.host(), host.port()),
        (Some("me"), "box", Some(2222))
    );
    assert_eq!(host.destination(), "me@box:2222");
    assert_eq!(path.as_bytes(), b"/srv/app");
}

#[test]
fn a_bracketed_address_takes_a_port() {
    let (host, _) = remote("ssh://[fe80::1]:22/x");
    assert_eq!((host.host(), host.port()), ("[fe80::1]", Some(22)));
    let (host, _) = remote("ssh://[::ffff:192.0.2.1]/x");
    assert_eq!((host.host(), host.port()), ("[::ffff:192.0.2.1]", None));
}

#[test]
fn display_round_trips_the_text() {
    for text in [
        "ssh://neon/home/me/src/app",
        "ssh://me@box:2222/srv/app",
        "ssh://neon/",
        "ssh://neon/a%20b/c%25d/%3F%23",
        "ssh://neon/caf%C3%A9",
        "ssh://[::1]/x:y@z",
    ] {
        let location = ProjectLocation::parse(text).unwrap();
        assert_eq!(location.to_string(), text);
        assert_eq!(
            ProjectLocation::parse(&location.to_string()).unwrap(),
            location
        );
    }
}

#[test]
fn a_non_utf8_byte_is_percent_encoded_and_read_back() {
    let location = ProjectLocation::Remote {
        host: HostId::ssh("neon").unwrap(),
        path: RemotePath::new(b"/home/me/\xFFapp".to_vec()).unwrap(),
    };
    assert_eq!(location.to_string(), "ssh://neon/home/me/%FFapp");
    assert_eq!(
        ProjectLocation::parse("ssh://neon/home/me/%FFapp").unwrap(),
        location
    );
    assert_eq!(
        ProjectLocation::parse("ssh://neon/home/me/%ffapp").unwrap(),
        location
    );
}

#[test]
fn only_the_exact_form_parses() {
    for (text, error) in [
        ("neon/home/me", LocationError::NotSsh),
        ("SSH://neon/x", LocationError::NotSsh),
        ("sftp://neon/x", LocationError::NotSsh),
        ("ssh://neon", LocationError::Path),
        ("ssh:///x", LocationError::Host),
        ("ssh://-oProxyCommand=x/y", LocationError::Host),
        ("ssh://me@-x/y", LocationError::Host),
        ("ssh://@neon/x", LocationError::Host),
        ("ssh://me:pw@neon/x", LocationError::Host),
        ("ssh://ne on/x", LocationError::Host),
        ("ssh://[zz]/x", LocationError::Host),
        ("ssh://[:::]/x", LocationError::Host),
        ("ssh://[1::2::3]/x", LocationError::Host),
        ("ssh://[1.2.3.4]/x", LocationError::Host),
        ("ssh://[::1/x", LocationError::Host),
        ("ssh://[fe80::1%eth0]/x", LocationError::Host),
        ("ssh://neon:/x", LocationError::Port),
        ("ssh://neon:0/x", LocationError::Port),
        ("ssh://neon:022/x", LocationError::Port),
        ("ssh://neon:65536/x", LocationError::Port),
        ("ssh://neon:22:22/x", LocationError::Port),
        ("ssh://neon/a%2", LocationError::Escape),
        ("ssh://neon/a%zz", LocationError::Escape),
        ("ssh://neon/a%00", LocationError::Path),
        ("ssh://neon/a b", LocationError::Unescaped),
        ("ssh://neon/café", LocationError::Unescaped),
        ("ssh://neon/a?b", LocationError::QueryOrFragment),
        ("ssh://neon/a#b", LocationError::QueryOrFragment),
    ] {
        assert_eq!(ProjectLocation::parse(text), Err(error), "{text}");
    }
}

#[test]
fn a_local_path_is_its_own_text_form() {
    let local = ProjectLocation::Local(PathBuf::from("/home/me/src/app"));
    assert_eq!(local.text(), OsStr::new("/home/me/src/app"));
    assert_eq!(ProjectLocation::from_text(&local.text()), local);
    assert_eq!(local, PathBuf::from("/home/me/src/app"));
    assert!(!local.is_remote() && local.local().is_some());
}

#[test]
fn a_remote_text_form_is_its_uri() {
    let text = OsStr::new("ssh://neon/home/me/src/app");
    let location = ProjectLocation::from_text(text);
    assert!(location.is_remote() && location.local().is_none());
    assert_eq!(location.text(), text);
    assert!(ProjectLocation::is_remote_text(text));
    assert!(!ProjectLocation::is_remote_text(OsStr::new("/ssh://neon")));
}

#[test]
fn a_text_form_that_does_not_parse_is_a_local_path() {
    let text = OsStr::new("ssh://neon");
    assert_eq!(
        ProjectLocation::from_text(text),
        ProjectLocation::Local(PathBuf::from(text))
    );
}

#[cfg(unix)]
#[test]
fn a_non_utf8_local_path_keeps_its_bytes() {
    use std::os::unix::ffi::OsStrExt as _;
    let text = OsStr::from_bytes(b"/home/\xFF");
    assert_eq!(ProjectLocation::from_text(text).text(), text);
}

#[test]
fn a_remote_path_has_a_name_and_a_parent() {
    let path = RemotePath::new(b"/home/me/app/".to_vec()).unwrap();
    assert_eq!(path.file_name(), Some(&b"app"[..]));
    assert_eq!(path.parent().unwrap().as_bytes(), b"/home/me");
    let top = RemotePath::new(b"/app".to_vec()).unwrap();
    assert_eq!(top.parent().unwrap().as_bytes(), b"/");
    let root = RemotePath::new(b"/".to_vec()).unwrap();
    assert_eq!((root.file_name(), root.parent()), (None, None));
    assert!(RemotePath::new(b"rel".to_vec()).is_none());
    assert!(RemotePath::new(b"/a\0b".to_vec()).is_none());
}

#[test]
fn a_remote_location_has_no_hosts_yet() {
    let (host, path) = remote("ssh://neon/x");
    let location = ProjectLocation::Remote { host, path };
    let error = location.hosts().unwrap_err();
    assert_eq!(error.to_string(), "remote folders aren't supported yet");
    assert!(ProjectLocation::Local(PathBuf::from("/x")).hosts().is_ok());
}
