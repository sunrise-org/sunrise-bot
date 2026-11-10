// Ported from Kira (github.com/horizon-rust/kira, src/texts.js `rolesMessage` and `howToPost`; src/adopt.js `isCopy`), MIT © 2026 Ludovic Marie and the Kira contributors; see LICENSES/kira-MIT.txt
//! The bot-owned messages as Discord wants them, and how to tell whether a live message already says it.
use serde::Serialize;

use super::live::LiveMessage;
use crate::{config::*, ids::Snowflake};

/// The colour of an embed with none set in the file: Sunrise's yellow.
pub const EMBED_COLOR: u32 = 0xFFCF5C;

/// The `custom_id` of a role button is this and the role's key.
pub const ROLE_BUTTON: &str = "role:";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EmbedBody {
    pub title: String,
    pub description: String,
    pub color: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ButtonBody {
    #[serde(rename = "type")]
    pub kind: u8,
    pub style: u8,
    pub label: String,
    pub custom_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RowBody {
    #[serde(rename = "type")]
    pub kind: u8,
    pub components: Vec<ButtonBody>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AllowedMentions {
    pub parse: Vec<String>,
}

/// What the bot sends, or edits a message to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MessageBody {
    pub embeds: Vec<EmbedBody>,
    pub components: Vec<RowBody>,
    pub allowed_mentions: AllowedMentions,
}

impl MessageBody {
    fn new(embed: EmbedBody, buttons: Vec<ButtonBody>) -> MessageBody {
        let components = if buttons.is_empty() {
            Vec::new()
        } else {
            vec![RowBody {
                kind: 1,
                components: buttons,
            }]
        };
        MessageBody {
            embeds: vec![embed],
            components,
            allowed_mentions: AllowedMentions { parse: Vec::new() },
        }
    }

    pub fn title(&self) -> &str {
        &self.embeds[0].title
    }
}

/// `channel_id`: the id of a channel by its key (for `{#key}` in texts), when it exists.
pub fn embed_message(
    config: &ServerConfig,
    key: &Key,
    channel_id: &dyn Fn(&str) -> Option<Snowflake>,
) -> Option<MessageBody> {
    let Some(MessageSpec::Embed {
        embed,
        role_buttons,
    }) = config.messages.get(key)
    else {
        return None;
    };
    let buttons = role_buttons
        .iter()
        .filter_map(|k| config.role(k.as_str()))
        .map(|r| ButtonBody {
            kind: 2,
            style: 2,
            label: r.name.clone(),
            custom_id: format!("{ROLE_BUTTON}{}", r.key),
        })
        .collect();
    let body = EmbedBody {
        title: embed.title.clone(),
        description: render_channel_refs(&embed.description, channel_id),
        color: embed.color.map_or(EMBED_COLOR, |c| c.0),
    };
    Some(MessageBody::new(body, buttons))
}

/// The pinned guide post of a forum: what it is for, the template to copy, and the habits that keep posts easy to answer.
pub fn how_to_post(forum: &ChannelSpec) -> Option<MessageBody> {
    let template = forum.template.as_ref()?;
    let mut lines = vec![
        forum.topic.clone().unwrap_or_default(),
        String::new(),
        "**Copy this into your post and fill it in:**".to_string(),
        "```".to_string(),
        template.text.replace("**", ""),
        "```".to_string(),
        "**Before you post**".to_string(),
        format!(
            "• Pick {}.",
            if forum.tags.iter().any(|t| t.team_only) {
                "the tags that fit (some are set by the team)"
            } else {
                "the tags that fit"
            }
        ),
    ];
    if let Some(done) = &forum.done_tag {
        lines.push(format!(
            "• When you're done, type `/done` in your post: it gets the **{done}** tag and closes."
        ));
    }
    let description: String = lines.join("\n").chars().take(4096).collect();
    Some(MessageBody::new(
        EmbedBody {
            title: format!("How to post in #{}", forum.name),
            description,
            color: EMBED_COLOR,
        },
        Vec::new(),
    ))
}

/// How a live message differs from what is wanted: empty when it already says it.
pub fn differences(want: &MessageBody, have: &LiveMessage) -> Vec<&'static str> {
    let mut out = Vec::new();
    let rich: Vec<_> = have
        .embeds
        .iter()
        .filter(|e| e.kind.as_deref().is_none_or(|k| k == "rich"))
        .collect();
    match rich.as_slice() {
        [e] => {
            let w = &want.embeds[0];
            if e.title.as_deref() != Some(w.title.as_str())
                || e.description.as_deref() != Some(w.description.as_str())
            {
                out.push("text");
            }
            if e.color != Some(w.color) {
                out.push("colour");
            }
        }
        _ => out.push("text"),
    }
    let buttons =
        |rows: &mut dyn Iterator<Item = (Option<String>, Option<String>)>| rows.collect::<Vec<_>>();
    let want_buttons = buttons(
        &mut want
            .components
            .iter()
            .flat_map(|r| &r.components)
            .map(|b| (Some(b.custom_id.clone()), Some(b.label.clone()))),
    );
    let have_buttons = buttons(
        &mut have
            .components
            .iter()
            .flat_map(|r| &r.components)
            .filter(|c| c.kind == 2)
            .map(|c| (c.custom_id.clone(), c.label.clone())),
    );
    if want_buttons != have_buttons {
        out.push("buttons");
    }
    out
}

/// Is `message` a copy of what the bot posts as `want`: its own, with the same embed title (Kira's `isCopy`)?
pub fn is_copy(message: &LiveMessage, bot: Snowflake, want: &MessageBody) -> bool {
    message.author.id == bot
        && message
            .embeds
            .iter()
            .find(|e| e.kind.as_deref().is_none_or(|k| k == "rich"))
            .and_then(|e| e.title.as_deref())
            == Some(want.title())
}
