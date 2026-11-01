//! `system.status` (spec §9.4): CPU, memory, battery and network, read through the platform
//! adapter. Read-only, no arguments.

use super::ports::SystemStatus;
use super::{Prepared, Tool, ToolContext, ToolError, ToolOutput, empty_schema, parse_args};
use crate::ipc::types::ToolRisk;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

pub struct SystemStatusTool;

pub fn describe(s: &SystemStatus) -> String {
    let mut lines = Vec::new();
    if let Some(c) = s.cpu_percent {
        lines.push(format!("CPU: {c:.0}% busy"));
    }
    if let (Some(u), Some(t)) = (s.memory_used_mb, s.memory_total_mb) {
        lines.push(format!("Memory: {u} MB used of {t} MB"));
    }
    if let Some(b) = s.battery_percent {
        let state = match s.charging {
            Some(true) => ", charging",
            Some(false) => ", on battery",
            None => "",
        };
        lines.push(format!("Battery: {b}%{state}"));
    }
    if let Some(o) = s.online {
        lines.push(format!("Network: {}", if o { "online" } else { "offline" }));
    }
    if lines.is_empty() {
        "This device did not report any status.".into()
    } else {
        lines.join("\n")
    }
}

#[async_trait]
impl Tool for SystemStatusTool {
    fn name(&self) -> &'static str {
        "system.status"
    }
    fn description(&self) -> &'static str {
        "Read CPU load, memory use, battery and network state of this computer."
    }
    fn input_schema(&self) -> Value {
        empty_schema()
    }
    fn base_risk(&self) -> ToolRisk {
        ToolRisk::Low
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(5)
    }
    fn prepare(&self, args: &Value, _ctx: &ToolContext) -> Result<Prepared, ToolError> {
        parse_args::<NoArgs>(args)?;
        Ok(Prepared {
            arg_risk: None,
            targets: vec![],
            summary: "Check system status".into(),
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
        let source = ctx.ports.status.clone();
        let sample = tokio::task::spawn_blocking(move || source.sample())
            .await
            .map_err(|_| ToolError::Failed("The status could not be read.".into()))?;
        let Some(s) = sample else {
            return Err(ToolError::Unavailable(
                "System status is not available in this build yet.".into(),
            ));
        };
        Ok(ToolOutput {
            text: describe(&s),
            external_content: false,
        })
    }
}
