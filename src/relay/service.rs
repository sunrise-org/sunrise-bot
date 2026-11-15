use std::{net::SocketAddr, sync::Arc, time::Duration};

use poise::serenity_prelude as serenity;
use tokio::sync::{broadcast::error::RecvError, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use super::{
    Hub, admin, message, pb,
    pb::relay_server::{Relay, RelayServer},
    user,
};

/// Cloudflare closes a stream that stays silent for 100 s.
const HEARTBEAT: Duration = Duration::from_secs(30);
const BACKLOG_PAGE: u32 = 200;
const MAX_CONTENT: usize = 2000;

pub struct RelayService {
    hub: Arc<Hub>,
    http: Arc<serenity::Http>,
    guild: serenity::GuildId,
}

impl RelayService {
    pub fn new(hub: Arc<Hub>, http: Arc<serenity::Http>, guild: serenity::GuildId) -> RelayService {
        RelayService { hub, http, guild }
    }
}

/// Serves the relay on `addr` until `shutdown` resolves; every call must carry the token.
pub async fn serve(
    service: RelayService,
    addr: SocketAddr,
    token: String,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    let expected = format!("Bearer {token}");
    let check = move |request: Request<()>| {
        let given = request
            .metadata()
            .get("authorization")
            .map(|v| v.as_bytes())
            .unwrap_or_default();
        if same(given, expected.as_bytes()) {
            Ok(request)
        } else {
            Err(Status::unauthenticated("bad or missing relay token"))
        }
    };
    tracing::info!(%addr, "relay listening");
    tonic::transport::Server::builder()
        .http2_keepalive_interval(Some(HEARTBEAT))
        .add_service(RelayServer::with_interceptor(service, check))
        .serve_with_shutdown(addr, shutdown)
        .await?;
    Ok(())
}

/// Compares in time that does not depend on where the inputs differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn internal(e: impl std::fmt::Display) -> Status {
    tracing::warn!(error = %e, "relay call failed");
    Status::internal(e.to_string())
}

fn limit(n: u32) -> Result<u8, Status> {
    match n {
        0 => Ok(50),
        1..=100 => Ok(n as u8),
        _ => Err(Status::invalid_argument("limit is 1 to 100")),
    }
}

fn content(text: &str) -> Result<(), Status> {
    if text.trim().is_empty() || text.chars().count() > MAX_CONTENT {
        return Err(Status::invalid_argument(
            "content must be 1 to 2000 characters",
        ));
    }
    Ok(())
}

/// Users may be pinged, never everyone or a role.
fn mentions() -> serenity::CreateAllowedMentions {
    serenity::CreateAllowedMentions::new()
        .all_users(true)
        .everyone(false)
        .all_roles(false)
        .replied_user(true)
}

impl RelayService {
    /// The channel, if it is a channel of Sunny's guild.
    async fn guild_channel(&self, id: u64) -> Result<serenity::GuildChannel, Status> {
        let channel = serenity::ChannelId::new(id)
            .to_channel(&self.http)
            .await
            .map_err(|_| Status::not_found("no such channel"))?;
        match channel {
            serenity::Channel::Guild(c) if c.guild_id == self.guild => Ok(c),
            _ => Err(Status::permission_denied(
                "not a channel of the Sunrise guild",
            )),
        }
    }

    /// A category of Sunny's guild, for `parent_id`.
    async fn category(&self, id: u64) -> Result<serenity::ChannelId, Status> {
        let c = self.guild_channel(id).await?;
        if c.kind != serenity::ChannelType::Category {
            return Err(Status::invalid_argument("parent_id is not a category"));
        }
        Ok(c.id)
    }

    async fn dm_channel(&self, user_id: u64) -> Result<serenity::PrivateChannel, Status> {
        if user_id == 0 {
            return Err(Status::invalid_argument("user_id is required"));
        }
        serenity::UserId::new(user_id)
            .create_dm_channel(&self.http)
            .await
            .map_err(internal)
    }
}

#[tonic::async_trait]
impl Relay for RelayService {
    type SubscribeStream = ReceiverStream<Result<pb::SubscribeResponse, Status>>;

