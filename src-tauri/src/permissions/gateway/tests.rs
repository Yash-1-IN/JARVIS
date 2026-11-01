use super::*;
use serde_json::json;
use std::sync::atomic::AtomicU64;

struct Rig {
    gw: Gateway,
    log: Arc<ActivityLog>,
    clock: Arc<AtomicU64>,
}

fn rig() -> Rig {
    let clock = Arc::new(AtomicU64::new(1_000_000));
    let c = clock.clone();
    let log = Arc::new(ActivityLog::new(1000));
    let gw = Gateway::new(Box::new(move || c.load(Ordering::SeqCst)), log.clone());
    Rig { gw, log, clock }
}

fn proposal(tool: &str, base: ToolRisk, args: Value) -> Proposal {
    Proposal {
        request_id: "req-1".into(),
        tool: tool.into(),
        base_risk: base,
        arg_risk: None,
        targets: vec!["/home/u/Documents/report.pdf".into()],
        summary: format!("Run {tool}"),
        args,
    }
}

fn approve(r: &ApprovalRequest) -> ApprovalDecision {
    ApprovalDecision {
        approval_id: r.approval_id.clone(),
        args_digest: r.args_digest.clone(),
        decision: Decision::Approve,
    }
}

fn ask(v: Verdict) -> ApprovalRequest {
    match v {
        Verdict::Ask(r) => r,
        other => panic!("expected a question, got {other:?}"),
    }
}

#[test]
fn low_risk_runs_without_asking_and_is_logged_on_finish() {
    let g = rig();
    let Verdict::Run(t) =
        g.gw.evaluate(proposal("time.now", ToolRisk::Low, json!({})))
    else {
        panic!("low risk should run");
    };
    assert_eq!(t.decision, ToolDecision::AutoAllowed);
    assert_eq!(g.log.len(), 0, "logged when it finishes, not before");
    g.clock.fetch_add(40, Ordering::SeqCst);
    g.gw.finish(t, ToolOutcome::Ok);
    let e = &g.log.list(10, None)[0];
    assert_eq!(
        (e.decision, e.outcome, e.duration_ms),
        (ToolDecision::AutoAllowed, ToolOutcome::Ok, 40)
    );
}

#[test]
fn unknown_and_prohibited_tools_are_refused_and_logged_as_not_run() {
    let g = rig();
    for (tool, decision) in [
        ("shell.run", ToolDecision::Prohibited),
        ("files.delete", ToolDecision::Prohibited),
        ("files.delete_everything", ToolDecision::Prohibited),
        ("browser.open", ToolDecision::Prohibited),
        ("totally.made.up", ToolDecision::Prohibited),
    ] {
        assert!(
            matches!(
                g.gw.evaluate(proposal(tool, ToolRisk::Low, json!({}))),
                Verdict::Deny(_)
            ),
            "{tool}"
        );
        let e = g.log.list(1, None).remove(0);
        assert_eq!(
            (e.tool.as_str(), e.decision, e.outcome),
            (tool, decision, ToolOutcome::NotRun)
        );
    }
    assert!(g.gw.pending_requests().is_empty());
}

#[test]
fn the_registry_never_contains_a_prohibited_tool() {
    assert!(ALPHA_TOOLS.iter().all(|t| !is_prohibited(t)));
}

#[test]
fn medium_asks_by_default_and_high_always_asks() {
    let g = rig();
    let m = ask(g
        .gw
        .evaluate(proposal("files.move", ToolRisk::Medium, json!({"to": "a"}))));
    assert_eq!(
        (m.risk, m.allow_voice, m.tainted),
        (Risk::Medium, true, false)
    );
    assert_eq!(m.expires_at, 1_000_000 + APPROVAL_TTL_MS);
    g.gw.set_auto_allow_medium(true);
    let h = ask(g
        .gw
        .evaluate(proposal("apps.launch", ToolRisk::High, json!({"app": "x"}))));
    assert_eq!(
        (h.risk, h.allow_voice),
        (Risk::High, false),
        "auto-allow never covers high risk"
    );
}

#[test]
fn argument_risk_can_raise_but_not_lower_the_risk() {
    let g = rig();
    let mut p = proposal("files.search", ToolRisk::Low, json!({}));
    p.arg_risk = Some(ToolRisk::High);
    assert_eq!(ask(g.gw.evaluate(p)).risk, Risk::High);
    let mut p = proposal("files.move", ToolRisk::Medium, json!({}));
    p.arg_risk = Some(ToolRisk::Low);
    assert_eq!(ask(g.gw.evaluate(p)).risk, Risk::Medium);
}

