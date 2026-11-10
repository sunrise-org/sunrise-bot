use std::path::Path;

use sunrise_bot::{
    config::{Config, Key, ServerConfig},
    ids::Snowflake,
    kit::{
        live::{Live, LiveChannel, LiveGuild, LiveKind, LiveRole},
        perms::Permissions,
        plan::{Action, Known, ObjectKind, Origin, Plan, PlanError, PlanInput, plan},
    },
};

const BOT: Snowflake = Snowflake(1569822934766194688);

fn config() -> ServerConfig {
    Config::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/config")))
        .unwrap()
        .server
}

fn fixture<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let path = format!(
        "{}/tests/fixtures/live/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The real Sunrise server as it was on 2026-11-11 (Community off), set up by hand with oneshot/setup_server.py.
fn live() -> Live {
    Live::new(
        BOT,
        fixture::<LiveGuild>("guild"),
        fixture::<Vec<LiveRole>>("roles"),
        fixture::<Vec<LiveChannel>>("channels"),
    )
}

fn run(config: &ServerConfig, live: &Live, known: &Known) -> Result<Plan, PlanError> {
    plan(&PlanInput {
        config,
        live,
        known,
        community_denied: false,
        prune: false,
    })
}

fn key(s: &str) -> Key {
    Key::new(s).unwrap()
}

fn channel_mut<'a>(live: &'a mut Live, name: &str) -> &'a mut LiveChannel {
    live.channels.iter_mut().find(|c| c.name == name).unwrap()
}

#[test]
fn the_live_server_needs_no_changes() {
    let p = run(&config(), &live(), &Known::default()).unwrap();
    assert!(p.is_empty(), "{}", p.render());
}

#[test]
fn what_exists_is_adopted_by_name_and_the_team_role_by_its_pinned_id() {
    let p = run(&config(), &live(), &Known::default()).unwrap();
    // 5 roles + 6 categories + 19 channels (the three Community ones do not exist)
    assert_eq!(p.adopt.len(), 30);
    let team = p
        .adopt
        .iter()
        .find(|a| a.kind == ObjectKind::Role && a.key == key("team"))
        .unwrap();
    assert_eq!(team.id, Snowflake(1569117406108983326));
}

#[test]
fn community_off_reports_what_waits_for_it() {
    let p = run(&config(), &live(), &Known::default()).unwrap();
    assert_eq!(p.waiting.len(), 1);
    assert_eq!(
        p.waiting[0].to_string(),
        "#announcements, #support, #ideas waiting for Community (enable it in Server Settings → Enable Community, rules channel #rules, updates channel #team)"
    );
    assert!(
        p.render()
            .contains("! #announcements, #support, #ideas waiting for Community")
    );
}

#[test]
fn community_on_creates_them_and_points_the_guild_at_the_two_channels() {
    let mut live = live();
    live.guild.features.push("COMMUNITY".into());
    let p = run(&config(), &live, &Known::default()).unwrap();
    assert!(p.waiting.is_empty());
    let created = p
        .actions
        .iter()
        .filter(|a| matches!(a, Action::CreateChannel { .. }))
        .count();
    assert_eq!(created, 3, "{}", p.render());
    assert!(
        p.render().contains("guild rules channel: none → #rules"),
        "{}",
        p.render()
    );
    // the channels and settings come after what exists, and the guild edit after the channels
    let last = p.actions.last().unwrap();
    assert!(matches!(last, Action::EditGuild(_)), "{}", p.render());
}

#[test]
fn the_bot_enables_community_when_the_file_lets_it() {
    let mut config = config();
    config.guild.community.enable = sunrise_bot::config::CommunityEnable::Bot;
    let p = run(&config, &live(), &Known::default()).unwrap();
    let kinds: Vec<&Action> = p.actions.iter().collect();
    assert!(
        matches!(kinds[0], Action::EnableCommunity),
        "{}",
        p.render()
    );
    assert_eq!(
        p.actions
            .iter()
            .filter(|a| matches!(a, Action::CreateChannel { .. }))
            .count(),
        3
    );
    assert!(p.waiting.is_empty());
    // after Discord said no, it is back to waiting
    let denied = plan(&PlanInput {
        config: &config,
        live: &live(),
        known: &Known::default(),
        community_denied: true,
        prune: false,
    })
    .unwrap();
    assert!(denied.is_empty() && denied.waiting.len() == 1);
}

