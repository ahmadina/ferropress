# Option add, update and delete

How native crates write the site `options` table. Implemented by `crates/cms-options`
(`contracts/entries/option-write.toml`). Provenance: `wp-includes/option.php` in the pinned
reference (inventory names only). PHP-visible names, hooks, serialization, sanitizing and the
object cache are not bound yet. Values are the raw stored bytes.

## Names

The same trim and rewrite rules as `contracts/options/load.md`. The names `alloptions` and
`notoptions` are not written: the outcome is protected, not a store error. The reference dies
when those names are written through the PHP-visible functions; that fatal is not bound here.

## Autoload tokens

A write stores one of three tokens: `on`, `off`, or `auto`. A boolean flag maps true to `on`
and false to `off`. The tokens `yes` and `on` are treated as loaded; `no` and `off` are not.
`auto`, `auto-on` and `auto-off` are the family the reference re-evaluates when an update omits
the autoload argument.

## Add

If the name already has stored bytes (autoload bag, a remembered single, or one `SELECT
option_value`), the outcome is unchanged and no `INSERT` is issued. Otherwise:

```
INSERT INTO `{table}` (`option_name`, `option_value`, `autoload`) VALUES ({name}, {value}, {token}) ON DUPLICATE KEY UPDATE `option_name` = VALUES(`option_name`), `option_value` = VALUES(`option_value`), `autoload` = VALUES(`autoload`)
```

The table identifier is backtick-quoted. Name, value and token use the same MySQL string
literal as load (`contracts/options/load.md`). A `Done` reply with `affected` greater than
zero is changed; `affected` zero or a server rejection is unchanged. A platform `Err` is not
an unchanged write.

A changed add that is loaded (`on` or `auto`) is kept in the autoload bag. A changed add that
is `off` is kept as a single-name hit and dropped from the bag. Either way the name is removed
from the remembered-miss set.

## Update

If the name is missing, the write is an add (autoload defaults to `auto` when omitted). If the
stored bytes already equal the new bytes, the outcome is unchanged even when an autoload
argument is supplied. Comparison is the raw stored bytes, not the slash-stripped get result.

When the bytes differ and autoload was omitted, one statement is issued first (unquoted table,
no `LIMIT`):

```
SELECT autoload FROM {table} WHERE option_name = {quoted}
```

If that cell is `auto-on`, `auto-off` or `auto`, and is not already `auto`, the following
`UPDATE` also sets `autoload` to `auto`. Otherwise only the value is written:

```
UPDATE `{table}` SET `option_value` = {value} WHERE `option_name` = {quoted}
UPDATE `{table}` SET `option_value` = {value}, `autoload` = {token} WHERE `option_name` = {quoted}
```

`Done` / rejection / platform `Err` mean the same as add. After a changed update, the bag or
single-name cache is refreshed the same way as add when a token was written; when the token
was omitted, the new bytes replace the existing bag or single-name entry.

## Delete

Existence is the autoload column (same `SELECT` as update). No row, or a rejected select, is
unchanged. Otherwise:

```
DELETE FROM `{table}` WHERE `option_name` = {quoted}
```

A changed delete drops the name from the autoload bag when the token was loaded (`yes`, `on`,
`auto-on`, `auto`) and from the single-name cache otherwise, then remembers a miss.

## Not bound yet

`pre_option` / `add_option` / `update_option` / `delete_option` and their dynamic twins,
`sanitize_option`, `maybe_serialize`, the default-value size filter, the persistent object
cache, network/site options, transients, and the PHP-visible functions.
