//! Port the native core uses to run a PHP callable (ADR-0003).

use crate::{Slot, Value};

/// How the native core asks the engine to run a PHP callable.
///
/// The callee is a live handle, never a copy. Arguments that are [`Slot::Reference`] are written
/// through when the callable takes that parameter by reference, so the caller sees the mutation.
/// Implementations must allow reentrant `invoke` on the same request (core → PHP → core → PHP).
pub trait Invoke {
    /// Why the call did not produce a PHP value (not callable, engine failure, thrown error).
    type Error;

    /// Runs `callee` with `args` and returns the value the callable returned.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] when the engine cannot run `callee` or when PHP throws and the error is not
    /// turned into a value.
    ///
    /// # Examples
    ///
    /// ```
    /// use php_value::{Invoke, Slot, Value};
    ///
    /// struct Echo;
    ///
    /// impl Invoke for Echo {
    ///     type Error = &'static str;
    ///
    ///     fn invoke(&mut self, _callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
    ///         Ok(args.first().map_or(Value::Null, Slot::value))
    ///     }
    /// }
    ///
    /// let mut echo = Echo;
    /// let got = echo.invoke(&Value::from("id"), &mut [Slot::from(Value::from(1_i64))])?;
    /// assert!(matches!(got, Value::Int(1)));
    /// # Ok::<(), &'static str>(())
    /// ```
    fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::Invoke;
    use crate::{Reference, Slot, Value};

    struct Script {
        calls: usize,
        reply: Value,
        write: Option<Value>,
    }

    impl Invoke for Script {
        type Error = &'static str;

        fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
            if matches!(callee, Value::Null) {
                return Err("not callable");
            }
            self.calls += 1;
            if let (Some(written), Some(Slot::Reference(cell))) = (self.write.clone(), args.first())
            {
                cell.set(written);
            }
            Ok(self.reply.clone())
        }
    }

    #[test]
    fn a_script_returns_the_callable_result() {
        let mut script = Script {
            calls: 0,
            reply: Value::from(3_i64),
            write: None,
        };
        let got = script
            .invoke(&Value::from("inc"), &mut [])
            .expect("callable");
        assert!(matches!(got, Value::Int(3)));
        assert_eq!(script.calls, 1);
    }

    #[test]
    fn a_reference_argument_is_written_through() {
        let cell = Reference::new(Value::from(1_i64));
        let mut script = Script {
            calls: 0,
            reply: Value::Null,
            write: Some(Value::from(2_i64)),
        };
        script
            .invoke(&Value::from("bump"), &mut [Slot::Reference(cell.clone())])
            .expect("callable");
        assert!(matches!(cell.get(), Value::Int(2)));
    }

    #[test]
    fn a_non_callable_is_an_error() {
        let mut script = Script {
            calls: 0,
            reply: Value::Null,
            write: None,
        };
        assert!(matches!(
            script.invoke(&Value::Null, &mut []),
            Err("not callable")
        ));
        assert_eq!(script.calls, 0);
    }
}
