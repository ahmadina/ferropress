//! A live session invokes functions and closures after main has finished.

use php_compiler::compile;
use php_parser::{ParseOptions, parse};
use php_value::{Invoke, Object, Reference, Slot, Value};
use php_vm::{Session, SessionError};

fn session<'a>(source: &[u8], output: &'a mut Vec<u8>) -> Result<Session<'a, Vec<u8>>, String> {
    let program = parse(source, ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    let compiled = compile(&program, b"/t.php").map_err(|error| error.to_string())?;
    let unit = compiled
        .unit
        .ok_or_else(|| format!("compile error {:?}", compiled.diagnostics.fatal))?;
    Session::boot(&unit, b"/t.php", output).map_err(|error| error.to_string())
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
fn a_named_function_is_invoked_after_main() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(b"<?php function inc($x) { return $x + 1; }", &mut output)?;
    assert_eq!(session.status(), 0);
    let got = session
        .invoke(&Value::from("inc"), &mut [Slot::from(Value::from(2_i64))])
        .map_err(|error| error.to_string())?;
    assert_eq!(as_int(&got)?, 3);
    let again = Invoke::invoke(
        &mut session,
        &Value::from("INC"),
        &mut [Slot::from(Value::from(8_i64))],
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(as_int(&again)?, 9);
    Ok(())
}

#[test]
fn a_returned_closure_is_invoked() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(
        b"<?php return function($x) { return $x + 1; };",
        &mut output,
    )?;
    let callee = session.returned().clone();
    let got = session
        .invoke(&callee, &mut [Slot::from(Value::from(4_i64))])
        .map_err(|error| error.to_string())?;
    assert_eq!(as_int(&got)?, 5);
    Ok(())
}

#[test]
fn a_reference_argument_is_written_through() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(b"<?php function bump(&$n) { $n = $n + 1; }", &mut output)?;
    let cell = Reference::new(Value::from(1_i64));
    session
        .invoke(&Value::from("bump"), &mut [Slot::Reference(cell.clone())])
        .map_err(|error| error.to_string())?;
    assert_eq!(as_int(&cell.get())?, 2);
    Ok(())
}

#[test]
fn an_object_keeps_its_identity_across_invoke() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(
        b"<?php class Box {} function id($x) { return $x; } return new Box();",
        &mut output,
    )?;
    let original = as_object(session.returned())?;
    let back = session
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
fn an_undefined_function_is_thrown() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(b"<?php ", &mut output)?;
    match session.invoke(&Value::from("nope"), &mut []) {
        Err(SessionError::Thrown { class, message, .. }) => {
            assert_eq!(class, "Error");
            let text = String::from_utf8_lossy(&message);
            assert!(text.contains("Call to undefined function nope()"), "{text}");
            Ok(())
        }
        other => Err(format!("expected thrown, got {other:?}")),
    }
}

#[test]
fn a_non_callable_is_thrown() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(b"<?php ", &mut output)?;
    match session.invoke(&Value::Null, &mut []) {
        Err(SessionError::Thrown { class, message, .. }) => {
            assert_eq!(class, "Error");
            let text = String::from_utf8_lossy(&message);
            assert!(
                text.contains("Value of type null is not callable"),
                "{text}"
            );
            Ok(())
        }
        other => Err(format!("expected thrown, got {other:?}")),
    }
}

#[test]
fn invoke_output_is_written_to_the_session() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(b"<?php function speak() { echo \"hi\"; }", &mut output)?;
    session
        .invoke(&Value::from("speak"), &mut [])
        .map_err(|error| error.to_string())?;
    drop(session);
    assert_eq!(output, b"hi");
    Ok(())
}

#[test]
fn a_throw_from_the_callable_is_returned() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(
        b"<?php function boom() { throw new Exception(\"nope\"); }",
        &mut output,
    )?;
    match session.invoke(&Value::from("boom"), &mut []) {
        Err(SessionError::Thrown { class, message, .. }) => {
            assert_eq!(class, "Exception");
            assert_eq!(message, b"nope");
            Ok(())
        }
        other => Err(format!("expected thrown, got {other:?}")),
    }
}