#[test]
fn an_empty_guild_gets_everything_but_the_adopt_only_role_stops_it() {
    let mut live = live();
    live.channels.clear();
    live.roles.retain(|r| r.id == live.guild.id);
    let err = run(&config(), &live, &Known::default()).unwrap_err();
    assert!(matches!(err, PlanError::AdoptMissing { .. }), "{err}");
    // with the team role there, the rest is created
    let mut live2 = self::live();
    live2.channels.clear();
    live2
        .roles
        .retain(|r| r.name == "@everyone" || r.name == "Sunrise Team");
    let p = run(&config(), &live2, &Known::default()).unwrap();
    let count = |f: fn(&Action) -> bool| p.actions.iter().filter(|a| f(a)).count();
    assert_eq!(count(|a| matches!(a, Action::CreateRole { .. })), 4);
    assert_eq!(count(|a| matches!(a, Action::CreateCategory { .. })), 6);
    assert_eq!(count(|a| matches!(a, Action::CreateChannel { .. })), 19);
    assert!(p.waiting.len() == 1);
}

#[test]
fn a_channel_is_found_by_its_aka_and_renamed() {
    let mut config = config();
    let ch = config.categories[1]
        .channels
        .iter_mut()
        .find(|c| c.key.as_str() == "general")
        .unwrap();
    ch.name = "lobby".into();
    ch.aka = vec!["general".into()];
    let p = run(&config, &live(), &Known::default()).unwrap();
    let line = p.render();
    assert!(
        line.contains("#general: name \"general\" → \"lobby\""),
        "{line}"
    );
    assert_eq!(p.actions.len(), 1);
}

#[test]
fn two_copies_stop_the_plan() {
    let mut live = live();
    let mut copy = live
        .channels
        .iter()
        .find(|c| c.name == "general")
        .unwrap()
        .clone();
    copy.id = Snowflake(7);
    live.channels.push(copy);
    let err = run(&config(), &live, &Known::default()).unwrap_err();
    assert!(
        err.to_string()
            .contains("2 copies of channel #general (1569116592833564714, 7)"),
        "{err}"
    );
}

#[test]
fn a_recorded_id_wins_over_a_name() {
    let mut live = live();
    // somebody renamed #general; the store still knows it by id
    channel_mut(&mut live, "general").name = "chat".into();
    let id = channel_mut(&mut live, "chat").id;
    let mut known = Known::default();
    known.insert(ObjectKind::Channel, key("general"), id, Origin::Created);
    let p = run(&config(), &live, &known).unwrap();
    assert!(
        p.render().contains("#chat: name \"chat\" → \"general\""),
        "{}",
        p.render()
    );
    assert!(!p.render().contains("create"), "{}", p.render());
}

#[test]
fn drifted_channels_are_reconciled_and_nothing_else_is_touched() {
    let mut live = live();
    channel_mut(&mut live, "general").topic = Some("old".into());
    channel_mut(&mut live, "showcase").rate_limit_per_user = 0;
    channel_mut(&mut live, "profiler")
        .permission_overwrites
        .clear();
    live.guild.verification_level = 1;
    let p = run(&config(), &live, &Known::default()).unwrap();
    let text = p.render();
    assert!(
        text.contains("#general: topic \"old\" → \"Talk about Sunrise.\""),
        "{text}"
    );
    assert!(text.contains("#showcase: slowmode 0s → 60s"), "{text}");
    assert!(
        text.contains(
            "#profiler: permission overwrites differ: +(the bot: +[ViewChannel SendMessages"
        ),
        "{text}"
    );
    assert!(
        text.contains("guild verification level: 1 → Medium"),
        "{text}"
    );
    assert_eq!(p.actions.len(), 4, "{text}");
}

#[test]
fn roles_are_edited_except_the_adopt_only_one() {
    let mut live = live();
    for r in &mut live.roles {
        match r.name.as_str() {
            "Compat" => r.color = 0,
            "Contributor" => r.permissions = Permissions(8),
            "Sunrise Team" => {
                r.color = 1;
                r.permissions = Permissions(0);
                r.hoist = false;
            }
            _ => {}
        }
    }
    let p = run(&config(), &live, &Known::default()).unwrap();
    let text = p.render();
    assert!(
        text.contains("role Compat: colour #000000 → #3FA7D6"),
        "{text}"
    );
    assert!(
        text.contains("role Contributor: permissions -[Administrator] +[]"),
        "{text}"
    );
    assert!(!text.contains("role Sunrise Team:"), "{text}");
    assert_eq!(p.actions.len(), 2);
}

