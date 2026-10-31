# Page path lookup

How native crates find one page row from a slash-separated slug path.
Implemented by `crates/cms-content` (`contracts/entries/query-load-path.toml`).
Provenance: `get_page_by_path` in `wp-includes/post.php` in the pinned
reference (inventory names only). The object cache, output formats, array
post types, hooks and PHP-visible names are not bound yet.

The path is percent-decoded, re-encoded, then `%2F` and `%20` are restored to
`/` and space. Leading and trailing slashes are stripped and the remainder is
split on `/`. Each segment is passed through the query-title dash sanitizer
used as the default `sanitize_title` filter (`sanitize_title_for_query`: no
accent folding, query context, not save).

One statement is issued against the physical posts table (prefix + `posts`),
unquoted, with each name and type as a prepared string literal:

```
		SELECT ID, post_name, post_parent, post_type
		FROM {table}
		WHERE post_name IN ({names})
		AND post_type IN ('page','attachment')
```

The leading/trailing whitespace and tabs match the reference string. A string
post type also searches `attachment`. Rows are walked from the last segment
toward the root: each parent id must exist in the set and its `post_name` must
equal the next segment; the root parent must be `0` and the depth must match.
A page-type hit wins over an attachment hit at the same path. A hit then loads
that id through `contracts/content/get.md`. A miss is no row.

## Not bound yet

`sanitize_title` / `sanitize_title_with_dashes` save-context replacements,
Unicode case folding parity, HTML `strip_tags` trees, the posts object-cache
group, array `$post_type`, and `get_post` output formats.
