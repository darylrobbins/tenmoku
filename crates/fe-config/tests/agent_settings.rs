//! The `[agents]` settings: defaults, and a project file that can only
//! tighten them.

use fe_config::{
    Settings, SettingsStore,
    agent_settings::{AgentLevel, PROJECT_AGENTS},
};

fn load(user: &str, project: Option<&str>) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user_path = dir.path().join("settings.toml");
    std::fs::write(&user_path, user).unwrap();
    let project_path = project.map(|contents| {
        let path = dir.path().join("project.toml");
        std::fs::write(&path, contents).unwrap();
        path
    });
    SettingsStore::load(Some(user_path), project_path)
}

#[test]
fn agents_defaults_match_the_record() {
    let agents = Settings::default().agents;
    assert!(agents.tool_loop);
    assert_eq!(agents.max_rounds, 25);
    assert_eq!(agents.task_budget, 0);
    assert_eq!(agents.turn_time_limit_secs, 600);
    assert_eq!(agents.command_timeout_secs, 120);
    let p = &agents.permissions;
    assert_eq!(p.read, AgentLevel::Allow);
    assert_eq!(p.search, AgentLevel::Allow);
    assert_eq!(p.propose_edit, AgentLevel::Allow);
    assert_eq!(p.run_command, AgentLevel::Ask);
    assert_eq!(p.fetch, AgentLevel::Ask);
    assert_eq!(p.other, AgentLevel::Ask);
    assert!(p.commands_allow.is_empty() && p.commands_ask.is_empty() && p.commands_deny.is_empty());
    assert_eq!(agents.claude_code.adapter, "");
    assert_eq!(agents.claude_code.claude, "");
    assert!(agents.claude_code.sandbox_allow_write.is_empty());
    assert!(!agents.claude_code.mcp_servers);
    assert_eq!(agents.effective_permissions(), agents.permissions);

    let store = load(
        "[agents]\nmax_rounds = 50\ntask_budget = 20000\n[agents.permissions]\nrun_command = \"deny\"\ncommands_allow = [\"git status\"]\n",
        None,
    );
    assert_eq!(store.diagnostics(), []);
    let agents = &store.settings().agents;
    assert_eq!(agents.max_rounds, 50);
    assert_eq!(agents.task_budget, 20000);
    for ok in [0u64, 20_000, 200_000_000] {
        let store = load(&format!("[agents]\ntask_budget = {ok}\n"), None);
        assert_eq!(store.diagnostics(), [], "{ok}");
        assert_eq!(u64::from(store.settings().agents.task_budget), ok);
    }
    assert_eq!(agents.permissions.run_command, AgentLevel::Deny);
}

#[test]
fn agents_values_out_of_range_are_diagnostics() {
    for (source, key) in [
        ("[agents]\nmax_rounds = 0\n", "agents.max_rounds"),
        ("[agents]\nmax_rounds = 201\n", "agents.max_rounds"),
        ("[agents]\ntask_budget = 1\n", "agents.task_budget"),
        ("[agents]\ntask_budget = 19999\n", "agents.task_budget"),
        (
            "[agents]\nturn_time_limit_secs = 29\n",
            "agents.turn_time_limit_secs",
        ),
        (
            "[agents]\ncommand_timeout_secs = 0\n",
            "agents.command_timeout_secs",
        ),
        (
            "[agents.permissions]\nread = \"maybe\"\n",
            "agents.permissions.read",
        ),
    ] {
        let store = load(source, None);
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
        assert!(diagnostics[0].is_error(), "{:?}", diagnostics[0]);
        assert_eq!(diagnostics[0].key.as_deref(), Some(key), "{source}");
    }
}

#[test]
fn a_projects_permissions_move_and_only_tighten() {
    let store = load(
        "[agents.permissions]\nread = \"ask\"\nsearch = \"deny\"\nrun_command = \"allow\"\ncommands_ask = [\"git push\"]\ncommands_deny = [\"rm\"]\n",
        Some(
            "[agents.permissions]\nread = \"deny\"\nsearch = \"allow\"\nfetch = \"deny\"\ncommands_ask = [\"npm publish\"]\ncommands_deny = [\"rm\", \"curl\"]\n",
        ),
    );
    assert_eq!(store.diagnostics(), []);
    let agents = &store.settings().agents;
    // The user's own levels are untouched.
    assert_eq!(agents.permissions.read, AgentLevel::Ask);
    assert_eq!(agents.permissions.search, AgentLevel::Deny);
    assert_eq!(agents.permissions.fetch, AgentLevel::Ask);
    assert_eq!(agents.permissions.commands_ask, ["git push"]);
    assert_eq!(agents.project_permissions.read, Some(AgentLevel::Deny));
    assert_eq!(agents.project_permissions.fetch, Some(AgentLevel::Deny));

    let effective = agents.effective_permissions();
    assert_eq!(effective.read, AgentLevel::Deny); // tightened
    assert_eq!(effective.search, AgentLevel::Deny); // allow can't loosen deny
    assert_eq!(effective.fetch, AgentLevel::Deny);
    assert_eq!(effective.run_command, AgentLevel::Allow);
    assert_eq!(effective.commands_ask, ["git push", "npm publish"]);
    assert_eq!(effective.commands_deny, ["rm", "curl"]);
}

