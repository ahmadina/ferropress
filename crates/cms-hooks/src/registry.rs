//! The request-local hook table.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use cms_types::{ArgCount, HookName, Priority};
use php_value::{Invoke, PhpString, Slot, Value};

use crate::identity::Identity;

/// Callbacks stored for one request, keyed by hook name.
///
/// Registration and dispatch take `&self` so a callback may add, remove or apply again on the same
/// table (ADR-0003). The table holds [`Value`] handles and is not [`Send`].
/// Contract: `contracts/hooks/dispatch.md`.
#[derive(Debug, Default)]
pub struct Hooks {
    inner: RefCell<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    chains: HashMap<HookName, Chain>,
    current: Vec<HookName>,
    filters: HashMap<HookName, u32>,
    actions: HashMap<HookName, u32>,
    next_unique: u64,
}

#[derive(Debug, Default)]
struct Chain {
    by_priority: BTreeMap<Priority, Vec<Entry>>,
}

#[derive(Clone, Debug)]
struct Entry {
    id: Identity,
    callee: Value,
    accepted: ArgCount,
}

impl Hooks {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `callee` on `name` at `priority`, replacing a matching entry in place.
    pub fn add(&self, name: HookName, callee: Value, priority: Priority, accepted: ArgCount) {
        self.inner
            .borrow_mut()
            .add(name, callee, priority, accepted);
    }

    /// Removes the matching callable at `priority`. Reports whether it was stored.
    #[must_use]
    pub fn remove(&self, name: &HookName, callee: &Value, priority: Priority) -> bool {
        self.inner.borrow_mut().remove(name, callee, priority)
    }

    /// Clears every callback on `name`, or only those at `priority`.
    pub fn remove_all(&self, name: &HookName, priority: Option<Priority>) {
        self.inner.borrow_mut().remove_all(name, priority);
    }

    /// Whether any callback is stored on `name`.
    #[must_use]
    pub fn has(&self, name: &HookName) -> bool {
        self.inner.borrow().has(name)
    }

    /// Whether `callee` is stored on `name` at `priority`.
    #[must_use]
    pub fn has_at(&self, name: &HookName, callee: &Value, priority: Priority) -> bool {
        self.inner.borrow().has_at(name, callee, priority)
    }

    /// The lowest priority at which `callee` is stored on `name`.
    #[must_use]
    pub fn first_priority(&self, name: &HookName, callee: &Value) -> Option<Priority> {
        self.inner.borrow().first_priority(name, callee)
    }

    /// The hook whose callbacks are running, if any.
    #[must_use]
    pub fn current(&self) -> Option<HookName> {
        self.inner.borrow().current.last().cloned()
    }

    /// Whether `name` is on the running stack.
    #[must_use]
    pub fn doing(&self, name: &HookName) -> bool {
        self.inner.borrow().current.iter().any(|held| held == name)
    }

    /// How many times `name` has been applied, including empty walks.
    #[must_use]
    pub fn filter_count(&self, name: &HookName) -> u32 {
        self.inner.borrow().filters.get(name).copied().unwrap_or(0)
    }

    /// How many times `name` has been acted, including empty walks.
    #[must_use]
    pub fn action_count(&self, name: &HookName) -> u32 {
        self.inner.borrow().actions.get(name).copied().unwrap_or(0)
    }

    /// Runs filter callbacks on `name` and returns the last value.
    ///
    /// # Errors
    ///
    /// The [`Invoke::Error`] of `I` when a callback cannot be invoked.
    ///
    /// # Examples
    ///
    /// ```
    /// use cms_hooks::Hooks;
    /// use cms_types::{ArgCount, HookName, Priority};
    /// use php_value::{Invoke, Slot, Value};
    ///
    /// struct Echo;
    ///
    /// impl Invoke for Echo {
    ///     type Error = &'static str;
    ///
    ///     fn invoke(&mut self, _callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
    ///         Ok(args.first().map_or(Value::Null, Slot::value))
    ///     }
    /// }
    ///
    /// let hooks = Hooks::new();
    /// hooks.add(
    ///     HookName::from("title"),
    ///     Value::from("id"),
    ///     Priority::DEFAULT,
    ///     ArgCount::DEFAULT,
    /// );
    /// let got = hooks.apply(&HookName::from("title"), Value::from("x"), &mut [], &mut Echo)?;
    /// assert!(matches!(got, Value::String(_)));
    /// # Ok::<(), &'static str>(())
    /// ```
    pub fn apply<I: Invoke + ?Sized>(
        &self,
        name: &HookName,
        value: Value,
        extra: &mut [Slot],
        invoke: &mut I,
    ) -> Result<Value, I::Error> {
        bump(&mut self.inner.borrow_mut().filters, name);
        self.run(name, value, extra, true, invoke)
    }

    /// Runs action callbacks on `name`. Returns do not feed the next callback.
    ///
    /// # Errors
    ///
    /// The [`Invoke::Error`] of `I` when a callback cannot be invoked.
    pub fn action<I: Invoke + ?Sized>(
        &self,
        name: &HookName,
        args: &mut [Slot],
        invoke: &mut I,
    ) -> Result<(), I::Error> {
        bump(&mut self.inner.borrow_mut().actions, name);
        self.run(name, Value::Null, args, false, invoke).map(drop)
    }

