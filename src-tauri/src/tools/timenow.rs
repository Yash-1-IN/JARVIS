use super::{Prepared, Tool, ToolContext, ToolError, ToolOutput, empty_schema, parse_args};
use crate::intelligence::spend::Day;
use crate::ipc::types::ToolRisk;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

/// `time.now`: the current UTC date and time. (The local time zone comes with the platform
/// adapter; the answer says UTC so it is never mistaken for local time.)
pub struct TimeNow;

/// `2026-11-01T07:04:00Z` from Unix seconds.
pub fn format_utc(secs: u64) -> String {
    let d = Day::from_unix_secs(secs);
    let t = secs % 86_400;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        d.year,
        d.month,
        d.day,
        t / 3600,
        (t % 3600) / 60,
        t % 60
    )
}

#[async_trait]
impl Tool for TimeNow {
    fn name(&self) -> &'static str {
        "time.now"
    }
    fn description(&self) -> &'static str {
        "Get the current date and time in UTC."
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
            summary: "Check the current time".into(),
        })
    }
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        _cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        parse_args::<NoArgs>(args)?;
        Ok(ToolOutput {
            text: format!(
                "The current date and time is {} (UTC).",
                format_utc((ctx.now_secs)())
            ),
            external_content: false,
        })
    }
}
