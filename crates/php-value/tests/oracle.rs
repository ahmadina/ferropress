//! Differential test: replays every scenario recorded from the reference PHP (`contracts/value/`) on
//! the value model and compares the dumps, errors and warnings event by event.
//!
//! The scenarios are plain variable operations, so this file carries the small slice of engine
//! behaviour they need on top of the model: evaluation order, autovivification and the
//! diagnostics PHP raises for invalid containers.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use php_value::{
    ArrayKey, NextIndexOccupied, ObjectStore, PhpArray, PhpString, Reference, Slot, Value,
};
use serde_json::{Value as Json, json};

/// Scenarios whose PHP result the model deliberately does not reproduce, with the reason.
const KNOWN_DIFFERENCES: [(&str, &str); 1] = [(
    r#"[["assign","a",["k"],["int",1]],["bind","b",[],"a",[]],["assign","a",[null],["copy","b",[]]],["dump","a"],["dump","b"],["dump","c"]]"#,
    "appending a variable to an array bound to the same reference makes PHP store the array inside \
     itself without a reference; copy-on-write values cannot contain themselves",
)];

const DUMP_DEPTH: usize = 8;
/// Objects the oracle script itself keeps alive while a scenario runs, holding the first ids.
const SCRIPT_OBJECTS: usize = 2;
/// The class of what the scenarios throw. Each throw is an object with its own id, and the oracle
/// script's `catch` keeps the last one in a variable until the next replaces it.
const ERROR_CLASS: &str = "Error";

#[derive(Debug, Clone)]
enum Step {
    Dim(ArrayKey),
    Append,
    Property(PhpString),
}

#[derive(Debug)]
enum Expr {
    Value(Value),
    Array,
    New,
    Copy(String, Vec<Step>),
}

#[derive(Debug)]
enum Op {
    Assign(String, Vec<Step>, Expr),
    Bind(String, Vec<Step>, String, Vec<Step>),
    Unset(String, Vec<Step>),
    Dump(String),
}

/// Why an operation stopped: PHP threw, or the scenario needs behaviour this harness lacks.
#[derive(Debug)]
enum Stop {
    Thrown(String),
    Unsupported(String),
}

type Outcome<T> = Result<T, Stop>;

fn thrown<T>(message: impl Into<String>) -> Outcome<T> {
    Err(Stop::Thrown(message.into()))
}

fn unsupported<T>(what: impl Into<String>) -> Outcome<T> {
    Err(Stop::Unsupported(what.into()))
}

fn occupied(error: NextIndexOccupied) -> Stop {
    Stop::Thrown(error.to_string())
}

fn bytes(json: &Json) -> Result<Vec<u8>, String> {
    let text = json
        .as_str()
        .ok_or_else(|| format!("{json} is not a string"))?;
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).map_err(|error| format!("{c:?}: {error}")))
        .collect()
}

fn latin1(bytes: &[u8]) -> Json {
    Json::String(bytes.iter().map(|&b| char::from(b)).collect())
}

fn name(json: &Json) -> Result<String, String> {
    json.as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{json} is not a variable name"))
}

fn path(json: &Json) -> Result<Vec<Step>, String> {
    let steps = json
        .as_array()
        .ok_or_else(|| format!("{json} is not a path"))?;
    steps
        .iter()
        .map(|step| match step {
            Json::Null => Ok(Step::Append),
            Json::Number(n) => n
                .as_i64()
                .map(|n| Step::Dim(ArrayKey::Int(n)))
                .ok_or_else(|| format!("{n} is not an integer key")),
            Json::String(_) => Ok(Step::Dim(ArrayKey::from_bytes(&bytes(step)?))),
            Json::Array(pair) if pair.len() == 2 && pair[0] == "prop" => {
                Ok(Step::Property(PhpString::from(bytes(&pair[1])?)))
            }
            other => Err(format!("{other} is not a path step")),
        })
        .collect()
}

fn expr(json: &Json) -> Result<Expr, String> {
    let parts = json
        .as_array()
        .ok_or_else(|| format!("{json} is not an expression"))?;
    let argument = |index: usize| parts.get(index).unwrap_or(&Json::Null);
    let parsed = match parts.first().and_then(Json::as_str) {
        Some("null") => Expr::Value(Value::Null),
        Some("bool") => Expr::Value(Value::Bool(argument(1) == true)),
        Some("int") => Expr::Value(Value::Int(argument(1).as_i64().ok_or("integer expected")?)),
        Some("float") => {
            let hex = argument(1).as_str().ok_or("float bits expected")?;
            let bits = u64::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
            Expr::Value(Value::Float(f64::from_bits(bits)))
        }
        Some("str") => Expr::Value(Value::String(PhpString::from(bytes(argument(1))?))),
        Some("array") => Expr::Array,
        Some("new") => Expr::New,
        Some("copy") => Expr::Copy(name(argument(1))?, path(argument(2))?),
        _ => return Err(format!("{json} is not an expression")),
    };
    Ok(parsed)
}

