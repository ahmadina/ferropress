//! [`Host`] implementation that delegates to [`cms_hooks::Hooks`].

use std::rc::Rc;

use cms_hooks::Hooks;
use cms_request::Request;
use cms_types::{ArgCount, HookName, Priority};
use php_value::{Invoke, PhpString, Slot, Value};
use php_vm::{Host, SessionError};

use crate::convert::{accepted, hook_name, is_absent_callback, is_absent_priority, priority};

const NAMES: &[&[u8]] = &[
    b"add_action",
    b"add_filter",
    b"apply_filters",
    b"current_action",
    b"current_filter",
    b"did_action",
    b"did_filter",
    b"do_action",
    b"doing_action",
    b"doing_filter",
    b"has_action",
    b"has_filter",
    b"remove_action",
    b"remove_all_actions",
    b"remove_all_filters",
    b"remove_filter",
];

/// Host functions that store and run callbacks on one request's hook table.
#[derive(Debug)]
pub struct HookHost {
    hooks: Rc<Hooks>,
}

impl HookHost {
    /// Binds the hook table of `request`.
    #[must_use]
    pub fn for_request(request: &Request) -> Rc<dyn Host> {
        Rc::new(Self {
            hooks: request.shared_hooks(),
        })
    }
}

impl Host for HookHost {
    fn exists(&self, name: &[u8]) -> bool {
        NAMES.contains(&name)
    }

    fn call(
        &self,
        name: &[u8],
        args: &mut [Slot],
        invoke: &mut dyn Invoke<Error = SessionError>,
    ) -> Result<Value, SessionError> {
        match name {
            b"add_filter" | b"add_action" => Ok(add(&self.hooks, args)),
            b"remove_filter" | b"remove_action" => Ok(remove(&self.hooks, args)),
            b"remove_all_filters" | b"remove_all_actions" => Ok(remove_all(&self.hooks, args)),
            b"has_filter" | b"has_action" => Ok(has(&self.hooks, args)),
            b"apply_filters" => apply(&self.hooks, args, invoke),
            b"do_action" => action(&self.hooks, args, invoke),
            b"current_filter" | b"current_action" => Ok(current(&self.hooks)),
            b"doing_filter" | b"doing_action" => Ok(doing(&self.hooks, args)),
            b"did_filter" => Ok(count(&self.hooks, args, true)),
            b"did_action" => Ok(count(&self.hooks, args, false)),
            _ => Ok(Value::Null),
        }
    }
}

fn add(hooks: &Hooks, args: &mut [Slot]) -> Value {
    let values = taken(args);
    let Some(name) = values.first().and_then(|value| hook_name(value).ok()) else {
        return Value::Bool(true);
    };
    let callee = values.get(1).cloned().unwrap_or(Value::Null);
    let stored_priority = priority(values.get(2)).unwrap_or(Priority::DEFAULT);
    let stored_accepted = accepted(values.get(3)).unwrap_or(ArgCount::DEFAULT);
    hooks.add(name, callee, stored_priority, stored_accepted);
    Value::Bool(true)
}

fn remove(hooks: &Hooks, args: &mut [Slot]) -> Value {
    let values = taken(args);
    let Some(name) = values.first().and_then(|value| hook_name(value).ok()) else {
        return Value::Bool(false);
    };
    let callee = values.get(1).cloned().unwrap_or(Value::Null);
    let stored_priority = priority(values.get(2)).unwrap_or(Priority::DEFAULT);
    Value::Bool(hooks.remove(&name, &callee, stored_priority))
}

fn remove_all(hooks: &Hooks, args: &mut [Slot]) -> Value {
    let values = taken(args);
    let Some(name) = values.first().and_then(|value| hook_name(value).ok()) else {
        return Value::Bool(true);
    };
    let stored = if is_absent_priority(values.get(2)) {
        None
    } else {
        priority(values.get(2)).ok()
    };
    hooks.remove_all(&name, stored);
    Value::Bool(true)
}

fn has(hooks: &Hooks, args: &mut [Slot]) -> Value {
    let values = taken(args);
    let Some(name) = values.first().and_then(|value| hook_name(value).ok()) else {
        return Value::Bool(false);
    };
    if is_absent_callback(values.get(1)) {
        return Value::Bool(hooks.has(&name));
    }
    let callee = values.get(1).cloned().unwrap_or(Value::Null);
    match hooks.first_priority(&name, &callee) {
        Some(priority) => Value::Int(i64::from(priority.get())),
        None => Value::Bool(false),
    }
}

fn apply(
    hooks: &Hooks,
    args: &mut [Slot],
    invoke: &mut dyn Invoke<Error = SessionError>,
) -> Result<Value, SessionError> {
    let name = name_at(args, 0);
    let value = args.get(1).map_or(Value::Null, Slot::value);
    let start = 2.min(args.len());
    let mut extra: Vec<Slot> = args[start..].iter().map(copy_slot).collect();
    hooks.apply(&name, value, &mut extra, invoke)
}

fn action(
    hooks: &Hooks,
    args: &mut [Slot],
    invoke: &mut dyn Invoke<Error = SessionError>,
) -> Result<Value, SessionError> {
    let name = name_at(args, 0);
    let start = 1.min(args.len());
    let mut extra: Vec<Slot> = args[start..].iter().map(copy_slot).collect();
    hooks.action(&name, &mut extra, invoke)?;
    Ok(Value::Null)
}

fn current(hooks: &Hooks) -> Value {
    match hooks.current() {
        Some(name) => Value::String(PhpString::new(name.as_bytes())),
        None => Value::Bool(false),
    }
}

fn doing(hooks: &Hooks, args: &mut [Slot]) -> Value {
    if is_absent_priority(args.first().map(Slot::value).as_ref()) {
        return Value::Bool(hooks.current().is_some());
    }
    Value::Bool(hooks.doing(&name_at(args, 0)))
}

fn count(hooks: &Hooks, args: &mut [Slot], filters: bool) -> Value {
    let name = name_at(args, 0);
    let n = if filters {
        hooks.filter_count(&name)
    } else {
        hooks.action_count(&name)
    };
    Value::Int(i64::from(n))
}

fn name_at(args: &[Slot], index: usize) -> HookName {
    args.get(index)
        .map(Slot::value)
        .and_then(|value| hook_name(&value).ok())
        .unwrap_or_else(|| HookName::from(""))
}

fn taken(args: &[Slot]) -> Vec<Value> {
    args.iter().map(Slot::value).collect()
}

fn copy_slot(slot: &Slot) -> Slot {
    match slot {
        Slot::Value(value) => Slot::from(value.clone()),
        Slot::Reference(cell) => Slot::Reference(cell.clone()),
    }
}
