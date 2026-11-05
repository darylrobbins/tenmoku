// SPDX-License-Identifier: Apache-2.0

use kiln_command::capability::Target;
use kiln_command::{Capability, Grants};

fn cap(capability: &str) -> Capability {
    capability.parse().unwrap()
}

#[test]
fn capabilities_round_trip_t839_strings() {
    for capability in [
        "buffer.read",
        "buffer.write",
        "commands",
        "decorations",
        "status",
        "fs.read:workspace",
        "fs.write:workspace",
        "process.spawn:rg",
        "process.spawn:*",
        "net:api.anthropic.com",
        "net:*",
    ] {
        assert_eq!(cap(capability).to_string(), capability);
    }
    assert_eq!(
        cap("net:Api.Anthropic.COM"),
        Capability::Net(Target::Named("api.anthropic.com".into()))
    );
    assert_eq!(
        cap("net:Api.Anthropic.COM").to_string(),
        "net:api.anthropic.com"
    );

    for bad in [
        "net:https://x",
        "net:x:443",
        "net:x/y",
        "fs.read:home",
        "clipboard.read",
        "net:",
        "net",
        "net:*.example.com",
        "buffer.read:x",
        "process.spawn:",
    ] {
        let error = bad.parse::<Capability>().unwrap_err();
        assert_eq!(error.capability, bad);
        assert!(error.to_string().contains(bad), "{error}");
    }
}

#[test]
fn grants_allow_what_they_name() {
    let grants: Grants = [cap("buffer.read"), cap("net:api.anthropic.com")]
        .into_iter()
        .collect();
    assert!(grants.allows(&cap("buffer.read")));
    assert!(grants.allows(&cap("net:api.anthropic.com")));
    assert!(!grants.allows(&cap("net:example.com")));
    assert!(!grants.allows(&cap("buffer.write")));
    assert!(!grants.allows(&cap("net:*")));
    let hosts = grants.net_hosts();
    assert_eq!(hosts.hosts, ["api.anthropic.com"]);
    assert!(!hosts.any);

    let wildcard: Grants = [cap("net:*"), cap("net:api.anthropic.com")]
        .into_iter()
        .collect();
    assert!(wildcard.allows(&cap("net:example.com")));
    assert!(wildcard.allows(&cap("net:*")));
    assert!(!wildcard.allows(&cap("process.spawn:rg")));
    let hosts = wildcard.net_hosts();
    assert_eq!(hosts.hosts, ["api.anthropic.com"]);
    assert!(hosts.any);

    let spawn: Grants = [cap("process.spawn:*")].into_iter().collect();
    assert!(spawn.allows(&cap("process.spawn:rg")));
    assert!(!spawn.allows(&cap("net:example.com")));

    let json = serde_json::to_string(&grants).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        serde_json::json!(["buffer.read", "net:api.anthropic.com"])
    );
    assert_eq!(serde_json::from_str::<Grants>(&json).unwrap(), grants);
    let bad = serde_json::from_str::<Grants>(r#"["net:x:443"]"#).unwrap_err();
    assert!(bad.to_string().contains("net:x:443"), "{bad}");
}
