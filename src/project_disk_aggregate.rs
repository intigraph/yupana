//! Bounded command-class summaries; optimistic updates never overwrite a race.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const BUCKETS: usize = 66;
const MAX_STATE_BYTES: usize = 32 * 1024;

/// Finite classes: operands, repository names and cwd never become a key.
pub(crate) fn command_class(signature: &str) -> String {
    let head = signature.split('|').next().unwrap_or("");
    let (binary, sub) = head.split_once(':').unwrap_or((head, "_"));
    let sub = match binary {
        "cargo" => match sub {
            "build" | "test" | "check" | "clippy" | "fmt" | "clean" | "install" | "doc" | "run"
            | "fetch" => sub,
            _ => "other",
        },
        "npm" | "pnpm" | "yarn" => match sub {
            "install" | "ci" | "build" | "test" | "run" | "add" | "remove" => sub,
            _ => "other",
        },
        "docker" | "podman" => match sub {
            "build" | "pull" | "run" | "compose" | "image" | "system" => sub,
            _ => "other",
        },
        "rustc" | "dd" | "fallocate" | "cp" | "rsync" | "tar" => "_",
        "tambo" | "caboodle" => match sub {
            "apply" | "install" | "update" => sub,
            _ => "other",
        },
        _ => return "other:_".into(),
    };
    format!("{binary}:{sub}")
}

