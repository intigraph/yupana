use super::*;
use crate::action_certification::{sign, ActionInput};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};

fn fixture() -> (AdmissionPolicy, SignedActionRecord) {
    fixture_with(|_| {})
}

fn fixture_with(mutate: impl FnOnce(&mut ActionInput)) -> (AdmissionPolicy, SignedActionRecord) {
    let key = Ed25519KeyPair::from_pkcs8(
        Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .unwrap()
            .as_ref(),
    )
    .unwrap();
    let policy = AdmissionPolicy {
        revision: "c".repeat(64),
        issuer_public_key: hex::encode(key.public_key().as_ref()),
        max_age_secs: 120,
        required_checks: vec!["current-admission".into()],
    };
    let mut input: ActionInput = serde_json::from_value(serde_json::json!({
        "record_id":"admission-1", "correlation_id":"attempt-1", "session":"session-1",
        "ts":100, "agent":"helper", "item":"task-1", "verb":"merge",
        "target":"repository-1", "target_class":"repo", "tenant":"worker",
        "result":"admitted", "repo":"example/project", "sha":"a".repeat(40),
        "ref":"main", "remote_authority":"github.com",
        "scope_provenance":{"pr":42,"base":"b".repeat(40),"policy_revision":policy.revision,"transport":"canonical-helper","command_sha256":"e".repeat(64)},
        "checks":[{"id":"current-admission","expected":true,"observed":true,"evidence_ref":"proof-1"}]
    })).unwrap();
    mutate(&mut input);
    (policy, sign(input, &key).unwrap())
}

fn target() -> AdmissionTarget<'static> {
    AdmissionTarget {
        authority: "github.com",
        repository: "example/project",
        pull_request: 42,
        head: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        base: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        base_ref: "main",
        command_sha256: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
    }
}

#[test]
fn exact_current_admission_satisfies_but_missing_is_not_exempt() {
    let (policy, proof) = fixture();
    assert_eq!(
        evaluate(&policy, &target(), Some(&proof), 100),
        AdmissionVerdict::Satisfied
    );
    assert_eq!(
        evaluate(&policy, &target(), None, 100),
        AdmissionVerdict::Missing
    );
}

#[test]
fn every_target_component_is_bound_independently_of_the_proof() {
    let (policy, proof) = fixture();
    type Mutation = fn(&mut AdmissionTarget<'_>);
    let mutations: [Mutation; 7] = [
        |t| t.authority = "other.example",
        |t| t.repository = "other/project",
        |t| t.pull_request = 43,
        |t| t.head = "dddddddddddddddddddddddddddddddddddddddd",
        |t| t.base = "dddddddddddddddddddddddddddddddddddddddd",
        |t| t.base_ref = "topic",
        |t| t.command_sha256 = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
    ];
    for mutate in mutations {
        let mut changed = target();
        mutate(&mut changed);
        assert_eq!(
            evaluate(&policy, &changed, Some(&proof), 100),
            AdmissionVerdict::Unsatisfied
        );
    }
    let mut unknown = target();
    unknown.head = "";
    assert_eq!(
        evaluate(&policy, &unknown, Some(&proof), 100),
        AdmissionVerdict::Unknown
    );
}

#[test]
fn evidence_expiry_and_future_clock_are_not_silent_grace_periods() {
    let (policy, proof) = fixture();
    assert_eq!(
        evaluate(&policy, &target(), Some(&proof), 220),
        AdmissionVerdict::Satisfied
    );
    for now in [99, 221, u64::MAX] {
        assert_eq!(
            evaluate(&policy, &target(), Some(&proof), now),
            AdmissionVerdict::Unsatisfied
        );
    }
}

#[test]
fn policy_revision_issuer_and_required_checks_are_authoritative() {
    let (policy, proof) = fixture();
    let mut changed = policy.clone();
    changed.revision = "d".repeat(64);
    assert_eq!(
        evaluate(&changed, &target(), Some(&proof), 100),
        AdmissionVerdict::Unsatisfied
    );
    changed = policy.clone();
    changed.issuer_public_key = "0".repeat(64);
    assert_eq!(
        evaluate(&changed, &target(), Some(&proof), 100),
        AdmissionVerdict::Unsatisfied
    );
    changed = policy.clone();
    changed.required_checks.push("review-when-required".into());
    assert_eq!(
        evaluate(&changed, &target(), Some(&proof), 100),
        AdmissionVerdict::Unsatisfied
    );
    changed = policy;
    changed.required_checks.clear();
    assert_eq!(
        evaluate(&changed, &target(), Some(&proof), 100),
        AdmissionVerdict::Unknown
    );
}

#[test]
fn duplicate_checks_and_unbounded_or_empty_contracts_are_unknown() {
    let (policy, _) = fixture();
    for age in [0, 3601, u64::MAX] {
        let mut changed = policy.clone();
        changed.max_age_secs = age;
        assert!(!changed.valid());
    }
    let mut changed = policy;
    changed
        .required_checks
        .push(changed.required_checks[0].clone());
    assert!(!changed.valid());
}

#[test]
fn authenticated_semantic_failures_do_not_become_admissions() {
    type Mutation = fn(&mut ActionInput);
    let mutations: [Mutation; 10] = [
        |i| i.result = "attempted".into(),
        |i| i.verb = "push".into(),
        |i| i.checks.clear(),
        |i| i.checks.push(i.checks[0].clone()),
        |i| i.checks[0].evidence_ref.clear(),
        |i| {
            i.checks[0].expected = false.into();
            i.checks[0].observed = false.into();
        },
        |i| {
            i.checks[0].expected = 1.into();
            i.checks[0].observed = 1.into();
        },
        |i| i.checks[0].observed = false.into(),
        |i| {
            i.scope_provenance
                .as_object_mut()
                .unwrap()
                .remove("transport");
        },
        |i| i.scope_provenance["pr"] = "42".into(),
    ];
    for mutate in mutations {
        let (policy, proof) = fixture_with(mutate);
        assert!(verify(
            &proof,
            &hex::decode(&policy.issuer_public_key).unwrap()
        ));
        assert_eq!(
            evaluate(&policy, &target(), Some(&proof), 100),
            AdmissionVerdict::Unsatisfied
        );
    }
}
