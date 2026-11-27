use super::*;

pub(crate) fn row(overrides: &[(&str, &str)]) -> serde_json::Value {
    let mut row = serde_json::json!({
        "policy":{"value":"http://example/policy_media_build_hold"},
        "label":{"value":"media quiet-time build hold"},
        "pattern":{"value":"(^|[;&|[:space:]])(cargo[[:space:]]+(build|test|clippy)|just[[:space:]]+(build|test|lint|check))"},
        "matchType":{"value":"must-match"},
        "effect":{"value":"deny"},
        "constraintClass":{"value":"hard"},
        "verificationPoint":{"value":"PAG"},
        "kind":{"value":"quiet_time_hold"},
        "series":{"value":"aegis_quiet_time_hold_active{effect=\"no-heavy-builds\"}"},
        "signal":{"value":"/var/lib/node_exporter/textfile/gaming.prom"},
        "maxAge":{"value":"300"}
    });
    for (k, v) in overrides {
        row[*k] = serde_json::json!({ "value": v });
    }
    row
}

fn body(rows: &[serde_json::Value]) -> String {
    serde_json::json!({"results":{"bindings":rows}}).to_string()
}

#[test]
fn decodes_a_governed_hold() {
    let p = decode_hold_policies(&body(&[row(&[])])).unwrap().remove(0);
    assert_eq!(p.max_age_secs, 300);
    assert_eq!(
        p.series,
        "aegis_quiet_time_hold_active{effect=\"no-heavy-builds\"}"
    );
    assert!(p.matches("cd repo && cargo build --release"));
    assert!(p.matches("just test"));
    assert!(!p.matches("cargo fmt"));
    assert!(!p.matches("git status"));
    assert!(p.blocks(Mode::Enforce));
    assert!(!p.blocks(Mode::Advise));
}

#[test]
fn malformed_policy_data_is_refused_not_guessed() {
    for bad in [
        ("kind", "deterministic_threshold"),
        ("maxAge", "0"),
        ("maxAge", "soon"),
        ("signal", "relative/gaming.prom"),
        ("series", "two words"),
        ("pattern", "(unclosed"),
        ("matchType", "may-match"),
        ("verificationPoint", "CAG"),
    ] {
        assert!(
            decode_hold_policies(&body(&[row(&[bad])])).is_err(),
            "{bad:?} must not decode"
        );
    }
}

#[test]
fn conflicting_rows_for_one_policy_are_an_error() {
    let rows = [row(&[]), row(&[("maxAge", "60")])];
    assert!(decode_hold_policies(&body(&rows)).is_err());
    let same = [row(&[]), row(&[])];
    assert_eq!(decode_hold_policies(&body(&same)).unwrap().len(), 1);
}
