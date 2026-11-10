mod bot;
mod key;
mod server;
mod text;
mod validate;

use std::path::Path;

pub use bot::*;
pub use key::*;
pub use server::*;
pub use text::*;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("{path}: {message}")]
    Parse { path: String, message: String },
    #[error("invalid configuration:\n{}", .0.iter().map(|p| format!("  - {p}")).collect::<Vec<_>>().join("\n"))]
    Invalid(Vec<String>),
}

/// Both files, checked against each other.
#[derive(Debug, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub bot: BotConfig,
}

impl Config {
    /// Reads `server.json` and `bot.toml` from `dir`.
    pub fn load(dir: &Path) -> Result<Config, ConfigError> {
        let server_path = dir.join("server.json");
        let bot_path = dir.join("bot.toml");
        let server_text = read(&server_path)?;
        let bot_text = read(&bot_path)?;
        let server: ServerConfig =
            serde_json::from_str(&server_text).map_err(|e| ConfigError::Parse {
                path: server_path.display().to_string(),
                message: e.to_string(),
            })?;
        let bot: BotConfig = toml::from_str(&bot_text).map_err(|e| ConfigError::Parse {
            path: bot_path.display().to_string(),
            message: e.to_string(),
        })?;
        let config = Config { server, bot };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let problems = validate::problems(&self.server, &self.bot);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Invalid(problems))
        }
    }
}

fn read(path: &Path) -> Result<String, ConfigError> {
    std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.display().to_string(),
        source,
    })
}
