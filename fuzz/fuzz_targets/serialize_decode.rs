//! Arbitrary bytes through the serialized-format reader, with the default and a shallow depth limit.
//!
//! The reader must never panic, a refusal must point inside the input or just past its end, and an
//! accepted document's canonical bytes must read back completely to the same canonical bytes.

#![no_main]

use libfuzzer_sys::fuzz_target;
use php_serialize::{DecodeOptions, decode, decode_with};

const SHALLOW_DEPTH: usize = 3;

fuzz_target!(|input: &[u8]| {
    let limits = [
        DecodeOptions::default(),
        DecodeOptions::default().with_max_depth(SHALLOW_DEPTH),
    ];
    for options in limits {
        match decode_with(input, options) {
            Err(error) => assert!(error.offset() <= input.len()),
            Ok(document) => {
                assert!(document.consumed() <= input.len());
                let canonical = document.to_bytes();
                let again = decode(&canonical);
                assert!(again.is_ok(), "canonical form must decode: {again:?}");
                if let Ok(again) = again {
                    assert_eq!(again.consumed(), canonical.len());
                    assert_eq!(again.to_bytes(), canonical);
                }
            }
        }
    }
});