#[test]
fn medium_runs_unasked_only_when_policy_allows_and_nothing_is_tainted() {
    let g = rig();
    g.gw.set_auto_allow_medium(true);
    assert!(matches!(
        g.gw.evaluate(proposal("files.move", ToolRisk::Medium, json!({}))),
        Verdict::Run(_)
    ));
    g.gw.mark_tainted();
    let r = ask(g
        .gw
        .evaluate(proposal("files.move", ToolRisk::Medium, json!({}))));
    assert!(
        r.tainted,
        "the card says untrusted content is in the context"
    );
    g.gw.reset_taint();
    assert!(matches!(
        g.gw.evaluate(proposal("files.move", ToolRisk::Medium, json!({}))),
        Verdict::Run(_)
    ));
}

#[test]
fn injection_a_file_that_asks_for_a_move_never_auto_runs() {
    // The user allowed medium actions to run unasked. The model reads a file (that read is
    // allowed), the content enters the context and taints it, and the file's text makes the
    // model propose a move: that must become an approval card, never an automatic run.
    let g = rig();
    g.gw.set_auto_allow_medium(true);
    let Verdict::Run(read) = g.gw.evaluate(proposal(
        "files.read_text",
        ToolRisk::Medium,
        json!({"path": "/n.txt"}),
    )) else {
        panic!("an untainted medium read is auto-allowed under this policy");
    };
    g.gw.mark_tainted(); // the tool returned external content
    g.gw.finish(read, ToolOutcome::Ok);
    let card = ask(g.gw.evaluate(proposal(
        "files.move",
        ToolRisk::Medium,
        json!({"from": "~/Documents", "to": "/tmp/x"}),
    )));
    assert!(card.tainted);
    assert!(
        g.gw.pending_requests().len() == 1,
        "nothing ran; the user is asked"
    );
}

#[test]
fn approval_is_single_use() {
    let g = rig();
    let r = ask(g
        .gw
        .evaluate(proposal("files.move", ToolRisk::Medium, json!({"a": 1}))));
    assert!(matches!(
        g.gw.resolve(approve(&r), Channel::Window),
        Ok(Resolved::Approved(_))
    ));
    assert_eq!(
        g.gw.resolve(approve(&r), Channel::Window).unwrap_err(),
        ResolveError::NotPending
    );
    assert!(g.gw.pending_requests().is_empty());
}

#[test]
fn a_wrong_digest_is_rejected_and_burns_the_approval() {
    let g = rig();
    let r = ask(g
        .gw
        .evaluate(proposal("files.move", ToolRisk::Medium, json!({"a": 1}))));
    let mut bad = approve(&r);
    bad.args_digest = args_digest("files.move", &json!({"a": 2}));
    assert_eq!(
        g.gw.resolve(bad, Channel::Window).unwrap_err(),
        ResolveError::DigestMismatch
    );
    // Failing closed: the right digest afterwards is too late, the model must propose again.
    assert_eq!(
        g.gw.resolve(approve(&r), Channel::Window).unwrap_err(),
        ResolveError::NotPending
    );
    let e = g.log.list(1, None).remove(0);
    assert_eq!(
        (e.decision, e.outcome),
        (ToolDecision::Denied, ToolOutcome::NotRun)
    );
}

#[test]
fn changing_the_arguments_after_approval_fails_verification() {
    let g = rig();
    let args = json!({"from": "/a", "to": "/b"});
    let r = ask(g
        .gw
        .evaluate(proposal("files.move", ToolRisk::Medium, args.clone())));
    let Ok(Resolved::Approved(t)) = g.gw.resolve(approve(&r), Channel::Window) else {
        panic!()
    };
    assert!(g.gw.verify_args(&t, &args));
    assert!(
        g.gw.verify_args(&t, &json!({"to": "/b", "from": "/a"})),
        "key order does not matter"
    );
    assert!(
        !g.gw
            .verify_args(&t, &json!({"from": "/a", "to": "/elsewhere"}))
    );
    assert!(
        !g.gw
            .verify_args(&t, &json!({"from": "/a", "to": "/b", "extra": true}))
    );
}

#[test]
fn digest_covers_the_tool_name_and_canonical_form() {
    let a = args_digest("files.move", &json!({"x": [1, 2], "y": {"b": 1, "a": 2}}));
    let b = args_digest("files.move", &json!({"y": {"a": 2, "b": 1}, "x": [1, 2]}));
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
    assert_ne!(
        a,
        args_digest("files.rename", &json!({"x": [1, 2], "y": {"b": 1, "a": 2}}))
    );
    assert_ne!(
        a,
        args_digest("files.move", &json!({"x": [2, 1], "y": {"b": 1, "a": 2}}))
    );
}