    async fn subscribe(
        &self,
        request: Request<pb::SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let mut last = request.into_inner().after_seq;
        let hub = self.hub.clone();
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let send = |item| {
                let tx = tx.clone();
                async move {
                    tx.send(Ok(pb::SubscribeResponse { item: Some(item) }))
                        .await
                        .is_ok()
                }
            };
            let mut live = hub.live.subscribe();
            let mut beat = tokio::time::interval(HEARTBEAT);
            'replay: loop {
                loop {
                    let page = match hub.after(last, BACKLOG_PAGE).await {
                        Ok(page) => page,
                        Err(e) => {
                            let _ = tx.send(Err(internal(e))).await;
                            return;
                        }
                    };
                    let done = page.len() < BACKLOG_PAGE as usize;
                    for event in page {
                        last = event.seq;
                        if !send(pb::subscribe_response::Item::Event(event)).await {
                            return;
                        }
                    }
                    if done {
                        break;
                    }
                }
                loop {
                    tokio::select! {
                        got = live.recv() => match got {
                            Ok(event) if event.seq > last => {
                                last = event.seq;
                                if !send(pb::subscribe_response::Item::Event(event)).await {
                                    return;
                                }
                            }
                            Ok(_) => {}
                            Err(RecvError::Lagged(_)) => continue 'replay,
                            Err(RecvError::Closed) => return,
                        },
                        _ = beat.tick() => {
                            if !send(pb::subscribe_response::Item::Heartbeat(pb::Heartbeat {})).await {
                                return;
                            }
                        }
                        () = tx.closed() => return,
                    }
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn send_message(
        &self,
        request: Request<pb::SendMessageRequest>,
    ) -> Result<Response<pb::SendMessageResponse>, Status> {
        let r = request.into_inner();
        content(&r.content)?;
        let channel = self.guild_channel(r.channel_id).await?;
        let mut builder = serenity::CreateMessage::new()
            .content(r.content)
            .allowed_mentions(mentions());
        if let Some(reply) = r.reply_to_id {
            builder = builder.reference_message(
                serenity::MessageReference::from((channel.id, serenity::MessageId::new(reply)))
                    .fail_if_not_exists(false),
            );
        }
        let sent = channel
            .id
            .send_message(&self.http, builder)
            .await
            .map_err(internal)?;
        Ok(Response::new(pb::SendMessageResponse {
            message: Some(message(&sent, channel.name)),
        }))
    }

    async fn send_dm(
        &self,
        request: Request<pb::SendDmRequest>,
    ) -> Result<Response<pb::SendDmResponse>, Status> {
        let r = request.into_inner();
        content(&r.content)?;
        let dm = self.dm_channel(r.user_id).await?;
        let sent = dm
            .id
            .send_message(
                &self.http,
                serenity::CreateMessage::new()
                    .content(r.content)
                    .allowed_mentions(mentions()),
            )
            .await
            .map_err(internal)?;
        Ok(Response::new(pb::SendDmResponse {
            message: Some(message(&sent, String::new())),
        }))
    }

    async fn read_channel(
        &self,
        request: Request<pb::ReadChannelRequest>,
    ) -> Result<Response<pb::ReadChannelResponse>, Status> {
        let r = request.into_inner();
        let n = limit(r.limit)?;
        let channel = self.guild_channel(r.channel_id).await?;
        let got = channel
            .id
            .messages(&self.http, serenity::GetMessages::new().limit(n))
            .await
            .map_err(internal)?;
        Ok(Response::new(pb::ReadChannelResponse {
            messages: got
                .iter()
                .rev()
                .map(|m| message(m, channel.name.clone()))
                .collect(),
        }))
    }

    async fn read_dms(
        &self,
        request: Request<pb::ReadDmsRequest>,
    ) -> Result<Response<pb::ReadDmsResponse>, Status> {
        let r = request.into_inner();
        let n = limit(r.limit)?;
        let dm = self.dm_channel(r.user_id).await?;
        let got = dm
            .id
            .messages(&self.http, serenity::GetMessages::new().limit(n))
            .await
            .map_err(internal)?;
        Ok(Response::new(pb::ReadDmsResponse {
            messages: got
                .iter()
                .rev()
                .map(|m| message(m, String::new()))
                .collect(),
        }))
    }

    async fn find_user(
        &self,
        request: Request<pb::FindUserRequest>,
    ) -> Result<Response<pb::FindUserResponse>, Status> {
        let query = request.into_inner().query;
        let query = query.trim().trim_start_matches('@');
        if query.is_empty() {
            return Err(Status::invalid_argument("query is required"));
        }
        let wanted = query.to_lowercase();
        let members = self
            .guild
            .search_members(&self.http, query, Some(25))
            .await
            .map_err(internal)?;
        let users = members
            .iter()
            .filter(|m| {
                [
                    Some(m.user.name.as_str()),
                    m.user.global_name.as_deref(),
                    m.nick.as_deref(),
                ]
                .into_iter()
                .flatten()
                .any(|n| n.to_lowercase() == wanted)
            })
            .map(|m| user(&m.user, m.nick.as_deref()))
            .collect();
        Ok(Response::new(pb::FindUserResponse { users }))
    }

    async fn list_channels(
        &self,
        _: Request<pb::ListChannelsRequest>,
    ) -> Result<Response<pb::ListChannelsResponse>, Status> {
        let mut channels: Vec<_> = self
            .guild
            .channels(&self.http)
            .await
            .map_err(internal)?
            .into_values()
            .collect();
        admin::sort(&mut channels);
        Ok(Response::new(pb::ListChannelsResponse {
            channels: channels.iter().map(admin::channel).collect(),
        }))
    }

    async fn list_roles(
        &self,
        _: Request<pb::ListRolesRequest>,
    ) -> Result<Response<pb::ListRolesResponse>, Status> {
        let mut roles: Vec<_> = self
            .guild
            .roles(&self.http)
            .await
            .map_err(internal)?
            .into_values()
            .collect();
        roles.sort_by_key(|r| std::cmp::Reverse((r.position, r.id)));
        Ok(Response::new(pb::ListRolesResponse {
            roles: roles.iter().map(admin::role).collect(),
        }))
    }

    async fn create_channel(
        &self,
        request: Request<pb::CreateChannelRequest>,
    ) -> Result<Response<pb::CreateChannelResponse>, Status> {
        let r = request.into_inner();
        admin::text("name", &r.name, 1, admin::MAX_NAME)?;
        admin::text("topic", &r.topic, 0, admin::MAX_TOPIC)?;
        let kind = admin::creatable(r.kind())?;
        let mut builder = serenity::CreateChannel::new(r.name).kind(kind);
        if let Some(parent) = r.parent_id {
            if kind == serenity::ChannelType::Category {
                return Err(Status::invalid_argument("a category has no parent"));
            }
            builder = builder.category(self.category(parent).await?);
        }
        if !r.topic.is_empty() {
            builder = builder.topic(r.topic);
        }
        let made = self
            .guild
            .create_channel(&self.http, builder.audit_log_reason("relay"))
            .await
            .map_err(internal)?;
        tracing::info!(channel = %made.id, name = %made.name, "relay created a channel");
        Ok(Response::new(pb::CreateChannelResponse {
            channel: Some(admin::channel(&made)),
        }))
    }

    async fn edit_channel(
        &self,
        request: Request<pb::EditChannelRequest>,
    ) -> Result<Response<pb::EditChannelResponse>, Status> {
        let r = request.into_inner();
        let channel = self.guild_channel(r.channel_id).await?;
        let mut body = admin::EditBody::default();
        if let Some(name) = r.name {
            admin::text("name", &name, 1, admin::MAX_NAME)?;
            body.name = Some(name);
        }
        if let Some(topic) = r.topic {
            admin::text("topic", &topic, 0, admin::MAX_TOPIC)?;
            body.topic = Some(topic);
        }
        body.nsfw = r.nsfw;
        if let Some(parent) = r.parent_id {
            body.parent_id = Some(match parent {
                0 => None,
                id => Some(self.category(id).await?),
            });
        }
        if let Some(position) = r.position {
            body.position =
                Some(u16::try_from(position).map_err(|_| Status::invalid_argument("position"))?);
        }
        if let Some(s) = r.rate_limit_per_user {
            body.rate_limit_per_user = Some(admin::slowmode("rate_limit_per_user", s)?);
        }
        let forum = admin::is_forum(channel.kind);
        if let Some(s) = r.default_thread_rate_limit_per_user {
            body.default_thread_rate_limit_per_user =
                Some(admin::slowmode("default_thread_rate_limit_per_user", s)?);
        }
        if let Some(require) = r.require_tag {
            if !forum {
                return Err(Status::invalid_argument("require_tag is for forums"));
            }
            let mut flags = channel.flags;
            flags.set(serenity::ChannelFlags::REQUIRE_TAG, require);
            body.flags = Some(flags.bits());
        }
        if let Some(tags) = r.tags {
            if !forum {
                return Err(Status::invalid_argument("tags are for forums"));
            }
            body.available_tags = Some(admin::tag_bodies(tags.tags)?);
        }
        // Discord checks REQUIRE_TAG against the tags the forum already has, so new tags go first.
        if body.flags.is_some() && body.available_tags.is_some() {
            let tags = admin::EditBody {
                available_tags: body.available_tags.take(),
                ..Default::default()
            };
            self.http
                .edit_channel(channel.id, &tags, Some("relay"))
                .await
                .map_err(internal)?;
        }
        let edited = self
            .http
            .edit_channel(channel.id, &body, Some("relay"))
            .await
            .map_err(internal)?;
        tracing::info!(channel = %edited.id, "relay edited a channel");
        Ok(Response::new(pb::EditChannelResponse {
            channel: Some(admin::channel(&edited)),
        }))
    }

    async fn set_channel_permission(
        &self,
        request: Request<pb::SetChannelPermissionRequest>,
    ) -> Result<Response<pb::SetChannelPermissionResponse>, Status> {
        let r = request.into_inner();
        let o = r
            .overwrite
            .ok_or_else(|| Status::invalid_argument("overwrite is required"))?;
        if o.target_id == 0 {
            return Err(Status::invalid_argument("target_id is required"));
        }
        if admin::permissions(o.allow).contains(serenity::Permissions::ADMINISTRATOR) {
            return Err(Status::permission_denied(
                "a channel overwrite never grants Administrator",
            ));
        }
        let kind = match o.target() {
            pb::OverwriteTarget::Role => {
                serenity::PermissionOverwriteType::Role(serenity::RoleId::new(o.target_id))
            }
            pb::OverwriteTarget::Member => {
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(o.target_id))
            }
            pb::OverwriteTarget::Unspecified => {
                return Err(Status::invalid_argument("target must be role or member"));
            }
        };
        let channel = self.guild_channel(r.channel_id).await?;
        if o.allow == 0 && o.deny == 0 {
            channel
                .id
                .delete_permission(&self.http, kind)
                .await
                .map_err(internal)?;
        } else {
            channel
                .id
                .create_permission(
                    &self.http,
                    serenity::PermissionOverwrite {
                        allow: admin::permissions(o.allow),
                        deny: admin::permissions(o.deny),
                        kind,
                    },
                )
                .await
                .map_err(internal)?;
        }
        tracing::info!(channel = %channel.id, target = o.target_id, "relay set a channel permission");
        let fresh = self.guild_channel(r.channel_id).await?;
        Ok(Response::new(pb::SetChannelPermissionResponse {
            channel: Some(admin::channel(&fresh)),
        }))
    }

    async fn create_forum_post(
        &self,
        request: Request<pb::CreateForumPostRequest>,
    ) -> Result<Response<pb::CreateForumPostResponse>, Status> {
        let r = request.into_inner();
        admin::text("title", &r.title, 1, admin::MAX_NAME)?;
        content(&r.content)?;
        let forum = self.guild_channel(r.channel_id).await?;
        if !admin::is_forum(forum.kind) {
            return Err(Status::invalid_argument("not a forum channel"));
        }
        let post = serenity::CreateForumPost::new(
            r.title,
            serenity::CreateMessage::new()
                .content(r.content)
                .allowed_mentions(mentions()),
        )
        .set_applied_tags(r.tag_ids.into_iter().map(serenity::ForumTagId::new));
        let thread = forum
            .id
            .create_forum_post(&self.http, post)
            .await
            .map_err(internal)?;
        // A forum post's starter message shares the thread's id.
        let first = thread
            .id
            .message(&self.http, serenity::MessageId::new(thread.id.get()))
            .await
            .map_err(internal)?;
        Ok(Response::new(pb::CreateForumPostResponse {
            thread_id: thread.id.get(),
            message: Some(message(&first, thread.name.clone())),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_must_match_exactly() {
        assert!(same(b"Bearer abc", b"Bearer abc"));
        assert!(!same(b"Bearer abd", b"Bearer abc"));
        assert!(!same(b"Bearer ab", b"Bearer abc"));
        assert!(!same(b"", b"Bearer abc"));
    }

    #[test]
    fn limits_and_content_are_checked() {
        assert_eq!(limit(0).unwrap(), 50);
        assert_eq!(limit(100).unwrap(), 100);
        assert!(limit(101).is_err());
        assert!(content("  ").is_err());
        assert!(content(&"x".repeat(2001)).is_err());
        assert!(content(&"é".repeat(2000)).is_ok());
    }
}