fn op(json: &Json) -> Result<Op, String> {
    let parts = json
        .as_array()
        .ok_or_else(|| format!("{json} is not an operation"))?;
    let argument = |index: usize| parts.get(index).unwrap_or(&Json::Null);
    let parsed = match parts.first().and_then(Json::as_str) {
        Some("assign") => Op::Assign(name(argument(1))?, path(argument(2))?, expr(argument(3))?),
        Some("bind") => Op::Bind(
            name(argument(1))?,
            path(argument(2))?,
            name(argument(3))?,
            path(argument(4))?,
        ),
        Some("unset") => Op::Unset(name(argument(1))?, path(argument(2))?),
        Some("dump") => Op::Dump(name(argument(1))?),
        _ => return Err(format!("{json} is not an operation")),
    };
    Ok(parsed)
}

/// The type name PHP uses in diagnostics about a value.
fn type_label(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(true) => "true",
        Value::Bool(false) => "false",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn key_text(key: &ArrayKey) -> String {
    match key {
        ArrayKey::Int(n) => n.to_string(),
        ArrayKey::String(s) => format!(
            "\"{}\"",
            s.as_bytes()
                .iter()
                .map(|&b| char::from(b))
                .collect::<String>()
        ),
    }
}

fn class_text(value: &Value) -> String {
    match value {
        Value::Object(object) => object
            .class_name()
            .as_bytes()
            .iter()
            .map(|&b| char::from(b))
            .collect(),
        _ => String::new(),
    }
}

fn warn(events: &mut Vec<Json>, message: String) {
    events.push(Json::Array(vec![
        Json::from("warning"),
        Json::String(message),
    ]));
}

/// The array a write fetch on `value` works on, autovivifying `null` and `false`.
fn write_array<'v>(events: &mut Vec<Json>, value: &'v mut Value) -> Outcome<&'v mut PhpArray> {
    match value {
        Value::Array(array) => return Ok(array),
        Value::Null => {}
        Value::Bool(false) => events.push(json!([
            "deprecated",
            "Automatic conversion of false to array is deprecated"
        ])),
        Value::Object(_) => {
            return thrown(format!(
                "Cannot use object of type {} as array",
                class_text(value)
            ));
        }
        Value::String(_) => return unsupported("string offsets"),
        Value::Bool(true) | Value::Int(_) | Value::Float(_) => {
            return thrown("Cannot use a scalar value as an array");
        }
    }
    *value = Value::Array(PhpArray::new());
    match value {
        Value::Array(array) => Ok(array),
        _ => unsupported("autovivification"),
    }
}

/// What a write path does at its last step.
enum Action {
    Assign(Value),
    Bind(Reference),
    MakeReference,
}

/// What a write path left behind: the slot it replaced, or the reference it made.
enum Output {
    Old(
        #[expect(
            dead_code,
            reason = "held so the replaced slot is freed after the walk ends"
        )]
        Slot,
    ),
    Made(Reference),
}

/// Where a write walk goes next. Walks never re-enter a reference they are inside: they stop at
/// each reference (or object) they meet and continue from its handle once the borrow is released.
enum Advance<'p> {
    Done(Output),
    Into(Reference, &'p [Step], Action),
    Through(Reference, Value),
    Property(php_value::Object, PhpString, Action),
}

fn element_slot<'a>(array: &'a mut PhpArray, step: &Step) -> Outcome<&'a mut Slot> {
    match step {
        Step::Dim(key) => Ok(array.slot_mut(key.clone())),
        Step::Append => Ok(array.next_slot_mut().map_err(occupied)?.1),
        Step::Property(_) => unsupported("a property inside a path"),
    }
}

