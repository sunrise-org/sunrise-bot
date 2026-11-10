use std::collections::BTreeMap;

use super::Key;
use crate::ids::Snowflake;

/// `{#key}` in a text stands for the channel with that key: `<#id>` once the channel exists, `#name` before.
pub fn channel_refs(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{#") {
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                out.push(&after[..end]);
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

/// Fills `{#key}` from `ids`; a channel with no id yet reads as plain `#key`.
pub fn render_channel_refs(text: &str, ids: &BTreeMap<Key, Snowflake>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{#") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        match ids.get(name) {
            Some(id) => out.push_str(&format!("<#{id}>")),
            None => out.push_str(&format!("#{name}")),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_fills_channel_refs() {
        let text = "Talk: {#general}, help {#support} (and {#nope";
        assert_eq!(channel_refs(text), ["general", "support"]);
        let ids = BTreeMap::from([(Key::new("general").unwrap(), Snowflake(42))]);
        assert_eq!(
            render_channel_refs(text, &ids),
            "Talk: <#42>, help #support (and {#nope"
        );
    }
}
