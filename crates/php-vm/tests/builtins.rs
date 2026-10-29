//! Built-in string, type and count functions.

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
fn strlen_and_case_map_strings() -> Result<(), String> {
    let (output, status) =
        run(b"<?php echo strlen(\"AbC\"), strtolower(\"AbC\"), strtoupper(\"AbC\");")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"3abcABC");
    Ok(())
}

#[test]
fn count_rejects_a_non_array() -> Result<(), String> {
    let (output, status) = run(b"<?php echo count(1);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("must be of type Countable|array, int given"),
        "{text}"
    );
    Ok(())
}

#[test]
fn gettype_and_is_tests_name_the_value() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo gettype(1), gettype(1.5), gettype(true), gettype(null), gettype(\"a\"); echo is_int(1) ? \"y\" : \"n\", is_string(1) ? \"y\" : \"n\", is_numeric(\"12\") ? \"y\" : \"n\", is_numeric(\"12x\") ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"integerdoublebooleanNULLstringynyn");
    Ok(())
}

#[test]
fn function_exists_finds_user_and_builtin_names() -> Result<(), String> {
    let (output, status) = run(
        b"<?php function f() {} echo function_exists(\"f\") ? \"y\" : \"n\", function_exists(\"strlen\") ? \"y\" : \"n\", function_exists(\"missing\") ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"yyn");
    Ok(())
}

#[test]
fn class_exists_finds_declared_and_builtin_classes() -> Result<(), String> {
    let (output, status) = run(
        b"<?php class C {} echo class_exists(\"C\") ? \"y\" : \"n\", class_exists(\"Exception\") ? \"y\" : \"n\", class_exists(\"Missing\") ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"yyn");
    Ok(())
}

#[test]
fn is_helpers_cover_the_other_types() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo is_null(null) ? \"y\" : \"n\", is_bool(false) ? \"y\" : \"n\", is_float(1.5) ? \"y\" : \"n\", is_object(new Exception(\"x\")) ? \"y\" : \"n\", is_integer(2) ? \"y\" : \"n\", is_array(1) ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"yyyyyn");
    Ok(())
}

#[test]
fn a_computed_callee_finds_a_builtin() -> Result<(), String> {
    let (output, status) = run(b"<?php $n = \"strlen\"; echo $n(\"ab\");")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"2");
    Ok(())
}

#[test]
fn json_round_trips_scalars_and_lists() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo json_encode(1), json_encode(\"a\"), json_encode([1, 2]); echo implode(\",\", json_decode(\"[3,4]\"));",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"1\"a\"[1,2]3,4");
    Ok(())
}

#[test]
fn json_decode_of_invalid_input_is_null() -> Result<(), String> {
    let (output, status) = run(b"<?php echo json_decode(\"{\") === null ? \"n\" : \"x\";")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"n");
    Ok(())
}

#[test]
fn json_encode_maps_objects_assoc_and_scalars() -> Result<(), String> {
    let (output, status) = run(
        b"<?php class C { public $x = 1; } echo json_encode(new C), json_encode([\"a\" => 2]), json_encode(null), json_encode(true), json_encode(1.5);",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"{\"x\":1}{\"a\":2}nulltrue1.5");
    Ok(())
}

#[test]
fn json_decode_reads_scalars_and_objects() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo json_decode(\"true\") ? \"t\" : \"f\", json_decode(\"null\") === null ? \"n\" : \"x\", json_decode(\"1.5\"), count(json_decode(\"{\\\"a\\\":1}\"));",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"tn1.51");
    Ok(())
}

#[test]
fn json_encode_without_arguments_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php json_encode();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("json_encode() expects exactly 1 argument, 0 given"),
        "{text}"
    );
    Ok(())
}

#[test]
fn array_offsets_read_and_write() -> Result<(), String> {
    let (output, status) =
        run(b"<?php $a = [1, 2]; echo $a[0], $a[1]; $a[0] = 9; $a[] = 3; echo $a[0], $a[2];")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"1293");
    Ok(())
}

#[test]
fn array_offset_missing_key_warns() -> Result<(), String> {
    let (output, status) = run(b"<?php $a = []; echo $a[0] === null ? \"n\" : \"x\";")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 0);
    assert!(text.contains("Undefined array key 0"), "{text}");
    assert!(text.contains('n'), "{text}");
    Ok(())
}

#[test]
fn string_offset_reads_a_byte() -> Result<(), String> {
    let (output, status) = run(b"<?php $s = \"ab\"; echo $s[0], $s[1];")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"ab");
    Ok(())
}