fn advance<'p>(
    events: &mut Vec<Json>,
    value: &mut Value,
    steps: &'p [Step],
    action: Action,
) -> Outcome<Advance<'p>> {
    let Some((step, rest)) = steps.split_first() else {
        return unsupported("an empty write path");
    };
    if let Step::Property(name) = step {
        return match value {
            Value::Object(object) if rest.is_empty() => {
                Ok(Advance::Property(object.clone(), name.clone(), action))
            }
            Value::Object(_) => unsupported("a property inside a path"),
            other => thrown(assign_property_message(name, other)),
        };
    }
    let array = write_array(events, value)?;
    if rest.is_empty() {
        let done = match (action, step) {
            (Action::Bind(reference), Step::Dim(key)) => {
                Output::Old(array.bind(key.clone(), reference))
            }
            (Action::Bind(reference), _) => {
                array.append_reference(reference).map_err(occupied)?;
                Output::Old(Slot::Value(Value::Null))
            }
            (Action::MakeReference, Step::Dim(key)) => {
                Output::Made(array.reference_at(key.clone()))
            }
            (Action::MakeReference, _) => {
                Output::Made(array.reference_at_next().map_err(occupied)?.1)
            }
            (Action::Assign(value), _) => match element_slot(array, step)? {
                Slot::Reference(reference) => {
                    return Ok(Advance::Through(reference.clone(), value));
                }
                Slot::Value(current) => Output::Old(Slot::Value(std::mem::replace(current, value))),
            },
        };
        return Ok(Advance::Done(done));
    }
    match element_slot(array, step)? {
        Slot::Reference(reference) => Ok(Advance::Into(reference.clone(), rest, action)),
        Slot::Value(inner) => advance(events, inner, rest, action),
    }
}

/// Runs a write path (`$v[...] = ...`, `$v[...] = &...`, `&$v[...]`) from a variable's slot.
fn write(
    events: &mut Vec<Json>,
    root: &mut Slot,
    path: &[Step],
    action: Action,
) -> Outcome<Output> {
    let mut next = match root {
        Slot::Reference(reference) => Advance::Into(reference.clone(), path, action),
        Slot::Value(value) => advance(events, value, path, action)?,
    };
    loop {
        next = match next {
            Advance::Done(output) => return Ok(output),
            Advance::Into(reference, steps, action) => {
                reference.with_mut(|value| advance(events, value, steps, action))?
            }
            Advance::Through(reference, value) => {
                return Ok(Output::Old(Slot::Value(reference.set(value))));
            }
            Advance::Property(object, name, Action::Assign(value)) => {
                return Ok(Output::Old(Slot::Value(
                    object.with_properties_mut(|p| p.assign(name, value)),
                )));
            }
            Advance::Property(..) => return unsupported("binding a property"),
        };
    }
}

fn assign_property_message(name: &PhpString, container: &Value) -> String {
    let name: String = name.as_bytes().iter().map(|&b| char::from(b)).collect();
    format!(
        "Attempt to assign property \"{name}\" on {}",
        type_label(container)
    )
}

fn read_step(events: &mut Vec<Json>, value: &Value, step: &Step) -> Outcome<Value> {
    match (step, value) {
        (Step::Dim(_), Value::Object(_)) => thrown(format!(
            "Cannot use object of type {} as array",
            class_text(value)
        )),
        (Step::Dim(key), Value::Array(array)) => Ok(array.get(key).map_or_else(
            || {
                warn(events, format!("Undefined array key {}", key_text(key)));
                Value::Null
            },
            Slot::value,
        )),
        (Step::Dim(_), Value::String(_)) => unsupported("string offsets"),
        (Step::Dim(_), other) => {
            warn(
                events,
                format!("Trying to access array offset on {}", type_label(other)),
            );
            Ok(Value::Null)
        }
        (Step::Property(name), Value::Object(object)) => {
            let found = object.with_properties(|p| p.get(name.as_bytes()).map(Slot::value));
            Ok(found.unwrap_or_else(|| {
                let name: String = name.as_bytes().iter().map(|&b| char::from(b)).collect();
                warn(
                    events,
                    format!("Undefined property: {}::${name}", class_text(value)),
                );
                Value::Null
            }))
        }
        (Step::Property(name), other) => {
            let name: String = name.as_bytes().iter().map(|&b| char::from(b)).collect();
            warn(
                events,
                format!(
                    "Attempt to read property \"{name}\" on {}",
                    type_label(other)
                ),
            );
            Ok(Value::Null)
        }
        (Step::Append, _) => unsupported("reading an append"),
    }
}

/// Where an unset walk goes next; like [`Advance`], it continues from handles, never re-entering.
enum UnsetAdvance<'p> {
    Done(Option<Slot>),
    Into(Reference, &'p [Step]),
    Property(php_value::Object, PhpString),
}

