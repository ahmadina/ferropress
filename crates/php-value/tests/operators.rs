//! Differential test: replays every operation recorded from the reference PHP
//! (`contracts/operators/`) on `php_value::ops` and compares results, diagnostics and thrown errors.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use php_value::ops::{self, Collect, OpError};
use php_value::{ArrayKey, PhpArray, PhpString, Value};
use serde_json::{Value as Json, json};

/// Operations recorded by the oracle that this crate does not implement yet.
const PENDING: [&str; 2] = ["preinc", "predec"];

/// Mismatches printed in full before the rest are only counted.
const SHOWN: usize = 25;

type Pair = (String, String);
type Outcome = (Option<Json>, Vec<Pair>, Option<Pair>);
type Unary = fn(&Value, &mut Collect) -> Result<Value, OpError>;
type Binary = fn(&Value, &Value, &mut Collect) -> Result<Value, OpError>;

fn bytes(text: &str) -> Result<Vec<u8>, String> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).map_err(|e| format!("{c:?}: {e}")))
        .collect()
}

fn text(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

fn index(json: &Json) -> Result<usize, String> {
    json.as_u64()
        .and_then(|i| usize::try_from(i).ok())
        .ok_or_else(|| format!("{json} is not an index"))
}

fn decode(json: &Json) -> Result<Value, String> {
    let malformed = || format!("malformed value {json}");
    let parts = json.as_array().ok_or_else(malformed)?;
    let tag = parts.first().and_then(Json::as_str).ok_or_else(malformed)?;
    Ok(match (tag, parts.get(1)) {
        ("null", None) => Value::Null,
        ("bool", Some(Json::Bool(b))) => Value::Bool(*b),
        ("int", Some(n)) => Value::Int(n.as_i64().ok_or_else(malformed)?),
        ("float", Some(Json::String(hex))) => Value::Float(f64::from_bits(
            u64::from_str_radix(hex, 16).map_err(|e| format!("{json}: {e}"))?,
        )),
        ("str", Some(Json::String(s))) => Value::String(PhpString::from(bytes(s)?)),
        ("array", Some(Json::Array(entries))) => {
            let mut array = PhpArray::new();
            for entry in entries {
                let (key, item) = match entry.as_array().map(Vec::as_slice) {
                    Some([Json::String(s), item]) => (ArrayKey::from_bytes(&bytes(s)?), item),
                    Some([n, item]) => (ArrayKey::Int(n.as_i64().ok_or_else(malformed)?), item),
                    _ => return Err(malformed()),
                };
                array.assign(key, decode(item)?);
            }
            Value::Array(array)
        }
        _ => return Err(malformed()),
    })
}

fn encode(value: &Value) -> Result<Json, String> {
    Ok(match value {
        Value::Null => json!(["null"]),
        Value::Bool(b) => json!(["bool", b]),
        Value::Int(n) => json!(["int", n]),
        Value::Float(x) => json!(["float", format!("{:016x}", x.to_bits())]),
        Value::String(s) => json!(["str", text(s.as_bytes())]),
        Value::Array(array) => {
            let mut entries = Vec::with_capacity(array.len());
            for (key, slot) in array.iter() {
                let key = match key {
                    ArrayKey::Int(n) => json!(n),
                    ArrayKey::String(s) => json!(text(s.as_bytes())),
                };
                entries.push(json!([key, encode(&slot.value())?]));
            }
            json!(["array", entries])
        }
        Value::Object(_) => return Err("an operation produced an object".to_owned()),
    })
}

/// Writes every `NAN` in an encoded value the same way. Its sign and payload come from the processor,
/// for the reference as here (x86-64 gives `INF - INF` the sign bit, arm64 does not), and PHP prints
/// every `NAN` alike.
fn canonical_nan(json: &mut Json) {
    let Json::Array(items) = json else {
        return;
    };
    if let [Json::String(tag), Json::String(hex)] = items.as_mut_slice()
        && tag == "float"
        && u64::from_str_radix(hex, 16).is_ok_and(|bits| f64::from_bits(bits).is_nan())
    {
        "nan".clone_into(hex);
        return;
    }
    items.iter_mut().for_each(canonical_nan);
}

fn unary(name: &str) -> Option<Unary> {
    Some(match name {
        "neg" => |v, cx| ops::mul(v, &Value::Int(-1), cx),
        "plus" => |v, cx| ops::mul(v, &Value::Int(1), cx),
        "bitnot" => |v, cx| ops::bit_not(v, cx),
        "not" => |v, cx| v.to_bool(cx).map(|b| Value::Bool(!b)),
        "int" => |v, cx| v.to_int(cx).map(Value::Int),
        "float" => |v, _| v.to_float().map(Value::Float),
        "string" => |v, cx| v.to_php_string(cx).map(Value::String),
        "bool" => |v, cx| v.to_bool(cx).map(Value::Bool),
        "array" => |v, cx| v.to_array(cx).map(Value::Array),
        _ => return None,
    })
}

fn binary(name: &str) -> Option<Binary> {
    Some(match name {
        "add" => |a, b, cx| ops::add(a, b, cx),
        "sub" => |a, b, cx| ops::sub(a, b, cx),
        "mul" => |a, b, cx| ops::mul(a, b, cx),
        "div" => |a, b, cx| ops::div(a, b, cx),
        "mod" => |a, b, cx| ops::modulo(a, b, cx),
        "pow" => |a, b, cx| ops::pow(a, b, cx),
        "shl" => |a, b, cx| ops::shift_left(a, b, cx),
        "shr" => |a, b, cx| ops::shift_right(a, b, cx),
        "bitor" => |a, b, cx| ops::bit_or(a, b, cx),
        "bitand" => |a, b, cx| ops::bit_and(a, b, cx),
        "bitxor" => |a, b, cx| ops::bit_xor(a, b, cx),
        "concat" => |a, b, cx| ops::concat(a, b, cx),
        "eq" => |a, b, cx| ops::loose_equals(a, b, cx).map(Value::Bool),
        "identical" => |a, b, _| ops::identical(a, b).map(Value::Bool),
        "lt" => |a, b, cx| ops::less(a, b, cx).map(Value::Bool),
        "le" => |a, b, cx| ops::less_or_equal(a, b, cx).map(Value::Bool),
        "spaceship" => |a, b, cx| ops::compare(a, b, cx).map(|o| Value::Int(o as i64)),
        _ => return None,
    })
}

fn outcome(result: Result<Value, OpError>, cx: &Collect) -> Result<Outcome, String> {
    let messages = cx
        .diagnostics
        .iter()
        .map(|d| (d.severity().label().to_owned(), text(&d.message())))
        .collect();
    Ok(match result {
        Ok(value) => (Some(encode(&value)?), messages, None),
        Err(OpError::Throw(thrown)) => (
            None,
            messages,
            Some((thrown.class().to_owned(), thrown.to_string())),
        ),
        Err(OpError::Object) => return Err("an operand was taken for an object".to_owned()),
    })
}

fn pairs(json: &Json) -> Result<Vec<Pair>, String> {
    let malformed = || format!("malformed table {json}");
    json.as_array()
        .ok_or_else(malformed)?
        .iter()
        .map(|pair| match pair.as_array().map(Vec::as_slice) {
            Some([Json::String(a), Json::String(b)]) => Ok((a.clone(), b.clone())),
            _ => Err(malformed()),
        })
        .collect()
}

/// The outcome the reference recorded, its indexes resolved against the header's tables.
fn recorded(json: &Json, messages: &[Pair], errors: &[Pair]) -> Result<Outcome, String> {
    let lookup = |table: &[Pair], i: &Json| {
        index(i).and_then(|i| table.get(i).cloned().ok_or_else(|| format!("no entry {i}")))
    };
    let malformed = || format!("malformed result {json}");
    let [result, raised, error] = json.as_array().map(Vec::as_slice).ok_or_else(malformed)? else {
        return Err(malformed());
    };
    Ok((
        Some(result.clone()).filter(|r| !r.is_null()),
        raised
            .as_array()
            .ok_or_else(malformed)?
            .iter()
            .map(|i| lookup(messages, i))
            .collect::<Result<_, _>>()?,
        if error.is_null() {
            None
        } else {
            Some(lookup(errors, error)?)
        },
    ))
}

#[test]
fn operations_match_the_reference() -> Result<(), String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/operators/cases.jsonl");
    let file = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = file.lines();
    let header: Json = serde_json::from_str(lines.next().unwrap_or_default())
        .map_err(|e| format!("header: {e}"))?;
    let operands = header["operands"]
        .as_array()
        .ok_or("no operands")?
        .iter()
        .map(decode)
        .collect::<Result<Vec<_>, _>>()?;
    let messages = pairs(&header["messages"])?;
    let errors = pairs(&header["errors"])?;

    let mut checked = 0_usize;
    let mut mismatches = Vec::new();
    let mut pending_seen = BTreeSet::new();
    for line in lines {
        let line: Json = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let name = line["op"].as_str().ok_or("no operation")?;
        let left = match line.get("left") {
            Some(i) => Some(operands.get(index(i)?).ok_or("no such operand")?),
            None => None,
        };
        let (unary, binary) = (unary(name), binary(name));
        if unary.is_none() && binary.is_none() {
            if !PENDING.contains(&name) {
                return Err(format!("unknown operation {name}"));
            }
            pending_seen.insert(name.to_owned());
            continue;
        }
        let results = line["results"].as_array().ok_or("no results")?;
        for (operand, recorded_json) in operands.iter().zip(results) {
            let mut expected = recorded(recorded_json, &messages, &errors)?;
            let mut cx = Collect::default();
            let result = match (left, unary, binary) {
                (None, Some(op), _) => op(operand, &mut cx),
                (Some(left), _, Some(op)) => op(left, operand, &mut cx),
                _ => return Err(format!("`{name}` has the wrong arity")),
            };
            let mut actual = outcome(result, &cx)?;
            for value in [&mut expected.0, &mut actual.0].into_iter().flatten() {
                canonical_nan(value);
            }
            checked += 1;
            if actual != expected {
                let left = left.map_or(Ok(json!("-")), encode)?;
                mismatches.push(format!(
                    "{name} left={left} right={}\n  expected {expected:?}\n  actual   {actual:?}",
                    encode(operand)?
                ));
            }
        }
    }
    if pending_seen.len() != PENDING.len() {
        return Err(format!("pending operations recorded: {pending_seen:?}"));
    }
    if checked == 0 || !mismatches.is_empty() {
        return Err(format!(
            "{} of {checked} operations differ from the reference:\n{}",
            mismatches.len(),
            mismatches[..mismatches.len().min(SHOWN)].join("\n")
        ));
    }
    Ok(())
}
