//! PHP-visible hook names delegate to cms-hooks (`contracts/hooks/dispatch.md`).

use cms_request::Request;
use php_bindings::HookHost;
use php_compiler::compile;
use php_parser::{ParseOptions, parse};
use php_vm::Session;

fn run(source: &[u8]) -> Result<(Vec<u8>, u8), String> {
    let request = Request::new();
    let host = HookHost::for_request(&request);
    let program = parse(source, ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    let compiled = compile(&program, b"/t.php").map_err(|error| error.to_string())?;
    let unit = compiled
        .unit
        .ok_or_else(|| format!("compile error {:?}", compiled.diagnostics.fatal))?;
    let mut output = Vec::new();
    let session = Session::boot_with_host(&unit, b"/t.php", &mut output, Some(host))
        .map_err(|error| error.to_string())?;
    let status = session.status();
    drop(session);
    Ok((output, status))
}

#[test]
fn add_filter_and_apply_filters_run_in_priority_order() -> Result<(), String> {
    let (output, status) = run(b"<?php
        function a($v) { return $v . \"a\"; }
        function b($v) { return $v . \"b\"; }
        add_filter(\"title\", \"b\", 20);
        add_filter(\"title\", \"a\", 5);
        echo apply_filters(\"title\", \"x\");
        echo has_filter(\"title\") ? \"1\" : \"0\";
        echo has_filter(\"title\", \"a\");
        ")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"xab15");
    Ok(())
}

#[test]
fn do_action_does_not_feed_returns() -> Result<(), String> {
    let (output, status) = run(b"<?php
        function one($v) { echo $v; return \"changed\"; }
        function two($v) { echo $v; }
        add_action(\"boot\", \"one\");
        add_action(\"boot\", \"two\");
        do_action(\"boot\", \"i\");
        echo did_action(\"boot\");
        ")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"ii1");
    Ok(())
}

#[test]
fn current_filter_is_the_running_name() -> Result<(), String> {
    let (output, status) = run(
        b"<?php
        function mark($v) { echo current_filter(); echo doing_filter(\"title\") ? \"1\" : \"0\"; return $v; }
        add_filter(\"title\", \"mark\");
        apply_filters(\"title\", \"x\");
        echo current_filter() === false ? \"0\" : \"1\";
        ",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"title10");
    Ok(())
}

#[test]
fn remove_filter_needs_the_same_priority() -> Result<(), String> {
    let (output, status) = run(b"<?php
        function keep($v) { return \"k\"; }
        add_filter(\"t\", \"keep\");
        echo remove_filter(\"t\", \"keep\") ? \"1\" : \"0\";
        echo apply_filters(\"t\", \"x\");
        echo function_exists(\"add_filter\") ? \"1\" : \"0\";
        ")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"1x1");
    Ok(())
}
