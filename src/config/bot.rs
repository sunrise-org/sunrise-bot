use serde::Deserialize;
use url::Url;

/// `config/bot.toml`: what the running bot says and does. Sections arrive with the features that read them.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BotConfig {
    pub help: HelpSpec,
    pub links: Vec<LinkSpec>,
}

/// `/help`: an embed whose text may name channels as `{#key}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelpSpec {
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkSpec {
    pub emoji: String,
    pub label: String,
    pub url: Url,
}
