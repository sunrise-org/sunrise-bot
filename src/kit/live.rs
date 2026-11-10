//! The REST shape of what the planner reads from Discord. Unknown fields are ignored: Discord adds them.
use serde::{Deserialize, Deserializer};

use super::perms::Permissions;
use crate::{config::ChannelKind, ids::Snowflake};

/// The guild as the planner sees it, taken in one go.
#[derive(Debug, Clone)]
pub struct Live {
    pub bot_id: Snowflake,
    pub guild: LiveGuild,
    pub roles: Vec<LiveRole>,
    pub channels: Vec<LiveChannel>,
}

impl Live {
    pub fn community(&self) -> bool {
        self.guild.features.iter().any(|f| f == "COMMUNITY")
    }

    pub fn role(&self, id: Snowflake) -> Option<&LiveRole> {
        self.roles.iter().find(|r| r.id == id)
    }

    pub fn channel(&self, id: Snowflake) -> Option<&LiveChannel> {
        self.channels.iter().find(|c| c.id == id)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LiveGuild {
    pub id: Snowflake,
    pub name: String,
    pub verification_level: u8,
    pub explicit_content_filter: u8,
    pub default_message_notifications: u8,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub rules_channel_id: Option<Snowflake>,
    #[serde(default)]
    pub public_updates_channel_id: Option<Snowflake>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LiveRole {
    pub id: Snowflake,
    pub name: String,
    pub color: u32,
    #[serde(default)]
    pub hoist: bool,
    #[serde(default)]
    pub mentionable: bool,
    pub permissions: Permissions,
    pub position: i32,
    #[serde(default)]
    pub managed: bool,
}

/// Discord's channel type number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveKind {
    Text,
    Voice,
    Category,
    Announcement,
    Forum,
    Other(u8),
}

impl LiveKind {
    pub fn number(self) -> u8 {
        match self {
            LiveKind::Text => 0,
            LiveKind::Voice => 2,
            LiveKind::Category => 4,
            LiveKind::Announcement => 5,
            LiveKind::Forum => 15,
            LiveKind::Other(n) => n,
        }
    }

    pub fn of(kind: ChannelKind) -> LiveKind {
        match kind {
            ChannelKind::Text => LiveKind::Text,
            ChannelKind::Announcement => LiveKind::Announcement,
            ChannelKind::Forum => LiveKind::Forum,
            ChannelKind::Voice => LiveKind::Voice,
        }
    }

    /// Text and announcement channels stand for each other (an announcement channel is a text channel before Community).
    pub fn same_as(self, other: LiveKind) -> bool {
        let textish = |k: LiveKind| matches!(k, LiveKind::Text | LiveKind::Announcement);
        self == other || (textish(self) && textish(other))
    }
}

impl<'de> Deserialize<'de> for LiveKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<LiveKind, D::Error> {
        Ok(match u8::deserialize(d)? {
            0 => LiveKind::Text,
            2 => LiveKind::Voice,
            4 => LiveKind::Category,
            5 => LiveKind::Announcement,
            15 => LiveKind::Forum,
            n => LiveKind::Other(n),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverwriteKind {
    Role,
    Member,
}

impl<'de> Deserialize<'de> for OverwriteKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<OverwriteKind, D::Error> {
        match u8::deserialize(d)? {
            0 => Ok(OverwriteKind::Role),
            1 => Ok(OverwriteKind::Member),
            n => Err(serde::de::Error::custom(format!(
                "unknown overwrite type {n}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Overwrite {
    pub id: Snowflake,
    #[serde(rename = "type")]
    pub kind: OverwriteKind,
    pub allow: Permissions,
    pub deny: Permissions,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LiveTag {
    pub id: Snowflake,
    pub name: String,
    #[serde(default)]
    pub emoji_name: Option<String>,
    #[serde(default)]
    pub moderated: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LiveReaction {
    #[serde(default)]
    pub emoji_name: Option<String>,
}

/// The REQUIRE_TAG channel flag.
pub const REQUIRE_TAG: u64 = 1 << 4;

#[derive(Debug, Clone, Deserialize)]
pub struct LiveChannel {
    pub id: Snowflake,
    #[serde(rename = "type")]
    pub kind: LiveKind,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<Snowflake>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub position: i32,
    #[serde(default)]
    pub rate_limit_per_user: u32,
    #[serde(default)]
    pub permission_overwrites: Vec<Overwrite>,
    #[serde(default)]
    pub available_tags: Vec<LiveTag>,
    #[serde(default)]
    pub default_sort_order: Option<u8>,
    #[serde(default)]
    pub default_forum_layout: Option<u8>,
    #[serde(default)]
    pub default_reaction_emoji: Option<LiveReaction>,
    #[serde(default)]
    pub flags: u64,
}
