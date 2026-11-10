// Ported from Kira (github.com/horizon-rust/kira, src/features/community.js `onButton`), MIT © 2026 Ludovic Marie and the Kira contributors; see LICENSES/kira-MIT.txt
//! What a click on a role button means.
use super::{
    message::ROLE_BUTTON,
    plan::{Known, ObjectKind},
};
use crate::{
    config::{Key, ServerConfig},
    ids::Snowflake,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Click {
    /// Not a role button: some other component.
    NotOurs,
    /// A role button for a role that is gone, not in the file, or not for self-assignment.
    Gone,
    Toggle {
        role: Snowflake,
        name: String,
        has: bool,
    },
}

/// A role is toggled only if the file marks it `self_assign`, whatever the button says.
pub fn click(
    config: &ServerConfig,
    known: &Known,
    custom_id: &str,
    member_roles: &[Snowflake],
) -> Click {
    let Some(key) = custom_id.strip_prefix(ROLE_BUTTON) else {
        return Click::NotOurs;
    };
    let spec = Key::new(key).ok().and_then(|k| {
        config
            .role(k.as_str())
            .filter(|r| r.self_assign)
            .map(|r| (k, r))
    });
    let Some((key, spec)) = spec else {
        return Click::Gone;
    };
    match known.get(ObjectKind::Role, &key) {
        Some(role) => Click::Toggle {
            role: role.id,
            name: spec.name.clone(),
            has: member_roles.contains(&role.id),
        },
        None => Click::Gone,
    }
}
