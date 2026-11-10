//! An in-memory guild that answers the requests the way Discord does (as far as bootstrap can tell): for tests.
use std::sync::Mutex;

use super::{Api, ApiError, Response};
use crate::{
    ids::Snowflake,
    kit::{
        live::*,
        message::MessageBody,
        request::{ChannelBody, ChannelPatch, OverwriteBody, Request, TagBody},
    },
};

pub struct FakeDiscord {
    state: Mutex<State>,
}

struct State {
    live: Live,
    next_id: u64,
    writes: Vec<Request>,
    /// May the bot switch Community on? (The real server said 403.)
    community_allowed: bool,
    /// Every write from this one on fails with a 500.
    fail_from_write: Option<usize>,
}

fn status(code: u16, body: &str) -> ApiError {
    ApiError::Status {
        status: code,
        body: body.to_string(),
    }
}

fn overwrite(o: &OverwriteBody) -> Overwrite {
    Overwrite {
        id: o.id,
        kind: if o.kind == 0 {
            OverwriteKind::Role
        } else {
            OverwriteKind::Member
        },
        allow: o.allow,
        deny: o.deny,
    }
}

impl FakeDiscord {
    pub fn new(live: Live) -> FakeDiscord {
        let newest = live
            .channels
            .iter()
            .map(|c| c.id.0)
            .chain(live.roles.iter().map(|r| r.id.0))
            .max()
            .unwrap_or(0);
        FakeDiscord {
            state: Mutex::new(State {
                live,
                next_id: newest.max(9_000_000),
                writes: Vec::new(),
                community_allowed: true,
                fail_from_write: None,
            }),
        }
    }

    pub fn community_allowed(self, allowed: bool) -> FakeDiscord {
        self.state.lock().unwrap().community_allowed = allowed;
        self
    }

    pub fn fail_from_write(self, n: usize) -> FakeDiscord {
        self.state.lock().unwrap().fail_from_write = Some(n);
        self
    }

    /// The requests that changed something, in order.
    pub fn writes(&self) -> Vec<Request> {
        self.state.lock().unwrap().writes.clone()
    }

    pub fn live(&self) -> Live {
        self.state.lock().unwrap().live.clone()
    }

    /// Somebody switches Community on in Server Settings.
    pub fn enable_community_by_hand(&self) {
        self.state
            .lock()
            .unwrap()
            .live
            .guild
            .features
            .push("COMMUNITY".to_string());
    }
}

impl State {
    fn id(&mut self) -> Snowflake {
        self.next_id += 1;
        Snowflake(self.next_id)
    }

    fn tags(&mut self, tags: Vec<TagBody>) -> Vec<LiveTag> {
        tags.into_iter()
            .map(|t| LiveTag {
                id: t.id.unwrap_or_else(|| self.id()),
                name: t.name,
                emoji_name: Some(t.emoji_name),
                moderated: t.moderated,
            })
            .collect()
    }

    fn community(&self) -> bool {
        self.live.community()
    }

    /// A message of the bot's, as Discord would return it.
    fn message(&self, id: Snowflake, body: &MessageBody) -> LiveMessage {
        LiveMessage {
            id,
            author: Author {
                id: self.live.bot_id,
            },
            content: String::new(),
            embeds: body
                .embeds
                .iter()
                .map(|e| LiveEmbed {
                    kind: Some("rich".to_string()),
                    title: Some(e.title.clone()),
                    description: Some(e.description.clone()),
                    color: Some(e.color),
                })
                .collect(),
            components: body
                .components
                .iter()
                .map(|r| LiveRow {
                    components: r
                        .components
                        .iter()
                        .map(|b| LiveComponent {
                            kind: b.kind,
                            custom_id: Some(b.custom_id.clone()),
                            label: Some(b.label.clone()),
                        })
                        .collect(),
                })
                .collect(),
            pinned: false,
        }
    }

