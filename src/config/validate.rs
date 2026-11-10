use std::collections::BTreeSet;

use regex::Regex;

use super::*;

const TOPIC_MAX: usize = 1024;
const FORUM_TOPIC_MAX: usize = 4096;
const FORUM_TAGS_MAX: usize = 20;
const SLOWMODE_MAX: u32 = 21_600;

/// Everything wrong with the two files, one line each; empty when they are fine.
pub fn problems(server: &ServerConfig, bot: &BotConfig) -> Vec<String> {
    let mut p = Vec::new();
    if server.version != 1 {
        p.push(format!(
            "server.json: version {} is not supported (this bot reads version 1)",
            server.version
        ));
    }
    guild(server, &mut p);
    roles(server, &mut p);
    categories(server, &mut p);
    messages(server, &mut p);
    bot_texts(server, bot, &mut p);
    p
}

fn duplicates<'a>(what: &str, keys: impl Iterator<Item = &'a str>, p: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    for k in keys {
        if !seen.insert(k) {
            p.push(format!("{what} key {k:?} is used twice"));
        }
    }
}

fn guild(s: &ServerConfig, p: &mut Vec<String>) {
    let c = &s.guild.community;
    for (what, key) in [
        ("rules_channel", &c.rules_channel),
        ("updates_channel", &c.updates_channel),
    ] {
        match s.channel(key.as_str()) {
            None => p.push(format!("guild.community.{what}: no channel with key {key}")),
            Some(ch) if ch.kind != ChannelKind::Text => p.push(format!(
                "guild.community.{what}: #{} must be a text channel",
                ch.name
            )),
            Some(_) => {}
        }
    }
    if s.guild.name.is_empty() || s.guild.name.chars().count() > 100 {
        p.push("guild.name must be 1 to 100 characters".into());
    }
}

fn roles(s: &ServerConfig, p: &mut Vec<String>) {
    duplicates("role", s.roles.iter().map(|r| r.key.as_str()), p);
    duplicates("role name", s.roles.iter().map(|r| r.name.as_str()), p);
    for r in &s.roles {
        if r.name.is_empty() || r.name.chars().count() > 100 {
            p.push(format!("role {}: name must be 1 to 100 characters", r.key));
        }
        match r.manage {
            RoleManage::AdoptOnly => {
                if r.color.is_some() || r.hoist || r.mentionable || !r.permissions.is_empty() {
                    p.push(format!("role {}: adopt_only roles are never edited, so they carry no colour, hoist, mentionable or permissions", r.key));
                }
            }
            RoleManage::Full => {
                if r.color.is_none() {
                    p.push(format!("role {}: colour is required", r.key));
                }
            }
        }
        if r.self_assign && r.manage == RoleManage::AdoptOnly {
            p.push(format!(
                "role {}: an adopt_only role cannot be self-assigned",
                r.key
            ));
        }
        if r.self_assign && !r.permissions.is_empty() {
            p.push(format!(
                "role {}: a self-assignable role must carry no permissions",
                r.key
            ));
        }
    }
    let listed: Vec<&str> = s.role_order.iter().map(Key::as_str).collect();
    duplicates("role_order", listed.iter().copied(), p);
    for k in &listed {
        if s.role(k).is_none() {
            p.push(format!("role_order: no role with key {k}"));
        }
    }
    for r in &s.roles {
        if !listed.contains(&r.key.as_str()) {
            p.push(format!("role_order: role {} is missing", r.key));
        }
    }
    for (name, id) in &s.team_members {
        if id.0 == 0 {
            p.push(format!("team_members.{name}: not a Discord id"));
        }
    }
}

fn channel_name_ok(c: &ChannelSpec) -> bool {
    let len = c.name.chars().count();
    if !(1..=100).contains(&len) {
        return false;
    }
    match c.kind {
        ChannelKind::Voice => true,
        _ => c
            .name
            .chars()
            .all(|ch| ch.is_alphanumeric() && !ch.is_uppercase() || ch == '-' || ch == '_'),
    }
}