fn unset_advance<'p>(
    events: &mut Vec<Json>,
    value: &mut Value,
    steps: &'p [Step],
) -> Outcome<UnsetAdvance<'p>> {
    let Some((step, rest)) = steps.split_first() else {
        return unsupported("an empty unset path");
    };
    match (step, value) {
        (Step::Property(name), Value::Object(object)) if rest.is_empty() => {
            Ok(UnsetAdvance::Property(object.clone(), name.clone()))
        }
        (Step::Property(_), _) if rest.is_empty() => Ok(UnsetAdvance::Done(None)),
        (Step::Dim(key), Value::Array(array)) if rest.is_empty() => {
            Ok(UnsetAdvance::Done(array.remove(key)))
        }
        (Step::Dim(key), Value::Array(array)) => match array.existing_slot_mut(key) {
            None => Ok(UnsetAdvance::Done(None)),
            Some(Slot::Reference(reference)) => Ok(UnsetAdvance::Into(reference.clone(), rest)),
            Some(Slot::Value(inner)) => unset_advance(events, inner, rest),
        },
        (Step::Dim(_), other) => unset_on_non_array(events, other).map(UnsetAdvance::Done),
        (Step::Append | Step::Property(_), _) => {
            unsupported("an append or property inside an unset path")
        }
    }
}

/// Runs `unset($v[...])` from a variable's slot.
fn unset_path(events: &mut Vec<Json>, root: &mut Slot, path: &[Step]) -> Outcome<Option<Slot>> {
    let mut next = match root {
        Slot::Reference(reference) => UnsetAdvance::Into(reference.clone(), path),
        Slot::Value(value) => unset_advance(events, value, path)?,
    };
    loop {
        next = match next {
            UnsetAdvance::Done(removed) => return Ok(removed),
            UnsetAdvance::Into(reference, steps) => {
                reference.with_mut(|value| unset_advance(events, value, steps))?
            }
            UnsetAdvance::Property(object, name) => {
                return Ok(object.with_properties_mut(|p| p.remove(name.as_bytes())));
            }
        };
    }
}

fn unset_on_non_array(events: &mut Vec<Json>, value: &mut Value) -> Outcome<Option<Slot>> {
    match value {
        Value::Null => Ok(None),
        Value::Bool(false) => write_array(events, value).map(|_| None),
        Value::Object(_) => thrown(format!(
            "Cannot use object of type {} as array",
            class_text(value)
        )),
        Value::String(_) => unsupported("string offsets"),
        _ => thrown("Cannot unset offset in a non-array variable"),
    }
}

/// Containers being dumped on the current path, for PHP's recursion markers.
enum Open<'a> {
    Root,
    Reference(&'a Reference, &'a Open<'a>),
    Object(u32, &'a Open<'a>),
}

impl Open<'_> {
    fn has_reference(&self, reference: &Reference) -> bool {
        match self {
            Open::Root => false,
            Open::Reference(r, rest) => r.ptr_eq(reference) || rest.has_reference(reference),
            Open::Object(_, rest) => rest.has_reference(reference),
        }
    }

    fn has_object(&self, id: u32) -> bool {
        match self {
            Open::Root => false,
            Open::Reference(_, rest) => rest.has_object(id),
            Open::Object(open, rest) => *open == id || rest.has_object(id),
        }
    }
}

/// Renders values the way the oracle script does, numbering shared references per dump.
#[derive(Default)]
struct Dumper {
    references: Vec<Reference>,
}

impl Dumper {
    fn value(&mut self, value: &Value, open: &Open<'_>, depth: usize) -> Json {
        match value {
            Value::Null => json!(["null"]),
            Value::Bool(b) => json!(["bool", b]),
            Value::Int(n) => json!(["int", n]),
            Value::Float(f) => json!(["float", format!("{:016x}", f.to_bits())]),
            Value::String(s) => json!(["string", latin1(s.as_bytes())]),
            Value::Array(_) | Value::Object(_) if depth >= DUMP_DEPTH => json!(["deep"]),
            Value::Array(array) => {
                let entries: Vec<Json> = array
                    .iter()
                    .map(|(key, slot)| self.entry(array, key, slot, open, depth))
                    .collect();
                json!(["array", entries])
            }
            Value::Object(object) => {
                let id = object.id().get();
                if open.has_object(id) {
                    return json!(["object", id]);
                }
                let inner = Open::Object(id, open);
                let properties: Vec<Json> = object.with_properties(|properties| {
                    properties
                        .iter()
                        .map(|(name, slot)| {
                            let value = slot.with_value(|v| self.value(v, &inner, depth + 1));
                            json!([latin1(name.as_bytes()), value])
                        })
                        .collect()
                });
                json!([
                    "object",
                    id,
                    latin1(object.class_name().as_bytes()),
                    properties
                ])
            }
        }
    }

