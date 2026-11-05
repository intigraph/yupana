use super::*;
use serde_json::json;

fn policy() -> OutboundPolicy {
    serde_json::from_value(json!({
        "revision":"a".repeat(64), "approval_public_key":"b".repeat(64),
        "gate_public_key":"c".repeat(64), "max_age_secs":60,
        "non_send_tools":["mcp__gmail__create_draft"],
        "send_adapters":[{"tool_name":"mcp__gmail__send_message", "text_pointer":"/body",
            "recipient_pointer":"/to", "thread_pointer":"/thread_id", "thread_required":false, "literal_fields":["subject","cc","bcc"]}]
    }))
    .unwrap()
}

fn send() -> Value {
    json!({"tool_name":"mcp__gmail__send_message", "tool_input":{
        "to":["fixture@example.invalid"], "body":"Fixture A", "thread_id":"fixture-thread", "subject":"Fixture"}})
}

fn binding(payload: &Value) -> Binding {
    let Selection::Send(binding) = select(&policy(), payload) else {
        panic!("expected fixture send");
    };
    binding
}

#[test]
fn unapproved_click_and_send_are_refused_but_draft_is_not_a_send() {
    let p = policy();
    let click = json!({"tool_name":"mcp__claude-in-chrome__computer", "tool_input":{"action":"left_click", "coordinate":[12,34],"tabId":1}});
    let mut clicks = 0;
    if decide(&p, &click, None, 100).decision == "allow" {
        clicks += 1;
    }
    assert_eq!(clicks, 0);
    assert_eq!(decide(&p, &send(), None, 100).decision, "deny");
    let draft = json!({"tool_name":"mcp__gmail__create_draft", "tool_input":{"body":"Fixture A"}});
    assert_eq!(decide(&p, &draft, None, 100).decision, "allow");
}

#[test]
fn exact_text_destination_and_entire_action_are_separate_bindings() {
    let original = send();
    let first = binding(&original);
    assert_eq!(first.text_sha256, digest(b"Fixture A"));
    for key in ["body", "subject", "to", "thread_id", "cc", "attachments"] {
        let mut changed = original.clone();
        changed["tool_input"][key] = json!("different fixture value");
        if let Selection::Send(next) = select(&policy(), &changed) {
            assert_ne!(first.action_sha256, next.action_sha256, "{key}");
        } else {
            assert!(matches!(key, "to" | "attachments"));
        }
    }
    let mut changed = original;
    changed["tool_input"]["body"] = json!("Fixture A\n");
    assert_ne!(first.text_sha256, binding(&changed).text_sha256);
}

