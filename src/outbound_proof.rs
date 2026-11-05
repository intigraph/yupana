//! Reuse authenticated action records; a generic certified trace is insufficient.
use super::{hex_digest, Binding, OutboundPolicy};
use crate::action_certification::{verify, SignedActionRecord};
use serde_json::Value;

pub(super) fn valid(
    policy: &OutboundPolicy,
    target: &Binding,
    value: &Value,
    approval: bool,
    now: u64,
) -> bool {
    let Ok(proof) = serde_json::from_value::<SignedActionRecord>(value.clone()) else {
        return false;
    };
    let (key, verb, result, check, observed) = if approval {
        (
            &policy.approval_public_key,
            "approve-outbound",
            "approved",
            "human-approval",
            "direct-user-chat",
        )
    } else {
        (
            &policy.gate_public_key,
            "content-gate",
            "send",
            "content-gate",
            "send",
        )
    };
    let Ok(key) = hex::decode(key) else {
        return false;
    };
    verify(&proof, &key)
        && proof.verb == verb
        && proof.result == result
        && proof.target_class == "outbound-human-message"
        && proof.certification_status == "certified"
        && proof.sha == target.text_sha256
        && proof.target == target.destination_sha256
        && proof.ts <= now
        && now - proof.ts <= policy.max_age_secs
        && proof
            .scope_provenance
            .get("policy_revision")
            .and_then(Value::as_str)
            == Some(&policy.revision)
        && proof
            .scope_provenance
            .get("action_sha256")
            .and_then(Value::as_str)
            == Some(&target.action_sha256)
        && proof
            .checks
            .iter()
            .map(|c| &c.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == proof.checks.len()
        && proof.checks.iter().any(|c| {
            c.id == check
                && c.expected == observed
                && c.observed == observed
                && c.evidence_ref
                    .strip_prefix("sha256:")
                    .is_some_and(hex_digest)
        })
}
