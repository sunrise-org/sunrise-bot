//! What an `Action` becomes on the wire: typed bodies in Discord's field names, built with the ids known so far.
use std::collections::BTreeMap;

use serde::Serialize;

use super::{
    access::{self, Line},
    live::{Live, LiveKind, OverwriteKind, PINNED, REQUIRE_TAG},
    message::{self, MessageBody},
    perms::Permissions,
    plan::{Action, ChannelEdit, Known, ObjectKind, RoleEdit},
};
use crate::{config::*, ids::Snowflake};

/// The ids of what exists, by kind and config key: recorded, adopted and made in this run.
#[derive(Debug, Clone, Default)]
pub struct Ids(BTreeMap<(ObjectKind, Key), Snowflake>);

impl Ids {
    /// The recorded ids that still exist on the server.
    pub fn from_known(known: &Known, live: &Live) -> Ids {
        let mut ids = Ids::default();
        for (kind, key, obj) in known.iter() {
            let exists = match kind {
                ObjectKind::Role => live.role(obj.id).is_some(),
                ObjectKind::Category | ObjectKind::Channel => live.channel(obj.id).is_some(),
                ObjectKind::Message => live.messages.values().flatten().any(|m| m.id == obj.id),
                ObjectKind::ForumPost => live.thread(obj.id).is_some(),
            };
            if exists {
                ids.set(kind, key.clone(), obj.id);
            }
        }
        ids
    }

    pub fn set(&mut self, kind: ObjectKind, key: Key, id: Snowflake) {
        self.0.insert((kind, key), id);
    }

    pub fn get(&self, kind: ObjectKind, key: &Key) -> Option<Snowflake> {
        self.0.get(&(kind, key.clone())).copied()
    }
}

pub struct Ctx<'a> {
    pub config: &'a ServerConfig,
    pub ids: &'a Ids,
    pub live: &'a Live,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("{0} has no id yet (it was not created)")]
    Unresolved(String),
    #[error("the file has no {0}")]
    NotInFile(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    PatchGuild(GuildPatch),
    CreateRole(RoleBody),
    PatchRole {
        id: Snowflake,
        body: RolePatch,
    },
    SetRolePositions(Vec<RolePosition>),
    CreateChannel(ChannelBody),
    PatchChannel {
        id: Snowflake,
        body: ChannelPatch,
    },
    SetChannelPositions(Vec<ChannelPosition>),
    DeleteChannel {
        id: Snowflake,
    },
    CreateMessage {
        channel: Snowflake,
        body: MessageBody,
    },
    EditMessage {
        channel: Snowflake,
        message: Snowflake,
        body: MessageBody,
    },
    CreateThread {
        forum: Snowflake,
        body: ThreadBody,
    },
}

