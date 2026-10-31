# Pagename path load

How native crates load the post row when the singular winner is a pagename
path. Implemented by `crates/cms-query` (`contracts/entries/query-load-path.toml`).
Provenance: `wp-includes/class-wp-query.php` (`get_posts` pagename branch)
and `get_page_by_path` in the pinned reference (inventory names only). Name
slugs, status/type filters on the main SELECT, joins, pagination, hooks and
PHP-visible names are not bound yet.

The winner is `contracts/query/singular.md`. When that winner is a page and
`pagename` is not empty, the path is resolved through
`contracts/content/path.md`. A hit is that row. A miss is an empty list. An
id winner (`p`, `page_id`, attachment id) still uses
`contracts/query/load-id.md` and does not issue the path statement.

## Not bound yet

`name` / attachment slug `post_name` clauses, `page_for_posts`, combining
`pagename` with `page_id`, and the full `get_posts` SELECT assembly.
