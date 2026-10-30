//! Filter and action dispatch (`contracts/hooks/dispatch.md`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use cms_hooks::Hooks;
use cms_types::{ArgCount, HookName, Priority};
use php_value::{Invoke, ObjectStore, PhpString, Reference, Slot, Value};

type Reply = Box<dyn Fn(&[Value]) -> Value>;

struct Script {
    replies: HashMap<String, Reply>,
}

impl Script {
    fn new() -> Self {
        Self {
            replies: HashMap::new(),
        }
    }

    fn on(&mut self, name: &str, reply: impl Fn(&[Value]) -> Value + 'static) {
        self.replies.insert(name.into(), Box::new(reply));
    }
}

impl Invoke for Script {
    type Error = String;

    fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
        let Value::String(name) = callee else {
            return Err("callee must be a function name".into());
        };
        let key = String::from_utf8_lossy(name.as_bytes()).into_owned();
        let values: Vec<Value> = args.iter().map(Slot::value).collect();
        let reply = self
            .replies
            .get(&key)
            .ok_or_else(|| format!("unknown {key}"))?;
        Ok(reply(&values))
    }
}

struct Nested<'a> {
    hooks: &'a Hooks,
    inner: HookName,
    seen: Rc<RefCell<Vec<String>>>,
}

impl Invoke for Nested<'_> {
    type Error = String;

    fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
        let Value::String(name) = callee else {
            return Err("name".into());
        };
        let current = self
            .hooks
            .current()
            .map(|held| String::from_utf8_lossy(held.as_bytes()).into_owned());
        self.seen.borrow_mut().push(format!(
            "{}@{current:?}",
            String::from_utf8_lossy(name.as_bytes())
        ));
        if name.as_bytes() == b"outer" {
            assert!(self.hooks.doing(&HookName::from("outer")));
            assert!(!self.hooks.doing(&self.inner));
            let hooks = self.hooks;
            let inner = self.inner.clone();
            let got = hooks.apply(&inner, Value::from("i"), args, self)?;
            assert_eq!(
                hooks.current().as_ref().map(HookName::as_bytes),
                Some(b"outer".as_slice())
            );
            return Ok(got);
        }
        assert!(self.hooks.doing(&HookName::from("outer")));
        assert!(self.hooks.doing(&self.inner));
        Ok(Value::from("ok"))
    }
}

enum Mid {
    AddSame,
    AddLater,
    RemoveLater,
    AddFresh,
}

struct During<'a> {
    hooks: &'a Hooks,
    inner: Script,
    mid: Mid,
}

impl Invoke for During<'_> {
    type Error = String;

    fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
        if matches!(callee, Value::String(name) if name.as_bytes() == b"first") {
            match self.mid {
                Mid::AddSame => add(
                    self.hooks,
                    "t",
                    "late",
                    Priority::DEFAULT,
                    ArgCount::DEFAULT,
                ),
                Mid::AddLater => add(
                    self.hooks,
                    "t",
                    "late",
                    Priority::new(20),
                    ArgCount::DEFAULT,
                ),
                Mid::RemoveLater => {
                    assert!(
                        self.hooks
                            .remove(&name("t"), &Value::from("gone"), Priority::new(20))
                    );
                }
                Mid::AddFresh => add(
                    self.hooks,
                    "t",
                    "fresh",
                    Priority::new(20),
                    ArgCount::DEFAULT,
                ),
            }
        }
        self.inner.invoke(callee, args)
    }
}

struct Probe<'a> {
    hooks: &'a Hooks,
    inner: Script,
}

impl Invoke for Probe<'_> {
    type Error = String;

    fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
        if matches!(callee, Value::String(name) if name.as_bytes() == b"watch") {
            assert_eq!(
                self.hooks.current().as_ref().map(HookName::as_bytes),
                Some(b"title".as_slice())
            );
        }
        self.inner.invoke(callee, args)
    }
}

struct Check;

impl Invoke for Check {
    type Error = String;

    fn invoke(&mut self, _callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
        match args.first() {
            Some(Slot::Reference(cell)) => {
                cell.set(Value::from("written"));
                Ok(Value::Null)
            }
            other => Err(format!("expected reference, got {other:?}")),
        }
    }
}

fn name(text: &str) -> HookName {
    HookName::from(text)
}

fn text(value: &Value) -> Result<String, String> {
    match value {
        Value::String(bytes) => Ok(String::from_utf8_lossy(bytes.as_bytes()).into_owned()),
        other => Err(format!("expected string, got {other:?}")),
    }
}

fn add(hooks: &Hooks, hook: &str, callee: &str, priority: Priority, accepted: ArgCount) {
    hooks.add(name(hook), Value::from(callee), priority, accepted);
}

