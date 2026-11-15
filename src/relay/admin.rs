//! Channel, role and forum shapes for the relay's management calls.
use poise::serenity_prelude as serenity;
use serde::Serialize;
use tonic::Status;

use super::pb;

pub const MAX_NAME: usize = 100;
pub const MAX_TOPIC: usize = 1024;
pub const MAX_TAG_NAME: usize = 20;
pub const MAX_TAGS: usize = 20;
/// Discord caps slowmode at six hours.
pub const MAX_SLOWMODE: u32 = 21_600;

pub fn kind(kind: serenity::ChannelType) -> pb::ChannelKind {
    use serenity::ChannelType as T;
    match kind {
        T::Text => pb::ChannelKind::Text,
        T::Voice => pb::ChannelKind::Voice,
        T::Category => pb::ChannelKind::Category,
        T::News => pb::ChannelKind::Announcement,
        T::Stage => pb::ChannelKind::Stage,
        T::Forum => pb::ChannelKind::Forum,
        T::Unknown(16) => pb::ChannelKind::Media,
        _ => pb::ChannelKind::Other,
    }
}

/// The kinds a client may create.
pub fn creatable(kind: pb::ChannelKind) -> Result<serenity::ChannelType, Status> {
    use serenity::ChannelType as T;
    match kind {
        pb::ChannelKind::Text => Ok(T::Text),
        pb::ChannelKind::Voice => Ok(T::Voice),
        pb::ChannelKind::Category => Ok(T::Category),
        pb::ChannelKind::Announcement => Ok(T::News),
        pb::ChannelKind::Forum => Ok(T::Forum),
        _ => Err(Status::invalid_argument(
            "kind must be text, voice, category, announcement or forum",
        )),
    }
}

pub fn is_forum(kind: serenity::ChannelType) -> bool {
    matches!(
        kind,
        serenity::ChannelType::Forum | serenity::ChannelType::Unknown(16)
    )
}

fn overwrite(o: &serenity::PermissionOverwrite) -> pb::Overwrite {
    let (target, target_id) = match o.kind {
        serenity::PermissionOverwriteType::Role(id) => (pb::OverwriteTarget::Role, id.get()),
        serenity::PermissionOverwriteType::Member(id) => (pb::OverwriteTarget::Member, id.get()),
        _ => (pb::OverwriteTarget::Unspecified, 0),
    };
    pb::Overwrite {
        target: target as i32,
        target_id,
        allow: o.allow.bits(),
        deny: o.deny.bits(),
    }
}

fn tag(t: &serenity::ForumTag) -> pb::ForumTag {
    pb::ForumTag {
        id: t.id.get(),
        name: t.name.clone(),
        moderated: t.moderated,
        emoji: match &t.emoji {
            Some(serenity::ForumEmoji::Name(name)) => name.clone(),
            _ => String::new(),
        },
    }
}

pub fn channel(c: &serenity::GuildChannel) -> pb::Channel {
    pb::Channel {
        id: c.id.get(),
        name: c.name.clone(),
        kind: kind(c.kind) as i32,
        parent_id: c.parent_id.map(|p| p.get()),
        position: c.position.into(),
        topic: c.topic.clone().unwrap_or_default(),
        nsfw: c.nsfw,
        rate_limit_per_user: c.rate_limit_per_user.unwrap_or_default().into(),
        default_thread_rate_limit_per_user: c
            .default_thread_rate_limit_per_user
            .unwrap_or_default()
            .into(),
        require_tag: c.flags.contains(serenity::ChannelFlags::REQUIRE_TAG),
        tags: c.available_tags.iter().map(tag).collect(),
        overwrites: c.permission_overwrites.iter().map(overwrite).collect(),
    }
}

pub fn role(r: &serenity::Role) -> pb::Role {
    pb::Role {
        id: r.id.get(),
        name: r.name.clone(),
        position: r.position.into(),
        permissions: r.permissions.bits(),
        managed: r.managed,
    }
}

/// Uncategorised channels first, then each category followed by its channels, all by position.
pub fn sort(channels: &mut [serenity::GuildChannel]) {
    let categories: Vec<(serenity::ChannelId, u16)> = channels
        .iter()
        .filter(|c| c.kind == serenity::ChannelType::Category)
        .map(|c| (c.id, c.position))
        .collect();
    let category = |id: serenity::ChannelId| {
        let pos = categories
            .iter()
            .find(|(c, _)| *c == id)
            .map_or(u16::MAX, |(_, pos)| *pos);
        (true, pos, id.get())
    };
    channels.sort_by_key(|c| {
        let group = match (c.kind, c.parent_id) {
            (serenity::ChannelType::Category, _) => category(c.id),
            (_, Some(parent)) => category(parent),
            (_, None) => (false, 0, 0),
        };
        let is_child = c.kind != serenity::ChannelType::Category;
        (group, is_child, c.position, c.id.get())
    });
}

pub fn text(field: &str, value: &str, min: usize, max: usize) -> Result<(), Status> {
    let n = value.trim().chars().count();
    if n < min || value.chars().count() > max {
        return Err(Status::invalid_argument(format!(
            "{field} must be {min} to {max} characters"
        )));
    }
    Ok(())
}

pub fn slowmode(field: &str, seconds: u32) -> Result<u16, Status> {
    if seconds > MAX_SLOWMODE {
        return Err(Status::invalid_argument(format!(
            "{field} is 0 to {MAX_SLOWMODE} seconds"
        )));
    }
    Ok(seconds as u16)
}

