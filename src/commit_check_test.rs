use super::*;

fn rule(tier: TextTier) -> TextRule {
    TextRule {
        name: "schedule-policy".into(),
        label: None,
        pattern: "(?s)^external-scheduler/v1:".into(),
        tier,
        class: Some("commit-scheduler/v1:change-root".into()),
        exempt_path_regex: None,
        exempt_repos: Vec::new(),
        exempt_line_marker: None,
        rationale: None,
    }
}

fn timer() -> Vec<Fact> {
    commit_schedule::facts("clock.timer", b"[Timer]\nOnCalendar=hourly\n").unwrap()
}

#[test]
fn governed_advise_then_block_and_missing_coverage() {
    assert_eq!(
        judge(&[rule(TextTier::Warn)], timer()).unwrap().outcome,
        "advise"
    );
    assert_eq!(
        judge(&[rule(TextTier::Block)], timer()).unwrap().outcome,
        "refuse"
    );
    assert_eq!(
        judge(&[rule(TextTier::Block)], Vec::new()).unwrap().outcome,
        "allow"
    );
    assert!(judge(&[], timer()).is_err());
    let mut bad = rule(TextTier::Block);
    bad.pattern = "[".into();
    assert!(judge(&[bad], timer()).is_err());
    let mut narrow = rule(TextTier::Block);
    narrow.pattern = "unrelated".into();
    assert!(judge(&[narrow], timer()).is_err());
    let rules = [rule(TextTier::Block)];
    let text = timer()[0].text();
    assert!(textrules::evaluate_edit_in(&rules, &text, "guide.md", None).is_empty());
    assert_eq!(
        textrules::evaluate_in(&rules, &text, "clock.timer", None).len(),
        1
    );
}

#[test]
fn exception_requires_exact_open_direct_child_and_unit() {
    let good = serde_json::json!([{"id":"change-123","status":"open","parent":"change-root","description":"Migration\nclock.timer\n"}]);
    assert!(validate_exception(&good, "change-123", "change-root", &timer()).is_ok());
    for (field, value) in [
        ("status", "closed"),
        ("status", "in_progress"),
        ("parent", "other"),
        ("id", "other"),
        ("description", "some-clock.timer.old"),
    ] {
        let mut bad = good.clone();
        bad[0][field] = value.into();
        assert!(
            validate_exception(&bad, "change-123", "change-root", &timer()).is_err(),
            "{field}"
        );
    }
    for bad in [
        serde_json::json!([]),
        serde_json::json!({"error":"unreadable"}),
        serde_json::json!([good[0], good[0]]),
    ] {
        assert!(validate_exception(&bad, "change-123", "change-root", &timer()).is_err());
    }
}

#[test]
fn git_trailers_body_duplicate_and_wrong_spelling() {
    assert_eq!(
        trailer("Subject\n\nSchedule-Exception: change-123\n")
            .unwrap()
            .as_deref(),
        Some("change-123")
    );
    assert!(
        trailer("Subject\n\nQuoted:\nSchedule-Exception: change-123\n\nordinary body after\n")
            .unwrap()
            .is_none()
    );
    assert!(
        trailer("Subject\n\nSchedule-Exception: change-123\nSchedule-Exception: change-124\n")
            .is_err()
    );
    assert!(trailer("Subject\n\nScheduler-Exception: change-123\n").is_err());
}

fn run(repo: &Path, args: &[&str]) {
    git(repo, args).unwrap();
}

#[test]
fn real_git_index_partial_staging_and_deleted_definition() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    run(repo, &["init", "-q"]);
    run(repo, &["config", "user.name", "fixture"]);
    run(repo, &["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(repo.join("README.md"), "fixture\n").unwrap();
    run(repo, &["add", "--", "README.md"]);
    run(repo, &["commit", "-qm", "base"]);
    let base = git_text(repo, &["rev-parse", "HEAD"]).unwrap();
    std::fs::write(repo.join("clock.timer"), "[Timer]\nOnCalendar=hourly\n").unwrap();
    run(repo, &["add", "--", "clock.timer"]);
    // The worktree looks harmless; the staged timer must STILL be refused.
    std::fs::write(repo.join("clock.timer"), "# unstaged removal\n").unwrap();
    let staged = git_text(repo, &["write-tree"]).unwrap();
    let facts = changed_facts(repo, &base, &staged).unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(
        judge(&[rule(TextTier::Block)], facts).unwrap().outcome,
        "refuse"
    );
    run(repo, &["commit", "-qm", "existing timer fixture"]);
    let current = git_text(repo, &["rev-parse", "HEAD"]).unwrap();
    run(repo, &["add", "--", "clock.timer"]);
    let staged = git_text(repo, &["write-tree"]).unwrap();
    assert!(changed_facts(repo, &current, &staged).unwrap().is_empty());
}
