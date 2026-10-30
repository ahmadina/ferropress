//! Loads the lab fixture plugins on Ferropress and records the hook trace.

use std::fs;
use std::path::Path;
use std::rc::Rc;

use cms_bootstrap::{Layout, LoadPlugin, PluginFile, Snapshot, plan, run};
use cms_hooks::{DispatchKind, Hooks};
use cms_request::{Flags, Request};
use php_bindings::HookHost;
use php_compiler::compile;
use php_parser::{ParseOptions, parse};
use php_value::{Invoke, Slot, Value};
use php_vm::{Host, Session};
use test_harness::{
    FileLoad, HookEvent, HookKind, Method, Observation, Profile, Request as ObservedRequest,
    RequestPath, Response, Slug, Trace,
};

use crate::error::OrchestratorError;

/// Directory of the lab fixture plugins, relative to the repository root.
pub(crate) const FIXTURES: &str = "lab/app/fixtures";

/// SHA-256 of the empty body both sides use; this scenario compares hooks and files only.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

const MU_PREFIX: &str = "wp-content/mu-plugins/";
const PLUGIN_PREFIX: &str = "wp-content/plugins/";

/// One request's plugin-load observation on Ferropress.
pub(crate) fn observe(
    repo_root: &Path,
    name: &Slug,
    path: &RequestPath,
    profile: Profile,
) -> Result<Observation, OrchestratorError> {
    let request = Request::with_flags(flags(profile));
    let host = HookHost::for_request(&request);
    let mut loader = VmLoader {
        repo_root,
        host,
        files: Vec::new(),
        hooks: request.hooks(),
    };
    let steps = plan(request.flags(), &Layout::content(), &lab_snapshot());
    run(&request, &steps, &mut loader, &mut RejectCallbacks).map_err(boot_error)?;
    Ok(observation(
        name.clone(),
        path.clone(),
        request.hooks(),
        loader.files,
    ))
}

fn flags(profile: Profile) -> Flags {
    Flags {
        network: matches!(profile, Profile::Network),
        ..Flags::default()
    }
}

/// Active plugins the lab install records (`contracts/lifecycle`).
fn lab_snapshot() -> Snapshot {
    let mut snapshot = Snapshot {
        mu_files: vec![file("ferropress-fixture-mu.php"), file("readme.txt")],
        network_active: vec![file("ferropress-fixture-alpha.php")],
        site_active: vec![
            file("ferropress-fixture-alpha.php"),
            file("ferropress-fixture-beta/ferropress-fixture-beta.php"),
        ],
        hack_file: false,
        hacks_present: false,
        ..Snapshot::default()
    };
    snapshot
        .mu_present
        .insert(file("ferropress-fixture-mu.php"));
    snapshot
        .plugins_present
        .insert(file("ferropress-fixture-alpha.php"));
    snapshot
        .plugins_present
        .insert(file("ferropress-fixture-beta/ferropress-fixture-beta.php"));
    snapshot
}

fn file(path: &str) -> PluginFile {
    PluginFile::from(path)
}

struct VmLoader<'a> {
    repo_root: &'a Path,
    host: Rc<dyn Host>,
    files: Vec<FileLoad>,
    hooks: &'a Hooks,
}

impl LoadPlugin for VmLoader<'_> {
    type Error = OrchestratorError;

    fn load(&mut self, path: &[u8]) -> Result<(), Self::Error> {
        let hook_path = String::from_utf8_lossy(path).into_owned();
        self.files.push(FileLoad {
            path: hook_path.clone(),
            before_hook: u32::try_from(self.hooks.dispatches().len()).unwrap_or(u32::MAX),
        });
        let source_path = resolve(self.repo_root, path)?;
        let source = fs::read(&source_path).map_err(|source| OrchestratorError::Read {
            path: source_path,
            source,
        })?;
        run_php(&source, path, Rc::clone(&self.host), &hook_path)
    }
}

fn resolve(repo_root: &Path, hook_path: &[u8]) -> Result<std::path::PathBuf, OrchestratorError> {
    let rel = std::str::from_utf8(hook_path).map_err(|error| OrchestratorError::Load {
        path: String::from_utf8_lossy(hook_path).into_owned(),
        message: error.to_string(),
    })?;
    if rel.contains("..") {
        return Err(OrchestratorError::Load {
            path: rel.to_owned(),
            message: "path is not loadable".into(),
        });
    }
    let suffix = rel
        .strip_prefix(MU_PREFIX)
        .map(|rest| format!("mu-plugins/{rest}"))
        .or_else(|| {
            rel.strip_prefix(PLUGIN_PREFIX)
                .map(|rest| format!("plugins/{rest}"))
        })
        .ok_or_else(|| OrchestratorError::Load {
            path: rel.to_owned(),
            message: "path is outside the plugin directories".into(),
        })?;
    Ok(repo_root.join(FIXTURES).join(suffix))
}

