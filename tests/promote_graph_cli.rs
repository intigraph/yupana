//! Exercise graph selection through the real CLI and captured HTTP envelopes.
#![cfg(feature = "quipu")]

use std::io::{Read, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

struct Capture {
    endpoint: String,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<Vec<serde_json::Value>>>,
}

impl Capture {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(45);
            while !done.load(Ordering::SeqCst) && Instant::now() < deadline {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("capture accept: {e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                    assert!(request.len() < 16_384);
                    if request.ends_with(b"\r\n\r\n") {
                        break request.len();
                    }
                };
                let header = String::from_utf8_lossy(&request);
                assert!(header.starts_with("POST /knot HTTP/1.1\r\n"));
                let length = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                assert!(length < 4 * 1024 * 1024);
                request.resize(header_end + length, 0);
                stream.read_exact(&mut request[header_end..]).unwrap();
                let body: serde_json::Value =
                    serde_json::from_slice(&request[header_end..]).unwrap();
                let response = serde_json::json!({"count":1,"tx_id":1,"conforms":true,
                    "valid_from":body["valid_from"]})
                .to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
                bodies.push(body);
            }
            bodies
        });
        Self {
            endpoint,
            stop,
            worker: Some(worker),
        }
    }

    fn finish(mut self) -> Vec<serde_json::Value> {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lib.rs"), "pub fn entry() {}\n").unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "lib.rs"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.org",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());
    }
    std::fs::write(
        dir.path().join("config.toml"),
        "[yupana.quipu]\nenabled=false\n[yupana.serve]\nuse_daemon=false\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("fixture-token"),
        "fixture-only-not-a-credential",
    )
    .unwrap();
    dir
}

fn command(dir: &tempfile::TempDir, endpoint: &str) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("yupana").unwrap();
    cmd.args(["promote", "--repo", "fixture", "--to", endpoint, "--config"])
        .arg(dir.path().join("config.toml"))
        .arg(dir.path())
        .env_remove("QUIPU_AUTH_TOKEN")
        .env("QUIPU_AUTH_TOKEN_FILE", dir.path().join("fixture-token"))
        .timeout(Duration::from_secs(30));
    cmd
}

#[test]
fn graph_reaches_full_subset_and_append_without_changing_identity() {
    let dir = fixture();
    for mode in [
        vec!["--replace-snapshot"],
        vec!["--subset", "--all"],
        vec!["--append"],
    ] {
        let baseline = Capture::start();
        command(&dir, &baseline.endpoint)
            .args(&mode)
            .assert()
            .success();
        let root = baseline.finish();
        assert!(!root.is_empty(), "positive control must actually write");
        assert!(root.iter().all(|b| b.get("graph").is_none()));

        let selected = Capture::start();
        command(&dir, &selected.endpoint)
            .args(&mode)
            .args(["--graph", "urn:fixture:ingest"])
            .assert()
            .success();
        let mut routed = selected.finish();
        assert_eq!(root.len(), routed.len());
        for body in &mut routed {
            assert_eq!(body["graph"], "urn:fixture:ingest");
            body.as_object_mut().unwrap().remove("graph");
        }
        assert_eq!(
            root, routed,
            "graph selection must not change Turtle, snapshot, actor, source or valid time"
        );
        println!("{mode:?}: ROOT -> urn:fixture:ingest; {} request(s); payload and snapshot identity unchanged", root.len());
    }
}

#[test]
fn invalid_graph_refuses_before_network_and_dry_run_never_writes() {
    let dir = fixture();
    let capture = Capture::start();
    for graph in ["", "relative/path", "urn:has space", "<urn:wrapped>"] {
        command(&dir, &capture.endpoint)
            .args(["--replace-snapshot", "--graph", graph])
            .assert()
            .failure();
    }
    command(&dir, &capture.endpoint)
        .args(["--dry-run", "--graph", "urn:fixture:ingest"])
        .assert()
        .success();
    assert!(capture.finish().is_empty());
}

#[test]
fn selecting_a_graph_does_not_authorize_a_discovered_endpoint() {
    let dir = fixture();
    let capture = Capture::start();
    std::fs::write(
        dir.path().join("config.toml"),
        format!(
            "[yupana.quipu]\nenabled=false\nendpoint={:?}\n[yupana.serve]\nuse_daemon=false\n",
            capture.endpoint
        ),
    )
    .unwrap();
    assert_cmd::Command::cargo_bin("yupana")
        .unwrap()
        .args([
            "promote",
            "--repo",
            "fixture",
            "--replace-snapshot",
            "--graph",
            "urn:fixture:ingest",
            "--config",
        ])
        .arg(dir.path().join("config.toml"))
        .arg(dir.path())
        .env_remove("QUIPU_AUTH_TOKEN")
        .env("QUIPU_AUTH_TOKEN_FILE", dir.path().join("fixture-token"))
        .timeout(Duration::from_secs(30))
        .assert()
        .failure();
    assert!(capture.finish().is_empty());
}
