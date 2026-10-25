//! Classes: `new`, properties, methods and `$this`.

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
fn property_defaults_copy_onto_each_instance() -> Result<(), String> {
    let (output, status) = run(
        b"<?php class C { public $a; public $b = 1.5; public $c = \"x\"; public $d = true; public $e = false; } $o = new C; echo $o->a === null ? \"n\" : \"x\", $o->b, $o->c, $o->d, $o->e;",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"n1.5x1");
    Ok(())
}

#[test]
fn a_class_constructs_and_calls_methods() -> Result<(), String> {
    let (output, status) = run(
        b"<?php class C { public $a = 1; function m($x) { return $x + $this->a; } } $o = new C; echo $o->m(2); $o->a = 10; echo $o->m(3);",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"313");
    Ok(())
}

#[test]
fn a_constructor_receives_arguments() -> Result<(), String> {
    let (output, status) = run(
        b"<?php class C { public $a; function __construct($n) { $this->a = $n; } } $o = new C(7); echo $o->a;",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"7");
    Ok(())
}

#[test]
fn extra_arguments_to_new_without_a_constructor_are_ignored() -> Result<(), String> {
    let (output, status) = run(b"<?php class C {} $i = 0; new C($i += 1); echo $i;")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"1");
    Ok(())
}

#[test]
fn extra_arguments_to_a_declared_constructor_are_allowed() -> Result<(), String> {
    let (output, status) =
        run(b"<?php class C { function __construct() { echo \"c\"; } } new C(1);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"c");
    Ok(())
}

#[test]
fn an_undefined_class_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php new Missing();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Class \"Missing\" not found"), "{text}");
    Ok(())
}

#[test]
fn reading_a_missing_property_warns() -> Result<(), String> {
    let (output, status) = run(b"<?php class C {} $o = new C; echo $o->z;")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 0);
    assert!(text.contains("Undefined property: C::$z"), "{text}");
    Ok(())
}

#[test]
fn assigning_a_property_on_a_non_object_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php $a = 1; $a->p = 2;")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Attempt to assign property \"p\" on int"),
        "{text}"
    );
    Ok(())
}

#[test]
fn an_undefined_method_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php class C {} $o = new C; $o->m();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Call to undefined method C::m()"), "{text}");
    Ok(())
}

#[test]
fn class_and_method_names_are_case_insensitive() -> Result<(), String> {
    let (output, status) =
        run(b"<?php class C { function M() { return 1; } } $o = new c; echo $o->m();")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"1");
    Ok(())
}

#[test]
fn two_instances_do_not_share_properties() -> Result<(), String> {
    let (output, status) = run(
        b"<?php class C { public $a = 1; } $x = new C; $y = new C; $x->a = 2; echo $x->a, $y->a;",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"21");
    Ok(())
}

#[test]
fn reading_a_property_on_a_non_object_warns() -> Result<(), String> {
    let (output, status) = run(b"<?php echo (1)->z;")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 0);
    assert!(
        text.contains("Attempt to read property \"z\" on int"),
        "{text}"
    );
    Ok(())
}

#[test]
fn calling_a_method_on_a_non_object_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php $a = 1; $a->m();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Call to a member function m() on int"),
        "{text}"
    );
    Ok(())
}

#[test]
fn assigning_on_true_and_false_names_the_type() -> Result<(), String> {
    for (source, expected) in [
        (
            b"<?php $a = true; $a->p = 1;" as &[u8],
            "Attempt to assign property \"p\" on true",
        ),
        (
            b"<?php $a = false; $a->p = 1;",
            "Attempt to assign property \"p\" on false",
        ),
    ] {
        let (output, status) = run(source)?;
        let text = String::from_utf8_lossy(&output);
        assert_eq!(status, 255, "{text}");
        assert!(text.contains(expected), "{text}");
    }
    Ok(())
}
