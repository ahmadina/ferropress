//! Generators: `yield` and the `Generator` object.

use php_compiler::compile;
use php_parser::{ParseOptions, parse};

fn run(source: &[u8]) -> Result<(Vec<u8>, u8), String> {
    let program = parse(source, ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    let compiled = compile(&program, b"/t.php").map_err(|error| error.to_string())?;
    let unit = compiled
        .unit
        .ok_or_else(|| format!("compile error {:?}", compiled.diagnostics.fatal))?;
    let mut output = Vec::new();
    let status = php_vm::run(&unit, b"/t.php", &mut output).map_err(|error| error.to_string())?;
    Ok((output, status))
}

#[test]
fn a_generator_yields_values_in_order() -> Result<(), String> {
    let (output, status) = run(
        b"<?php function g() { yield 1; yield 2; } $g = g(); echo $g->current(); $g->next(); echo $g->current(); echo $g->valid() ? \"y\" : \"n\"; $g->next(); echo $g->valid() ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"12yn");
    Ok(())
}

#[test]
fn a_generator_return_is_read_after_it_finishes() -> Result<(), String> {
    let (output, status) = run(
        b"<?php function g() { yield 1; return 9; } $g = g(); echo $g->current(); $g->next(); echo $g->getReturn();",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"19");
    Ok(())
}

#[test]
fn a_yielded_key_is_visible() -> Result<(), String> {
    let (output, status) =
        run(b"<?php function g() { yield \"a\" => 1; } $g = g(); echo $g->key(), $g->current();")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"a1");
    Ok(())
}

#[test]
fn send_injects_a_value_into_yield() -> Result<(), String> {
    let (output, status) =
        run(b"<?php function g() { $x = (yield 1); yield $x; } $g = g(); echo $g->send(\"z\");")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"z");
    Ok(())
}

#[test]
fn a_generator_is_an_instance_of_generator() -> Result<(), String> {
    let (output, status) =
        run(b"<?php function g() { yield 1; } echo g() instanceof Generator ? \"y\" : \"n\";")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"y");
    Ok(())
}

#[test]
fn get_return_before_finish_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php function g() { yield 1; } $g = g(); $g->getReturn();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Cannot get return value of a generator that hasn't returned"),
        "{text}"
    );
    Ok(())
}

#[test]
fn rewind_starts_an_unstarted_generator() -> Result<(), String> {
    let (output, status) =
        run(b"<?php function g() { yield 7; } $g = g(); $g->rewind(); echo $g->current();")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"7");
    Ok(())
}

#[test]
fn throw_into_a_generator_can_be_caught() -> Result<(), String> {
    let (output, status) = run(
        b"<?php function g() { try { yield 1; } catch (Exception $e) { yield \"c\"; } } $g = g(); $g->current(); $g->throw(new Exception(\"x\")); echo $g->current();",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"c");
    Ok(())
}

#[test]
fn new_generator_is_rejected() -> Result<(), String> {
    let (output, status) = run(b"<?php new Generator;")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("The \"Generator\" class is reserved for internal use"),
        "{text}"
    );
    Ok(())
}

#[test]
fn an_unknown_generator_method_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php function g() { yield 1; } g()->missing();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Call to undefined method Generator::missing()"),
        "{text}"
    );
    Ok(())
}

#[test]
fn rewind_after_current_is_a_noop() -> Result<(), String> {
    let (output, status) =
        run(b"<?php function g() { yield 1; } $g = g(); $g->current(); $g->rewind(); echo $g->current();")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"1");
    Ok(())
}

#[test]
fn rewind_after_next_throws() -> Result<(), String> {
    let (output, status) =
        run(b"<?php function g() { yield 1; yield 2; } $g = g(); $g->next(); $g->rewind();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Cannot rewind a generator that was already run"),
        "{text}"
    );
    Ok(())
}
