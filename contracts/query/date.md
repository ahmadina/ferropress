# Date query vars

How native crates read `year`, `monthnum` and `day` after fill-and-parse, and
which archive-date flags they set. Implemented by `crates/cms-query`
(`contracts/entries/query-date.toml`). Provenance: `wp-includes/class-wp-query.php`
(`parse_query`) and `wp_checkdate` / PHP `checkdate` in the pinned reference
(inventory names only). The compact `m` key, week `w`, hour/minute/second,
SQL, hooks and PHP-visible names are not bound yet.

Each raw value is a string of bytes, or absent.

## Fill

An absent key is filled as empty bytes. Empty bytes are then parsed as below.

## Parse

A scalar `year`, `monthnum` or `day` is stored as `absint`: PHP `(int)` then
the absolute value. Missing or empty bytes store `0`. Negatives are not a 404
from parse alone.

`get_query_var` after this parse returns those stored integers.

## Archive flags

These flags are the archive-date half. The caller applies them only when no
singular key claimed the query (`contracts/query/singular.md`). Time keys
(`hour`, `minute`, `second`) are treated as unset.

Order:

1. A `day` greater than zero: when `monthnum` and `year` are also greater than
   zero and the Gregorian date is invalid (`checkdate`, years 1–32767), set
   404 and leave the date flags unset. Otherwise this is a day archive
   (`is_day` and `is_date`). A day without both month and year is still a day
   archive (the calendar check is skipped).
2. A `monthnum` greater than zero, when `is_date` is still unset: a month
   greater than 12 is a 404. Otherwise this is a month archive (`is_month`
   and `is_date`).
3. A `year` greater than zero, when `is_date` is still unset: this is a year
   archive (`is_year` and `is_date`).

A 404 from an earlier step does not stop a later step. An invalid day with a
valid month therefore becomes a month archive and a 404. `is_year` is false
when a day or month already set `is_date`.

## Not bound yet

`m`, `w`, `hour`, `minute`, `second`, `is_time`, the `wp_checkdate` filter,
date SQL, and combining these keys with a singular query.
