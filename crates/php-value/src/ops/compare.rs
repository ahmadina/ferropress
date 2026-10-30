//! Comparison operators: `==`, `<`, `<=`, `<=>` and `===`.
//!
//! Nested arrays are walked with an explicit stack rather than recursion, so deep nesting costs heap
//! instead of call stack.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashSet;

use super::numeric::{Number, Overflow, numeric_prefix, whole_numeric};
use super::{Context, OpError, Thrown};
use crate::{PhpArray, PhpString, Value};

/// Arrays nested deeper than this stop a comparison with [`Thrown::ComparisonTooDeep`]. The reference
/// stops when its call stack runs low, which depends on the stack size.
const MAX_DEPTH: usize = 32_768;

/// A pair of equally long arrays being compared, and how far each has been walked.
struct Frame {
    left: PhpArray,
    right: PhpArray,
    left_at: usize,
    right_at: usize,
}

/// The arrays a comparison is inside, innermost last.
#[derive(Default)]
struct Walk {
    frames: Vec<Frame>,
    /// Storage of every left array in `frames`, to detect arrays that contain themselves.
    open: HashSet<usize>,
}

impl Walk {
    /// Starts comparing two arrays. Arrays sharing storage are equal and arrays of different sizes
    /// are ordered by size without being entered; otherwise both are entered and their elements are
    /// left to compare, reported as `Equal` so far.
    fn open(&mut self, left: PhpArray, right: PhpArray) -> Result<Ordering, OpError> {
        if left.ptr_eq(&right) {
            return Ok(Ordering::Equal);
        }
        if self.open.contains(&left.storage_address()) {
            return Err(Thrown::RecursiveComparison.into());
        }
        if left.len() != right.len() {
            return Ok(left.len().cmp(&right.len()));
        }
        if self.frames.len() >= MAX_DEPTH {
            return Err(Thrown::ComparisonTooDeep.into());
        }
        self.open.insert(left.storage_address());
        self.frames.push(Frame {
            left,
            right,
            left_at: 0,
            right_at: 0,
        });
        Ok(Ordering::Equal)
    }

    /// Finishes the innermost pair of arrays.
    fn close(&mut self) {
        if let Some(frame) = self.frames.pop() {
            self.open.remove(&frame.left.storage_address());
        }
    }
}

/// What comparing two values gave: an order, or two arrays whose elements decide it.
enum Next {
    Done(Ordering),
    Arrays(PhpArray, PhpArray),
}

/// The three-way comparison of two floats, `NAN` comparing greater.
fn three_way(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Greater)
}

/// Compares two strings, numerically when both are wholly numeric, otherwise byte-wise.
fn strings(left: &PhpString, right: &PhpString) -> Ordering {
    let as_text = || left.as_bytes().cmp(right.as_bytes());
    let (Some(lhs), Some(rhs)) = (
        whole_numeric(left.as_bytes()),
        whole_numeric(right.as_bytes()),
    ) else {
        return as_text();
    };
    let side = |overflow| match overflow {
        Overflow::Above => Ordering::Greater,
        Overflow::Below => Ordering::Less,
        Overflow::None => Ordering::Equal,
    };
    let overflowed = |overflow| overflow != Overflow::None;
    match (lhs.number, rhs.number) {
        (Number::Int(lhs), Number::Int(rhs)) => lhs.cmp(&rhs),
        (Number::Float(lhs_float), Number::Float(rhs_float))
            if overflowed(lhs.overflow)
                && lhs.overflow == rhs.overflow
                && three_way(lhs_float - rhs_float, 0.0).is_eq() =>
        {
            as_text()
        }
        (Number::Int(_), Number::Float(_)) if overflowed(rhs.overflow) => {
            side(rhs.overflow).reverse()
        }
        (Number::Float(_), Number::Int(_)) if overflowed(lhs.overflow) => side(lhs.overflow),
        (Number::Float(lhs_float), Number::Float(rhs_float))
            if lhs_float.is_infinite() && three_way(lhs_float, rhs_float).is_eq() =>
        {
            as_text()
        }
        (lhs, rhs) => three_way(lhs.as_float() - rhs.as_float(), 0.0),
    }
}