/// A forum post: its name, the tags it carries, and its first message.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThreadBody {
    pub name: String,
    pub applied_tags: Vec<Snowflake>,
    pub message: MessageBody,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct GuildPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explicit_content_filter: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_message_notifications: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules_channel_id: Option<Snowflake>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_updates_channel_id: Option<Snowflake>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_locale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoleBody {
    pub name: String,
    pub color: u32,
    pub hoist: bool,
    pub mentionable: bool,
    pub permissions: Permissions,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RolePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoist: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mentionable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RolePosition {
    pub id: Snowflake,
    pub position: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelPosition {
    pub id: Snowflake,
    pub position: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OverwriteBody {
    pub id: Snowflake,
    #[serde(rename = "type")]
    pub kind: u8,
    pub allow: Permissions,
    pub deny: Permissions,
}

impl From<&Line> for OverwriteBody {
    fn from(l: &Line) -> Self {
        OverwriteBody {
            id: l.id,
            kind: if l.kind == OverwriteKind::Role { 0 } else { 1 },
            allow: l.allow,
            deny: l.deny,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TagBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Snowflake>,
    pub name: String,
    pub emoji_name: String,
    pub moderated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReactionBody {
    pub emoji_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ChannelBody {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Snowflake>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_per_user: Option<u32>,
    pub permission_overwrites: Vec<OverwriteBody>,
    pub position: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_tags: Option<Vec<TagBody>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_sort_order: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_forum_layout: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_reaction_emoji: Option<ReactionBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ChannelPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Snowflake>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_per_user: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_overwrites: Option<Vec<OverwriteBody>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_tags: Option<Vec<TagBody>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_sort_order: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_forum_layout: Option<u8>,
    /// `Some(None)` clears it (a JSON null).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_reaction_emoji: Option<Option<ReactionBody>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<u64>,
    /// Forum posts only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
}

fn kind_number(kind: ChannelKind) -> u8 {
    LiveKind::of(kind).number()
}

fn tags(spec: &ChannelSpec) -> Vec<TagBody> {
    spec.tags
        .iter()
        .map(|t| TagBody {
            id: None,
            name: t.name.clone(),
            emoji_name: t.emoji.clone(),
            moderated: t.team_only,
        })
        .collect()
}

fn sort_number(sort: ForumSort) -> u8 {
    u8::from(sort == ForumSort::CreationDate)
}

fn layout_number(layout: ForumLayout) -> u8 {
    match layout {
        ForumLayout::List => 1,
        ForumLayout::Gallery => 2,
    }
}

impl Ctx<'_> {
    fn id(&self, kind: ObjectKind, key: &Key) -> Result<Snowflake, BuildError> {
        self.ids
            .get(kind, key)
            .ok_or_else(|| BuildError::Unresolved(format!("{kind} {key}")))
    }

    fn overwrites(&self, want: &[access::WantOverwrite]) -> Result<Vec<OverwriteBody>, BuildError> {
        for w in want {
            if let access::Principal::Role(k) = &w.who {
                self.id(ObjectKind::Role, k)?;
            }
        }
        let lines = access::resolve(want, self.live.guild.id, self.live.bot_id, |k| {
            self.ids.get(ObjectKind::Role, k)
        })
        .expect("every role was checked");
        Ok(lines.iter().map(OverwriteBody::from).collect())
    }

    fn channel_edit(&self, e: &ChannelEdit) -> Result<ChannelPatch, BuildError> {
        Ok(ChannelPatch {
            name: e.name.as_ref().map(|c| c.to.clone()),
            kind: e.kind.as_ref().map(|c| kind_number(c.to)),
            parent_id: e
                .parent
                .as_ref()
                .map(|c| self.id(ObjectKind::Category, &c.to))
                .transpose()?,
            topic: e.topic.as_ref().map(|c| c.to.clone()),
            rate_limit_per_user: e.slowmode.as_ref().map(|c| c.to),
            permission_overwrites: e
                .overwrites
                .as_ref()
                .map(|o| self.overwrites(&o.want))
                .transpose()?,
            available_tags: e.tags.as_ref().map(|t| {
                t.iter()
                    .map(|t| TagBody {
                        id: t.id,
                        name: t.name.clone(),
                        emoji_name: t.emoji.clone(),
                        moderated: t.moderated,
                    })
                    .collect()
            }),
            default_sort_order: e.sort.as_ref().map(|c| sort_number(c.to)),
            default_forum_layout: e.layout.as_ref().map(|c| layout_number(c.to)),
            default_reaction_emoji: e.default_reaction.as_ref().map(|c| {
                (!c.to.is_empty()).then(|| ReactionBody {
                    emoji_name: c.to.clone(),
                })
            }),
            flags: e
                .require_tag
                .as_ref()
                .map(|c| if c.to { REQUIRE_TAG } else { 0 }),
            archived: None,
            locked: None,
        })
    }

    fn role_edit(e: &RoleEdit) -> RolePatch {
        RolePatch {
            name: e.name.as_ref().map(|c| c.to.clone()),
            color: e.color.as_ref().map(|c| c.to.0),
            hoist: e.hoist.as_ref().map(|c| c.to),
            mentionable: e.mentionable.as_ref().map(|c| c.to),
            permissions: e.permissions.as_ref().map(|c| c.to),
        }
    }

    fn channel_body(
        &self,
        spec: &ChannelSpec,
        category: &CategorySpec,
        position: u32,
    ) -> Result<ChannelBody, BuildError> {
        let access = spec.access.unwrap_or(category.access);
        let mut body = ChannelBody {
            name: spec.name.clone(),
            kind: kind_number(spec.kind),
            parent_id: Some(self.id(ObjectKind::Category, &category.key)?),
            permission_overwrites: self.overwrites(&access::wanted(access, spec.visible_to))?,
            position,
            ..ChannelBody::default()
        };
        if spec.kind != ChannelKind::Voice {
            body.topic = spec.topic.clone();
            body.rate_limit_per_user = Some(spec.slowmode_seconds);
        }
        if spec.kind == ChannelKind::Forum {
            body.available_tags = Some(tags(spec));
            body.default_sort_order = spec.sort.map(sort_number);
            body.default_forum_layout = spec.layout.map(layout_number);
            body.default_reaction_emoji = spec
                .default_reaction
                .clone()
                .map(|emoji_name| ReactionBody { emoji_name });
            body.flags = spec.require_tag.then_some(REQUIRE_TAG);
        }
        Ok(body)
    }
}

fn build_one(action: &Action, ctx: &Ctx) -> Result<Request, BuildError> {
    let config = ctx.config;
    Ok(match action {
        Action::EditGuild(e) => Request::PatchGuild(GuildPatch {
            name: e.name.as_ref().map(|c| c.to.clone()),
            verification_level: e.verification_level.as_ref().map(|c| c.to.api()),
            explicit_content_filter: e.explicit_content_filter.as_ref().map(|c| c.to.api()),
            default_message_notifications: e.default_notifications.as_ref().map(|c| c.to.api()),
            rules_channel_id: e
                .rules_channel
                .as_ref()
                .map(|c| ctx.id(ObjectKind::Channel, &c.to))
                .transpose()?,
            public_updates_channel_id: e
                .updates_channel
                .as_ref()
                .map(|c| ctx.id(ObjectKind::Channel, &c.to))
                .transpose()?,
            ..GuildPatch::default()
        }),
        Action::SetEveryonePermissions { to, .. } => Request::PatchRole {
            id: ctx.live.guild.id,
            body: RolePatch {
                permissions: Some(*to),
                ..RolePatch::default()
            },
        },
        Action::EnableCommunity => {
            let g = &config.guild;
            let mut features = ctx.live.guild.features.clone();
            if !features.iter().any(|f| f == "COMMUNITY") {
                features.push("COMMUNITY".to_string());
            }
            Request::PatchGuild(GuildPatch {
                verification_level: Some(g.verification_level.api()),
                explicit_content_filter: Some(g.explicit_content_filter.api()),
                default_message_notifications: Some(g.default_notifications.api()),
                features: Some(features),
                rules_channel_id: Some(ctx.id(ObjectKind::Channel, &g.community.rules_channel)?),
                public_updates_channel_id: Some(
                    ctx.id(ObjectKind::Channel, &g.community.updates_channel)?,
                ),
                preferred_locale: Some("en-US".to_string()),
                ..GuildPatch::default()
            })
        }
        Action::CreateRole { key } => {
            let r = config
                .role(key.as_str())
                .ok_or_else(|| BuildError::NotInFile(format!("role {key}")))?;
            Request::CreateRole(RoleBody {
                name: r.name.clone(),
                color: r.color.map_or(0, |c| c.0),
                hoist: r.hoist,
                mentionable: r.mentionable,
                permissions: Permissions::of(&r.permissions),
            })
        }
        Action::EditRole { id, edit, .. } => Request::PatchRole {
            id: *id,
            body: Ctx::role_edit(edit),
        },
        Action::ReorderRoles { order } => {
            let n = order.len() as u32;
            let mut positions = Vec::new();
            for (i, k) in order.iter().enumerate() {
                positions.push(RolePosition {
                    id: ctx.id(ObjectKind::Role, k)?,
                    position: n - i as u32,
                });
            }
            Request::SetRolePositions(positions)
        }
        Action::CreateCategory { key } => {
            let (i, cat) = config
                .categories
                .iter()
                .enumerate()
                .find(|(_, c)| &c.key == key)
                .ok_or_else(|| BuildError::NotInFile(format!("category {key}")))?;
            Request::CreateChannel(ChannelBody {
                name: cat.name.clone(),
                kind: LiveKind::Category.number(),
                permission_overwrites: ctx.overwrites(&access::wanted(cat.access, None))?,
                position: i as u32,
                ..ChannelBody::default()
            })
        }
        Action::EditCategory { id, edit, .. } | Action::EditChannel { id, edit, .. } => {
            Request::PatchChannel {
                id: *id,
                body: ctx.channel_edit(edit)?,
            }
        }
        Action::CreateChannel { key, category } => {
            let cat = config
                .categories
                .iter()
                .find(|c| &c.key == category)
                .ok_or_else(|| BuildError::NotInFile(format!("category {category}")))?;
            let (i, spec) = cat
                .channels
                .iter()
                .enumerate()
                .find(|(_, c)| &c.key == key)
                .ok_or_else(|| BuildError::NotInFile(format!("channel {key}")))?;
            Request::CreateChannel(ctx.channel_body(spec, cat, i as u32)?)
        }
        Action::ReorderCategories { order } => {
            let mut positions = Vec::new();
            for (i, k) in order.iter().enumerate() {
                positions.push(ChannelPosition {
                    id: ctx.id(ObjectKind::Category, k)?,
                    position: i as u32,
                });
            }
            Request::SetChannelPositions(positions)
        }
        Action::ReorderChannels { order, .. } => {
            let mut positions = Vec::new();
            for (i, k) in order.iter().enumerate() {
                positions.push(ChannelPosition {
                    id: ctx.id(ObjectKind::Channel, k)?,
                    position: i as u32,
                });
            }
            Request::SetChannelPositions(positions)
        }
        Action::DeleteChannel { id, .. } => Request::DeleteChannel { id: *id },
        _ => unreachable!("posts are built by `build_posts`"),
    })
}

/// The requests for one action, in the order to send them. The first one is the one that creates, if any does.
pub fn build(action: &Action, ctx: &Ctx) -> Result<Vec<Request>, BuildError> {
    let channel_id = |k: &str| {
        Key::new(k)
            .ok()
            .and_then(|k| ctx.ids.get(ObjectKind::Channel, &k))
    };
    let embed = |key: &Key| {
        message::embed_message(ctx.config, key, &channel_id)
            .ok_or_else(|| BuildError::NotInFile(format!("message {key}")))
    };
    let forum = |key: &Key| {
        let spec = ctx
            .config
            .channel(key.as_str())
            .ok_or_else(|| BuildError::NotInFile(format!("channel {key}")))?;
        let body = message::how_to_post(spec)
            .ok_or_else(|| BuildError::NotInFile(format!("template of {key}")))?;
        Ok::<_, BuildError>((spec, body, ctx.id(ObjectKind::Channel, key)?))
    };
    Ok(match action {
        Action::CreateMessage { key, channel } => vec![Request::CreateMessage {
            channel: ctx.id(ObjectKind::Channel, channel)?,
            body: embed(key)?,
        }],
        Action::EditMessage { key, channel, id } => vec![Request::EditMessage {
            channel: ctx.id(ObjectKind::Channel, channel)?,
            message: *id,
            body: embed(key)?,
        }],
        Action::CreateForumPost { key, forum: f } => {
            let (spec, body, forum_id) = forum(f)?;
            let Some(MessageSpec::ForumPost { title, tag, .. }) = ctx.config.messages.get(key)
            else {
                return Err(BuildError::NotInFile(format!("forum post {key}")));
            };
            let live_forum = ctx.live.channel(forum_id);
            let applied_tags = match tag {
                None => Vec::new(),
                Some(name) => vec![
                    live_forum
                        .and_then(|c| c.available_tags.iter().find(|t| &t.name == name))
                        .map(|t| t.id)
                        .ok_or_else(|| {
                            BuildError::Unresolved(format!("tag {name} of #{}", spec.name))
                        })?,
                ],
            };
            vec![Request::CreateThread {
                forum: forum_id,
                body: ThreadBody {
                    name: title.clone(),
                    applied_tags,
                    message: body,
                },
            }]
        }
        Action::EditForumPost {
            key,
            forum: f,
            id,
            edit,
        } => {
            let (_, body, _) = forum(f)?;
            let Some(MessageSpec::ForumPost {
                title,
                pinned,
                locked,
                ..
            }) = ctx.config.messages.get(key)
            else {
                return Err(BuildError::NotInFile(format!("forum post {key}")));
            };
            let mut out = Vec::new();
            if edit.unarchive {
                out.push(Request::PatchChannel {
                    id: *id,
                    body: ChannelPatch {
                        archived: Some(false),
                        ..ChannelPatch::default()
                    },
                });
            }
            if edit.name.is_some() || edit.locked.is_some() || edit.pinned.is_some() {
                out.push(Request::PatchChannel {
                    id: *id,
                    body: ChannelPatch {
                        name: edit.name.as_ref().map(|_| title.clone()),
                        locked: edit.locked.as_ref().map(|_| *locked),
                        flags: edit
                            .pinned
                            .as_ref()
                            .map(|_| if *pinned { PINNED } else { 0 }),
                        ..ChannelPatch::default()
                    },
                });
            }
            if edit.starter {
                out.push(Request::EditMessage {
                    channel: *id,
                    message: *id,
                    body,
                });
            }
            out
        }
        other => vec![build_one(other, ctx)?],
    })
}
