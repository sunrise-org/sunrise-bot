use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{Api, ApiError, Response};
use crate::{
    ids::Snowflake,
    kit::{
        live::{Live, LiveChannel, LiveGuild, LiveMessage, LiveRole, LiveThread},
        request::Request,
    },
};

const API: &str = "https://discord.com/api/v10";
const USER_AGENT: &str = "DiscordBot (https://playsunrise.net, 1)";
const ATTEMPTS: u32 = 5;

/// Discord's REST API with the bot's token; the token never leaves this struct.
pub struct Rest {
    http: reqwest::Client,
    token: String,
    base: String,
    guild: Snowflake,
}

impl std::fmt::Debug for Rest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rest")
            .field("base", &self.base)
            .field("guild", &self.guild)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct Me {
    id: Snowflake,
}

#[derive(Deserialize)]
struct Threads {
    threads: Vec<LiveThread>,
}

#[derive(Deserialize)]
struct RateLimited {
    retry_after: f64,
}

impl Rest {
    pub fn new(token: String, guild: Snowflake) -> Rest {
        Rest::with_base(token, guild, API.to_string())
    }

    pub fn with_base(token: String, guild: Snowflake, base: String) -> Rest {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("a client with default TLS");
        Rest {
            http,
            token,
            base,
            guild,
        }
    }

    /// One call, retried after a 429 (nothing happened) and, for reads only, after a 5xx.
    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<String, ApiError> {
        let read = method == Method::GET;
        for attempt in 1..=ATTEMPTS {
            let mut req = self
                .http
                .request(method.clone(), format!("{}{path}", self.base))
                .header("Authorization", format!("Bot {}", self.token))
                .header("X-Audit-Log-Reason", "Sunny bootstrap");
            if let Some(b) = &body {
                req = req
                    .header("Content-Type", "application/json")
                    .body(b.clone());
            }
            let res = req
                .send()
                .await
                .map_err(|e| ApiError::Transport(chain(e)))?;
            let status = res.status();
            let text = res
                .text()
                .await
                .map_err(|e| ApiError::Transport(chain(e)))?;
            if status.is_success() {
                return Ok(text);
            }
            if status == StatusCode::TOO_MANY_REQUESTS && attempt < ATTEMPTS {
                let wait =
                    serde_json::from_str::<RateLimited>(&text).map_or(1.0, |r| r.retry_after);
                tokio::time::sleep(Duration::from_secs_f64(wait.clamp(0.0, 60.0) + 0.2)).await;
                continue;
            }
            if status.is_server_error() && read && attempt < ATTEMPTS {
                tokio::time::sleep(Duration::from_secs(u64::from(attempt))).await;
                continue;
            }
            return Err(if status == StatusCode::FORBIDDEN {
                ApiError::Forbidden(text)
            } else {
                ApiError::Status {
                    status: status.as_u16(),
                    body: text,
                }
            });
        }
        Err(ApiError::Status {
            status: 429,
            body: "rate limited too often".to_string(),
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let text = self.call(Method::GET, path, None).await?;
        serde_json::from_str(&text).map_err(|e| ApiError::Decode(format!("{path}: {e}")))
    }

    async fn write<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let text = self
            .call(
                method,
                path,
                Some(serde_json::to_vec(body).expect("bodies serialize")),
            )
            .await?;
        serde_json::from_str(&text).map_err(|e| ApiError::Decode(format!("{path}: {e}")))
    }

    async fn write_done<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: &B,
    ) -> Result<Response, ApiError> {
        self.call(
            method,
            path,
            Some(serde_json::to_vec(body).expect("bodies serialize")),
        )
        .await?;
        Ok(Response::Done)
    }
}

impl Api for Rest {
    async fn snapshot(&self) -> Result<Live, ApiError> {
        let g = self.guild;
        let me: Me = self.get("/users/@me").await?;
        let guild: LiveGuild = self.get(&format!("/guilds/{g}")).await?;
        let roles: Vec<LiveRole> = self.get(&format!("/guilds/{g}/roles")).await?;
        let channels: Vec<LiveChannel> = self.get(&format!("/guilds/{g}/channels")).await?;
        Ok(Live::new(me.id, guild, roles, channels))
    }

    async fn messages(&self, channel: Snowflake) -> Result<Vec<LiveMessage>, ApiError> {
        self.get(&format!("/channels/{channel}/messages?limit=50"))
            .await
    }

    async fn forum_threads(&self, forum: Snowflake) -> Result<Vec<LiveThread>, ApiError> {
        let g = self.guild;
        let active: Threads = self.get(&format!("/guilds/{g}/threads/active")).await?;
        let archived: Threads = self
            .get(&format!(
                "/channels/{forum}/threads/archived/public?limit=100"
            ))
            .await?;
        Ok(active
            .threads
            .into_iter()
            .filter(|t| t.parent_id == Some(forum))
            .chain(archived.threads)
            .collect())
    }

    async fn send(&self, request: Request) -> Result<Response, ApiError> {
        let g = self.guild;
        match request {
            Request::PatchGuild(b) => {
                self.write_done(Method::PATCH, &format!("/guilds/{g}"), &b)
                    .await
            }
            Request::CreateRole(b) => Ok(Response::Role(
                self.write(Method::POST, &format!("/guilds/{g}/roles"), &b)
                    .await?,
            )),
            Request::PatchRole { id, body } => {
                self.write_done(Method::PATCH, &format!("/guilds/{g}/roles/{id}"), &body)
                    .await
            }
            Request::SetRolePositions(b) => {
                self.write_done(Method::PATCH, &format!("/guilds/{g}/roles"), &b)
                    .await
            }
            Request::CreateChannel(b) => Ok(Response::Channel(
                self.write(Method::POST, &format!("/guilds/{g}/channels"), &b)
                    .await?,
            )),
            Request::PatchChannel { id, body } => {
                self.write_done(Method::PATCH, &format!("/channels/{id}"), &body)
                    .await
            }
            Request::SetChannelPositions(b) => {
                self.write_done(Method::PATCH, &format!("/guilds/{g}/channels"), &b)
                    .await
            }
            Request::DeleteChannel { id } => {
                self.call(Method::DELETE, &format!("/channels/{id}"), None)
                    .await?;
                Ok(Response::Done)
            }
            Request::CreateMessage { channel, body } => Ok(Response::Message(
                self.write(
                    Method::POST,
                    &format!("/channels/{channel}/messages"),
                    &body,
                )
                .await?,
            )),
            Request::EditMessage {
                channel,
                message,
                body,
            } => {
                self.write_done(
                    Method::PATCH,
                    &format!("/channels/{channel}/messages/{message}"),
                    &body,
                )
                .await
            }
            Request::CreateThread { forum, body } => Ok(Response::Thread(
                self.write(Method::POST, &format!("/channels/{forum}/threads"), &body)
                    .await?,
            )),
        }
    }
}

/// An error and everything under it, without the URL.
fn chain(e: reqwest::Error) -> String {
    let mut parts = vec![e.to_string()];
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        parts.push(s.to_string());
        source = s.source();
    }
    parts.join(": ")
}