#[test]
fn filters_run_in_priority_then_insertion_order() -> Result<(), String> {
    let hooks = Hooks::new();
    let mut script = Script::new();
    script.on("a", |args| {
        Value::from(format!("{}a", text(&args[0]).unwrap_or_default()).as_str())
    });
    script.on("b", |args| {
        Value::from(format!("{}b", text(&args[0]).unwrap_or_default()).as_str())
    });
    script.on("c", |args| {
        Value::from(format!("{}c", text(&args[0]).unwrap_or_default()).as_str())
    });
    add(&hooks, "title", "b", Priority::new(20), ArgCount::DEFAULT);
    add(&hooks, "title", "a", Priority::new(5), ArgCount::DEFAULT);
    add(&hooks, "title", "c", Priority::new(5), ArgCount::DEFAULT);
    let got = hooks.apply(&name("title"), Value::from("x"), &mut [], &mut script)?;
    assert_eq!(text(&got)?, "xacb");
    Ok(())
}

#[test]
fn the_same_callable_replaces_at_one_priority() -> Result<(), String> {
    let hooks = Hooks::new();
    let mut script = Script::new();
    script.on("once", |_| Value::from("first"));
    add(&hooks, "t", "once", Priority::DEFAULT, ArgCount::DEFAULT);
    script.on("once", |_| Value::from("second"));
    add(&hooks, "t", "once", Priority::DEFAULT, ArgCount::from(3));
    let got = hooks.apply(&name("t"), Value::from("x"), &mut [], &mut script)?;
    assert_eq!(text(&got)?, "second");
    assert!(hooks.has_at(&name("t"), &Value::from("once"), Priority::DEFAULT));
    assert!(!hooks.has_at(&name("t"), &Value::from("once"), Priority::new(20)));
    assert!(hooks.remove(&name("t"), &Value::from("once"), Priority::DEFAULT));
    assert!(!hooks.has(&name("t")));
    Ok(())
}

#[test]
fn an_action_does_not_feed_returns_forward() -> Result<(), String> {
    let hooks = Hooks::new();
    let mut script = Script::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let first = Rc::clone(&seen);
    script.on("one", move |args| {
        first.borrow_mut().push(text(&args[0]).unwrap_or_default());
        Value::from("changed")
    });
    let second = Rc::clone(&seen);
    script.on("two", move |args| {
        second.borrow_mut().push(text(&args[0]).unwrap_or_default());
        Value::from("changed")
    });
    add(&hooks, "boot", "one", Priority::DEFAULT, ArgCount::DEFAULT);
    add(&hooks, "boot", "two", Priority::DEFAULT, ArgCount::DEFAULT);
    hooks.action(
        &name("boot"),
        &mut [Slot::from(Value::from("init"))],
        &mut script,
    )?;
    assert_eq!(seen.borrow().as_slice(), ["init", "init"]);
    assert_eq!(hooks.action_count(&name("boot")), 1);
    assert_eq!(hooks.filter_count(&name("boot")), 0);
    Ok(())
}

#[test]
fn all_runs_before_the_named_hook() -> Result<(), String> {
    let hooks = Hooks::new();
    let mut script = Script::new();
    let order = Rc::new(RefCell::new(Vec::new()));
    let all = Rc::clone(&order);
    script.on("watch", move |args| {
        all.borrow_mut()
            .push(format!("all:{}", text(&args[0]).unwrap_or_default()));
        Value::from("ignored")
    });
    let named = Rc::clone(&order);
    script.on("edit", move |args| {
        named
            .borrow_mut()
            .push(format!("edit:{}", text(&args[0]).unwrap_or_default()));
        Value::from("done")
    });
    add(&hooks, "all", "watch", Priority::DEFAULT, ArgCount::from(2));
    add(
        &hooks,
        "title",
        "edit",
        Priority::DEFAULT,
        ArgCount::DEFAULT,
    );
    let got = hooks.apply(&name("title"), Value::from("raw"), &mut [], &mut script)?;
    assert_eq!(text(&got)?, "done");
    assert_eq!(order.borrow().as_slice(), ["all:title", "edit:raw"]);
    Ok(())
}

#[test]
fn accepted_args_slice_the_call() -> Result<(), String> {
    let hooks = Hooks::new();
    let mut script = Script::new();
    script.on("none", |args| {
        assert!(args.is_empty());
        Value::from("z")
    });
    script.on("two", |args| {
        Value::from(format!("{}+{}", text(&args[0]).unwrap_or_default(), args.len()).as_str())
    });
    add(&hooks, "t", "none", Priority::new(1), ArgCount::NONE);
    add(&hooks, "t", "two", Priority::new(2), ArgCount::from(2));
    let got = hooks.apply(
        &name("t"),
        Value::from("v"),
        &mut [Slot::from(Value::from("e")), Slot::from(Value::from("x"))],
        &mut script,
    )?;
    assert_eq!(text(&got)?, "z+2");
    Ok(())
}

