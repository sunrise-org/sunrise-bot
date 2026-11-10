//! `sunrise-bot run`: the gateway connection, the guild's slash commands, the role buttons and the health probes.
mod commands;
pub mod roles;

use std::sync::Arc;

use anyhow::Context as _;
use poise::serenity_prelude as serenity;
use tokio::net::TcpListener;

use crate::{
    bootstrap::Env,
    config::Config,
    discord::executor::Ledger,
    http::{self, Health},
    ids::Snowflake,
    kit::plan::{Known, ObjectKind},
    store::Store,
};

pub type Error = anyhow::Error;

/// What every command and event handler can reach.
pub struct Data {
    pub config: Arc<Config>,
    pub store: Store,
    pub health: Health,
    pub guild: serenity::GuildId,
}

impl Data {
    /// The ids the bot has written down (channels and roles by config key).
    pub async fn known(&self) -> anyhow::Result<Known> {
        self.store.load().await
    }
}

/// The id of a channel by its config key.
pub fn channel_ids(known: &Known) -> impl Fn(&str) -> Option<Snowflake> + '_ {
    move |key| {
        crate::config::Key::new(key)
            .ok()
            .and_then(|k| known.get(ObjectKind::Channel, &k))
            .map(|o| o.id)
    }
}

/// Only what the commands and buttons need; the privileged intents come with the features that read messages.
fn intents() -> serenity::GatewayIntents {
    serenity::GatewayIntents::GUILDS
}

pub async fn run(env: Env) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = Arc::new(Config::load(&env.config_dir)?);
    let store = Store::open(&env.database).await?;
    let health = Health::new(store.clone());
    let guild = serenity::GuildId::new(env.guild.0);

    let listener = TcpListener::bind(&env.http_addr)
        .await
        .with_context(|| format!("cannot listen on {}", env.http_addr))?;
    tracing::info!(addr = %env.http_addr, "http listening");
    let server = axum::serve(listener, http::router(health.clone()))
        .with_graceful_shutdown(shutdown_signal());

    let data = Data {
        config,
        store,
        health,
        guild,
    };
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands::all(),
            event_handler: |ctx, event, framework, data| {
                Box::pin(on_event(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                // Commands live in the one guild; anything registered globally is cleared. A refusal (the bot was invited without the applications.commands scope) is logged, not fatal: the buttons still work.
                if let Err(e) = serenity::Command::set_global_commands(&ctx.http, Vec::new()).await
                {
                    tracing::error!(error = %e, "cannot clear the global commands");
                }
                if let Err(e) = poise::builtins::register_in_guild(
                    ctx,
                    &framework.options().commands,
                    data.guild,
                )
                .await
                {
                    tracing::error!(error = %e, "cannot register the guild commands; re-invite the bot with the applications.commands scope");
                }
                tracing::info!(user = %ready.user.name, "ready");
                Ok(data)
            })
        })
        .build();
    let mut client = serenity::ClientBuilder::new(&env.token, intents())
        .framework(framework)
        .await
        .context("cannot build the Discord client")?;
    let shards = client.shard_manager.clone();

    tokio::select! {
        r = client.start() => r.context("the gateway connection ended")?,
        r = server => r.context("the http server stopped")?,
        () = shutdown_signal() => {
            tracing::info!("shutting down");
            shards.shutdown_all().await;
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("a SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

async fn on_event<'a>(
    ctx: &'a serenity::Context,
    event: &'a serenity::FullEvent,
    _framework: poise::FrameworkContext<'a, Data, Error>,
    data: &'a Data,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::Ready { .. } => data.health.set_gateway(true),
        serenity::FullEvent::ShardStageUpdate { event } => {
            data.health
                .set_gateway(event.new == serenity::ConnectionStage::Connected);
        }
        serenity::FullEvent::InteractionCreate {
            interaction: serenity::Interaction::Component(click),
        } => roles::on_click(ctx, data, click).await?,
        _ => {}
    }
    Ok(())
}
