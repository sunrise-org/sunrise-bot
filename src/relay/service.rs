use std::{net::SocketAddr, sync::Arc, time::Duration};

use poise::serenity_prelude as serenity;
use tokio::sync::{broadcast::error::RecvError, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use super::{
    Hub, message, pb,
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
