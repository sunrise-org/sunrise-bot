use std::path::Path;

use sunrise_bot::{
    config::{CommunityEnable, Config, ServerConfig},
    discord::{
        ApiError,
        executor::{ApplyError, Ledger, MemoryLedger, apply},
        fake::FakeDiscord,
    },
    ids::Snowflake,
    kit::{
        live::{Live, LiveChannel, LiveGuild, LiveKind, LiveRole},
        plan::{ObjectKind, Origin, PlanInput, plan},
        request::Request,
    },
};

const BOT: Snowflake = Snowflake(1569822934766194688);

fn config(enable: CommunityEnable) -> ServerConfig {
    let mut c = Config::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/config")))
        .unwrap()
        .server;
    c.guild.community.enable = enable;
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

/// The real server (Community off), 2026-11-11.
fn live() -> Live {
    Live {
        bot_id: BOT,
        guild: fixture::<LiveGuild>("guild"),
        roles: fixture::<Vec<LiveRole>>("roles"),
        channels: fixture::<Vec<LiveChannel>>("channels"),
    }
}

/// A server with the team role and nothing else.
fn bare() -> Live {
    let mut l = live();
    l.channels.clear();
    l.roles
        .retain(|r| r.name == "@everyone" || r.name == "Sunrise Team" || r.managed);
    l.guild.verification_level = 0;
    l.guild.explicit_content_filter = 0;
    l.guild.default_message_notifications = 0;
    l
}

async fn replan(
    config: &ServerConfig,
    fake: &FakeDiscord,
    ledger: &MemoryLedger,
) -> sunrise_bot::kit::plan::Plan {
    let known = ledger.load().await.unwrap();
    plan(&PlanInput {
        config,
        live: &fake.live(),
        known: &known,
        community_denied: false,
        prune: false,
    })
    .unwrap()
}

fn count(live: &Live, name: &str) -> usize {
    live.channels.iter().filter(|c| c.name == name).count()
}

#[tokio::test]
async fn an_empty_server_is_built_and_the_second_run_writes_nothing() {
    let config = config(CommunityEnable::Bot);
    let (fake, ledger) = (FakeDiscord::new(bare()), MemoryLedger::default());
    let first = apply(&config, &fake, &ledger, false).await.unwrap();
    assert!(first.writes > 25, "{first:?}");
    assert!(first.waiting.is_empty() && !first.community_denied);
    let after = fake.live();
    assert!(after.community());
    assert_eq!(after.channels.len(), 6 + 22);
    for name in ["announcements", "support", "ideas", "mod-log", "Dev call"] {
        assert_eq!(count(&after, name), 1, "{name}");
    }
    assert_eq!(
        after
            .roles
            .iter()
            .filter(|r| ["Contributor", "Modding", "Compat", "Site"].contains(&r.name.as_str()))
            .count(),
        4
    );
    assert_eq!(
        after.guild.rules_channel_id,
        after
            .channels
            .iter()
            .find(|c| c.name == "rules")
            .map(|c| c.id)
    );
    assert_eq!(after.guild.verification_level, 2);
    // everything the bot made is recorded as created, the team role as found
    let known = ledger.load().await.unwrap();
    assert_eq!(
        known
            .iter()
            .filter(|(_, _, o)| o.origin == Origin::Created)
            .count(),
        4 + 6 + 22
    );
    assert_eq!(
        known
            .iter()
            .filter(|(_, _, o)| o.origin == Origin::Adopted)
            .count(),
        1
    );
    assert!(replan(&config, &fake, &ledger).await.is_empty());

    let writes_before = fake.writes().len();
    let second = apply(&config, &fake, &ledger, false).await.unwrap();
    assert_eq!(second.writes, 0);
    assert_eq!(
        fake.writes().len(),
        writes_before,
        "the second run made a write call"
    );
}

#[tokio::test]
async fn the_live_server_with_community_off_is_recorded_and_not_written_to() {
    let config = config(CommunityEnable::Manual);
    let (fake, ledger) = (FakeDiscord::new(live()), MemoryLedger::default());
    let out = apply(&config, &fake, &ledger, false).await.unwrap();
    assert_eq!(out.writes, 0);
    assert!(fake.writes().is_empty());
    assert_eq!(out.adopted, 30);
    assert_eq!(out.waiting.len(), 1);
    assert_eq!(ledger.load().await.unwrap().iter().count(), 30);
    // the second run adopts nothing more
    assert_eq!(
        apply(&config, &fake, &ledger, false).await.unwrap().adopted,
        0
    );
}

#[tokio::test]
async fn community_turned_on_by_hand_is_picked_up_on_the_next_run() {
    let config = config(CommunityEnable::Manual);
    let (fake, ledger) = (FakeDiscord::new(live()), MemoryLedger::default());
    apply(&config, &fake, &ledger, false).await.unwrap();
    fake.enable_community_by_hand();
    let out = apply(&config, &fake, &ledger, false).await.unwrap();
    assert!(out.waiting.is_empty());
    let after = fake.live();
    for name in ["announcements", "support", "ideas"] {
        assert_eq!(count(&after, name), 1, "{name}");
    }
    assert_eq!(
        after.guild.rules_channel_id,
        after
            .channels
            .iter()
            .find(|c| c.name == "rules")
            .map(|c| c.id)
    );
    assert_eq!(
        after.guild.public_updates_channel_id,
        after
            .channels
            .iter()
            .find(|c| c.name == "team" && c.kind == LiveKind::Text)
            .map(|c| c.id)
    );
    let before = fake.writes().len();
    assert_eq!(
        apply(&config, &fake, &ledger, false).await.unwrap().writes,
        0
    );
    assert_eq!(fake.writes().len(), before);
}

#[tokio::test]
async fn a_refused_community_leaves_the_rest_built_and_the_forums_waiting() {
    let config = config(CommunityEnable::Bot);
    let (fake, ledger) = (
        FakeDiscord::new(bare()).community_allowed(false),
        MemoryLedger::default(),
    );
    let out = apply(&config, &fake, &ledger, false).await.unwrap();
    assert!(out.community_denied);
    assert_eq!(out.waiting.len(), 1);
    let after = fake.live();
    assert!(!after.community());
    assert_eq!(after.channels.len(), 6 + 19);
    assert_eq!(count(&after, "support"), 0);
    // the one refused call is the only write a rerun tries
    let before = fake.writes().len();
    apply(&config, &fake, &ledger, false).await.unwrap();
    let again: Vec<Request> = fake.writes().split_off(before);
    assert_eq!(again.len(), 1);
    assert!(matches!(&again[0], Request::PatchGuild(p) if p.features.is_some()));
}

#[tokio::test]
async fn a_crash_midway_makes_no_second_copy_even_if_the_ledger_is_lost() {
    let config = config(CommunityEnable::Bot);
    let (fake, kept) = (
        FakeDiscord::new(bare()).fail_from_write(9),
        MemoryLedger::default(),
    );
    let err = apply(&config, &fake, &kept, false).await.unwrap_err();
    assert!(
        matches!(
            err,
            ApplyError::Api {
                source: ApiError::Status { status: 500, .. },
                ..
            }
        ),
        "{err}"
    );
    let half = fake.live();
    assert!(half.channels.len() < 28 && !half.channels.is_empty());

    // run again on what was left: with the ledger that kept the ids, then with a fresh one
    for ledger in [kept, MemoryLedger::default()] {
        let fake = FakeDiscord::new(half.clone());
        apply(&config, &fake, &ledger, false).await.unwrap();
        let done = fake.live();
        assert_eq!(done.channels.len(), 6 + 22);
        for c in &done.channels {
            let twins = done
                .channels
                .iter()
                .filter(|o| o.name == c.name && o.parent_id == c.parent_id)
                .count();
            assert_eq!(twins, 1, "{} was made twice", c.name);
        }
        assert_eq!(
            done.roles
                .iter()
                .filter(|r| r.name == "Contributor")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn a_recorded_id_of_something_deleted_by_hand_is_not_used() {
    let config = config(CommunityEnable::Bot);
    let (fake, ledger) = (FakeDiscord::new(bare()), MemoryLedger::default());
    apply(&config, &fake, &ledger, false).await.unwrap();
    // somebody deletes the HELP category and its channel; the ledger still holds their ids
    let mut after = fake.live();
    let help = after.channels.iter().find(|c| c.name == "HELP").unwrap().id;
    after
        .channels
        .retain(|c| c.id != help && c.parent_id != Some(help));
    let fake = FakeDiscord::new(after);
    apply(&config, &fake, &ledger, false).await.unwrap();
    let done = fake.live();
    assert_eq!(count(&done, "HELP"), 1);
    assert_eq!(count(&done, "profiler"), 1);
    assert_eq!(
        done.channels
            .iter()
            .find(|c| c.name == "profiler")
            .unwrap()
            .parent_id,
        done.channels
            .iter()
            .find(|c| c.name == "HELP")
            .map(|c| c.id)
    );
}

#[tokio::test]
async fn drift_is_fixed_and_unknown_objects_are_left_alone() {
    let config = config(CommunityEnable::Manual);
    let mut l = live();
    l.channels
        .iter_mut()
        .find(|c| c.name == "general")
        .unwrap()
        .topic = Some("old".into());
    l.channels
        .iter_mut()
        .find(|c| c.name == "profiler")
        .unwrap()
        .permission_overwrites
        .clear();
    l.channels
        .iter_mut()
        .find(|c| c.name == "showcase")
        .unwrap()
        .rate_limit_per_user = 5;
    l.roles.iter_mut().find(|r| r.name == "Site").unwrap().color = 1;
    // somebody else's channel in one of our categories, and a role we never heard of
    let mut stray = l
        .channels
        .iter()
        .find(|c| c.name == "general")
        .unwrap()
        .clone();
    stray.id = Snowflake(77);
    stray.name = "members-only".into();
    stray.topic = Some("not ours".into());
    l.channels.push(stray.clone());
    let mut role = l.roles.iter().find(|r| r.name == "Site").unwrap().clone();
    role.id = Snowflake(78);
    role.name = "Booster".into();
    l.roles.push(role);
    let (fake, ledger) = (FakeDiscord::new(l), MemoryLedger::default());
    let out = apply(&config, &fake, &ledger, false).await.unwrap();
    assert_eq!(out.writes, 4, "{:?}", out.applied);
    let after = fake.live();
    assert_eq!(
        after
            .channels
            .iter()
            .find(|c| c.name == "general")
            .unwrap()
            .topic
            .as_deref(),
        Some("Talk about Sunrise.")
    );
    assert_eq!(
        after
            .channels
            .iter()
            .find(|c| c.name == "showcase")
            .unwrap()
            .rate_limit_per_user,
        60
    );
    assert_eq!(
        after.roles.iter().find(|r| r.name == "Site").unwrap().color,
        0xE05C7A
    );
    let kept = after
        .channels
        .iter()
        .find(|c| c.id == Snowflake(77))
        .unwrap();
    assert_eq!(kept.topic.as_deref(), Some("not ours"));
    assert!(
        after
            .roles
            .iter()
            .any(|r| r.id == Snowflake(78) && r.name == "Booster")
    );
    let team = after
        .roles
        .iter()
        .find(|r| r.name == "Sunrise Team")
        .unwrap();
    assert_eq!((team.permissions.0, team.color), (8, 0x2ecc71));
}

#[tokio::test]
async fn prune_deletes_only_what_the_bot_made() {
    let config = config(CommunityEnable::Manual);
    let mut l = live();
    for (id, name) in [(60, "made"), (61, "found")] {
        let mut c = l
            .channels
            .iter()
            .find(|c| c.name == "general")
            .unwrap()
            .clone();
        c.id = Snowflake(id);
        c.name = name.into();
        l.channels.push(c);
    }
    let (fake, ledger) = (FakeDiscord::new(l), MemoryLedger::default());
    for (key, id, origin) in [
        ("made", 60, Origin::Created),
        ("found", 61, Origin::Adopted),
    ] {
        ledger
            .record(
                ObjectKind::Channel,
                &sunrise_bot::config::Key::new(key).unwrap(),
                Snowflake(id),
                origin,
            )
            .await
            .unwrap();
    }
    let kept = apply(&config, &fake, &ledger, false).await.unwrap();
    assert_eq!(kept.writes, 0);
    assert_eq!(count(&fake.live(), "made"), 1);
    apply(&config, &fake, &ledger, true).await.unwrap();
    assert_eq!(count(&fake.live(), "made"), 0);
    assert_eq!(count(&fake.live(), "found"), 1);
    assert!(
        ledger
            .load()
            .await
            .unwrap()
            .get(
                ObjectKind::Channel,
                &sunrise_bot::config::Key::new("made").unwrap()
            )
            .is_none()
    );
}

#[tokio::test]
async fn the_forum_is_made_the_way_discord_wants_it() {
    let config = config(CommunityEnable::Bot);
    let (fake, ledger) = (FakeDiscord::new(bare()), MemoryLedger::default());
    apply(&config, &fake, &ledger, false).await.unwrap();
    let body = fake.writes().into_iter().find_map(|w| match w {
        Request::CreateChannel(b) if b.name == "support" => Some(b),
        _ => None,
    });
    let b = body.expect("support was created");
    assert_eq!(
        (
            b.kind,
            b.flags,
            b.default_sort_order,
            b.default_forum_layout
        ),
        (15, Some(16), Some(0), Some(1))
    );
    let tags = b.available_tags.clone().unwrap();
    assert_eq!(
        tags.iter()
            .map(|t| (t.name.as_str(), t.emoji_name.as_str(), t.moderated))
            .collect::<Vec<_>>(),
        [
            ("install", "📦", false),
            ("crash", "💥", false),
            ("mods", "🧩", false),
            ("performance", "⚡", false),
            ("solved", "✅", true),
        ]
    );
    assert_eq!(serde_json::to_value(&b).unwrap()["type"], 15);
}
