use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

macro_rules! permissions {
    ($($name:ident = $bit:expr),+ $(,)?) => {
        /// A Discord permission, named as in discord.js's `PermissionFlagsBits`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
        pub enum Permission { $($name),+ }

        impl Permission {
            pub const ALL: &'static [Permission] = &[$(Permission::$name),+];

            pub const fn bit(self) -> u64 {
                match self { $(Permission::$name => 1u64 << $bit),+ }
            }

            pub const fn name(self) -> &'static str {
                match self { $(Permission::$name => stringify!($name)),+ }
            }
        }
    };
}

permissions! {
    CreateInstantInvite = 0, KickMembers = 1, BanMembers = 2, Administrator = 3, ManageChannels = 4,
    ManageGuild = 5, AddReactions = 6, ViewAuditLog = 7, PrioritySpeaker = 8, Stream = 9, ViewChannel = 10,
    SendMessages = 11, SendTTSMessages = 12, ManageMessages = 13, EmbedLinks = 14, AttachFiles = 15,
    ReadMessageHistory = 16, MentionEveryone = 17, UseExternalEmojis = 18, ViewGuildInsights = 19,
    Connect = 20, Speak = 21, MuteMembers = 22, DeafenMembers = 23, MoveMembers = 24, UseVAD = 25,
    ChangeNickname = 26, ManageNicknames = 27, ManageRoles = 28, ManageWebhooks = 29,
    ManageGuildExpressions = 30, UseApplicationCommands = 31, RequestToSpeak = 32, ManageEvents = 33,
    ManageThreads = 34, CreatePublicThreads = 35, CreatePrivateThreads = 36, UseExternalStickers = 37,
    SendMessagesInThreads = 38, UseEmbeddedActivities = 39, ModerateMembers = 40,
    ViewCreatorMonetizationAnalytics = 41, UseSoundboard = 42, CreateGuildExpressions = 43,
    CreateEvents = 44, UseExternalSounds = 45, SendVoiceMessages = 46, SendPolls = 49,
    UseExternalApps = 50, PinMessages = 51, BypassSlowmode = 52,
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A set of permissions: a string of the number on the wire, as Discord sends it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Permissions(pub u64);

impl Permissions {
    pub const NONE: Permissions = Permissions(0);

    pub fn of(list: &[Permission]) -> Self {
        Permissions(list.iter().fold(0, |acc, p| acc | p.bit()))
    }

    pub fn contains(self, p: Permission) -> bool {
        self.0 & p.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn names(self) -> Vec<String> {
        let known: Vec<String> = Permission::ALL
            .iter()
            .filter(|p| self.contains(**p))
            .map(|p| p.name().to_string())
            .collect();
        let unknown = self.0 & !Permissions::of(Permission::ALL).0;
        let mut out = known;
        (0..64)
            .filter(|b| unknown >> b & 1 == 1)
            .for_each(|b| out.push(format!("bit{b}")));
        out
    }

    /// `self` without the bits of `other`.
    pub fn without(self, other: Permissions) -> Permissions {
        Permissions(self.0 & !other.0)
    }
}

impl std::ops::BitOr for Permissions {
    type Output = Permissions;
    fn bitor(self, rhs: Permissions) -> Permissions {
        Permissions(self.0 | rhs.0)
    }
}

impl fmt::Debug for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for Permissions {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Permissions {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl de::Visitor<'_> for V {
            type Value = Permissions;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a permission bitfield (a string of digits)")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Permissions, E> {
                v.parse()
                    .map(Permissions)
                    .map_err(|_| E::custom(format!("not a permission bitfield: {v:?}")))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Permissions, E> {
                Ok(Permissions(v))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Values from the live server's overwrites (Discord's own numbers), not computed from the enum.
    #[test]
    fn bits_match_discords() {
        assert_eq!(Permission::ViewChannel.bit(), 1024);
        assert_eq!(Permission::SendMessagesInThreads.bit(), 274_877_906_944);
        assert_eq!(Permission::Connect.bit(), 1_048_576);
        let write = Permissions::of(&[
            Permission::SendMessages,
            Permission::SendMessagesInThreads,
            Permission::CreatePublicThreads,
            Permission::CreatePrivateThreads,
        ]);
        assert_eq!(write, Permissions(377_957_124_096));
    }

    #[test]
    fn names_list_known_and_unknown_bits() {
        assert_eq!(
            Permissions(1024 | 1 << 47).names(),
            ["ViewChannel", "bit47"]
        );
    }

    #[test]
    fn config_names_are_discordjs_names() {
        let p: Permission = serde_json::from_str("\"UseVAD\"").unwrap();
        assert_eq!(p, Permission::UseVAD);
        assert!(serde_json::from_str::<Permission>("\"Nope\"").is_err());
    }
}
