use std::path::Path;

use sunrise_bot::{
    config::{CommunityEnable, Config, ServerConfig},
    discord::{
        executor::{Ledger, MemoryLedger, apply},
        fake::FakeDiscord,
    },
    ids::Snowflake,
    kit::{
        live::*,
        plan::{Action, Known, ObjectKind, PlanInput, plan},
        request::Request,
    },
};

const BOT: Snowflake = Snowflake(1569822934766194688);

fn config() -> ServerConfig {
    let mut c = Config::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/config")))
        .unwrap()
        .server;
    c.guild.community.enable = CommunityEnable::Bot;
    c
}

fn fixture<T: serde::de::DeserializeOwned>(name: &str) -> T {
    serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/tests/fixtures/live/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap()
}

/// The real server with its three messages.
fn live() -> Live {
    let mut l = Live::new(BOT, fixture("guild"), fixture("roles"), fixture("channels"));
    for name in ["welcome", "rules", "roles"] {
        let id = channel(&l, name);
        l.messages.insert(id, fixture(&format!("messages/{name}")));
    }
    l
}

fn channel(l: &Live, name: &str) -> Snowflake {
    l.channels.iter().find(|c| c.name == name).unwrap().id
}

fn key(s: &str) -> sunrise_bot::config::Key {
    sunrise_bot::config::Key::new(s).unwrap()
}

/// A server with everything built and Community on, as the bot leaves it.
async fn built() -> (ServerConfig, FakeDiscord, MemoryLedger) {
    let config = config();
    let mut bare = live();
    bare.channels.clear();
    bare.messages.clear();
    bare.roles
        .retain(|r| r.name == "@everyone" || r.name == "Sunrise Team" || r.managed);
    let (fake, ledger) = (FakeDiscord::new(bare), MemoryLedger::default());
    apply(&config, &fake, &ledger, false).await.unwrap();
    (config, fake, ledger)
}

#[test]
fn the_existing_embeds_are_adopted_and_only_the_roles_one_changes() {
    let config = config();
    let live = live();
    let p = plan(&PlanInput {
        config: &config,
        live: &live,
        known: &Known::default(),
        community_denied: true,
        prune: false,
    })
    .unwrap();
    assert_eq!(p.actions.len(), 1, "{}", p.render());
    assert!(
        matches!(&p.actions[0], Action::EditMessage { key, id, .. } if key.as_str() == "role_buttons" && id.0 == 1569825096338509979)
    );
    assert!(
        p.render()
            .contains("~ message role_buttons in #roles: text and buttons differ"),
        "{}",
        p.render()
    );
    assert_eq!(
        p.adopt
            .iter()
            .filter(|a| a.kind == ObjectKind::Message)
            .count(),
        3
    );
}

#[test]
fn a_message_of_somebody_else_is_not_adopted_and_two_copies_stop_the_plan() {
    let config = config();
    let mut live = live();
    let welcome = channel(&live, "welcome");
    // the same title, written by a person
    live.messages.get_mut(&welcome).unwrap()[0].author.id = Snowflake(5);
    let p = plan(&PlanInput {
        config: &config,
        live: &live,
        known: &Known::default(),
        community_denied: true,
        prune: false,
    })
    .unwrap();
    assert!(
        p.render().contains("+ post message welcome in #welcome"),
        "{}",
        p.render()
    );

    let mut live = self::live();
    let mut copy = live.messages[&welcome][0].clone();
    copy.id = Snowflake(6);
    live.messages.get_mut(&welcome).unwrap().push(copy);
    let err = plan(&PlanInput {
        config: &config,
        live: &live,
        known: &Known::default(),
        community_denied: true,
        prune: false,
    })
    .unwrap_err();
    assert!(
        err.to_string().contains("2 copies of the message welcome"),
        "{err}"
    );
}

#[test]
fn a_channel_whose_messages_were_not_read_gets_no_message_actions() {
    let config = config();
    let mut live = live();
    live.messages.clear();
    let p = plan(&PlanInput {
        config: &config,
        live: &live,
        known: &Known::default(),
        community_denied: true,
        prune: false,
    })
    .unwrap();
    assert!(p.is_empty(), "{}", p.render());
}

#[tokio::test]
async fn the_support_guide_is_posted_with_its_tag_pinned_and_locked() {
    let (_, fake, _) = built().await;
    let live = fake.live();
    let [thread] = live.threads.as_slice() else {
        panic!("{:?}", live.threads)
    };
    assert_eq!(thread.name, "📌 How to post here: read me first");
    assert!(thread.pinned() && thread.thread_metadata.locked);
    assert_eq!(
        Some(thread.parent_id),
        Some(Some(channel(&live, "support")))
    );
    // the post carries the "read me" tag of the forum, by its id
    let forum = live.channels.iter().find(|c| c.name == "support").unwrap();
    let tag = forum
        .available_tags
        .iter()
        .find(|t| t.name == "read me")
        .unwrap()
        .id;
    let created = fake.writes().into_iter().find_map(|w| match w {
        Request::CreateThread { body, .. } => Some(body),
        _ => None,
    });
    let body = created.expect("the post was created");
    assert_eq!(body.applied_tags, [tag]);
    let text = &body.message.embeds[0];
    assert_eq!(text.title, "How to post in #support");
    assert!(
        text.description
            .starts_with("Help with Sunrise: installing, crashes, mods, performance.")
    );
    assert!(
        text.description
            .contains("```\nWhat's wrong:\nWhat I tried:"),
        "the template is copied without its bold marks"
    );
    assert!(
        text.description
            .contains("it gets the **solved** tag and closes")
    );
    assert!(body.message.allowed_mentions.parse.is_empty());
}