#[test]
fn object_order_is_canonical_but_array_order_and_text_bytes_are_not_rewritten() {
    let a: Value = serde_json::from_str(r#"{"tool_name":"mcp__gmail__send_message","tool_input":{"body":"Fixture A","to":["a","b"]}}"#).unwrap();
    let b: Value = serde_json::from_str(r#"{"tool_input":{"to":["a","b"],"body":"Fixture A"},"tool_name":"mcp__gmail__send_message"}"#).unwrap();
    assert_eq!(binding(&a), binding(&b));
    let mut changed = b;
    changed["tool_input"]["to"] = json!(["b", "a"]);
    assert_ne!(binding(&a).action_sha256, binding(&changed).action_sha256);
    assert_ne!(digest("é".as_bytes()), digest("e\u{301}".as_bytes()));
}

#[test]
fn missing_message_and_required_thread_or_ambiguous_policy_refuse() {
    let mut p = policy();
    for value in [
        json!({}),
        json!({"tool_name":"unknown","tool_input":{}}),
        json!({"tool_name":"mcp__gmail__send_message","tool_input":{"body":"A"}}),
    ] {
        assert_eq!(select(&p, &value), Selection::Unknown);
    }
    let mut message = send();
    message["tool_input"]
        .as_object_mut()
        .unwrap()
        .remove("thread_id");
    assert!(matches!(select(&p, &message), Selection::Send(_)));
    p.send_adapters[0].thread_required = true;
    assert_eq!(select(&p, &message), Selection::Unknown);
    p.non_send_tools.push("mcp__gmail__send_message".into());
    assert!(!p.valid());
}

#[test]
fn response_does_not_disclose_input_or_destination() {
    let response = serde_json::to_string(&decide(&policy(), &send(), None, 100)).unwrap();
    for private in ["Fixture A", "example.invalid", "fixture-thread"] {
        assert!(!response.contains(private));
    }
}

#[cfg(feature = "quipu")]
mod signed {
    use super::*;
    use crate::action_certification::{sign, ActionInput};
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };

    fn key() -> Ed25519KeyPair {
        Ed25519KeyPair::from_pkcs8(
            Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                .unwrap()
                .as_ref(),
        )
        .unwrap()
    }

    fn fixture(
        approval_change: impl FnOnce(&mut ActionInput),
        gate_change: impl FnOnce(&mut ActionInput),
    ) -> (OutboundPolicy, Proofs) {
        let approval_key = key();
        let gate_key = key();
        let mut p = policy();
        p.approval_public_key = hex::encode(approval_key.public_key().as_ref());
        p.gate_public_key = hex::encode(gate_key.public_key().as_ref());
        let bound = binding(&send());
        let mut approval: ActionInput = serde_json::from_value(json!({
            "record_id":"approval-1", "correlation_id":"fixture-1", "session":"fixture-session",
            "ts":100, "agent":"protected-approval-producer", "verb":"approve-outbound",
            "target":bound.destination_sha256, "target_class":"outbound-human-message", "tenant":"fixture",
            "result":"approved", "repo":"", "sha":bound.text_sha256, "ref":"", "remote_authority":"",
            "scope_provenance":{"policy_revision":p.revision, "action_sha256":bound.action_sha256},
            "checks":[{"id":"human-approval","expected":"direct-user-chat","observed":"direct-user-chat","evidence_ref":format!("sha256:{}", "d".repeat(64))}]
        })).unwrap();
        let mut gate = approval.clone();
        gate.record_id = "gate-1".into();
        gate.verb = "content-gate".into();
        gate.result = "send".into();
        gate.checks[0].id = "content-gate".into();
        gate.checks[0].expected = json!("send");
        gate.checks[0].observed = json!("send");
        approval_change(&mut approval);
        gate_change(&mut gate);
        let proofs = Proofs {
            approval: serde_json::to_value(sign(approval, &approval_key).unwrap()).unwrap(),
            content_gate: serde_json::to_value(sign(gate, &gate_key).unwrap()).unwrap(),
        };
        (p, proofs)
    }

    #[test]
    fn valid_protected_approval_and_send_gate_allow_only_the_exact_operation() {
        let (p, proofs) = fixture(|_| {}, |_| {});
        assert_eq!(decide(&p, &send(), Some(&proofs), 100).decision, "allow");
        for field in ["body", "to", "thread_id", "subject", "cc", "attachments"] {
            let mut changed = send();
            changed["tool_input"][field] = json!("Fixture B");
            assert_eq!(
                decide(&p, &changed, Some(&proofs), 100).decision,
                "deny",
                "{field}"
            );
        }
    }

    #[test]
    fn relayed_approval_and_blocked_gate_do_not_become_human_permission() {
        let (p, proofs) = fixture(
            |a| {
                a.checks[0].expected = json!("agent-relay");
                a.checks[0].observed = json!("agent-relay");
            },
            |_| {},
        );
        assert_eq!(decide(&p, &send(), Some(&proofs), 100).decision, "deny");
        let (p, proofs) = fixture(
            |_| {},
            |g| {
                g.result = "blocked".into();
                g.checks[0].expected = json!("blocked");
                g.checks[0].observed = json!("blocked");
            },
        );
        assert_eq!(decide(&p, &send(), Some(&proofs), 100).decision, "deny");
    }

    #[test]
    fn wrong_keys_expiry_future_evidence_revision_and_tampering_refuse() {
        let (mut p, mut proofs) = fixture(|_| {}, |_| {});
        assert_eq!(decide(&p, &send(), Some(&proofs), 99).decision, "deny");
        assert_eq!(decide(&p, &send(), Some(&proofs), 161).decision, "deny");
        p.revision = "e".repeat(64);
        assert_eq!(decide(&p, &send(), Some(&proofs), 100).decision, "deny");
        p.revision = "a".repeat(64);
        std::mem::swap(&mut p.approval_public_key, &mut p.gate_public_key);
        assert_eq!(decide(&p, &send(), Some(&proofs), 100).decision, "deny");
        std::mem::swap(&mut p.approval_public_key, &mut p.gate_public_key);
        proofs.approval["result"] = json!("different");
        assert_eq!(decide(&p, &send(), Some(&proofs), 100).decision, "deny");
    }
}

#[test]
fn mutable_references_and_undeclared_fields_never_select_a_send() {
    for field in ["attachments", "draft_id", "body_file", "html", "unknown"] {
        let mut payload = send();
        // Keeping the same reference after its backing content changes remains
        // refused: the guard never resolves or admits the reference at all.
        payload["tool_input"][field] = json!("unchanged-reference");
        assert_eq!(select(&policy(), &payload), Selection::Unknown);
        assert_eq!(decide(&policy(), &payload, None, 100).decision, "deny");
    }
    let mut p = policy();
    p.gate_public_key = p.approval_public_key.clone();
    assert!(!p.valid());
    let mut p = policy();
    p.send_adapters[0].literal_fields.push("attachments".into());
    assert!(!p.valid());
    let mut p = policy();
    p.send_adapters[0].text_pointer = "/nested/body".into();
    assert!(!p.valid());
}