#[test]
fn current_tracks_nested_applies() -> Result<(), String> {
    let hooks = Hooks::new();
    add(
        &hooks,
        "outer",
        "outer",
        Priority::DEFAULT,
        ArgCount::DEFAULT,
    );
    add(
        &hooks,
        "inner",
        "inner",
        Priority::DEFAULT,
        ArgCount::DEFAULT,
    );
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut invoke = Nested {
        hooks: &hooks,
        inner: name("inner"),
        seen: Rc::clone(&seen),
    };
    let got = hooks.apply(&name("outer"), Value::from("x"), &mut [], &mut invoke)?;
    assert_eq!(text(&got)?, "ok");
    assert_eq!(
        seen.borrow().as_slice(),
        ["outer@Some(\"outer\")", "inner@Some(\"inner\")"]
    );
    assert!(hooks.current().is_none());
    assert!(!hooks.doing(&name("outer")));
    Ok(())
}

#[test]
fn a_late_same_priority_add_misses_the_current_pass() -> Result<(), String> {
    let hooks = Hooks::new();
    let mut script = Script::new();
    script.on("first", |_| Value::from("1"));
    script.on("late", |_| Value::from("2"));
    add(&hooks, "t", "first", Priority::DEFAULT, ArgCount::DEFAULT);
    let mut invoke = During {
        hooks: &hooks,
        inner: script,
        mid: Mid::AddSame,
    };
    let got = hooks.apply(&name("t"), Value::from("x"), &mut [], &mut invoke)?;
    assert_eq!(text(&got)?, "1");
    assert!(hooks.has_at(&name("t"), &Value::from("late"), Priority::DEFAULT));
    Ok(())
}

#[test]
fn a_late_later_priority_add_runs_in_this_pass() -> Result<(), String> {
    let hooks = Hooks::new();
    let mut script = Script::new();
    script.on("first", |_| Value::from("1"));
    script.on("keep", |args| args[0].clone());
    script.on("late", |args| {
        Value::from(format!("{}2", text(&args[0]).unwrap_or_default()).as_str())
    });
    add(&hooks, "t", "first", Priority::new(5), ArgCount::DEFAULT);
    add(&hooks, "t", "keep", Priority::new(20), ArgCount::DEFAULT);
    let mut invoke = During {
        hooks: &hooks,
        inner: script,
        mid: Mid::AddLater,
    };
    let got = hooks.apply(&name("t"), Value::from("x"), &mut [], &mut invoke)?;
    assert_eq!(text(&got)?, "12");
    Ok(())
}

#[test]
fn a_late_later_priority_remove_misses_that_priority() -> Result<(), String> {
    let hooks = Hooks::new();
    let mut script = Script::new();
    script.on("first", |_| Value::from("1"));
    script.on("gone", |_| Value::from("2"));
    add(&hooks, "t", "first", Priority::new(5), ArgCount::DEFAULT);
    add(&hooks, "t", "gone", Priority::new(20), ArgCount::DEFAULT);
    let mut invoke = During {
        hooks: &hooks,
        inner: script,
        mid: Mid::RemoveLater,
    };
    let got = hooks.apply(&name("t"), Value::from("x"), &mut [], &mut invoke)?;
    assert_eq!(text(&got)?, "1");
    assert!(hooks.has_at(&name("t"), &Value::from("first"), Priority::new(5)));
    assert!(!hooks.has_at(&name("t"), &Value::from("gone"), Priority::new(20)));
    Ok(())
}

#[test]
fn empty_apply_still_counts() -> Result<(), String> {
    let hooks = Hooks::new();
    let mut script = Script::new();
    let got = hooks.apply(&name("missing"), Value::from("keep"), &mut [], &mut script)?;
    assert_eq!(text(&got)?, "keep");
    assert_eq!(hooks.filter_count(&name("missing")), 1);
    assert_eq!(hooks.action_count(&name("missing")), 0);
    assert!(!hooks.has(&name("missing")));
    Ok(())
}

#[test]
fn empty_action_still_counts() -> Result<(), String> {
    let hooks = Hooks::new();
    let mut script = Script::new();
    hooks.action(&name("missing"), &mut [], &mut script)?;
    assert_eq!(hooks.action_count(&name("missing")), 1);
    assert_eq!(hooks.filter_count(&name("missing")), 0);
    Ok(())
}

#[test]
fn remove_all_clears_one_priority() {
    let hooks = Hooks::new();
    add(&hooks, "t", "a", Priority::new(5), ArgCount::DEFAULT);
    add(&hooks, "t", "b", Priority::DEFAULT, ArgCount::DEFAULT);
    hooks.remove_all(&name("t"), Some(Priority::new(5)));
    assert!(hooks.has(&name("t")));
    assert!(!hooks.has_at(&name("t"), &Value::from("a"), Priority::new(5)));
    hooks.remove_all(&name("t"), None);
    assert!(!hooks.has(&name("t")));
}