    fn create_channel(&mut self, b: ChannelBody) -> Result<LiveChannel, ApiError> {
        let kind = match b.kind {
            0 => LiveKind::Text,
            2 => LiveKind::Voice,
            4 => LiveKind::Category,
            5 => LiveKind::Announcement,
            15 => LiveKind::Forum,
            n => LiveKind::Other(n),
        };
        if matches!(kind, LiveKind::Forum | LiveKind::Announcement) && !self.community() {
            return Err(status(
                400,
                "This guild does not have the community feature enabled",
            ));
        }
        if let Some(p) = b.parent_id
            && self
                .live
                .channel(p)
                .is_none_or(|c| c.kind != LiveKind::Category)
        {
            return Err(status(400, "Invalid parent"));
        }
        if self
            .live
            .channels
            .iter()
            .any(|c| c.kind == kind && c.name == b.name && c.parent_id == b.parent_id)
            && kind != LiveKind::Category
        {
            // Discord allows twins; the fake refuses so a test notices a double create
            return Err(status(400, "a channel with that name exists (fake)"));
        }
        let id = self.id();
        let tags = self.tags(b.available_tags.unwrap_or_default());
        let channel = LiveChannel {
            id,
            kind,
            name: b.name,
            parent_id: b.parent_id,
            topic: b.topic,
            position: b.position as i32,
            rate_limit_per_user: b.rate_limit_per_user.unwrap_or(0),
            permission_overwrites: b.permission_overwrites.iter().map(overwrite).collect(),
            available_tags: tags,
            default_sort_order: b.default_sort_order,
            default_forum_layout: b.default_forum_layout,
            default_reaction_emoji: b.default_reaction_emoji.map(|r| LiveReaction {
                emoji_name: Some(r.emoji_name),
            }),
            flags: b.flags.unwrap_or(0),
        };
        self.live.channels.push(channel.clone());
        Ok(channel)
    }

    fn patch_channel(&mut self, id: Snowflake, p: ChannelPatch) -> Result<(), ApiError> {
        if let Some(t) = self.live.threads.iter_mut().find(|t| t.id == id) {
            if let Some(n) = p.name {
                t.name = n;
            }
            if let Some(a) = p.archived {
                t.thread_metadata.archived = a;
            }
            if let Some(l) = p.locked {
                t.thread_metadata.locked = l;
            }
            if let Some(f) = p.flags {
                t.flags = f;
            }
            return Ok(());
        }
        let community = self.community();
        if p.kind == Some(5) && !community {
            return Err(status(
                400,
                "This guild does not have the community feature enabled",
            ));
        }
        let tags = p.available_tags.map(|t| self.tags(t));
        let c = self
            .live
            .channels
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| status(404, "Unknown Channel"))?;
        if let Some(n) = p.name {
            c.name = n;
        }
        if let Some(k) = p.kind {
            c.kind = if k == 5 {
                LiveKind::Announcement
            } else {
                LiveKind::Text
            };
        }
        if let Some(parent) = p.parent_id {
            c.parent_id = Some(parent);
        }
        if let Some(t) = p.topic {
            c.topic = Some(t);
        }
        if let Some(r) = p.rate_limit_per_user {
            c.rate_limit_per_user = r;
        }
        if let Some(o) = p.permission_overwrites {
            c.permission_overwrites = o.iter().map(overwrite).collect();
        }
        if let Some(t) = tags {
            c.available_tags = t;
        }
        if let Some(s) = p.default_sort_order {
            c.default_sort_order = Some(s);
        }
        if let Some(l) = p.default_forum_layout {
            c.default_forum_layout = Some(l);
        }
        if let Some(r) = p.default_reaction_emoji {
            c.default_reaction_emoji = r.map(|r| LiveReaction {
                emoji_name: Some(r.emoji_name),
            });
        }
        if let Some(f) = p.flags {
            c.flags = f;
        }
        Ok(())
    }
}

impl Api for FakeDiscord {
    async fn snapshot(&self) -> Result<Live, ApiError> {
        let mut live = self.live();
        live.messages.clear();
        live.threads.clear();
        Ok(live)
    }

    async fn messages(&self, channel: Snowflake) -> Result<Vec<LiveMessage>, ApiError> {
        let s = self.state.lock().unwrap();
        Ok(s.live.messages.get(&channel).cloned().unwrap_or_default())
    }

    async fn forum_threads(&self, forum: Snowflake) -> Result<Vec<LiveThread>, ApiError> {
        let s = self.state.lock().unwrap();
        Ok(s.live
            .threads
            .iter()
            .filter(|t| t.parent_id == Some(forum))
            .cloned()
            .collect())
    }

