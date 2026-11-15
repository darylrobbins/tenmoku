//! `.vscode/launch.json`, read only: see the module's docs above.

use std::collections::HashSet;

use fe_dap::HostOs;
use fe_dap::registry::JAVA;
use kiln_l10n::tr;
use serde_json::{Map, Value};

use super::{
    Keys, LAUNCH_JSON, LaunchConfig, LaunchDiagnostic, Project, Source, entry_name, parse_common,
    problem, read,
};

const JSON_KEYS: Keys = Keys {
    adapter: "type",
    stop_on_entry: "stopOnEntry",
    pre_launch_task: "preLaunchTask",
    source: Source::LaunchJson,
};

/// The keys that are typed fields, not passed through.
const TYPED: [&str; 9] = [
    "name",
    "type",
    "request",
    "program",
    "args",
    "cwd",
    "env",
    "stopOnEntry",
    "preLaunchTask",
];

/// A configuration's keys that are dropped, with a diagnostic.
const IGNORED: [&str; 3] = ["serverReadyAction", "postDebugTask", "presentation"];

/// The platform sub-objects, each for one OS.
const PLATFORMS: [(&str, HostOs); 3] = [
    ("osx", HostOs::MacOs),
    ("linux", HostOs::Linux),
    ("windows", HostOs::Windows),
];

/// The file's usable configurations, before the launch file's names and
/// blockers are applied: none if it isn't there.
pub(super) fn configs(
    project: &Project<'_>,
    diagnostics: &mut Vec<LaunchDiagnostic>,
) -> Vec<LaunchConfig> {
    let Some(text) = read(project, LAUNCH_JSON, diagnostics) else {
        return Vec::new();
    };
    let file_problem = |problem: String| LaunchDiagnostic::file(LAUNCH_JSON, problem);
    // VS Code saves with or without a byte order mark; JSON has none.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let json = match serde_json::from_str::<Value>(&fe_config::jsonc::strip_jsonc(text)) {
        Ok(json) => json,
        Err(error) => {
            diagnostics.push(file_problem(error.to_string()));
            return Vec::new();
        }
    };
    let Some(json) = json.as_object() else {
        diagnostics.push(file_problem(
            tr!("workbench-debug-launch-json-not-object").into(),
        ));
        return Vec::new();
    };
    if json.contains_key("compounds") {
        diagnostics.push(file_problem(
            tr!("workbench-debug-launch-ignored-key", key = "compounds").into(),
        ));
    }
    let Some(entries) = json.get("configurations") else {
        return Vec::new();
    };
    let Some(entries) = entries.as_array() else {
        diagnostics.push(file_problem(
            tr!("workbench-debug-launch-not-array", key = "configurations").into(),
        ));
        return Vec::new();
    };

    let mut configs = Vec::new();
    let mut names = HashSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let name = entry_name(entry, index);
        let report = |problem| LaunchDiagnostic::entry(LAUNCH_JSON, &name, problem);
        let Some(entry) = entry.as_object() else {
            let not_object = tr!("workbench-debug-launch-not-an-object");
            diagnostics.push(report(tr!(
                "workbench-debug-launch-left-out",
                problem = problem(not_object)
            )));
            continue;
        };
        let mut entry = entry.clone();
        for (key, os) in PLATFORMS {
            match entry.remove(key) {
                Some(Value::Object(over)) if os == project.os => lay_over(&mut entry, over),
                Some(Value::Object(_)) | None => {}
                Some(_) => diagnostics.push(report(tr!(
                    "workbench-debug-launch-platform-not-object",
                    key = key
                ))),
            }
        }
        for key in IGNORED {
            if entry.remove(key).is_some() {
                diagnostics.push(report(tr!("workbench-debug-launch-ignored-key", key = key)));
            }
        }
        // java-debug reads `args` as one string, which vscode-java-debug
        // lets a configuration give: passed as written.
        let java = (entry.get("type").and_then(Value::as_str))
            .and_then(|ty| project.adapters.name_for(ty))
            .is_some_and(|name| name == JAVA);
        let string_args = match entry.get("args") {
            Some(Value::String(_)) if java => entry.remove("args"),
            _ => None,
        };
        match parse_common(&entry, &JSON_KEYS, project) {
            Ok(mut config) => {
                config.adapter_args = entry
                    .into_iter()
                    .filter(|(key, _)| !TYPED.contains(&key.as_str()))
                    .collect();
                if let Some(args) = string_args {
                    config.adapter_args.insert("args".to_owned(), args);
                }
                if names.insert(config.name.clone()) {
                    configs.push(config);
                } else {
                    diagnostics.push(report(tr!("workbench-debug-launch-repeated-name")));
                }
            }
            Err(problem) => {
                diagnostics.push(report(tr!(
                    "workbench-debug-launch-left-out",
                    problem = problem
                )));
            }
        }
    }
    configs
}

/// `over`'s keys laid over `base`'s, an object over an object merged key by
/// key, as VS Code applies a platform's sub-object.
fn lay_over(base: &mut Map<String, Value>, over: Map<String, Value>) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(Value::Object(below)), Value::Object(above)) => lay_over(below, above),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}