#[test]
fn unnamed_callables_cannot_be_removed() {
    let hooks = Hooks::new();
    let mut script = Script::new();
    hooks.add(
        name("t"),
        Value::Int(1),
        Priority::DEFAULT,
        ArgCount::DEFAULT,
    );
    assert!(!hooks.remove(&name("t"), &Value::Int(1), Priority::DEFAULT));
    assert!(hooks.has(&name("t")));
    assert!(!hooks.has_at(&name("t"), &Value::Int(1), Priority::DEFAULT));
    script.on("unused", |_| Value::from("no"));
    assert!(
        hooks
            .apply(&name("t"), Value::from("x"), &mut [], &mut script)
            .is_err()
    );
}

#[test]
fn object_callables_match_by_identity() -> Result<(), String> {
    let hooks = Hooks::new();
    let store = ObjectStore::new();
    let first = store
        .create(PhpString::from("Closure"))
        .map_err(|error| error.to_string())?;
    let second = store
        .create(PhpString::from("Closure"))
        .map_err(|error| error.to_string())?;
    hooks.add(
        name("t"),
        Value::Object(first.clone()),
        Priority::DEFAULT,
        ArgCount::DEFAULT,
    );
    hooks.add(
        name("t"),
        Value::Object(second.clone()),
        Priority::new(20),
        ArgCount::DEFAULT,
    );
    assert!(hooks.has_at(&name("t"), &Value::Object(first.clone()), Priority::DEFAULT));
    assert!(hooks.remove(&name("t"), &Value::Object(first), Priority::DEFAULT));
    assert!(hooks.has_at(&name("t"), &Value::Object(second), Priority::new(20)));
    Ok(())
}

#[test]
fn current_is_the_target_name_during_all() -> Result<(), String> {
    let hooks = Hooks::new();
    add(&hooks, "all", "watch", Priority::DEFAULT, ArgCount::from(2));
    add(
        &hooks,
        "title",
        "edit",
        Priority::DEFAULT,
        ArgCount::DEFAULT,
    );
    let mut script = Script::new();
    script.on("watch", |_| Value::from("ignored"));
    script.on("edit", |args| args[0].clone());
    let mut invoke = Probe {
        hooks: &hooks,
        inner: script,
    };
    let got = hooks.apply(&name("title"), Value::from("raw"), &mut [], &mut invoke)?;
    assert_eq!(text(&got)?, "raw");
    Ok(())
}

#[test]
fn reference_slots_stay_references() -> Result<(), String> {
    let hooks = Hooks::new();
    add(&hooks, "t", "mark", Priority::DEFAULT, ArgCount::DEFAULT);
    let cell = Reference::new(Value::from("init"));
    hooks.action(&name("t"), &mut [Slot::from(cell.clone())], &mut Check)?;
    assert_eq!(text(&cell.get())?, "written");
    Ok(())
}

#[test]
fn a_new_priority_added_during_a_pass_is_skipped() -> Result<(), String> {
    let hooks = Hooks::new();
    let mut script = Script::new();
    script.on("first", |_| Value::from("1"));
    script.on("fresh", |_| Value::from("2"));
    add(&hooks, "t", "first", Priority::new(5), ArgCount::DEFAULT);
    let mut invoke = During {
        hooks: &hooks,
        inner: script,
        mid: Mid::AddFresh,
    };
    let got = hooks.apply(&name("t"), Value::from("x"), &mut [], &mut invoke)?;
    assert_eq!(text(&got)?, "1");
    assert!(hooks.has_at(&name("t"), &Value::from("fresh"), Priority::new(20)));
    Ok(())
}

#[test]
fn remove_of_a_missing_entry_is_false() {
    let hooks = Hooks::new();
    assert!(!hooks.remove(&name("t"), &Value::from("gone"), Priority::DEFAULT));
    hooks.remove_all(&name("missing"), None);
    add(&hooks, "t", "keep", Priority::DEFAULT, ArgCount::DEFAULT);
    assert!(!hooks.remove(&name("t"), &Value::from("keep"), Priority::new(20)));
    assert!(hooks.has(&name("t")));
}

#[test]
fn first_priority_is_the_lowest_stored() {
    let hooks = Hooks::new();
    add(&hooks, "t", "once", Priority::new(20), ArgCount::DEFAULT);
    add(&hooks, "t", "once", Priority::new(5), ArgCount::DEFAULT);
    assert_eq!(
        hooks.first_priority(&name("t"), &Value::from("once")),
        Some(Priority::new(5))
    );
    assert_eq!(
        hooks.first_priority(&name("t"), &Value::from("missing")),
        None
    );
}
