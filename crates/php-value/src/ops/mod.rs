//! PHP's operators and conversions over values, with every diagnostic they raise.
//!
//! The rules are described in `contracts/runtime/operators.md` and checked against the reference PHP
//! by the capture in `contracts/operators/`.
//!
//! Operations take a [`Context`], which supplies the `precision` setting and receives the warnings
//! and deprecations raised, in order. An operation that throws returns [`OpError::Throw`] with the
//! error PHP throws; warnings raised before the throw have already been passed to the context.
//! Objects convert, compare and combine through their class's handlers, which belong to the engine:
//! an object operand gives [`OpError::Object`] unless the rule needs no handler.
//!
//! ```
//! use php_value::Value;
//! use php_value::ops::{Collect, Diagnostic, NanTarget};
//!
//! let mut cx = Collect::default();
//! assert_eq!(Value::Float(1.5).to_int(&mut cx), Ok(1));
//! assert_eq!(Value::Float(f64::NAN).to_bool(&mut cx), Ok(true));
//! assert_eq!(cx.diagnostics, [Diagnostic::NanCoerced(NanTarget::Bool)]);
//! ```

mod arith;
mod bitwise;
mod convert;
mod numeric;

use std::fmt;

use crate::{Precision, Value};

pub use arith::{add, div, modulo, mul, pow, sub};
pub use bitwise::{bit_and, bit_not, bit_or, bit_xor, shift_left, shift_right};
pub use convert::concat;
pub use numeric::{Number, NumericPrefix, Overflow, numeric_prefix, parse_float, whole_numeric};

/// What an operation needs from the engine running it.
pub trait Context {
    /// The `precision` setting, used when a float becomes a string.
    fn precision(&self) -> Precision;

    /// Receives a warning or deprecation the operation raised.
    fn raise(&mut self, diagnostic: Diagnostic);
}

/// A [`Context`] that collects diagnostics, with a fixed precision.
#[derive(Clone, Debug, PartialEq)]
pub struct Collect {
    /// The `precision` setting.
    pub precision: Precision,
    /// Every diagnostic raised so far, in order.
    pub diagnostics: Vec<Diagnostic>,
}

impl Default for Collect {
    /// PHP's default `precision` of 14 and no diagnostics.
    fn default() -> Self {
        Self {
            precision: Precision::from_setting(14),
            diagnostics: Vec::new(),
        }
    }
}

impl Context for Collect {
    fn precision(&self) -> Precision {
        self.precision
    }

    fn raise(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

/// How serious a [`Diagnostic`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// `E_WARNING`.
    Warning,
    /// `E_DEPRECATED`.
    Deprecated,
}

impl Severity {
    /// The label PHP prints before the message.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Warning => "Warning",
            Self::Deprecated => "Deprecated",
        }
    }
}

/// The type a non-number was converted to when it was `NAN`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NanTarget {
    /// A boolean.
    Bool,
    /// A string.
    String,
    /// An array.
    Array,
}

/// A step operation: `++` or `--`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// `++`.
    Increment,
    /// `--`.
    Decrement,
}

impl Step {
    const fn verb(self) -> &'static str {
        match self {
            Self::Increment => "increment",
            Self::Decrement => "decrement",
        }
    }
}

