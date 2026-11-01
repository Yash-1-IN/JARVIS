//! The gateway algorithm (spec §9.3). It never runs a tool: it turns a proposal into a
//! verdict (run, ask the user, or refuse), keeps approvals single-use and bound to the
//! exact arguments, and writes the audit trail.

use super::activity::ActivityLog;
use crate::ipc::types::{
    ActivityEntry, ApprovalDecision, ApprovalRequest, Decision, Risk, ToolDecision, ToolOutcome,
    ToolRisk,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// An approval is answered within this time or it expires (spec §9.2).
pub const APPROVAL_TTL_MS: u64 = 60_000;
/// At most this many questions wait at once; more proposals are refused.
pub const MAX_PENDING: usize = 8;

/// The alpha tool registry (spec §9.4). Anything else is refused.
pub const ALPHA_TOOLS: &[&str] = &[
    "system.status",
    "time.now",
    "apps.list_allowed",
    "apps.launch",
    "files.search",
    "files.read_text",
    "files.move",
    "files.rename",
    "memory.propose",
];

/// Never available in alpha (spec §3.2, §9.4), whatever the model asks.
pub const PROHIBITED_PREFIXES: &[&str] = &[
    "shell.",
    "script.",
    "exec.",
    "install.",
    "files.delete",
    "files.write",
    "message.",
    "purchase.",
    "settings.",
    "system.settings",
    "browser.",
];

/// A tool call the model has proposed, already resolved by the tool (paths canonical).
pub struct Proposal {
    pub request_id: String,
    pub tool: String,
    pub base_risk: ToolRisk,
    /// Risk derived from the arguments (for example a large folder), if higher.
    pub arg_risk: Option<ToolRisk>,
    pub targets: Vec<String>,
    /// Human sentence written by the backend, never by the model.
    pub summary: String,
    pub args: Value,
}

/// Permission to run exactly one call. Present it to `verify_args` before executing.
#[derive(Debug, Clone)]
pub struct Ticket {
    pub activity_id: String,
    pub request_id: String,
    pub tool: String,
    pub risk: ToolRisk,
    pub summary: String,
    pub targets: Vec<String>,
    pub decision: ToolDecision,
    pub args_digest: String,
    started_ms: u64,
}

#[derive(Debug)]
pub enum Verdict {
    /// Allowed without asking (low risk, or medium risk auto-allowed while untainted).
    Run(Ticket),
    /// The user must answer the card first.
    Ask(ApprovalRequest),
    /// Refused; the refusal is already in the activity log.
    Deny(String),
}

#[derive(Debug)]
pub enum Resolved {
    Approved(Ticket),
    Denied,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error("this approval is no longer pending; nothing was run")]
    NotPending,
    #[error("this approval expired; nothing was run")]
    Expired,
    #[error("the approval does not match the request; nothing was run")]
    DigestMismatch,
    #[error("this action can only be approved on screen")]
    VoiceNotAllowed,
}

/// Where the answer came from. The command from the window is `Window`; speech is `Voice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Window,
    Voice,
}

struct Pending {
    request: ApprovalRequest,
    risk: ToolRisk,
    args_digest: String,
    created_ms: u64,
}

pub struct Gateway {
    now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
    log: Arc<ActivityLog>,
    tainted: AtomicBool,
    auto_allow_medium: AtomicBool,
    pending: Mutex<HashMap<String, Pending>>,
}

