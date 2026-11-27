use super::*;
use crate::policy::Mode;
use crate::project_hold::HoldPolicy;

const SERIES: &str = "aegis_quiet_time_hold_active{effect=\"no-heavy-builds\"}";

fn policy() -> HoldPolicy {
    HoldPolicy {
        id: "http://example/policy_media_build_hold".into(),
        label: "media build hold".into(),
        command_regex: "(^|[;&|[:space:]])(cargo[[:space:]]+(build|test|clippy)|just[[:space:]]+(build|test|lint|check))".into(),
        series: SERIES.into(),
        signal_path: "/var/lib/node_exporter/textfile/gaming.prom".into(),
        max_age_secs: 300,
        retry: Some("retry after media stops plus lift_delay (`st fleet hold all --status` exits 0)".into()),
        effect: "deny".into(),
        class: crate::constraint::ConstraintClass::Hard,
        verification_point: crate::constraint::VerificationPoint::Pag,
        rationale: None,
    }
}

fn textfile(builds: u8) -> String {
    format!(
        "# TYPE aegis_quiet_time_hold_active gauge\n\
         aegis_quiet_time_hold_active{{effect=\"full\"}} 0\n\
         aegis_quiet_time_hold_active{{effect=\"no-heavy-builds\"}} {builds}\n"
    )
}

#[test]
fn media_playing_refuses_cargo_build_naming_the_hold() {
    let got = evaluate(&[policy()], "cargo build", |_| Signal::Held, Mode::Enforce);
    match got {
        Outcome::Deny(m) => {
            assert!(m.contains("media build hold"), "{m}");
            assert!(m.contains(SERIES), "{m}");
            assert!(m.contains("lift_delay"), "names when to retry: {m}");
        }
        other => panic!("expected deny, got {other:?}"),
    }
}

#[test]
fn light_work_is_never_touched_by_a_hold() {
    for cmd in [
        "git push origin HEAD",
        "st agent spawn --report",
        "cargo fmt",
        "ls",
    ] {
        assert_eq!(
            evaluate(&[policy()], cmd, |_| Signal::Held, Mode::Enforce),
            Outcome::Allow,
            "{cmd}"
        );
    }
}

#[test]
fn media_stopped_allows_builds() {
    assert_eq!(
        evaluate(&[policy()], "just test", |_| Signal::Clear, Mode::Enforce),
        Outcome::Allow
    );
}

#[test]
fn unknown_signal_fails_closed() {
    let unknown = |_: &HoldPolicy| Signal::Unknown("cannot read".into());
    assert!(matches!(
        evaluate(&[policy()], "cargo clippy", unknown, Mode::Enforce),
        Outcome::Deny(m) if m.contains("UNKNOWN")
    ));
}

#[test]
fn advise_reports_without_refusing() {
    assert!(matches!(
        evaluate(&[policy()], "cargo test", |_| Signal::Held, Mode::Advise),
        Outcome::Notify(_)
    ));
}

#[test]
fn reads_the_exact_series_not_the_full_hold() {
    assert_eq!(read_signal(&textfile(1), SERIES, 5, 300), Signal::Held);
    assert_eq!(read_signal(&textfile(0), SERIES, 5, 300), Signal::Clear);
}

#[test]
fn stale_missing_or_garbled_signal_is_unknown() {
    assert!(matches!(
        read_signal(&textfile(0), SERIES, 301, 300),
        Signal::Unknown(_)
    ));
    // A pre-effect fleet writes only the unlabelled gauge: not a reading.
    let legacy = "aegis_quiet_time_hold_active 0\n";
    assert!(matches!(
        read_signal(legacy, SERIES, 5, 300),
        Signal::Unknown(_)
    ));
    let garbled = format!("{SERIES} 0.5\n");
    assert!(matches!(
        read_signal(&garbled, SERIES, 5, 300),
        Signal::Unknown(_)
    ));
}

#[test]
fn routing_superset_covers_declared_heavy_commands() {
    assert!(might_be_heavy_build("nice cargo build"));
    assert!(might_be_heavy_build("just test"));
    assert!(!might_be_heavy_build("git status"));
    assert!(!might_be_heavy_build("echo cargo_notes"));
}
