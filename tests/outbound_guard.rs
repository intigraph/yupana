//! Real CLI/wrapper replay against synthetic signatures; no connector or account.
#![cfg(feature = "quipu")]
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};
use yupana::action_certification::{sign, ActionInput};
use yupana::outbound::{select, OutboundPolicy, Proofs, Selection};

fn key() -> Ed25519KeyPair {
    Ed25519KeyPair::from_pkcs8(
        Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .unwrap()
            .as_ref(),
    )
    .unwrap()
}

fn message() -> Value {
    json!({"tool_name":"mcp__gmail__send_message", "tool_input":{
        "to":["fixture@example.invalid"], "thread_id":"fixture-thread", "body":"Fixture A"}})
}

fn fixture() -> (OutboundPolicy, Proofs) {
    let user = key();
    let gate = key();
    let policy: OutboundPolicy = serde_json::from_value(json!({
        "revision":"a".repeat(64), "approval_public_key":hex::encode(user.public_key().as_ref()),
        "gate_public_key":hex::encode(gate.public_key().as_ref()), "max_age_secs":60,
        "non_send_tools":["mcp__gmail__create_draft"],
        "send_adapters":[{"tool_name":"mcp__gmail__send_message", "text_pointer":"/body",
            "recipient_pointer":"/to", "thread_pointer":"/thread_id", "thread_required":false, "literal_fields":["subject","cc","bcc"]}]
    }))
    .unwrap();
    let Selection::Send(bound) = select(&policy, &message()) else {
        panic!("fixture send");
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut input: ActionInput = serde_json::from_value(json!({
        "record_id":"approval", "correlation_id":"fixture", "session":"fixture", "ts":now,
        "agent":"protected-fixture-producer", "verb":"approve-outbound", "target_class":"outbound-human-message",
        "target":bound.destination_sha256, "sha":bound.text_sha256, "tenant":"fixture", "result":"approved",
        "repo":"", "ref":"", "remote_authority":"",
        "scope_provenance":{"policy_revision":policy.revision,"action_sha256":bound.action_sha256},
        "checks":[{"id":"human-approval", "expected":"direct-user-chat", "observed":"direct-user-chat",
                   "evidence_ref":format!("sha256:{}", "d".repeat(64))}]
    })).unwrap();
    let approval = serde_json::to_value(sign(input.clone(), &user).unwrap()).unwrap();
    input.record_id = "gate".into();
    input.verb = "content-gate".into();
    input.result = "send".into();
    input.checks[0].id = "content-gate".into();
    input.checks[0].expected = json!("send");
    input.checks[0].observed = json!("send");
    let content_gate = serde_json::to_value(sign(input, &gate).unwrap()).unwrap();
    (
        policy,
        Proofs {
            approval,
            content_gate,
        },
    )
}

fn guard(policy: &OutboundPolicy, proofs: Option<&Proofs>, payload: &Value) -> String {
    let root = tempfile::tempdir().unwrap();
    let policy_file = root.path().join("policy.json");
    let receipts = root.path().join("receipts");
    std::fs::create_dir(&receipts).unwrap();
    std::fs::write(&policy_file, serde_json::to_vec(policy).unwrap()).unwrap();
    if let (Some(proofs), Selection::Send(bound)) = (proofs, select(policy, payload)) {
        // Copying text-A evidence under text-B's lookup name must still refuse.
        std::fs::write(
            receipts.join(format!("{}.json", bound.action_sha256)),
            serde_json::to_vec(proofs).unwrap(),
        )
        .unwrap();
    }
    let mut child = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/guard-outbound.py"
        ))
        .args(["--yupana", env!("CARGO_BIN_EXE_yupana"), "--policy"])
        .arg(&policy_file)
        .arg("--receipts")
        .arg(&receipts)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(payload).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    let response = String::from_utf8(result.stdout).unwrap();
    for private in [
        "Fixture A",
        "Fixture B",
        "example.invalid",
        "fixture-thread",
    ] {
        assert!(!response.contains(private));
        assert!(!String::from_utf8_lossy(&result.stderr).contains(private));
    }
    serde_json::from_str::<Value>(&response).unwrap()["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn actual_boundary_refuses_unapproved_click_changed_text_and_missing_gate() {
    let (policy, mut proofs) = fixture();
    let click = json!({"tool_name":"mcp__claude-in-chrome__computer", "tool_input":{
        "action":"left_click", "coordinate":[12,34], "tabId":1}});
    let mut dispatches = 0;
    for payload in [&click, &message()] {
        if guard(&policy, None, payload) == "allow" {
            dispatches += 1;
        }
    }
    let mut changed = message();
    changed["tool_input"]["body"] = json!("Fixture B");
    if guard(&policy, Some(&proofs), &changed) == "allow" {
        dispatches += 1;
    }
    proofs.content_gate = Value::Null;
    if guard(&policy, Some(&proofs), &message()) == "allow" {
        dispatches += 1;
    }
    assert_eq!(dispatches, 0);
}

#[test]
fn actual_boundary_accepts_exact_protected_receipts_and_allows_draft_creation() {
    let (policy, proofs) = fixture();
    assert_eq!(guard(&policy, Some(&proofs), &message()), "allow");
    let draft = json!({"tool_name":"mcp__gmail__create_draft", "tool_input":{"body":"Fixture A"}});
    assert_eq!(guard(&policy, None, &draft), "allow");
}
