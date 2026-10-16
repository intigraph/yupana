//! Verification against a caller-pinned issuer, never a key supplied by evidence.
use super::{
    ActionInput, CheckInput, Payload, SignedActionRecord, CANONICALIZATION, SCHEMA_VERSION,
};
use ring::signature::{UnparsedPublicKey, ED25519};
use sha2::{Digest, Sha256};

/// Authenticate an action record and its derived decision fields.
///
/// This establishes who signed these observations, not whether they are true.
/// Consumers must separately bind target, operation, time, policy and checks.
/// A public key copied from the record itself is not an issuer trust decision.
#[must_use]
pub fn verify(record: &SignedActionRecord, issuer_public_key: &[u8]) -> bool {
    if issuer_public_key.len() != 32
        || record.schema_version != SCHEMA_VERSION
        || record.kind != "action"
        || record.verifier_id != crate::verdict::VERIFIER
        || record.signature_alg != "ed25519"
        || record.canonicalization != CANONICALIZATION
        || record.record_id.is_empty()
        || record.correlation_id.is_empty()
        || record.session.is_empty()
    {
        return false;
    }
    let key_id = format!("sha256:{}", hex::encode(Sha256::digest(issuer_public_key)));
    if record.key_id != key_id {
        return false;
    }
    let reasons: Vec<_> = record
        .checks
        .iter()
        .filter(|check| check.expected != check.observed)
        .map(|check| format!("{}_mismatch", check.id.replace('-', "_")))
        .collect();
    let status = if reasons.is_empty() {
        "certified"
    } else {
        "uncertified"
    };
    if record.reason_codes != reasons
        || record.certification_status != status
        || record.checks.iter().any(|check| {
            check.outcome
                != if check.expected == check.observed {
                    "satisfied"
                } else {
                    "unsatisfied"
                }
        })
    {
        return false;
    }
    let input = ActionInput {
        record_id: record.record_id.clone(),
        correlation_id: record.correlation_id.clone(),
        session: record.session.clone(),
        ts: record.ts,
        agent: record.agent.clone(),
        item: record.item.clone(),
        verb: record.verb.clone(),
        target: record.target.clone(),
        target_class: record.target_class.clone(),
        tenant: record.tenant.clone(),
        result: record.result.clone(),
        repo: record.repo.clone(),
        sha: record.sha.clone(),
        git_ref: record.git_ref.clone(),
        remote_authority: record.remote_authority.clone(),
        scope_provenance: record.scope_provenance.clone(),
        checks: record
            .checks
            .iter()
            .map(|check| CheckInput {
                id: check.id.clone(),
                expected: check.expected.clone(),
                observed: check.observed.clone(),
                evidence_ref: check.evidence_ref.clone(),
            })
            .collect(),
    };
    let payload = Payload {
        schema_version: SCHEMA_VERSION,
        kind: "action",
        input: &input,
        checks: &record.checks,
        certification_status: status,
        reason_codes: &reasons,
        verifier_id: crate::verdict::VERIFIER,
        key_id: &key_id,
        canonicalization: CANONICALIZATION,
    };
    let Ok(bytes) = serde_json::to_vec(&payload) else {
        return false;
    };
    let Ok(signature) = hex::decode(&record.signature) else {
        return false;
    };
    record.signed_payload_hash == format!("sha256:{}", hex::encode(Sha256::digest(&bytes)))
        && record.verdict_id
            == format!(
                "action-verdict-{}",
                &record.signature[..record.signature.len().min(32)]
            )
        && UnparsedPublicKey::new(&ED25519, issuer_public_key)
            .verify(&bytes, &signature)
            .is_ok()
}

#[cfg(test)]
#[path = "action_certification_verify_test.rs"]
mod tests;
