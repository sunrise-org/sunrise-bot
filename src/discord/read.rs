use super::{Api, ApiError};
use crate::{
    config::{ChannelKind, ServerConfig},
    kit::{
        live::{Live, LiveThread},
        plan::{Known, find_channels},
    },
};

/// The snapshot plus what the bot-owned messages need: the messages of the channels that hold one, and the bot's own forum posts with their first message.
pub async fn full<A: Api>(api: &A, config: &ServerConfig, known: &Known) -> Result<Live, ApiError> {
    let mut live = api.snapshot().await?;
    // A duplicate channel is the planner's error to report; here it only means nothing more is read.
    let ids = find_channels(config, &live, known).unwrap_or_default();
    for (_, ch) in config.channels().filter(|(_, c)| !c.messages.is_empty()) {
        let Some(id) = ids.get(&ch.key).copied() else {
            continue;
        };
        if ch.kind == ChannelKind::Forum {
            let mine: Vec<LiveThread> = api
                .forum_threads(id)
                .await?
                .into_iter()
                .filter(|t| t.owner_id == Some(live.bot_id))
                .collect();
            for t in mine {
                live.messages.insert(t.id, api.messages(t.id).await?);
                live.threads.push(t);
            }
        } else {
            live.messages.insert(id, api.messages(id).await?);
        }
    }
    Ok(live)
}
