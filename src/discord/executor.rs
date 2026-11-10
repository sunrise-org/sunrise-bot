// Ported from Kira (github.com/horizon-rust/kira, src/setup.js: ids saved as each object is made), MIT © 2026 Ludovic Marie and the Kira contributors; see LICENSES/kira-MIT.txt
//! Applies the plan to a guild, saving every id the moment the object exists.
use std::sync::Mutex;

use super::{Api, ApiError, Response, read};
use crate::{
    config::{Key, ServerConfig},
    ids::Snowflake,
    kit::{
        plan::{Action, Known, ObjectKind, Origin, PlanError, PlanInput, Waiting, plan},
        request::{BuildError, Ctx, Ids, build},
    },
};

/// Where the bot writes down the ids it knows.
#[allow(async_fn_in_trait)]
pub trait Ledger {
    async fn load(&self) -> anyhow::Result<Known>;
    async fn record(
        &self,
        kind: ObjectKind,
        key: &Key,
        id: Snowflake,
        origin: Origin,
    ) -> anyhow::Result<()>;
    async fn forget(&self, kind: ObjectKind, key: &Key) -> anyhow::Result<()>;
}

/// A ledger in memory, for tests.
#[derive(Default)]
pub struct MemoryLedger(Mutex<Known>);

impl Ledger for MemoryLedger {
    async fn load(&self) -> anyhow::Result<Known> {
        Ok(self.0.lock().unwrap().clone())
    }

    async fn record(
        &self,
        kind: ObjectKind,
        key: &Key,
        id: Snowflake,
        origin: Origin,
    ) -> anyhow::Result<()> {
        self.0.lock().unwrap().insert(kind, key.clone(), id, origin);
        Ok(())
    }

    async fn forget(&self, kind: ObjectKind, key: &Key) -> anyhow::Result<()> {
        self.0.lock().unwrap().remove(kind, key);
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error(transparent)]
    Plan(#[from] PlanError),
    #[error(transparent)]
    Build(#[from] BuildError),
    #[error("{action}: {source}")]
    Api { action: String, source: ApiError },
    #[error("cannot read the server: {0}")]
    Read(#[from] ApiError),
    #[error(transparent)]
    Ledger(#[from] anyhow::Error),
    #[error("the server did not settle after {0} passes; what is left:\n{1}")]
    NotSettled(usize, String),
}

#[derive(Debug, Default)]
pub struct Outcome {
    /// What was done, as the plan printed it.
    pub applied: Vec<String>,
    /// Calls that changed something on Discord.
    pub writes: usize,
    pub adopted: usize,
    pub waiting: Vec<Waiting>,
    /// The bot asked for Community and Discord said no.
    pub community_denied: bool,
}

const PASSES: usize = 4;

/// Plans, applies, and plans again until nothing is left (making channels can leave their order to fix).
pub async fn apply<A: Api, L: Ledger>(
    config: &ServerConfig,
    api: &A,
    ledger: &L,
    prune: bool,
) -> Result<Outcome, ApplyError> {
    let mut out = Outcome::default();
    for _ in 0..PASSES {
        let mut known = ledger.load().await?;
        let live = read::full(api, config, &known).await?;
        let p = plan(&PlanInput {
            config,
            live: &live,
            known: &known,
            community_denied: out.community_denied,
            prune,
        })?;
        for a in &p.adopt {
            ledger.record(a.kind, &a.key, a.id, Origin::Adopted).await?;
            known.insert(a.kind, a.key.clone(), a.id, Origin::Adopted);
            out.adopted += 1;
        }
        out.waiting = p.waiting.clone();
        if p.is_empty() {
            return Ok(out);
        }
        let mut ids = Ids::from_known(&known, &live);
        for (action, lines) in p.steps() {
            if out.community_denied && action.needs_community(config) {
                continue;
            }
            let requests = build(
                action,
                &Ctx {
                    config,
                    ids: &ids,
                    live: &live,
                },
            )?;
            let mut first = None;
            let mut refused = false;
            for request in requests {
                match api.send(request).await {
                    Ok(r) => {
                        out.writes += 1;
                        first.get_or_insert(r);
                    }
                    Err(ApiError::Forbidden(why)) if matches!(action, Action::EnableCommunity) => {
                        tracing::warn!(%why, "the bot may not enable Community; waiting for a person to");
                        out.community_denied = true;
                        refused = true;
                    }
                    Err(source) => {
                        return Err(ApplyError::Api {
                            action: lines.first().cloned().unwrap_or_default(),
                            source,
                        });
                    }
                }
            }
            let Some(response) = first.filter(|_| !refused) else {
                continue;
            };
            out.applied.extend(lines.iter().cloned());
            tracing::info!(action = %lines.join("; "), "applied");
            match (action, response) {
                (Action::CreateRole { key }, Response::Role(r)) => {
                    created(ledger, &mut ids, ObjectKind::Role, key, r.id).await?
                }
                (Action::CreateCategory { key }, Response::Channel(c)) => {
                    created(ledger, &mut ids, ObjectKind::Category, key, c.id).await?
                }
                (Action::CreateChannel { key, .. }, Response::Channel(c)) => {
                    created(ledger, &mut ids, ObjectKind::Channel, key, c.id).await?
                }
                (Action::CreateMessage { key, .. }, Response::Message(m)) => {
                    created(ledger, &mut ids, ObjectKind::Message, key, m.id).await?
                }
                (Action::CreateForumPost { key, .. }, Response::Thread(t)) => {
                    created(ledger, &mut ids, ObjectKind::ForumPost, key, t.id).await?
                }
                (Action::DeleteChannel { kind, key, .. }, _) => ledger.forget(*kind, key).await?,
                _ => {}
            }
        }
    }
    let known = ledger.load().await?;
    let live = read::full(api, config, &known).await?;
    let left = plan(&PlanInput {
        config,
        live: &live,
        known: &known,
        community_denied: out.community_denied,
        prune,
    })?;
    if left.is_empty() {
        Ok(out)
    } else {
        Err(ApplyError::NotSettled(PASSES, left.render()))
    }
}

/// The id goes to the ledger before anything else happens, so a crash cannot make a second copy.
async fn created<L: Ledger>(
    ledger: &L,
    ids: &mut Ids,
    kind: ObjectKind,
    key: &Key,
    id: Snowflake,
) -> anyhow::Result<()> {
    ledger.record(kind, key, id, Origin::Created).await?;
    ids.set(kind, key.clone(), id);
    Ok(())
}
