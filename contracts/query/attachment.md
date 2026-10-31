# Query vars `attachment` and `attachment_id`

How native crates read the attachment query vars after fill-and-parse.
Implemented by `crates/cms-query` (`contracts/entries/query-attachment.toml`).
Provenance: `wp-includes/class-wp-query.php` (`fill_query_vars`, `parse_query`)
and `wp-includes/load.php` (`absint`) in the pinned reference (inventory names
only). Object matching, SQL, hooks and PHP-visible names are not bound yet.

`attachment` names an attachment slug. `attachment_id` names an attachment id.
Each raw value is a string of bytes, or absent. Compat keys `subpost` and
`subpost_id` may replace them.

## Fill

An absent key is filled as empty bytes. Empty bytes are then parsed as below.

## Parse

When `subpost` is a scalar that is not loosely equal to empty (`!= ''`,
`contracts/runtime/operators.md`), it replaces `attachment`. When `subpost_id`
is a scalar that is not loosely equal to empty, it replaces `attachment_id`.
Empty bytes do not replace. The string `0` is not loosely equal to empty
(both sides are strings), so it does replace. An integer `0` would not, and
is not bound here.

`attachment` is stored as those bytes with no trim. `attachment_id` is stored
as `absint` (PHP `(int)` then the absolute value). Missing or empty id bytes
store `0`.

`get_query_var( 'attachment' )` returns the stored slug. `get_query_var(
'attachment_id' )` returns the stored id.

`is_attachment` and `is_single` are true when the stored slug is not empty
(strict: the bytes `0` and a lone space both count) or the stored id is
greater than zero.

## Not bound yet

Non-scalar values (arrays/objects), matching a specific attachment argument,
SQL, and the rest of `WP_Query`.
