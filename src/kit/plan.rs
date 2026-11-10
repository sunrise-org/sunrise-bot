// Ported from Kira (github.com/horizon-rust/kira, src/layout-diff.js and src/adopt.js), MIT © 2026 Ludovic Marie and the Kira contributors; see LICENSES/kira-MIT.txt
//! The bootstrap planner: the config and a snapshot of the guild in, a list of changes out. No network, no clock.
use std::{collections::BTreeMap, fmt};

use super::{
    access::{self, Line},
    live::*,
    perms::Permissions,
};
use crate::{config::*, ids::Snowflake};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectKind {
    Role,
    Category,
    Channel,
}

impl ObjectKind {
    pub fn parse(s: &str) -> Option<ObjectKind> {
        [ObjectKind::Role, ObjectKind::Category, ObjectKind::Channel]
            .into_iter()
            .find(|k| k.to_string() == s)
    }
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ObjectKind::Role => "role",
            ObjectKind::Category => "category",
            ObjectKind::Channel => "channel",
        })
    }
}

/// Whether the bot made an object or found it already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Created,
    Adopted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownObject {
    pub id: Snowflake,
    pub origin: Origin,
}

/// The ids the bot has recorded (the store), by kind and config key.
#[derive(Debug, Clone, Default)]
pub struct Known(BTreeMap<(ObjectKind, Key), KnownObject>);

impl Known {
    pub fn insert(&mut self, kind: ObjectKind, key: Key, id: Snowflake, origin: Origin) {
        self.0.insert((kind, key), KnownObject { id, origin });
    }

    pub fn remove(&mut self, kind: ObjectKind, key: &Key) {
        self.0.remove(&(kind, key.clone()));
    }

    pub fn get(&self, kind: ObjectKind, key: &Key) -> Option<KnownObject> {
        self.0.get(&(kind, key.clone())).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (ObjectKind, &Key, KnownObject)> {
        self.0.iter().map(|((kind, key), o)| (*kind, key, *o))
    }
}

pub struct PlanInput<'a> {
    pub config: &'a ServerConfig,
    pub live: &'a Live,
    pub known: &'a Known,
    /// The bot tried to switch Community on and Discord said no: plan as if the file said `manual`.
    pub community_denied: bool,
    /// Plan the deletion of channels and categories the bot made that left the file.
    pub prune: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("{count} copies of {what} ({ids}): delete the extra ones, then run bootstrap again")]
    Duplicate {
        count: usize,
        what: String,
        ids: String,
    },
    #[error(
        "role {name:?} is adopt_only but the server has no such role: create it by hand or pin its id"
    )]
    AdoptMissing { name: String },
}

/// A value that differs: what the server has, what the file wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change<T> {
    pub from: String,
    pub to: T,
}

