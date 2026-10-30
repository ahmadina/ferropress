//! Request owns one hook table (`contracts/hooks/dispatch.md`).

use cms_request::{Flags, Request};
use cms_types::{ArgCount, HookName, Priority};
use php_value::Value;

#[test]
fn hooks_survive_flag_changes() {
    let mut request = Request::with_flags(Flags {
        installing: true,
        recovery: false,
        network: false,
    });
    request.hooks().add(
        HookName::from("title"),
        Value::from("edit"),
        Priority::DEFAULT,
        ArgCount::DEFAULT,
    );
    request.set_flags(Flags::default());
    assert!(request.hooks().has(&HookName::from("title")));
    assert!(!request.flags().installing);
}
