//! Answers the tool-shaped quick questions (time, system status, open an app, find a file)
//! without the model, for when the cloud cannot be used (spend cap, no key, no network).
//! The call goes through the same path as a model's: `Registry::propose`, the permission
//! gateway, `execute`, and the activity log. Only the wording is local.

use super::{CancellationToken, ToolWiring, TurnRunner, TurnSink};
use crate::intelligence::intents::Intent;
use crate::ipc::types::{StopReason, TurnStreamEvent};
use serde_json::{Value, json};

enum Plan {
    Call(&'static str, Value),
    /// No call: the answer is already known (an unknown or ambiguous app name).
    Say(String),
}

/// The one allowed app that `name` matches (case-insensitive, part of its name), or why not.
fn pick_app(w: &ToolWiring, name: &str) -> Plan {
    let wanted = name.trim().to_lowercase();
    let apps = (w.context)().apps;
    let hits: Vec<_> = apps
        .iter()
        .filter(|a| !wanted.is_empty() && a.name.to_lowercase().contains(&wanted))
        .collect();
    match hits.as_slice() {
        [one] => Plan::Call("apps.launch", json!({ "id": one.id })),
        [] => Plan::Say(format!(
            "I can't find an allowed app called \"{}\". You can allow apps in Settings.",
            name.chars()
                .filter(|c| !c.is_control())
                .take(60)
                .collect::<String>()
        )),
        many => Plan::Say(format!(
            "Which one do you mean: {}?",
            many.iter()
                .map(|a| a
                    .name
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(60)
                    .collect::<String>())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn plan(w: &ToolWiring, intent: &Intent) -> Option<Plan> {
    Some(match intent {
        Intent::TimeNow | Intent::DateToday => Plan::Call("time.now", json!({})),
        Intent::SystemStatus => Plan::Call("system.status", json!({})),
        Intent::LaunchApp(name) => pick_app(w, name),
        Intent::FindFile(q) => Plan::Call("files.search", json!({ "query": q })),
        _ => return None,
    })
}

impl TurnRunner {
    /// Returns true if the turn was finished here. False means this intent has no local
    /// answer (or tools are not wired) and the caller reports the original problem.
    pub(super) async fn answer_with_tools(
        &self,
        id: &str,
        intent: &Intent,
        cancel: &CancellationToken,
        sink: &dyn TurnSink,
    ) -> bool {
        let Some(w) = self.tools.as_ref() else {
            return false;
        };
        match plan(w, intent) {
            None => false,
            Some(Plan::Say(reply)) => {
                self.finish_text(id, &reply, sink);
                true
            }
            Some(Plan::Call(name, args)) => {
                match self.run_tool_call(w, id, name, args, cancel, sink).await {
                    // Cancelled while waiting: `cancel` already returned the phase to idle.
                    None => sink.event(TurnStreamEvent::Done {
                        stop_reason: StopReason::Cancelled,
                    }),
                    Some(r) => self.finish_text(id, &r.text, sink),
                }
                true
            }
        }
    }

    /// "Remember that ...": the text becomes a proposed memory and the window shows the
    /// confirmation card. Nothing is kept until the user accepts it.
    pub(super) fn remember(&self, text: &str) -> String {
        let Some(w) = self.tools.as_ref() else {
            return "Memory is not available in this build.".into();
        };
        match (w.context)().ports.memory.propose(text) {
            Ok(_) => "Okay. Confirm it on the card and I will keep it.".into(),
            Err(why) => why,
        }
    }
}