#[tokio::test]
async fn a_changed_guide_is_put_back_and_an_archived_one_is_opened_first() {
    let (config, fake, ledger) = built().await;
    let mut live = fake.live();
    let t = live.threads[0].clone();
    live.threads[0].thread_metadata.locked = false;
    live.threads[0].thread_metadata.archived = true;
    live.threads[0].flags = 0;
    live.messages.get_mut(&t.id).unwrap()[0].embeds[0].description =
        Some("edited by a person".into());
    let fake = FakeDiscord::new(live);
    let out = apply(&config, &fake, &ledger, false).await.unwrap();
    assert_eq!(out.writes, 3, "{:?}", fake.writes());
    let writes = fake.writes();
    assert!(
        matches!(&writes[0], Request::PatchChannel { body, .. } if body.archived == Some(false))
    );
    assert!(
        matches!(&writes[1], Request::PatchChannel { body, .. } if body.locked == Some(true) && body.flags == Some(2))
    );
    assert!(matches!(&writes[2], Request::EditMessage { message, .. } if *message == t.id));
    let after = fake.live();
    assert!(after.threads[0].pinned() && after.threads[0].thread_metadata.locked);
    assert_eq!(fake.writes().len(), 3);
    assert_eq!(
        apply(&config, &fake, &ledger, false).await.unwrap().writes,
        0
    );
}

#[tokio::test]
async fn a_deleted_message_is_posted_again_and_an_edited_one_set_back() {
    let (config, fake, ledger) = built().await;
    let mut live = fake.live();
    let (rules, welcome) = (channel(&live, "rules"), channel(&live, "welcome"));
    live.messages.get_mut(&rules).unwrap().clear();
    live.messages.get_mut(&welcome).unwrap()[0].embeds[0].description = Some("vandalised".into());
    let old_rules = ledger
        .load()
        .await
        .unwrap()
        .get(ObjectKind::Message, &key("rules"))
        .unwrap()
        .id;
    let fake = FakeDiscord::new(live);
    let out = apply(&config, &fake, &ledger, false).await.unwrap();
    assert_eq!(out.writes, 2, "{:?}", out.applied);
    let after = fake.live();
    assert_eq!(after.messages[&rules].len(), 1);
    assert_ne!(after.messages[&rules][0].id, old_rules);
    assert_eq!(
        ledger
            .load()
            .await
            .unwrap()
            .get(ObjectKind::Message, &key("rules"))
            .unwrap()
            .id,
        after.messages[&rules][0].id
    );
    assert!(
        after.messages[&welcome][0].embeds[0]
            .description
            .as_deref()
            .unwrap()
            .starts_with("Sunrise is a community fork of Horizon")
    );
}

#[tokio::test]
async fn channel_mentions_in_texts_use_the_real_ids() {
    let (_, fake, _) = built().await;
    let live = fake.live();
    let text = live.messages[&channel(&live, "welcome")][0].embeds[0]
        .description
        .clone()
        .unwrap();
    for name in ["general", "profiler", "roles"] {
        assert!(
            text.contains(&format!("<#{}>", channel(&live, name))),
            "{name}: {text}"
        );
    }
    assert!(!text.contains("{#"));
}

#[tokio::test]
async fn a_forum_post_without_its_tag_waits_for_the_next_pass_instead_of_failing() {
    // the forum exists but has no "read me" tag yet: the tag is added first, the post comes after
    let (config, fake, ledger) = built().await;
    let mut live = fake.live();
    let support = channel(&live, "support");
    live.channels
        .iter_mut()
        .find(|c| c.id == support)
        .unwrap()
        .available_tags
        .retain(|t| t.name != "read me");
    live.threads.clear();
    live.messages
        .retain(|id, _| live.channels.iter().any(|c| c.id == *id));
    let fake = FakeDiscord::new(live);
    let known_before = ledger.load().await.unwrap();
    let out = apply(&config, &fake, &ledger, false).await;
    assert!(out.is_ok(), "{:?}", out.err().map(|e| e.to_string()));
    let after = fake.live();
    assert_eq!(after.threads.len(), 1);
    assert!(
        known_before
            .get(ObjectKind::ForumPost, &key("support_howto"))
            .is_some()
    );
}