#[test]
fn offset_on_a_scalar_warns() -> Result<(), String> {
    let (output, status) = run(b"<?php $n = 1; echo $n[0] === null ? \"n\" : \"x\";")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 0);
    assert!(
        text.contains("Trying to access array offset on int"),
        "{text}"
    );
    assert!(text.contains('n'), "{text}");
    Ok(())
}

#[test]
fn array_lookups_and_merge() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo array_key_exists(\"a\", [\"a\" => 1]) ? \"y\" : \"n\", in_array(2, [1, 2]) ? \"y\" : \"n\", implode(\",\", array_merge([1], [2])), array_keys([\"a\" => 1])[0], array_values([\"a\" => 3])[0];",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"yy1,2a3");
    Ok(())
}

#[test]
fn mbstring_counts_and_slices_codepoints() -> Result<(), String> {
    let (output, status) = run(b"<?php echo mb_strlen(\"\\u{e9}\"), mb_substr(\"\\u{e9}a\", 0, 1) === \"\\u{e9}\" ? \"y\" : \"n\";")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"1y");
    Ok(())
}

#[test]
fn time_returns_unix_seconds() -> Result<(), String> {
    let (output, status) = run(b"<?php echo gettype(time()), time() > 0 ? \"y\" : \"n\";")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"integery");
    Ok(())
}

#[test]
fn time_rejects_arguments() -> Result<(), String> {
    let (output, status) = run(b"<?php time(1);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("expects exactly 0 arguments"), "{text}");
    Ok(())
}

#[test]
fn dirname_and_basename_split_unix_paths() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo dirname(\"/a/b/c\"), dirname(\"/a/b/c\", 2), dirname(\"foo\"), basename(\"/a/b/c.txt\", \".txt\");",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"/a/b/a.c");
    Ok(())
}

#[test]
fn dirname_rejects_zero_levels() -> Result<(), String> {
    let (output, status) = run(b"<?php dirname(\"/a/b\", 0);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("must be greater than or equal to 1"),
        "{text}"
    );
    Ok(())
}

#[test]
fn dirname_rejects_too_many_arguments() -> Result<(), String> {
    let (output, status) = run(b"<?php dirname(\"/a\", 1, 2);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("expects at most 2 arguments"), "{text}");
    Ok(())
}

#[test]
fn dirname_rejects_a_non_int_level() -> Result<(), String> {
    let (output, status) = run(b"<?php dirname(\"/a/b\", []);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("must be of type int, array given"), "{text}");
    Ok(())
}

#[test]
fn dirname_without_arguments_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php dirname();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("expects at least 1 argument"), "{text}");
    Ok(())
}

#[test]
fn dirname_accepts_a_numeric_string_level() -> Result<(), String> {
    let (output, status) = run(b"<?php echo dirname(\"/a/b/c\", \"2\");")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"/a");
    Ok(())
}

#[test]
fn dirname_warns_when_levels_is_null() -> Result<(), String> {
    let (output, status) = run(b"<?php dirname(\"/a/b\", null);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Passing null to parameter #2"), "{text}");
    assert!(
        text.contains("must be greater than or equal to 1"),
        "{text}"
    );
    Ok(())
}

#[test]
fn basename_rejects_too_many_arguments() -> Result<(), String> {
    let (output, status) = run(b"<?php basename(\"/a\", \"a\", 1);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("expects at most 2 arguments"), "{text}");
    Ok(())
}

#[test]
fn dirname_rejects_a_non_string_path() -> Result<(), String> {
    let (output, status) = run(b"<?php dirname([]);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("must be of type string, array given"),
        "{text}"
    );
    Ok(())
}

#[test]
fn gmdate_and_strtotime_use_utc_civil_time() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo gmdate(\"Y-m-d H:i:s\", 0), gmdate(\"Y-m-d\", 1609459200), date(\"Y-m-d H:i:s\", 0), strtotime(\"2021-01-01 00:00:00\");",
    )?;
    assert_eq!(status, 0);
    assert_eq!(
        output,
        b"1970-01-01 00:00:002021-01-011970-01-01 00:00:001609459200"
    );
    Ok(())
}

#[test]
fn array_key_exists_rejects_a_non_array() -> Result<(), String> {
    let (output, status) = run(b"<?php array_key_exists(\"a\", 1);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("must be of type array, int given"), "{text}");
    Ok(())
}

#[test]
fn string_slice_search_and_trim() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo substr(\"abcd\", 1, 2), strpos(\"abcd\", \"c\"), str_contains(\"abcd\", \"bc\") ? \"y\" : \"n\", trim(\" a \");",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"bc2ya");
    Ok(())
}

