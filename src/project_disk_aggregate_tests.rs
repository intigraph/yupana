use super::*;
use crate::project_disk::DiskSample;

fn id(n: usize) -> String {
    format!("{n:064x}")
}
const AT: &str = "2026-11-06T00:00:00Z";

#[test]
fn class_identity_drops_repo_cwd_and_arbitrary_operands() {
    let first = "cargo:build|repo:first|cwd:root";
    let second = "cargo:build|repo:second|cwd:subdir";
    assert_eq!(subject(first, "root:test"), subject(second, "root:test"));
    assert_ne!(subject(first, "root:test"), subject(first, "mount:other"));
    assert_ne!(
        subject(first, "root:test"),
        subject("cargo:test", "root:test")
    );
    let classes: std::collections::BTreeSet<_> = (0..10_000)
        .flat_map(|n| {
            [
                format!("cp:private-{n}"),
                format!("cargo:private-{n}"),
                format!("unknown-{n}:argv"),
            ]
        })
        .map(|s| command_class(&s))
        .collect();
    assert_eq!(classes.len(), 3);
    assert_eq!(command_class("cp:private-path"), "cp:_");
}

#[test]
fn fixed_histogram_preserves_counts_and_conservative_projection() {
    let mut state = None;
    let mut raw = Vec::new();
    for n in 0..1_000 {
        let delta = n as i64 - 900;
        raw.push(DiskSample { delta_bytes: delta });
        state = Some(State::record(state.as_ref(), &id(n), delta, AT).unwrap());
    }
    let state = state.unwrap();
    assert_eq!(state.count, 1000);
    assert_eq!(state.maximum, 99);
    assert_eq!(state.bins.len(), BUCKETS);
    assert_eq!(state.bins.iter().sum::<u64>(), 1000);
    assert!(state.quantile(90) >= crate::project_disk::p90(&raw).unwrap());
    assert!(state.maximum() >= crate::project_disk::p90(&raw[900..]).unwrap());
    assert_eq!(state.quantile(95), 64);
    let retry = State::record(Some(&state), &id(999), 99, AT).unwrap();
    assert_eq!(retry, state);
    let old = State::record(Some(&state), &id(1001), -4096, "2026-11-05T00:00:00Z").unwrap();
    assert_eq!(old.maximum, 99);
    assert_eq!(old.last_seen, state.last_seen);
    let body = serde_json::to_string(&old).unwrap();
    assert!(body.len() < 2048);
    assert!(!body.contains("samples"));
    assert_eq!(State::decode(&body).unwrap(), old);
    for delta in [i64::MIN, -1, 0, 1, 2, 3, i64::MAX] {
        let single = State::record(None, &id(1), delta, AT).unwrap();
        assert_eq!(single.quantile(90), delta);
        assert_eq!(
            State::decode(&serde_json::to_string(&single).unwrap()).unwrap(),
            single
        );
    }
}

#[test]
fn malformed_summary_is_unknown_not_empty_history() {
    let good = State::record(None, &id(1), -4096, AT).unwrap();
    let body = serde_json::to_string(&good).unwrap();
    assert_eq!(State::decode(&body).unwrap(), good);
    for broken in [String::new(), "{}".into(), "x".repeat(MAX_STATE_BYTES + 1)] {
        assert!(State::decode(&broken).is_err());
    }
    for field in ["version", "count", "bins", "last_seen", "maximum"] {
        let mut value = serde_json::to_value(&good).unwrap();
        value.as_object_mut().unwrap().remove(field);
        assert!(State::decode(&value.to_string()).is_err());
    }
    let mut broken = good.clone();
    broken.count = 0;
    assert!(State::decode(&serde_json::to_string(&broken).unwrap()).is_err());
    broken = good.clone();
    broken.bins[0] += 1;
    assert!(State::decode(&serde_json::to_string(&broken).unwrap()).is_err());
}

#[test]
fn aggregate_projection_reads_one_subject_and_reports_conservative_basis() {
    let state = State::record(None, &id(1), 123, AT).unwrap();
    let body = serde_json::to_string(&state).unwrap();
    let expected_subject = subject("cargo:build", "root:test");
    let (endpoint, server) = crate::test_stub::stub(1, move |_, request| {
        let query = request["query"].as_str().unwrap();
        assert!(query.contains(&expected_subject));
        assert!(query.contains("aggregateState"));
        assert!(!query.contains("commandSignature"));
        (
            200,
            serde_json::json!({"results":{"bindings":[{"state":{"type":"literal","value":body}}]}}),
        )
    });
    let prediction = crate::project_disk::fetch_prediction(&endpoint, "cargo:build", "root:test")
        .unwrap()
        .unwrap();
    assert_eq!(
        (prediction.bytes, prediction.samples, prediction.basis),
        (123, 1, "class maximum")
    );
    server.join().unwrap();
}

