// Ported from Kira (github.com/horizon-rust/kira, src/features/community.js `linksEmbed`/`helpEmbed`, src/texts.js `helpText`), MIT © 2026 Ludovic Marie and the Kira contributors; see LICENSES/kira-MIT.txt
//! The texts of `/help` and `/links`.
use crate::{
    config::{BotConfig, render_channel_refs},
    ids::Snowflake,
};

/// An embed's words; the colour is the bot's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub title: String,
    pub description: String,
}

/// `channel_id`: a channel's id by its key, so the texts can mention it.
pub fn help(bot: &BotConfig, channel_id: impl Fn(&str) -> Option<Snowflake>) -> Card {
    Card {
        title: bot.help.title.clone(),
        description: render_channel_refs(&bot.help.text, channel_id),
    }
}

pub fn links(bot: &BotConfig) -> Card {
    let lines: Vec<String> = bot
        .links
        .iter()
        .map(|l| format!("{} {}: {}", l.emoji, l.label, l.url))
        .collect();
    Card {
        title: "🌅 Sunrise".to_string(),
        description: lines.join("\n"),
    }
}
