use std::path::Path;

use serde_json::{Value, json};
use sunrise_bot::config::{BotConfig, Config, ConfigError, ServerConfig};

fn config_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/config"))
}

fn server_json() -> Value {
    serde_json::from_str(&std::fs::read_to_string(config_dir().join("server.json")).unwrap())
        .unwrap()
}

fn bot() -> BotConfig {
    toml::from_str(&std::fs::read_to_string(config_dir().join("bot.toml")).unwrap()).unwrap()
}

/// What `validate` says about `server.json` after `edit`.
fn problems_after(edit: impl FnOnce(&mut Value)) -> Vec<String> {
    let mut v = server_json();
    edit(&mut v);
    let server: ServerConfig = serde_json::from_value(v).expect("the edit keeps the file readable");
    match (Config { server, bot: bot() }).validate() {
        Ok(()) => Vec::new(),
        Err(ConfigError::Invalid(p)) => p,
        Err(e) => panic!("{e}"),
    }
}

fn says(problems: &[String], needle: &str) -> bool {
    problems.iter().any(|p| p.contains(needle))
}

#[test]
fn the_shipped_config_is_valid() {
    let c = Config::load(config_dir()).unwrap();
    assert_eq!(c.server.categories.len(), 6);
    assert_eq!(c.server.channels().count(), 22);
    assert_eq!(c.server.roles.len(), 5);
}

#[test]
fn unknown_fields_are_refused() {
    let mut v = server_json();
    v["guild"]["colour"] = json!("red");
    assert!(
        serde_json::from_value::<ServerConfig>(v)
            .unwrap_err()
            .to_string()
            .contains("unknown field `colour`")
    );
    let mut v = server_json();
    v["messages"]["welcome"]["embed"]["footer"] = json!("x");
    assert!(serde_json::from_value::<ServerConfig>(v).is_err());
    assert!(
        toml::from_str::<BotConfig>("[help]\ntitle = \"t\"\ntext = \"x\"\nextra = 1\n").is_err()
    );
}

#[test]
fn bad_enum_values_and_keys_are_refused() {
    let mut v = server_json();
    v["categories"][0]["access"] = json!("everyone");
    assert!(serde_json::from_value::<ServerConfig>(v).is_err());
    let mut v = server_json();
    v["roles"][1]["key"] = json!("Contributor");
    assert!(serde_json::from_value::<ServerConfig>(v).is_err());
    let mut v = server_json();
    v["roles"][1]["permissions"] = json!(["Fly"]);
    assert!(serde_json::from_value::<ServerConfig>(v).is_err());
}

#[test]
fn duplicate_keys_are_reported() {
    let p = problems_after(|v| v["categories"][1]["channels"][0]["key"] = json!("welcome"));
    assert!(says(&p, "channel key \"welcome\" is used twice"), "{p:?}");
    let p = problems_after(|v| v["roles"][2]["key"] = json!("compat"));
    assert!(says(&p, "role key \"compat\" is used twice"), "{p:?}");
}

#[test]
fn dangling_references_are_reported() {
    let p = problems_after(|v| v["guild"]["community"]["rules_channel"] = json!("nowhere"));
    assert!(says(&p, "no channel with key nowhere"), "{p:?}");
    let p = problems_after(|v| {
        v["role_order"] = json!(["team", "contributor", "modding", "compat", "ghost"])
    });
    assert!(
        says(&p, "no role with key ghost") && says(&p, "role site is missing"),
        "{p:?}"
    );
    let p = problems_after(|v| {
        v["categories"][0]["channels"][0]["messages"] = json!(["welcome", "nothing"])
    });
    assert!(says(&p, "no message with key nothing"), "{p:?}");
    let p =
        problems_after(|v| v["messages"]["welcome"]["embed"]["description"] = json!("see {#void}"));
    assert!(says(&p, "{#void} names no channel"), "{p:?}");
}

#[test]
fn only_self_assign_roles_get_buttons() {
    let p = problems_after(|v| {
        v["messages"]["role_buttons"]["role_buttons"] = json!(["modding", "contributor"])
    });
    assert!(says(&p, "role contributor is not self_assign"), "{p:?}");
    let p = problems_after(|v| v["roles"][1]["self_assign"] = json!(true));
    assert!(p.is_empty(), "{p:?}");
}

#[test]
fn adopt_only_roles_carry_nothing_to_edit() {
    let p = problems_after(|v| v["roles"][0]["permissions"] = json!(["Administrator"]));
    assert!(says(&p, "adopt_only roles are never edited"), "{p:?}");
}

#[test]
fn forum_rules_are_checked() {
    let tags: Vec<Value> = (0..21)
        .map(|i| json!({ "emoji": "🧩", "name": format!("t{i}") }))
        .collect();
    let p = problems_after(|v| v["categories"][3]["channels"][5]["tags"] = Value::Array(tags));
    assert!(says(&p, "21 tags, Discord allows 20"), "{p:?}");
    let p = problems_after(|v| v["categories"][2]["channels"][0]["done_tag"] = json!("fixed"));
    assert!(
        says(&p, "done_tag \"fixed\" is not one of its tags"),
        "{p:?}"
    );
    let p = problems_after(|v| {
        v["categories"][2]["channels"][0]["template"]["needs"][0]["regex"] = json!("(unclosed")
    });
    assert!(says(&p, "regex does not compile"), "{p:?}");
    let p = problems_after(|v| {
        v["categories"][2]["channels"][1]["tags"] = json!([{ "emoji": "🧩", "name": "x" }])
    });
    assert!(says(&p, "tags belong to forum channels"), "{p:?}");
}

#[test]
fn names_and_limits_are_checked() {
    let p = problems_after(|v| v["categories"][1]["channels"][0]["name"] = json!("General Chat"));
    assert!(says(&p, "name \"General Chat\" is not valid"), "{p:?}");
    let p =
        problems_after(|v| v["categories"][1]["channels"][1]["slowmode_seconds"] = json!(30_000));
    assert!(says(&p, "slowmode_seconds"), "{p:?}");
    let p = problems_after(|v| v["categories"][4]["channels"][0]["topic"] = json!("voice"));
    assert!(says(&p, "a voice channel has no topic"), "{p:?}");
}

#[test]
fn messages_must_sit_in_a_fitting_channel() {
    let p = problems_after(|v| {
        v["categories"][0]["channels"][1]["messages"] = json!(["rules", "support_howto"])
    });
    assert!(
        says(
            &p,
            "message support_howto is a forum post, but this is not a forum"
        ),
        "{p:?}"
    );
    let p = problems_after(|v| v["categories"][0]["channels"][0]["messages"] = json!([]));
    assert!(
        says(&p, "message welcome is not placed in any channel"),
        "{p:?}"
    );
}

#[test]
fn bot_toml_texts_must_name_real_channels() {
    let mut b = bot();
    b.help.text = "go to {#lobby}".into();
    let server: ServerConfig = serde_json::from_value(server_json()).unwrap();
    let Err(ConfigError::Invalid(p)) = (Config { server, bot: b }).validate() else {
        panic!("expected a problem")
    };
    assert!(says(&p, "{#lobby} names no channel"), "{p:?}");
}
