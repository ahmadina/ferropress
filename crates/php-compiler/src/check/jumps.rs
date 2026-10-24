//! Labels, `goto`, and the jumps the reference only resolves once a function body has compiled.
//!
//! While the body compiles, [`Jumps`] tracks the enclosing loops, switches and `finally` blocks,
//! defines labels and queues every jump that can still fail. [`Checker::resolve_jumps`] then
//! checks the queue in source order, after the function's other checks.

use std::collections::HashMap;

use super::{Checker, Flow};

/// A loop or `switch` that `break` and `continue` can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Jumpable {
    Loop,
    Switch,
}

/// An enclosing loop or switch.
#[derive(Debug, Clone, Copy)]
struct Enclosing {
    kind: Jumpable,
    /// Distinguishes it from every other loop and switch of the function.
    id: u32,
    /// How many `finally` blocks were open when it started.
    finally_depth: usize,
}

/// Where a label is defined.
#[derive(Debug)]
struct Label {
    /// The innermost enclosing loop or switch.
    enclosing: Option<u32>,
    /// The enclosing `finally` blocks, outermost first.
    finally: Vec<u32>,
}

/// A jump that can only be checked once every label is known.
#[derive(Debug)]
enum Pending {
    Goto {
        label: Vec<u8>,
        line: u32,
        /// The enclosing loops and switches.
        enclosing: Vec<u32>,
        /// The enclosing `finally` blocks, outermost first.
        finally: Vec<u32>,
    },
    /// A `break` or `continue` inside a `finally` block that targets a loop outside it.
    FinallyBreakout { line: u32 },
}

/// The jump state of one function.
#[derive(Debug, Default)]
pub(super) struct Jumps {
    enclosing: Vec<Enclosing>,
    /// The open `finally` blocks, outermost first. Ids follow the order of the `try` keywords,
    /// so they grow with nesting.
    finally: Vec<u32>,
    labels: HashMap<Vec<u8>, Label>,
    pending: Vec<Pending>,
    loops_seen: u32,
    finally_seen: u32,
}

impl Jumps {
    /// The number of enclosing loops and switches.
    pub(super) fn depth(&self) -> usize {
        self.enclosing.len()
    }

    /// The kind of the enclosing loop or switch at `index`, outermost first.
    pub(super) fn kind(&self, index: usize) -> Option<Jumpable> {
        self.enclosing.get(index).map(|enclosing| enclosing.kind)
    }

    pub(super) fn enter(&mut self, kind: Jumpable) {
        self.enclosing.push(Enclosing {
            kind,
            id: self.loops_seen,
            finally_depth: self.finally.len(),
        });
        self.loops_seen += 1;
    }

    pub(super) fn leave(&mut self) {
        self.enclosing.pop();
    }

    /// Claims the id of a `try` statement with a `finally` block, at its `try` keyword.
    pub(super) fn try_with_finally(&mut self) -> u32 {
        let id = self.finally_seen;
        self.finally_seen += 1;
        id
    }

    pub(super) fn enter_finally(&mut self, id: u32) {
        self.finally.push(id);
    }

    pub(super) fn leave_finally(&mut self) {
        self.finally.pop();
    }

    /// Defines `name` at the current position; `false` when it already is.
    pub(super) fn define(&mut self, name: &[u8]) -> bool {
        if self.labels.contains_key(name) {
            return false;
        }
        let label = Label {
            enclosing: self.enclosing.last().map(|enclosing| enclosing.id),
            finally: self.finally.clone(),
        };
        self.labels.insert(name.to_vec(), label);
        true
    }

    pub(super) fn goto(&mut self, label: &[u8], line: u32) {
        self.pending.push(Pending::Goto {
            label: label.to_vec(),
            line,
            enclosing: self
                .enclosing
                .iter()
                .map(|enclosing| enclosing.id)
                .collect(),
            finally: self.finally.clone(),
        });
    }

    /// Queues a `break` or `continue` on `line` that targets the loop or switch at `target`,
    /// outermost first, when a `finally` block lies between them.
    pub(super) fn break_continue(&mut self, target: usize, line: u32) {
        let escapes = self
            .enclosing
            .get(target)
            .is_some_and(|enclosing| enclosing.finally_depth < self.finally.len());
        if escapes {
            self.pending.push(Pending::FinallyBreakout { line });
        }
    }

    /// The first jump that fails, as its line and message.
    fn first_error(&self) -> Option<(u32, String)> {
        self.pending.iter().find_map(|pending| match pending {
            Pending::FinallyBreakout { line } => Some((
                *line,
                "jump out of a finally block is disallowed".to_owned(),
            )),
            Pending::Goto {
                label,
                line,
                enclosing,
                finally,
            } => goto_error(self.labels.get(label), label, enclosing, finally)
                .map(|message| (*line, message)),
        })
    }
}