/// A warning or deprecation raised by an operation.
#[derive(Clone, Debug, PartialEq)]
pub enum Diagnostic {
    /// A string operand only starts with a number.
    NonNumeric,
    /// A float with a fractional part, or `NAN`, became an integer.
    FloatToIntLosesPrecision(f64),
    /// A numeric string holding a float that is not an integer became an integer.
    FloatStringToIntLosesPrecision(crate::PhpString),
    /// A float outside the integer range, or not finite, became an integer.
    FloatNotRepresentable(f64),
    /// `NAN` became a boolean, string or array.
    NanCoerced(NanTarget),
    /// An array became a string.
    ArrayToString,
    /// `++`/`--` on a type it leaves unchanged (`"bool"` or `"null"`).
    StepHasNoEffect(Step, &'static str),
    /// `++` on a string that is not numeric.
    IncrementNonNumericString,
    /// `--` on the empty string.
    DecrementEmptyString,
    /// `--` on a string that is not numeric.
    DecrementNonNumericString,
    /// `0 ** n` with a negative `n`.
    ZeroToNegativePower,
}

impl Diagnostic {
    /// How serious the diagnostic is.
    pub const fn severity(&self) -> Severity {
        match self {
            Self::NonNumeric
            | Self::FloatNotRepresentable(_)
            | Self::NanCoerced(_)
            | Self::ArrayToString
            | Self::StepHasNoEffect(..) => Severity::Warning,
            Self::FloatToIntLosesPrecision(_)
            | Self::FloatStringToIntLosesPrecision(_)
            | Self::IncrementNonNumericString
            | Self::DecrementEmptyString
            | Self::DecrementNonNumericString
            | Self::ZeroToNegativePower => Severity::Deprecated,
        }
    }

    /// The message PHP prints, as bytes.
    pub fn message(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Self::NonNumeric => out.extend_from_slice(b"A non-numeric value encountered"),
            Self::FloatToIntLosesPrecision(value) => {
                out.extend_from_slice(b"Implicit conversion from float ");
                crate::write_float(&mut out, *value);
                out.extend_from_slice(b" to int loses precision");
            }
            Self::FloatStringToIntLosesPrecision(text) => {
                out.extend_from_slice(b"Implicit conversion from float-string \"");
                out.extend_from_slice(until_nul(text.as_bytes()));
                out.extend_from_slice(b"\" to int loses precision");
            }
            Self::FloatNotRepresentable(value) => {
                out.extend_from_slice(b"The float ");
                crate::write_float(&mut out, *value);
                out.extend_from_slice(b" is not representable as an int, cast occurred");
            }
            Self::NanCoerced(target) => {
                out.extend_from_slice(b"unexpected NAN value was coerced to ");
                out.extend_from_slice(match target {
                    NanTarget::Bool => b"bool".as_slice(),
                    NanTarget::String => b"string",
                    NanTarget::Array => b"array",
                });
            }
            Self::ArrayToString => out.extend_from_slice(b"Array to string conversion"),
            Self::StepHasNoEffect(step, type_name) => {
                let verb = step.verb();
                let (first, rest) = verb.split_at(1);
                out.extend_from_slice(first.to_ascii_uppercase().as_bytes());
                out.extend_from_slice(rest.as_bytes());
                out.extend_from_slice(b" on type ");
                out.extend_from_slice(type_name.as_bytes());
                out.extend_from_slice(
                    b" has no effect, this will change in the next major version of PHP",
                );
            }
            Self::IncrementNonNumericString => out.extend_from_slice(
                b"Increment on non-numeric string is deprecated, use str_increment() instead",
            ),
            Self::DecrementEmptyString => {
                out.extend_from_slice(b"Decrement on empty string is deprecated as non-numeric");
            }
            Self::DecrementNonNumericString => out.extend_from_slice(
                b"Decrement on non-numeric string has no effect and is deprecated",
            ),
            Self::ZeroToNegativePower => {
                out.extend_from_slice(b"Power of base 0 and negative exponent is deprecated");
            }
        }
        out
    }
}

/// The bytes before the first NUL, as C formatting prints a string.
fn until_nul(bytes: &[u8]) -> &[u8] {
    bytes
        .iter()
        .position(|&byte| byte == 0)
        .map_or(bytes, |end| &bytes[..end])
}