    async fn send(&self, request: Request) -> Result<Response, ApiError> {
        let mut s = self.state.lock().unwrap();
        if s.fail_from_write.is_some_and(|n| s.writes.len() >= n) {
            return Err(status(500, "the fake guild is down"));
        }
        s.writes.push(request.clone());
        match request {
            Request::PatchGuild(p) => {
                if let Some(features) = p.features {
                    if features.iter().any(|f| f == "COMMUNITY") && !s.community() {
                        if !s.community_allowed {
                            return Err(ApiError::Forbidden(
                                "{\"message\": \"Missing Access\", \"code\": 50001}".to_string(),
                            ));
                        }
                        if p.rules_channel_id.is_none() || p.public_updates_channel_id.is_none() {
                            return Err(status(
                                400,
                                "Community needs a rules channel and an updates channel",
                            ));
                        }
                    }
                    s.live.guild.features = features;
                }
                let g = &mut s.live.guild;
                if let Some(n) = p.name {
                    g.name = n;
                }
                if let Some(v) = p.verification_level {
                    g.verification_level = v;
                }
                if let Some(v) = p.explicit_content_filter {
                    g.explicit_content_filter = v;
                }
                if let Some(v) = p.default_message_notifications {
                    g.default_message_notifications = v;
                }
                if let Some(v) = p.rules_channel_id {
                    g.rules_channel_id = Some(v);
                }
                if let Some(v) = p.public_updates_channel_id {
                    g.public_updates_channel_id = Some(v);
                }
                Ok(Response::Done)
            }
            Request::CreateRole(b) => {
                let id = s.id();
                let role = LiveRole {
                    id,
                    name: b.name,
                    color: b.color,
                    hoist: b.hoist,
                    mentionable: b.mentionable,
                    permissions: b.permissions,
                    position: 1,
                    managed: false,
                };
                s.live.roles.push(role.clone());
                Ok(Response::Role(role))
            }
            Request::PatchRole { id, body } => {
                let r = s
                    .live
                    .roles
                    .iter_mut()
                    .find(|r| r.id == id)
                    .ok_or_else(|| status(404, "Unknown Role"))?;
                if let Some(n) = body.name {
                    r.name = n;
                }
                if let Some(c) = body.color {
                    r.color = c;
                }
                if let Some(h) = body.hoist {
                    r.hoist = h;
                }
                if let Some(m) = body.mentionable {
                    r.mentionable = m;
                }
                if let Some(p) = body.permissions {
                    r.permissions = p;
                }
                Ok(Response::Done)
            }
            Request::SetRolePositions(list) => {
                for p in list {
                    if let Some(r) = s.live.roles.iter_mut().find(|r| r.id == p.id) {
                        r.position = p.position as i32;
                    }
                }
                Ok(Response::Done)
            }
            Request::CreateChannel(b) => s.create_channel(b).map(Response::Channel),
            Request::PatchChannel { id, body } => {
                s.patch_channel(id, body).map(|()| Response::Done)
            }
            Request::SetChannelPositions(list) => {
                for p in list {
                    if let Some(c) = s.live.channels.iter_mut().find(|c| c.id == p.id) {
                        c.position = p.position as i32;
                    }
                }
                Ok(Response::Done)
            }
            Request::DeleteChannel { id } => {
                s.live.channels.retain(|c| c.id != id);
                Ok(Response::Done)
            }
            Request::CreateMessage { channel, body } => {
                if s.live.channel(channel).is_none() {
                    return Err(status(404, "Unknown Channel"));
                }
                let id = s.id();
                let message = s.message(id, &body);
                s.live
                    .messages
                    .entry(channel)
                    .or_default()
                    .insert(0, message.clone());
                Ok(Response::Message(message))
            }
            Request::EditMessage {
                channel,
                message,
                body,
            } => {
                let new = s.message(message, &body);
                let m = s
                    .live
                    .messages
                    .get_mut(&channel)
                    .and_then(|l| l.iter_mut().find(|m| m.id == message))
                    .ok_or_else(|| status(404, "Unknown Message"))?;
                m.embeds = new.embeds;
                m.components = new.components;
                Ok(Response::Done)
            }
            Request::CreateThread { forum, body } => {
                let Some(f) = s.live.channel(forum).cloned() else {
                    return Err(status(404, "Unknown Channel"));
                };
                if f.kind != LiveKind::Forum {
                    return Err(status(400, "Not a forum channel"));
                }
                if f.flags & REQUIRE_TAG != 0 && body.applied_tags.is_empty() {
                    return Err(status(400, "A tag is required"));
                }
                let id = s.id();
                let thread = LiveThread {
                    id,
                    name: body.name,
                    parent_id: Some(forum),
                    owner_id: Some(s.live.bot_id),
                    flags: 0,
                    thread_metadata: ThreadMetadata::default(),
                };
                let starter = s.message(id, &body.message);
                s.live.messages.insert(id, vec![starter]);
                s.live.threads.push(thread.clone());
                Ok(Response::Thread(thread))
            }
        }
    }
}
