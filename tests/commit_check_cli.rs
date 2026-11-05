//! Actual Git commit-msg refusal and CI-range controls. The policy endpoint is
//! a finite local fixture, never the production store or a synthetic signer.
#![cfg(feature = "quipu")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn policy(tier: &str) -> Vec<Value> {
    let property = |name: &str, value: &str| {
        json!({
            "property":{"type":"uri","value":format!("http://aegis.gastown.local/ontology/{name}")},
            "value":{"type":"literal","value":value}
        })
    };
    vec![
        property("regex", "(?m)^external-scheduler/v1:"),
        property("enforcementTier", tier),
        property("identifierClass", "commit-scheduler/v1:change-root"),
    ]
}

fn server(tier: Option<&str>) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let replies = match tier {
        Some(tier) => vec![
            json!({"results":{"bindings":[{"s":{"type":"uri","value":"https://example.org/scheduler-policy"}}]}}),
            json!({"results":{"bindings":policy(tier)}}),
        ],
        None => vec![json!({"results":{"bindings":[]}})],
    };
    let thread = std::thread::spawn(move || {
        for reply in replies {
            let start = Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            start.elapsed() < Duration::from_secs(20),
                            "native command did not request policy"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&chunk[..n]);
                assert!(request.len() < 64 * 1024);
                if let Some(p) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                    let head = std::str::from_utf8(&request[..p]).unwrap();
                    let len: usize = head
                        .lines()
                        .find_map(|line| {
                            line.split_once(':')
                                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                .map(|(_, v)| v.trim().parse().unwrap())
                        })
                        .unwrap();
                    if request.len() >= p + 4 + len {
                        break;
                    }
                }
            }
            let reply = reply.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", reply.len(), reply).unwrap();
        }
    });
    (endpoint, thread)
}

fn git(repo: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap()
}

fn ok(repo: &Path, args: &[&str]) {
    let output = git(repo, args);
    assert!(output.status.success(), "{args:?}: {output:?}");
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    ok(repo, &["init", "-q"]);
    ok(repo, &["config", "user.name", "fixture"]);
    ok(repo, &["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(repo.join("README.md"), "control\n").unwrap();
    ok(repo, &["add", "--", "README.md"]);
    ok(repo, &["commit", "-qm", "base"]);
    dir
}

#[cfg(unix)]
#[test]
fn actual_commit_msg_gate_refuses_timer_and_counts_refusal() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fixture();
    let repo = dir.path();
    let (endpoint, thread) = server(Some("block"));
    let hook = repo.join(".git/hooks/commit-msg");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nexec '{}' commit-check --endpoint '{}' --message-file \"$1\"\n",
            env!("CARGO_BIN_EXE_yupana"),
            endpoint
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(repo.join("new.timer"), "[Timer]\nOnCalendar=hourly\n").unwrap();
    ok(repo, &["add", "--", "new.timer"]);
    let spool = repo.join("metrics.jsonl");
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["commit", "-qm", "new timer"])
        .env("YUPANA_METRICS_PATH", &spool)
        .output()
        .unwrap();
    thread.join().unwrap();
    assert!(
        !output.status.success(),
        "timer commit accepted: {output:?}"
    );
    // Git forwards hook stdout onto its diagnostic stream.
    let response = format!(
        "{}{}",
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap()
    );
    assert!(response.contains("\"refuse\""), "{response}");
    assert!(response.contains("new.timer"), "{response}");
    let metric: Value =
        serde_json::from_str(std::fs::read_to_string(spool).unwrap().trim()).unwrap();
    assert_eq!(metric["kind"], "commit-schedule");
    assert_eq!(metric["result"], "refuse");
    // No commit was made; the staged candidate and normal base are preserved.
    assert_eq!(
        String::from_utf8(git(repo, &["log", "-1", "--format=%s"]).stdout)
            .unwrap()
            .trim(),
        "base"
    );
}

