// Presets after the `readonly`/`writers`/`staff` branches of Kira's permissions.js (github.com/horizon-rust/kira), MIT © 2026 Ludovic Marie and the Kira contributors; see LICENSES/kira-MIT.txt
use super::{
    live::{Overwrite, OverwriteKind},
    perms::{Permission as P, Permissions},
};
use crate::{
    config::{Access, Audience, Key},
    ids::Snowflake,
};

/// Who an overwrite is for. The two roles are fixed keys of the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    Everyone,
    Bot,
    Role(Key),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantOverwrite {
    pub who: Principal,
    pub allow: Permissions,
    pub deny: Permissions,
}

pub const TEAM: &str = "team";
pub const CONTRIBUTOR: &str = "contributor";

fn role(key: &str) -> Principal {
    Principal::Role(Key::new(key).expect("a fixed key"))
}

fn write() -> Permissions {
    Permissions::of(&[
        P::SendMessages,
        P::SendMessagesInThreads,
        P::CreatePublicThreads,
        P::CreatePrivateThreads,
    ])
}

fn view() -> Permissions {
    Permissions::of(&[P::ViewChannel])
}

fn ow(who: Principal, allow: Permissions, deny: Permissions) -> WantOverwrite {
    WantOverwrite { who, allow, deny }
}

/// The overwrites a preset stands for. Channels carry them explicitly (never synced to the category).
pub fn wanted(access: Access, visible_to: Option<Audience>) -> Vec<WantOverwrite> {
    let none = Permissions::NONE;
    let bot_writes = view() | write();
    match (access, visible_to) {
        (Access::Public, _) => vec![ow(
            Principal::Bot,
            bot_writes | Permissions::of(&[P::ManageMessages, P::ManageThreads]),
            none,
        )],
        (Access::Readonly, _) => vec![
            ow(Principal::Everyone, none, write()),
            ow(role(TEAM), write(), none),
            ow(Principal::Bot, bot_writes, none),
        ],
        (Access::BotFeed, None) => vec![
            ow(Principal::Everyone, none, write()),
            ow(Principal::Bot, bot_writes, none),
        ],
        (Access::BotFeed, Some(Audience::Team)) => vec![
            ow(Principal::Everyone, none, view()),
            ow(role(TEAM), view(), write()),
            ow(Principal::Bot, bot_writes, none),
        ],
        (Access::ContributorsWrite, _) => vec![
            ow(Principal::Everyone, none, write()),
            ow(role(CONTRIBUTOR), write(), none),
            ow(role(TEAM), write(), none),
            ow(
                Principal::Bot,
                bot_writes | Permissions::of(&[P::ManageThreads]),
                none,
            ),
        ],
        (Access::TeamPrivate, _) => vec![
            ow(Principal::Everyone, none, view()),
            ow(role(TEAM), view(), none),
            ow(Principal::Bot, bot_writes, none),
        ],
        (Access::ContributorsConnect, _) => {
            let connect = Permissions::of(&[P::Connect]);
            vec![
                ow(Principal::Everyone, none, connect),
                ow(role(CONTRIBUTOR), connect, none),
                ow(role(TEAM), connect, none),
            ]
        }
    }
}

/// One overwrite as Discord holds it, comparable: ordered by kind, then id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Line {
    pub kind: OverwriteKind,
    pub id: Snowflake,
    pub allow: Permissions,
    pub deny: Permissions,
}

/// The overwrites of an object as comparable lines: empty ones dropped, sorted.
pub fn lines(list: &[Overwrite]) -> Vec<Line> {
    let mut out: Vec<Line> = list
        .iter()
        .filter(|o| !o.allow.is_empty() || !o.deny.is_empty())
        .map(|o| Line {
            kind: o.kind,
            id: o.id,
            allow: o.allow,
            deny: o.deny,
        })
        .collect();
    out.sort();
    out
}

/// `wanted` with ids filled in, as lines; `None` when someone it names does not exist (yet).
pub fn resolve(
    want: &[WantOverwrite],
    everyone: Snowflake,
    bot: Snowflake,
    role_id: impl Fn(&Key) -> Option<Snowflake>,
) -> Option<Vec<Line>> {
    let mut out = Vec::new();
    for w in want {
        let (kind, id) = match &w.who {
            Principal::Everyone => (OverwriteKind::Role, everyone),
            Principal::Bot => (OverwriteKind::Member, bot),
            Principal::Role(k) => (OverwriteKind::Role, role_id(k)?),
        };
        if !w.allow.is_empty() || !w.deny.is_empty() {
            out.push(Line {
                kind,
                id,
                allow: w.allow,
                deny: w.deny,
            });
        }
    }
    out.sort();
    Some(out)
}