/// Compares an integer with a string: numerically when the string is wholly numeric, otherwise as
/// the integer's decimal text.
fn int_to_string(number: i64, text: &PhpString) -> Ordering {
    match whole_numeric(text.as_bytes()).map(|p| p.number) {
        Some(Number::Int(other)) => number.cmp(&other),
        Some(Number::Float(other)) => three_way(Number::Int(number).as_float(), other),
        None => {
            let mut digits = Vec::new();
            super::convert::write_int(&mut digits, number);
            digits.as_slice().cmp(text.as_bytes())
        }
    }
}

/// Compares a float (not `NAN`) with a string: numerically when the string is wholly numeric,
/// otherwise as the float's text under the context's precision.
fn float_to_string(number: f64, text: &PhpString, cx: &impl Context) -> Ordering {
    if let Some(other) = whole_numeric(text.as_bytes()) {
        return three_way(number, other.number.as_float());
    }
    let mut digits = Vec::new();
    crate::write_float_with(&mut digits, number, cx.precision());
    digits.as_slice().cmp(text.as_bytes())
}

/// A value as comparisons convert it to a number: silently, a string by its numeric prefix or as 0,
/// an array staying an array.
fn silent_number(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(false) => Value::Int(0),
        Value::Bool(true) => Value::Int(1),
        Value::String(text) => match numeric_prefix(text.as_bytes()).map(|p| p.number) {
            Some(Number::Int(number)) => Value::Int(number),
            Some(Number::Float(number)) => Value::Float(number),
            None => Value::Int(0),
        },
        other => other.clone(),
    }
}

/// Truthiness for comparisons with booleans and `null`; `NAN` never reaches it.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Int(number) => *number != 0,
        Value::Float(number) => *number != 0.0,
        Value::String(text) => !matches!(text.as_bytes(), b"" | b"0"),
        Value::Array(array) => !array.is_empty(),
        Value::Object(_) => true,
    }
}

const fn is_falsy_type(value: &Value) -> bool {
    matches!(value, Value::Null | Value::Bool(false))
}

const fn is_nan(value: &Value) -> bool {
    matches!(value, Value::Float(number) if number.is_nan())
}

/// `Less` when `less`, otherwise `Equal`.
const fn less_if(less: bool) -> Ordering {
    if less {
        Ordering::Less
    } else {
        Ordering::Equal
    }
}

/// Compares two values loosely, leaving the elements of two arrays to the caller.
fn loose(left: &Value, right: &Value, cx: &impl Context) -> Result<Next, OpError> {
    let (mut left, mut right) = (Cow::Borrowed(left), Cow::Borrowed(right));
    let mut converted = false;
    loop {
        let order = match (&*left, &*right) {
            (Value::Int(lhs), Value::Int(rhs)) => lhs.cmp(rhs),
            (Value::Int(lhs), Value::Float(rhs)) => three_way(Number::Int(*lhs).as_float(), *rhs),
            (Value::Float(lhs), Value::Int(rhs)) => three_way(*lhs, Number::Int(*rhs).as_float()),
            (Value::Float(lhs), Value::Float(rhs)) => three_way(*lhs, *rhs),
            (Value::Array(lhs), Value::Array(rhs)) => {
                return Ok(Next::Arrays(lhs.clone(), rhs.clone()));
            }
            (Value::Null | Value::Bool(false), Value::Null | Value::Bool(false))
            | (Value::Bool(true), Value::Bool(true)) => Ordering::Equal,
            (Value::Null, Value::Bool(true) | Value::Object(_)) => Ordering::Less,
            (Value::Bool(true) | Value::Object(_), Value::Null) => Ordering::Greater,
            (Value::String(lhs), Value::String(rhs)) => strings(lhs, rhs),
            (Value::Null, Value::String(rhs)) => less_if(!rhs.is_empty()),
            (Value::String(lhs), Value::Null) => less_if(!lhs.is_empty()).reverse(),
            (Value::Int(lhs), Value::String(rhs)) => int_to_string(*lhs, rhs),
            (Value::String(lhs), Value::Int(rhs)) => int_to_string(*rhs, lhs).reverse(),
            (Value::Float(nan), Value::String(_)) | (Value::String(_), Value::Float(nan))
                if nan.is_nan() =>
            {
                Ordering::Greater
            }
            (Value::Float(lhs), Value::String(rhs)) => float_to_string(*lhs, rhs, cx),
            (Value::String(lhs), Value::Float(rhs)) => float_to_string(*rhs, lhs, cx).reverse(),
            (Value::Object(_), _) | (_, Value::Object(_)) => return Err(OpError::Object),
            (Value::Array(_), _) if converted => Ordering::Greater,
            _ if converted => Ordering::Less,
            (lhs, rhs) if is_nan(lhs) || is_nan(rhs) => {
                if is_falsy_type(lhs) {
                    Ordering::Less
                } else if matches!(lhs, Value::Bool(true)) || matches!(rhs, Value::Bool(true)) {
                    Ordering::Equal
                } else if is_falsy_type(rhs) {
                    Ordering::Greater
                } else {
                    if matches!(lhs, Value::Float(_)) {
                        right = Cow::Owned(silent_number(rhs));
                    } else {
                        left = Cow::Owned(silent_number(lhs));
                    }
                    converted = true;
                    continue;
                }
            }
            (lhs, rhs) if is_falsy_type(lhs) => less_if(truthy(rhs)),
            (Value::Bool(true), rhs) => less_if(!truthy(rhs)).reverse(),
            (lhs, rhs) if is_falsy_type(rhs) => less_if(truthy(lhs)).reverse(),
            (lhs, Value::Bool(true)) => less_if(!truthy(lhs)),
            (lhs, rhs) => {
                (left, right) = (
                    Cow::Owned(silent_number(lhs)),
                    Cow::Owned(silent_number(rhs)),
                );
                converted = true;
                continue;
            }
        };
        return Ok(Next::Done(order));
    }
}

