use std::{path::Path, str::FromStr};

use anyhow::Context;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use crate::{
    config::Key,
    discord::executor::Ledger,
    ids::Snowflake,
    kit::plan::{Known, ObjectKind, Origin},
};

/// The bot's SQLite file. Only the bot process writes it.
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn open(path: &Path) -> anyhow::Result<Store> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        Store::connect(options, 4).await
    }

    /// A database that lives as long as the store.
    pub async fn memory() -> anyhow::Result<Store> {
        Store::connect(SqliteConnectOptions::from_str("sqlite::memory:")?, 1).await
    }

    async fn connect(options: SqliteConnectOptions, connections: u32) -> anyhow::Result<Store> {
        let pool = SqlitePoolOptions::new()
            .max_connections(connections)
            .connect_with(options)
            .await
            .context("cannot open the database")?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("cannot migrate the database")?;
        Ok(Store { pool })
    }
}

impl Store {
    /// Does the database answer?
    pub async fn ping(&self) -> anyhow::Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }
}

/// How many relay events the store keeps for clients that reconnect.
const RELAY_KEEP: i64 = 2000;

impl Store {
    /// Writes an encoded relay event and returns its sequence number, or `None` when that message was already recorded.
    pub async fn relay_append(
        &self,
        message_id: Snowflake,
        event: &[u8],
        created_at: i64,
    ) -> anyhow::Result<Option<u64>> {
        let seq: Option<i64> = sqlx::query_scalar(
            "INSERT INTO relay_events (message_id, event, created_at) VALUES (?, ?, ?)
             ON CONFLICT (message_id) DO NOTHING RETURNING seq",
        )
        .bind(message_id.to_string())
        .bind(event)
        .bind(created_at)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(seq) = seq {
            sqlx::query("DELETE FROM relay_events WHERE seq <= ?")
                .bind(seq - RELAY_KEEP)
                .execute(&self.pool)
                .await?;
        }
        Ok(seq.map(|s| s as u64))
    }

    /// Up to `limit` encoded events after `seq`, oldest first.
    pub async fn relay_after(&self, seq: u64, limit: u32) -> anyhow::Result<Vec<(u64, Vec<u8>)>> {
        let rows =
            sqlx::query("SELECT seq, event FROM relay_events WHERE seq > ? ORDER BY seq LIMIT ?")
                .bind(seq as i64)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get::<i64, _>("seq") as u64, r.get("event")))
            .collect())
    }
}

impl Ledger for Store {
    async fn load(&self) -> anyhow::Result<Known> {
        let mut known = Known::default();
        for row in sqlx::query("SELECT kind, key, id, origin FROM discord_objects")
            .fetch_all(&self.pool)
            .await?
        {
            let kind = ObjectKind::parse(row.get("kind"))
                .context("unknown object kind in the database")?;
            let key = Key::new(row.get("key")).map_err(anyhow::Error::msg)?;
            let id: Snowflake = row
                .get::<String, _>("id")
                .parse()
                .context("bad id in the database")?;
            let origin = if row.get::<String, _>("origin") == "created" {
                Origin::Created
            } else {
                Origin::Adopted
            };
            known.insert(kind, key, id, origin);
        }
        Ok(known)
    }

    async fn record(
        &self,
        kind: ObjectKind,
        key: &Key,
        id: Snowflake,
        origin: Origin,
    ) -> anyhow::Result<()> {
        let origin = if origin == Origin::Created {
            "created"
        } else {
            "adopted"
        };
        sqlx::query(
            "INSERT INTO discord_objects (kind, key, id, origin) VALUES (?, ?, ?, ?)
             ON CONFLICT (kind, key) DO UPDATE SET id = excluded.id, origin = CASE WHEN discord_objects.id = excluded.id THEN discord_objects.origin ELSE excluded.origin END",
        )
        .bind(kind.as_str())
        .bind(key.as_str())
        .bind(id.to_string())
        .bind(origin)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn forget(&self, kind: ObjectKind, key: &Key) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM discord_objects WHERE kind = ? AND key = ?")
            .bind(kind.as_str())
            .bind(key.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
