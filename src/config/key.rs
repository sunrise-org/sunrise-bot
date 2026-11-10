use std::{borrow::Borrow, fmt};

use serde::{Deserialize, Deserializer, Serialize, de};

/// A stable name for a role, category, channel or message in the config: `[a-z][a-z0-9_]*`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Key(String);

impl Key {
    pub fn new(s: &str) -> Result<Key, String> {
        let mut chars = s.chars();
        let ok = chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if ok && s.len() <= 32 {
            Ok(Key(s.to_string()))
        } else {
            Err(format!(
                "bad key {s:?}: use a-z, 0-9 and _, starting with a letter, up to 32 characters"
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Borrow<str> for Key {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Key, D::Error> {
        let s = String::deserialize(d)?;
        Key::new(&s).map_err(de::Error::custom)
    }
}

/// A colour as `#RRGGBB`; the number Discord uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Color, D::Error> {
        let s = String::deserialize(d)?;
        let hex = s.strip_prefix('#').filter(|h| h.len() == 6);
        hex.and_then(|h| u32::from_str_radix(h, 16).ok())
            .map(Color)
            .ok_or_else(|| de::Error::custom(format!("bad colour {s:?}: write #RRGGBB")))
    }
}

impl Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&format_args!("#{:06X}", self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_snake_case_words() {
        assert!(Key::new("modapi").is_ok());
        assert!(Key::new("role_buttons2").is_ok());
        for bad in ["", "Team", "2fast", "a-b", "a b"] {
            assert!(Key::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn colours_are_hash_rrggbb() {
        assert_eq!(
            serde_json::from_str::<Color>("\"#F28C28\"").unwrap(),
            Color(0xF28C28)
        );
        assert!(serde_json::from_str::<Color>("\"F28C28\"").is_err());
        assert!(serde_json::from_str::<Color>("\"#F28\"").is_err());
    }
}
