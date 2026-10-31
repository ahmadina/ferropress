//! Request-local object cache (`contracts/cache/memory.md`). Cases follow the 7.1.3 phpunit key table.

use cms_cache::{CacheKey, Group, Memory};

fn key(name: &str) -> CacheKey {
    let key = CacheKey::new(name);
    assert!(key.is_ok());
    match key {
        Ok(key) => key,
        Err(_) => CacheKey::from_int(0),
    }
}

#[test]
fn add_get_and_replace_follow_presence() {
    let mut cache = Memory::new();
    let name = key("k");
    let group = Group::new("");
    assert!(!cache.replace(&name, b"no", &group));
    assert!(cache.get(&name, &group).is_none());
    assert!(cache.add(&name, b"v1", &group));
    assert_eq!(cache.get(&name, &group).as_deref(), Some(b"v1".as_slice()));
    assert!(!cache.add(&name, b"v2", &group));
    assert_eq!(cache.get(&name, &group).as_deref(), Some(b"v1".as_slice()));
    assert!(cache.replace(&name, b"v2", &group));
    assert_eq!(cache.get(&name, &group).as_deref(), Some(b"v2".as_slice()));
    cache.set(&name, b"", &group);
    assert_eq!(cache.get(&name, &group).as_deref(), Some(b"".as_slice()));
    assert!(cache.delete(&name, &group));
    assert!(!cache.delete(&name, &group));
    assert!(cache.get(&name, &group).is_none());
    assert!(cache.hits() >= 4);
    assert!(cache.misses() >= 2);
}

#[test]
fn groups_and_empty_names_are_separate_slots() {
    let mut cache = Memory::default();
    let name = key("k");
    let default = Group::new("");
    let zero = Group::new("0");
    let other = Group::new("posts");
    let space = Group::new(" ");
    cache.set(&name, b"def", &default);
    cache.set(&name, b"post", &other);
    cache.set(&name, b"sp", &space);
    assert_eq!(zero.as_bytes(), default.as_bytes());
    assert_eq!(cache.get(&name, &zero).as_deref(), Some(b"def".as_slice()));
    assert_eq!(
        cache.get(&name, &other).as_deref(),
        Some(b"post".as_slice())
    );
    assert_eq!(cache.get(&name, &space).as_deref(), Some(b"sp".as_slice()));
    cache.flush_group(&Group::exact(""));
    assert_eq!(
        cache.get(&name, &default).as_deref(),
        Some(b"def".as_slice())
    );
    cache.flush_group(&Group::default_group());
    assert!(cache.get(&name, &default).is_none());
    assert_eq!(
        cache.get(&name, &other).as_deref(),
        Some(b"post".as_slice())
    );
}

#[test]
fn network_prefix_skips_global_groups() {
    let mut cache = Memory::network(1);
    let name = key("home");
    let local = Group::new("options");
    let global = Group::new("users");
    cache.add_global(global.clone());
    cache.set(&name, b"one", &local);
    cache.set(&name, b"user", &global);
    cache.switch_to(2);
    assert!(cache.get(&name, &local).is_none());
    assert_eq!(
        cache.get(&name, &global).as_deref(),
        Some(b"user".as_slice())
    );
    cache.set(&name, b"two", &local);
    cache.switch_to(1);
    assert_eq!(cache.get(&name, &local).as_deref(), Some(b"one".as_slice()));
    let mut single = Memory::new();
    single.set(&name, b"s", &local);
    single.switch_to(9);
    assert_eq!(single.get(&name, &local).as_deref(), Some(b"s".as_slice()));
}

#[test]
fn flush_clears_the_bag() {
    let mut cache = Memory::new();
    let name = key("k");
    let group = Group::new("g");
    cache.set(&name, b"v", &group);
    cache.flush();
    assert!(cache.get(&name, &group).is_none());
    cache.flush_group(&group);
    let shown = format!("{cache:?}");
    assert!(shown.contains("Memory"));
}

#[test]
fn integer_zero_is_a_key() {
    let mut cache = Memory::new();
    let zero = CacheKey::from_int(0);
    let group = Group::new("");
    assert!(cache.add(&zero, b"z", &group));
    assert_eq!(cache.get(&zero, &group).as_deref(), Some(b"z".as_slice()));
    let as_string = key("0");
    assert_eq!(
        cache.get(&as_string, &group).as_deref(),
        Some(b"z".as_slice())
    );
}
