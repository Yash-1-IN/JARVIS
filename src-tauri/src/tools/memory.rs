//! `memory.propose` (spec §10): the model may suggest something worth remembering. It is only
//! ever stored as `proposed`; nothing reaches a prompt until the user approves it (a later
//! milestone). Today proposals are kept in memory only and are lost when the app closes.

use super::{Prepared, Tool, ToolContext, ToolError, ToolOutput, parse_args};
use crate::ipc::types::ToolRisk;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const MAX_CHARS: usize = 500;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeArgs {
    text: String,
}

/// One line, no control characters, trimmed and within the limit; or why not.
fn clean(text: &str) -> Result<String, ToolError> {
    let t: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return Err(ToolError::Invalid("The memory text is empty.".into()));
    }
    if t.chars().count() > MAX_CHARS {
        return Err(ToolError::Invalid(format!(
            "The memory text is longer than {MAX_CHARS} characters."
        )));
    }
    Ok(t)
}

pub struct MemoryPropose;

#[async_trait]
impl Tool for MemoryPropose {
    fn name(&self) -> &'static str {
        "memory.propose"
    }
    fn description(&self) -> &'static str {
        "Suggest a short fact about the user worth remembering. It is only a proposal: the user must approve it before it is ever used."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "text": { "type": "string", "description": "One short fact, at most 500 characters." } },
            "required": ["text"],
            "additionalProperties": false
        })
    }
    fn base_risk(&self) -> ToolRisk {
        ToolRisk::Low
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(2)
    }
    fn prepare(&self, args: &Value, _ctx: &ToolContext) -> Result<Prepared, ToolError> {
        let a: ProposeArgs = parse_args(args)?;
        clean(&a.text)?;
        // The summary never repeats the model's text.
        Ok(Prepared {
            arg_risk: None,
            targets: vec![],
            summary: "Suggest something to remember (needs your approval)".into(),
        })
    }
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let a: ProposeArgs = parse_args(args)?;
        let text = clean(&a.text)?;
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        ctx.ports.memory.propose(&text).map_err(ToolError::Failed)?;
        Ok(ToolOutput {
            text: "Saved as a proposal. It will not be used until the user approves it.".into(),
            external_content: false,
        })
    }
}
