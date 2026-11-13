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
    relay::{self, Hub},
    store::Store,
};

pub type Error = anyhow::Error;

/// What every command and event handler can reach.
pub struct Data {
    pub config: Arc<Config>,
    pub store: Store,
    pub health: Health,
    pub guild: serenity::GuildId,
    pub relay: Arc<Hub>,
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

/// The commands and buttons need guilds; the relay needs the messages that reach Sunny, whose content Discord sends without the privileged intent.
fn intents() -> serenity::GatewayIntents {
    serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::DIRECT_MESSAGES
}

pub async fn run(env: Env) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = Arc::new(Config::load(&env.config_dir)?);
    let store = Store::open(&env.database).await?;
    let health = Health::new(store.clone());
    let guild = serenity::GuildId::new(env.guild.0);
    let hub = Hub::new(store.clone());

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
        relay: hub.clone(),
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
    let http = client.http.clone();
    let relay = async move {
        let Some(token) = env.relay_token else {
            tracing::info!("RELAY_TOKEN is not set: the relay is off");
            return std::future::pending().await;
        };
        let addr = env
            .relay_addr
            .parse()
            .with_context(|| format!("RELAY_ADDR {} is not an address", env.relay_addr))?;
        let service = relay::RelayService::new(hub, http, guild);
        relay::serve(service, addr, token, shutdown_signal()).await
    };

    tokio::select! {
        r = client.start() => r.context("the gateway connection ended")?,
        r = server => r.context("the http server stopped")?,
        r = relay => r.context("the relay stopped")?,
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
    framework: poise::FrameworkContext<'a, Data, Error>,
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
        serenity::FullEvent::Message { new_message } => {
            if let Some(kind) = relay::classify(new_message, framework.bot_id) {
                let channel_name = match new_message.guild_id {
                    Some(_) => new_message.channel_id.name(ctx).await.unwrap_or_default(),
                    None => String::new(),
                };
                let message = relay::message(new_message, channel_name);
                // A relay that cannot record must not break the commands and buttons.
                if let Err(e) = data.relay.publish(kind, message).await {
                    tracing::error!(error = %e, "cannot record a relay event");
                }
            }
        }
        _ => {}
    }
    Ok(())
}
