use std::path::Path;
use std::time::Instant;

use super::*;

/// A judge stub: records its argv to `<dir>/argv`, then runs `body`.
fn stub(dir: &Path, body: &str) -> Vec<String> {
    let script = format!(
        "for a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}/argv'\n{body}",
        dir.display()
    );
    vec!["sh".into(), "-c".into(), script, "judge".into()]
}

fn advise(c: &DocsHistoryConfig, file: &Path, cwd: &Path, old: &str, new: &str) -> Option<String> {
    advisory(c, crate::audit::PathRecording::Off, file, cwd, old, new)
}

fn config(command: Vec<String>) -> DocsHistoryConfig {
    DocsHistoryConfig {
        enabled: true,
        command,
        ..DocsHistoryConfig::default()
    }
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    dir
}

const HISTORY: &str = "The old exporter was retired on 2026-09-20; we moved to the new one.\n";
const ANSWER_LOW: &str = r#"echo '{"answers":{"q":{"type":"noul","noul":0.1}}}'"#;
const ANSWER_HIGH: &str = r#"echo '{"answers":{"q":{"type":"noul","noul":0.85}}}'"#;

#[test]
fn product_docs_are_in_scope_and_runbooks_are_not() {
    let c = DocsHistoryConfig::default();
    for rel in [
        "README.md",
        "crates/x/README.md",
        "CLAUDE.md",
        "skills/a/SKILL.md",
        "docs/reference/guard.md",
        "docs/book/src/reference/policy-guard.md",
    ] {
        assert!(in_scope(&c, rel), "{rel} should be in scope");
    }
    for rel in [
        "docs/runbooks/retired.md",
        "ops/runbooks/README.md",
        "src/main.rs",
        "docs/design/plan.md",
        "CHANGELOG.md",
    ] {
        assert!(!in_scope(&c, rel), "{rel} should be out of scope");
    }
}

#[test]
fn only_added_lines_are_judged() {
    let old = "# Title\n\nKept line.\n";
    let new = "# Title\n\nKept line.\nX was retired in 2026.\n\n";
    assert_eq!("X was retired in 2026.", added_lines(old, new));
    assert_eq!("", added_lines(old, old));
}

#[test]
fn both_answer_shapes_parse_and_nonsense_does_not() {
    assert_eq!(Some(0.1), parse_noul(r#"{"answers":{"q":{"noul":0.1}}}"#));
    assert_eq!(Some(0.7), parse_noul(r#"{"answer":{"noul":0.7}}"#));
    for bad in [
        "",
        "not json",
        r#"{"answers":{}}"#,
        r#"{"answer":{"noul":1.5}}"#,
        r#"{"answer":{"noul":"0.1"}}"#,
    ] {
        assert_eq!(None, parse_noul(bad), "{bad}");
    }
}

#[test]
fn a_history_hunk_gets_the_advisory_with_its_score() {
    let dir = repo();
    let file = dir.path().join("README.md");
    let text = advise(
        &config(stub(dir.path(), ANSWER_LOW)),
        &file,
        dir.path(),
        "",
        HISTORY,
    )
    .expect("advisory");
    assert!(
        text.contains("README.md") && text.contains("0.10"),
        "{text}"
    );
    assert!(text.contains("present-tense"), "{text}");
}

#[test]
fn the_judge_sees_the_added_lines_and_the_calibrated_question() {
    let dir = repo();
    let file = dir.path().join("README.md");
    let _ = advise(
        &config(stub(dir.path(), ANSWER_LOW)),
        &file,
        dir.path(),
        "# Demo\n",
        &format!("# Demo\n{HISTORY}"),
    );
    let argv = std::fs::read_to_string(dir.path().join("argv")).unwrap();
    let args: Vec<&str> = argv.lines().collect();
    assert_eq!(&["--state", HISTORY.trim_end(), "--ask"], &args[..3]);
    assert!(argv.contains(QUESTION));
}

#[test]
fn a_current_state_hunk_gets_nothing() {
    let dir = repo();
    let file = dir.path().join("README.md");
    let c = config(stub(dir.path(), ANSWER_HIGH));
    assert_eq!(
        None,
        advise(&c, &file, dir.path(), "", "X is not supported; use Y.\n")
    );
    assert!(
        dir.path().join("argv").exists(),
        "the control must actually reach the judge"
    );
}

#[test]
fn every_judge_failure_is_silence() {
    let dir = repo();
    let file = dir.path().join("README.md");
    for body in ["exit 1", "echo nonsense", "true", "exit 0"] {
        let c = config(stub(dir.path(), body));
        assert_eq!(None, advise(&c, &file, dir.path(), "", HISTORY), "{body}");
    }
    let missing = config(vec!["/nonexistent/judge".into()]);
    assert_eq!(None, advise(&missing, &file, dir.path(), "", HISTORY));
}

#[test]
fn a_slow_judge_is_abandoned_at_the_deadline() {
    let dir = repo();
    let file = dir.path().join("README.md");
    let mut c = config(stub(dir.path(), &format!("sleep 5; {ANSWER_LOW}")));
    c.deadline_ms = 200;
    let started = Instant::now();
    assert_eq!(None, advise(&c, &file, dir.path(), "", HISTORY));
    assert!(
        started.elapsed().as_millis() < 2000,
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn nothing_runs_when_off_out_of_scope_or_without_new_words() {
    let dir = repo();
    let low = stub(dir.path(), ANSWER_LOW);
    let readme = dir.path().join("README.md");
    let mut off = config(low.clone());
    off.enabled = false;
    assert_eq!(None, advise(&off, &readme, dir.path(), "", HISTORY));
    let runbook = dir.path().join("docs/runbooks/old.md");
    assert_eq!(
        None,
        advise(&config(low.clone()), &runbook, dir.path(), "", HISTORY)
    );
    let code = dir.path().join("src/lib.rs");
    assert_eq!(
        None,
        advise(&config(low.clone()), &code, dir.path(), "", HISTORY)
    );
    assert_eq!(
        None,
        advise(&config(low.clone()), &readme, dir.path(), "", "---\n\n")
    );
    let long = "word ".repeat(MAX_TEXT);
    assert_eq!(None, advise(&config(low), &readme, dir.path(), "", &long));
    assert!(
        !dir.path().join("argv").exists(),
        "the judge must not have been called"
    );
}

#[test]
fn the_path_is_scoped_from_the_files_own_repo_not_the_cwd() {
    let outer = tempfile::tempdir().unwrap();
    let inner = outer.path().join("sub");
    std::fs::create_dir_all(inner.join(".git")).unwrap();
    let file = inner.join("README.md");
    let c = config(stub(outer.path(), ANSWER_LOW));
    assert!(advise(&c, &file, outer.path(), "", HISTORY).is_some());
}
