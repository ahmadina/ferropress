# Term get by id

How native crates read one joined row from the site `terms` and `term_taxonomy` tables.
Implemented by `crates/cms-taxonomy` (`contracts/entries/taxonomy-get.toml`). Provenance:
`wp-includes/class-wp-term.php` in the pinned reference (inventory names only). PHP-visible
objects, filters, sanitizing, the taxonomy registry, writes, object relationships and the
object cache are not bound yet.

## Identifiers

A term id is an unsigned integer greater than zero. Zero and every non-numeric value are a
constructor error and do not hit the store.

## Join

A first read of an id issues one statement. Both physical table names (prefix + `terms`,
prefix + `term_taxonomy`) are unquoted. The id is a decimal integer (`%d`, no quotes). There
is no `LIMIT`:

```
SELECT t.*, tt.* FROM {terms} AS t INNER JOIN {term_taxonomy} AS tt ON t.term_id = tt.term_id WHERE t.term_id = {id}
```

Measured from `WP_Term::get_instance` (7.1.3). Each result row is the cells the server sent,
keyed by the column names in the inventory `terms` and `term_taxonomy` tables. SQL `NULL`
becomes empty bytes. A row whose `term_id` cell is not a usable id is ignored.

## Taxonomy filter

An empty taxonomy matches any join row. A non-empty taxonomy keeps the first row whose
`taxonomy` cell equals those bytes.

- No usable row: miss.
- Empty filter and exactly one usable row: one term.
- Empty filter and more than one usable row: shared. The reference can still return one row
  when only one of the taxonomies is registered; that registry is not bound here, so every
  multi-row empty-filter read is shared.
- Non-empty filter and a matching row: one term, even when other taxonomies share the id.

A miss is no usable row, a server rejection, or a `Done` reply. A miss is not remembered. A
platform `Err` from the store is not a miss.

A later get of the same id in this request does not hit the store again only when the first
select produced exactly one usable row (the reference does not cache shared terms). A cached
row is ignored when a non-empty filter does not match its `taxonomy` cell; that get issues
the statement again.

## Returned row

The value is the stored bytes, not sanitized and not filtered. The `filter` flag is not here.

## Not bound yet

`get_term` output formats, `sanitize_term`, `taxonomy_exists`, `ambiguous_term_id` /
`invalid_taxonomy` as PHP-visible errors, the `terms` object-cache group, `wp_insert_term` /
`wp_update_term` / `wp_delete_term`, `term_relationships`, and the PHP-visible term object.
