use std::{path::Path, sync::Mutex};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use sunrise_bot::{
    bot::roles::{RoleEditor, respond},
    config::{Config, Key},
    http::{Health, router},
    ids::Snowflake,
    kit::{
        info,
        plan::{Known, ObjectKind, Origin},
        roles::{Click, click},
    },
    store::Store,
};
use tower::ServiceExt;

fn config() -> Config {
    Config::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/config"))).unwrap()
}

fn key(s: &str) -> Key {
    Key::new(s).unwrap()
}

/// The roles and channels of the live server, as the ledger holds them after bootstrap.
fn known() -> Known {
    let mut k = Known::default();
    for (kind, name, id) in [
        (ObjectKind::Role, "team", 1569117406108983326),
        (ObjectKind::Role, "contributor", 1569824695740793064),
        (ObjectKind::Role, "modding", 1569824697036709918),
        (ObjectKind::Role, "compat", 1569824698383208639),
        (ObjectKind::Role, "site", 1569824700299870369),
        (ObjectKind::Channel, "general", 1569116592833564714),
        (ObjectKind::Channel, "roles", 1569824804012425218),
    ] {
        k.insert(kind, key(name), Snowflake(id), Origin::Adopted);
    }
    k
}

const MODDING: Snowflake = Snowflake(1569824697036709918);

#[test]
fn a_click_toggles_only_self_assign_roles_the_bot_knows() {
    let c = config().server;
    let k = known();
    assert_eq!(
        click(&c, &k, "role:modding", &[]),
        Click::Toggle {
            role: MODDING,
            name: "Modding".into(),
            has: false
        }
    );
    assert_eq!(
        click(&c, &k, "role:modding", &[Snowflake(1), MODDING]),
        Click::Toggle {
            role: MODDING,
            name: "Modding".into(),
            has: true
        }
    );
    // the team and contributor roles are known to the bot but are not for members to take
    for forged in [
        "role:team",
        "role:contributor",
        "role:nonsense",
        "role:",
        "role:Modding",
        "role:../team",
    ] {
        assert_eq!(click(&c, &k, forged, &[]), Click::Gone, "{forged}");
    }
    assert_eq!(click(&c, &k, "other:thing", &[]), Click::NotOurs);
    // a self-assign role the bot has no id for (deleted, never made)
    assert_eq!(click(&c, &Known::default(), "role:site", &[]), Click::Gone);
}

#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<(&'static str, u64)>>,
    fail: bool,
}

impl RoleEditor for Recorder {
    async fn add(&self, role: Snowflake) -> Result<(), String> {
        self.calls.lock().unwrap().push(("add", role.0));
        if self.fail {
            Err("Missing Permissions".into())
        } else {
            Ok(())
        }
    }

    async fn remove(&self, role: Snowflake) -> Result<(), String> {
        self.calls.lock().unwrap().push(("remove", role.0));
        Ok(())
    }
}

#[tokio::test]
async fn clicking_adds_then_removes_and_answers_privately() {
    let (c, k, rec) = (config().server, known(), Recorder::default());
    let added = respond(&c, &k, "role:modding", &[], &rec).await;
    assert_eq!(added.as_deref(), Some("Added **Modding**."));
    let removed = respond(&c, &k, "role:modding", &[MODDING], &rec).await;
    assert_eq!(removed.as_deref(), Some("Removed **Modding**."));
    assert_eq!(
        *rec.calls.lock().unwrap(),
        [("add", MODDING.0), ("remove", MODDING.0)]
    );
}

#[tokio::test]
async fn a_button_for_a_role_members_may_not_take_changes_nothing() {
    let (c, k, rec) = (config().server, known(), Recorder::default());
    assert_eq!(
        respond(&c, &k, "role:team", &[], &rec).await.as_deref(),
        Some("That role is gone.")
    );
    assert_eq!(respond(&c, &k, "something:else", &[], &rec).await, None);
    assert!(rec.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_refusal_from_discord_is_told_kindly() {
    let (c, k) = (config().server, known());
    let rec = Recorder {
        fail: true,
        ..Recorder::default()
    };
    let text = respond(&c, &k, "role:site", &[], &rec).await.unwrap();
    assert_eq!(text, "I could not change your roles. Ask a team member.");
}

#[test]
fn help_mentions_channels_by_id_and_links_lists_every_link() {
    let cfg = config();
    let k = known();
    let ids = |name: &str| {
        sunrise_bot::config::Key::new(name)
            .ok()
            .and_then(|key| k.get(ObjectKind::Channel, &key))
            .map(|o| o.id)
    };
    let card = info::help(&cfg.bot, ids);
    assert_eq!(card.title, "👋 I'm Sunny, Sunrise's bot");
    assert!(
        card.description.contains("<#1569824804012425218>"),
        "{}",
        card.description
    );
    assert!(
        card.description.contains("#welcome"),
        "a channel with no id yet reads as its name"
    );
    assert!(!card.description.contains("{#"));
    let links = info::links(&cfg.bot);
    assert_eq!(links.description.lines().count(), 8);
    assert!(
        links
            .description
            .starts_with("🌐 Website: https://playsunrise.net")
    );
    assert!(
        links
            .description
            .contains("📊 Profiler: https://playsunrise.net/perf")
    );
}

async fn get(app: &axum::Router, path: &str) -> StatusCode {
    app.clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn the_probes_say_alive_at_once_and_ready_when_the_gateway_is_up() {
    let health = Health::new(Store::memory().await.unwrap());
    let app = router(health.clone());
    assert_eq!(get(&app, "/healthz").await, StatusCode::OK);
    assert_eq!(get(&app, "/readyz").await, StatusCode::SERVICE_UNAVAILABLE);
    health.set_gateway(true);
    assert_eq!(get(&app, "/readyz").await, StatusCode::OK);
    health.set_gateway(false);
    assert_eq!(get(&app, "/readyz").await, StatusCode::SERVICE_UNAVAILABLE);
    for other in ["/", "/github/webhook", "/admin"] {
        assert_eq!(get(&app, other).await, StatusCode::NOT_FOUND, "{other}");
    }
}
