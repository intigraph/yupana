//! Partial maps and malformed scopes cannot certify that a push is unaffected.

use super::{decode_map, select, CiCheck, Selection};
use serde_json::json;

fn row() -> serde_json::Value {
    json!({"job":{"value":"ci:source"},"label":{"value":"source check"},
        "command":{"value":"true"},"scope":{"value":"src/**/*.rs"}})
}

#[test]
fn partial_and_failed_projections_are_refused_before_selection() {
    for marker in [json!(true), json!(null), json!("false"), json!(0)] {
        let body = json!({"truncated":marker,"results":{"bindings":[row()]}});
        assert!(decode_map(&body.to_string()).is_err(), "{body}");
    }
    let error = json!({"error":"read failed","results":{"bindings":[row()]}});
    assert!(decode_map(&error.to_string()).is_err());
    for body in [
        json!({"results":{"bindings":[row()]}}),
        json!({"truncated":false,"results":{"bindings":[row()]}}),
    ] {
        let checks = decode_map(&body.to_string()).unwrap();
        assert!(matches!(
            select(&checks, &["src/lib.rs".into()]),
            Selection::Checks(_)
        ));
        assert_eq!(
            select(&checks, &["docs/readme.md".into()]),
            Selection::Quiet
        );
    }
}

#[test]
fn malformed_or_empty_scopes_are_not_quiet_or_partially_executable() {
    for scope in ["src/[", "[a", "", " "] {
        let mut bad = row();
        bad["scope"]["value"] = json!(scope);
        let body = json!({"results":{"bindings":[row(),bad]}});
        assert!(decode_map(&body.to_string()).is_err());
        let mut checks = decode_map(&json!({"results":{"bindings":[row()]}}).to_string()).unwrap();
        checks.push(CiCheck {
            id: "ci:bad".into(),
            label: "bad scope".into(),
            command: "true".into(),
            scopes: vec![scope.into()],
        });
        assert_eq!(select(&checks, &["src/lib.rs".into()]), Selection::Unknown);
        assert_eq!(
            select(&checks, &["docs/readme.md".into()]),
            Selection::Unknown
        );
    }
    let mut checks = decode_map(&json!({"results":{"bindings":[row()]}}).to_string()).unwrap();
    checks[0].scopes.clear();
    assert_eq!(select(&checks, &["src/lib.rs".into()]), Selection::Unknown);
}

#[test]
fn blank_identifiers_labels_or_commands_cannot_be_successful_checks() {
    for key in ["job", "label", "command"] {
        for blank in ["", " "] {
            let mut bad = row();
            bad[key]["value"] = json!(blank);
            assert!(decode_map(&json!({"results":{"bindings":[bad]}}).to_string()).is_err());
        }
    }
}