fn run_php(
    source: &[u8],
    file: &[u8],
    host: Rc<dyn Host>,
    hook_path: &str,
) -> Result<(), OrchestratorError> {
    let fail = |message: String| OrchestratorError::Load {
        path: hook_path.to_owned(),
        message,
    };
    let parsed = parse(source, ParseOptions::default()).map_err(|error| fail(error.to_string()))?;
    let program = parsed.result.map_err(|error| fail(format!("{error:?}")))?;
    let compiled = compile(&program, file).map_err(|error| fail(error.to_string()))?;
    if let Some(fatal) = compiled.diagnostics.fatal {
        return Err(fail(String::from_utf8_lossy(&fatal.message).into_owned()));
    }
    let unit = compiled
        .unit
        .ok_or_else(|| fail("the compiler produced no unit".into()))?;
    let mut output = Vec::new();
    let session = Session::boot_with_host(&unit, file, &mut output, Some(host))
        .map_err(|error| fail(error.to_string()))?;
    if session.status() != 0 {
        return Err(fail(format!(
            "exit {} ({})",
            session.status(),
            String::from_utf8_lossy(&output)
        )));
    }
    Ok(())
}

struct RejectCallbacks;

impl Invoke for RejectCallbacks {
    type Error = OrchestratorError;

    fn invoke(&mut self, _callee: &Value, _args: &mut [Slot]) -> Result<Value, Self::Error> {
        Err(OrchestratorError::UnexpectedCallback)
    }
}

fn observation(name: Slug, path: RequestPath, hooks: &Hooks, files: Vec<FileLoad>) -> Observation {
    Observation {
        schema: Observation::SCHEMA,
        name,
        request: ObservedRequest {
            method: Method::Get,
            path,
        },
        response: Response {
            status: 200,
            headers: Vec::new(),
            body: String::new(),
            body_sha256: EMPTY_SHA256.to_owned(),
        },
        trace: Trace {
            schema: Trace::SCHEMA,
            hooks: hooks
                .dispatches()
                .into_iter()
                .map(|event| HookEvent {
                    kind: match event.kind {
                        DispatchKind::Action => HookKind::Action,
                        DispatchKind::Filter => HookKind::Filter,
                    },
                    name: String::from_utf8_lossy(event.name.as_bytes()).into_owned(),
                    args: event.args,
                    depth: event.depth,
                })
                .collect(),
            files,
            queries: Vec::new(),
            mail: Vec::new(),
            cron: Vec::new(),
        },
        database: test_harness::DatabaseDiff::new(),
    }
}

fn boot_error(
    error: cms_bootstrap::BootError<OrchestratorError, OrchestratorError>,
) -> OrchestratorError {
    match error {
        cms_bootstrap::BootError::Load(error) | cms_bootstrap::BootError::Invoke(error) => error,
        other => OrchestratorError::Parse {
            path: "boot".into(),
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn resolve_maps_hook_paths_onto_lab_fixtures() {
        let root = repo_root();
        let mu = resolve(
            root.as_path(),
            b"wp-content/mu-plugins/ferropress-fixture-mu.php",
        )
        .unwrap();
        assert!(mu.is_file(), "{}", mu.display());
        let plugin = resolve(
            root.as_path(),
            b"wp-content/plugins/ferropress-fixture-alpha.php",
        )
        .unwrap();
        assert!(plugin.is_file(), "{}", plugin.display());
        assert!(resolve(root.as_path(), b"wp-includes/plugin.php").is_err());
        assert!(resolve(root.as_path(), b"wp-content/plugins/../evil.php").is_err());
    }

    #[test]
    fn reject_callbacks_refuse_an_invoke() {
        assert!(matches!(
            RejectCallbacks.invoke(&Value::Null, &mut []),
            Err(OrchestratorError::UnexpectedCallback)
        ));
    }

    #[test]
    fn run_php_reports_compiler_and_status_failures() {
        let host = HookHost::for_request(&Request::new());
        let parse = run_php(b"<?php (", b"/t.php", Rc::clone(&host), "broken.php").unwrap_err();
        assert!(
            matches!(parse, OrchestratorError::Load { ref path, .. } if path == "broken.php"),
            "{parse}"
        );
        let boom = run_php(
            b"<?php throw new Exception(\"no\");",
            b"/t.php",
            host,
            "throw.php",
        )
        .unwrap_err();
        assert!(
            matches!(boom, OrchestratorError::Load { ref path, .. } if path == "throw.php"),
            "{boom}"
        );
    }
}
