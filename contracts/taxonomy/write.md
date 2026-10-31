# Term insert, update and delete

How native crates write the site `terms` and `term_taxonomy` tables. Implemented by
`crates/cms-taxonomy` (`contracts/entries/taxonomy-write.toml`). Provenance:
`wp-includes/taxonomy.php` and `wp-includes/class-wpdb.php` in the pinned reference
(inventory names only). PHP-visible functions, hooks, sanitizing, unique slugs, the
taxonomy registry, duplicate-term cleanup, term meta, object relationships and the
object cache are not bound yet. Values are the raw stored bytes.

## Columns

A terms write carries `name`, `slug`, and `term_group`. A taxonomy write carries
`taxonomy`, `description`, `parent`, and `count`. Missing cells are empty bytes.

`term_id`, `term_taxonomy_id`, `term_group`, `parent`, and `count` use the integer
format (`%d`): a decimal signed integer, unquoted. Bytes that are not a full integer
become `0`. Every other column is a MySQL string literal, same escapes as
`contracts/options/load.md`.

## Insert

Two statements. The table identifiers and every column name are backtick-quoted.
Measured from `wpdb::insert` on `$wpdb->terms` then `$wpdb->term_taxonomy` after
`wp_insert_term` compact (7.1.3):

```
INSERT INTO `{terms}` (`name`, `slug`, `term_group`) VALUES ({name}, {slug}, {term_group})
INSERT INTO `{term_taxonomy}` (`term_id`, `taxonomy`, `description`, `parent`, `count`) VALUES ({term_id}, {taxonomy}, {description}, {parent}, {count})
```

The second statement runs only when the first `Done` reply has a usable `insert_id`.
That id is the `term_id` written on the second row. A second `Done` whose `insert_id`
is a usable term taxonomy id is inserted. A zero `insert_id`, a row set, or a server
rejection on either statement is unchanged. A platform `Err` is not an unchanged
write. The new rows are not remembered: the next get issues `SELECT`. Unique slugs,
the pre-insert existence select, empty-slug follow-up, and duplicate-term cleanup are
not bound; a rejected second insert leaves the terms row, matching the reference
error path.

## Update

Two statements. The terms row is chosen by `term_id`; the taxonomy row by
`term_taxonomy_id`. `count` is not rewritten:

```
UPDATE `{terms}` SET `name` = {name}, `slug` = {slug}, `term_group` = {term_group} WHERE `term_id` = {term_id}
UPDATE `{term_taxonomy}` SET `term_id` = {term_id}, `taxonomy` = {taxonomy}, `description` = {description}, `parent` = {parent} WHERE `term_taxonomy_id` = {tt_id}
```

Measured from `wpdb::update` in `wp_update_term`. Both statements are issued when
the store answers. A `Done` reply with `affected` greater than zero on either
statement is replaced; both zero, row sets, or server rejections are unchanged.
After the store answers the first statement, a remembered hit for that term id is
dropped.

## Delete

The taxonomy row is removed first. Then one count, unquoted table, `%d` id:

```
DELETE FROM `{term_taxonomy}` WHERE `term_taxonomy_id` = {tt_id}
SELECT COUNT(*) FROM {term_taxonomy} WHERE term_id = {term_id}
```

When that count is zero, missing, unparseable, or the select is rejected, the terms
row is also removed (the reference treats a failed count as empty):

```
DELETE FROM `{terms}` WHERE `term_id` = {term_id}
```

When the count is greater than zero, the terms row stays (the id is still used by
another taxonomy). A `Done` reply with `affected` greater than zero on either delete
is removed. After the store answers the taxonomy delete, a remembered hit for that
term id is dropped.

## Not bound yet

`wp_insert_term` / `wp_update_term` / `wp_delete_term`, `wp_unique_term_slug`,
`sanitize_term`, `taxonomy_exists`, shared-term splitting, default-category
reassignment, term meta, `term_relationships`, `clean_term_cache`, and the
PHP-visible functions.