fn categories(s: &ServerConfig, p: &mut Vec<String>) {
    duplicates("category", s.categories.iter().map(|c| c.key.as_str()), p);
    duplicates("channel", s.channels().map(|(_, c)| c.key.as_str()), p);
    for cat in &s.categories {
        if cat.name.is_empty() || cat.name.chars().count() > 100 {
            p.push(format!(
                "category {}: name must be 1 to 100 characters",
                cat.key
            ));
        }
        if cat.access == Access::ContributorsConnect {
            p.push(format!(
                "category {}: contributors_connect is for voice channels",
                cat.key
            ));
        }
        for ch in &cat.channels {
            channel(s, cat, ch, p);
        }
    }
}

fn channel(s: &ServerConfig, cat: &CategorySpec, ch: &ChannelSpec, p: &mut Vec<String>) {
    let at = format!("channel {}", ch.key);
    if !channel_name_ok(ch) {
        p.push(format!("{at}: name {:?} is not valid (lowercase letters, digits, - and _ for text channels, up to 100)", ch.name));
    }
    let topic_max = if ch.kind == ChannelKind::Forum {
        FORUM_TOPIC_MAX
    } else {
        TOPIC_MAX
    };
    if ch
        .topic
        .as_ref()
        .is_some_and(|t| t.chars().count() > topic_max)
    {
        p.push(format!("{at}: topic is longer than {topic_max} characters"));
    }
    if ch.kind == ChannelKind::Voice
        && (ch.topic.is_some() || ch.slowmode_seconds > 0 || !ch.messages.is_empty())
    {
        p.push(format!(
            "{at}: a voice channel has no topic, slowmode or messages"
        ));
    }
    if ch.slowmode_seconds > SLOWMODE_MAX {
        p.push(format!("{at}: slowmode_seconds is above {SLOWMODE_MAX}"));
    }
    let access = ch.access.unwrap_or(cat.access);
    if access == Access::ContributorsConnect && ch.kind != ChannelKind::Voice {
        p.push(format!(
            "{at}: contributors_connect goes with voice channels only"
        ));
    }
    if ch.visible_to.is_some() && access != Access::BotFeed {
        p.push(format!("{at}: visible_to goes with access bot_feed"));
    }
    if ch.kind != ChannelKind::Forum && !ch.forum_fields().is_empty() {
        p.push(format!(
            "{at}: {} belong to forum channels",
            ch.forum_fields().join(", ")
        ));
    }
    if ch.kind == ChannelKind::Forum {
        forum(ch, &at, p);
    }
    for m in &ch.messages {
        match s.messages.get(m) {
            None => p.push(format!("{at}: no message with key {m}")),
            Some(MessageSpec::ForumPost { .. }) if ch.kind != ChannelKind::Forum => p.push(
                format!("{at}: message {m} is a forum post, but this is not a forum"),
            ),
            Some(MessageSpec::Embed { .. })
                if ch.kind == ChannelKind::Forum || ch.kind == ChannelKind::Voice =>
            {
                p.push(format!(
                    "{at}: message {m} is an embed message, which a {:?} channel cannot hold",
                    ch.kind
                ));
            }
            Some(MessageSpec::ForumPost {
                body_from: BodyFrom::Template,
                ..
            }) if ch.template.is_none() => p.push(format!(
                "{at}: message {m} is built from the template, which this forum has not"
            )),
            Some(MessageSpec::ForumPost { tag: Some(tag), .. }) if ch.tag(tag).is_none() => p.push(
                format!("{at}: message {m} carries tag {tag:?}, which the forum does not have"),
            ),
            Some(MessageSpec::ForumPost { tag: None, .. }) if ch.require_tag => p.push(format!(
                "{at}: message {m} needs a tag: the forum requires one"
            )),
            Some(_) => {}
        }
    }
}