fn change<T>(from: impl Into<String>, to: T) -> Option<Change<T>> {
    Some(Change {
        from: from.into(),
        to,
    })
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuildEdit {
    pub name: Option<Change<String>>,
    pub verification_level: Option<Change<VerificationLevel>>,
    pub explicit_content_filter: Option<Change<ContentFilter>>,
    pub default_notifications: Option<Change<Notifications>>,
    pub rules_channel: Option<Change<Key>>,
    pub updates_channel: Option<Change<Key>>,
}

impl GuildEdit {
    fn is_empty(&self) -> bool {
        *self == GuildEdit::default()
    }

    fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut line =
            |what: &str, from: &str, to: String| out.push(format!("guild {what}: {from} → {to}"));
        if let Some(c) = &self.name {
            line("name", &c.from, format!("{:?}", c.to));
        }
        if let Some(c) = &self.verification_level {
            line("verification level", &c.from, format!("{:?}", c.to));
        }
        if let Some(c) = &self.explicit_content_filter {
            line("explicit content filter", &c.from, format!("{:?}", c.to));
        }
        if let Some(c) = &self.default_notifications {
            line("default notifications", &c.from, format!("{:?}", c.to));
        }
        if let Some(c) = &self.rules_channel {
            line("rules channel", &c.from, format!("#{}", c.to));
        }
        if let Some(c) = &self.updates_channel {
            line("updates channel", &c.from, format!("#{}", c.to));
        }
        out
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoleEdit {
    pub name: Option<Change<String>>,
    pub color: Option<Change<Color>>,
    pub hoist: Option<Change<bool>>,
    pub mentionable: Option<Change<bool>>,
    pub permissions: Option<Change<Permissions>>,
}

impl RoleEdit {
    fn is_empty(&self) -> bool {
        *self == RoleEdit::default()
    }

    fn lines(&self, role: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(c) = &self.name {
            out.push(format!("role {role}: name {:?} → {:?}", c.from, c.to));
        }
        if let Some(c) = &self.color {
            out.push(format!("role {role}: colour {} → #{:06X}", c.from, c.to.0));
        }
        if let Some(c) = &self.hoist {
            out.push(format!("role {role}: hoist {} → {}", c.from, c.to));
        }
        if let Some(c) = &self.mentionable {
            out.push(format!("role {role}: mentionable {} → {}", c.from, c.to));
        }
        if let Some(c) = &self.permissions {
            let from = Permissions(c.from.parse().unwrap_or(0));
            out.push(format!(
                "role {role}: permissions -[{}] +[{}]",
                from.without(c.to).names().join(", "),
                c.to.without(from).names().join(", ")
            ));
        }
        out
    }
}

/// A forum tag as wanted; `id` is the live tag of that name, kept so posts keep their tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagWant {
    pub id: Option<Snowflake>,
    pub name: String,
    pub emoji: String,
    pub moderated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverwriteEdit {
    pub want: Vec<access::WantOverwrite>,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChannelEdit {
    pub name: Option<Change<String>>,
    pub kind: Option<Change<ChannelKind>>,
    pub parent: Option<Change<Key>>,
    pub topic: Option<Change<String>>,
    pub slowmode: Option<Change<u32>>,
    pub overwrites: Option<OverwriteEdit>,
    pub tags: Option<Vec<TagWant>>,
    pub sort: Option<Change<ForumSort>>,
    pub layout: Option<Change<ForumLayout>>,
    pub default_reaction: Option<Change<String>>,
    pub require_tag: Option<Change<bool>>,
}

impl ChannelEdit {
    fn is_empty(&self) -> bool {
        *self == ChannelEdit::default()
    }

    fn lines(&self, label: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(c) = &self.name {
            out.push(format!("{label}: name {:?} → {:?}", c.from, c.to));
        }
        if let Some(c) = &self.kind {
            out.push(format!("{label}: type {} → {:?}", c.from, c.to));
        }
        if let Some(c) = &self.parent {
            out.push(format!("{label}: move from {} to {}", c.from, c.to));
        }
        if let Some(c) = &self.topic {
            out.push(format!("{label}: topic {:?} → {:?}", c.from, c.to));
        }
        if let Some(c) = &self.slowmode {
            out.push(format!("{label}: slowmode {} → {}s", c.from, c.to));
        }
        if let Some(o) = &self.overwrites {
            out.push(format!(
                "{label}: permission overwrites differ: {}",
                o.summary
            ));
        }
        if let Some(t) = &self.tags {
            out.push(format!(
                "{label}: forum tags differ (wanted: {})",
                t.iter()
                    .map(|t| format!("{} {}", t.emoji, t.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if let Some(c) = &self.sort {
            out.push(format!("{label}: sort order {} → {:?}", c.from, c.to));
        }
        if let Some(c) = &self.layout {
            out.push(format!("{label}: layout {} → {:?}", c.from, c.to));
        }
        if let Some(c) = &self.default_reaction {
            out.push(format!(
                "{label}: default reaction {:?} → {:?}",
                c.from, c.to
            ));
        }
        if let Some(c) = &self.require_tag {
            out.push(format!("{label}: require a tag {} → {}", c.from, c.to));
        }
        out
    }
}

/// One change to the guild. Actions run in the order of the plan; each says what to do, the executor knows how.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    EditGuild(GuildEdit),
    SetEveryonePermissions {
        from: Permissions,
        to: Permissions,
    },
    /// Needs the rules and updates channels to exist.
    EnableCommunity,
    CreateRole {
        key: Key,
    },
    EditRole {
        key: Key,
        id: Snowflake,
        edit: RoleEdit,
    },
    /// Highest first; only the keys listed.
    ReorderRoles {
        order: Vec<Key>,
    },
    CreateCategory {
        key: Key,
    },
    EditCategory {
        key: Key,
        id: Snowflake,
        edit: ChannelEdit,
    },
    ReorderCategories {
        order: Vec<Key>,
    },
    CreateChannel {
        key: Key,
        category: Key,
    },
    EditChannel {
        key: Key,
        id: Snowflake,
        edit: ChannelEdit,
    },
    ReorderChannels {
        category: Key,
        order: Vec<Key>,
    },
    DeleteChannel {
        kind: ObjectKind,
        key: Key,
        id: Snowflake,
    },
}

impl Action {
    /// Creating or retyping to a kind that exists only with Community on.
    pub fn needs_community(&self, config: &ServerConfig) -> bool {
        match self {
            Action::CreateChannel { key, .. } => config
                .channel(key.as_str())
                .is_some_and(|c| c.kind.needs_community()),
            Action::EditChannel { key, edit, .. } => {
                edit.kind.is_some()
                    && config
                        .channel(key.as_str())
                        .is_some_and(|c| c.kind.needs_community())
            }
            _ => false,
        }
    }
}

/// Something found by name that the bot now owns the record of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adoption {
    pub kind: ObjectKind,
    pub key: Key,
    pub id: Snowflake,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    pub kind: ObjectKind,
    pub key: Key,
    pub id: Snowflake,
}

/// Work that cannot be done until a person acts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waiting {
    Community {
        channels: Vec<String>,
        rules: String,
        updates: String,
    },
}

impl fmt::Display for Waiting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Waiting::Community {
            channels,
            rules,
            updates,
        } = self;
        write!(
            f,
            "{} waiting for Community (enable it in Server Settings → Enable Community, rules channel #{rules}, updates channel #{updates})",
            channels
                .iter()
                .map(|c| format!("#{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub actions: Vec<Action>,
    pub adopt: Vec<Adoption>,
    pub orphans: Vec<Orphan>,
    pub waiting: Vec<Waiting>,
    /// The lines `actions` print as, one list per action.
    lines: Vec<Vec<String>>,
}

impl Plan {
    /// Nothing for the bot to change on Discord (waiting work and records to adopt do not count).
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    pub fn render(&self) -> String {
        let mut out = Vec::new();
        for l in self.lines.iter().flatten() {
            out.push(l.clone());
        }
        for a in &self.adopt {
            out.push(format!(
                "= adopt {} {} ({}) as {:?}",
                a.kind,
                a.name,
                a.id,
                a.key.as_str()
            ));
        }
        for o in &self.orphans {
            out.push(format!("? {} {:?} ({}) is not in the file anymore: delete it by hand (or --apply --prune if the bot made it)", o.kind, o.key.as_str(), o.id));
        }
        for w in &self.waiting {
            out.push(format!("! {w}"));
        }
        out.push(if self.is_empty() {
            "Nothing to change.".to_string()
        } else {
            format!("{} change(s).", self.actions.len())
        });
        out.join("\n")
    }

    /// Each action with the lines it prints as.
    pub fn steps(&self) -> impl Iterator<Item = (&Action, &[String])> {
        self.actions
            .iter()
            .zip(self.lines.iter().map(Vec::as_slice))
    }

    fn push(&mut self, action: Action, lines: Vec<String>) {
        self.actions.push(action);
        self.lines.push(lines);
    }
}

// ── lookup ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Via {
    Stored,
    Pinned,
    Name,
}

/// The names an object is written with: its name, the name without its decoration ("💬│chat" is "chat"), its `aka`.
fn handles(name: &str, aka: &[String]) -> Vec<String> {
    let plain = name.rsplit('│').next().unwrap_or(name);
    [name, plain]
        .into_iter()
        .map(str::to_string)
        .chain(aka.iter().cloned())
        .map(|n| n.to_lowercase())
        .collect()
}

/// One match is the thing; none means create it; more than one stops everything (Kira's `unique`).
fn unique<T>(
    found: Vec<&T>,
    what: String,
    id: impl Fn(&T) -> Snowflake,
) -> Result<Option<&T>, PlanError> {
    if found.len() > 1 {
        return Err(PlanError::Duplicate {
            count: found.len(),
            what,
            ids: found
                .iter()
                .map(|f| id(f).to_string())
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    Ok(found.into_iter().next())
}

struct Resolved<'a> {
    roles: BTreeMap<Key, (&'a LiveRole, Via)>,
    categories: BTreeMap<Key, (&'a LiveChannel, Via)>,
    channels: BTreeMap<Key, (&'a LiveChannel, Via)>,
}

impl<'a> Resolved<'a> {
    fn new(
        config: &ServerConfig,
        live: &'a Live,
        known: &Known,
    ) -> Result<Resolved<'a>, PlanError> {
        let mut roles = BTreeMap::new();
        for spec in &config.roles {
            let stored = known
                .get(ObjectKind::Role, &spec.key)
                .and_then(|k| live.role(k.id));
            let pinned = spec.id.and_then(|id| live.role(id));
            let hit = match (stored, pinned) {
                (Some(r), _) => Some((r, Via::Stored)),
                (None, Some(r)) => Some((r, Via::Pinned)),
                (None, None) => {
                    let named: Vec<&LiveRole> = live
                        .roles
                        .iter()
                        .filter(|r| !r.managed && r.id != live.guild.id && r.name == spec.name)
                        .collect();
                    unique(named, format!("role {}", spec.name), |r| r.id)?.map(|r| (r, Via::Name))
                }
            };
            if let Some(h) = hit {
                roles.insert(spec.key.clone(), h);
            }
        }
        let mut categories = BTreeMap::new();
        let mut channels = BTreeMap::new();
        for cat in &config.categories {
            let found = Self::channel(
                ObjectKind::Category,
                &cat.key,
                cat.id,
                &handles(&cat.name, &cat.aka),
                |k| k == LiveKind::Category,
                &cat.name,
                live,
                known,
            )?;
            if let Some(f) = found {
                categories.insert(cat.key.clone(), f);
            }
            for ch in &cat.channels {
                let want = LiveKind::of(ch.kind);
                let label = format!("channel #{}", ch.name);
                let found = Self::channel(
                    ObjectKind::Channel,
                    &ch.key,
                    ch.id,
                    &handles(&ch.name, &ch.aka),
                    |k| k.same_as(want),
                    &label,
                    live,
                    known,
                )?;
                if let Some(f) = found {
                    channels.insert(ch.key.clone(), f);
                }
            }
        }
        Ok(Resolved {
            roles,
            categories,
            channels,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn channel(
        kind: ObjectKind,
        key: &Key,
        pinned: Option<Snowflake>,
        names: &[String],
        kind_ok: impl Fn(LiveKind) -> bool,
        label: &str,
        live: &'a Live,
        known: &Known,
    ) -> Result<Option<(&'a LiveChannel, Via)>, PlanError> {
        if let Some(c) = known.get(kind, key).and_then(|k| live.channel(k.id)) {
            return Ok(Some((c, Via::Stored)));
        }
        if let Some(c) = pinned.and_then(|id| live.channel(id)) {
            return Ok(Some((c, Via::Pinned)));
        }
        let named: Vec<&LiveChannel> = live
            .channels
            .iter()
            .filter(|c| kind_ok(c.kind) && names.contains(&c.name.to_lowercase()))
            .collect();
        Ok(unique(named, label.to_string(), |c| c.id)?.map(|c| (c, Via::Name)))
    }
}

// ── the plan ────────────────────────────────────────────────────────────────

pub fn plan(input: &PlanInput) -> Result<Plan, PlanError> {
    let PlanInput {
        config,
        live,
        known,
        community_denied,
        prune,
    } = *input;
    let res = Resolved::new(config, live, known)?;
    let mut plan = Plan::default();
    let community_on = live.community();
    let will_enable =
        !community_on && config.guild.community.enable == CommunityEnable::Bot && !community_denied;

    adoptions(config, &res, &mut plan);

    // The guild itself, then roles, categories and the channels that exist without Community.
    let base = guild_edit(config, live, &res, false);
    if !base.is_empty() {
        plan.push(Action::EditGuild(base.clone()), base.lines());
    }
    if let Some(list) = &config.guild.everyone_permissions {
        let to = Permissions::of(list);
        if let Some(everyone) = live.role(live.guild.id)
            && everyone.permissions != to
        {
            plan.push(
                Action::SetEveryonePermissions {
                    from: everyone.permissions,
                    to,
                },
                vec![format!(
                    "@everyone permissions: -[{}] +[{}]",
                    everyone.permissions.without(to).names().join(", "),
                    to.without(everyone.permissions).names().join(", ")
                )],
            );
        }
    }
    roles(config, &res, &mut plan)?;
    categories(config, live, &res, &mut plan);

    let mut deferred = Vec::new();
    let mut waiting = Vec::new();
    for (cat, ch) in config.channels() {
        let blocked = ch.kind.needs_community() && !community_on;
        match res.channels.get(&ch.key) {
            None if blocked && will_enable => deferred.push((
                Action::CreateChannel {
                    key: ch.key.clone(),
                    category: cat.key.clone(),
                },
                vec![format!(
                    "+ create {} #{} in {}",
                    kind_word(ch.kind),
                    ch.name,
                    cat.name
                )],
            )),
            None if blocked => waiting.push(ch.name.clone()),
            None => plan.push(
                Action::CreateChannel {
                    key: ch.key.clone(),
                    category: cat.key.clone(),
                },
                vec![format!(
                    "+ create {} #{} in {}",
                    kind_word(ch.kind),
                    ch.name,
                    cat.name
                )],
            ),
            Some((live_ch, _)) => {
                let edit = channel_edit(live, &res, cat, ch, live_ch);
                if edit.is_empty() {
                    continue;
                }
                let lines = edit.lines(&format!("#{}", live_ch.name));
                let action = Action::EditChannel {
                    key: ch.key.clone(),
                    id: live_ch.id,
                    edit,
                };
                if action.needs_community(config) && !community_on {
                    if will_enable {
                        deferred.push((action, lines))
                    } else {
                        waiting.push(ch.name.clone())
                    }
                } else {
                    plan.push(action, lines);
                }
            }
        }
    }
    if will_enable {
        plan.push(
            Action::EnableCommunity,
            vec!["+ enable Community (rules and updates channels set)".to_string()],
        );
    }
    for (a, l) in deferred {
        plan.push(a, l);
    }
    if !waiting.is_empty() {
        let c = &config.guild.community;
        let name = |k: &Key| {
            config
                .channel(k.as_str())
                .map_or(k.to_string(), |c| c.name.clone())
        };
        plan.waiting.push(Waiting::Community {
            channels: waiting,
            rules: name(&c.rules_channel),
            updates: name(&c.updates_channel),
        });
    }
    if community_on {
        let edit = guild_edit(config, live, &res, true);
        if !edit.is_empty() {
            plan.push(Action::EditGuild(edit.clone()), edit.lines());
        }
    }
    orders(config, live, &res, &mut plan);
    orphans(config, live, known, prune, &mut plan);
    Ok(plan)
}

fn kind_word(kind: ChannelKind) -> &'static str {
    match kind {
        ChannelKind::Text => "text channel",
        ChannelKind::Announcement => "announcement channel",
        ChannelKind::Forum => "forum",
        ChannelKind::Voice => "voice channel",
    }
}

fn adoptions(config: &ServerConfig, res: &Resolved, plan: &mut Plan) {
    for spec in &config.roles {
        if let Some((r, _)) = res.roles.get(&spec.key).filter(|(_, v)| *v != Via::Stored) {
            plan.adopt.push(Adoption {
                kind: ObjectKind::Role,
                key: spec.key.clone(),
                id: r.id,
                name: r.name.clone(),
            });
        }
    }
    for cat in &config.categories {
        if let Some((c, _)) = res
            .categories
            .get(&cat.key)
            .filter(|(_, v)| *v != Via::Stored)
        {
            plan.adopt.push(Adoption {
                kind: ObjectKind::Category,
                key: cat.key.clone(),
                id: c.id,
                name: c.name.clone(),
            });
        }
        for ch in &cat.channels {
            if let Some((c, _)) = res.channels.get(&ch.key).filter(|(_, v)| *v != Via::Stored) {
                plan.adopt.push(Adoption {
                    kind: ObjectKind::Channel,
                    key: ch.key.clone(),
                    id: c.id,
                    name: c.name.clone(),
                });
            }
        }
    }
}

/// `community`: the settings that exist only with Community on; otherwise the plain ones.
fn guild_edit(config: &ServerConfig, live: &Live, res: &Resolved, community: bool) -> GuildEdit {
    let (g, lg) = (&config.guild, &live.guild);
    let mut e = GuildEdit::default();
    if community {
        let c = &g.community;
        for (want, have, slot) in [
            (&c.rules_channel, lg.rules_channel_id, &mut e.rules_channel),
            (
                &c.updates_channel,
                lg.public_updates_channel_id,
                &mut e.updates_channel,
            ),
        ] {
            let wanted = res.channels.get(want).map(|(ch, _)| ch.id);
            if wanted.is_none() || wanted != have {
                *slot = change(
                    have.map_or("none".to_string(), |i| i.to_string()),
                    want.clone(),
                );
            }
        }
        return e;
    }
    if lg.name != g.name {
        e.name = change(lg.name.clone(), g.name.clone());
    }
    if lg.verification_level != g.verification_level.api() {
        e.verification_level = change(lg.verification_level.to_string(), g.verification_level);
    }
    if lg.explicit_content_filter != g.explicit_content_filter.api() {
        e.explicit_content_filter = change(
            lg.explicit_content_filter.to_string(),
            g.explicit_content_filter,
        );
    }
    if lg.default_message_notifications != g.default_notifications.api() {
        e.default_notifications = change(
            lg.default_message_notifications.to_string(),
            g.default_notifications,
        );
    }
    e
}

fn roles(config: &ServerConfig, res: &Resolved, plan: &mut Plan) -> Result<(), PlanError> {
    for spec in &config.roles {
        let found = res.roles.get(&spec.key).map(|(r, _)| *r);
        match (found, spec.manage) {
            (None, RoleManage::AdoptOnly) => {
                return Err(PlanError::AdoptMissing {
                    name: spec.name.clone(),
                });
            }
            (None, RoleManage::Full) => plan.push(
                Action::CreateRole {
                    key: spec.key.clone(),
                },
                vec![format!("+ create role {}", spec.name)],
            ),
            (Some(_), RoleManage::AdoptOnly) => {}
            (Some(r), RoleManage::Full) => {
                let mut e = RoleEdit::default();
                if r.name != spec.name {
                    e.name = change(r.name.clone(), spec.name.clone());
                }
                if let Some(color) = spec.color
                    && r.color != color.0
                {
                    e.color = change(format!("#{:06X}", r.color), color);
                }
                if r.hoist != spec.hoist {
                    e.hoist = change(r.hoist.to_string(), spec.hoist);
                }
                if r.mentionable != spec.mentionable {
                    e.mentionable = change(r.mentionable.to_string(), spec.mentionable);
                }
                let want = Permissions::of(&spec.permissions);
                if r.permissions != want {
                    e.permissions = change(r.permissions.0.to_string(), want);
                }
                if !e.is_empty() {
                    let lines = e.lines(&r.name);
                    plan.push(
                        Action::EditRole {
                            key: spec.key.clone(),
                            id: r.id,
                            edit: e,
                        },
                        lines,
                    );
                }
            }
        }
    }
    // Order: only where it is strictly wrong; roles Discord reports at the same position are left alone.
    let movable: Vec<(&Key, i32)> = config
        .role_order
        .iter()
        .filter(|k| {
            config
                .role(k.as_str())
                .is_some_and(|r| r.manage == RoleManage::Full)
        })
        .filter_map(|k| res.roles.get(k).map(|(r, _)| (k, r.position)))
        .collect();
    if movable.windows(2).any(|w| w[0].1 < w[1].1) {
        let order: Vec<Key> = movable.iter().map(|(k, _)| (*k).clone()).collect();
        plan.push(
            Action::ReorderRoles { order },
            vec!["~ role order differs".to_string()],
        );
    }
    Ok(())
}

fn categories(config: &ServerConfig, live: &Live, res: &Resolved, plan: &mut Plan) {
    for cat in &config.categories {
        let Some((c, _)) = res.categories.get(&cat.key) else {
            plan.push(
                Action::CreateCategory {
                    key: cat.key.clone(),
                },
                vec![format!("+ create category {}", cat.name)],
            );
            continue;
        };
        let mut e = ChannelEdit::default();
        if c.name != cat.name {
            e.name = change(c.name.clone(), cat.name.clone());
        }
        e.overwrites = overwrite_edit(live, res, cat.access, None, c);
        if !e.is_empty() {
            let lines = e.lines(&format!("category {}", c.name));
            plan.push(
                Action::EditCategory {
                    key: cat.key.clone(),
                    id: c.id,
                    edit: e,
                },
                lines,
            );
        }
    }
}

fn overwrite_edit(
    live: &Live,
    res: &Resolved,
    access: Access,
    visible_to: Option<Audience>,
    ch: &LiveChannel,
) -> Option<OverwriteEdit> {
    let want = access::wanted(access, visible_to);
    let have = access::lines(&ch.permission_overwrites);
    let resolved = access::resolve(&want, live.guild.id, live.bot_id, |k| {
        res.roles.get(k).map(|(r, _)| r.id)
    });
    if resolved.as_ref() == Some(&have) {
        return None;
    }
    let name = |l: &Line| match (l.kind, l.id) {
        (OverwriteKind::Role, id) if id == live.guild.id => "@everyone".to_string(),
        (OverwriteKind::Role, id) => live
            .role(id)
            .map_or(format!("role {id}"), |r| r.name.clone()),
        (OverwriteKind::Member, id) if id == live.bot_id => "the bot".to_string(),
        (OverwriteKind::Member, id) => format!("member {id}"),
    };
    let show = |l: &Line| {
        format!(
            "{}: +[{}] -[{}]",
            name(l),
            l.allow.names().join(" "),
            l.deny.names().join(" ")
        )
    };
    let summary = match &resolved {
        Some(w) => {
            let added: Vec<String> = w.iter().filter(|l| !have.contains(l)).map(show).collect();
            let removed: Vec<String> = have.iter().filter(|l| !w.contains(l)).map(show).collect();
            format!("+({}) -({})", added.join("; "), removed.join("; "))
        }
        None => "a role it names does not exist yet".to_string(),
    };
    Some(OverwriteEdit { want, summary })
}

fn channel_edit(
    live: &Live,
    res: &Resolved,
    cat: &CategorySpec,
    spec: &ChannelSpec,
    ch: &LiveChannel,
) -> ChannelEdit {
    let mut e = ChannelEdit::default();
    if ch.name != spec.name {
        e.name = change(ch.name.clone(), spec.name.clone());
    }
    if ch.kind != LiveKind::of(spec.kind) {
        e.kind = change(format!("{:?}", ch.kind), spec.kind);
    }
    let parent = res.categories.get(&cat.key).map(|(c, _)| c.id);
    if parent.is_none() || parent != ch.parent_id {
        e.parent = change(
            ch.parent_id
                .and_then(|p| live.channel(p))
                .map_or("no category".to_string(), |c| c.name.clone()),
            cat.key.clone(),
        );
    }
    if spec.kind != ChannelKind::Voice {
        let (want, have) = (
            spec.topic.as_deref().unwrap_or(""),
            ch.topic.as_deref().unwrap_or(""),
        );
        if want != have {
            e.topic = change(have, want.to_string());
        }
        if spec.slowmode_seconds != ch.rate_limit_per_user {
            e.slowmode = change(
                format!("{}s", ch.rate_limit_per_user),
                spec.slowmode_seconds,
            );
        }
    }
    e.overwrites = overwrite_edit(
        live,
        res,
        spec.access.unwrap_or(cat.access),
        spec.visible_to,
        ch,
    );
    if spec.kind == ChannelKind::Forum {
        forum_edit(spec, ch, &mut e);
    }
    e
}

fn forum_edit(spec: &ChannelSpec, ch: &LiveChannel, e: &mut ChannelEdit) {
    let wanted: Vec<TagWant> = spec
        .tags
        .iter()
        .map(|t| TagWant {
            id: ch
                .available_tags
                .iter()
                .find(|l| l.name == t.name)
                .map(|l| l.id),
            name: t.name.clone(),
            emoji: t.emoji.clone(),
            moderated: t.team_only,
        })
        .collect();
    let have: Vec<(&str, Option<&str>, bool)> = ch
        .available_tags
        .iter()
        .map(|t| (t.name.as_str(), t.emoji_name.as_deref(), t.moderated))
        .collect();
    let want: Vec<(&str, Option<&str>, bool)> = wanted
        .iter()
        .map(|t| (t.name.as_str(), Some(t.emoji.as_str()), t.moderated))
        .collect();
    if have != want {
        e.tags = Some(wanted);
    }
    if let Some(sort) = spec.sort {
        let live_sort = ch.default_sort_order.unwrap_or(0);
        let want_sort = u8::from(sort == ForumSort::CreationDate);
        if live_sort != want_sort {
            e.sort = change(live_sort.to_string(), sort);
        }
    }
    if let Some(layout) = spec.layout {
        let live_layout = ch.default_forum_layout.unwrap_or(0);
        let ok = match layout {
            ForumLayout::List => live_layout <= 1,
            ForumLayout::Gallery => live_layout == 2,
        };
        if !ok {
            e.layout = change(live_layout.to_string(), layout);
        }
    }
    let have_reaction = ch
        .default_reaction_emoji
        .as_ref()
        .and_then(|r| r.emoji_name.as_deref());
    if spec.default_reaction.as_deref() != have_reaction {
        e.default_reaction = change(
            have_reaction.unwrap_or(""),
            spec.default_reaction.clone().unwrap_or_default(),
        );
    }
    let have_required = ch.flags & REQUIRE_TAG != 0;
    if spec.require_tag != have_required {
        e.require_tag = change(have_required.to_string(), spec.require_tag);
    }
}

fn rank(mut items: Vec<&LiveChannel>) -> Vec<Snowflake> {
    items.sort_by_key(|c| (c.position, c.id));
    items.into_iter().map(|c| c.id).collect()
}

/// Order of categories, then of the channels in each, as the file lists them (only what exists).
fn orders(config: &ServerConfig, live: &Live, res: &Resolved, plan: &mut Plan) {
    let cats: Vec<(&Key, &LiveChannel)> = config
        .categories
        .iter()
        .filter_map(|c| res.categories.get(&c.key).map(|(l, _)| (&c.key, *l)))
        .collect();
    let want: Vec<Snowflake> = cats.iter().map(|(_, l)| l.id).collect();
    if rank(cats.iter().map(|(_, l)| *l).collect()) != want {
        plan.push(
            Action::ReorderCategories {
                order: cats.iter().map(|(k, _)| (*k).clone()).collect(),
            },
            vec!["~ category order differs".to_string()],
        );
    }
    for cat in &config.categories {
        let Some((c, _)) = res.categories.get(&cat.key) else {
            continue;
        };
        let here: Vec<(&Key, &LiveChannel)> = cat
            .channels
            .iter()
            .filter_map(|ch| res.channels.get(&ch.key).map(|(l, _)| (&ch.key, *l)))
            .filter(|(_, l)| l.parent_id == Some(c.id))
            .collect();
        let want: Vec<Snowflake> = here.iter().map(|(_, l)| l.id).collect();
        let sorted = rank(
            live.channels
                .iter()
                .filter(|l| want.contains(&l.id))
                .collect(),
        );
        if sorted != want {
            plan.push(
                Action::ReorderChannels {
                    category: cat.key.clone(),
                    order: here.iter().map(|(k, _)| (*k).clone()).collect(),
                },
                vec![format!("~ channel order in {} differs", cat.name)],
            );
        }
    }
}

fn orphans(config: &ServerConfig, live: &Live, known: &Known, prune: bool, plan: &mut Plan) {
    for (kind, key, obj) in known.iter() {
        let in_file = match kind {
            ObjectKind::Role => config.role(key.as_str()).is_some(),
            ObjectKind::Category => config.categories.iter().any(|c| &c.key == key),
            ObjectKind::Channel => config.channel(key.as_str()).is_some(),
        };
        if in_file {
            continue;
        }
        let exists = match kind {
            ObjectKind::Role => live.role(obj.id).is_some(),
            _ => live.channel(obj.id).is_some(),
        };
        if !exists {
            continue;
        }
        plan.orphans.push(Orphan {
            kind,
            key: key.clone(),
            id: obj.id,
        });
        if prune && obj.origin == Origin::Created && kind != ObjectKind::Role {
            plan.push(
                Action::DeleteChannel {
                    kind,
                    key: key.clone(),
                    id: obj.id,
                },
                vec![format!("- delete {kind} {:?} ({})", key.as_str(), obj.id)],
            );
        }
    }
}
