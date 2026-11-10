pub mod executor;
pub mod fake;
pub mod read;
pub mod rest;

use crate::{
    ids::Snowflake,
    kit::{
        live::{Live, LiveChannel, LiveMessage, LiveRole, LiveThread},
        request::Request,
    },
};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Discord refused (403): {0}")]
    Forbidden(String),
    #[error("Discord answered {status}: {body}")]
    Status { status: u16, body: String },
    #[error("cannot reach Discord: {0}")]
    Transport(String),
    #[error("unexpected answer from Discord: {0}")]
    Decode(String),
}

/// What Discord returns for the requests that return something.
#[derive(Debug, Clone)]
pub enum Response {
    Done,
    Role(LiveRole),
    Channel(LiveChannel),
    Message(LiveMessage),
    Thread(LiveThread),
}

/// The guild as the bootstrap sees it: read it, change it with typed requests.
#[allow(async_fn_in_trait)]
pub trait Api {
    /// The guild, its roles and channels (no messages: see `read::full`).
    async fn snapshot(&self) -> Result<Live, ApiError>;
    /// The latest messages of a channel or forum post, newest first.
    async fn messages(&self, channel: Snowflake) -> Result<Vec<LiveMessage>, ApiError>;
    /// The posts of a forum, active and archived.
    async fn forum_threads(&self, forum: Snowflake) -> Result<Vec<LiveThread>, ApiError>;
    async fn send(&self, request: Request) -> Result<Response, ApiError>;
}
