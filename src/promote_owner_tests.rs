use super::*;
use std::net::TcpListener;

fn files(endpoint: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let pins = dir.path().join("pins.json");
    let token = dir.path().join("token");
    let custody = dir.path().join("attempts");
    std::fs::create_dir(&custody).unwrap();
    std::fs::write(
        &pins,
        json!({"version":1,"endpoint":endpoint,"graph":"urn:test:owner",
        "catalog_revision":7,"owner_generation":4,"policy":"test-policy"})
        .to_string(),
    )
    .unwrap();
    std::fs::write(&token, "synthetic-owner-token\n").unwrap();
    (dir, pins, token, custody)
}

#[test]
fn malformed_pins_and_missing_credentials_refuse_before_transport() {
    let (_dir, pins, token, custody) = files("http://127.0.0.1:1");
    assert!(OwnerTarget::load("http://127.0.0.1:2", &pins, &token, &custody).is_err());
    std::fs::remove_file(&token).unwrap();
    assert!(OwnerTarget::load("http://127.0.0.1:1", &pins, &token, &custody).is_err());
    std::fs::write(&token, "synthetic-owner-token").unwrap();
    std::fs::write(&pins, r#"{"version":1,"graph":"urn:test:owner"}"#).unwrap();
    assert!(OwnerTarget::load("http://127.0.0.1:1", &pins, &token, &custody).is_err());
    assert_eq!(std::fs::read_dir(custody).unwrap().count(), 0);
}

#[test]
fn owner_wire_requires_status_and_never_uses_legacy_or_seals_success() {
    for state in ["finished", "admitted", "committed", "unknown"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (_dir, pins, token, custody) = files(&endpoint);
        let target = OwnerTarget::load(&endpoint, &pins, &token, &custody).unwrap();
        let retained = custody.clone();
        let server = std::thread::spawn(move || {
            let mut original = None;
            let mut write_id = String::new();
            let count = if state == "admitted" { 3 } else { 2 };
            for index in 0..count {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && std::time::Instant::now() < deadline =>
                        {
                            std::thread::sleep(std::time::Duration::from_millis(5))
                        }
                        Err(e) => panic!("missing expected owner request: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let end = loop {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                    if bytes.ends_with(b"\r\n\r\n") {
                        break bytes.len();
                    }
                };
                let headers = String::from_utf8(bytes[..end].to_vec())
                    .unwrap()
                    .to_lowercase();
                let expected = match index {
                    0 => "/owner/knot",
                    1 => "/owner/knot/status",
                    _ => "/owner/knot/seal",
                };
                // MUTANT: replacing the write route with /knot fails this assertion.
                assert!(
                    headers.starts_with(&format!("post {expected} http/1.1\r\n")),
                    "{headers}"
                );
                assert!(headers.contains("authorization: bearer synthetic-owner-token\r\n"));
                for pin in [
                    "x-quipu-completion-contract: quipu.knot-completion/v2",
                    "x-quipu-catalog-revision: 7",
                    "x-quipu-owner-generation: 4",
                    "x-quipu-owner-policy: test-policy",
                ] {
                    assert!(headers.contains(pin));
                }
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                let id = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("x-quipu-write-id: "))
                    .unwrap();
                assert_eq!(id.len(), 64);
                assert!(id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
                let proof = if index == 0 {
                    let attempts: Vec<_> = std::fs::read_dir(&retained)
                        .unwrap()
                        .filter(|e| {
                            e.as_ref()
                                .unwrap()
                                .path()
                                .extension()
                                .is_some_and(|x| x == "json")
                        })
                        .collect();
                    assert_eq!(attempts.len(), 1, "custody must precede the write");
                    let frozen =
                        std::fs::read_to_string(attempts[0].as_ref().unwrap().path()).unwrap();
                    assert!(!frozen.contains("synthetic-owner-token"));
                    let frozen: Value = serde_json::from_str(&frozen).unwrap();
                    assert_eq!(frozen["binding"]["request"], body);
                    assert_eq!(body["graph"], "urn:test:owner");
                    original = Some(body);
                    write_id = id.into();
                    json!({"conforms":true,"count":1,"tx_id":12})
                } else {
                    assert_eq!(id, write_id);
                    assert_eq!(body["request"], *original.as_ref().unwrap());
                    assert_eq!(body["write_id"], write_id);
                    json!({"contract":"quipu.knot-completion/v2","write_id":write_id,"state":if index == 2 { "sealed" } else { state },
                        "commit":{"tx_id":12},"receipt":{"conforms":true,"count":1,"tx_id":12}})
                };
                let encoded = proof.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    encoded.len(),
                    encoded
                )
                .unwrap();
            }
        });
        let result = target.write("<urn:s> <urn:p> 1 .", "test", None, None);
        server.join().unwrap();
        assert_eq!(result.is_ok(), state == "finished", "{state}");
        drop(target);
        assert_eq!(std::fs::read_dir(&custody).unwrap().count(), 1);
        let next = OwnerTarget::load(&endpoint, &pins, &token, &custody);
        assert_eq!(next.is_ok(), matches!(state, "finished" | "admitted"));
    }
}
