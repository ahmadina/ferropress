# Object–term relationships

How native crates read and write the site `term_relationships` table. Implemented by
`crates/cms-taxonomy` (`contracts/entries/taxonomy-link.toml`). Provenance:
`wp-includes/taxonomy.php` and `wp-includes/class-wpdb.php` in the pinned reference
(inventory names only). PHP-visible functions, hooks, the taxonomy registry, term-count
updates, append-vs-replace sets, `term_order` sorting and the object cache are not bound
yet.

## Identifiers

An object id and a term taxonomy id are unsigned integers greater than zero. Zero is a
constructor error and does not hit the store.

## Exists

One statement. The table is unquoted. Both ids are decimal integers (`%d`, no quotes):

```
SELECT term_taxonomy_id FROM {relationships} WHERE object_id = {object} AND term_taxonomy_id = {tt}
```

Measured from the existence check in `wp_set_object_terms` (7.1.3). A usable
`term_taxonomy_id` cell is a hit. No row, a rejected statement, or a `Done` reply is a
miss. A platform `Err` is not a miss.

## Objects for one taxonomy row

```
SELECT object_id FROM {relationships} WHERE term_taxonomy_id = {tt}
```

Measured from `wp_delete_term`. Each usable `object_id` cell is kept, in server order.
Junk and zero cells are skipped. A rejected select is an empty list.

## Attach

If the pair already exists, the outcome is unchanged and no `INSERT` is issued. Otherwise:

```
INSERT INTO `{relationships}` (`object_id`, `term_taxonomy_id`) VALUES ({object}, {tt})
```

Measured from `wpdb::insert` on `$wpdb->term_relationships`. Both values are unquoted
integers. `term_order` is omitted (the column default is `0`). A `Done` reply with
`affected` greater than zero is attached; `affected` zero or a server rejection is
unchanged.

## Detach

An empty id list issues no statement and is unchanged. Otherwise one statement. The table
is unquoted. The object id is `%d`. The `IN` list is quoted decimal integers, matching the
interpolated list the reference builds:

```
DELETE FROM {relationships} WHERE object_id = {object} AND term_taxonomy_id IN ('{tt}', …)
```

Measured from `wp_remove_object_terms`. A `Done` reply with `affected` greater than zero
is detached; zero or a rejection is unchanged.

## Not bound yet

`wp_set_object_terms` / `wp_add_object_terms` / `wp_remove_object_terms` /
`wp_get_object_terms`, `taxonomy_exists`, `term_exists`, creating missing terms, replacing
a taxonomy set, `term_order` rewrite, `wp_update_term_count`, relationship object-cache
groups, and the PHP-visible functions.
