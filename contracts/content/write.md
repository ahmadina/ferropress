# Post insert, update and delete

How native crates write one row on the site `posts` table. Implemented by `crates/cms-content`
(`contracts/entries/content-write.toml`). Provenance: `wp-includes/post.php` and
`wp-includes/class-wpdb.php` in the pinned reference (inventory names only). PHP-visible
functions, hooks, sanitizing, unique slugs, dates, emoji encoding, meta, terms, revisions,
trash and the object cache are not bound yet. Values are the raw stored bytes.

## Columns

A write carries the compact insert list (no `ID`, no `comment_count`): `post_author`,
`post_date`, `post_date_gmt`, `post_content`, `post_content_filtered`, `post_title`,
`post_excerpt`, `post_status`, `post_type`, `comment_status`, `ping_status`, `post_password`,
`post_name`, `to_ping`, `pinged`, `post_modified`, `post_modified_gmt`, `post_parent`,
`menu_order`, `post_mime_type`, `guid`. Missing cells are empty bytes.

`post_author`, `post_parent`, `menu_order` and `ID` use the integer format (`%d`): a decimal
signed integer, unquoted. Bytes that are not a full integer become `0`. Every other column is
a MySQL string literal, same escapes as `contracts/options/load.md`.

## Insert

One statement. The table identifier and every column name are backtick-quoted:

```
INSERT INTO `{table}` (`post_author`, `post_date`, `post_date_gmt`, `post_content`, `post_content_filtered`, `post_title`, `post_excerpt`, `post_status`, `post_type`, `comment_status`, `ping_status`, `post_password`, `post_name`, `to_ping`, `pinged`, `post_modified`, `post_modified_gmt`, `post_parent`, `menu_order`, `post_mime_type`, `guid`) VALUES ({author}, {date}, {date_gmt}, {content}, {content_filtered}, {title}, {excerpt}, {status}, {type}, {comment_status}, {ping_status}, {password}, {name}, {to_ping}, {pinged}, {modified}, {modified_gmt}, {parent}, {menu_order}, {mime_type}, {guid})
```

Measured from `wpdb::insert` on `$wpdb->posts` after `wp_insert_post` compact (7.1.3). A
`Done` reply whose `insert_id` is a usable post id is inserted. `insert_id` zero, a row set,
or a server rejection is unchanged. A platform `Err` is not an unchanged write. The new row
is not remembered: the next get issues `SELECT`.

## Update

One statement. The same columns are written; the row is chosen by `ID`:

```
UPDATE `{table}` SET `post_author` = {author}, …, `guid` = {guid} WHERE `ID` = {id}
```

Measured from `wpdb::update( $wpdb->posts, $data, array( 'ID' => $post_id ) )`. A `Done`
reply with `affected` greater than zero is replaced; `affected` zero, a row set, or a server
rejection is unchanged. After the store answers, a remembered hit for that id is dropped.

## Delete

```
DELETE FROM `{table}` WHERE `ID` = {id}
```

Measured from `wpdb::delete( $wpdb->posts, array( 'ID' => $post_id ) )`. `Done` / rejection /
platform `Err` mean the same as update. After the store answers, a remembered hit for that id
is dropped.

## Not bound yet

`wp_insert_post` / `wp_update_post` / `wp_delete_post`, `wp_unslash`, unique `post_name`,
default dates and GMT conversion, emoji encoding, `import_id`, the follow-up `guid` rewrite,
`comment_count`, `wp_transition_post_status`, taxonomies, post meta, revisions, trash,
`clean_post_cache`, and the PHP-visible functions.
