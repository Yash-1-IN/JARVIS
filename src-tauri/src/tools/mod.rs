//! Tools (spec §9.4, §9.5). A tool never runs on its own: it turns the model's arguments into
//! a `Prepared` proposal (strictly validated, paths resolved against the allowed folders), the
//! permission gateway decides, and only then does `execute` run it. Tools re-check their
//! resources immediately before acting, so a path swapped after approval is caught.
//!
//! Nothing here is callable by the webview; the turn runner is the only caller.

// Wired into the turn runner by the next M4 PR.
#![allow(dead_code)]

pub mod adapters;
pub mod apps;
pub mod files;
pub mod memory;
pub mod ports;
pub mod status;
pub mod timenow;

use crate::intelligence::provider::ToolSpec;
use crate::ipc::types::{ToolOutcome, ToolRisk};
use crate::permissions::gateway::{ALPHA_TOOLS, Proposal};
use crate::permissions::paths::PathScope;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    /// The arguments are malformed or break a rule. Safe to show to the user and the model; serde's messages can include the offending value.
    #[error("{0}")]
    Invalid(String),
    /// Outside the allowed folders, or an app that is not on the allow-list.
    #[error("{0}")]
    NotAllowed(String),
    #[error("{0}")]
    Failed(String),
    #[error("the tool timed out")]
    Timeout,
    #[error("the tool was cancelled")]
    Cancelled,
    #[error("{0}")]
    Unavailable(String),
}

impl ToolError {
    pub fn outcome(&self) -> ToolOutcome {
        match self {
            Self::Timeout => ToolOutcome::TimedOut,
            Self::Cancelled => ToolOutcome::Cancelled,
            Self::Invalid(_) | Self::NotAllowed(_) | Self::Unavailable(_) => ToolOutcome::NotRun,
            Self::Failed(_) => ToolOutcome::Failed,
        }
    }
}

/// What a tool resolved from its arguments, for the gateway and the approval card.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// Risk derived from the arguments (for example a whole folder), if higher.
    pub arg_risk: Option<ToolRisk>,
    /// Exact paths or app ids, in the form the tool will use.
    pub targets: Vec<String>,
    /// A sentence the backend writes (never the model), shown on the approval card.
    pub summary: String,
}

#[derive(Debug)]
pub struct ToolOutput {
    /// What the model sees. The turn runner wraps it as untrusted data.
    pub text: String,
    /// True when the text came from outside (file contents, file names): the gateway then
    /// treats the context as tainted.
    pub external_content: bool,
}

/// What a tool may touch, built fresh from the saved settings for each call.
pub struct ToolContext {
    pub scope: PathScope,
    pub now_secs: Arc<dyn Fn() -> u64 + Send + Sync>,
    /// The apps the user allowed in settings (ids come from the platform's installed list).
    pub apps: Vec<crate::ipc::types::AllowedApp>,
    pub ports: Arc<ports::Ports>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// JSON Schema shown to the model; `additionalProperties` is always false.
    fn input_schema(&self) -> Value;
    fn base_risk(&self) -> ToolRisk;
    fn timeout(&self) -> Duration;
    /// Validates the arguments strictly and resolves resources. Does not act.
    fn prepare(&self, args: &Value, ctx: &ToolContext) -> Result<Prepared, ToolError>;
    /// Acts. Must re-check every resource again, and stop promptly when `cancel` fires.
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError>;
}

/// Strict argument parsing: unknown fields are an error (each Args struct is
/// `deny_unknown_fields`) and the message never contains the offending value.
pub fn parse_args<T: DeserializeOwned>(args: &Value) -> Result<T, ToolError> {
    serde_json::from_value(args.clone()).map_err(|e| {
        // serde's message names fields and expected types, not the data itself.
        ToolError::Invalid(format!("Invalid arguments: {e}"))
    })
}

/// Shortens and cleans text the backend puts in a summary (file names come from disk).
pub fn clean_for_summary(s: &str) -> String {
    let cleaned: String = s.chars().filter(|c| !c.is_control()).take(120).collect();
    cleaned
}

/// A schema for a tool with no arguments.
pub fn empty_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

pub struct Registry {
    tools: BTreeMap<&'static str, Arc<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            tools: BTreeMap::new(),
        }
    }

    /// Only tools the gateway knows can be registered, so the two lists cannot drift.
    pub fn register(&mut self, t: Arc<dyn Tool>) {
        assert!(
            ALPHA_TOOLS.contains(&t.name()),
            "{} is not in the gateway's alpha registry",
            t.name()
        );
        self.tools.insert(t.name(), t);
    }

    /// The tools the model is offered.
    pub fn wired() -> Self {
        let mut r = Self::new();
        r.register(Arc::new(timenow::TimeNow));
        r.register(Arc::new(files::FilesSearch));
        r.register(Arc::new(files::FilesReadText));
        r.register(Arc::new(files::FilesMove));
        r.register(Arc::new(files::FilesRename));
        r.register(Arc::new(apps::AppsListAllowed));
        r.register(Arc::new(apps::AppsLaunch));
        r.register(Arc::new(status::SystemStatusTool));
        r.register(Arc::new(memory::MemoryPropose));
        r
    }

    /// Every tool implemented so far (tests, and the future full set).
    pub fn alpha() -> Self {
        Self::wired()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.tools.keys().copied().collect()
    }

    /// What the model is told it can call.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools
            .values()
            .map(|t| ToolSpec {
                name: t.name().to_owned(),
                description: t.description().to_owned(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Prepares a call and turns it into the gateway's proposal.
    pub fn propose(
        &self,
        request_id: &str,
        tool: &str,
        args: Value,
        ctx: &ToolContext,
    ) -> Result<Proposal, ToolError> {
        let t = self
            .get(tool)
            .ok_or_else(|| ToolError::NotAllowed("That is not a known tool.".into()))?;
        let p = t.prepare(&args, ctx)?;
        Ok(Proposal {
            request_id: request_id.to_owned(),
            tool: t.name().to_owned(),
            base_risk: t.base_risk(),
            arg_risk: p.arg_risk,
            targets: p.targets,
            summary: p.summary,
            args,
        })
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs an approved call with its timeout and cancellation. The caller has already passed the
/// gateway's `verify_args`.
pub async fn execute(
    tool: &dyn Tool,
    args: &Value,
    ctx: &ToolContext,
    cancel: &CancellationToken,
) -> Result<ToolOutput, ToolError> {
    let child = cancel.child_token();
    let run = tool.run(args, ctx, child.clone());
    let out = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(ToolError::Cancelled),
        r = tokio::time::timeout(tool.timeout(), run) => r.unwrap_or(Err(ToolError::Timeout)),
    };
    // Dropping the future on a timeout does not stop blocking work it started (a folder
    // walk); the token does. Cancelling after a normal finish is harmless.
    child.cancel();
    out
}

#[cfg(test)]
mod tests;