    fn run<I: Invoke + ?Sized>(
        &self,
        name: &HookName,
        mut value: Value,
        rest: &mut [Slot],
        replace: bool,
        invoke: &mut I,
    ) -> Result<Value, I::Error> {
        let all = HookName::new(HookName::ALL);
        let has_all = self.has(&all);
        if has_all {
            self.inner.borrow_mut().current.push(name.clone());
            let mut leading = all_args(name, &value, rest, replace);
            self.walk(&all, Value::Null, &mut leading, false, invoke)?;
        }
        if !self.has(name) {
            if has_all {
                self.inner.borrow_mut().current.pop();
            }
            return Ok(value);
        }
        if !has_all {
            self.inner.borrow_mut().current.push(name.clone());
        }
        value = self.walk(name, value, rest, replace, invoke)?;
        self.inner.borrow_mut().current.pop();
        Ok(value)
    }

    fn walk<I: Invoke + ?Sized>(
        &self,
        name: &HookName,
        mut value: Value,
        rest: &mut [Slot],
        replace: bool,
        invoke: &mut I,
    ) -> Result<Value, I::Error> {
        let priorities = self.inner.borrow().priorities(name);
        for priority in priorities {
            let entries = self.inner.borrow().entries_at(name, priority);
            for entry in entries {
                let mut args = bind_args(&value, rest, entry.accepted, replace);
                let returned = invoke.invoke(&entry.callee, &mut args)?;
                if replace {
                    value = returned;
                }
            }
        }
        Ok(value)
    }
}

impl Inner {
    fn add(&mut self, name: HookName, callee: Value, priority: Priority, accepted: ArgCount) {
        let id = Identity::of(&callee, &mut self.next_unique);
        let list = self
            .chains
            .entry(name)
            .or_default()
            .by_priority
            .entry(priority)
            .or_default();
        if let Some(existing) = list.iter_mut().find(|entry| entry.id == id) {
            existing.callee = callee;
            existing.accepted = accepted;
            return;
        }
        list.push(Entry {
            id,
            callee,
            accepted,
        });
    }

    fn remove(&mut self, name: &HookName, callee: &Value, priority: Priority) -> bool {
        let Some(id) = Identity::named(callee) else {
            return false;
        };
        let Some(chain) = self.chains.get_mut(name) else {
            return false;
        };
        let Some(list) = chain.by_priority.get_mut(&priority) else {
            return false;
        };
        let Some(index) = list.iter().position(|entry| entry.id == id) else {
            return false;
        };
        list.remove(index);
        if list.is_empty() {
            chain.by_priority.remove(&priority);
        }
        if chain.by_priority.is_empty() {
            self.chains.remove(name);
        }
        true
    }

    fn remove_all(&mut self, name: &HookName, priority: Option<Priority>) {
        let Some(chain) = self.chains.get_mut(name) else {
            return;
        };
        match priority {
            Some(priority) => {
                chain.by_priority.remove(&priority);
            }
            None => chain.by_priority.clear(),
        }
        if chain.by_priority.is_empty() {
            self.chains.remove(name);
        }
    }

    fn has(&self, name: &HookName) -> bool {
        self.chains
            .get(name)
            .is_some_and(|chain| chain.by_priority.values().any(|list| !list.is_empty()))
    }

    fn has_at(&self, name: &HookName, callee: &Value, priority: Priority) -> bool {
        let Some(id) = Identity::named(callee) else {
            return false;
        };
        self.chains.get(name).is_some_and(|chain| {
            chain
                .by_priority
                .get(&priority)
                .is_some_and(|list| list.iter().any(|entry| entry.id == id))
        })
    }

    fn first_priority(&self, name: &HookName, callee: &Value) -> Option<Priority> {
        let id = Identity::named(callee)?;
        self.chains.get(name).and_then(|chain| {
            chain.by_priority.iter().find_map(|(priority, list)| {
                list.iter().any(|entry| entry.id == id).then_some(*priority)
            })
        })
    }

    fn priorities(&self, name: &HookName) -> Vec<Priority> {
        self.chains.get(name).map_or_else(Vec::new, |chain| {
            chain.by_priority.keys().copied().collect()
        })
    }

    fn entries_at(&self, name: &HookName, priority: Priority) -> Vec<Entry> {
        self.chains
            .get(name)
            .and_then(|chain| chain.by_priority.get(&priority))
            .map_or_else(Vec::new, ToOwned::to_owned)
    }
}

fn bump(counts: &mut HashMap<HookName, u32>, name: &HookName) {
    *counts.entry(name.clone()).or_insert(0) += 1;
}

fn all_args(name: &HookName, value: &Value, rest: &[Slot], replace: bool) -> Vec<Slot> {
    let mut args = vec![Slot::from(Value::String(PhpString::new(name.as_bytes())))];
    if replace {
        args.push(Slot::from(value.clone()));
    }
    args.extend(rest.iter().map(copy_slot));
    args
}

fn bind_args(value: &Value, rest: &[Slot], accepted: ArgCount, replace: bool) -> Vec<Slot> {
    let count = usize::try_from(accepted.get()).unwrap_or(usize::MAX);
    if count == 0 {
        return Vec::new();
    }
    let mut args = Vec::new();
    if replace {
        args.push(Slot::from(value.clone()));
    }
    args.extend(rest.iter().map(copy_slot));
    if count < args.len() {
        args.truncate(count);
    }
    args
}

fn copy_slot(slot: &Slot) -> Slot {
    match slot {
        Slot::Value(value) => Slot::from(value.clone()),
        Slot::Reference(cell) => Slot::Reference(cell.clone()),
    }
}
