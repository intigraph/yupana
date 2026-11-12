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
    let replies = match tier {
        Some(tier) => vec![
            json!({"results":{"bindings":[{"s":{"type":"uri","value":"https://example.org/scheduler-policy"}}]}}),
            json!({"results":{"bindings":policy(tier)}}),
        ],
        None => vec![json!({"results":{"bindings":[]}})],
    };
    server_replies(replies)
}

fn server_replies(replies: Vec<Value>) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
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
            if let Some(iri) = reply["results"]["bindings"][0]["repo"]["value"].as_str() {
                let repository = iri.rsplit('/').next().unwrap();
                let request = String::from_utf8(request).unwrap();
                assert!(
                    request.contains(&format!("https://example.org/team/{repository}")),
                    "identity query did not use actual origin: {request}"
                );
                assert!(
                    request.contains("externalUrl"),
                    "identity query omitted governed registration"
                );
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

#[cfg(unix)]
#[test]
fn actual_cli_exception_is_bound_to_derived_repo_path_and_open_lifetime() {
    use std::os::unix::fs::PermissionsExt;
    for (case, repository, description, status, allowed) in [
        (
            "bound",
            "one",
            "Repository: https://example.org/repos/one\nclock.timer",
            "open",
            true,
        ),
        (
            "cross-repository",
            "two",
            "Repository: https://example.org/repos/one\nclock.timer",
            "open",
            false,
        ),
        (
            "wrong-path",
            "one",
            "Repository: https://example.org/repos/one\nother.timer",
            "open",
            false,
        ),
        ("missing-repository", "one", "clock.timer", "open", false),
        (
            "closed",
            "one",
            "Repository: https://example.org/repos/one\nclock.timer",
            "closed",
            false,
        ),
    ] {
        let dir = fixture();
        let repo = dir.path();
        ok(
            repo,
            &[
                "remote",
                "add",
                "origin",
                &format!("https://example.org/team/{repository}.git"),
            ],
        );
        std::fs::write(repo.join("clock.timer"), "[Timer]\nOnCalendar=hourly\n").unwrap();
        ok(repo, &["add", "--", "clock.timer"]);
        let message = repo.join("message");
        std::fs::write(&message, "timer\n\nSchedule-Exception: change-123\n").unwrap();
        let bin = repo.join("fixture-bin");
        std::fs::create_dir(&bin).unwrap();
        let reader = bin.join("sd");
        let response = json!([{"id":"change-123", "status":status, "parent":"change-root", "description":description}]).to_string();
        // A private finite reader fixture: no credential/config copied and no
        // production read/write. The repository identity comes from Git+HTTP.
        std::fs::write(
            &reader,
            format!("#!/bin/sh\ncat <<'RESULT'\n{response}\nRESULT\n"),
        )
        .unwrap();
        std::fs::set_permissions(&reader, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (endpoint, thread) = server_replies(vec![
            json!({"results":{"bindings":[{"s":{"type":"uri","value":"https://example.org/scheduler-policy"}}]}}),
            json!({"results":{"bindings":policy("block")}}),
            json!({"results":{"bindings":[{"repo":{"type":"uri","value":format!("https://example.org/repos/{repository}")}}]}}),
        ]);
        let path = std::env::join_paths(
            std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_yupana"))
            .args(["commit-check", "--repo"])
            .arg(repo)
            .args(["--endpoint", &endpoint, "--message-file"])
            .arg(&message)
            .env("PATH", path)
            .env("YUPANA_METRICS_PATH", repo.join("metrics.jsonl"))
            .output()
            .unwrap();
        thread.join().unwrap();
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.success(), allowed, "{case}: {report}");
        assert_eq!(
            report["exception"].as_str(),
            allowed.then_some("change-123"),
            "{case}"
        );
        assert_eq!(
            report["outcome"],
            if allowed { "allow" } else { "unknown" },
            "{case}"
        );
    }
}

/// Answer exactly one request with a bare HTTP status and no body.
fn status_server(code: u16) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        // Read the WHOLE request (headers + Content-Length body) before
        // answering. Closing with unread bytes makes the kernel send RST, and the
        // client can then see "connection reset" instead of this status: a race
        // that turned the 400 row into "unavailable" in the CI release feature set.
        let mut request = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0, "client closed before sending its request");
            request.extend_from_slice(&chunk[..n]);
            if let Some(p) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = std::str::from_utf8(&request[..p]).unwrap();
                let len: usize = head
                    .lines()
                    .find_map(|line| {
                        line.split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .map(|(_, v)| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= p + 4 + len {
                    break;
                }
            }
        }
        write!(
            stream,
            "HTTP/1.1 {code} Fixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.flush().unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
    });
    (endpoint, thread)
}

#[test]
fn unreachable_quipu_is_unavailable_and_counted_but_a_refusing_quipu_still_fails_closed() {
    // An outage must not refuse every commit,
    // must never read as a pass, and must not become a way past a real answer.
    let dir = fixture();
    let repo = dir.path();
    std::fs::write(repo.join("clock.timer"), "[Timer]\nOnCalendar=hourly\n").unwrap();
    ok(repo, &["add", "--", "clock.timer"]);
    let message = repo.join("message");
    std::fs::write(&message, "add timer\n").unwrap();
    let refused = {
        // Bind then drop: nothing listens on this port any more.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    let run = |endpoint: &str, spool: &Path| {
        Command::new(env!("CARGO_BIN_EXE_yupana"))
            .arg("commit-check")
            .arg("--repo")
            .arg(repo)
            .arg("--endpoint")
            .arg(endpoint)
            .arg("--message-file")
            .arg(&message)
            .env("YUPANA_METRICS_PATH", spool)
            .output()
            .unwrap()
    };
    for (case, code) in [("refused", None), ("503", Some(503)), ("400", Some(400))] {
        let spool = repo.join(format!("metrics-{case}.jsonl"));
        let (endpoint, thread) = match code {
            Some(code) => {
                let (endpoint, thread) = status_server(code);
                (endpoint, Some(thread))
            }
            None => (refused.clone(), None),
        };
        let output = run(&endpoint, &spool);
        if let Some(thread) = thread {
            thread.join().unwrap();
        }
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        let metric: Value =
            serde_json::from_str(std::fs::read_to_string(&spool).unwrap().trim()).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        if case == "400" {
            // quipu answered and refused the request: that is data, not an outage.
            assert!(!output.status.success(), "{case}: {report}");
            assert_eq!(report["outcome"], "unknown", "{case}");
            assert_eq!(metric["result"], "unknown", "{case}");
        } else {
            assert!(output.status.success(), "{case}: {report} {stderr}");
            assert_eq!(report["outcome"], "unavailable", "{case}");
            assert_eq!(metric["result"], "unavailable", "{case}");
            assert_ne!(
                metric["result"], "allow",
                "{case}: an outage is never a pass"
            );
            assert!(stderr.contains("UNAVAILABLE"), "{case}: not loud: {stderr}");
        }
    }
}
