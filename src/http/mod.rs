//! The HTTP side: health probes only (the webhook comes in a later step); every other path is a 404.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use axum::{Router, extract::State, http::StatusCode, routing::get};

use crate::store::Store;

/// What the probes look at.
#[derive(Clone)]
pub struct Health {
    gateway: Arc<AtomicBool>,
    store: Store,
}

impl Health {
    pub fn new(store: Store) -> Health {
        Health {
            gateway: Arc::new(AtomicBool::new(false)),
            store,
        }
    }

    /// The gateway connection is up (set from the Discord events).
    pub fn set_gateway(&self, up: bool) {
        self.gateway.store(up, Ordering::Relaxed);
    }

    pub fn gateway(&self) -> bool {
        self.gateway.load(Ordering::Relaxed)
    }
}

pub fn router(health: Health) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .with_state(health)
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(State(h): State<Health>) -> (StatusCode, &'static str) {
    if !h.gateway() {
        return (StatusCode::SERVICE_UNAVAILABLE, "gateway not connected");
    }
    match h.store.ping().await {
        Ok(()) => (StatusCode::OK, "ready"),
        Err(e) => {
            tracing::warn!(error = %e, "readiness: the database does not answer");
            (StatusCode::SERVICE_UNAVAILABLE, "database not open")
        }
    }
}
