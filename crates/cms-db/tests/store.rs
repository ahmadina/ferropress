//! SQL store port (`contracts/db/store.md`).

use cms_db::{
    Cell, NameError, Reply, Scripted, Sql, Store, StoreError, TablePrefix, TableStem,
    store_contract,
};

#[test]
fn scripted_store_passes_the_trait_contract() {
    store_contract(Scripted::new, |store| {
        assert!(store.seen().len() == 3);
        assert!(store.query(&Sql::from("SELECT 1")).is_err());
    });
}

#[test]
fn a_mismatched_statement_is_unavailable() {
    let sql = Sql::from("SELECT 1");
    let mut store = Scripted::once(sql.clone(), Reply::single_column("n", [Cell::bytes("1")]));
    let other = store.query(&Sql::from("SELECT 2"));
    assert!(matches!(other, Err(StoreError::Unavailable)));
    assert!(other.as_ref().is_err_and(StoreError::is_unavailable));
    let reply = store.query(&sql);
    assert!(reply.as_ref().is_ok_and(|value| value.as_rows().is_some()));
    assert_eq!(store.seen(), [sql]);
}

#[test]
fn table_names_follow_the_inventory_stems() {
    let prefix = TablePrefix::new("fp_");
    let options = TableStem::new("options");
    let posts = TableStem::new("posts");
    assert!(prefix.is_ok() && options.is_ok() && posts.is_ok());
    if let (Ok(prefix), Ok(options), Ok(posts)) = (prefix, options, posts) {
        assert_eq!(prefix.table(&options).as_bytes(), b"fp_options");
        assert_eq!(prefix.table(&posts).as_bytes(), b"fp_posts");
    }
    assert!(matches!(TableStem::new(""), Err(NameError::Empty)));
}