/// Why a `goto` to `label`, defined at `target`, from inside `enclosing` and `finally` fails.
fn goto_error(
    target: Option<&Label>,
    label: &[u8],
    enclosing: &[u32],
    finally: &[u32],
) -> Option<String> {
    let Some(target) = target else {
        return Some(format!(
            "'goto' to undefined label '{}'",
            String::from_utf8_lossy(label)
        ));
    };
    if target.enclosing.is_some_and(|id| !enclosing.contains(&id)) {
        return Some("'goto' into loop or switch statement is disallowed".to_owned());
    }
    let mut from = finally.iter().peekable();
    let mut to = target.finally.iter().peekable();
    loop {
        match (from.peek(), to.peek()) {
            (Some(a), Some(b)) if a == b => {
                from.next();
                to.next();
            }
            (Some(a), Some(b)) if b < a => {
                return Some("jump into a finally block is disallowed".to_owned());
            }
            (Some(_), _) => return Some("jump out of a finally block is disallowed".to_owned()),
            (None, Some(_)) => return Some("jump into a finally block is disallowed".to_owned()),
            (None, None) => return None,
        }
    }
}

impl Checker<'_> {
    /// Checks the jumps of the function just compiled, whose state is `jumps`.
    pub(super) fn resolve_jumps(&mut self, jumps: &Jumps) -> Flow {
        match jumps.first_error() {
            Some((line, message)) => {
                self.line = line;
                self.fatal(message)
            }
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Jumpable, Jumps};

    fn error(jumps: &Jumps) -> Option<(u32, String)> {
        jumps.first_error()
    }

    #[test]
    fn labels_are_defined_once() {
        let mut jumps = Jumps::default();
        assert!(jumps.define(b"a"));
        assert!(!jumps.define(b"a"));
        assert!(jumps.define(b"A"));
    }

    #[test]
    fn gotos_need_a_label_outside_inner_loops() {
        let mut jumps = Jumps::default();
        jumps.goto(b"a", 3);
        assert_eq!(
            error(&jumps),
            Some((3, "'goto' to undefined label 'a'".to_owned()))
        );
        jumps.enter(Jumpable::Loop);
        jumps.define(b"a");
        jumps.leave();
        assert_eq!(
            error(&jumps),
            Some((
                3,
                "'goto' into loop or switch statement is disallowed".to_owned()
            ))
        );

        let mut jumps = Jumps::default();
        jumps.enter(Jumpable::Switch);
        jumps.define(b"a");
        jumps.enter(Jumpable::Loop);
        jumps.goto(b"a", 1);
        jumps.leave();
        jumps.leave();
        assert_eq!(error(&jumps), None);
        assert_eq!(jumps.depth(), 0);
        assert_eq!(jumps.kind(0), None);
    }

    #[test]
    fn gotos_must_not_cross_finally_blocks() {
        let into = Some((1, "jump into a finally block is disallowed".to_owned()));
        let out = Some((1, "jump out of a finally block is disallowed".to_owned()));

        let mut jumps = Jumps::default();
        let first = jumps.try_with_finally();
        jumps.goto(b"a", 1);
        jumps.enter_finally(first);
        jumps.define(b"a");
        jumps.leave_finally();
        assert_eq!(error(&jumps), into);

        let mut jumps = Jumps::default();
        let first = jumps.try_with_finally();
        jumps.enter_finally(first);
        jumps.goto(b"a", 1);
        jumps.leave_finally();
        jumps.define(b"a");
        assert_eq!(error(&jumps), out);

        let mut jumps = Jumps::default();
        let first = jumps.try_with_finally();
        jumps.enter_finally(first);
        jumps.goto(b"a", 1);
        jumps.define(b"a");
        let nested = jumps.try_with_finally();
        jumps.enter_finally(nested);
        jumps.goto(b"a", 1);
        jumps.define(b"b");
        jumps.leave_finally();
        jumps.goto(b"b", 1);
        jumps.leave_finally();
        assert_eq!(error(&jumps), out);
        jumps.pending.pop();
        assert_eq!(error(&jumps), out);
        jumps.pending.pop();
        assert_eq!(error(&jumps), None);

        let mut jumps = Jumps::default();
        let first = jumps.try_with_finally();
        jumps.enter_finally(first);
        jumps.goto(b"a", 1);
        jumps.leave_finally();
        let second = jumps.try_with_finally();
        jumps.enter_finally(second);
        jumps.define(b"a");
        jumps.leave_finally();
        assert_eq!(error(&jumps), out);

        let mut jumps = Jumps::default();
        let first = jumps.try_with_finally();
        jumps.enter_finally(first);
        jumps.define(b"a");
        jumps.leave_finally();
        let second = jumps.try_with_finally();
        jumps.enter_finally(second);
        jumps.goto(b"a", 1);
        jumps.leave_finally();
        assert_eq!(error(&jumps), into);
    }

    #[test]
    fn breaks_must_not_leave_a_finally_block() {
        let mut jumps = Jumps::default();
        jumps.enter(Jumpable::Loop);
        let id = jumps.try_with_finally();
        jumps.enter_finally(id);
        jumps.enter(Jumpable::Loop);
        jumps.break_continue(1, 4);
        jumps.break_continue(2, 4);
        assert_eq!(error(&jumps), None);
        jumps.break_continue(0, 5);
        assert_eq!(
            error(&jumps),
            Some((5, "jump out of a finally block is disallowed".to_owned()))
        );
    }
}
