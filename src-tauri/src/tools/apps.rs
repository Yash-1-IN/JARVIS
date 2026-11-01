//! `apps.list_allowed` and `apps.launch` (spec §9.4). Only apps the user ticked in settings
//! can be launched, matched by exact stable id (never by name), with fixed arguments.

use super::{
    Prepared, Tool, ToolContext, ToolError, ToolOutput, clean_for_summary, empty_schema, parse_args,
};
use crate::ipc::types::{AllowedApp, ToolRisk};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchArgs {
    id: String,
}

fn find<'a>(ctx: &'a ToolContext, id: &str) -> Result<&'a AllowedApp, ToolError> {
    ctx.apps
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| ToolError::NotAllowed("That app is not on the allowed list.".into()))
}

pub struct AppsListAllowed;

#[async_trait]
impl Tool for AppsListAllowed {
    fn name(&self) -> &'static str {
        "apps.list_allowed"
    }
    fn description(&self) -> &'static str {
        "List the apps the user has allowed JARVIS to open, with the id to pass to apps.launch."
    }
    fn input_schema(&self) -> Value {
        empty_schema()
    }
    fn base_risk(&self) -> ToolRisk {
        ToolRisk::Low
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(2)
    }
    fn prepare(&self, args: &Value, _ctx: &ToolContext) -> Result<Prepared, ToolError> {
        parse_args::<NoArgs>(args)?;
        Ok(Prepared {
            arg_risk: None,
            targets: vec![],
            summary: "List the apps you allowed".into(),
        })
    }
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        parse_args::<NoArgs>(args)?;
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let text = if ctx.apps.is_empty() {
            "No apps are allowed yet. The user can allow apps in settings.".to_owned()
        } else {
            ctx.apps
                .iter()
                .map(|a| {
                    format!(
                        "{} (id: {})",
                        clean_for_summary(&a.name),
                        clean_for_summary(&a.id)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        Ok(ToolOutput {
            text,
            external_content: false,
        })
    }
}

pub struct AppsLaunch;

#[async_trait]
impl Tool for AppsLaunch {
    fn name(&self) -> &'static str {
        "apps.launch"
    }
    fn description(&self) -> &'static str {
        "Open an app the user has allowed. Pass the exact id from apps.list_allowed. No arguments or files can be passed."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string", "description": "The app id from apps.list_allowed." } },
            "required": ["id"],
            "additionalProperties": false
        })
    }
    fn base_risk(&self) -> ToolRisk {
        ToolRisk::Low
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }
    fn prepare(&self, args: &Value, ctx: &ToolContext) -> Result<Prepared, ToolError> {
        let a: LaunchArgs = parse_args(args)?;
        let app = find(ctx, &a.id)?;
        Ok(Prepared {
            arg_risk: None,
            targets: vec![app.id.clone()],
            summary: format!("Open {}", clean_for_summary(&app.name)),
        })
    }
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let a: LaunchArgs = parse_args(args)?;
        // Checked again here: the allow-list may have changed since the proposal.
        let app = find(ctx, &a.id)?.clone();
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let launcher = ctx.ports.launcher.clone();
        let id = app.id.clone();
        tokio::task::spawn_blocking(move || launcher.launch(&id))
            .await
            .map_err(|_| ToolError::Failed("The app could not be opened.".into()))?
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::Unsupported => ToolError::Unavailable(
                    "Opening apps is not available in this build yet.".into(),
                ),
                _ => ToolError::Failed("The app could not be opened.".into()),
            })?;
        Ok(ToolOutput {
            text: format!("Opened {}.", clean_for_summary(&app.name)),
            external_content: false,
        })
    }
}
