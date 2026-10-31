//! Single-site capability mapping (`contracts/auth/map.md`).

use cms_users::UserId;

/// Who is being checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    /// No stored user (id less than 1).
    Anonymous,
    /// A stored user id.
    User(UserId),
}

/// Primitive names a capability requires after mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapped {
    names: Vec<Box<[u8]>>,
}

impl Mapped {
    fn one(name: &[u8]) -> Self {
        Self {
            names: vec![Box::from(name)],
        }
    }

    fn pair(first: &[u8], second: &[u8]) -> Self {
        Self {
            names: vec![Box::from(first), Box::from(second)],
        }
    }

    /// Primitive names in mapped order.
    pub fn names(&self) -> impl Iterator<Item = &[u8]> {
        self.names.iter().map(AsRef::as_ref)
    }
}

/// Primitive names required for `cap` on a single site with no object id.
#[must_use]
pub fn map_meta(cap: &[u8], actor: Actor) -> Mapped {
    match cap {
        b"edit_user" | b"edit_users" => match actor {
            Actor::Anonymous => Mapped::one(b"do_not_allow"),
            Actor::User(_) => Mapped::one(b"edit_users"),
        },
        b"update_https" => Mapped::pair(b"manage_options", b"update_core"),
        b"delete_site" => Mapped::one(b"do_not_allow"),
        other => Mapped::one(alias(other)),
    }
}

fn alias(cap: &[u8]) -> &[u8] {
    for (from, to) in ALIASES {
        if *from == cap {
            return to;
        }
    }
    for (from, to) in BLOCKS {
        if *from == cap {
            return to;
        }
    }
    cap
}

const ALIASES: &[(&[u8], &[u8])] = &[
    (b"promote_user", b"promote_users"),
    (b"add_users", b"promote_users"),
    (b"remove_user", b"remove_users"),
    (b"delete_user", b"delete_users"),
    (b"delete_users", b"delete_users"),
    (b"resume_plugin", b"resume_plugins"),
    (b"resume_theme", b"resume_themes"),
    (b"customize", b"edit_theme_options"),
    (b"manage_post_tags", b"manage_categories"),
    (b"edit_categories", b"manage_categories"),
    (b"edit_post_tags", b"manage_categories"),
    (b"delete_categories", b"manage_categories"),
    (b"delete_post_tags", b"manage_categories"),
    (b"assign_categories", b"edit_posts"),
    (b"assign_post_tags", b"edit_posts"),
    (b"setup_network", b"manage_options"),
    (b"update_php", b"update_core"),
    (b"export_others_personal_data", b"manage_options"),
    (b"erase_others_personal_data", b"manage_options"),
    (b"manage_privacy_options", b"manage_options"),
];

const BLOCKS: &[(&[u8], &[u8])] = &[
    (b"edit_blocks", b"edit_posts"),
    (b"edit_others_blocks", b"edit_others_posts"),
    (b"publish_blocks", b"publish_posts"),
    (b"read_private_blocks", b"read_private_posts"),
    (b"delete_blocks", b"delete_posts"),
    (b"delete_private_blocks", b"delete_private_posts"),
    (b"delete_published_blocks", b"delete_published_posts"),
    (b"delete_others_blocks", b"delete_others_posts"),
    (b"edit_private_blocks", b"edit_private_posts"),
    (b"edit_published_blocks", b"edit_published_posts"),
];

#[cfg(test)]
mod tests {
    use cms_users::UserId;

    use super::{Actor, map_meta};

    fn user() -> Actor {
        let id = UserId::new(3);
        assert!(id.is_ok());
        if let Ok(id) = id {
            return Actor::User(id);
        }
        Actor::Anonymous
    }

    fn assert_names(cap: &[u8], actor: Actor, expected: &[&[u8]]) {
        let mapped = map_meta(cap, actor);
        let names: Vec<&[u8]> = mapped.names().collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn remaps_and_identity() {
        let actor = user();
        assert_names(b"promote_user", actor, &[b"promote_users"]);
        assert_names(b"add_users", actor, &[b"promote_users"]);
        assert_names(b"remove_user", actor, &[b"remove_users"]);
        assert_names(b"delete_user", actor, &[b"delete_users"]);
        assert_names(b"delete_users", actor, &[b"delete_users"]);
        assert_names(b"resume_plugin", actor, &[b"resume_plugins"]);
        assert_names(b"resume_theme", actor, &[b"resume_themes"]);
        assert_names(b"customize", actor, &[b"edit_theme_options"]);
        assert_names(b"delete_site", actor, &[b"do_not_allow"]);
        assert_names(b"manage_post_tags", actor, &[b"manage_categories"]);
        assert_names(b"edit_categories", actor, &[b"manage_categories"]);
        assert_names(b"edit_post_tags", actor, &[b"manage_categories"]);
        assert_names(b"delete_categories", actor, &[b"manage_categories"]);
        assert_names(b"delete_post_tags", actor, &[b"manage_categories"]);
        assert_names(b"assign_categories", actor, &[b"edit_posts"]);
        assert_names(b"assign_post_tags", actor, &[b"edit_posts"]);
        assert_names(b"setup_network", actor, &[b"manage_options"]);
        assert_names(b"update_php", actor, &[b"update_core"]);
        assert_names(b"update_https", actor, &[b"manage_options", b"update_core"]);
        assert_names(b"export_others_personal_data", actor, &[b"manage_options"]);
        assert_names(b"erase_others_personal_data", actor, &[b"manage_options"]);
        assert_names(b"manage_privacy_options", actor, &[b"manage_options"]);
        assert_names(b"edit_blocks", actor, &[b"edit_posts"]);
        assert_names(b"edit_others_blocks", actor, &[b"edit_others_posts"]);
        assert_names(b"publish_blocks", actor, &[b"publish_posts"]);
        assert_names(b"read_private_blocks", actor, &[b"read_private_posts"]);
        assert_names(b"delete_blocks", actor, &[b"delete_posts"]);
        assert_names(b"delete_private_blocks", actor, &[b"delete_private_posts"]);
        assert_names(
            b"delete_published_blocks",
            actor,
            &[b"delete_published_posts"],
        );
        assert_names(b"delete_others_blocks", actor, &[b"delete_others_posts"]);
        assert_names(b"edit_private_blocks", actor, &[b"edit_private_posts"]);
        assert_names(b"edit_published_blocks", actor, &[b"edit_published_posts"]);
        assert_names(b"manage_options", actor, &[b"manage_options"]);
        assert_names(b"exist", actor, &[b"exist"]);
        assert_names(b"create_users", actor, &[b"create_users"]);
        assert_names(b"edit_user", actor, &[b"edit_users"]);
        assert_names(b"edit_users", actor, &[b"edit_users"]);
    }

    #[test]
    fn anonymous_edit_user_is_denied() {
        assert_names(b"edit_user", Actor::Anonymous, &[b"do_not_allow"]);
        assert_names(b"edit_users", Actor::Anonymous, &[b"do_not_allow"]);
        assert_names(b"manage_options", Actor::Anonymous, &[b"manage_options"]);
    }
}
