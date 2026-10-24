//! Source nested as deeply as the parser accepts is checked whatever stack the caller has.

use php_parser::{MAX_NESTING, ParseOptions, parse};

/// Stack of the thread that drops each tree, which recurses as deep as the tree.
const DROP_STACK: usize = 512 << 20;
/// Levels below the parser's limit, leaving room for the statement around the nesting.
const LEVELS: usize = MAX_NESTING - 10;

/// Whether `source` parses and then checks without a diagnostic.
fn checks_quietly(source: &str) -> Result<bool, String> {
    let program = parse(source.as_bytes(), ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    let checked = php_compiler::check(&program, b"").map_err(|error| error.to_string())?;
    std::thread::Builder::new()
        .stack_size(DROP_STACK)
        .spawn(move || drop(program))
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_payload| "dropping the tree panicked".to_owned())?;
    Ok(checked.warnings.is_empty() && checked.fatal.is_none())
}

#[test]
fn deep_operands_are_checked() -> Result<(), String> {
    let source = format!("<?php $test = [{}1];", "~".repeat(LEVELS));
    assert!(checks_quietly(&source)?);
    Ok(())
}

#[test]
fn deep_statements_are_checked() -> Result<(), String> {
    let source = format!("<?php {}echo 1;{}", "{".repeat(LEVELS), "}".repeat(LEVELS));
    assert!(checks_quietly(&source)?);
    Ok(())
}
