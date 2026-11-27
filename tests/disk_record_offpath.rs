//! The off-path disk-delta writer completes a write slower than the old inline
//! 3 s bound (aegis-u4f: `/update` p99 was 4.8 s and `yupana-hook` was the main
//! cancelled client).
#![cfg(feature = "quipu")]
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    time::Duration,
};

fn read_request(connection: &mut std::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let n = connection.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
        if let Some(split) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            let length = String::from_utf8_lossy(&bytes[..split])
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|s| s.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= split + 4 + length {
                break;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn a_write_slower_than_the_old_inline_bound_is_recorded_not_cancelled() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut client_labels = Vec::new();
        for connection in listener.incoming().take(2) {
            let mut connection = connection.unwrap();
            let request = read_request(&mut connection);
            let body = if request.starts_with("POST /update?") {
                client_labels.push(
                    request
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("x-quipu-client:"))
                        .map(str::to_owned),
                );
                // Slower than the retired 3 s client timeout, under the new one.
                std::thread::sleep(Duration::from_secs(4));
                r#"{"asserted":9,"retracted":0,"tx":7}"#.to_string()
            } else {
                r#"{"results":{"bindings":[]}}"#.to_string()
            };
            let _ = write!(
                connection,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
        client_labels
    });

    let home = tempfile::tempdir().unwrap();
    let spool = home.path().join("metrics.jsonl");
    let job = serde_json::json!({
        "endpoint": format!("http://{address}"),
        "signature": "cargo:build|repo:x|cwd:root",
        "filesystem": "root:test",
        "observed_at": "2026-11-27T06:00:00+00:00",
        "delta": 4096,
        "session": "fixture-session",
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_yupana"))
        .args(["hook", "disk-record"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("YUPANA_METRICS_PATH", &spool)
        .env("QUIPU_AUTH_TOKEN", "fixture-token")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(job.to_string().as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());

    let labels = server.join().unwrap();
    assert_eq!(labels.len(), 1, "exactly one /update");
    assert!(
        labels[0]
            .as_deref()
            .is_some_and(|l| l.contains("yupana-hook")),
        "the after-measure reads the same client series: {labels:?}"
    );
    let spooled = std::fs::read_to_string(&spool).unwrap();
    assert!(
        spooled.contains("\"disk_delta_write\"") && spooled.contains("\"recorded\""),
        "outcome must be spooled as recorded: {spooled}"
    );
}
