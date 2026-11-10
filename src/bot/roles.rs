// Ported from Kira (github.com/horizon-rust/kira, src/features/community.js `onButton`), MIT © 2026 Ludovic Marie and the Kira contributors; see LICENSES/kira-MIT.txt
//! The role buttons: a click toggles the role if the file lets members have it.
use poise::serenity_prelude as serenity;

use super::{Data, Error};
use crate::{
    config::ServerConfig,
    ids::Snowflake,
    kit::{
        plan::Known,
        roles::{Click, click},
    },
};

/// Adds and removes a role of the member who clicked.
#[allow(async_fn_in_trait)]
pub trait RoleEditor {
    async fn add(&self, role: Snowflake) -> Result<(), String>;
    async fn remove(&self, role: Snowflake) -> Result<(), String>;
}

/// What to answer a click with (only the clicker sees it); `None` if the button is not a role button.
pub async fn respond<E: RoleEditor>(
    config: &ServerConfig,
    known: &Known,
    custom_id: &str,
    member_roles: &[Snowflake],
    editor: &E,
) -> Option<String> {
    Some(match click(config, known, custom_id, member_roles) {
        Click::NotOurs => return None,
        Click::Gone => "That role is gone.".to_string(),
        Click::Toggle { role, name, has } => {
            let done = if has {
                editor.remove(role).await
            } else {
                editor.add(role).await
            };
            match done {
                Ok(()) => format!("{} **{name}**.", if has { "Removed" } else { "Added" }),
                Err(why) => {
                    tracing::warn!(%why, role = %name, "role button failed");
                    "I could not change your roles. Ask a team member.".to_string()
                }
            }
        }
    })
}

struct Http<'a> {
    http: &'a serenity::Http,
    guild: serenity::GuildId,
    user: serenity::UserId,
}

impl RoleEditor for Http<'_> {
    async fn add(&self, role: Snowflake) -> Result<(), String> {
        self.http
            .add_member_role(
                self.guild,
                self.user,
                serenity::RoleId::new(role.0),
                Some("role button"),
            )
            .await
            .map_err(|e| e.to_string())
    }

    async fn remove(&self, role: Snowflake) -> Result<(), String> {
        self.http
            .remove_member_role(
                self.guild,
                self.user,
                serenity::RoleId::new(role.0),
                Some("role button"),
            )
            .await
            .map_err(|e| e.to_string())
    }
}

pub async fn on_click(
    ctx: &serenity::Context,
    data: &Data,
    click: &serenity::ComponentInteraction,
) -> Result<(), Error> {
    if click.guild_id != Some(data.guild) {
        return Ok(());
    }
    let roles: Vec<Snowflake> = click
        .member
        .as_ref()
        .map(|m| m.roles.iter().map(|r| Snowflake(r.get())).collect())
        .unwrap_or_default();
    let known = data.known().await?;
    let editor = Http {
        http: &ctx.http,
        guild: data.guild,
        user: click.user.id,
    };
    let Some(text) = respond(
        &data.config.server,
        &known,
        &click.data.custom_id,
        &roles,
        &editor,
    )
    .await
    else {
        return Ok(());
    };
    tracing::info!(user = %click.user.id, button = %click.data.custom_id, answer = %text, "role button");
    click
        .create_response(
            &ctx.http,
            serenity::CreateInteractionResponse::Message(
                serenity::CreateInteractionResponseMessage::new()
                    .content(text)
                    .ephemeral(true),
            ),
        )
        .await?;
    Ok(())
}