pub(crate) fn subject(signature: &str, filesystem: &str) -> String {
    let class = command_class(signature);
    let hash = hex::encode(Sha256::digest(format!("{filesystem}\0{class}").as_bytes()));
    format!("{}disk-impact-class-{hash}", crate::export::ONTO)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct State {
    version: u8,
    count: u64,
    maximum: i64,
    last_seen: String,
    // Fixed signed-byte histogram: negatives, zero, then powers-of-two upper
    // bounds. No raw per-run samples are retained in the graph.
    bins: Vec<u64>,
    last_id: String,
    last_delta: i64,
}

fn bucket(delta: i64) -> usize {
    if delta < 0 {
        0
    } else if delta == 0 {
        1
    } else {
        2 + (64 - (delta.unsigned_abs() - 1).leading_zeros()) as usize
    }
}

impl State {
    fn decode(body: &str) -> Result<Self, String> {
        if body.len() > MAX_STATE_BYTES {
            return Err("oversized disk aggregate".into());
        }
        let state: Self = serde_json::from_str(body).map_err(|e| e.to_string())?;
        let sum = state
            .bins
            .iter()
            .try_fold(0u64, |sum, n| sum.checked_add(*n));
        if state.version != 1
            || state.count == 0
            || state.bins.len() != BUCKETS
            || sum != Some(state.count)
            || chrono::DateTime::parse_from_rfc3339(&state.last_seen).is_err()
            || state.last_id.len() != 64
            || !state.last_id.bytes().all(|b| b.is_ascii_hexdigit())
            || state.last_delta > state.maximum
            || state.bins[bucket(state.last_delta)] == 0
            || state
                .bins
                .iter()
                .enumerate()
                .any(|(index, n)| *n > 0 && index > bucket(state.maximum))
        {
            return Err("invalid disk aggregate".into());
        }
        if serde_json::to_string(&state).map_err(|e| e.to_string())? != body {
            return Err("noncanonical aggregate state".into());
        }
        Ok(state)
    }

    fn record(
        previous: Option<&Self>,
        id: &str,
        delta: i64,
        observed: &str,
    ) -> Result<Self, String> {
        let at = chrono::DateTime::parse_from_rfc3339(observed).map_err(|e| e.to_string())?;
        let mut state = previous.cloned().unwrap_or(Self {
            version: 1,
            count: 0,
            maximum: delta,
            last_seen: at.to_rfc3339(),
            bins: vec![0; BUCKETS],
            last_id: String::new(),
            last_delta: delta,
        });
        if state.last_id == id {
            return if state.last_delta == delta {
                Ok(state)
            } else {
                Err("retry delta differs".into())
            };
        }
        state.count = state
            .count
            .checked_add(1)
            .ok_or("aggregate count overflow")?;
        state.maximum = state.maximum.max(delta);
        if at > chrono::DateTime::parse_from_rfc3339(&state.last_seen).map_err(|e| e.to_string())? {
            state.last_seen = at.to_rfc3339();
        }
        let index = bucket(delta);
        state.bins[index] = state.bins[index]
            .checked_add(1)
            .ok_or("histogram overflow")?;
        state.last_id = id.into();
        state.last_delta = delta;
        Ok(state)
    }

    pub(crate) fn maximum(&self) -> i64 {
        self.maximum
    }
    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    // Nearest-rank conservative bucket upper bound, capped by exact maximum.
    fn quantile(&self, numerator: u64) -> i64 {
        let rank = (u128::from(numerator) * u128::from(self.count)).div_ceil(100);
        let mut seen = 0u128;
        for (index, n) in self.bins.iter().enumerate() {
            seen += u128::from(*n);
            if seen >= rank {
                let bound = match index {
                    0 | 1 => 0,
                    65 => i64::MAX,
                    _ => 1i64 << (index - 2),
                };
                return bound.min(self.maximum);
            }
        }
        self.maximum
    }
}

/// Read one known subject, never scan all historical observations.
pub(crate) fn read(
    endpoint: &str,
    signature: &str,
    filesystem: &str,
) -> Result<Option<(String, State)>, String> {
    let iri = subject(signature, filesystem);
    let query = format!(
        "SELECT ?state WHERE {{ <{iri}> <{}aggregateState> ?state }}",
        crate::export::ONTO
    );
    let body = crate::project::query(endpoint, &query).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let rows = crate::project_decode::rows_of(&value).map_err(|e| e.to_string())?;
    match rows.as_slice() {
        [] => Ok(None),
        [row] => {
            let body = crate::project_decode::binding_value(row, "state")
                .ok_or("missing aggregate state")?;
            let state = State::decode(&body)?;
            Ok(Some((body, state)))
        }
        _ => Err("multiple aggregate states".into()),
    }
}

fn literal(value: &str) -> String {
    format!("\"{}\"", crate::sparql_steps::literal(value))
}

fn triples(iri: &str, signature: &str, filesystem: &str, state: &State) -> Result<String, String> {
    let class = command_class(signature);
    let body = serde_json::to_string(state).map_err(|e| e.to_string())?;
    Ok(format!(
        "<{iri}> a aegis:CommandDiskImpactObservation ; \
        aegis:commandSignature {} ; aegis:commandClass {} ; aegis:filesystemIdentity {} ; \
        aegis:aggregateState {} ; aegis:runCount \"{}\"^^xsd:integer ; \
        aegis:maximumDeltaBytes \"{}\"^^xsd:integer ; aegis:p95DeltaBytes \"{}\"^^xsd:integer ; \
        aegis:diskDeltaBytes \"{}\"^^xsd:integer ; aegis:observedAt {}^^xsd:dateTime ; rdfs:label {} .",
        literal(&format!("{class}|repo:all|cwd:all")), literal(&class), literal(filesystem), literal(&body),
        state.count, state.maximum, state.quantile(95), state.quantile(90), literal(&state.last_seen),
        literal(&format!("disk impact aggregate {class}"))
    ))
}

fn update(
    iri: &str,
    signature: &str,
    filesystem: &str,
    previous: Option<&(String, State)>,
    next: &State,
) -> Result<String, String> {
    let prefix = format!("PREFIX aegis: <{}> PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#> ", crate::export::ONTO);
    let insert = triples(iri, signature, filesystem, next)?;
    match previous {
        None => Ok(format!(
            "{prefix}INSERT {{ {insert} }} WHERE {{ FILTER NOT EXISTS {{ <{iri}> ?p ?o }} }}"
        )),
        Some((body, old)) => {
            let delete = triples(iri, signature, filesystem, old)?;
            let expected = literal(body);
            Ok(format!("{prefix}DELETE {{ {delete} }} INSERT {{ {insert} }} WHERE {{ <{iri}> aegis:aggregateState {expected} . FILTER NOT EXISTS {{ <{iri}> aegis:aggregateState ?other . FILTER(?other != {expected}) }} }}"))
        }
    }
}

/// Update one coherent bounded summary. Conflicts retry at most three times;
/// network ambiguity is read back, never blindly counted twice. Auth refusals
/// are terminal. The last sample ID verifies immediate retries; an ambiguous
/// result overwritten by another writer remains UNKNOWN, never a blind retry.
enum WriteFailure {
    Terminal(String),
    Uncertain(String),
}

fn send(
    endpoint: &str,
    token: &str,
    session: Option<&str>,
    query: &str,
) -> Result<serde_json::Value, WriteFailure> {
    let url = format!(
        "{}/update?actor=yupana&source=command-disk-impact-aggregate",
        endpoint.trim_end_matches('/')
    );
    let request = crate::quipu_label::json_post(&url, crate::quipu_label::HOOK)
        .set("Content-Type", "application/sparql-update")
        .set("Authorization", &format!("Bearer {token}"));
    let request = crate::quipu_provenance::with_hook_session(request, session);
    let response = match request
        .timeout(std::time::Duration::from_secs(3))
        .send_string(query)
    {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) => {
            return Err(WriteFailure::Terminal(
                crate::quipu_auth::rejected_for_hook(endpoint, session),
            ))
        }
        Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
            return Err(WriteFailure::Terminal(format!(
                "aggregate write refused HTTP {code}"
            )))
        }
        Err(error) => return Err(WriteFailure::Uncertain(error.to_string())),
    };
    let body = response
        .into_string()
        .map_err(|e| WriteFailure::Uncertain(e.to_string()))?;
    serde_json::from_str(&body).map_err(|e| WriteFailure::Uncertain(e.to_string()))
}

