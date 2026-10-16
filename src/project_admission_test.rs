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

fn server(replies: Vec<String>) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        for reply in replies {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut stream = loop {
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
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.len(),
                reply
            )
            .unwrap();
        }
    });
    (endpoint, thread)
}

#[test]
fn indexed_fetch_preserves_typed_policy_selector_and_predicate() {
    let rule = fixture();
    let rdf_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    let p = |name| format!("{NS}{name}");
    let replies = vec![
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
    ];
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