#[test]
fn approval_expires_after_sixty_seconds() {
    let g = rig();
    let r = ask(g
        .gw
        .evaluate(proposal("files.move", ToolRisk::Medium, json!({}))));
    g.clock.fetch_add(APPROVAL_TTL_MS, Ordering::SeqCst); // exactly at the limit: still valid
    assert!(g.gw.pending_requests().len() == 1);
    g.clock.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        g.gw.resolve(approve(&r), Channel::Window).unwrap_err(),
        ResolveError::NotPending
    );
    let e = g.log.list(1, None).remove(0);
    assert_eq!(
        (e.decision, e.outcome),
        (ToolDecision::Expired, ToolOutcome::NotRun)
    );
}

#[test]
fn late_answer_is_rejected_even_if_nothing_purged_it_first() {
    let g = rig();
    let r = ask(g
        .gw
        .evaluate(proposal("files.move", ToolRisk::Medium, json!({}))));
    g.clock.fetch_add(APPROVAL_TTL_MS + 5, Ordering::SeqCst);
    let res = g.gw.resolve(approve(&r), Channel::Window);
    assert!(matches!(
        res,
        Err(ResolveError::NotPending | ResolveError::Expired)
    ));
}

#[test]
fn voice_cannot_approve_high_risk_but_can_approve_medium() {
    let g = rig();
    let high = ask(g
        .gw
        .evaluate(proposal("apps.launch", ToolRisk::High, json!({"a": 1}))));
    assert_eq!(
        g.gw.resolve(approve(&high), Channel::Voice).unwrap_err(),
        ResolveError::VoiceNotAllowed
    );
    // The refusal did not consume it: the user can still click.
    assert!(matches!(
        g.gw.resolve(approve(&high), Channel::Window),
        Ok(Resolved::Approved(_))
    ));
    let medium = ask(g
        .gw
        .evaluate(proposal("files.move", ToolRisk::Medium, json!({"a": 1}))));
    assert!(matches!(
        g.gw.resolve(approve(&medium), Channel::Voice),
        Ok(Resolved::Approved(_))
    ));
}

#[test]
fn deny_runs_nothing_and_is_logged() {
    let g = rig();
    let r = ask(g
        .gw
        .evaluate(proposal("files.move", ToolRisk::Medium, json!({}))));
    let mut d = approve(&r);
    d.decision = Decision::Deny;
    assert!(matches!(
        g.gw.resolve(d, Channel::Window),
        Ok(Resolved::Denied)
    ));
    let e = g.log.list(1, None).remove(0);
    assert_eq!(
        (e.decision, e.outcome),
        (ToolDecision::Denied, ToolOutcome::NotRun)
    );
}

#[test]
fn unknown_approval_id_is_not_pending() {
    let g = rig();
    let d = ApprovalDecision {
        approval_id: "nope".into(),
        args_digest: "x".into(),
        decision: Decision::Approve,
    };
    assert_eq!(
        g.gw.resolve(d, Channel::Window).unwrap_err(),
        ResolveError::NotPending
    );
}

#[test]
fn too_many_waiting_questions_are_refused() {
    let g = rig();
    for i in 0..MAX_PENDING {
        ask(g
            .gw
            .evaluate(proposal("files.move", ToolRisk::Medium, json!({"n": i}))));
    }
    assert!(matches!(
        g.gw.evaluate(proposal("files.move", ToolRisk::Medium, json!({"n": 99}))),
        Verdict::Deny(_)
    ));
    assert_eq!(g.gw.pending_requests().len(), MAX_PENDING);
}

#[test]
fn every_call_ends_up_in_the_audit_log_without_contents() {
    let g = rig();
    let secret_args = json!({"path": "/home/u/diary.txt", "note": "TOP SECRET CONTENTS"});
    let r = ask(g.gw.evaluate(proposal(
        "files.read_text",
        ToolRisk::Medium,
        secret_args.clone(),
    )));
    let Ok(Resolved::Approved(t)) = g.gw.resolve(approve(&r), Channel::Window) else {
        panic!()
    };
    g.gw.finish(t, ToolOutcome::Ok);
    g.gw.evaluate(proposal("shell.run", ToolRisk::Low, json!({})));
    let rows = g.log.list(10, None);
    assert_eq!(rows.len(), 2);
    let dump = serde_json::to_string(&rows).unwrap();
    assert!(
        !dump.contains("TOP SECRET") && !dump.contains("diary"),
        "{dump}"
    );
    assert!(
        rows.iter()
            .all(|e| e.request_id == "req-1" && !e.id.is_empty())
    );
}
