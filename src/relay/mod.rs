//! The relay: DMs, mentions and replies to Sunny are kept and streamed to a gRPC client, which can also speak through the bot.
mod service;

use std::sync::Arc;

use poise::serenity_prelude as serenity;
use prost::Message as _;
use tokio::sync::broadcast;

use crate::{ids::Snowflake, store::Store};

pub use service::{RelayService, serve};

pub mod pb {
    tonic::include_proto!("sunny.relay.v1");
}

/// Recorded events and the live feed of new ones.
pub struct Hub {
    store: Store,
    live: broadcast::Sender<pb::Event>,
}

impl Hub {
    pub fn new(store: Store) -> Arc<Hub> {
        Arc::new(Hub {
            store,
            live: broadcast::channel(256).0,
        })
    }

    /// Records the event and hands it to live subscribers; a message seen twice is dropped.
    pub async fn publish(&self, kind: pb::EventKind, message: pb::Message) -> anyhow::Result<()> {
        let mut event = pb::Event {
            seq: 0,
            kind: kind as i32,
            message: Some(message),
        };
        let message = event.message.as_ref().expect("set above");
        let Some(seq) = self
            .store
            .relay_append(
                Snowflake(message.id),
                &event.encode_to_vec(),
                message.created_unix_ms,
            )
            .await?
        else {
            return Ok(());
        };
        event.seq = seq;
        // No subscriber is fine: the store keeps the event for the next one.
        let _ = self.live.send(event);
        Ok(())
    }

    /// Stored events after `seq`, oldest first.
    async fn after(&self, seq: u64, limit: u32) -> anyhow::Result<Vec<pb::Event>> {
        self.store
            .relay_after(seq, limit)
            .await?
            .into_iter()
            .map(|(seq, bytes)| {
                let mut event = pb::Event::decode(bytes.as_slice())?;
                event.seq = seq;
                Ok(event)
            })
            .collect()
    }
}

/// Whether a message is for Sunny: a DM, a mention of it, or a reply to one of its messages.
pub fn classify(message: &serenity::Message, me: serenity::UserId) -> Option<pb::EventKind> {
    if message.author.id == me {
        return None;
    }
    if message.guild_id.is_none() {
        return Some(pb::EventKind::Dm);
    }
    if message.mentions_user_id(me) {
        return Some(pb::EventKind::Mention);
    }
    message
        .referenced_message
        .as_ref()
        .filter(|r| r.author.id == me)
        .map(|_| pb::EventKind::Reply)
}

pub fn user(user: &serenity::User, nick: Option<&str>) -> pb::User {
    pb::User {
        id: user.id.get(),
        username: user.name.clone(),
        display_name: nick
            .or(user.global_name.as_deref())
            .unwrap_or(&user.name)
            .to_string(),
        bot: user.bot,
    }
}