#[test]
fn compare_and_swap_never_uses_per_run_subjects_or_unconditional_set() {
    let signature = "cargo:test|repo:example|cwd:root";
    let iri = subject(signature, "root:test");
    let state = State::record(None, &id(1), 123, AT).unwrap();
    let first = update(&iri, signature, "root:test", None, &state).unwrap();
    assert!(first.contains("FILTER NOT EXISTS"));
    let body = serde_json::to_string(&state).unwrap();
    let previous = (body.clone(), state.clone());
    let next = State::record(Some(&state), &id(2), 456, AT).unwrap();
    let second = update(&iri, signature, "root:test", Some(&previous), &next).unwrap();
    assert!(
        second.contains("DELETE {") && second.contains("INSERT {") && second.contains("WHERE {")
    );
    assert!(second.contains(&literal(&body)));
    assert!(second.contains("?other"));
    assert!(!second.contains("repo:example"));
    assert_eq!(
        subject(signature, "root:test"),
        subject("cargo:test|repo:another|cwd:subdir", "root:test")
    );
}

#[test]
fn shared_production_loop_retries_cas_but_never_refusals_or_unknown_writes() {
    use std::cell::Cell;
    let other = State::record(None, &id(777), 55, AT).unwrap();
    let snapshot = (serde_json::to_string(&other).unwrap(), other);
    let reads = Cell::new(0);
    let writes = Cell::new(0);
    accumulate(
        "cargo:build",
        "root:test",
        AT,
        123,
        || {
            reads.set(reads.get() + 1);
            Ok(if reads.get() == 1 {
                None
            } else {
                Some(snapshot.clone())
            })
        },
        |query| {
            writes.set(writes.get() + 1);
            if writes.get() == 1 {
                Ok(serde_json::json!({"asserted":0,"retracted":0,"tx":null}))
            } else {
                assert!(query.contains("runCount \"2\""));
                Ok(serde_json::json!({"asserted":10,"retracted":10,"tx":42}))
            }
        },
    )
    .unwrap();
    assert_eq!((reads.get(), writes.get()), (2, 2));
    for failure in [false, true] {
        let reads = Cell::new(0);
        let writes = Cell::new(0);
        let result = accumulate(
            "cargo:build",
            "root:test",
            AT,
            123,
            || {
                reads.set(reads.get() + 1);
                Ok(None)
            },
            |_| {
                writes.set(writes.get() + 1);
                Err(if failure {
                    WriteFailure::Uncertain("timeout".into())
                } else {
                    WriteFailure::Terminal("forbidden".into())
                })
            },
        );
        assert!(result.is_err());
        assert_eq!(writes.get(), 1);
        assert_eq!(reads.get(), if failure { 2 } else { 1 });
    }
    let writes = Cell::new(0);
    let result = accumulate(
        "cargo:build",
        "root:test",
        AT,
        123,
        || Ok(None),
        |_| {
            writes.set(writes.get() + 1);
            Ok(serde_json::json!({"asserted":0,"retracted":0,"tx":null}))
        },
    );
    assert!(result.is_err());
    assert_eq!(writes.get(), 3);
}

#[test]
fn ambiguous_committed_write_is_verified_without_a_second_write() {
    use std::cell::Cell;
    let sample_id = hex::encode(Sha256::digest(
        format!("cargo:build\0root:test\0{AT}").as_bytes(),
    ));
    let state = State::record(None, &sample_id, 123, AT).unwrap();
    let snapshot = (serde_json::to_string(&state).unwrap(), state);
    let reads = Cell::new(0);
    let writes = Cell::new(0);
    accumulate(
        "cargo:build",
        "root:test",
        AT,
        123,
        || {
            reads.set(reads.get() + 1);
            Ok(if reads.get() == 1 {
                None
            } else {
                Some(snapshot.clone())
            })
        },
        |_| {
            writes.set(writes.get() + 1);
            Err(WriteFailure::Uncertain("lost response".into()))
        },
    )
    .unwrap();
    assert_eq!((reads.get(), writes.get()), (2, 1));
    assert!(accumulate(
        "cargo:build",
        "root:test",
        AT,
        124,
        || Ok(Some(snapshot.clone())),
        |_| panic!("changed retry must not write")
    )
    .is_err());
}

#[test]
fn absent_summary_preserves_empty_legacy_history_and_corrupt_summary_refuses() {
    let (endpoint, server) = crate::test_stub::stub(2, move |index, request| {
        let query = request["query"].as_str().unwrap();
        assert!(query.contains(if index == 0 {
            "aggregateState"
        } else {
            "commandSignature"
        }));
        (200, serde_json::json!({"results":{"bindings":[]}}))
    });
    assert!(
        crate::project_disk::fetch_prediction(&endpoint, "cargo:build", "root:test")
            .unwrap()
            .is_none()
    );
    server.join().unwrap();
    let (endpoint, server) = crate::test_stub::stub(1, |_, _| {
        (
            200,
            serde_json::json!({"results":{"bindings":[{"state":{"type":"literal","value":"{}"}}]}}),
        )
    });
    assert!(crate::project_disk::fetch_prediction(&endpoint, "cargo:build", "root:test").is_err());
    server.join().unwrap();
}

#[test]
fn invalid_receipt_cannot_become_a_success_or_a_retry() {
    use std::cell::Cell;
    for receipt in [
        serde_json::json!({}),
        serde_json::json!({"asserted":0,"retracted":0}),
        serde_json::json!({"asserted":1,"retracted":0,"tx":0}),
    ] {
        let writes = Cell::new(0);
        assert!(accumulate(
            "cargo:build",
            "root:test",
            AT,
            123,
            || Ok(None),
            |_| {
                writes.set(writes.get() + 1);
                Ok(receipt.clone())
            }
        )
        .is_err());
        assert_eq!(writes.get(), 1);
    }
}
