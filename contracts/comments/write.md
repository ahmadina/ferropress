# Comment insert, update and delete

How native crates write one row on the site `comments` table. Implemented by
`crates/cms-comments` (`contracts/entries/comment-write.toml`). Provenance:
`wp-includes/comment.php` and `wp-includes/class-wpdb.php` in the pinned reference
(inventory names only). PHP-visible functions, hooks, sanitizing, default dates,
comment type defaults, emoji encoding, meta, children, trash, comment counts and
the object cache are not bound yet. Values are the raw stored bytes.

## Columns

A write carries the fourteen stored cells (no `comment_ID`): `comment_post_ID`,
`comment_author_IP`, `comment_author`, `comment_author_email`, `comment_author_url`,
`comment_date`, `comment_date_gmt`, `comment_content`, `comment_karma`,
`comment_approved`, `comment_agent`, `comment_type`, `comment_parent`, `user_id`.
Missing cells are empty bytes.

`comment_post_ID`, `comment_karma`, `comment_parent`, `user_id` and `comment_ID`
use the integer format (`%d`): a decimal signed integer, unquoted. Bytes that are
not a full integer become `0`. Every other column is a MySQL string literal, same
escapes as `contracts/options/load.md`. `comment_approved` is a string.

Insert and update walk those cells in different orders, as measured below.

## Insert

One statement. The table identifier and every column name are backtick-quoted.
Column order is the compact list after `comment_post_ID` and `comment_author_IP`
are placed first:

```
INSERT INTO `{table}` (`comment_post_ID`, `comment_author_IP`, `comment_author`, `comment_author_email`, `comment_author_url`, `comment_date`, `comment_date_gmt`, `comment_content`, `comment_karma`, `comment_approved`, `comment_agent`, `comment_type`, `comment_parent`, `user_id`) VALUES ({post}, {ip}, {author}, {email}, {url}, {date}, {date_gmt}, {content}, {karma}, {approved}, {agent}, {type}, {parent}, {user})
```

Measured from `wpdb::insert` on `$wpdb->comments` after `wp_insert_comment`
compact (7.1.3). A `Done` reply whose `insert_id` is a usable comment id is
inserted. `insert_id` zero, a row set, or a server rejection is unchanged. A
platform `Err` is not an unchanged write. The new row is not remembered: the next
get issues `SELECT`.

## Update

One statement. The same cells are written; the walk order puts `comment_author_IP`
after `comment_author_url`. The row is chosen by `comment_ID`:

```
UPDATE `{table}` SET `comment_post_ID` = {post}, `comment_author` = {author}, `comment_author_email` = {email}, `comment_author_url` = {url}, `comment_author_IP` = {ip}, `comment_date` = {date}, `comment_date_gmt` = {date_gmt}, `comment_content` = {content}, `comment_karma` = {karma}, `comment_approved` = {approved}, `comment_agent` = {agent}, `comment_type` = {type}, `comment_parent` = {parent}, `user_id` = {user} WHERE `comment_ID` = {id}
```

Measured from `wpdb::update( $wpdb->comments, $data, array( 'comment_ID' => $id ) )`
after the `wp_update_comment` key list. A `Done` reply with `affected` greater
than zero is replaced; `affected` zero, a row set, or a server rejection is
unchanged. After the store answers, a remembered hit for that id is dropped.

## Delete

```
DELETE FROM `{table}` WHERE `comment_ID` = {id}
```

Measured from `wpdb::delete( $wpdb->comments, array( 'comment_ID' => $id ) )`.
`Done` / rejection / platform `Err` mean the same as update. After the store
answers, a remembered hit for that id is dropped.

## Not bound yet

`wp_insert_comment` / `wp_update_comment` / `wp_delete_comment`, `wp_unslash`,
default dates and GMT conversion, default `comment_type`, approved-status aliases,
emoji encoding, comment meta, children re-parenting, trash, `wp_update_comment_count`,
`clean_comment_cache`, status transitions, and the PHP-visible functions.