/// SHA-256 over the tool name and the canonical arguments (keys sorted, no whitespace),
/// as lowercase hex. The approval binds to this; a changed argument changes the digest.
pub fn args_digest(tool: &str, args: &Value) -> String {
    let mut s = String::new();
    s.push_str(tool);
    s.push('\n');
    canonical(args, &mut s);
    let hash = Sha256::digest(s.as_bytes());
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap_or_default());
                out.push(':');
                canonical(&m[k], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(x, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

fn max_risk(a: ToolRisk, b: ToolRisk) -> ToolRisk {
    let rank = |r: ToolRisk| match r {
        ToolRisk::Low => 0,
        ToolRisk::Medium => 1,
        ToolRisk::High => 2,
    };
    if rank(b) > rank(a) { b } else { a }
}

fn is_prohibited(tool: &str) -> bool {
    PROHIBITED_PREFIXES.iter().any(|p| tool.starts_with(p))
}

impl Gateway {
    pub fn new(now_ms: Box<dyn Fn() -> u64 + Send + Sync>, log: Arc<ActivityLog>) -> Self {
        // Fail closed: the registry may never contain a prohibited tool.
        assert!(
            ALPHA_TOOLS.iter().all(|t| !is_prohibited(t)),
            "a prohibited tool is in the registry"
        );
        Self {
            now_ms,
            log,
            tainted: AtomicBool::new(false),
            auto_allow_medium: AtomicBool::new(false),
            pending: Mutex::new(HashMap::new()),
        }
    }

    fn pending(&self) -> MutexGuard<'_, HashMap<String, Pending>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_auto_allow_medium(&self, on: bool) {
        self.auto_allow_medium.store(on, Ordering::SeqCst);
    }

    /// Call when a tool returned external content (file text, web page). From then on
    /// every medium or high action asks, whatever the policy (spec §9.2, prompt injection).
    pub fn mark_tainted(&self) {
        self.tainted.store(true, Ordering::SeqCst);
    }

    /// A new conversation starts clean.
    pub fn reset_taint(&self) {
        self.tainted.store(false, Ordering::SeqCst);
    }

    pub fn is_tainted(&self) -> bool {
        self.tainted.load(Ordering::SeqCst)
    }

    /// Questions waiting for an answer, for `Snapshot.pendingApprovals`.
    pub fn pending_requests(&self) -> Vec<ApprovalRequest> {
        self.expire_stale();
        let mut v: Vec<_> = self.pending().values().map(|p| p.request.clone()).collect();
        v.sort_by_key(|r| r.expires_at);
        v
    }

    fn record(
        &self,
        id: String,
        started_ms: u64,
        p: &Proposal,
        risk: ToolRisk,
        decision: ToolDecision,
        outcome: ToolOutcome,
    ) {
        let now = (self.now_ms)();
        self.log.append(ActivityEntry {
            id,
            ts: now,
            request_id: p.request_id.clone(),
            tool: p.tool.clone(),
            risk,
            summary: p.summary.clone(),
            targets: p.targets.clone(),
            decision,
            outcome,
            duration_ms: now.saturating_sub(started_ms),
        });
    }

    fn refuse(&self, p: &Proposal, risk: ToolRisk, decision: ToolDecision, why: &str) -> Verdict {
        let now = (self.now_ms)();
        self.record(
            uuid::Uuid::now_v7().to_string(),
            now,
            p,
            risk,
            decision,
            ToolOutcome::NotRun,
        );
        Verdict::Deny(why.to_owned())
    }

    /// Step 1 to 5 of the algorithm. Arguments are already validated and resolved by the
    /// tool; this decides what to do with them.
    pub fn evaluate(&self, p: Proposal) -> Verdict {
        self.expire_stale();
        let risk = max_risk(p.base_risk, p.arg_risk.unwrap_or(ToolRisk::Low));
        if is_prohibited(&p.tool) {
            return self.refuse(
                &p,
                risk,
                ToolDecision::Prohibited,
                "That action is not allowed.",
            );
        }
        if !ALPHA_TOOLS.contains(&p.tool.as_str()) {
            return self.refuse(
                &p,
                risk,
                ToolDecision::Prohibited,
                "That is not a known tool.",
            );
        }
        let tainted = self.is_tainted();
        let digest = args_digest(&p.tool, &p.args);
        let now = (self.now_ms)();

        let ask = match risk {
            ToolRisk::Low => false,
            ToolRisk::Medium => tainted || !self.auto_allow_medium.load(Ordering::SeqCst),
            ToolRisk::High => true,
        };
        if !ask {
            return Verdict::Run(Ticket {
                activity_id: uuid::Uuid::now_v7().to_string(),
                request_id: p.request_id,
                tool: p.tool,
                risk,
                summary: p.summary,
                targets: p.targets,
                decision: ToolDecision::AutoAllowed,
                args_digest: digest,
                started_ms: now,
            });
        }

        let mut pending = self.pending();
        if pending.len() >= MAX_PENDING {
            drop(pending);
            return self.refuse(
                &p,
                risk,
                ToolDecision::Denied,
                "Too many questions are waiting.",
            );
        }
        let request = ApprovalRequest {
            approval_id: uuid::Uuid::now_v7().to_string(),
            request_id: p.request_id,
            tool: p.tool,
            risk: if risk == ToolRisk::High {
                Risk::High
            } else {
                Risk::Medium
            },
            summary: p.summary,
            targets: p.targets,
            args_digest: digest.clone(),
            expires_at: now + APPROVAL_TTL_MS,
            // Spoken approval is allowed for medium risk only, never high (spec §9.1).
            allow_voice: risk == ToolRisk::Medium,
            tainted,
        };
        pending.insert(
            request.approval_id.clone(),
            Pending {
                request: request.clone(),
                risk,
                args_digest: digest,
                created_ms: now,
            },
        );
        Verdict::Ask(request)
    }

    /// Answers a pending approval. Single-use: the entry is removed on the first answer,
    /// including a wrong one, so a mismatched or late answer can never be retried.
    pub fn resolve(&self, d: ApprovalDecision, via: Channel) -> Result<Resolved, ResolveError> {
        self.expire_stale();
        let mut pending = self.pending();
        let allow_voice = pending
            .get(&d.approval_id)
            .map(|p| p.request.allow_voice)
            .ok_or(ResolveError::NotPending)?;
        if via == Channel::Voice && !allow_voice {
            return Err(ResolveError::VoiceNotAllowed); // stays pending: the user can still click
        }
        let p = pending
            .remove(&d.approval_id)
            .ok_or(ResolveError::NotPending)?;
        drop(pending);
        let now = (self.now_ms)();
        let as_proposal = Proposal {
            request_id: p.request.request_id.clone(),
            tool: p.request.tool.clone(),
            base_risk: p.risk,
            arg_risk: None,
            targets: p.request.targets.clone(),
            summary: p.request.summary.clone(),
            args: Value::Null,
        };
        if now > p.request.expires_at {
            self.record(
                uuid::Uuid::now_v7().to_string(),
                p.created_ms,
                &as_proposal,
                p.risk,
                ToolDecision::Expired,
                ToolOutcome::NotRun,
            );
            return Err(ResolveError::Expired);
        }
        if d.args_digest != p.args_digest {
            self.record(
                uuid::Uuid::now_v7().to_string(),
                p.created_ms,
                &as_proposal,
                p.risk,
                ToolDecision::Denied,
                ToolOutcome::NotRun,
            );
            return Err(ResolveError::DigestMismatch);
        }
        match d.decision {
            Decision::Deny => {
                self.record(
                    uuid::Uuid::now_v7().to_string(),
                    p.created_ms,
                    &as_proposal,
                    p.risk,
                    ToolDecision::Denied,
                    ToolOutcome::NotRun,
                );
                Ok(Resolved::Denied)
            }
            Decision::Approve => Ok(Resolved::Approved(Ticket {
                activity_id: uuid::Uuid::now_v7().to_string(),
                request_id: p.request.request_id,
                tool: p.request.tool,
                risk: p.risk,
                summary: p.request.summary,
                targets: p.request.targets,
                decision: ToolDecision::Approved,
                args_digest: p.args_digest,
                started_ms: p.created_ms,
            })),
        }
    }

    fn expire_stale(&self) {
        let now = (self.now_ms)();
        let mut pending = self.pending();
        let stale: Vec<String> = pending
            .iter()
            .filter(|(_, p)| now > p.request.expires_at)
            .map(|(k, _)| k.clone())
            .collect();
        let removed: Vec<Pending> = stale.iter().filter_map(|k| pending.remove(k)).collect();
        drop(pending);
        for p in removed {
            let prop = Proposal {
                request_id: p.request.request_id,
                tool: p.request.tool,
                base_risk: p.risk,
                arg_risk: None,
                targets: p.request.targets,
                summary: p.request.summary,
                args: Value::Null,
            };
            self.record(
                uuid::Uuid::now_v7().to_string(),
                p.created_ms,
                &prop,
                p.risk,
                ToolDecision::Expired,
                ToolOutcome::NotRun,
            );
        }
    }

    /// Run only if the arguments are exactly the ones that were approved (bait and switch).
    pub fn verify_args(&self, t: &Ticket, args: &Value) -> bool {
        args_digest(&t.tool, args) == t.args_digest
    }

    /// Records how the call ended. Call exactly once per ticket.
    pub fn finish(&self, t: Ticket, outcome: ToolOutcome) {
        let now = (self.now_ms)();
        self.log.append(ActivityEntry {
            id: t.activity_id,
            ts: now,
            request_id: t.request_id,
            tool: t.tool,
            risk: t.risk,
            summary: t.summary,
            targets: t.targets,
            decision: t.decision,
            outcome,
            duration_ms: now.saturating_sub(t.started_ms),
        });
    }
}

#[cfg(test)]
mod tests;
