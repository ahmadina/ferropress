# Single-site capability mapping

How native crates turn one capability name into the primitive names that
`contracts/auth/has.md` checks. Implemented by `crates/cms-auth`
(`contracts/entries/auth-map.toml`). Provenance: `wp-includes/capabilities.php`
(`map_meta_cap`) in the pinned reference (inventory names only). The
`map_meta_cap` filter, post/comment/term object ids, options, file-mod
constants, multisite, and PHP-visible names are not bound yet.

This is the single-site mapping: there is no network and no super-admin.

## Who is checked

The actor is either a stored user id or anonymous (id less than 1). Anonymous is
the logged-out path. Only `edit_user` / `edit_users` read the actor when there
is no user object: anonymous maps those names to `do_not_allow`; a stored user
maps them to `edit_users`.

## User object

When a user object id is supplied:

- `edit_user` against the same stored actor requires no primitives (an empty
  list, which `contracts/auth/has.md` allows).
- `edit_user` against a different stored user is `edit_users`.
- `edit_users` never uses the object id; it stays `edit_users` for a stored
  actor and `do_not_allow` for anonymous.
- `remove_user` against the same stored actor is `do_not_allow`.
- `remove_user` against a different user, or with an anonymous actor, is
  `remove_users`.
- Anonymous `edit_user` stays `do_not_allow` even when an object id is present.
- Other names ignore the object id and use the remaps below.

## Remaps

These names become one or two primitives, in this order:

- `promote_user` and `add_users` become `promote_users`.
- `remove_user` without a user object becomes `remove_users`.
- `delete_user` and `delete_users` become `delete_users`.
- `resume_plugin` becomes `resume_plugins`.
- `resume_theme` becomes `resume_themes`.
- `customize` becomes `edit_theme_options`.
- `delete_site` becomes `do_not_allow`.
- `manage_post_tags`, `edit_categories`, `edit_post_tags`, `delete_categories`,
  and `delete_post_tags` become `manage_categories`.
- `assign_categories` and `assign_post_tags` become `edit_posts`.
- `setup_network` becomes `manage_options`.
- `update_php` becomes `update_core`.
- `update_https` becomes `manage_options` then `update_core`.
- `export_others_personal_data`, `erase_others_personal_data`, and
  `manage_privacy_options` become `manage_options`.
- `edit_blocks`, `edit_others_blocks`, `publish_blocks`, `read_private_blocks`,
  `delete_blocks`, `delete_private_blocks`, `delete_published_blocks`,
  `delete_others_blocks`, `edit_private_blocks`, and `edit_published_blocks`
  replace the `_blocks` suffix with `_posts`.

Every other name stays itself, including primitives such as `manage_options` and
`exist`.

## Not bound yet

Post, comment, term and meta object ids (`edit_post`, `edit_comment`), file-mod
and unfiltered constants, `activate_plugin` network extras, custom post-type
meta caps, and the `map_meta_cap` filter. `manage_links` is
`contracts/auth/links.md`. The current-user check is
`contracts/auth/current-can.md`.
