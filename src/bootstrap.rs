//! `sunrise-bot bootstrap`: the plan, or the plan applied.
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::{
    config::Config,
    discord::{
        Api,
        executor::{Ledger, apply},
        read,
        rest::Rest,
    },
    ids::Snowflake,
    kit::plan::{Known, PlanInput, plan},
    store::Store,
};

pub struct Env {
    pub token: String,
    pub guild: Snowflake,
    pub database: PathBuf,
    pub config_dir: PathBuf,
    pub http_addr: String,
    /// The relay serves only when `RELAY_TOKEN` is set.
    pub relay_token: Option<String>,
    pub relay_addr: String,
}

impl Env {
    /// `DISCORD_TOKEN` and `GUILD_ID` are required; the rest have defaults.
    pub fn from_env() -> anyhow::Result<Env> {
        let token = std::env::var("DISCORD_TOKEN").context("DISCORD_TOKEN is not set")?;
        let guild = std::env::var("GUILD_ID")
            .context("GUILD_ID is not set")?
            .parse()
            .context("GUILD_ID is not a Discord id")?;
        let database = std::env::var("DATABASE_PATH")
            .unwrap_or_else(|_| "sunrise-bot.sqlite".to_string())
            .into();
        let config_dir = std::env::var("CONFIG_DIR")
            .unwrap_or_else(|_| "config".to_string())
            .into();
        let http_addr = std::env::var("HTTP_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
        let relay_token = std::env::var("RELAY_TOKEN").ok().filter(|t| !t.is_empty());
        let relay_addr =
            std::env::var("RELAY_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".to_string());
        Ok(Env {
            token,
            guild,
            database,
            config_dir,
            http_addr,
            relay_token,
            relay_addr,
        })
    }
}

pub async fn run(env: Env, apply_plan: bool, prune: bool) -> anyhow::Result<()> {
    let config = Config::load(&env.config_dir)?;
    let rest = Rest::new(env.token, env.guild);
    if apply_plan {
        let store = Store::open(&env.database).await?;
        let out = apply(&config.server, &rest, &store, prune).await?;
        for line in &out.applied {
            println!("{line}");
        }
        for w in &out.waiting {
            println!("! {w}");
        }
        if out.writes == 0 {
            println!(
                "Nothing to do. ({} object(s) recorded as found)",
                out.adopted
            );
        } else {
            println!(
                "Done: {} call(s) to Discord, {} object(s) recorded as found.",
                out.writes, out.adopted
            );
        }
    } else {
        println!("{}", read_plan(&rest, &config, &env.database, prune).await?);
    }
    Ok(())
}

/// The plan as text, without touching Discord or the database (the ledger is read if the file exists).
pub async fn read_plan<A: Api>(
    api: &A,
    config: &Config,
    database: &Path,
    prune: bool,
) -> anyhow::Result<String> {
    let known = if database.exists() {
        Store::open(database).await?.load().await?
    } else {
        Known::default()
    };
    let live = read::full(api, &config.server, &known).await?;
    let p = plan(&PlanInput {
        config: &config.server,
        live: &live,
        known: &known,
        community_denied: false,
        prune,
    })?;
    Ok(p.render())
}
