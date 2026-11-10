// Field set adapted from Kira's LAYOUT (github.com/horizon-rust/kira, src/config.js), MIT © 2026 Ludovic Marie and the Kira contributors; see LICENSES/kira-MIT.txt
use std::collections::BTreeMap;

use serde::Deserialize;

use super::{Color, Key};
use crate::{ids::Snowflake, kit::perms::Permission};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub version: u32,
    pub guild: GuildSpec,
    pub roles: Vec<RoleSpec>,
    /// Highest first.
    pub role_order: Vec<Key>,
    #[serde(default)]
    pub team_members: BTreeMap<String, Snowflake>,
    pub categories: Vec<CategorySpec>,
    #[serde(default)]
    pub messages: BTreeMap<Key, MessageSpec>,
}

impl ServerConfig {
    pub fn channels(&self) -> impl Iterator<Item = (&CategorySpec, &ChannelSpec)> {
        self.categories
            .iter()
            .flat_map(|c| c.channels.iter().map(move |ch| (c, ch)))
    }

    pub fn channel(&self, key: &str) -> Option<&ChannelSpec> {
        self.channels()
            .map(|(_, c)| c)
            .find(|c| c.key.as_str() == key)
    }

    pub fn role(&self, key: &str) -> Option<&RoleSpec> {
        self.roles.iter().find(|r| r.key.as_str() == key)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuildSpec {
    pub name: String,
    pub verification_level: VerificationLevel,
    pub explicit_content_filter: ContentFilter,
    pub default_notifications: Notifications,
    pub community: CommunitySpec,
    /// Absent: @everyone is left as it is.
    #[serde(default)]
    pub everyone_permissions: Option<Vec<Permission>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationLevel {
    None,
    Low,
    Medium,
    High,
    VeryHigh,
}

impl VerificationLevel {
    pub fn api(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentFilter {
    Disabled,
    MembersWithoutRoles,
    AllMembers,
}

impl ContentFilter {
    pub fn api(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Notifications {
    AllMessages,
    OnlyMentions,
}

impl Notifications {
    pub fn api(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommunitySpec {
    pub rules_channel: Key,
    pub updates_channel: Key,
    #[serde(default)]
    pub enable: CommunityEnable,
}

/// Who switches Community on: the bot (it needs the permission) or a person in Server Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunityEnable {
    #[default]
    Bot,
    Manual,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleSpec {
    pub key: Key,
    pub name: String,
    #[serde(default)]
    pub manage: RoleManage,
    /// Pinned id, for a role that exists already.
    #[serde(default)]
    pub id: Option<Snowflake>,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub hoist: bool,
    #[serde(default)]
    pub mentionable: bool,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// Members may toggle it with a button.
    #[serde(default)]
    pub self_assign: bool,
    #[serde(default)]
    pub description: Option<String>,
}

/// `AdoptOnly`: found by name and remembered, never edited (the bot cannot, or must not).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleManage {
    #[default]
    Full,
    AdoptOnly,
}

/// Who sees and writes in a channel; the presets of SPEC §3.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Public,
    Readonly,
    BotFeed,
    ContributorsWrite,
    TeamPrivate,
    /// Voice: everyone sees it, only Contributor and Sunrise Team join.
    ContributorsConnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    Team,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategorySpec {
    pub key: Key,
    pub name: String,
    #[serde(default)]
    pub aka: Vec<String>,
    #[serde(default)]
    pub id: Option<Snowflake>,
    pub access: Access,
    pub channels: Vec<ChannelSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Text,
    Announcement,
    Forum,
    Voice,
}

impl ChannelKind {
    /// Forums and announcement channels exist only with Community on.
    pub fn needs_community(self) -> bool {
        matches!(self, ChannelKind::Forum | ChannelKind::Announcement)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForumLayout {
    List,
    Gallery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForumSort {
    LatestActivity,
    CreationDate,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelSpec {
    pub key: Key,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ChannelKind,
    #[serde(default)]
    pub aka: Vec<String>,
    #[serde(default)]
    pub id: Option<Snowflake>,
    #[serde(default)]
    pub topic: Option<String>,
    /// Absent: the category's.
    #[serde(default)]
    pub access: Option<Access>,
    #[serde(default)]
    pub visible_to: Option<Audience>,
    #[serde(default)]
    pub slowmode_seconds: u32,
    /// Keys of the bot-owned messages that live here.
    #[serde(default)]
    pub messages: Vec<Key>,
    // Forum only from here.
    #[serde(default)]
    pub require_tag: bool,
    #[serde(default)]
    pub layout: Option<ForumLayout>,
    #[serde(default)]
    pub sort: Option<ForumSort>,
    #[serde(default)]
    pub default_reaction: Option<String>,
    #[serde(default)]
    pub tags: Vec<TagSpec>,
    #[serde(default)]
    pub done_tag: Option<String>,
    #[serde(default)]
    pub template: Option<TemplateSpec>,
}

impl ChannelSpec {
    pub fn tag(&self, name: &str) -> Option<&TagSpec> {
        self.tags.iter().find(|t| t.name == name)
    }

    /// The forum-only fields that are set, for validation.
    pub fn forum_fields(&self) -> Vec<&'static str> {
        let set = [
            (self.require_tag, "require_tag"),
            (self.layout.is_some(), "layout"),
            (self.sort.is_some(), "sort"),
            (self.default_reaction.is_some(), "default_reaction"),
            (!self.tags.is_empty(), "tags"),
            (self.done_tag.is_some(), "done_tag"),
            (self.template.is_some(), "template"),
        ];
        set.into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, n)| n)
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagSpec {
    pub emoji: String,
    pub name: String,
    /// Only the team may apply it.
    #[serde(default)]
    pub team_only: bool,
}

/// What a post in a forum must contain (Kira's forum `check`); the checker comes in a later step.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateSpec {
    pub text: String,
    pub min_chars: usize,
    #[serde(default)]
    pub needs: Vec<Need>,
    #[serde(default)]
    pub needs_by_tag: BTreeMap<String, Vec<Need>>,
    pub grace_minutes: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Need {
    pub label: String,
    pub regex: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MessageSpec {
    /// One message with an embed (and buttons for the roles listed).
    Embed {
        embed: EmbedSpec,
        #[serde(default)]
        role_buttons: Vec<Key>,
    },
    /// The pinned guide post of a forum.
    ForumPost {
        title: String,
        /// The forum tag the post carries (a forum that requires one needs it).
        #[serde(default)]
        tag: Option<String>,
        pinned: bool,
        locked: bool,
        body_from: BodyFrom,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyFrom {
    Template,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbedSpec {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub color: Option<Color>,
}
