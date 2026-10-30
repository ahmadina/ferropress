//! Plugin load order (`contracts/bootstrap/plugin-load.md`).

use std::cell::RefCell;
use std::rc::Rc;

use cms_bootstrap::{
    Layout, LoadKind, LoadPlugin, LoadStep, PluginFile, Snapshot, plan, run, sort_active,
};
use cms_request::{Flags, Request};
use cms_types::{ArgCount, HookName, Priority};
use php_value::{Invoke, Slot, Value};

struct Log {
    order: Rc<RefCell<Vec<String>>>,
}

impl Invoke for Log {
    type Error = String;

    fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
        let Value::String(name) = callee else {
            return Err("name".into());
        };
        let extra = args.first().map(|slot| match slot.value() {
            Value::String(bytes) => String::from_utf8_lossy(bytes.as_bytes()).into_owned(),
            _ => String::new(),
        });
        self.order.borrow_mut().push(match extra {
            Some(path) if !path.is_empty() => {
                format!("{}:{path}", String::from_utf8_lossy(name.as_bytes()))
            }
            _ => String::from_utf8_lossy(name.as_bytes()).into_owned(),
        });
        Ok(Value::Null)
    }
}

struct Recorder {
    loaded: Rc<RefCell<Vec<String>>>,
}

impl LoadPlugin for Recorder {
    type Error = String;

    fn load(&mut self, path: &[u8]) -> Result<(), Self::Error> {
        self.loaded
            .borrow_mut()
            .push(String::from_utf8_lossy(path).into_owned());
        Ok(())
    }
}

fn file(path: &str) -> PluginFile {
    PluginFile::from(path)
}

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

fn names(steps: &[LoadStep]) -> Vec<(LoadKind, String)> {
    steps
        .iter()
        .map(|step| {
            (
                step.kind,
                String::from_utf8_lossy(&step.hook_path).into_owned(),
            )
        })
        .collect()
}

#[test]
fn single_site_matches_the_lab_front_page() {
    let steps = plan(Flags::default(), &Layout::content(), &lab_snapshot());
    assert_eq!(
        names(&steps),
        [
            (
                LoadKind::MustUse,
                "wp-content/mu-plugins/ferropress-fixture-mu.php".into()
            ),
            (
                LoadKind::Site,
                "wp-content/plugins/ferropress-fixture-alpha.php".into()
            ),
            (
                LoadKind::Site,
                "wp-content/plugins/ferropress-fixture-beta/ferropress-fixture-beta.php".into()
            ),
        ]
    );
}

#[test]
fn network_loads_the_shared_plugin_once() {
    let flags = Flags {
        network: true,
        ..Flags::default()
    };
    let steps = plan(flags, &Layout::content(), &lab_snapshot());
    assert_eq!(
        names(&steps),
        [
            (
                LoadKind::MustUse,
                "wp-content/mu-plugins/ferropress-fixture-mu.php".into()
            ),
            (
                LoadKind::Network,
                "wp-content/plugins/ferropress-fixture-alpha.php".into()
            ),
            (
                LoadKind::Site,
                "wp-content/plugins/ferropress-fixture-beta/ferropress-fixture-beta.php".into()
            ),
        ]
    );
}

#[test]
fn activating_re_sorts_the_site_list() {
    let stored = sort_active(vec![file("z.php"), file("a.php")]);
    assert_eq!(stored, [file("a.php"), file("z.php")]);
}

#[test]
fn installing_skips_site_plugins() {
    let flags = Flags {
        installing: true,
        ..Flags::default()
    };
    let steps = plan(flags, &Layout::content(), &lab_snapshot());
    assert_eq!(
        names(&steps),
        [(
            LoadKind::MustUse,
            "wp-content/mu-plugins/ferropress-fixture-mu.php".into()
        )]
    );
}

#[test]
fn recovery_skips_paused_plugins() {
    let mut snapshot = lab_snapshot();
    snapshot.paused.insert(file("ferropress-fixture-alpha.php"));
    let flags = Flags {
        recovery: true,
        ..Flags::default()
    };
    let steps = plan(flags, &Layout::content(), &snapshot);
    assert_eq!(
        names(&steps),
        [
            (
                LoadKind::MustUse,
                "wp-content/mu-plugins/ferropress-fixture-mu.php".into()
            ),
            (
                LoadKind::Site,
                "wp-content/plugins/ferropress-fixture-beta/ferropress-fixture-beta.php".into()
            ),
        ]
    );
}

#[test]
fn hacks_file_loads_before_site_plugins() {
    let mut snapshot = lab_snapshot();
    snapshot.hack_file = true;
    snapshot.hacks_present = true;
    let steps = plan(Flags::default(), &Layout::content(), &snapshot);
    assert_eq!(steps[1].kind, LoadKind::Hacks);
    assert_eq!(steps[1].hook_path.as_ref(), b"my-hacks.php");
    assert_eq!(steps[2].kind, LoadKind::Site);
}

#[test]
fn invalid_or_missing_plugins_are_skipped() {
    let mut snapshot = Snapshot {
        site_active: vec![
            file("../evil.php"),
            file("/abs.php"),
            file("gone.php"),
            file("ok.php"),
        ],
        ..Snapshot::default()
    };
    snapshot.plugins_present.insert(file("ok.php"));
    let steps = plan(Flags::default(), &Layout::content(), &snapshot);
    assert_eq!(
        names(&steps),
        [(LoadKind::Site, "wp-content/plugins/ok.php".into())]
    );
}

#[test]
fn run_fires_group_actions_in_order() -> Result<(), String> {
    let request = Request::new();
    let paths = Rc::new(RefCell::new(Vec::new()));
    let order = Rc::new(RefCell::new(Vec::new()));
    request.hooks().add(
        HookName::from("mu_plugin_loaded"),
        Value::from("mu"),
        Priority::DEFAULT,
        ArgCount::DEFAULT,
    );
    request.hooks().add(
        HookName::from("plugin_loaded"),
        Value::from("site"),
        Priority::DEFAULT,
        ArgCount::DEFAULT,
    );
    request.hooks().add(
        HookName::from("muplugins_loaded"),
        Value::from("mu_done"),
        Priority::DEFAULT,
        ArgCount::NONE,
    );
    request.hooks().add(
        HookName::from("plugins_loaded"),
        Value::from("site_done"),
        Priority::DEFAULT,
        ArgCount::NONE,
    );
    let steps = plan(Flags::default(), &Layout::content(), &lab_snapshot());
    let mut loader = Recorder {
        loaded: Rc::clone(&paths),
    };
    run(
        &request,
        &steps,
        &mut loader,
        &mut Log {
            order: Rc::clone(&order),
        },
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        paths.borrow().as_slice(),
        [
            "wp-content/mu-plugins/ferropress-fixture-mu.php",
            "wp-content/plugins/ferropress-fixture-alpha.php",
            "wp-content/plugins/ferropress-fixture-beta/ferropress-fixture-beta.php",
        ]
    );
    assert_eq!(
        order.borrow().as_slice(),
        [
            "mu:wp-content/mu-plugins/ferropress-fixture-mu.php",
            "mu_done",
            "site:wp-content/plugins/ferropress-fixture-alpha.php",
            "site:wp-content/plugins/ferropress-fixture-beta/ferropress-fixture-beta.php",
            "site_done",
        ]
    );
    Ok(())
}