/// A PATCH body for Discord's channel endpoint; unset fields are left out so they don't change.
#[derive(Serialize, Default)]
pub struct EditBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsfw: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Option<serenity::ChannelId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_per_user: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_thread_rate_limit_per_user: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_tags: Option<Vec<TagBody>>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TagBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serenity::ForumTagId>,
    pub name: String,
    pub moderated: bool,
    pub emoji_name: Option<String>,
}

pub fn tag_bodies(tags: Vec<pb::ForumTag>) -> Result<Vec<TagBody>, Status> {
    if tags.len() > MAX_TAGS {
        return Err(Status::invalid_argument("a forum has at most 20 tags"));
    }
    tags.into_iter()
        .map(|t| {
            text("tag name", &t.name, 1, MAX_TAG_NAME)?;
            Ok(TagBody {
                id: (t.id != 0).then(|| serenity::ForumTagId::new(t.id)),
                name: t.name,
                moderated: t.moderated,
                emoji_name: (!t.emoji.is_empty()).then_some(t.emoji),
            })
        })
        .collect()
}

pub fn permissions(bits: u64) -> serenity::Permissions {
    serenity::Permissions::from_bits_truncate(bits)
}

/// Discord caps a timeout at 28 days.
pub const MAX_TIMEOUT: u32 = 28 * 24 * 3600;

/// Members the relay never moderates: the owner, bots, and anyone whose roles grant Administrator or Manage Server.
pub fn protected(
    user: serenity::UserId,
    is_bot: bool,
    owner: serenity::UserId,
    member_roles: &[serenity::RoleId],
    role_permissions: &std::collections::HashMap<serenity::RoleId, serenity::Permissions>,
) -> bool {
    let powerful = serenity::Permissions::ADMINISTRATOR | serenity::Permissions::MANAGE_GUILD;
    user == owner
        || is_bot
        || member_roles
            .iter()
            .filter_map(|id| role_permissions.get(id))
            .any(|perms| perms.intersects(powerful))
}

pub fn timeout(seconds: u32) -> Result<i64, Status> {
    if !(60..=MAX_TIMEOUT).contains(&seconds) {
        return Err(Status::invalid_argument(
            "timeout_seconds is 60 s to 28 days",
        ));
    }
    Ok(seconds.into())
}

pub fn emoji(text: &str) -> Result<serenity::ReactionType, Status> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Status::invalid_argument("emoji is required"));
    }
    serenity::ReactionType::try_from(text.to_string())
        .map_err(|_| Status::invalid_argument("emoji must be unicode or <:name:id>"))
}

pub fn action_name(action: pb::ModAction) -> &'static str {
    match action {
        pb::ModAction::Timeout => "timed out",
        pb::ModAction::Untimeout => "removed the timeout of",
        pb::ModAction::Kick => "kicked",
        pb::ModAction::Ban => "banned",
        pb::ModAction::Unban => "unbanned",
        pb::ModAction::Unspecified => "did nothing to",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_fields_stay_out_of_the_patch_and_null_clears_the_category() {
        let body = EditBody {
            topic: Some("hi".into()),
            parent_id: Some(None),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            serde_json::json!({"topic": "hi", "parent_id": null})
        );
    }

    #[test]
    fn tags_keep_their_ids_and_new_ones_get_none() {
        let got = tag_bodies(vec![
            pb::ForumTag {
                id: 7,
                name: "mod".into(),
                moderated: false,
                emoji: String::new(),
            },
            pb::ForumTag {
                id: 0,
                name: "WIP".into(),
                moderated: true,
                emoji: "🛠".into(),
            },
        ])
        .unwrap();
        assert_eq!(
            serde_json::to_value(&got).unwrap(),
            serde_json::json!([
                {"id": "7", "name": "mod", "moderated": false, "emoji_name": null},
                {"name": "WIP", "moderated": true, "emoji_name": "🛠"}
            ])
        );
        let long = pb::ForumTag {
            name: "x".repeat(21),
            ..Default::default()
        };
        assert!(tag_bodies(vec![long]).is_err());
    }

    #[test]
    fn slowmode_is_capped() {
        assert_eq!(slowmode("s", 600).unwrap(), 600);
        assert!(slowmode("s", 21_601).is_err());
    }

    #[test]
    fn owners_bots_and_admins_are_protected() {
        let admin = serenity::RoleId::new(10);
        let plain = serenity::RoleId::new(11);
        let roles = std::collections::HashMap::from([
            (admin, serenity::Permissions::MANAGE_GUILD),
            (plain, serenity::Permissions::SEND_MESSAGES),
        ]);
        let owner = serenity::UserId::new(1);
        let user = serenity::UserId::new(2);
        assert!(protected(owner, false, owner, &[], &roles));
        assert!(protected(user, true, owner, &[], &roles));
        assert!(protected(user, false, owner, &[plain, admin], &roles));
        assert!(!protected(user, false, owner, &[plain], &roles));
    }

    #[test]
    fn timeouts_and_emoji_are_checked() {
        assert!(timeout(59).is_err());
        assert_eq!(timeout(600).unwrap(), 600);
        assert!(timeout(MAX_TIMEOUT + 1).is_err());
        assert!(emoji(" ").is_err());
        assert!(matches!(
            emoji("💀").unwrap(),
            serenity::ReactionType::Unicode(_)
        ));
        assert!(matches!(
            emoji("<:hi:1179198518787842078>").unwrap(),
            serenity::ReactionType::Custom { .. }
        ));
    }
}
