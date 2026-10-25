//! Math built-ins: `abs`, `min`/`max`, rounding, and conversions.

use php_compiler::bytecode::Operand;
use php_value::ops::OpError;
use php_value::{Value, ops};

use crate::Output;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};

impl<O: Output> Machine<'_, '_, O> {
    pub(crate) fn native_abs(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("abs", args)?;
        let out = match value {
            Value::Int(n) => abs_int(n),
            Value::Float(n) => Value::Float(n.abs()),
            other => {
                let n = self.compute(|cx| other.to_int(cx))?;
                abs_int(n)
            }
        };
        self.write(result, out)
    }

    pub(crate) fn native_minmax(&mut self, args: &[Argument], result: Operand, min: bool) -> Exec {
        let name = if min { "min" } else { "max" };
        if args.is_empty() {
            return Err(self.too_few_at_least(name, 0, 1));
        }
        let mut best = args[0].slot.value();
        for arg in args.iter().skip(1) {
            let next = arg.slot.value();
            let less = self.compute(|cx| ops::less(&next, &best, cx))?;
            if min == less {
                best = next;
            }
        }
        self.write(result, best)
    }

    pub(crate) fn native_floor_ceil(
        &mut self,
        args: &[Argument],
        result: Operand,
        floor: bool,
    ) -> Exec {
        let name = if floor { "floor" } else { "ceil" };
        let value = self.native_one(name, args)?;
        let n = self.as_float(name, &value)?;
        let out = if floor { n.floor() } else { n.ceil() };
        self.write(result, Value::Float(out))
    }

    pub(crate) fn native_round(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("round", args)?;
        let n = self.as_float("round", &value)?;
        self.write(result, Value::Float(n.round()))
    }

    pub(crate) fn native_intval(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("intval", args)?;
        let n = self.compute(|cx| value.to_int(cx))?;
        self.write(result, Value::Int(n))
    }

    pub(crate) fn native_floatval(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("floatval", args)?;
        let n = self.as_float("floatval", &value)?;
        self.write(result, Value::Float(n))
    }

    pub(crate) fn native_strval(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("strval", args)?;
        let text = self.stringify(&value)?;
        self.write(result, Value::String(text))
    }

    pub(crate) fn native_intdiv(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 2 {
            return Err(self.too_few("intdiv", args.len(), 2));
        }
        let left = self.compute(|cx| args[0].slot.value().to_int(cx))?;
        let right = self.compute(|cx| args[1].slot.value().to_int(cx))?;
        if right == 0 {
            return Err(self.throw_from(
                "DivisionByZeroError",
                b"Division by zero".to_vec(),
                b"intdiv",
                vec![args[0].slot.value(), args[1].slot.value()],
            ));
        }
        self.write(result, Value::Int(left / right))
    }

    pub(crate) fn native_sqrt(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("sqrt", args)?;
        let n = self.as_float("sqrt", &value)?;
        self.write(result, Value::Float(n.sqrt()))
    }

    fn as_float(&mut self, name: &str, value: &Value) -> Exec<f64> {
        match value.to_float() {
            Ok(n) => Ok(n),
            Err(OpError::Throw(thrown)) => {
                let mut message = name.as_bytes().to_vec();
                message.extend_from_slice(b"(): Argument #1 must be of type float, ");
                message.extend_from_slice(value.type_name().as_bytes());
                message.extend_from_slice(b" given");
                let _ = thrown;
                Err(self.throw("TypeError", message))
            }
            Err(OpError::Object) => Err(self.throw(
                "TypeError",
                format!("{name}(): Argument #1 must be of type float, object given").into_bytes(),
            )),
        }
    }

    fn too_few_at_least(&self, name: &str, passed: usize, expected: usize) -> crate::machine::Stop {
        let mut message = name.as_bytes().to_vec();
        message.extend_from_slice(b"() expects at least ");
        message.extend_from_slice(expected.to_string().as_bytes());
        message.extend_from_slice(if expected == 1 {
            b" argument, "
        } else {
            b" arguments, "
        });
        message.extend_from_slice(passed.to_string().as_bytes());
        message.extend_from_slice(b" given");
        self.throw_from("ArgumentCountError", message, name.as_bytes(), Vec::new())
    }
}

fn abs_int(n: i64) -> Value {
    n.checked_abs()
        .map_or(Value::Float(9_223_372_036_854_775_808.0), Value::Int)
}
