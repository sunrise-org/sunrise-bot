pub mod executor;
pub mod fake;
pub mod rest;

use crate::kit::{
    live::{Live, LiveChannel, LiveRole},
    request::Request,
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
}

/// The guild as the bootstrap sees it: read it whole, change it with typed requests.
#[allow(async_fn_in_trait)]
pub trait Api {
    async fn snapshot(&self) -> Result<Live, ApiError>;
    async fn send(&self, request: Request) -> Result<Response, ApiError>;
}