/// `left <=> right`: PHP's loose three-way comparison. Values that cannot be ordered compare as
/// [`Ordering::Greater`], as PHP gives 1 for them.
///
/// Numbers compare numerically; two numeric strings too, otherwise strings compare byte-wise. A
/// number and a numeric string compare numerically, a number and any other string as strings. `null`
/// and booleans compare by truthiness, `null` and a string as the empty string. Arrays compare by
/// size, then element by element under the left array's keys, and are greater than any other value
/// but `null` and booleans.
///
/// # Errors
///
/// Throws `Error` for arrays that contain themselves or are nested too deeply;
/// [`OpError::Object`] when objects need their class to compare.
pub fn compare(left: &Value, right: &Value, cx: &impl Context) -> Result<Ordering, OpError> {
    let mut walk = Walk::default();
    let mut order = match loose(left, right, cx)? {
        Next::Done(order) => order,
        Next::Arrays(x, y) => walk.open(x, y)?,
    };
    loop {
        if order.is_ne() {
            return Ok(order);
        }
        let Some(frame) = walk.frames.last_mut() else {
            return Ok(order);
        };
        let Some((at, key, slot)) = frame.left.entry_from(frame.left_at) else {
            walk.close();
            continue;
        };
        frame.left_at = at + 1;
        let Some(other) = frame.right.get(&key) else {
            return Ok(Ordering::Greater);
        };
        let (a, b) = (slot.value(), other.value());
        order = match loose(&a, &b, cx)? {
            Next::Done(order) => order,
            Next::Arrays(x, y) => walk.open(x, y)?,
        };
    }
}

/// `left == right`.
///
/// # Errors
///
/// As [`compare`].
pub fn loose_equals(left: &Value, right: &Value, cx: &impl Context) -> Result<bool, OpError> {
    compare(left, right, cx).map(Ordering::is_eq)
}

/// `left < right`.
///
/// # Errors
///
/// As [`compare`].
pub fn less(left: &Value, right: &Value, cx: &impl Context) -> Result<bool, OpError> {
    compare(left, right, cx).map(Ordering::is_lt)
}

/// `left <= right`.
///
/// # Errors
///
/// As [`compare`].
pub fn less_or_equal(left: &Value, right: &Value, cx: &impl Context) -> Result<bool, OpError> {
    compare(left, right, cx).map(Ordering::is_le)
}

