//! Plugin-load subsequence of a recorded request lifecycle.
//!
//! The full lifecycle includes core files Ferropress does not load yet. This module keeps the
//! must-use and site plugin loads and the load actions the lab fired around them
//! (`contracts/bootstrap/plugin-load.md`).

use serde::Deserialize;
use test_harness::{FileLoad, HookEvent, HookKind};

use crate::error::OrchestratorError;

const PLUGIN_ACTIONS: &[&str] = &[
    "mu_plugin_loaded",
    "network_plugin_loaded",
    "plugin_loaded",
    "muplugins_loaded",
    "plugins_loaded",
];

/// One lifecycle event as written by `cargo xtask lab lifecycle`.
#[derive(Debug, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase", deny_unknown_fields)]
enum Event {
    Load { path: String },
    Action { name: String, args: u32, depth: u32 },
}

/// Plugin files and load actions extracted from a lifecycle document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginLoad {
    pub(crate) hooks: Vec<HookEvent>,
    pub(crate) files: Vec<FileLoad>,
}

/// Reads the plugin-load subsequence from `text`, a lifecycle JSONL document.
pub(crate) fn extract(id: &str, text: &str) -> Result<PluginLoad, OrchestratorError> {
    let mut hooks = Vec::new();
    let mut files = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index == 0 || line.is_empty() {
            continue;
        }
        let event: Event =
            serde_json::from_str(line).map_err(|error| OrchestratorError::Parse {
                path: format!("{id}:{index}"),
                message: error.to_string(),
            })?;
        match event {
            Event::Load { path } if is_plugin_file(&path) => files.push(FileLoad {
                path,
                before_hook: u32::try_from(hooks.len()).unwrap_or(u32::MAX),
            }),
            Event::Action { name, args, depth } if is_plugin_action(&name) => {
                hooks.push(HookEvent {
                    kind: HookKind::Action,
                    name,
                    args,
                    depth,
                });
            }
            _ => {}
        }
    }
    if hooks.is_empty() && files.is_empty() {
        return Err(OrchestratorError::EmptyLifecycle(id.to_owned()));
    }
    Ok(PluginLoad { hooks, files })
}

fn is_plugin_file(path: &str) -> bool {
    path.as_bytes().ends_with(b".php")
        && (path.starts_with("wp-content/mu-plugins/") || path.starts_with("wp-content/plugins/"))
        && !path.contains("..")
}

fn is_plugin_action(name: &str) -> bool {
    PLUGIN_ACTIONS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLICE: &str = r#"{"schema":1,"platform_version":"7.1.3","id":"x","profile":"single","path":"/"}
{"event":"load","path":"wp-includes/vars.php"}
{"event":"action","name":"init","args":0,"depth":1}
{"event":"load","path":"wp-content/mu-plugins/ferropress-fixture-mu.php"}
{"event":"action","name":"mu_plugin_loaded","args":1,"depth":1}
{"event":"action","name":"muplugins_loaded","args":0,"depth":1}
{"event":"load","path":"wp-admin/includes/plugin.php"}
{"event":"load","path":"wp-content/plugins/ferropress-fixture-alpha.php"}
{"event":"action","name":"plugin_loaded","args":1,"depth":1}
{"event":"action","name":"plugins_loaded","args":0,"depth":1}
{"event":"action","name":"init","args":0,"depth":1}
"#;

    #[test]
    fn extract_keeps_plugin_files_and_load_actions() {
        let got = extract("x", SLICE).unwrap();
        assert_eq!(
            got.files
                .iter()
                .map(|file| (file.path.as_str(), file.before_hook))
                .collect::<Vec<_>>(),
            [
                ("wp-content/mu-plugins/ferropress-fixture-mu.php", 0),
                ("wp-content/plugins/ferropress-fixture-alpha.php", 2),
            ]
        );
        assert_eq!(
            got.hooks
                .iter()
                .map(|hook| (hook.name.as_str(), hook.args, hook.depth))
                .collect::<Vec<_>>(),
            [
                ("mu_plugin_loaded", 1, 1),
                ("muplugins_loaded", 0, 1),
                ("plugin_loaded", 1, 1),
                ("plugins_loaded", 0, 1),
            ]
        );
    }

    #[test]
    fn extract_rejects_an_empty_or_broken_lifecycle() {
        assert!(matches!(
            extract("empty", "{\"schema\":1}\n"),
            Err(OrchestratorError::EmptyLifecycle(_))
        ));
        assert!(matches!(
            extract("bad", "{\"schema\":1}\nnot-json\n"),
            Err(OrchestratorError::Parse { .. })
        ));
    }
}
