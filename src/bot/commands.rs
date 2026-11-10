// Ported from Kira (github.com/horizon-rust/kira, src/features/community.js: `/help` is private to the asker, `/links` is public), MIT © 2026 Ludovic Marie and the Kira contributors; see LICENSES/kira-MIT.txt
use poise::serenity_prelude as serenity;

use super::{Data, Error, channel_ids};
use crate::kit::{
    info::{self, Card},
    message::EMBED_COLOR,
};

type Context<'a> = poise::Context<'a, Data, Error>;

pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![help(), links()]
}

fn embed(card: Card) -> serenity::CreateEmbed {
    serenity::CreateEmbed::new()
        .colour(EMBED_COLOR)
        .title(card.title)
        .description(card.description)
}

/// What Sunny does here, and where things are
#[poise::command(slash_command, guild_only)]
async fn help(ctx: Context<'_>) -> Result<(), Error> {
    let known = ctx.data().known().await?;
    let card = info::help(&ctx.data().config.bot, channel_ids(&known));
    ctx.send(
        poise::CreateReply::default()
            .embed(embed(card))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// Sunrise's website, code and profiler
#[poise::command(slash_command, guild_only)]
async fn links(ctx: Context<'_>) -> Result<(), Error> {
    let card = info::links(&ctx.data().config.bot);
    ctx.send(poise::CreateReply::default().embed(embed(card)))
        .await?;
    Ok(())
}