pub(crate) fn record(
    endpoint: &str,
    signature: &str,
    filesystem: &str,
    observed: &str,
    delta: i64,
    session: Option<&str>,
) -> Result<(), String> {
    let token = crate::quipu_auth::require_for_hook(endpoint, session)?;
    accumulate(
        signature,
        filesystem,
        observed,
        delta,
        || read(endpoint, signature, filesystem),
        |query| send(endpoint, &token, session, query),
    )
}

// Shared production loop: tests inject observations/receipts, not another copy
// of the aggregation or retry algorithm.
fn accumulate(
    signature: &str,
    filesystem: &str,
    observed: &str,
    delta: i64,
    mut read: impl FnMut() -> Result<Option<(String, State)>, String>,
    mut write: impl FnMut(&str) -> Result<serde_json::Value, WriteFailure>,
) -> Result<(), String> {
    let id = hex::encode(Sha256::digest(
        format!("{signature}\0{filesystem}\0{observed}").as_bytes(),
    ));
    let iri = subject(signature, filesystem);
    for _ in 0..3 {
        let previous = read()?;
        if let Some((_, state)) = &previous {
            if state.last_id == id {
                return if state.last_delta == delta {
                    Ok(())
                } else {
                    Err("retry delta differs".into())
                };
            }
        }
        let next = State::record(
            previous.as_ref().map(|(_, state)| state),
            &id,
            delta,
            observed,
        )?;
        let query = update(&iri, signature, filesystem, previous.as_ref(), &next)?;
        let result = match write(&query) {
            Ok(result) => result,
            Err(WriteFailure::Terminal(reason)) => return Err(reason),
            Err(WriteFailure::Uncertain(reason)) => {
                let current = read()?;
                if current
                    .is_some_and(|(_, state)| state.last_id == id && state.last_delta == delta)
                {
                    return Ok(());
                }
                return Err(format!("aggregate write not verified: {reason}"));
            }
        };
        match (result["asserted"].as_u64(), result["retracted"].as_u64()) {
            (Some(n), Some(_)) if n > 0 && result["tx"].as_i64().is_some_and(|tx| tx > 0) => {
                return Ok(())
            }
            (Some(0), Some(0)) if result.get("tx") == Some(&serde_json::Value::Null) => {}
            _ => return Err("invalid aggregate update receipt".into()),
        }
    }
    Err("aggregate update contention; sample not recorded".into())
}

#[cfg(test)]
#[path = "project_disk_aggregate_tests.rs"]
mod tests;
