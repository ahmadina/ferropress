//! Host functions are callable from PHP (ADR-0003).

use std::rc::Rc;

use php_compiler::compile;
use php_parser::{ParseOptions, parse};
use php_value::{Invoke, Slot, Value};
use php_vm::{Host, Session, SessionError};

struct Echo;

impl Host for Echo {
    fn exists(&self, name: &[u8]) -> bool {
        name == b"host_echo"
    }

    fn call(
        &self,
        name: &[u8],
        args: &mut [Slot],
        _invoke: &mut dyn Invoke<Error = SessionError>,
    ) -> Result<Value, SessionError> {
        assert_eq!(name, b"host_echo");
        Ok(args.first().map_or(Value::Null, Slot::value))
    }
}

fn session_with_host<'a>(
    source: &[u8],
    output: &'a mut Vec<u8>,
) -> Result<Session<'a, Vec<u8>>, String> {
    let program = parse(source, ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    let compiled = compile(&program, b"/t.php").map_err(|error| error.to_string())?;
    let unit = compiled
        .unit
        .ok_or_else(|| format!("compile error {:?}", compiled.diagnostics.fatal))?;
    Session::boot_with_host(&unit, b"/t.php", output, Some(Rc::new(Echo)))
        .map_err(|error| error.to_string())
}

#[test]
fn php_calls_a_host_function() -> Result<(), String> {
    let mut output = Vec::new();
    let session = session_with_host(
        b"<?php echo host_echo(\"ok\"), function_exists(\"host_echo\") ? \"1\" : \"0\";",
        &mut output,
    )?;
    assert_eq!(session.status(), 0);
    assert_eq!(output, b"ok1");
    Ok(())
}

#[test]
fn attach_host_after_boot_is_visible() -> Result<(), String> {
    let mut output = Vec::new();
    let program = parse(
        b"<?php function id($x) { return $x; }",
        ParseOptions::default(),
    )
    .map_err(|error| error.to_string())?
    .result
    .map_err(|error| format!("{error:?}"))?;
    let compiled = compile(&program, b"/t.php").map_err(|error| error.to_string())?;
    let unit = compiled
        .unit
        .ok_or_else(|| format!("compile error {:?}", compiled.diagnostics.fatal))?;
    let mut session =
        Session::boot(&unit, b"/t.php", &mut output).map_err(|error| error.to_string())?;
    session.attach_host(Rc::new(Echo));
    let got = session
        .invoke(
            &Value::from("host_echo"),
            &mut [Slot::from(Value::from("z"))],
        )
        .map_err(|error| error.to_string())?;
    match got {
        Value::String(text) => assert_eq!(text.as_bytes(), b"z"),
        other => return Err(format!("expected string, got {other:?}")),
    }
    Ok(())
}