#[test]
fn order_is_fixed_only_when_wrong() {
    let mut live = live();
    // swap two categories and two channels
    for c in &mut live.channels {
        match c.name.as_str() {
            "COMMUNITY" => c.position = 3,
            "HELP" => c.position = 2,
            "mod-api" => c.position = 14,
            _ => {}
        }
    }
    let p = run(&config(), &live, &Known::default()).unwrap();
    let text = p.render();
    assert!(text.contains("category order differs"), "{text}");
    assert!(
        text.contains("channel order in DEVELOPMENT differs"),
        "{text}"
    );
    assert_eq!(p.actions.len(), 2);
}

#[test]
fn a_category_edit_never_reaches_unknown_objects() {
    let mut live = live();
    // a channel nobody knows about, in a category of the file
    let mut stray = live
        .channels
        .iter()
        .find(|c| c.name == "general")
        .unwrap()
        .clone();
    stray.id = Snowflake(99);
    stray.name = "stray".into();
    live.channels.push(stray);
    let p = run(&config(), &live, &Known::default()).unwrap();
    assert!(p.is_empty(), "{}", p.render());
    assert!(!p.render().contains("stray"));
}

#[test]
fn a_channel_that_left_the_file_is_reported_and_pruned_only_if_the_bot_made_it() {
    let mut live = live();
    let old = {
        let mut c = live
            .channels
            .iter()
            .find(|c| c.name == "general")
            .unwrap()
            .clone();
        c.id = Snowflake(50);
        c.name = "old".into();
        c
    };
    let mut theirs = old.clone();
    theirs.id = Snowflake(51);
    theirs.name = "theirs".into();
    live.channels.extend([old, theirs]);
    let mut known = Known::default();
    known.insert(
        ObjectKind::Channel,
        key("old"),
        Snowflake(50),
        Origin::Created,
    );
    known.insert(
        ObjectKind::Channel,
        key("theirs"),
        Snowflake(51),
        Origin::Adopted,
    );
    let config = config();
    let report = plan(&PlanInput {
        config: &config,
        live: &live,
        known: &known,
        community_denied: false,
        prune: false,
    })
    .unwrap();
    assert!(report.is_empty());
    assert_eq!(report.orphans.len(), 2);
    assert!(
        report
            .render()
            .contains("is not in the file anymore: delete it by hand")
    );
    let pruned = plan(&PlanInput {
        config: &config,
        live: &live,
        known: &known,
        community_denied: false,
        prune: true,
    })
    .unwrap();
    assert_eq!(
        pruned.actions,
        [Action::DeleteChannel {
            kind: ObjectKind::Channel,
            key: key("old"),
            id: Snowflake(50)
        }]
    );
}

#[test]
fn forum_tags_keep_their_ids() {
    let mut live = live();
    live.guild.features.push("COMMUNITY".into());
    // a support forum with the tags in another order and one missing
    let mut forum: LiveChannel = live
        .channels
        .iter()
        .find(|c| c.name == "general")
        .unwrap()
        .clone();
    forum.id = Snowflake(300);
    forum.name = "support".into();
    forum.kind = LiveKind::Forum;
    forum.parent_id = live
        .channels
        .iter()
        .find(|c| c.name == "HELP")
        .map(|c| c.id);
    forum.topic = config().channel("support").unwrap().topic.clone();
    forum.permission_overwrites = live
        .channels
        .iter()
        .find(|c| c.name == "profiler")
        .unwrap()
        .permission_overwrites
        .clone();
    forum.flags = 16;
    forum.available_tags = serde_json::from_str(
        r#"[{"id":"11","name":"crash","emoji_name":"💥","moderated":false},{"id":"12","name":"install","emoji_name":"📦","moderated":false}]"#,
    )
    .unwrap();
    live.channels.push(forum);
    let p = run(&config(), &live, &Known::default()).unwrap();
    let Some(Action::EditChannel { edit, .. }) = p
        .actions
        .iter()
        .find(|a| matches!(a, Action::EditChannel { key, .. } if key.as_str() == "support"))
    else {
        panic!("{}", p.render())
    };
    let tags = edit.tags.as_ref().unwrap();
    assert_eq!(
        tags.iter()
            .map(|t| (t.name.as_str(), t.id))
            .collect::<Vec<_>>(),
        [
            ("install", Some(Snowflake(12))),
            ("crash", Some(Snowflake(11))),
            ("mods", None),
            ("performance", None),
            ("solved", None),
            ("read me", None),
        ]
    );
    assert!(tags[4].moderated);
}
