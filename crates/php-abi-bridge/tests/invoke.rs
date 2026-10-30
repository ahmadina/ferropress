//! The native core invokes live PHP functions and closures (ADR-0003).

use php_abi_bridge::{Bridge, BridgeError};
use php_compiler::compile;
use php_parser::{ParseOptions, parse};
use php_value::{Invoke, Object, Reference, Slot, Value};
use php_vm::SessionError;

fn bridge<'a>(source: &[u8], output: &'a mut Vec<u8>) -> Result<Bridge<'a, Vec<u8>>, String> {
    let program = parse(source, ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    let compiled = compile(&program, b"/t.php").map_err(|error| error.to_string())?;
    let unit = compiled
        .unit
        .ok_or_else(|| format!("compile error {:?}", compiled.diagnostics.fatal))?;
    Bridge::boot(&unit, b"/t.php", output).map_err(|error| error.to_string())
}

fn as_int(value: &Value) -> Result<i64, String> {
    match value {
        Value::Int(number) => Ok(*number),
        other => Err(format!("expected int, got {other:?}")),
    }
}

fn as_object(value: &Value) -> Result<Object, String> {
    match value {
        Value::Object(object) => Ok(object.clone()),
        other => Err(format!("expected object, got {other:?}")),
    }
}

#[test]
fn rust_calls_a_php_function() -> Result<(), String> {
    let mut output = Vec::new();
    let mut bridge = bridge(b"<?php function inc($x) { return $x + 1; }", &mut output)?;
    assert_eq!(bridge.status(), 0);
    let got = bridge
        .invoke(&Value::from("inc"), &mut [Slot::from(Value::from(2_i64))])
        .map_err(|error| error.to_string())?;
    assert_eq!(as_int(&got)?, 3);
    let again = Invoke::invoke(
        &mut bridge,
        &Value::from("inc"),
        &mut [Slot::from(Value::from(9_i64))],
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(as_int(&again)?, 10);
    Ok(())
}

#[test]
fn rust_calls_a_returned_closure() -> Result<(), String> {
    let mut output = Vec::new();
    let mut bridge = bridge(
        b"<?php return function($x) { return $x + 1; };",
        &mut output,
    )?;
    let callee = bridge.returned().clone();
    let got = bridge
        .invoke(&callee, &mut [Slot::from(Value::from(4_i64))])
        .map_err(|error| error.to_string())?;
    assert_eq!(as_int(&got)?, 5);
    Ok(())
}

#[test]
fn a_reference_argument_survives_the_round_trip() -> Result<(), String> {
    let mut output = Vec::new();
    let mut bridge = bridge(b"<?php function bump(&$n) { $n = $n + 1; }", &mut output)?;
    let cell = Reference::new(Value::from(1_i64));
    bridge
        .invoke(&Value::from("bump"), &mut [Slot::Reference(cell.clone())])
        .map_err(|error| error.to_string())?;
    assert_eq!(as_int(&cell.get())?, 2);
    Ok(())
}

#[test]
fn an_object_keeps_its_identity_across_the_round_trip() -> Result<(), String> {
    let mut output = Vec::new();
    let mut bridge = bridge(
        b"<?php class Box {} function id($x) { return $x; } return new Box();",
        &mut output,
    )?;
    let original = as_object(bridge.returned())?;
    let back = bridge
        .invoke(
            &Value::from("id"),
            &mut [Slot::from(Value::Object(original.clone()))],
        )
        .map_err(|error| error.to_string())?;
    let back = as_object(&back)?;
    assert_eq!(original, back);
    assert_eq!(original.id(), back.id());
    Ok(())
}

#[test]
fn a_thrown_error_crosses_the_boundary() -> Result<(), String> {
    let mut output = Vec::new();
    let mut bridge = bridge(
        b"<?php function boom() { throw new Exception(\"nope\"); }",
        &mut output,
    )?;
    match bridge.invoke(&Value::from("boom"), &mut []) {
        Err(BridgeError::Session(SessionError::Thrown { class, message, .. })) => {
            assert_eq!(class, "Exception");
            assert_eq!(message, b"nope");
            Ok(())
        }
        other => Err(format!("expected thrown, got {other:?}")),
    }
}

#[test]
fn a_bridge_names_its_status() -> Result<(), String> {
    let mut output = Vec::new();
    let bridge = bridge(b"<?php ", &mut output)?;
    assert_eq!(format!("{bridge:?}"), "Bridge { status: 0, .. }");
    Ok(())
}
