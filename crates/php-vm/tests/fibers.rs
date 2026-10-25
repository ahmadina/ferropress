//! Fibers: `start`/`suspend`/`resume` and `FiberError`.

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
fn start_returns_the_first_suspend_value() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $f = new Fiber(function() { Fiber::suspend(1); return 2; }); echo $f->start(); echo $f->resume(); echo $f->getReturn();",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"12");
    Ok(())
}

#[test]
fn resume_injects_a_value_into_suspend() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $f = new Fiber(function() { $x = Fiber::suspend(1); Fiber::suspend($x); }); echo $f->start(); echo $f->resume(\"z\");",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"1z");
    Ok(())
}

#[test]
fn flags_follow_the_fiber_state() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $f = new Fiber(function() { Fiber::suspend(1); }); echo $f->isStarted() ? \"y\" : \"n\"; $f->start(); echo $f->isSuspended() ? \"y\" : \"n\"; echo $f->isRunning() ? \"y\" : \"n\"; $f->resume(); echo $f->isTerminated() ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"nyny");
    Ok(())
}

#[test]
fn a_fiber_that_returns_without_suspend_is_terminated() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $f = new Fiber(function() { return 9; }); echo $f->start() === null ? \"n\" : \"x\"; echo $f->getReturn();",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"n9");
    Ok(())
}

#[test]
fn suspend_outside_a_fiber_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php Fiber::suspend(1);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Cannot suspend outside of a fiber"), "{text}");
    assert!(text.contains("FiberError"), "{text}");
    Ok(())
}

#[test]
fn resume_before_start_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php $f = new Fiber(function() {}); $f->resume();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Cannot resume a fiber that is not suspended"),
        "{text}"
    );
    Ok(())
}

#[test]
fn start_twice_throws() -> Result<(), String> {
    let (output, status) =
        run(b"<?php $f = new Fiber(function() { Fiber::suspend(1); }); $f->start(); $f->start();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Cannot start a fiber that has already been started"),
        "{text}"
    );
    Ok(())
}

#[test]
fn get_return_before_finish_throws() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $f = new Fiber(function() { Fiber::suspend(1); }); $f->start(); $f->getReturn();",
    )?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Cannot get fiber return value: The fiber has not returned"),
        "{text}"
    );
    Ok(())
}

#[test]
fn construct_requires_a_callable() -> Result<(), String> {
    let (output, status) = run(b"<?php new Fiber(1);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("must be a valid callback"), "{text}");
    Ok(())
}

#[test]
fn a_throw_from_the_callback_reaches_the_caller() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $f = new Fiber(function() { throw new Exception(\"x\"); }); try { $f->start(); echo \"n\"; } catch (Exception $e) { echo \"c\"; }",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"c");
    Ok(())
}

#[test]
fn throw_into_a_suspended_fiber() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $f = new Fiber(function() { try { Fiber::suspend(1); } catch (Exception $e) { Fiber::suspend(\"c\"); } }); echo $f->start(); echo $f->throw(new Exception(\"x\"));",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"1c");
    Ok(())
}

#[test]
fn a_string_names_the_callback() -> Result<(), String> {
    let (output, status) = run(
        b"<?php function f() { Fiber::suspend(3); } $fiber = new Fiber(\"f\"); echo $fiber->start();",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"3");
    Ok(())
}

#[test]
fn a_fiber_is_an_instance_of_fiber() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $f = new Fiber(function() {}); echo $f instanceof Fiber ? \"y\" : \"n\"; try { Fiber::suspend(1); } catch (Error $e) { echo $e instanceof FiberError ? \"y\" : \"n\"; }",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"yy");
    Ok(())
}
