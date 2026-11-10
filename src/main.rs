use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sunrise-bot", version, about = "Sunny, Sunrise's Discord bot")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Connect to Discord and serve
    Run,
    /// Compare the server with config/server.json and print the plan; --apply makes it so
    Bootstrap {
        #[arg(long)]
        apply: bool,
        /// With --apply: also delete channels the bot made that left the file
        #[arg(long, requires = "apply")]
        prune: bool,
    },
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    match Cli::parse().command {
        Command::Run => tracing::info!("run: not implemented yet"),
        Command::Bootstrap { apply, prune } => {
            tracing::info!(apply, prune, "bootstrap: not implemented yet")
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_needs_apply() {
        assert!(Cli::try_parse_from(["sunrise-bot", "bootstrap", "--prune"]).is_err());
        assert!(Cli::try_parse_from(["sunrise-bot", "bootstrap", "--apply", "--prune"]).is_ok());
        assert!(Cli::try_parse_from(["sunrise-bot", "bootstrap"]).is_ok());
    }
}
