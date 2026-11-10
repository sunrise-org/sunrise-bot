use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// A Discord id: a string on the wire, a u64 here.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Snowflake(pub u64);

impl fmt::Display for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Debug for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl std::str::FromStr for Snowflake {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Snowflake)
    }
}

impl Serialize for Snowflake {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Snowflake {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl de::Visitor<'_> for V {
            type Value = Snowflake;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a Discord id (a string of digits)")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Snowflake, E> {
                v.parse()
                    .map_err(|_| E::custom(format!("not a Discord id: {v:?}")))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Snowflake, E> {
                Ok(Snowflake(v))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_strings_and_numbers_and_writes_strings() {
        let a: Snowflake = serde_json::from_str("\"1569116592132857946\"").unwrap();
        let b: Snowflake = serde_json::from_str("1569116592132857946").unwrap();
        assert_eq!(a, b);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            "\"1569116592132857946\""
        );
        assert!(serde_json::from_str::<Snowflake>("\"abc\"").is_err());
    }
}