fn embeds(embeds: &[serenity::Embed]) -> Vec<String> {
    embeds
        .iter()
        .map(|e| {
            [
                e.title.as_deref(),
                e.description.as_deref(),
                e.url.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" — ")
        })
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn message(message: &serenity::Message, channel_name: String) -> pb::Message {
    let nick = message.member.as_ref().and_then(|m| m.nick.as_deref());
    let origin = message.message_reference.as_ref();
    let forwarded: Vec<pb::Forwarded> = message
        .message_snapshots
        .iter()
        .map(|s| pb::Forwarded {
            channel_id: origin.map(|r| r.channel_id.get()),
            message_id: origin.and_then(|r| r.message_id).map(|id| id.get()),
            content: s.content.clone(),
            attachment_urls: s.attachments.iter().map(|a| a.url.clone()).collect(),
            embeds: embeds(&s.embeds),
            created_unix_ms: s.timestamp.unix_timestamp() * 1000,
        })
        .collect();
    let is_forward = !forwarded.is_empty();
    pb::Message {
        forwarded,
        embeds: embeds(&message.embeds),
        id: message.id.get(),
        channel_id: message.channel_id.get(),
        guild_id: message.guild_id.map(|g| g.get()),
        channel_name,
        author: Some(user(&message.author, nick)),
        content: message.content.clone(),
        attachment_urls: message.attachments.iter().map(|a| a.url.clone()).collect(),
        replied_to_id: message
            .referenced_message
            .as_ref()
            .map(|r| r.id.get())
            .or_else(|| origin.and_then(|r| r.message_id).map(|id| id.get()))
            .filter(|_| !is_forward),
        created_unix_ms: message.timestamp.unix_timestamp() * 1000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: u64) -> pb::Message {
        pb::Message {
            id,
            channel_id: 7,
            content: format!("hello {id}"),
            created_unix_ms: 1,
            ..Default::default()
        }
    }

    fn from_discord(extra: serde_json::Value) -> serenity::Message {
        let mut json = serde_json::json!({
            "id": "1570738321565687909", "channel_id": "1570522508000000000", "type": 0,
            "author": {"id": "307839762528600064", "username": "vmut", "global_name": "Umut", "discriminator": "0", "avatar": null},
            "content": "", "timestamp": "2026-11-13T09:53:18.000000+00:00", "edited_timestamp": null,
            "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
            "attachments": [], "embeds": [], "pinned": false, "flags": 0
        });
        json.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_forward_carries_its_copy_and_is_not_a_reply() {
        let forward = from_discord(serde_json::json!({
            "message_reference": {"type": 1, "channel_id": "1569824723389522012", "message_id": "1570723195697238111"},
            "message_snapshots": [{"message": {
                "type": 0, "content": "the forwarded text", "timestamp": "2026-11-13T08:53:18.000000+00:00",
                "edited_timestamp": null, "mentions": [], "mention_roles": [], "flags": 0,
                "attachments": [], "embeds": [{"title": "A link", "url": "https://example.com"}]
            }}]
        }));
        let m = message(&forward, String::new());
        assert_eq!(m.content, "");
        assert_eq!(m.replied_to_id, None);
        let f = &m.forwarded[0];
        assert_eq!(f.content, "the forwarded text");
        assert_eq!(f.message_id, Some(1570723195697238111));
        assert_eq!(f.channel_id, Some(1569824723389522012));
        assert_eq!(f.embeds, ["A link — https://example.com"]);
    }

    #[test]
    fn a_reply_keeps_its_target() {
        let reply = from_discord(serde_json::json!({
            "content": "yes",
            "message_reference": {"channel_id": "1570522508000000000", "message_id": "1570522508850106420"}
        }));
        let m = message(&reply, String::new());
        assert_eq!(m.replied_to_id, Some(1570522508850106420));
        assert!(m.forwarded.is_empty());
    }

    #[tokio::test]
    async fn a_reconnecting_client_gets_what_it_missed_once() {
        let hub = Hub::new(Store::memory().await.unwrap());
        hub.publish(pb::EventKind::Dm, event(10)).await.unwrap();
        hub.publish(pb::EventKind::Mention, event(11))
            .await
            .unwrap();
        hub.publish(pb::EventKind::Dm, event(10)).await.unwrap();

        let all = hub.after(0, 100).await.unwrap();
        let ids: Vec<u64> = all.iter().map(|e| e.message.as_ref().unwrap().id).collect();
        assert_eq!(ids, [10, 11]);
        assert_eq!(all[1].kind, pb::EventKind::Mention as i32);

        let rest = hub.after(all[0].seq, 100).await.unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].message.as_ref().unwrap().content, "hello 11");
    }

    #[tokio::test]
    async fn live_subscribers_see_the_stored_sequence_number() {
        let hub = Hub::new(Store::memory().await.unwrap());
        let mut rx = hub.live.subscribe();
        hub.publish(pb::EventKind::Reply, event(20)).await.unwrap();
        let live = rx.recv().await.unwrap();
        assert_eq!(live.seq, hub.after(0, 1).await.unwrap()[0].seq);
    }
}