#[test]
fn a_project_cannot_allow_commands_or_set_the_rest() {
    let store = load(
        "",
        Some(
            "[agents]\ntool_loop = false\nmax_rounds = 200\n[agents.permissions]\ncommands_allow = [\"rm\"]\nrun_command = \"deny\"\n[agents.claude_code]\nadapter = \"/tmp/evil\"\nmcp_servers = true\n",
        ),
    );
    let agents = &store.settings().agents;
    let defaults = Settings::default().agents;
    assert!(agents.tool_loop);
    assert_eq!(agents.max_rounds, 25);
    assert_eq!(agents.permissions, defaults.permissions);
    assert_eq!(agents.claude_code, defaults.claude_code);
    assert_eq!(
        agents.project_permissions.run_command,
        Some(AgentLevel::Deny)
    );

    let mut keys: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| {
            assert!(!d.is_error(), "{d:?}");
            assert_eq!(d.message, PROJECT_AGENTS);
            d.key.clone().unwrap()
        })
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "agents.claude_code.adapter",
            "agents.claude_code.mcp_servers",
            "agents.max_rounds",
            "agents.permissions.commands_allow",
            "agents.tool_loop",
        ]
    );
}

#[test]
fn the_settings_model_shows_what_the_store_loads() {
    use fe_config::SettingsModel;
    let user = "[agents.permissions]\nread = \"ask\"\n";
    let project = "[agents]\nmax_rounds = 200\n[agents.permissions]\nread = \"deny\"\ncommands_allow = [\"rm\"]\ncommands_deny = [\"curl\"]\n[agents.claude_code]\nadapter = \"x\"\n";
    let dir = tempfile::tempdir().unwrap();
    let (user_path, project_path) = (dir.path().join("u.toml"), dir.path().join("p.toml"));
    std::fs::write(&user_path, user).unwrap();
    std::fs::write(&project_path, project).unwrap();
    let store = SettingsStore::load(Some(user_path), Some(project_path));
    let settings = serde_json::to_value(store.settings().as_ref()).unwrap();

    let model = SettingsModel::new(Some(user), Some(project));
    let mut checked = 0;
    for entry in model.entries() {
        if entry.key.first().map(String::as_str) != Some("agents") {
            continue;
        }
        let mut expected = &settings;
        for name in &entry.key {
            expected = expected.get(name).unwrap_or(&serde_json::Value::Null);
        }
        let value = model.value(entry).value;
        if !(expected.is_null() && value.is_null()) {
            assert_eq!(&value, expected, "{}", entry.key_path());
        }
        checked += 1;
        let allowed = model.workspace_allowed(entry);
        let key = entry.key_path();
        let tighten = key.starts_with("agents.permissions.") && !key.ends_with("commands_allow");
        assert_eq!(allowed, tighten, "{key}");
    }
    assert!(checked > 15);
}

#[test]
fn a_user_set_project_permissions_is_dropped_with_a_warning() {
    let store = load(
        "[agents.project_permissions]\nread = \"deny\"\ncommands_deny = [\"rm\"]\n",
        None,
    );
    let agents = &store.settings().agents;
    assert_eq!(agents.project_permissions, Default::default());
    assert_eq!(agents.effective_permissions(), agents.permissions);
    let diagnostics = store.diagnostics();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.key.as_deref() == Some("agents.project_permissions")
                && d.message == fe_config::agent_settings::PROJECT_PERMISSIONS_SET
                && !d.is_error()),
        "{diagnostics:#?}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|d| d.key.as_deref() == Some("agents.project_permissions")),
        "{diagnostics:#?}"
    );
}

#[test]
fn a_project_cannot_loosen_a_user_deny_in_any_class() {
    let classes = [
        "read",
        "search",
        "propose_edit",
        "run_command",
        "fetch",
        "other",
    ];
    for class in classes {
        let store = load(
            &format!("[agents.permissions]\n{class} = \"deny\"\n"),
            Some(&format!("[agents.permissions]\n{class} = \"allow\"\n")),
        );
        assert_eq!(store.diagnostics(), [], "{class}");
        let agents = &store.settings().agents;
        let effective = agents.effective_permissions();
        let level = match class {
            "read" => effective.read,
            "search" => effective.search,
            "propose_edit" => effective.propose_edit,
            "run_command" => effective.run_command,
            "fetch" => effective.fetch,
            _ => effective.other,
        };
        assert_eq!(level, AgentLevel::Deny, "{class}");
    }
    // A project that sets its own project_permissions can't skip narrowing
    // or replace what narrow_agents moves.
    let store = load(
        "[agents.permissions]\nread = \"deny\"\n",
        Some("[agents.project_permissions]\nread = \"allow\"\n"),
    );
    assert_eq!(store.settings().agents.project_permissions.read, None);
    assert_eq!(
        store.settings().agents.effective_permissions().read,
        AgentLevel::Deny
    );
}

#[test]
fn empty_project_lists_erase_nothing() {
    let store = load(
        "[agents.permissions]\ncommands_ask = [\"git push\"]\ncommands_deny = [\"rm\"]\n",
        Some("[agents.permissions]\ncommands_ask = []\ncommands_deny = []\n"),
    );
    assert_eq!(store.diagnostics(), []);
    let agents = &store.settings().agents;
    assert_eq!(agents.permissions.commands_ask, ["git push"]);
    assert_eq!(agents.permissions.commands_deny, ["rm"]);
    let effective = agents.effective_permissions();
    assert_eq!(effective.commands_ask, ["git push"]);
    assert_eq!(effective.commands_deny, ["rm"]);
}