/// An error an operation throws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Thrown {
    /// `TypeError`: the operator does not accept these operand types, named as PHP names them.
    UnsupportedOperands {
        /// The left operand's type.
        left: &'static str,
        /// The operator as written.
        operator: &'static str,
        /// The right operand's type.
        right: &'static str,
    },
    /// `TypeError`: `~` on a value that is not a number or string, named as PHP names it.
    BitwiseNot(&'static str),
    /// `TypeError`: `++`/`--` on an array.
    CannotStep(Step, &'static str),
    /// `DivisionByZeroError` from `/`.
    DivisionByZero,
    /// `DivisionByZeroError` from `%`.
    ModuloByZero,
    /// `ArithmeticError`: a shift by a negative count.
    NegativeShift,
    /// `Error`: comparing an array that contains itself.
    RecursiveComparison,
    /// `Error`: comparing arrays nested deeper than the comparison supports.
    ComparisonTooDeep,
}

impl Thrown {
    /// The class of the thrown error.
    pub const fn class(&self) -> &'static str {
        match self {
            Self::UnsupportedOperands { .. } | Self::BitwiseNot(_) | Self::CannotStep(..) => {
                "TypeError"
            }
            Self::DivisionByZero | Self::ModuloByZero => "DivisionByZeroError",
            Self::NegativeShift => "ArithmeticError",
            Self::RecursiveComparison | Self::ComparisonTooDeep => "Error",
        }
    }
}

impl fmt::Display for Thrown {
    /// The error's message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedOperands {
                left,
                operator,
                right,
            } => write!(f, "Unsupported operand types: {left} {operator} {right}"),
            Self::BitwiseNot(name) => write!(f, "Cannot perform bitwise not on {name}"),
            Self::CannotStep(step, name) => write!(f, "Cannot {} {name}", step.verb()),
            Self::DivisionByZero => f.write_str("Division by zero"),
            Self::ModuloByZero => f.write_str("Modulo by zero"),
            Self::NegativeShift => f.write_str("Bit shift by negative number"),
            Self::RecursiveComparison => {
                f.write_str("Nesting level too deep - recursive dependency?")
            }
            Self::ComparisonTooDeep => {
                f.write_str("Maximum call stack size reached during comparison")
            }
        }
    }
}

/// Why an operation produced no result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpError {
    /// The operation throws this error.
    Throw(Thrown),
    /// An operand is an object, whose conversions belong to its class.
    Object,
}

impl From<Thrown> for OpError {
    fn from(thrown: Thrown) -> Self {
        Self::Throw(thrown)
    }
}

