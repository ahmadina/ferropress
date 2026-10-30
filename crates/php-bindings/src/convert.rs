//! PHP values as hook names, priorities and argument counts.

use cms_types::{ArgCount, HookName, Priority};
use php_value::ops::Collect;
use php_value::{Value, ops::OpError};

pub(crate) fn hook_name(value: &Value) -> Result<HookName, OpError> {
    let mut cx = Collect::default();
    Ok(HookName::new(value.to_php_string(&mut cx)?.as_bytes()))
}

pub(crate) fn priority(value: Option<&Value>) -> Result<Priority, OpError> {
    match value {
        None => Ok(Priority::DEFAULT),
        Some(Value::Null) => Ok(Priority::NULL),
        Some(other) => {
            let mut cx = Collect::default();
            let number = other.to_int(&mut cx)?;
            let clipped =
                i32::try_from(number).unwrap_or(if number < 0 { i32::MIN } else { i32::MAX });
            Ok(Priority::new(clipped))
        }
    }
}

pub(crate) fn accepted(value: Option<&Value>) -> Result<ArgCount, OpError> {
    match value {
        None => Ok(ArgCount::DEFAULT),
        Some(other) => {
            let mut cx = Collect::default();
            let number = other.to_int(&mut cx)?;
            let clipped = u32::try_from(number).unwrap_or(0);
            Ok(ArgCount::new(clipped))
        }
    }
}

pub(crate) fn is_absent_callback(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null | Value::Bool(false)))
}

pub(crate) fn is_absent_priority(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null | Value::Bool(false)))
}

#[cfg(test)]
mod tests {
    use super::{accepted, hook_name, is_absent_callback, priority};
    use cms_types::{ArgCount, Priority};
    use php_value::Value;

    #[test]
    fn defaults_and_null_priority_match_the_contract() -> Result<(), String> {
        assert_eq!(
            hook_name(&Value::from("title")).map_err(|error| format!("{error:?}"))?,
            cms_types::HookName::from("title")
        );
        assert_eq!(
            priority(None).map_err(|error| format!("{error:?}"))?,
            Priority::DEFAULT
        );
        assert_eq!(
            priority(Some(&Value::Null)).map_err(|error| format!("{error:?}"))?,
            Priority::NULL
        );
        assert_eq!(
            accepted(None).map_err(|error| format!("{error:?}"))?,
            ArgCount::DEFAULT
        );
        assert!(is_absent_callback(None));
        assert!(is_absent_callback(Some(&Value::Bool(false))));
        assert!(!is_absent_callback(Some(&Value::from("inc"))));
        Ok(())
    }
}