#[test]
fn too_few_arguments_are_thrown() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(b"<?php function need($x) { return $x; }", &mut output)?;
    match session.invoke(&Value::from("need"), &mut []) {
        Err(SessionError::Thrown { class, message, .. }) => {
            assert_eq!(class, "ArgumentCountError");
            let text = String::from_utf8_lossy(&message);
            assert!(text.contains("Too few arguments"), "{text}");
            Ok(())
        }
        other => Err(format!("expected thrown, got {other:?}")),
    }
}

#[test]
fn a_session_names_its_status() -> Result<(), String> {
    let mut output = Vec::new();
    let session = session(b"<?php ", &mut output)?;
    assert_eq!(format!("{session:?}"), "Session { status: 0, .. }");
    Ok(())
}

#[test]
fn rust_invokes_while_a_php_frame_is_live() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(
        b"<?php function inner() { return 7; } function outer() { return call_user_func(\"inner\") + 1; }",
        &mut output,
    )?;
    match session.invoke(&Value::from("outer"), &mut []) {
        Ok(Value::Int(8)) => Ok(()),
        other => Err(format!("expected 8, got {other:?}")),
    }
}

#[test]
fn a_throw_crosses_a_native_php_call() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(
        b"<?php function boom() { throw new Exception(\"deep\"); } function outer() { return call_user_func(\"boom\"); }",
        &mut output,
    )?;
    match session.invoke(&Value::from("outer"), &mut []) {
        Err(SessionError::Thrown { class, message, .. }) => {
            assert_eq!(class, "Exception");
            assert_eq!(message, b"deep");
            Ok(())
        }
        other => Err(format!("expected thrown, got {other:?}")),
    }
}

#[test]
fn a_hook_is_removed_during_apply() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(
        b"<?php function a($v) { return $v . \"a\"; } function drop($v) { bridge_remove(\"later\"); return $v . \"d\"; } function later($v) { return $v . \"l\"; }",
        &mut output,
    )?;
    session
        .invoke(
            &Value::from("bridge_add"),
            &mut [Slot::from(Value::from("a"))],
        )
        .map_err(|error| format!("{error:?}"))?;
    session
        .invoke(
            &Value::from("bridge_add"),
            &mut [Slot::from(Value::from("drop"))],
        )
        .map_err(|error| format!("{error:?}"))?;
    session
        .invoke(
            &Value::from("bridge_add"),
            &mut [Slot::from(Value::from("later"))],
        )
        .map_err(|error| format!("{error:?}"))?;
    match session.invoke(
        &Value::from("bridge_apply"),
        &mut [Slot::from(Value::from("x"))],
    ) {
        Ok(Value::String(text)) => {
            assert_eq!(text.as_bytes(), b"xad");
            Ok(())
        }
        other => Err(format!("expected xad, got {other:?}")),
    }
}

#[test]
fn a_non_callable_hook_is_rejected() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(b"<?php ", &mut output)?;
    match session.invoke(&Value::from("bridge_add"), &mut [Slot::from(Value::Null)]) {
        Err(SessionError::Thrown { class, message, .. }) => {
            assert_eq!(class, "Error");
            let text = String::from_utf8_lossy(&message);
            assert!(
                text.contains("Value of type null is not callable"),
                "{text}"
            );
            Ok(())
        }
        other => Err(format!("expected thrown, got {other:?}")),
    }
}

#[test]
fn call_user_func_needs_a_callable() -> Result<(), String> {
    let mut output = Vec::new();
    let mut session = session(b"<?php ", &mut output)?;
    match session.invoke(&Value::from("call_user_func"), &mut []) {
        Err(SessionError::Thrown { class, message, .. }) => {
            assert_eq!(class, "ArgumentCountError");
            let text = String::from_utf8_lossy(&message);
            assert!(text.contains("expects at least"), "{text}");
            Ok(())
        }
        other => Err(format!("expected thrown, got {other:?}")),
    }
}
