//! HTTP promotion envelope and Quipu response handling.

use crate::errors::{Error, Result};

/// Post validated Turtle to Quipu's `/knot`. Returns the number of triples the
/// transaction reports as present for these facts — the count that makes
/// idempotence checkable (a re-promotion returns the same count, not a larger one).
///
/// `endpoint` is the Quipu base URL (e.g. from `--to` / config); this appends
/// `/knot`. NEVER defaulted to a hardcoded host — a promotion that silently picks a
/// graph is how facts land in the wrong one.
pub fn write_knot(endpoint: &str, turtle: &str, source: &str) -> Result<KnotResult> {
    write_knot_request(endpoint, turtle, source, None, None, None)
}

/// Atomically replace one stable producer snapshot through `/knot`.
pub fn write_knot_snapshot(
    endpoint: &str,
    turtle: &str,
    source: &str,
    snapshot: &str,
) -> Result<KnotResult> {
    write_knot_request(endpoint, turtle, source, Some(snapshot), None, None)
}

/// Validate an explicitly selected graph without touching an endpoint.
/// Empty, relative or malformed IRIs refuse rather than silently targeting ROOT.
pub fn validate_target_graph(graph: Option<&str>) -> Result<()> {
    if let Some(graph) = graph {
        oxrdf::NamedNode::new(graph)
            .map_err(|e| Error::Promote(format!("invalid target graph IRI {graph:?}: {e}")))?;
    }
    Ok(())
}

pub(super) fn write_knot_request(
    endpoint: &str,
    turtle: &str,
    source: &str,
    snapshot: Option<&str>,
    valid_from: Option<&str>,
    graph: Option<&str>,
) -> Result<KnotResult> {
    validate_target_graph(graph)?;
    let url = format!("{}/knot", endpoint.trim_end_matches('/'));
    let auth = crate::quipu_auth::require(endpoint, None).map_err(Error::Promote)?;
    // Provenance on every write (promotion tail item 4): quipu records actor +
    // source per transaction; an anonymous writer is unauditable, and yupana was
    // the only anonymous one left.
    let mut body = serde_json::json!({
        "turtle": turtle,
        "actor": "yupana",
        "source": source
    });
    if let Some(key) = snapshot {
        body["replace_snapshot"] = serde_json::Value::Bool(true);
        body["snapshot"] = serde_json::Value::String(key.to_string());
    }
    if let Some(time) = valid_from {
        body["valid_from"] = serde_json::Value::String(time.to_string());
    }
    if let Some(graph) = graph {
        body["graph"] = serde_json::Value::String(graph.to_string());
    }
    let body = body.to_string();

    // Quipu is known to flap (transient 503 "no available server", recovering in
    // seconds). Ride through TRANSIENT failures — 5xx and transport errors — with
    // a short backoff; a 4xx is a real answer and fails immediately. The
    // all-or-nothing guarantee is unaffected: every attempt is the same full
    // idempotent write, and exhausting retries still fails loud, never partial.
    const ATTEMPTS: u32 = 3;
    let mut resp = None;
    let mut last_err = String::new();
    for attempt in 1..=ATTEMPTS {
        let mut req = crate::quipu_label::json_post(&url, crate::quipu_label::PROMOTE);
        req = req.set("Authorization", &format!("Bearer {auth}"));
        match req.send_string(&body) {
            Ok(r) => {
                resp = Some(r);
                break;
            }
            Err(ureq::Error::Status(401, _)) => {
                return Err(Error::Promote(crate::quipu_auth::rejected(endpoint, None)));
            }
            Err(ureq::Error::Status(code, _)) if code < 500 => {
                return Err(Error::Promote(format!("POST {url} failed: status {code}")));
            }
            Err(e) => {
                last_err = e.to_string();
                if attempt < ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_secs(2 * u64::from(attempt)));
                }
            }
        }
    }
    let resp = resp.ok_or_else(|| {
        Error::Promote(format!(
            "POST {url} failed after {ATTEMPTS} attempts (transient errors retried): {last_err}"
        ))
    })?;

    let text = resp
        .into_string()
        .map_err(|e| Error::Promote(format!("could not read /knot response: {e}")))?;
    // Quipu can REFUSE the write server-side: its persistent shape registry,
    // when loaded, validates independently of yupana's in-process gate, and a
    // shape the server holds that yupana's copy lacks surfaces HERE as HTTP 200
    // with conforms:false (seen live: a stored symbolKind maxCount(1) refused
    // a projection yupana's shapes accepted). That is a real refusal and must
    // read as one — not as a JSON parse error on a missing `count` field.
    if let Ok(refusal) = serde_json::from_str::<KnotRefusal>(&text) {
        if !refusal.conforms {
            let issues = refusal
                .issues
                .iter()
                .map(|i| format!("{} {} on {}", i.component, i.message, i.focus_node))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(Error::Promote(format!(
                "quipu refused the write (server-side SHACL, {} violation(s)): {issues}. \
                 yupana's own shapes ACCEPTED this projection — the two shape sets have \
                 drifted; reconcile shapes/code-edges.ttl with quipu's stored registry.",
                refusal.violations
            )));
        }
    }
    let parsed: KnotResult = serde_json::from_str(&text)
        .map_err(|e| Error::Promote(format!("unexpected /knot response {text:?}: {e}")))?;
    require_transaction_receipt(&parsed)?;
    if valid_from.is_some() && parsed.valid_from.as_deref().is_none_or(str::is_empty) {
        return Err(Error::Promote(
            "write may have landed, but Quipu did not confirm valid_from; upgrade the server before claiming commit-time provenance".into(),
        ));
    }
    Ok(parsed)
}

