use super::*;

fn fixture() -> ProjectedAdmission {
    serde_json::from_value(serde_json::json!({
        "id":"https://example.org/admission", "rationale":"Current admission required",
        "tier":"warn", "effect":"warn", "verification_point":"PAG", "match_type":"must-match",
        "selector":{"repositories":[{"authority":"github.com","repository":"example/project","protected_refs":["main"]}]},
        "predicate":{"revision":"c".repeat(64),"issuer_public_key":"a".repeat(64),"max_age_secs":120,"required_checks":["current-admission"]}
    })).unwrap()
}

#[test]
fn graph_scope_is_exact_and_no_repository_or_tier_ladder_lives_in_engine() {
    let mut rule = fixture();
    rule.validate().unwrap();
    assert!(rule.selects("github.com", "example/project", "main"));
    assert!(!rule.selects("other.example", "example/project", "main"));
    assert!(!rule.selects("github.com", "other/project", "main"));
    assert!(!rule.selects("github.com", "example/project", "topic"));
    rule.selector.repositories[0].repository = "different/name".into();
    rule.selector.repositories[0].protected_refs = vec!["stable".into()];
    rule.validate().unwrap();
    assert!(rule.selects("github.com", "different/name", "stable"));
}

#[test]
fn invalid_and_ambiguous_contracts_cannot_be_projected_as_empty() {
    for change in ["effect", "tier", "verification_point", "match_type"] {
        let mut value = serde_json::to_value(fixture()).unwrap();
        value[change] = "unsupported".into();
        let rule: ProjectedAdmission = serde_json::from_value(value).unwrap();
        assert!(rule.validate().is_err(), "{change}");
    }
    let mut rule = fixture();
    rule.selector
        .repositories
        .push(rule.selector.repositories[0].clone());
    assert!(rule.validate().is_err());
    rule = fixture();
    rule.selector.repositories.clear();
    assert!(rule.validate().is_err());
    rule = fixture();
    rule.predicate.required_checks.clear();
    assert!(rule.validate().is_err());
    rule = fixture();
    rule.tier = "block".into();
    rule.effect = "deny".into();
    rule.validate().unwrap();
}

fn results(rows: Vec<serde_json::Value>) -> String {
    serde_json::json!({"results":{"bindings":serde_json::Value::Array(rows)}}).to_string()
}

fn properties_response(fields: &[(&str, String)]) -> String {
    results(
        fields
            .iter()
            .map(|(key, value)| {
                serde_json::json!({
                    "property":{"value":key}, "value":{"value":value}
                })
            })
            .collect(),
    )
}

struct Server {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl Server {
    fn join(self) -> std::thread::Result<()> {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.thread.join()
    }
}

fn server(replies: Vec<String>) -> (String, Server) {
    server_with_headers(replies.into_iter().map(|body| (body, None)).collect())
}

fn server_with_headers(replies: Vec<(String, Option<&'static str>)>) -> (String, Server) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = stop.clone();
    let thread = std::thread::spawn(move || {
        for (reply, header) in replies {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut stream = loop {
                if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(error) => panic!("expected request: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut raw = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let n = stream.read(&mut buffer).unwrap();
                assert_ne!(n, 0);
                raw.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&raw);
                if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if body.len() >= length {
                        let request: serde_json::Value =
                            serde_json::from_str(&body[..length]).unwrap();
                        assert!(!request["query"].as_str().unwrap().contains("OPTIONAL"));
                        break;
                    }
                }
            }
            let header = header.map_or_else(String::new, |value| {
                format!("X-Quipu-Truncated: {value}\r\n")
            });
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/sparql-results+json\r\n{header}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.len(),
                reply
            )
            .unwrap();
        }
    });
    (endpoint, Server { stop, thread })
}

fn valid_policy_replies() -> Vec<String> {
    let rule = fixture();
    let rdf_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    let p = |name| format!("{NS}{name}");
    vec![
        results(vec![serde_json::json!({"policy":{"value":rule.id}})]),
        properties_response(&[
            (rdf_type, p("Policy")),
            (&p("selector"), "https://example.org/selector".into()),
            (&p("predicate"), "https://example.org/predicate".into()),
            (&p("enforcementTier"), rule.tier),
            (&p("effect"), rule.effect),
            (&p("verificationPoint"), rule.verification_point),
            (
                "http://www.w3.org/2000/01/rdf-schema#comment",
                rule.rationale,
            ),
        ]),
        properties_response(&[
            (rdf_type, p("Selector")),
            (
                &p("evidenceSource"),
                serde_json::to_string(&rule.selector).unwrap(),
            ),
        ]),
        properties_response(&[
            (rdf_type, p("Predicate")),
            (
                &p("evidenceSource"),
                serde_json::to_string(&rule.predicate).unwrap(),
            ),
            (&p("matchType"), rule.match_type),
        ]),
    ]
}