    fn entry(
        &mut self,
        array: &PhpArray,
        key: &ArrayKey,
        slot: &Slot,
        open: &Open<'_>,
        depth: usize,
    ) -> Json {
        let name = match key {
            ArrayKey::Int(n) => json!(n),
            ArrayKey::String(s) => latin1(s.as_bytes()),
        };
        let Some(reference) = slot.as_reference().filter(|r| array.keeps_reference(r)) else {
            return json!([name, slot.with_value(|v| self.value(v, open, depth + 1))]);
        };
        let number = if let Some(index) = self.references.iter().position(|r| r.ptr_eq(reference)) {
            index + 1
        } else {
            self.references.push(reference.clone());
            self.references.len()
        };
        if open.has_reference(reference) {
            return json!([name, ["recursion"], number]);
        }
        let inner = Open::Reference(reference, open);
        json!([
            name,
            reference.with(|v| self.value(v, &inner, depth + 1)),
            number
        ])
    }
}

/// The variables of one scenario and the events they produced.
struct Machine {
    store: ObjectStore,
    variables: HashMap<String, Slot>,
    events: Vec<Json>,
    _script_objects: Vec<php_value::Object>,
    caught: Option<php_value::Object>,
}

impl Machine {
    fn new() -> Result<Self, String> {
        let store = ObjectStore::new();
        let script_objects = (0..SCRIPT_OBJECTS)
            .map(|_| store.create(PhpString::from("Closure")))
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            store,
            variables: HashMap::new(),
            events: Vec::new(),
            _script_objects: script_objects,
            caught: None,
        })
    }

    fn run(&mut self, op: &Op) -> Result<(), String> {
        let outcome = match op {
            Op::Assign(name, path, expr) => return self.assign(name, path, expr),
            Op::Bind(target, target_path, source, source_path) => {
                self.bind(target, target_path, source, source_path)
            }
            Op::Unset(name, path) => self.unset(name, path),
            Op::Dump(name) => {
                let value = self.variables.get(name).map_or(Json::Null, |slot| {
                    slot.with_value(|v| Dumper::default().value(v, &Open::Root, 0))
                });
                self.events.push(json!(["dump", name, value]));
                Ok(())
            }
        };
        self.settle(outcome, None)
    }

    /// Records how an operation ended. A throw creates its error while `pending` (the operation's
    /// right-hand side) is still alive; unwinding then frees `pending`, and only after that does the
    /// catch replace the previously caught error, which fixes the order ids are freed and reused in.
    fn settle(&mut self, outcome: Outcome<()>, pending: Option<Value>) -> Result<(), String> {
        match outcome {
            Ok(()) => Ok(()),
            Err(Stop::Thrown(message)) => {
                let error = self
                    .store
                    .create(PhpString::from(ERROR_CLASS))
                    .map_err(|error| error.to_string())?;
                drop(pending);
                self.events.push(json!(["error", ERROR_CLASS, message]));
                drop(self.caught.replace(error));
                Ok(())
            }
            Err(Stop::Unsupported(what)) => Err(format!("the harness does not model {what}")),
        }
    }

    fn evaluate(&mut self, expr: &Expr) -> Outcome<Value> {
        match expr {
            Expr::Value(value) => Ok(value.clone()),
            Expr::Array => Ok(Value::Array(PhpArray::new())),
            Expr::New => self
                .store
                .create(PhpString::from("stdClass"))
                .map(Value::Object)
                .or_else(|error| unsupported(error.to_string())),
            Expr::Copy(name, path) => {
                let mut value = self.variables.get(name).map_or_else(
                    || {
                        warn(&mut self.events, format!("Undefined variable $v_{name}"));
                        Value::Null
                    },
                    Slot::value,
                );
                for step in path {
                    value = read_step(&mut self.events, &value, step)?;
                }
                Ok(value)
            }
        }
    }

    fn assign(&mut self, name: &str, path: &[Step], expr: &Expr) -> Result<(), String> {
        let value = match self.evaluate(expr) {
            Ok(value) => value,
            Err(stop) => return self.settle(Err(stop), None),
        };
        if path.is_empty() {
            drop(self.slot(name).assign(value));
            return Ok(());
        }
        let pending = value.clone();
        let outcome = match path {
            [Step::Property(property)] if !self.variables.contains_key(name) => {
                thrown(assign_property_message(property, &Value::Null))
            }
            _ => self.write(name, path, Action::Assign(value)).map(drop),
        };
        self.settle(outcome, Some(pending))
    }

    fn bind(
        &mut self,
        target: &str,
        target_path: &[Step],
        source: &str,
        source_path: &[Step],
    ) -> Outcome<()> {
        let reference = if source_path.is_empty() {
            let defined = self.variables.contains_key(source);
            let reference = self.slot(source).make_reference();
            if target_path.is_empty() {
                reference
            } else {
                // A plain source variable comes into existence only once the target was fetched.
                return match self.write(target, target_path, Action::Bind(reference)) {
                    Err(stop) if !defined => {
                        self.variables.remove(source);
                        Err(stop)
                    }
                    outcome => outcome.map(drop),
                };
            }
        } else {
            match self.write(source, source_path, Action::MakeReference)? {
                Output::Made(reference) => reference,
                Output::Old(_) => return unsupported("a reference fetch without a reference"),
            }
        };
        if target_path.is_empty() {
            drop(std::mem::replace(
                self.slot(target),
                Slot::Reference(reference),
            ));
            return Ok(());
        }
        self.write(target, target_path, Action::Bind(reference))
            .map(drop)
    }

    fn write(&mut self, name: &str, path: &[Step], action: Action) -> Outcome<Output> {
        let slot = self
            .variables
            .entry(name.to_owned())
            .or_insert(Slot::Value(Value::Null));
        write(&mut self.events, slot, path, action)
    }

    fn unset(&mut self, name: &str, path: &[Step]) -> Outcome<()> {
        if path.is_empty() {
            drop(self.variables.remove(name));
            return Ok(());
        }
        let Some(slot) = self.variables.get_mut(name) else {
            if !matches!(path, [Step::Property(_), ..]) {
                warn(&mut self.events, format!("Undefined variable $v_{name}"));
            }
            return Ok(());
        };
        unset_path(&mut self.events, slot, path).map(drop)
    }

    fn slot(&mut self, name: &str) -> &mut Slot {
        self.variables
            .entry(name.to_owned())
            .or_insert(Slot::Value(Value::Null))
    }
}