#[test]
fn assigning_to_a_scalar_offset_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php $n = 1; $n[0] = 2;")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Cannot use a scalar value as an array"),
        "{text}"
    );
    Ok(())
}

#[test]
fn count_of_an_array_literal() -> Result<(), String> {
    let (output, status) = run(b"<?php echo count([1, 2, 3]);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"3");
    Ok(())
}

#[test]
fn implode_joins_and_explode_splits() -> Result<(), String> {
    let (output, status) = run(b"<?php echo implode(\"-\", explode(\",\", \"a,b,c\"));")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"a-b-c");
    Ok(())
}

#[test]
fn explode_rejects_an_empty_separator() -> Result<(), String> {
    let (output, status) = run(b"<?php explode(\"\", \"ab\");")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("must not be empty"), "{text}");
    Ok(())
}

#[test]
fn math_builtins_compute_scalars() -> Result<(), String> {
    let (output, status) =
        run(b"<?php echo abs(-3), min(3, 1, 2), max(3, 1, 2), intval(1.9), strval(2);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"31312");
    Ok(())
}

#[test]
fn intdiv_and_rounding() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo intdiv(7, 2), intval(floor(1.5)), intval(ceil(1.2)), intval(round(1.5));",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"3122");
    Ok(())
}

#[test]
fn intdiv_by_zero_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php intdiv(1, 0);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Division by zero"), "{text}");
    Ok(())
}

#[test]
fn min_without_arguments_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php min();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("expects at least 1 argument"), "{text}");
    Ok(())
}

#[test]
fn foreach_walks_arrays_and_objects() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $s = \"\"; foreach ([1, 2] as $v) { $s = $s . $v; } foreach ([\"a\" => 3] as $k => $v) { $s = $s . $k . $v; } class C { public $x = 9; } foreach (new C as $v) { $s = $s . $v; } echo $s;",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"12a39");
    Ok(())
}

#[test]
fn isset_and_empty_are_quiet() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo isset($u) ? \"n\" : \"u\"; echo empty($u) ? \"e\" : \"x\"; $a = [1, null]; echo isset($a[0]) ? \"y\" : \"n\"; echo isset($a[1]) ? \"y\" : \"n\"; echo isset($a[2]) ? \"y\" : \"n\"; echo empty($a[2]) ? \"e\" : \"x\"; echo isset($m[0]) ? \"y\" : \"n\"; echo empty(0) ? \"z\" : \"x\"; echo empty(1) ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"ueynnenzn");
    Ok(())
}

#[test]
fn str_replace_and_htmlspecialchars() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo str_replace(\"a\", \"b\", \"aAa\"); echo str_replace([\"a\", \"A\"], [\"x\", \"y\"], \"aAa\"); echo htmlspecialchars(\"<a & \\\"b\\\" 'c'>\");",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"bAbxyx&lt;a &amp; &quot;b&quot; &#039;c&#039;&gt;");
    Ok(())
}

#[test]
fn str_replace_rejects_a_string_search_with_array_replace() -> Result<(), String> {
    let (output, status) = run(b"<?php str_replace(\"a\", [\"x\"], \"aa\");")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("must be of type string when argument #1 ($search) is a string"),
        "{text}"
    );
    Ok(())
}

#[test]
fn sprintf_formats_common_specifiers() -> Result<(), String> {
    let (output, status) =
        run(b"<?php echo sprintf(\"<%s %05d %% %x>\", \"a\", 3, 255); echo sprintf(\"%+d\", 3);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"<a 00003 % ff>+3");
    Ok(())
}

#[test]
fn sprintf_counts_arguments() -> Result<(), String> {
    let (output, status) = run(b"<?php sprintf(\"%s %s\", \"a\");")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("3 arguments are required, 2 given"), "{text}");
    Ok(())
}

#[test]
fn htmlspecialchars_without_arguments_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php htmlspecialchars();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("expects at least 1 argument, 0 given"),
        "{text}"
    );
    Ok(())
}

#[test]
fn isset_of_an_object_offset_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php class C { public $x = 1; } $o = new C; isset($o[0]);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Cannot use object of type C as array"),
        "{text}"
    );
    Ok(())
}

#[test]
fn foreach_of_a_scalar_warns() -> Result<(), String> {
    let (output, status) = run(b"<?php foreach (1 as $v) {}")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 0);
    assert!(
        text.contains("foreach() argument must be of type array|object"),
        "{text}"
    );
    Ok(())
}

#[test]
fn strlen_without_arguments_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php strlen();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("strlen() expects exactly 1 argument, 0 given"),
        "{text}"
    );
    Ok(())
}