#[test]
fn actual_cli_advise_clean_ci_range_and_missing_policy_unknown() {
    let dir = fixture();
    let repo = dir.path();
    std::fs::write(repo.join("clock.timer"), "[Timer]\nOnCalendar=hourly\n").unwrap();
    ok(repo, &["add", "--", "clock.timer"]);
    let message = repo.join("message");
    std::fs::write(&message, "add timer\n").unwrap();
    for (tier, expected, success) in [(Some("warn"), "advise", true), (None, "unknown", false)] {
        let (endpoint, thread) = server(tier);
        let output = Command::new(env!("CARGO_BIN_EXE_yupana"))
            .arg("commit-check")
            .arg("--repo")
            .arg(repo)
            .arg("--endpoint")
            .arg(endpoint)
            .arg("--message-file")
            .arg(&message)
            .env("YUPANA_METRICS_PATH", repo.join("metrics.jsonl"))
            .output()
            .unwrap();
        thread.join().unwrap();
        assert_eq!(output.status.success(), success, "{output:?}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["outcome"], expected);
    }
    ok(repo, &["commit", "-qm", "existing timer"]);
    std::fs::write(repo.join("clock.timer"), "# removed scheduler\n").unwrap();
    ok(repo, &["add", "--", "clock.timer"]);
    ok(repo, &["commit", "-qm", "remove scheduler"]);
    let (endpoint, thread) = server(Some("block"));
    let output = Command::new(env!("CARGO_BIN_EXE_yupana"))
        .arg("commit-check")
        .arg("--repo")
        .arg(repo)
        .args(["--base", "HEAD^", "--to", "HEAD", "--endpoint", &endpoint])
        .env("YUPANA_METRICS_PATH", repo.join("metrics.jsonl"))
        .output()
        .unwrap();
    thread.join().unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], "allow");
}

#[test]
fn data_formats_allow_but_unsupported_executable_unknown_still_refuses_in_warn() {
    let dir = fixture();
    let repo = dir.path();
    for extension in ["json", "jsonl", "nt", "nq", "ttl", "csv"] {
        let path = format!("archive.{extension}");
        std::fs::write(repo.join(&path), "crontab -\nOnCalendar=hourly\n").unwrap();
        ok(repo, &["add", "--", &path]);
    }
    let message = repo.join("message");
    std::fs::write(&message, "data edit\n").unwrap();
    for expected in ["allow", "unknown", "advise"] {
        if expected == "unknown" {
            std::fs::write(
                repo.join("task.py"),
                "import subprocess\nsubprocess.run(['crontab', '-'])\n",
            )
            .unwrap();
            ok(repo, &["add", "--", "task.py"]);
        } else if expected == "advise" {
            ok(repo, &["rm", "--cached", "--", "task.py"]);
            std::fs::remove_file(repo.join("task.py")).unwrap();
            std::fs::write(repo.join("clock.timer"), "[Timer]\nOnCalendar=hourly\n").unwrap();
            ok(repo, &["add", "--", "clock.timer"]);
        }
        let (endpoint, thread) = server(Some("warn"));
        let output = Command::new(env!("CARGO_BIN_EXE_yupana"))
            .arg("commit-check")
            .arg("--repo")
            .arg(repo)
            .arg("--message-file")
            .arg(&message)
            .arg("--endpoint")
            .arg(endpoint)
            .env("YUPANA_METRICS_PATH", repo.join("metrics.jsonl"))
            .output()
            .unwrap();
        thread.join().unwrap();
        assert_eq!(output.status.success(), expected != "unknown", "{output:?}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["outcome"], expected);
    }
}

#[test]
fn committed_ci_range_refuses_amend_that_drops_its_exception() {
    let dir = fixture();
    let repo = dir.path();
    let base = String::from_utf8(git(repo, &["rev-parse", "HEAD"]).stdout).unwrap();
    std::fs::write(repo.join("clock.timer"), "[Timer]\nOnCalendar=hourly\n").unwrap();
    ok(repo, &["add", "--", "clock.timer"]);
    ok(
        repo,
        &[
            "commit",
            "-qm",
            "add timer\n\nSchedule-Exception: change-123",
        ],
    );
    ok(repo, &["commit", "--amend", "-qm", "drop the trailer"]);
    let (endpoint, thread) = server(Some("block"));
    let output = Command::new(env!("CARGO_BIN_EXE_yupana"))
        .args(["commit-check", "--repo"])
        .arg(repo)
        .args([
            "--endpoint",
            &endpoint,
            "--base",
            base.trim(),
            "--to",
            "HEAD",
        ])
        .env("YUPANA_METRICS_PATH", repo.join("amend-metrics.jsonl"))
        .output()
        .unwrap();
    thread.join().unwrap();
    assert!(
        !output.status.success(),
        "amended timer admitted: {output:?}"
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], "refuse");
    assert_eq!(report["additions"].as_array().unwrap().len(), 1);
    assert!(report["exception"].is_null());
}