fn replay(ops: &[Json]) -> Result<Vec<Json>, String> {
    let ops = ops.iter().map(op).collect::<Result<Vec<_>, _>>()?;
    let mut machine = Machine::new()?;
    for op in &ops {
        machine.run(op)?;
    }
    Ok(machine.events)
}

fn scenarios() -> Result<Vec<(Json, Vec<Json>)>, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/value/scenarios.jsonl");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .skip(1)
        .map(|line| {
            let mut scenario: Json = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let ops = scenario["ops"].take();
            let events = scenario["events"].take();
            match events {
                Json::Array(events) => Ok((ops, events)),
                other => Err(format!("{other} is not an event list")),
            }
        })
        .collect()
}

#[test]
fn model_matches_every_recorded_scenario() {
    let scenarios = scenarios().unwrap();
    assert!(scenarios.len() > 600, "only {} scenarios", scenarios.len());
    let mut failures = Vec::new();
    let mut known = 0;
    for (index, (ops, expected)) in scenarios.iter().enumerate() {
        let ops_text = ops.to_string();
        let actual = replay(ops.as_array().unwrap());
        let is_known = KNOWN_DIFFERENCES
            .iter()
            .any(|(known, _)| *known == ops_text);
        match (&actual, is_known) {
            (Ok(events), false) if events == expected => {}
            (Ok(events), true) if events != expected => known += 1,
            (_, true) => failures.push(format!(
                "scenario {}: a known difference now matches or fails: {actual:?}",
                index + 1
            )),
            (Ok(events), false) => failures.push(format!(
                "scenario {}\n  ops:      {ops_text}\n  expected: {}\n  actual:   {}",
                index + 1,
                Json::Array(expected.clone()),
                Json::Array(events.clone())
            )),
            (Err(error), false) => failures.push(format!(
                "scenario {}: {error}\n  ops: {ops_text}",
                index + 1
            )),
        }
    }
    assert_eq!(
        known,
        KNOWN_DIFFERENCES.len(),
        "every known difference must be recorded"
    );
    assert!(
        failures.is_empty(),
        "{} of {} scenarios differ:\n{}",
        failures.len(),
        scenarios.len(),
        failures.join("\n")
    );
}