#[test]
fn indexed_fetch_preserves_typed_policy_selector_and_predicate() {
    let replies = valid_policy_replies();
    let (endpoint, thread) = server(replies);
    let rules = fetch_admission_policies(&endpoint).unwrap();
    thread.join().unwrap();
    assert_eq!(rules.len(), 1);
    assert!(rules[0].selects("github.com", "example/project", "main"));
}

#[test]
fn observed_empty_and_incomplete_member_have_different_results() {
    let (endpoint, thread) = server(vec![results(vec![])]);
    assert!(fetch_admission_policies(&endpoint).unwrap().is_empty());
    thread.join().unwrap();
    let (endpoint, thread) = server(vec![
        results(vec![
            serde_json::json!({"policy":{"value":"https://example.org/incomplete"}}),
        ]),
        results(vec![]),
    ]);
    assert!(fetch_admission_policies(&endpoint)
        .unwrap_err()
        .to_string()
        .contains("typed Policy"));
    thread.join().unwrap();
}

#[test]
fn truncated_replies_never_become_an_observed_catalogue() {
    let truncated = serde_json::json!({"truncated":true,"results":{"bindings":[]}}).to_string();
    let (endpoint, thread) = server(vec![truncated.clone()]);
    assert!(fetch_admission_policies(&endpoint).is_err());
    thread.join().unwrap();
    let (endpoint, thread) = server(vec![
        results(vec![
            serde_json::json!({"policy":{"value":"https://example.org/policy"}}),
        ]),
        truncated,
    ]);
    assert!(fetch_admission_policies(&endpoint)
        .unwrap_err()
        .to_string()
        .contains("truncated"));
    thread.join().unwrap();
}

fn rejects_http_header_at(index: usize) {
    let replies = valid_policy_replies()
        .into_iter()
        .enumerate()
        .map(|(i, body)| (body, (i == index).then_some("true")))
        .collect();
    let (endpoint, server) = server_with_headers(replies);
    let result = fetch_admission_policies(&endpoint);
    server.join().unwrap();
    assert!(
        result.is_err(),
        "accepted partial response at query {index}"
    );
}

#[test]
fn http_header_truncated_nonempty_discovery_is_refused() {
    rejects_http_header_at(0);
}
#[test]
fn http_header_truncated_policy_member_is_refused() {
    rejects_http_header_at(1);
}
#[test]
fn http_header_truncated_selector_member_is_refused() {
    rejects_http_header_at(2);
}
#[test]
fn http_header_truncated_predicate_member_is_refused() {
    rejects_http_header_at(3);
}

#[test]
fn http_header_truncated_empty_discovery_is_not_known_empty() {
    let (endpoint, server) = server_with_headers(vec![(results(vec![]), Some("true"))]);
    let result = fetch_admission_policies(&endpoint);
    server.join().unwrap();
    assert!(result.is_err());
}

#[test]
fn http_header_false_allows_complete_nonempty_catalogue() {
    let replies = valid_policy_replies()
        .into_iter()
        .map(|body| (body, Some("false")))
        .collect();
    let (endpoint, server) = server_with_headers(replies);
    let result = fetch_admission_policies(&endpoint);
    server.join().unwrap();
    assert_eq!(result.unwrap().len(), 1);
}

#[test]
fn http_header_false_allows_observed_empty_catalogue() {
    let (endpoint, server) = server_with_headers(vec![(results(vec![]), Some("false"))]);
    let result = fetch_admission_policies(&endpoint);
    server.join().unwrap();
    assert!(result.unwrap().is_empty());
}

#[test]
fn http_header_invalid_completeness_is_not_known_empty() {
    let (endpoint, server) = server_with_headers(vec![(results(vec![]), Some("unknown"))]);
    let result = fetch_admission_policies(&endpoint);
    server.join().unwrap();
    assert!(result.is_err());
}