impl Value {
    /// The type name PHP uses in operand errors: `null`, `bool`, `int`, `float`, `string`, `array`,
    /// or `object` (PHP prints the class name, which is the engine's to supply).
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }

    /// Like [`Value::type_name`], except that booleans are named `true` and `false`.
    pub const fn value_name(&self) -> &'static str {
        match self {
            Self::Bool(true) => "true",
            Self::Bool(false) => "false",
            _ => self.type_name(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PhpArray, PhpString};

    #[test]
    fn diagnostics_print_php_messages() {
        let cases: [(Diagnostic, Severity, &str); 13] = [
            (
                Diagnostic::NonNumeric,
                Severity::Warning,
                "A non-numeric value encountered",
            ),
            (
                Diagnostic::FloatToIntLosesPrecision(1e-10),
                Severity::Deprecated,
                "Implicit conversion from float 1.0E-10 to int loses precision",
            ),
            (
                Diagnostic::FloatStringToIntLosesPrecision(PhpString::new(b"1.5\0x")),
                Severity::Deprecated,
                "Implicit conversion from float-string \"1.5\" to int loses precision",
            ),
            (
                Diagnostic::FloatNotRepresentable(f64::NEG_INFINITY),
                Severity::Warning,
                "The float -INF is not representable as an int, cast occurred",
            ),
            (
                Diagnostic::NanCoerced(NanTarget::Bool),
                Severity::Warning,
                "unexpected NAN value was coerced to bool",
            ),
            (
                Diagnostic::NanCoerced(NanTarget::String),
                Severity::Warning,
                "unexpected NAN value was coerced to string",
            ),
            (
                Diagnostic::NanCoerced(NanTarget::Array),
                Severity::Warning,
                "unexpected NAN value was coerced to array",
            ),
            (
                Diagnostic::ArrayToString,
                Severity::Warning,
                "Array to string conversion",
            ),
            (
                Diagnostic::StepHasNoEffect(Step::Decrement, "null"),
                Severity::Warning,
                "Decrement on type null has no effect, this will change in the next major version of PHP",
            ),
            (
                Diagnostic::IncrementNonNumericString,
                Severity::Deprecated,
                "Increment on non-numeric string is deprecated, use str_increment() instead",
            ),
            (
                Diagnostic::DecrementEmptyString,
                Severity::Deprecated,
                "Decrement on empty string is deprecated as non-numeric",
            ),
            (
                Diagnostic::DecrementNonNumericString,
                Severity::Deprecated,
                "Decrement on non-numeric string has no effect and is deprecated",
            ),
            (
                Diagnostic::ZeroToNegativePower,
                Severity::Deprecated,
                "Power of base 0 and negative exponent is deprecated",
            ),
        ];
        for (diagnostic, severity, message) in cases {
            assert_eq!(diagnostic.severity(), severity, "{diagnostic:?}");
            assert_eq!(diagnostic.message(), message.as_bytes(), "{diagnostic:?}");
        }
        assert_eq!(Severity::Warning.label(), "Warning");
        assert_eq!(Severity::Deprecated.label(), "Deprecated");
    }

    #[test]
    fn thrown_errors_carry_php_classes_and_messages() {
        let cases = [
            (
                Thrown::UnsupportedOperands {
                    left: "array",
                    operator: "+",
                    right: "int",
                },
                "TypeError",
                "Unsupported operand types: array + int",
            ),
            (
                Thrown::BitwiseNot("true"),
                "TypeError",
                "Cannot perform bitwise not on true",
            ),
            (
                Thrown::CannotStep(Step::Increment, "array"),
                "TypeError",
                "Cannot increment array",
            ),
            (
                Thrown::DivisionByZero,
                "DivisionByZeroError",
                "Division by zero",
            ),
            (
                Thrown::ModuloByZero,
                "DivisionByZeroError",
                "Modulo by zero",
            ),
            (
                Thrown::NegativeShift,
                "ArithmeticError",
                "Bit shift by negative number",
            ),
            (
                Thrown::RecursiveComparison,
                "Error",
                "Nesting level too deep - recursive dependency?",
            ),
            (
                Thrown::ComparisonTooDeep,
                "Error",
                "Maximum call stack size reached during comparison",
            ),
        ];
        for (thrown, class, message) in cases {
            assert_eq!(thrown.class(), class);
            assert_eq!(thrown.to_string(), message);
            assert_eq!(OpError::from(thrown.clone()), OpError::Throw(thrown));
        }
    }

    #[test]
    fn values_are_named_as_php_names_them() {
        let store = crate::ObjectStore::new();
        let object = store.create("C".into()).unwrap();
        let cases = [
            (Value::Null, "null", "null"),
            (Value::Bool(true), "bool", "true"),
            (Value::Bool(false), "bool", "false"),
            (Value::Int(1), "int", "int"),
            (Value::Float(1.0), "float", "float"),
            (Value::from("a"), "string", "string"),
            (Value::from(PhpArray::new()), "array", "array"),
            (Value::from(object), "object", "object"),
        ];
        for (value, type_name, value_name) in cases {
            assert_eq!(value.type_name(), type_name);
            assert_eq!(value.value_name(), value_name);
        }
    }

    #[test]
    fn collect_keeps_the_precision_and_every_diagnostic() {
        let mut cx = Collect::default();
        assert_eq!(cx.precision(), Precision::from_setting(14));
        cx.raise(Diagnostic::NonNumeric);
        cx.raise(Diagnostic::ArrayToString);
        assert_eq!(
            cx.diagnostics,
            [Diagnostic::NonNumeric, Diagnostic::ArrayToString]
        );
    }
}