/// Whether two values are identical, entering two arrays instead of comparing their elements.
fn strict(a: &Value, b: &Value, walk: &mut Walk) -> Result<bool, OpError> {
    Ok(match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => three_way(*x, *y).is_eq(),
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Object(x), Value::Object(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => walk.open(x.clone(), y.clone())?.is_eq(),
        _ => false,
    })
}

/// `left === right`: the same type and value. Floats compare numerically (`NAN` is never identical,
/// `0.0` and `-0.0` are); arrays need the same keys in the same order with identical values; objects
/// must be the same object.
///
/// # Errors
///
/// Throws `Error` for arrays that contain themselves or are nested too deeply.
pub fn identical(left: &Value, right: &Value) -> Result<bool, OpError> {
    let mut walk = Walk::default();
    let mut same = strict(left, right, &mut walk)?;
    loop {
        if !same {
            return Ok(false);
        }
        let Some(frame) = walk.frames.last_mut() else {
            return Ok(true);
        };
        let (Some((left_at, left_key, left_slot)), Some((right_at, right_key, right_slot))) = (
            frame.left.entry_from(frame.left_at),
            frame.right.entry_from(frame.right_at),
        ) else {
            walk.close();
            continue;
        };
        (frame.left_at, frame.right_at) = (left_at + 1, right_at + 1);
        if left_key != right_key {
            return Ok(false);
        }
        same = strict(&left_slot.value(), &right_slot.value(), &mut walk)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Collect;
    use crate::{ArrayKey, ObjectStore, Reference};

    fn cmp(a: impl Into<Value>, b: impl Into<Value>) -> Result<Ordering, OpError> {
        compare(&a.into(), &b.into(), &Collect::default())
    }

    fn list(values: &[i64]) -> PhpArray {
        let mut array = PhpArray::new();
        for &value in values {
            array.append(Value::Int(value)).unwrap();
        }
        array
    }

    /// `[1, &self]`: an array holding a reference to itself.
    fn looped() -> Value {
        let reference = Reference::new(Value::Null);
        let mut array = list(&[1]);
        array.bind(ArrayKey::Int(1), reference.clone());
        reference.set(Value::Array(array));
        reference.get()
    }

    fn nested(depth: usize) -> Value {
        (0..depth).fold(Value::Int(0), |inner, _| {
            let mut wrapper = PhpArray::new();
            wrapper.append(inner).unwrap();
            Value::Array(wrapper)
        })
    }

    #[test]
    fn numeric_strings_compare_as_numbers() {
        assert_eq!(cmp("10", "9"), Ok(Ordering::Greater));
        assert_eq!(cmp("abc", "abd"), Ok(Ordering::Less));
        assert_eq!(cmp("ab", "abc"), Ok(Ordering::Less));
        assert_eq!(cmp("1e3", "1000"), Ok(Ordering::Equal));
        assert_eq!(cmp(" 1", "1 "), Ok(Ordering::Equal));
        assert_eq!(cmp("1e1000", "2e1000"), Ok(Ordering::Less));
        assert_eq!(cmp("1e1000", "-1e1000"), Ok(Ordering::Greater));
        assert_eq!(
            cmp("9223372036854775808", "9223372036854775808.0"),
            Ok(Ordering::Equal)
        );
        assert_eq!(
            cmp("19223372036854775808", "19223372036854775808.0"),
            Ok(Ordering::Less)
        );
        assert_eq!(cmp("9223372036854775808", "1"), Ok(Ordering::Greater));
        assert_eq!(cmp("1", "-9223372036854775809"), Ok(Ordering::Greater));
        assert_eq!(cmp("-9223372036854775809", "1"), Ok(Ordering::Less));
        assert_eq!(cmp("1", "9223372036854775808"), Ok(Ordering::Less));
        assert_eq!(cmp("9223372036854775808", "1.5"), Ok(Ordering::Greater));
        assert_eq!(cmp("1.5", "2.5"), Ok(Ordering::Less));
    }

    #[test]
    fn numbers_and_strings_compare_numerically_or_as_text() {
        assert_eq!(cmp(10, "9"), Ok(Ordering::Greater));
        assert_eq!(cmp(10, "9.5"), Ok(Ordering::Greater));
        assert_eq!(cmp(10, "abc"), Ok(Ordering::Less));
        assert_eq!(cmp("abc", 10), Ok(Ordering::Greater));
        assert_eq!(cmp(1.5, "1.5"), Ok(Ordering::Equal));
        assert_eq!(cmp(1.5, "x"), Ok(Ordering::Less));
        assert_eq!(cmp("x", 1.5), Ok(Ordering::Greater));
        assert_eq!(cmp(f64::NAN, "x"), Ok(Ordering::Greater));
        assert_eq!(cmp("x", f64::NAN), Ok(Ordering::Greater));
        assert_eq!(cmp(f64::NAN, f64::NAN), Ok(Ordering::Greater));
        assert_eq!(cmp(1, 1.5), Ok(Ordering::Less));
        assert_eq!(cmp(2.5, 2), Ok(Ordering::Greater));
    }

    #[test]
    fn null_and_booleans_compare_by_truthiness() {
        assert_eq!(cmp(Value::Null, false), Ok(Ordering::Equal));
        assert_eq!(cmp(Value::Null, true), Ok(Ordering::Less));
        assert_eq!(cmp(true, Value::Null), Ok(Ordering::Greater));
        assert_eq!(cmp(true, true), Ok(Ordering::Equal));
        assert_eq!(cmp(Value::Null, ""), Ok(Ordering::Equal));
        assert_eq!(cmp("a", Value::Null), Ok(Ordering::Greater));
        assert_eq!(cmp(Value::Null, 0), Ok(Ordering::Equal));
        assert_eq!(cmp(Value::Null, list(&[1])), Ok(Ordering::Less));
        assert_eq!(cmp(true, 0.0), Ok(Ordering::Greater));
        assert_eq!(cmp(0, false), Ok(Ordering::Equal));
        assert_eq!(cmp("0", true), Ok(Ordering::Less));
        assert_eq!(cmp(list(&[]), true), Ok(Ordering::Less));
        assert_eq!(cmp(true, f64::NAN), Ok(Ordering::Equal));
        assert_eq!(cmp(f64::NAN, true), Ok(Ordering::Equal));
        assert_eq!(cmp(false, f64::NAN), Ok(Ordering::Less));
        assert_eq!(cmp(f64::NAN, Value::Null), Ok(Ordering::Greater));
    }

    #[test]
    fn arrays_compare_by_size_then_elements() {
        assert_eq!(cmp(list(&[1, 2]), list(&[3])), Ok(Ordering::Greater));
        assert_eq!(cmp(list(&[1, 2]), list(&[1, 3])), Ok(Ordering::Less));
        assert_eq!(cmp(list(&[1, 2]), list(&[1, 2])), Ok(Ordering::Equal));
        let mut keyed = PhpArray::new();
        keyed.assign(ArrayKey::from("a"), Value::Int(1));
        assert_eq!(cmp(keyed.clone(), list(&[1])), Ok(Ordering::Greater));
        assert_eq!(cmp(list(&[1]), keyed), Ok(Ordering::Greater));
        assert_eq!(cmp(list(&[1]), 5), Ok(Ordering::Greater));
        assert_eq!(cmp(5, list(&[1])), Ok(Ordering::Less));
        assert_eq!(cmp(list(&[1]), f64::NAN), Ok(Ordering::Greater));
        assert_eq!(cmp(f64::NAN, list(&[1])), Ok(Ordering::Less));
        assert_eq!(cmp(f64::NAN, 1), Ok(Ordering::Greater));
        assert_eq!(cmp(1, f64::NAN), Ok(Ordering::Greater));
        assert_eq!(cmp("abc", f64::INFINITY), Ok(Ordering::Greater));
        let mut emptied = list(&[1]);
        emptied.remove(&ArrayKey::Int(0));
        assert_eq!(cmp(emptied, PhpArray::new()), Ok(Ordering::Equal));
    }

    #[test]
    fn self_containing_arrays_throw() {
        let (a, c) = (looped(), looped());
        let cx = Collect::default();
        assert_eq!(compare(&a, &a, &cx), Ok(Ordering::Equal));
        assert_eq!(
            compare(&a, &c, &cx).err(),
            Some(Thrown::RecursiveComparison.into())
        );
        assert_eq!(
            identical(&a, &c).err(),
            Some(Thrown::RecursiveComparison.into())
        );
        let Value::Array(inner) = a.clone() else {
            panic!("not an array");
        };
        let mut copy = list(&[1]);
        copy.assign(ArrayKey::Int(1), Value::Array(inner));
        assert_eq!(compare(&Value::Array(copy), &a, &cx), Ok(Ordering::Equal));
    }

    #[test]
    fn deep_nesting_is_walked_without_recursion_up_to_the_limit() {
        let run = || {
            let cx = Collect::default();
            let (a, b) = (nested(MAX_DEPTH), nested(MAX_DEPTH));
            assert_eq!(compare(&a, &b, &cx), Ok(Ordering::Equal));
            assert_eq!(identical(&a, &b), Ok(true));
            let (a, b) = (nested(MAX_DEPTH + 1), nested(MAX_DEPTH + 1));
            let too_deep = Some(Thrown::ComparisonTooDeep.into());
            assert_eq!(compare(&a, &b, &cx).err(), too_deep);
            assert_eq!(identical(&a, &b).err(), too_deep);
        };
        // Dropping deeply nested arrays recurses, so the values need a larger stack than the test's.
        std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn(run)
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn objects_need_their_class_except_against_null() {
        let store = ObjectStore::new();
        let object = Value::from(store.create("C".into()).unwrap());
        let other = Value::from(store.create("C".into()).unwrap());
        assert_eq!(cmp(object.clone(), Value::Null), Ok(Ordering::Greater));
        assert_eq!(cmp(Value::Null, object.clone()), Ok(Ordering::Less));
        assert_eq!(cmp(object.clone(), 1), Err(OpError::Object));
        assert_eq!(cmp(list(&[1]), object.clone()), Err(OpError::Object));
        assert_eq!(identical(&object, &object.clone()), Ok(true));
        assert_eq!(identical(&object, &other), Ok(false));
        assert!(truthy(&object));
    }

    #[test]
    fn truthiness_follows_php() {
        assert!(!truthy(&Value::Null));
        assert!(!truthy(&Value::Int(0)));
        assert!(truthy(&Value::Float(0.5)));
        assert!(!truthy(&Value::from("0")));
        assert!(truthy(&Value::from("00")));
        assert!(truthy(&Value::Array(list(&[0]))));
    }

    #[test]
    fn derived_operators_follow_the_ordering() {
        let cx = Collect::default();
        let (one, two) = (Value::Int(1), Value::Int(2));
        assert_eq!(loose_equals(&one, &Value::from("1"), &cx), Ok(true));
        assert_eq!(less(&one, &two, &cx), Ok(true));
        assert_eq!(less_or_equal(&two, &one, &cx), Ok(false));
    }

    #[test]
    fn identity_needs_type_order_and_value() {
        let id = |a: Value, b: Value| identical(&a, &b);
        assert_eq!(id(Value::Float(0.0), Value::Float(-0.0)), Ok(true));
        assert_eq!(
            id(Value::Float(f64::NAN), Value::Float(f64::NAN)),
            Ok(false)
        );
        assert_eq!(id(Value::Int(1), Value::Float(1.0)), Ok(false));
        assert_eq!(id(Value::Null, Value::Null), Ok(true));
        assert_eq!(id(Value::Bool(true), Value::Bool(true)), Ok(true));
        assert_eq!(id("a".into(), "a".into()), Ok(true));
        assert_eq!(id(list(&[1, 2]).into(), list(&[1, 2]).into()), Ok(true));
        assert_eq!(id(list(&[1, 2]).into(), list(&[1]).into()), Ok(false));
        assert_eq!(id(list(&[1, 2]).into(), list(&[1, 3]).into()), Ok(false));
        let mut reordered = PhpArray::new();
        reordered.assign(ArrayKey::Int(1), Value::Int(2));
        reordered.assign(ArrayKey::Int(0), Value::Int(1));
        assert_eq!(id(list(&[1, 2]).into(), reordered.into()), Ok(false));
        let mut named = PhpArray::new();
        named.assign(ArrayKey::from("a"), Value::Int(1));
        let mut other = PhpArray::new();
        other.assign(ArrayKey::from("b"), Value::Int(1));
        assert_eq!(id(named.clone().into(), other.into()), Ok(false));
        assert_eq!(id(named.into(), list(&[1]).into()), Ok(false));
    }
}
