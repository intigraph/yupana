use super::*;
use crate::action_certification::sign;
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

fn input() -> ActionInput {
    serde_json::from_value(serde_json::json!({
        "record_id":"admission-1", "correlation_id":"attempt-1", "session":"session-1",
        "ts":100, "agent":"helper", "item":"task-1", "verb":"merge",
        "target":"repository-1", "target_class":"repo", "tenant":"worker",
        "result":"admitted", "repo":"example/project", "sha":"a".repeat(40),
        "ref":"main", "remote_authority":"github.com",
        "scope_provenance":{"pr":42,"base":"b".repeat(40),"policy":"policy-1"},
        "checks":[{"id":"current-admission","expected":true,"observed":true,"evidence_ref":"proof-1"}]
    })).unwrap()
}

#[test]
fn authentic_record_round_trips_but_does_not_supply_its_own_trust() {
    let issuer = key();
    let record = sign(input(), &issuer).unwrap();
    let restored = serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
    assert!(verify(&restored, issuer.public_key().as_ref()));
    assert!(!verify(&restored, key().public_key().as_ref()));
    assert!(!verify(&restored, &[]));
}

#[test]
fn all_decision_and_binding_fields_are_authenticated() {
    let issuer = key();
    let record = sign(input(), &issuer).unwrap();
    let pristine = serde_json::to_value(&record).unwrap();
    // Include metadata outside the signed input: those must be checked too.
    for field in [
        "schema_version",
        "kind",
        "record_id",
        "correlation_id",
        "session",
        "agent",
        "item",
        "verb",
        "target",
        "target_class",
        "tenant",
        "result",
        "repo",
        "sha",
        "ref",
        "remote_authority",
        "certification_status",
        "verifier_id",
        "key_id",
        "signature_alg",
        "canonicalization",
        "signed_payload_hash",
        "verdict_id",
        "signature",
    ] {
        let mut value = pristine.clone();
        value[field] = "changed".into();
        let changed = serde_json::from_value(value).unwrap();
        assert!(!verify(&changed, issuer.public_key().as_ref()), "{field}");
    }
    for pointer in [
        "/ts",
        "/scope_provenance/pr",
        "/scope_provenance/base",
        "/scope_provenance/policy",
        "/checks/0/id",
        "/checks/0/expected",
        "/checks/0/observed",
        "/checks/0/evidence_ref",
        "/checks/0/outcome",
    ] {
        let mut value = pristine.clone();
        let old = value.pointer_mut(pointer).unwrap();
        *old = if old.is_number() {
            101.into()
        } else if old.is_boolean() {
            false.into()
        } else {
            "changed".into()
        };
        let changed = serde_json::from_value(value).unwrap();
        assert!(!verify(&changed, issuer.public_key().as_ref()), "{pointer}");
    }
    let mut changed = record.clone();
    changed.reason_codes.push("invented".into());
    assert!(!verify(&changed, issuer.public_key().as_ref()));
}

#[test]
fn authenticated_failure_is_valid_evidence_not_an_admission() {
    let issuer = key();
    let mut failed = input();
    failed.checks[0].observed = false.into();
    let record = sign(failed, &issuer).unwrap();
    assert!(verify(&record, issuer.public_key().as_ref()));
    assert_eq!(record.certification_status, "uncertified");
    let mut forged = record;
    forged.certification_status = "certified".into();
    forged.reason_codes.clear();
    forged.checks[0].outcome = "satisfied".into();
    assert!(!verify(&forged, issuer.public_key().as_ref()));
}

#[test]
fn malformed_signatures_refuse_without_panicking() {
    let issuer = key();
    let mut record = sign(input(), &issuer).unwrap();
    for signature in ["", "0", "zz", "é", &"0".repeat(128)] {
        record.signature = signature.into();
        assert!(!verify(&record, issuer.public_key().as_ref()));
    }
}