fn forum(ch: &ChannelSpec, at: &str, p: &mut Vec<String>) {
    if ch.tags.len() > FORUM_TAGS_MAX {
        p.push(format!(
            "{at}: {} tags, Discord allows {FORUM_TAGS_MAX}",
            ch.tags.len()
        ));
    }
    duplicates(
        &format!("{at}: tag"),
        ch.tags.iter().map(|t| t.name.as_str()),
        p,
    );
    for t in &ch.tags {
        if t.name.is_empty() || t.name.chars().count() > 20 {
            p.push(format!("{at}: tag {:?} must be 1 to 20 characters", t.name));
        }
    }
    if ch.require_tag && ch.tags.is_empty() {
        p.push(format!("{at}: require_tag needs tags"));
    }
    if let Some(done) = &ch.done_tag
        && ch.tag(done).is_none()
    {
        p.push(format!("{at}: done_tag {done:?} is not one of its tags"));
    }
    if let Some(t) = &ch.template {
        let all = t.needs.iter().chain(t.needs_by_tag.values().flatten());
        for n in all {
            if let Err(e) = Regex::new(&n.regex) {
                p.push(format!(
                    "{at}: template need {:?}: regex does not compile: {e}",
                    n.label
                ));
            }
        }
        for tag in t.needs_by_tag.keys() {
            if ch.tag(tag).is_none() {
                p.push(format!(
                    "{at}: template needs_by_tag {tag:?} is not one of its tags"
                ));
            }
        }
    }
}

fn messages(s: &ServerConfig, p: &mut Vec<String>) {
    let placed: Vec<&Key> = s.channels().flat_map(|(_, c)| c.messages.iter()).collect();
    duplicates("message placed", placed.iter().map(|k| k.as_str()), p);
    for (key, m) in &s.messages {
        if !placed.contains(&key) {
            p.push(format!("message {key} is not placed in any channel"));
        }
        match m {
            MessageSpec::Embed {
                embed,
                role_buttons,
            } => {
                if embed.title.chars().count() > 256 {
                    p.push(format!(
                        "message {key}: embed title is longer than 256 characters"
                    ));
                }
                if embed.description.chars().count() > 4096 {
                    p.push(format!(
                        "message {key}: embed description is longer than 4096 characters"
                    ));
                }
                if role_buttons.len() > 5 {
                    p.push(format!("message {key}: at most 5 role buttons (one row)"));
                }
                for r in role_buttons {
                    match s.role(r.as_str()) {
                        None => p.push(format!("message {key}: role button for unknown role {r}")),
                        Some(role) if !role.self_assign => p.push(format!(
                            "message {key}: role {r} is not self_assign, so it cannot have a button"
                        )),
                        Some(_) => {}
                    }
                }
                refs(s, &embed.description, &format!("message {key}"), p);
            }
            MessageSpec::ForumPost { title, .. } => {
                if title.is_empty() || title.chars().count() > 100 {
                    p.push(format!(
                        "message {key}: post title must be 1 to 100 characters"
                    ));
                }
            }
        }
    }
}

fn refs(s: &ServerConfig, text: &str, at: &str, p: &mut Vec<String>) {
    for r in channel_refs(text) {
        if s.channel(r).is_none() {
            p.push(format!("{at}: {{#{r}}} names no channel"));
        }
    }
}

fn bot_texts(s: &ServerConfig, bot: &BotConfig, p: &mut Vec<String>) {
    refs(s, &bot.help.text, "bot.toml help.text", p);
    if bot.help.title.is_empty() || bot.help.title.chars().count() > 256 {
        p.push("bot.toml help.title must be 1 to 256 characters".into());
    }
    if bot.help.text.chars().count() > 4096 {
        p.push("bot.toml help.text is longer than 4096 characters".into());
    }
    if bot.links.is_empty() {
        p.push("bot.toml: no [[links]]".into());
    }
    for l in &bot.links {
        if l.url.scheme() != "https" {
            p.push(format!("bot.toml link {:?}: only https links", l.label));
        }
    }
}