fn require_transaction_receipt(result: &KnotResult) -> Result<()> {
    if !matches!(result.tx_id, Some(id) if id > 0 || result.count == 0) {
        return Err(Error::Promote(
            "write may have landed, but Quipu did not return a valid tx_id receipt; promotion is unconfirmed".into(),
        ));
    }
    Ok(())
}

/// Quipu's `/knot` refusal shape (HTTP 200, `conforms:false`).
#[derive(Debug, serde::Deserialize)]
struct KnotRefusal {
    conforms: bool,
    #[serde(default)]
    violations: u64,
    #[serde(default)]
    issues: Vec<KnotIssue>,
}

#[derive(Debug, serde::Deserialize)]
struct KnotIssue {
    #[serde(default)]
    component: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    focus_node: String,
}

/// Quipu `/knot` response. `conforms` here is Quipu's OWN field and is NOT the
/// validation gate — Quipu's persistent shape registry may be empty, in which case
/// it reports `conforms:true` for anything. yupana's gate is [`super::validate`],
/// which ran before this. `count` is the load-bearing field for idempotence.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct KnotResult {
    /// Triples present for these facts after the write — the idempotence signal.
    pub count: u64,
    /// Quipu's transaction receipt; zero is valid only with a zero-count no-op.
    #[serde(default)]
    pub tx_id: Option<u64>,
    /// Normalized valid-time query key returned by Quipu, never the input spelling.
    #[serde(default)]
    pub valid_from: Option<String>,
}

#[cfg(test)]
mod receipt_tests {
    use super::*;

    #[test]
    fn promotion_requires_explicit_integer_transaction_receipt() {
        for body in [
            r#"{"count":1}"#,
            r#"{"count":1,"tx_id":null}"#,
            r#"{"count":1,"tx_id":0}"#,
            r#"{"count":1,"tx_id":-1}"#,
            r#"{"count":1,"tx_id":true}"#,
            r#"{"count":1,"tx_id":"12"}"#,
            r#"{"count":1,"tx_id":1.5}"#,
        ] {
            let accepted = serde_json::from_str::<KnotResult>(body)
                .ok()
                .is_some_and(|result| require_transaction_receipt(&result).is_ok());
            assert!(!accepted, "accepted ambiguous receipt: {body}");
        }
        for count in [0, 1] {
            let result: KnotResult =
                serde_json::from_value(serde_json::json!({"count": count, "tx_id": 12})).unwrap();
            require_transaction_receipt(&result).unwrap();
        }
        let no_op: KnotResult =
            serde_json::from_value(serde_json::json!({"count": 0, "tx_id": 0})).unwrap();
        require_transaction_receipt(&no_op).unwrap();
    }
}
